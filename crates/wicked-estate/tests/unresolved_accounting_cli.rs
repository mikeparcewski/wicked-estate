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
