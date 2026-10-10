//! Capability benchmark logic (W1.6 / W8.1 / W3.5 / W8.3).
//!
//! Exported from `wicked_estate_bench::capability` and re-exported at the crate root.
//! The binary (`src/main.rs`) calls [`run_benchmark`] and formats the output.
//!
//! **W3.5** — Resolution precision dashboard: per-resolver edge counts, mean confidence, and
//! confidence-band histograms, derived from [`wicked_estate_core::GraphRead::all_edges`].  Because we have
//! no ground-truth labels, this is a *proxy* for precision: a resolver that consistently
//! produces high-confidence edges (≥ 0.8) is likely more precise than one skewed toward < 0.5.
//! True precision requires labeled recall/precision data; this surface flags low-confidence-heavy
//! resolvers for manual review.
//!
//! **W8.3** — Language coverage matrix: per-language extraction richness derived from
//! [`wicked_estate_core::GraphRead::all_nodes`] + [`wicked_estate_core::GraphRead::all_edges`], written to
//! `docs/benchmarks/coverage-matrix.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use wicked_estate_store::SqliteStore;

// ---------------------------------------------------------------------------
// Public metric types
// ---------------------------------------------------------------------------

/// Confidence bands used by the W3.5 resolution-precision dashboard.
///
/// Bands are half-open `[lo, hi)` except the top which is `[0.8, 1.0]` (inclusive).
/// Matches the tiers' `default_confidence` values:
/// - Parsed/Scip/Lsp → 1.0,   Tsg → 0.8,  ImportMap → 0.6,  Heuristic → 0.5,  Tags → 0.3
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfidenceBands {
    /// Count of edges with confidence == 1.0 (Parsed / Scip / Lsp tier).
    pub exact: u64,
    /// Count of edges with confidence in [0.8, 1.0) (Tsg tier).
    pub high: u64,
    /// Count of edges with confidence in [0.5, 0.8) (ImportMap / Heuristic tiers).
    pub medium: u64,
    /// Count of edges with confidence in [0.0, 0.5) (Tags tier or lower).
    pub low: u64,
}

/// Per-resolver precision summary (W3.5).
///
/// True precision requires labeled data; this reports the confidence distribution as a proxy.
/// A resolver skewed toward the `low` band should be flagged for manual review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolverStats {
    /// The `resolved_by` string recorded on the edge (e.g. `"import-map-py"`, `"parsed"`).
    pub resolver_id: String,
    /// Total edges emitted by this resolver in this repo.
    pub edge_count: u64,
    /// Mean confidence across all edges for this resolver (`[0.0, 1.0]`).
    pub mean_confidence: f64,
    /// Confidence-band histogram.
    pub bands: ConfidenceBands,
}

/// Per-language extraction quality row for the W8.3 coverage matrix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LangMatrixRow {
    /// Tree-sitter grammar name (the `Language.0` string).
    pub language: String,
    /// Total nodes extracted for this language.
    pub node_count: u64,
    /// Distinct `NodeKind` strings present (sorted for stability).
    pub kinds_present: Vec<String>,
    /// True when at least one `EdgeKind::Calls` edge has a source node in this language.
    pub has_calls_edges: bool,
}

