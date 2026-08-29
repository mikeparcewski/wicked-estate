//! CLI surfaces of the unresolved-accounting fix (docs/ENGINE-CONTRACT.md §2.1):
//! `stats` prints the count; `blast-radius` (text and `--json`) reports per-reference
//! unresolved counts — repeat call sites of a bound relationship are NOT unresolved, a call
//! to an undefined name is.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

/// Fixture A from the e2e suite: g is called three times (bound), h once (undefined).
fn fixture_a(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_unres_cli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("repo/src")).unwrap();
    fs::write(
        d.join("repo/src/mod.ts"),
        "export function g() {}\nexport function k() {}\n",
    )
    .unwrap();
    fs::write(
        d.join("repo/src/main.ts"),
        "import {g, k} from './mod';\nimport type {G} from './mod';\nexport function f() { g(); g(); g(); h(); k(); }\n",
    )
    .unwrap();
    d
}

fn run(cwd: &PathBuf, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

fn stdout_of(out: &Output) -> String {
    assert!(
        out.status.success(),
        "command failed\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn stats_and_blast_radius_report_per_reference_unresolved() {
    let dir = fixture_a("main");
    let db = dir.join("g.db");
    let db_s = db.to_str().unwrap();

    let out = run(&dir, &["index", "repo", "--db", db_s]);
    stdout_of(&out);

    // stats prints the unresolved count on its first line.
    let stats = stdout_of(&run(&dir, &["stats", "--db", db_s]));
    assert!(
        stats.contains("unresolved="),
        "stats must print the unresolved count: {stats}"
    );

    // g: three call sites, one bound relationship → 0 unresolved (HEAD printed 2).
    let g_text = stdout_of(&run(&dir, &["blast-radius", "g", "--db", db_s]));
    assert!(
        g_text.contains("0 unresolved call(s) reference 'g'"),
        "bound repeat sites must not be unresolved: {g_text}"
    );

    // h: undefined → exactly 1 unresolved (honest coverage keeps it).
    let h_text = stdout_of(&run(&dir, &["blast-radius", "h", "--db", db_s]));
    assert!(
        h_text.contains("1 unresolved call(s) reference 'h'"),
        "an undefined callee keeps its row: {h_text}"
    );

    // --json keeps the wire shape and agrees with the text path.
    let g_json = stdout_of(&run(&dir, &["blast-radius", "g", "--db", db_s, "--json"]));
    let v: serde_json::Value = serde_json::from_str(g_json.trim()).expect("valid JSON");
    assert_eq!(v["target"], "g");
    assert_eq!(v["unresolved"], 0, "json unresolved count for g: {v}");
    assert!(v["dependents"].is_array());

    let _ = fs::remove_dir_all(&dir);
}

/// Admissibility F-B, pinned end-to-end through the CLI: two SAME-LINE call sites of one
/// undefined name (`q(); q();`) must persist as two `unresolved_refs` rows with DISTINCT
/// `start_byte` — the within-line site discriminator that makes duplicate-site proofs pure SQL
/// (the closure's 1215 line-level "duplicate" groups needed 44 manual on-disk adjudications;
/// this is the mechanical replacement). Fresh fixture on purpose: Fixture A's pinned counts
/// ("h once / 1 unresolved") stay untouched.
#[test]
fn same_line_sites_persist_distinct_start_bytes() {
    use wicked_estate_core::GraphRead;
    use wicked_estate_store::SqliteStore;

    let d = std::env::temp_dir().join(format!("ci_unres_cli_bytes_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("repo/src")).unwrap();
    fs::write(
        d.join("repo/src/main.ts"),
        "export function f() { q(); q(); }\n",
    )
    .unwrap();

    let db = d.join("g.db");
    let db_s = db.to_str().unwrap();
    stdout_of(&run(&d, &["index", "repo", "--db", db_s]));

    let store = SqliteStore::open(&db).expect("open produced db");
    let refs = store
        .unresolved_refs_for_name("q")
        .expect("unresolved_refs_for_name");
    assert_eq!(refs.len(), 2, "both same-line sites keep their own row");
    let line0 = refs[0].location.span.start_line;
    let line1 = refs[1].location.span.start_line;
    assert_eq!(
        line0, line1,
        "the two sites ARE on one line (fixture premise)"
    );
    let b0 = refs[0].location.span.start_byte;
    let b1 = refs[1].location.span.start_byte;
    assert!(
        b0 != 0 && b1 != 0,
        "real sites must carry non-zero byte offsets (0 is the unknown/synthetic sentinel)"
    );
    assert_ne!(
        b0, b1,
        "same-line sites must be distinguishable by start_byte"
    );

    let _ = fs::remove_dir_all(&d);
}
