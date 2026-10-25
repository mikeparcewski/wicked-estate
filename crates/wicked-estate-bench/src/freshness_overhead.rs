//! Per-call overhead of per-query freshness (wicked-estate#351): what
//! [`wicked_estate::freshness::FreshnessProbe::annotate`] adds to one retrieval response, warm.
//!
//! The corpus is OFFLINE and deterministic: a committed git repo of [`FILES`] TypeScript modules,
//! indexed by the real `index_path` into an on-disk SQLite graph (the probe opens its own handle,
//! as the MCP server's does). Each measured call annotates an answer touching [`ANSWER_FILES`]
//! distinct files — the shape of a `BlastRadius` page — with one file edited on disk, so the
//! drift path (stat, digest cache, re-hash on change) is exercised. The cost is dominated by the
//! one `git status` per root per call; the commit count behind it is cached on HEAD.

use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Instant;
use wicked_estate::freshness::FreshnessProbe;
use wicked_estate_store::SqliteStore;

/// Modules in the generated repo.
pub const FILES: usize = 300;
/// Distinct files one measured answer touches.
pub const ANSWER_FILES: usize = 20;
/// Measured calls (after one warm-up call).
pub const CALLS: usize = 100;
/// The #351 target: p95 per-call overhead, warm, milliseconds.
pub const P95_TARGET_MS: f64 = 10.0;

/// One run's result.
#[derive(Debug, Clone)]
pub struct OverheadReport {
    pub files: usize,
    pub answer_files: usize,
    pub calls: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_AUTHOR_NAME", "bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.invalid")
        .env("GIT_COMMITTER_NAME", "bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.invalid")
        .output()?;
    anyhow::ensure!(out.status.success(), "git {args:?} failed");
    Ok(())
}

/// Build, commit and index the corpus under `dir`; returns the db path.
fn corpus(dir: &Path) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir.join("src"))?;
    for i in 0..FILES {
        let next = (i + 1) % FILES;
        std::fs::write(
            dir.join(format!("src/m{i:03}.ts")),
            format!(
                "import {{ f{next} }} from './m{next:03}';\n\
                 export function f{i}(x: number): number {{ return f{next}(x) + {i}; }}\n\
                 export function g{i}(): number {{ return f{i}(1); }}\n"
            ),
        )?;
    }
    std::fs::write(dir.join(".gitignore"), "*.db*\n")?;
    git(dir, &["init", "-q", "."])?;
    git(dir, &["add", "-A"])?;
    git(dir, &["commit", "-qm", "corpus"])?;
    let db = dir.join("graph.db");
    let mut store = SqliteStore::open(&db).map_err(|e| anyhow::anyhow!("{e}"))?;
    wicked_estate::index_path(&mut store, dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(db)
}

fn answer() -> Value {
    let rows: Vec<Value> = (0..ANSWER_FILES)
        .map(|i| json!({"symbol": format!("s{i}"), "file": format!("src/m{i:03}.ts")}))
        .collect();
    json!({ "dependents": rows, "total": ANSWER_FILES })
}

/// Build the corpus in a temp dir and measure [`CALLS`] warm `annotate` calls.
pub fn run_freshness_overhead_bench() -> Result<OverheadReport> {
    let dir =
        std::env::temp_dir().join(format!("wicked-estate-fresh-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = corpus(&dir)?;
    // One drifted file in the answer: the drift path is real work, not a no-op.
    std::fs::write(
        dir.join("src/m000.ts"),
        "export function f0(): number { return 0; }\n",
    )?;
    let mut probe = FreshnessProbe::open(db.to_str().unwrap_or_default());
    let mut warm = answer();
    probe.annotate(&mut warm, &mut vec![]);
    anyhow::ensure!(
        warm["freshness"]["stale_files_total"] == json!(1),
        "the bench must exercise drift: {}",
        warm["freshness"]
    );
    let mut lat: Vec<f64> = Vec::with_capacity(CALLS);
    for _ in 0..CALLS {
        let mut content = answer();
        let mut diags = Vec::new();
        let t = Instant::now();
        probe.annotate(&mut content, &mut diags);
        lat.push(t.elapsed().as_secs_f64() * 1_000.0);
    }
    let _ = std::fs::remove_dir_all(&dir);
    lat.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| lat[((p * lat.len() as f64).ceil() as usize).clamp(1, lat.len()) - 1];
    Ok(OverheadReport {
        files: FILES,
        answer_files: ANSWER_FILES,
        calls: CALLS,
        p50_ms: pct(0.50),
        p95_ms: pct(0.95),
        max_ms: *lat.last().unwrap_or(&0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the overhead and holds a regression tripwire. The #351 target (p95 ≤ 10 ms warm)
    /// is reported, not asserted: the cost is one `git status` process per root per call, which
    /// varies with the host's load far more than with this code — the number in
    /// `docs/benchmarks/freshness-overhead.md` is the measured claim. The tripwire (5x the
    /// target) catches a change that adds per-file processes or drops the digest cache.
    #[test]
    fn freshness_overhead_is_recorded_and_bounded() {
        let r = run_freshness_overhead_bench().expect("bench runs");
        eprintln!(
            "freshness overhead: {} files, {}-file answer, {} calls: p50 {:.2} ms, p95 {:.2} ms, \
             max {:.2} ms (target p95 <= {P95_TARGET_MS} ms)",
            r.files, r.answer_files, r.calls, r.p50_ms, r.p95_ms, r.max_ms
        );
        assert!(
            r.p95_ms <= P95_TARGET_MS * 5.0,
            "p95 {:.2} ms is past the 5x regression tripwire",
            r.p95_ms
        );
    }
}