/// Per-repo capability metrics captured by the benchmark.
///
/// New fields added for regression gating (Task 1) and capability receipts (Task 2):
/// - `db_bytes` / `bytes_per_node` — footprint regression gate; measured on-disk via WAL store.
/// - `direct_ref_resolution_pct` — matched-population direct resolution for the top symbol
///   (see the field); `None` when the unresolved count could not be read.
/// - `who_calls_count` — number of dependents of the top symbol (depth-3 blast-radius).
/// - `languages` — node counts by language, sorted by count descending.
/// - `edges_by_kind_vec` — edge counts as an ordered `Vec` for stable report rendering.
///
/// W3.5 fields:
/// - `resolver_breakdown` — per-resolver edge count, mean confidence, and confidence bands.
///
/// W8.3 fields:
/// - `language_matrix` — per-language extraction richness (node count, node kinds, calls edges).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoMetrics {
    /// Human-readable repo name (last path component).
    pub repo: String,
    /// Absolute path indexed.
    pub path: String,
    /// Wall-clock time to index the whole repo (ms).
    pub index_ms: u64,
    /// Source files parsed (NodeKind::File count).
    pub file_count: u64,
    /// Total graph nodes.
    pub node_count: u64,
    /// Total graph edges.
    pub edge_count: u64,
    /// Cross-file references that could not be resolved (coverage signal), one per
    /// unresolved reference — `docs/ENGINE-CONTRACT.md` §2.1.
    pub unresolved_ref_count: u64,
    /// Node counts keyed by JSON-serialised `NodeKind`.
    pub nodes_by_kind: BTreeMap<String, u64>,
    /// Edge counts keyed by JSON-serialised `EdgeKind`.
    pub edges_by_kind: BTreeMap<String, u64>,
    /// `wicked_estate::search` latency on the top symbol (µs).
    pub search_latency_us: u64,
    /// `wicked_estate::blast_radius_by_name` latency on the top symbol at depth 3 (µs).
    pub blast_radius_latency_us: u64,
    /// Dependent-node count returned by the blast-radius query.
    pub blast_radius_node_count: usize,
    /// The symbol name used for the query probes.
    pub query_symbol: String,
    /// Total characters in the context-pack (top-15 symbol stubs).
    pub context_pack_chars: usize,
    /// Estimated token count (chars / 4).
    pub context_pack_est_tokens: usize,
    /// Number of symbols included in the context pack.
    pub context_pack_symbol_count: usize,

    // -----------------------------------------------------------------------
    // Task 1 — regression gate: footprint + speed
    // -----------------------------------------------------------------------
    /// On-disk database size in bytes (`<db>` + `<db>-wal` + `<db>-shm` when present).
    ///
    /// Measured by indexing into a temporary on-disk `SqliteStore`, summing the file sizes,
    /// then cleaning up the temp files.  In-memory store is used for all other metrics so
    /// cross-run contamination is impossible.
    pub db_bytes: u64,

    /// `db_bytes / node_count` — space per node.
    ///
    /// Ceiling: `< 12_000.0` bytes/node (≈6.7 KB/node measured on prior art; 2× headroom to
    /// avoid flakes while still catching accidental schema bloat).
    pub bytes_per_node: f64,

    // -----------------------------------------------------------------------
    // Task 2 — must-have-value receipts
    // -----------------------------------------------------------------------
    /// Direct reference resolution for the top symbol (BENCH-01), over ONE population: distinct
    /// `(dependent, edge kind)` pairs that name it directly. `direct_refs_resolved` counts the
    /// resolved ones (direct incoming edges a name-binding resolver emitted into any node of that name);
    /// `direct_refs_unresolved` the unresolved ones (distinct `(from, kind)` among
    /// `unresolved_refs_for_name`). Transitive blast-radius dependents are NOT mixed in, so a
    /// longer downstream chain cannot raise it. A resolver-coverage diagnostic, not recall
    /// against labelled truth.
    /// `None` when the resolved side could not be read.
    pub direct_refs_resolved: Option<usize>,
    /// `None` when the unresolved query failed: unavailable, never assumed zero.
    pub direct_refs_unresolved: Option<usize>,
    /// `resolved / (resolved + unresolved)` in percent; `None` when EITHER side is unavailable
    /// or there are no direct references at all.
    pub direct_ref_resolution_pct: Option<f64>,

    /// Number of *resolved* dependents of the top symbol (alias of `blast_radius_node_count`
    /// kept as a named receipt field for the report table).
    pub who_calls_count: usize,

    /// Node counts by language, sorted by count descending.  Each entry is `(language, count)`.
    pub languages: Vec<(String, u64)>,

    /// Edge counts by kind as an ordered `Vec<(kind, count)>` sorted by count descending.
    /// Mirrors `edges_by_kind` but stable for report tables.
    pub edges_by_kind_vec: Vec<(String, u64)>,

    // -----------------------------------------------------------------------
    // W3.5 — Resolution precision dashboard
    // -----------------------------------------------------------------------
    /// Per-resolver edge statistics (count, mean confidence, confidence-band histogram).
    ///
    /// Derived from `all_edges()` grouped by `resolved_by`.  Sorted by edge count descending.
    ///
    /// **Precision caveat:** confidence is a *proxy*, not ground-truth precision.  True precision
    /// (fraction of edges that are correct) requires labeled data.  Use this to spot resolvers
    /// that are suspiciously low-confidence-heavy and schedule a manual review pass.
    pub resolver_breakdown: Vec<ResolverStats>,

    // -----------------------------------------------------------------------
    // W8.3 — Language coverage matrix
    // -----------------------------------------------------------------------
    /// Per-language extraction richness: node count, distinct node kinds, and whether
    /// any `Calls` edges were produced for nodes in that language.
    ///
    /// Sorted by node_count descending then language name ascending.
    pub language_matrix: Vec<LangMatrixRow>,
}

/// Aggregate run report — serialisable to JSON and renderable as Markdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    /// ISO-8601 UTC timestamp of when this run was generated.
    pub generated_at: String,
    /// One entry per repo that was successfully indexed.
    pub repos: Vec<RepoMetrics>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn repo_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn elapsed_us(d: Duration) -> u64 {
    d.as_micros() as u64
}

fn elapsed_ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// Render one symbol as a compact stub line for the context pack.
fn symbol_stub(node: &wicked_estate_core::Node, score: f32) -> String {
    let kind = serde_json::to_string(&node.kind).unwrap_or_default();
    let kind = kind.trim_matches('"');
    let file = &node.location.file;
    let line = node.location.span.start_line;
    let sig = node.signature.as_deref().unwrap_or("(none)");
    format!(
        "{kind} {name}  [{file}:{line}]  sig: {sig}  score: {score:.6}",
        kind = kind,
        name = node.name,
        file = file,
        line = line,
        sig = sig,
        score = score,
    )
}

/// Sum the on-disk size of `path` and its WAL/SHM sidecar files.
fn disk_size_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    for suffix in &["", "-wal", "-shm"] {
        let p = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            let mut s = path.as_os_str().to_owned();
            s.push(suffix);
            PathBuf::from(s)
        };
        if let Ok(meta) = fs::metadata(&p) {
            total += meta.len();
        }
    }
    total
}

/// Remove the on-disk SQLite file and its sidecar files (`-wal`, `-shm`).
fn remove_db_files(path: &Path) {
    for suffix in &["", "-wal", "-shm"] {
        let p = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            let mut s = path.as_os_str().to_owned();
            s.push(suffix);
            PathBuf::from(s)
        };
        let _ = fs::remove_file(p);
    }
}

// ---------------------------------------------------------------------------
// Per-repo benchmark
// ---------------------------------------------------------------------------

