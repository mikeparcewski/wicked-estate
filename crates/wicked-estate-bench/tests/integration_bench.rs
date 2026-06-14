//! Integration tests for the wicked-estate-bench capability benchmark.
//!
//! These tests create a minimal fixture repo in a temp directory (2-3 Rust source files),
//! run the benchmark logic against it, and assert that the produced metrics are sensible.
//! They do NOT depend on external repos (prior art, prior art, prior art).

use std::fs;
use std::path::PathBuf;
use wicked_estate_bench::run_benchmark;

/// Build a small fixture repo under a temp dir and return its path.
///
/// Layout:
///   <tmp>/src/lib.rs      — defines `add` and `multiply`
///   <tmp>/src/main.rs     — calls `add` and `multiply`, has `main`
///   <tmp>/src/utils.rs    — defines `double`
fn create_fixture_repo() -> PathBuf {
    // Guaranteed-unique path. `subsec_nanos()` alone collides across parallel test threads, and
    // since each test ends with `remove_dir_all(&fixture)`, a collision lets one test's cleanup
    // delete another's fixture mid-index → 0 nodes (a flake). A process-wide atomic counter makes
    // every call unique within the process; the pid disambiguates across processes.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "wicked_estate_bench_fixture_{}_{}",
        std::process::id(),
        uniq
    ));
    fs::create_dir_all(tmp.join("src")).expect("create fixture src dir");

    fs::write(
        tmp.join("src/lib.rs"),
        r#"/// Add two integers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Multiply two integers.
pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
"#,
    )
    .expect("write lib.rs");

    fs::write(
        tmp.join("src/main.rs"),
        r#"mod utils;
use crate::utils::double;

fn main() {
    let x = add(2, 3);
    let y = multiply(x, 4);
    let z = double(y);
    println!("{}", z);
}

fn add(a: i32, b: i32) -> i32 { a + b }
fn multiply(a: i32, b: i32) -> i32 { a * b }
"#,
    )
    .expect("write main.rs");

    fs::write(
        tmp.join("src/utils.rs"),
        r#"/// Double a value.
pub fn double(x: i32) -> i32 {
    x + x
}
"#,
    )
    .expect("write utils.rs");

    tmp
}

/// Build a LARGER fixture (`n_files` files × 8 functions each) so footprint ratios like
/// `bytes_per_node` are measured in the asymptotic regime. On a 3-file repo, SQLite's FIXED
/// overhead (schema + FTS5 shadow tables + WAL + minimum page allocation, ~140 KB) dominates and
/// `bytes_per_node` is meaningless (~14 KB/node). At several hundred nodes that fixed cost is
/// amortized and the ratio reflects real per-node storage. Unique path (atomic counter).
fn create_scaled_fixture_repo(n_files: usize) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "wicked_estate_bench_scaled_{}_{}",
        std::process::id(),
        uniq
    ));
    fs::create_dir_all(tmp.join("src")).expect("create scaled fixture src dir");
    for f in 0..n_files {
        let mut body = String::new();
        for i in 0..8 {
            // Each function calls the next in its file → real CALLS edges, a small cyclic graph.
            let next = (i + 1) % 8;
            body.push_str(&format!(
                "/// Function {f}_{i}.\npub fn f{f}_{i}(x: i32) -> i32 {{\n    let y = f{f}_{next}(x);\n    y + {i}\n}}\n\n"
            ));
        }
        fs::write(tmp.join(format!("src/m{f}.rs")), body).expect("write scaled fixture file");
    }
    tmp
}