fn benchmark_repo(repo_path: &Path) -> Result<RepoMetrics> {
    use wicked_estate_core::GraphRead;

    let name = repo_name(repo_path);
    eprintln!("  indexing {} ...", repo_path.display());

    // -----------------------------------------------------------------------
    // Primary metrics: fresh in-memory store — no cross-run contamination.
    // -----------------------------------------------------------------------
    let mut store = SqliteStore::in_memory().context("open in-memory store")?;

    let t0 = Instant::now();
    let stats = wicked_estate::index_path(&mut store, repo_path).context("index_path")?;
    let index_ms = elapsed_ms(t0.elapsed());

    eprintln!(
        "    {}ms  nodes={} edges={} files={}",
        index_ms, stats.node_count, stats.edge_count, stats.file_count
    );

    // Top-15 symbols by global PageRank for the context pack and query probes.
    let top_symbols = wicked_estate::important_symbols(&store, 15).context("important_symbols")?;

    let (query_symbol, context_pack_chars, context_pack_est_tokens, context_pack_symbol_count) =
        if top_symbols.is_empty() {
            ("(none)".to_string(), 0usize, 0usize, 0usize)
        } else {
            let pack: String = top_symbols
                .iter()
                .map(|(node, score)| symbol_stub(node, *score))
                .collect::<Vec<_>>()
                .join("\n");
            let chars = pack.len();
            let tokens = chars / 4;
            let sym_name = top_symbols[0].0.name.clone();
            (sym_name, chars, tokens, top_symbols.len())
        };

    // Query probes on the top symbol.
    let (search_latency_us, blast_radius_latency_us, blast_radius_node_count) =
        if query_symbol == "(none)" {
            (0u64, 0u64, 0usize)
        } else {
            let t1 = Instant::now();
            let _hits = wicked_estate::search(&store, &query_symbol).context("search")?;
            let search_us = elapsed_us(t1.elapsed());

            let t2 = Instant::now();
            let br = wicked_estate::blast_radius_by_name(&store, &query_symbol, 3)
                .context("blast_radius")?;
            let br_us = elapsed_us(t2.elapsed());

            (search_us, br_us, br.dependents.len())
        };

    // -----------------------------------------------------------------------
    // Task 2: direct reference resolution (BENCH-01: matched populations, honest failure).
    // -----------------------------------------------------------------------
    let direct = if query_symbol == "(none)" {
        DirectRefResolution::default()
    } else {
        direct_ref_resolution(&store, &query_symbol)
    };

    // -----------------------------------------------------------------------
    // Task 2: node counts by language, sorted by count descending.
    // -----------------------------------------------------------------------
    let languages: Vec<(String, u64)> = {
        let all_nodes = store.all_nodes().unwrap_or_default();
        let mut lang_map: BTreeMap<String, u64> = BTreeMap::new();
        for node in &all_nodes {
            *lang_map.entry(node.language.0.clone()).or_insert(0) += 1;
        }
        let mut v: Vec<(String, u64)> = lang_map.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    };

    // -----------------------------------------------------------------------
    // Task 2: edges_by_kind as a stable Vec sorted by count descending.
    // -----------------------------------------------------------------------
    let edges_by_kind_vec: Vec<(String, u64)> = {
        let mut v: Vec<(String, u64)> = stats
            .edges_by_kind
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    };

    // -----------------------------------------------------------------------
    // W3.5 + W8.3: fetch all edges once — shared by both dashboard passes.
    // -----------------------------------------------------------------------
    let all_edges_w35_w83 = store.all_edges().unwrap_or_default();

    // -----------------------------------------------------------------------
    // W3.5: resolver breakdown — group all_edges() by resolved_by.
    // -----------------------------------------------------------------------
    let resolver_breakdown: Vec<ResolverStats> = {
        // Accumulate per-resolver: (count, confidence_sum, bands).
        let mut map: BTreeMap<String, (u64, f64, ConfidenceBands)> = BTreeMap::new();
        for edge in &all_edges_w35_w83 {
            let c = edge.confidence.get() as f64;
            let entry = map.entry(edge.resolved_by.clone()).or_default();
            entry.0 += 1;
            entry.1 += c;
            let bands = &mut entry.2;
            if (c - 1.0_f64).abs() < 1e-9 {
                bands.exact += 1;
            } else if c >= 0.8 {
                bands.high += 1;
            } else if c >= 0.5 {
                bands.medium += 1;
            } else {
                bands.low += 1;
            }
        }

        let mut v: Vec<ResolverStats> = map
            .into_iter()
            .map(|(resolver_id, (edge_count, conf_sum, bands))| {
                let mean_confidence = if edge_count == 0 {
                    0.0
                } else {
                    conf_sum / edge_count as f64
                };
                ResolverStats {
                    resolver_id,
                    edge_count,
                    mean_confidence,
                    bands,
                }
            })
            .collect();
        // Sort by edge count descending, then resolver_id ascending for stability.
        v.sort_by(|a, b| {
            b.edge_count
                .cmp(&a.edge_count)
                .then_with(|| a.resolver_id.cmp(&b.resolver_id))
        });
        v
    };

    // -----------------------------------------------------------------------
    // W8.3: language coverage matrix — per-language node kinds + calls presence.
    // -----------------------------------------------------------------------
    let language_matrix: Vec<LangMatrixRow> = {
        let all_nodes = store.all_nodes().unwrap_or_default();

        // Map symbol_id → language for edges lookup.
        let mut sym_to_lang: BTreeMap<String, String> = BTreeMap::new();
        // Map language → (node_count, set of kind strings).
        let mut lang_info: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();

        for node in &all_nodes {
            sym_to_lang.insert(node.symbol.0.clone(), node.language.0.clone());
            let entry = lang_info.entry(node.language.0.clone()).or_default();
            entry.0 += 1;
            let kind_str = serde_json::to_string(&node.kind)
                .unwrap_or_default()
                .trim_matches('"')
                .to_owned();
            entry.1.insert(kind_str);
        }

        // Determine which languages have Calls edges by looking at edge sources.
        let mut langs_with_calls: BTreeSet<String> = BTreeSet::new();
        for edge in &all_edges_w35_w83 {
            if matches!(edge.kind, wicked_estate_core::EdgeKind::Calls) {
                if let Some(lang) = sym_to_lang.get(&edge.source.0) {
                    langs_with_calls.insert(lang.clone());
                }
            }
        }

        let mut v: Vec<LangMatrixRow> = lang_info
            .into_iter()
            .map(|(language, (node_count, kinds_set))| {
                let has_calls_edges = langs_with_calls.contains(&language);
                let kinds_present = kinds_set.into_iter().collect();
                LangMatrixRow {
                    language,
                    node_count,
                    kinds_present,
                    has_calls_edges,
                }
            })
            .collect();
        // Sort by node_count descending, then language name ascending.
        v.sort_by(|a, b| {
            b.node_count
                .cmp(&a.node_count)
                .then_with(|| a.language.cmp(&b.language))
        });
        v
    };

    // -----------------------------------------------------------------------
    // Task 1: footprint — index on-disk and measure file sizes, then clean up.
    //
    // Uses a process-unique temp path (same anti-collision pattern as the tests).
    // -----------------------------------------------------------------------
    static FOOTPRINT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let db_path = {
        let n = FOOTPRINT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "wicked_estate_bench_disk_{}_{}_{}.db",
            std::process::id(),
            n,
            repo_name(repo_path).replace('/', "_")
        ))
    };

    let (db_bytes, bytes_per_node) = {
        let result = (|| -> Result<(u64, f64)> {
            let mut disk_store = SqliteStore::open(&db_path).context("open on-disk store")?;
            wicked_estate::index_path(&mut disk_store, repo_path)
                .context("index_path for footprint")?;
            // Drop the store to flush WAL before measuring.
            drop(disk_store);
            let bytes = disk_size_bytes(&db_path);
            let bpn = if stats.node_count == 0 {
                0.0f64
            } else {
                bytes as f64 / stats.node_count as f64
            };
            Ok((bytes, bpn))
        })();
        remove_db_files(&db_path);
        result.unwrap_or((0, 0.0))
    };

    Ok(RepoMetrics {
        repo: name,
        path: repo_path.to_string_lossy().into_owned(),
        index_ms,
        file_count: stats.file_count,
        node_count: stats.node_count,
        edge_count: stats.edge_count,
        unresolved_ref_count: stats.unresolved_ref_count,
        nodes_by_kind: stats.nodes_by_kind,
        edges_by_kind: stats.edges_by_kind,
        search_latency_us,
        blast_radius_latency_us,
        blast_radius_node_count,
        query_symbol,
        context_pack_chars,
        context_pack_est_tokens,
        context_pack_symbol_count,
        db_bytes,
        bytes_per_node,
        direct_refs_resolved: direct.resolved,
        direct_refs_unresolved: direct.unresolved,
        direct_ref_resolution_pct: direct.pct,
        who_calls_count: blast_radius_node_count,
        languages,
        edges_by_kind_vec,
        resolver_breakdown,
        language_matrix,
    })
}