#[test]
fn benchmark_produces_metrics_for_fixture_repo() {
    let fixture = create_fixture_repo();

    let report = run_benchmark(std::slice::from_ref(&fixture), false)
        .expect("benchmark must not fail on fixture repo");

    // Exactly one repo in the report.
    assert_eq!(report.repos.len(), 1, "expected one repo in report");

    let m = &report.repos[0];

    // Node count must be positive — the extractor parsed something.
    assert!(
        m.node_count > 0,
        "node_count must be > 0 for a non-empty repo, got {}",
        m.node_count
    );

    // File count must be > 0.
    assert!(
        m.file_count > 0,
        "file_count must be > 0, got {}",
        m.file_count
    );

    // Index time must have been recorded (even if zero-ish on fast hardware).
    // We don't assert it's > 0 because in-memory indexing can be <1ms.
    let _ = m.index_ms; // field exists and is serializable

    // nodes_by_kind must not be empty (we have at least functions).
    assert!(
        !m.nodes_by_kind.is_empty(),
        "nodes_by_kind must not be empty"
    );

    // Context pack must be non-empty if there are nodes.
    // (If no important symbols, context_pack_chars may legitimately be 0 — but
    //  for a repo with functions it must be non-empty.)
    if m.node_count > 0 && m.query_symbol != "(none)" {
        assert!(
            m.context_pack_chars > 0,
            "context_pack_chars must be > 0 when symbols exist"
        );
        assert!(
            m.context_pack_est_tokens > 0,
            "context_pack_est_tokens must be > 0 when symbols exist"
        );
        assert!(
            m.context_pack_symbol_count > 0,
            "context_pack_symbol_count must be > 0 when symbols exist"
        );
    }

    // W3.5: resolver_breakdown invariants — confidence in [0,1] and bands sum to edge_count.
    // Non-empty is implied when edge_count > 0 (each edge belongs to a resolver).
    if m.edge_count > 0 {
        assert!(
            !m.resolver_breakdown.is_empty(),
            "resolver_breakdown must be non-empty when edge_count > 0"
        );
    }
    for r in &m.resolver_breakdown {
        assert!(
            r.mean_confidence >= 0.0 && r.mean_confidence <= 1.0,
            "resolver '{}' mean_confidence ({}) must be in [0, 1]",
            r.resolver_id,
            r.mean_confidence,
        );
        // Band counts must sum to edge_count.
        let band_sum = r.bands.exact + r.bands.high + r.bands.medium + r.bands.low;
        assert_eq!(
            band_sum, r.edge_count,
            "resolver '{}' band counts ({}) must sum to edge_count ({})",
            r.resolver_id, band_sum, r.edge_count,
        );
    }

    // W8.3: language_matrix must have at least one language for a non-empty repo.
    if m.node_count > 0 {
        assert!(
            !m.language_matrix.is_empty(),
            "language_matrix must be non-empty when node_count > 0"
        );
    }
    // Every row must have at least one kind_present and a positive node_count.
    for row in &m.language_matrix {
        assert!(
            row.node_count > 0,
            "language_matrix row '{}' must have node_count > 0",
            row.language,
        );
        assert!(
            !row.kinds_present.is_empty(),
            "language_matrix row '{}' must have at least one kind_present",
            row.language,
        );
    }

    // Report struct is serde-round-trippable.
    let json = serde_json::to_string(&report).expect("report must serialize to JSON");
    let _: wicked_estate_bench::CapabilityReport =
        serde_json::from_str(&json).expect("report must deserialize from JSON");

    // Cleanup.
    let _ = fs::remove_dir_all(&fixture);
}

#[test]
fn benchmark_with_multiple_fixture_repos() {
    let fixture1 = create_fixture_repo();
    let fixture2 = create_fixture_repo();

    let report = run_benchmark(&[fixture1.clone(), fixture2.clone()], false)
        .expect("benchmark must handle multiple repos");

    assert_eq!(
        report.repos.len(),
        2,
        "expected two repos, got {}",
        report.repos.len()
    );

    for m in &report.repos {
        assert!(m.node_count > 0, "each repo must have nodes");
    }

    let _ = fs::remove_dir_all(&fixture1);
    let _ = fs::remove_dir_all(&fixture2);
}

#[test]
fn benchmark_empty_paths_returns_empty_report() {
    let report = run_benchmark(&[], false).expect("benchmark with no paths must not fail");
    assert!(
        report.repos.is_empty(),
        "empty input must produce empty report"
    );
}