// ---------------------------------------------------------------------------
// Report formatting
// ---------------------------------------------------------------------------

/// BENCH-01: direct reference resolution for `name` over one population — distinct
/// `(dependent, edge kind)` pairs naming it directly, resolved vs unresolved. Any failed read makes
/// the side it feeds `None`, and a `None` side makes the percentage `None`: a measurement that
/// could not be taken is never reported as a number.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirectRefResolution {
    pub resolved: Option<usize>,
    pub unresolved: Option<usize>,
    pub pct: Option<f64>,
}

/// The resolvers that bind an `UnresolvedRef` BY THE TARGET'S NAME (`raw_name` == the target's
/// name) — the only resolvers whose edges belong to the name-keyed population the unresolved side
/// counts (`unresolved_refs_for_name`). A relationship an extractor emits locally (`Governs`,
/// `Contains`, a parsed flow) never was a reference, and two production resolvers bind by
/// something else: `relative-import` by a quoted path specifier, `rules-bridge-resolver` by a
/// `rules-engine:<scheme>` marker that fans out to every RuleSet. Counting any of those would
/// report resolution for references that never named the symbol (BENCH-01 review).
fn reference_resolver_ids() -> Vec<String> {
    use wicked_estate_core::Resolver;
    use wicked_estate_resolve::{
        ImportMapResolver, InfraResolver, NameResolver, ScopedNameResolver,
    };
    vec![
        NameResolver.id().to_string(),
        ScopedNameResolver.id().to_string(),
        ImportMapResolver.id().to_string(),
        InfraResolver.id().to_string(),
    ]
}

/// The percentage, only when both sides are known and the total is non-zero.
pub fn resolution_pct(resolved: Option<usize>, unresolved: Option<usize>) -> Option<f64> {
    let (resolved, unresolved) = (resolved?, unresolved?);
    let total = resolved + unresolved;
    (total > 0).then(|| 100.0 * resolved as f64 / total as f64)
}

pub fn direct_ref_resolution(
    store: &dyn wicked_estate_core::GraphRead,
    name: &str,
) -> DirectRefResolution {
    use wicked_estate_core::{Direction, SymbolQuery};
    let resolvers = reference_resolver_ids();
    let resolved = (|| -> wicked_estate_core::Result<usize> {
        let targets = store.find_symbols(&SymbolQuery {
            exact_name: Some(name.to_string()),
            ..Default::default()
        })?;
        let mut pairs = BTreeSet::new();
        for t in &targets {
            for e in store.neighbors(&t.symbol, Direction::Dependents)? {
                if e.target == t.symbol && resolvers.contains(&e.resolved_by) {
                    pairs.insert((e.source.0.clone(), format!("{:?}", e.kind)));
                }
            }
        }
        Ok(pairs.len())
    })()
    .ok();
    let unresolved = store.unresolved_refs_for_name(name).ok().map(|refs| {
        refs.iter()
            .map(|r| (r.from.0.clone(), format!("{:?}", r.kind)))
            .collect::<BTreeSet<_>>()
            .len()
    });
    DirectRefResolution {
        resolved,
        unresolved,
        pct: resolution_pct(resolved, unresolved),
    }
}

/// Resolved-edge share: every stored edge (all kinds) against every unresolved reference. A
/// graph-wide DIAGNOSTIC of how much the resolvers parked, not coverage or recall — the two
/// populations differ (Contains/Imports edges are not unresolved calls), see BENCH-01.
fn coverage_pct(m: &RepoMetrics) -> f64 {
    if m.node_count == 0 {
        return 0.0;
    }
    let total_ref = m.unresolved_ref_count + m.edge_count;
    if total_ref == 0 {
        return 100.0;
    }
    100.0 * (1.0 - m.unresolved_ref_count as f64 / total_ref as f64)
}

/// `"12.3%"`, or `"n/a"` when the measurement is unavailable (never a fabricated number).
fn pct_or_na(p: Option<f64>) -> String {
    p.map_or_else(|| "n/a".to_string(), |v| format!("{v:.1}%"))
}

/// Print the human-readable summary table to stdout.
pub fn print_summary_table(metrics: &[RepoMetrics]) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║     wicked_estate capability benchmark  (W1.6 / W3.5 / W8.1 / W8.3)  ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();

    for m in metrics {
        println!(
            "┌─ {} ─────────────────────────────────────────────",
            m.repo
        );
        println!("│  path            : {}", m.path);
        println!("│  index time      : {}ms", m.index_ms);
        println!(
            "│  files           : {}  nodes: {}  edges: {}",
            m.file_count, m.node_count, m.edge_count
        );
        println!(
            "│  unresolved refs : {}  (edge coverage: {:.1}%)",
            m.unresolved_ref_count,
            coverage_pct(m)
        );
        println!(
            "│  footprint       : {} bytes  ({:.0} bytes/node)",
            m.db_bytes, m.bytes_per_node
        );
        println!("│  nodes by kind   :");
        for (k, v) in &m.nodes_by_kind {
            println!("│      {:30}: {}", k, v);
        }
        println!("│  languages       :");
        for (lang, cnt) in &m.languages {
            println!("│      {:30}: {}", lang, cnt);
        }
        println!("│  edges by kind   :");
        for (k, v) in &m.edges_by_kind {
            println!("│      {:30}: {}", k, v);
        }
        println!("│  query symbol    : {}", m.query_symbol);
        if m.query_symbol != "(none)" {
            println!("│  search latency  : {}µs", m.search_latency_us);
            println!(
                "│  blast-radius    : {}µs  ({} dependents, depth 3)",
                m.blast_radius_latency_us, m.blast_radius_node_count
            );
            println!(
                "│  direct refs     : {} resolved / {} unresolved  ({})  ({} who-calls)",
                m.direct_refs_resolved
                    .map_or_else(|| "unavailable".to_string(), |r| r.to_string()),
                m.direct_refs_unresolved
                    .map_or_else(|| "unavailable".to_string(), |u| u.to_string()),
                pct_or_na(m.direct_ref_resolution_pct),
                m.who_calls_count
            );
            println!(
                "│  context pack    : {} chars  ~{} tokens  ({} symbols)",
                m.context_pack_chars, m.context_pack_est_tokens, m.context_pack_symbol_count
            );
        }
        // W3.5 — resolution precision by resolver.
        if !m.resolver_breakdown.is_empty() {
            println!("│  resolver breakdown (W3.5):");
            for r in &m.resolver_breakdown {
                println!(
                    "│      {:35}: {} edges  mean_conf={:.3}  exact={} high={} med={} low={}",
                    r.resolver_id,
                    r.edge_count,
                    r.mean_confidence,
                    r.bands.exact,
                    r.bands.high,
                    r.bands.medium,
                    r.bands.low,
                );
            }
        }
        // W8.3 — language coverage matrix.
        if !m.language_matrix.is_empty() {
            println!("│  language matrix (W8.3):");
            for row in &m.language_matrix {
                println!(
                    "│      {:20} nodes={:5}  calls={}  kinds: {}",
                    row.language,
                    row.node_count,
                    if row.has_calls_edges { "yes" } else { "no " },
                    row.kinds_present.join(", "),
                );
            }
        }
        println!("└──────────────────────────────────────────────────────────────────");
        println!();
    }
}