/// Regression gate: footprint and throughput must stay within their ceilings.
///
/// **Ceilings are deliberately loose** — they catch accidental schema bloat and catastrophic
/// throughput regressions without becoming a CI flake on slow CI machines.  Tighten them as
/// optimisations land (e.g. once sqlite-vec compression or page-size tuning is in).
///
/// | Gate                | Ceiling        | Rationale                                           |
/// |---------------------|----------------|-----------------------------------------------------|
/// | `bytes_per_node`    | `< 12_000.0`   | ≈6.7 KB/node measured on prior art; 2× headroom   |
/// | `nodes_per_second`  | `> 20.0`        | Very conservative; real repos see 1000+/s           |
#[test]
fn footprint_and_speed_within_ceilings() {
    // Use a SCALED fixture (64 files × 8 fns ≈ 512 nodes) so SQLite's ~140 KB fixed overhead is
    // amortized — on the 3-file fixture bytes_per_node is ~14 KB (all fixed cost), not a regression.
    let fixture = create_scaled_fixture_repo(64);

    let report = run_benchmark(std::slice::from_ref(&fixture), false)
        .expect("benchmark must not fail on fixture repo");

    assert_eq!(report.repos.len(), 1, "expected one repo");
    let m = &report.repos[0];

    // Only assert ceilings when the repo actually produced nodes — an empty fixture would give
    // a trivially-passing 0 bytes/node, masking a broken extractor.
    assert!(
        m.node_count > 0,
        "fixture repo must produce nodes for footprint gate to be meaningful"
    );

    // Footprint gate: bytes per node must be below the ceiling.
    assert!(
        m.bytes_per_node < 12_000.0,
        "bytes_per_node ({:.0}) exceeds ceiling of 12_000. Schema bloat or WAL not flushed?",
        m.bytes_per_node
    );

    // db_bytes must be non-zero when nodes exist — guards against a silent open failure.
    assert!(
        m.db_bytes > 0,
        "db_bytes must be > 0 when nodes were indexed (on-disk store did not write?)"
    );

    // Throughput floor: nodes per second must be above the floor.
    // index_ms is u64; use saturating_add(1) to avoid division-by-zero on <1ms runs.
    let index_ms = m.index_ms.max(1); // treat sub-millisecond as 1ms to avoid inflated rate
    let nodes_per_second = m.node_count as f64 / (index_ms as f64 / 1000.0);
    assert!(
        nodes_per_second > 20.0,
        "nodes_per_second ({:.1}) is below floor of 20.0 — throughput regression?",
        nodes_per_second
    );

    // Receipt fields introduced by Task 2 must be present and consistent.
    assert_eq!(
        m.who_calls_count, m.blast_radius_node_count,
        "who_calls_count must equal blast_radius_node_count"
    );
    assert!(
        m.blast_radius_coverage_pct >= 0.0 && m.blast_radius_coverage_pct <= 100.0,
        "blast_radius_coverage_pct must be in [0, 100]"
    );
    // languages Vec must have at least one entry for a non-empty repo.
    assert!(
        !m.languages.is_empty(),
        "languages must be non-empty when nodes exist"
    );

    // W3.5: resolver_breakdown must be non-empty on the scaled fixture (which has edges).
    assert!(
        !m.resolver_breakdown.is_empty(),
        "resolver_breakdown must be non-empty for a repo with edges"
    );
    for r in &m.resolver_breakdown {
        assert!(
            r.mean_confidence >= 0.0 && r.mean_confidence <= 1.0,
            "mean_confidence {} out of [0,1] for resolver '{}'",
            r.mean_confidence,
            r.resolver_id,
        );
        let band_sum = r.bands.exact + r.bands.high + r.bands.medium + r.bands.low;
        assert_eq!(
            band_sum, r.edge_count,
            "band counts must sum to edge_count for resolver '{}'",
            r.resolver_id,
        );
    }

    // W8.3: language_matrix must be non-empty on the scaled fixture.
    assert!(
        !m.language_matrix.is_empty(),
        "language_matrix must be non-empty for a repo with nodes"
    );

    let _ = fs::remove_dir_all(&fixture);
}

/// Verify that the new receipt fields are serde-round-trippable.
#[test]
fn new_receipt_fields_serialize_cleanly() {
    let fixture = create_fixture_repo();

    let report =
        run_benchmark(std::slice::from_ref(&fixture), false).expect("benchmark must not fail");

    let json = serde_json::to_string(&report).expect("report must serialize");
    let back: wicked_estate_bench::CapabilityReport =
        serde_json::from_str(&json).expect("report must deserialize");

    let m = &report.repos[0];
    let mb = &back.repos[0];

    assert_eq!(m.db_bytes, mb.db_bytes, "db_bytes round-trips");
    assert!(
        (m.bytes_per_node - mb.bytes_per_node).abs() < 1e-9,
        "bytes_per_node round-trips"
    );
    assert_eq!(
        m.who_calls_count, mb.who_calls_count,
        "who_calls_count round-trips"
    );
    assert_eq!(m.languages, mb.languages, "languages round-trips");
    assert_eq!(
        m.edges_by_kind_vec, mb.edges_by_kind_vec,
        "edges_by_kind_vec round-trips"
    );

    // W3.5 / W8.3: new fields must also round-trip.
    assert_eq!(
        m.resolver_breakdown.len(),
        mb.resolver_breakdown.len(),
        "resolver_breakdown length round-trips"
    );
    for (a, b) in m
        .resolver_breakdown
        .iter()
        .zip(mb.resolver_breakdown.iter())
    {
        assert_eq!(a.resolver_id, b.resolver_id, "resolver_id round-trips");
        assert_eq!(a.edge_count, b.edge_count, "edge_count round-trips");
        assert!(
            (a.mean_confidence - b.mean_confidence).abs() < 1e-9,
            "mean_confidence round-trips"
        );
        assert_eq!(a.bands.exact, b.bands.exact, "bands.exact round-trips");
        assert_eq!(a.bands.high, b.bands.high, "bands.high round-trips");
        assert_eq!(a.bands.medium, b.bands.medium, "bands.medium round-trips");
        assert_eq!(a.bands.low, b.bands.low, "bands.low round-trips");
    }
    assert_eq!(
        m.language_matrix.len(),
        mb.language_matrix.len(),
        "language_matrix length round-trips"
    );
    for (a, b) in m.language_matrix.iter().zip(mb.language_matrix.iter()) {
        assert_eq!(a.language, b.language, "language round-trips");
        assert_eq!(a.node_count, b.node_count, "node_count round-trips");
        assert_eq!(
            a.kinds_present, b.kinds_present,
            "kinds_present round-trips"
        );
        assert_eq!(
            a.has_calls_edges, b.has_calls_edges,
            "has_calls_edges round-trips"
        );
    }

    let _ = fs::remove_dir_all(&fixture);
}