/// Write the Markdown capability report to `report_path`.
pub fn write_markdown_report(metrics: &[RepoMetrics], report_path: &Path) -> Result<()> {
    let mut f = fs::File::create(report_path)
        .with_context(|| format!("create {}", report_path.display()))?;

    writeln!(f, "# wicked_estate capability benchmark")?;
    writeln!(f)?;
    writeln!(
        f,
        "> **Waves W1.6 / W8.1** — engine capability receipt.  \\"
    )?;
    writeln!(
        f,
        "> The full agent A/B (baseline vs treatment with an LLM in the loop) is future work."
    )?;
    writeln!(
        f,
        "> This report measures what the engine itself delivers: index speed, graph completeness,"
    )?;
    writeln!(
        f,
        "> query latency, context-pack compactness, on-disk footprint, and blast-radius coverage."
    )?;
    writeln!(f)?;
    writeln!(f, "## Methodology")?;
    writeln!(f)?;
    writeln!(
        f,
        "For each repo: index into a fresh in-memory `SqliteStore` via `wicked_estate::index_path`,"
    )?;
    writeln!(
        f,
        "then run `search` and `blast_radius_by_name` on the top-ranked symbol from"
    )?;
    writeln!(
        f,
        "`wicked_estate::important_symbols` (global PageRank over CALLS/IMPORTS edges).  Context-pack"
    )?;
    writeln!(
        f,
        "size is measured by rendering the top-15 symbol stubs (signature + file:line + score)."
    )?;
    writeln!(
        f,
        "Tokens are estimated as `chars / 4` (rough GPT tokenization proxy)."
    )?;
    writeln!(f)?;
    writeln!(
        f,
        "**Footprint:** a second index run writes to a temp on-disk `SqliteStore` (WAL mode)."
    )?;
    writeln!(
        f,
        "The `.db` + `.db-wal` + `.db-shm` files are summed and the store is deleted on exit."
    )?;
    writeln!(f)?;
    writeln!(
        f,
        "**Direct reference resolution:** distinct direct `(dependent, relation)` pairs naming the top"
    )?;
    writeln!(
        f,
        "symbol, resolved (an edge a name-binding resolver emitted) vs unresolved (`unresolved_refs_for_name`,"
    )?;
    writeln!(
        f,
        "`docs/ENGINE-CONTRACT.md` §2.1). `n/a` when either side could not be read or there are no direct"
    )?;
    writeln!(
        f,
        "references. A resolver-coverage diagnostic, not recall against labelled truth."
    )?;
    writeln!(f)?;
    writeln!(f, "## Results")?;
    writeln!(f)?;

    // Main summary table.
    writeln!(
        f,
        "| Repo | Index (ms) | Files | Nodes | Edges | Unresolved | Footprint (bytes) | bytes/node | Search (µs) | Blast-radius (µs) | Direct ref resolution | Who-calls | Context chars | Est. tokens (bytes/4) |"
    )?;
    writeln!(
        f,
        "|------|-----------|-------|-------|-------|-----------|------------------|-----------|------------|------------------|-------------|----------|--------------|------------|"
    )?;
    for m in metrics {
        let search_cell = if m.query_symbol == "(none)" {
            "—".to_string()
        } else {
            m.search_latency_us.to_string()
        };
        let br_cell = if m.query_symbol == "(none)" {
            "—".to_string()
        } else {
            m.blast_radius_latency_us.to_string()
        };
        let br_cov_cell = if m.query_symbol == "(none)" {
            "—".to_string()
        } else {
            pct_or_na(m.direct_ref_resolution_pct)
        };
        let who_calls_cell = if m.query_symbol == "(none)" {
            "—".to_string()
        } else {
            m.who_calls_count.to_string()
        };
        writeln!(
            f,
            "| {} | {} | {} | {} | {} | {} | {} | {:.0} | {} | {} | {} | {} | {} | {} |",
            m.repo,
            m.index_ms,
            m.file_count,
            m.node_count,
            m.edge_count,
            m.unresolved_ref_count,
            m.db_bytes,
            m.bytes_per_node,
            search_cell,
            br_cell,
            br_cov_cell,
            who_calls_cell,
            m.context_pack_chars,
            m.context_pack_est_tokens,
        )?;
    }
    writeln!(f)?;

    // Per-repo receipts tables.
    writeln!(f, "## Per-repo receipts")?;
    writeln!(f)?;
    for m in metrics {
        writeln!(f, "### {}", m.repo)?;
        writeln!(f)?;
        writeln!(f, "**Path:** `{}`  ", m.path)?;
        writeln!(f, "**Index time:** {}ms  ", m.index_ms)?;
        writeln!(
            f,
            "**Resolved-edge share (diagnostic, all edge kinds vs unresolved refs):** {:.1}%  ",
            coverage_pct(m)
        )?;
        writeln!(
            f,
            "**Footprint:** {} bytes  ({:.0} bytes/node)  ",
            m.db_bytes, m.bytes_per_node
        )?;
        writeln!(f)?;

        if !m.languages.is_empty() {
            writeln!(f, "**Nodes by language:**")?;
            writeln!(f)?;
            writeln!(f, "| Language | Nodes |")?;
            writeln!(f, "|----------|-------|")?;
            for (lang, cnt) in &m.languages {
                writeln!(f, "| `{}` | {} |", lang, cnt)?;
            }
            writeln!(f)?;
        }

        if !m.nodes_by_kind.is_empty() {
            writeln!(f, "**Nodes by kind:**")?;
            writeln!(f)?;
            for (k, v) in &m.nodes_by_kind {
                writeln!(f, "- `{}`: {}", k, v)?;
            }
            writeln!(f)?;
        }

        if !m.edges_by_kind_vec.is_empty() {
            writeln!(f, "**Edges by kind:**")?;
            writeln!(f)?;
            writeln!(f, "| Edge kind | Count |")?;
            writeln!(f, "|-----------|-------|")?;
            for (k, v) in &m.edges_by_kind_vec {
                writeln!(f, "| `{}` | {} |", k, v)?;
            }
            writeln!(f)?;
        }

        if m.query_symbol != "(none)" {
            writeln!(
                f,
                "**Capability receipts for top symbol `{}`:**",
                m.query_symbol
            )?;
            writeln!(f)?;
            writeln!(f, "| Metric | Value | What it proves |")?;
            writeln!(f, "|--------|-------|----------------|")?;
            writeln!(
                f,
                "| who-calls count | {} | Precise blast-radius: these nodes depend on `{}` |",
                m.who_calls_count, m.query_symbol
            )?;
            writeln!(
                f,
                "| direct ref resolution | {} ({} resolved / {} unresolved) | Distinct direct (dependent, relation) pairs naming the symbol that the resolvers bound; a diagnostic, not recall |",
                pct_or_na(m.direct_ref_resolution_pct),
                m.direct_refs_resolved
                    .map_or_else(|| "unavailable".to_string(), |r| r.to_string()),
                m.direct_refs_unresolved
                    .map_or_else(|| "unavailable".to_string(), |u| u.to_string())
            )?;
            writeln!(
                f,
                "| context-pack chars | {} | Agent receives {} chars of scoped context |",
                m.context_pack_chars, m.context_pack_chars
            )?;
            writeln!(
                f,
                "| context-pack est. tokens | ~{} | Estimated LLM token cost for one context retrieval |",
                m.context_pack_est_tokens
            )?;
            writeln!(
                f,
                "| context-pack symbols | {} | Symbols ranked into the pack |",
                m.context_pack_symbol_count
            )?;
            writeln!(
                f,
                "| search latency | {}µs | Time to locate symbol by name |",
                m.search_latency_us
            )?;
            writeln!(
                f,
                "| blast-radius latency | {}µs | Time for depth-3 dependent traversal |",
                m.blast_radius_latency_us
            )?;
            writeln!(f)?;
        }

        // W3.5 — Resolution precision (by tier/resolver).
        if !m.resolver_breakdown.is_empty() {
            writeln!(f, "**Resolution precision (by tier) — W3.5:**")?;
            writeln!(f)?;
            writeln!(
                f,
                "> **Precision caveat:** confidence is a *proxy*, not ground-truth precision."
            )?;
            writeln!(
                f,
                "> True precision (fraction of edges that are correct) requires labeled data."
            )?;
            writeln!(
                f,
                "> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review."
            )?;
            writeln!(f)?;
            writeln!(
                f,
                "| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |"
            )?;
            writeln!(
                f,
                "|----------|-------|-----------|-------------|----------------|-----------------|--------------|"
            )?;
            for r in &m.resolver_breakdown {
                writeln!(
                    f,
                    "| `{}` | {} | {:.3} | {} | {} | {} | {} |",
                    r.resolver_id,
                    r.edge_count,
                    r.mean_confidence,
                    r.bands.exact,
                    r.bands.high,
                    r.bands.medium,
                    r.bands.low,
                )?;
            }
            writeln!(f)?;
        }
    }

    writeln!(f, "## Regression ceilings")?;
    writeln!(f)?;
    writeln!(
        f,
        "The `footprint_and_speed_within_ceilings` test in `wicked-estate-bench/tests/integration_bench.rs`"
    )?;
    writeln!(
        f,
        "asserts these ceilings on the fixture repo.  Tighten them as optimisations land."
    )?;
    writeln!(f)?;
    writeln!(f, "| Gate | Ceiling | Rationale |")?;
    writeln!(f, "|------|---------|-----------|")?;
    writeln!(
        f,
        "| `bytes_per_node` | `< 12_000.0` | ≈6.7 KB/node on prior art; 2× headroom |"
    )?;
    writeln!(
        f,
        "| `nodes_per_second` | `> 20.0` | Very conservative; real runs see 1000+/s |"
    )?;
    writeln!(f)?;

    writeln!(f, "## How to run")?;
    writeln!(f)?;
    writeln!(f, "```bash")?;
    writeln!(
        f,
        "# Default repos (workspace root + any that exist on disk):"
    )?;
    writeln!(
        f,
        "cargo run -p wicked-estate-bench --bin wicked-estate-bench"
    )?;
    writeln!(f)?;
    writeln!(f, "# Explicit paths:")?;
    writeln!(
        f,
        "cargo run -p wicked-estate-bench --bin wicked-estate-bench -- /path/to/repo1 /path/to/repo2"
    )?;
    writeln!(f, "```")?;
    writeln!(f)?;
    writeln!(
        f,
        "*Last generated: {}*",
        metrics
            .first()
            .map(|_| "see generated_at in JSON")
            .unwrap_or("(no repos)")
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// W8.3 coverage matrix writer
// ---------------------------------------------------------------------------

/// Write the language coverage matrix to `matrix_path` (W8.3).
///
/// Renders a Markdown table with one row per language showing:
/// - Node count
/// - Which `NodeKind`s are present (function, class, method, import, …)
/// - Whether any `Calls` edges were produced for nodes in that language
///
/// Called automatically by [`run_benchmark`] when `write_report = true`.
pub fn write_coverage_matrix(metrics: &[RepoMetrics], matrix_path: &Path) -> Result<()> {
    let mut f = fs::File::create(matrix_path)
        .with_context(|| format!("create {}", matrix_path.display()))?;

    writeln!(f, "# Language coverage matrix (W8.3)")?;
    writeln!(f)?;
    writeln!(
        f,
        "> Generated by `wicked-estate-bench`. One row per language per repo."
    )?;
    writeln!(
        f,
        "> **Node count** = extraction richness; **Kinds** = which symbol types were extracted;"
    )?;
    writeln!(
        f,
        "> **Has calls** = whether any `Calls` edges were produced for nodes in that language."
    )?;
    writeln!(f)?;

    if metrics.is_empty() {
        writeln!(f, "*No repos benchmarked.*")?;
        return Ok(());
    }

    for m in metrics {
        writeln!(f, "## {}", m.repo)?;
        writeln!(f)?;
        if m.language_matrix.is_empty() {
            writeln!(f, "*No nodes extracted.*")?;
            writeln!(f)?;
            continue;
        }

        writeln!(f, "| Language | Nodes | Has calls | Kinds present |")?;
        writeln!(f, "|----------|-------|-----------|---------------|")?;
        for row in &m.language_matrix {
            let kinds = if row.kinds_present.is_empty() {
                "—".to_string()
            } else {
                row.kinds_present
                    .iter()
                    .map(|k| format!("`{}`", k))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let calls_mark = if row.has_calls_edges { "yes" } else { "no" };
            writeln!(
                f,
                "| `{}` | {} | {} | {} |",
                row.language, row.node_count, calls_mark, kinds,
            )?;
        }
        writeln!(f)?;
    }

    writeln!(f, "---")?;
    writeln!(f)?;
    writeln!(
        f,
        "*Last generated: {}*",
        metrics
            .first()
            .map(|_| "see generated_at in JSON")
            .unwrap_or("(no repos)")
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the capability benchmark against `paths`.
///
/// If `write_report` is `true`, writes `docs/benchmarks/capability-report.md` relative to the
/// workspace root (two levels above `CARGO_MANIFEST_DIR`).
///
/// Returns the full [`CapabilityReport`] regardless of whether the file write is requested.
pub fn run_benchmark(paths: &[PathBuf], write_report: bool) -> Result<CapabilityReport> {
    let mut repo_metrics = Vec::new();

    for path in paths {
        match benchmark_repo(path) {
            Ok(m) => repo_metrics.push(m),
            Err(e) => {
                eprintln!("  WARN: skipping {} — {:#}", path.display(), e);
            }
        }
    }

    let report = CapabilityReport {
        generated_at: chrono_now(),
        repos: repo_metrics,
    };

    if write_report {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
        let report_dir = PathBuf::from(&manifest)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("docs/benchmarks"))
            .unwrap_or_else(|| PathBuf::from("docs/benchmarks"));
        fs::create_dir_all(&report_dir)
            .with_context(|| format!("mkdir {}", report_dir.display()))?;

        let report_path = report_dir.join("capability-report.md");
        write_markdown_report(&report.repos, &report_path)?;
        eprintln!("  report written to {}", report_path.display());

        // W8.3 — language coverage matrix (separate file).
        let matrix_path = report_dir.join("coverage-matrix.md");
        write_coverage_matrix(&report.repos, &matrix_path)?;
        eprintln!("  coverage matrix written to {}", matrix_path.display());
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// Minimal timestamp helper (no chrono dep)
// ---------------------------------------------------------------------------

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = epoch_to_ymd_hms(secs);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, mi, s)
}

fn epoch_to_ymd_hms(mut secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let s = (secs % 60) as u32;
    secs /= 60;
    let mi = (secs % 60) as u32;
    secs /= 60;
    let h = (secs % 24) as u32;
    let mut days = secs / 24;

    let mut year = 1970u32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let months: [u64; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for dim in months {
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
    }
    (year, month, days as u32 + 1, h, mi, s)
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod direct_ref_tests {
    use super::*;
    use wicked_estate_core::{
        Descriptor, Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, ResolutionTier,
        Span, Symbol, UnresolvedRef,
    };

    fn func(name: &str) -> Node {
        Node::new(
            Symbol::global("bench", None, vec![Descriptor::method(name, None)]).id(),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new("a.rs", Span::ZERO),
        )
    }

    fn calls(from: &Node, to: &Node) -> Edge {
        Edge::new(
            from.symbol.clone(),
            to.symbol.clone(),
            EdgeKind::Calls,
            ResolutionTier::ImportMap,
            "name-resolver",
        )
    }

    /// One resolved and one unresolved direct reference to `target`, plus `downstream` callers
    /// of the resolved caller. The downstream chain must not move the ratio (BENCH-01).
    fn store(downstream: usize) -> SqliteStore {
        let mut s = SqliteStore::in_memory().unwrap();
        let (t, a, c) = (func("target"), func("caller"), func("unbound_caller"));
        let mut nodes = vec![t.clone(), a.clone(), c.clone()];
        let mut edges = vec![calls(&a, &t)];
        for i in 0..downstream {
            let d = func(&format!("down{i}"));
            edges.push(calls(&d, &a));
            nodes.push(d);
        }
        s.upsert_nodes(&nodes).unwrap();
        s.upsert_edges(&edges).unwrap();
        s.upsert_unresolved_refs(&[UnresolvedRef::new(
            c.symbol.clone(),
            "target",
            EdgeKind::Calls,
            Location::new("a.rs", Span::ZERO),
        )])
        .unwrap();
        s
    }

    #[test]
    fn downstream_dependents_do_not_change_direct_resolution() {
        for downstream in [0, 1, 25] {
            let r = direct_ref_resolution(&store(downstream), "target");
            assert_eq!(
                r,
                DirectRefResolution {
                    resolved: Some(1),
                    unresolved: Some(1),
                    pct: Some(50.0)
                },
                "downstream={downstream}"
            );
        }
    }

    #[test]
    fn an_unavailable_denominator_is_never_a_known_percentage() {
        // Every path into the percentage goes through `resolution_pct`: a failed read on either
        // side is `None` there (see `direct_ref_resolution`'s `.ok()`s), never a number.
        assert_eq!(
            resolution_pct(Some(5), None),
            None,
            "a failed unresolved read is not 100%"
        );
        assert_eq!(
            resolution_pct(None, Some(1)),
            None,
            "a failed resolved read is not 0%"
        );
        assert_eq!(
            resolution_pct(Some(0), Some(0)),
            None,
            "no references: no ratio"
        );
        assert_eq!(resolution_pct(Some(3), Some(1)), Some(75.0));
    }

    /// Resolvers that do not bind by the target's name are outside the population: a
    /// `rules-bridge-resolver` fan-out edge and a `relative-import` edge into a node named like
    /// the symbol count as nothing resolved.
    #[test]
    fn non_name_binding_resolvers_are_not_counted() {
        let mut s = store(0);
        let (t, b, r) = (func("target"), func("bridge_caller"), func("importer"));
        let edge = |from: &Node, kind: EdgeKind, by: &str| {
            Edge::new(
                from.symbol.clone(),
                t.symbol.clone(),
                kind,
                ResolutionTier::Heuristic,
                by,
            )
        };
        s.upsert_nodes(&[b.clone(), r.clone()]).unwrap();
        s.upsert_edges(&[
            edge(&b, EdgeKind::InvokedBy, "rules-bridge-resolver"),
            edge(&r, EdgeKind::Imports, "relative-import"),
        ])
        .unwrap();
        let r = direct_ref_resolution(&s, "target");
        assert_eq!(
            (r.resolved, r.unresolved, r.pct),
            (Some(1), Some(1), Some(50.0))
        );
    }

    /// A relationship an extractor emits locally (`Governs`) was never an unresolved reference:
    /// it is not counted as resolved.
    #[test]
    fn locally_emitted_relations_are_not_resolved_references() {
        let mut s = store(0);
        let (t, g) = (func("target"), func("ruleset"));
        let gov = Edge::new(
            g.symbol.clone(),
            t.symbol.clone(),
            EdgeKind::Governs,
            ResolutionTier::Parsed,
            "excel-rules",
        );
        s.upsert_nodes(&[g]).unwrap();
        s.upsert_edges(&[gov]).unwrap();
        let r = direct_ref_resolution(&s, "target");
        assert_eq!(
            (r.resolved, r.unresolved, r.pct),
            (Some(1), Some(1), Some(50.0))
        );
    }
}
