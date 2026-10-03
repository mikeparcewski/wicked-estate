//! `blast-radius --depth` is bounded by the same ceiling as the MCP `BlastRadius` tool.
//!
//! The recursive walk grows with depth on a cyclic graph: on estate's own graph `--depth 100000`
//! ran past 20 s and 397 MB without answering. The CUT line invites the user to raise `--depth`,
//! so an unbounded flag was a hang one suggestion away. Above the ceiling the CLI refuses with a
//! message that names it.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_brdepth_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

fn run(cwd: &PathBuf, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

#[test]
fn depth_above_the_ceiling_is_refused_and_the_ceiling_itself_is_accepted() {
    let dir = scratch("cap");
    fs::write(
        dir.join("src/a.ts"),
        "export function f(): number { return 1; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/b.ts"),
        "import { f } from './a';\nexport function g(): number { return f(); }\n",
    )
    .unwrap();
    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    let src = dir.join("src");
    let out = run(&dir, &["index", src.to_str().unwrap(), "--db", db]);
    assert!(out.status.success(), "index failed: {out:?}");

    let ceiling = wicked_estate_retrieve::BLAST_DEPTH_CEILING.to_string();
    let ok = run(
        &dir,
        &["blast-radius", "f", "--depth", &ceiling, "--db", db],
    );
    assert!(
        ok.status.success(),
        "--depth at the ceiling must run: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let stdout = String::from_utf8_lossy(&ok.stdout);
    assert!(
        stdout.contains("depend on 'f'"),
        "expected g as a dependent: {stdout}"
    );

    let over = (wicked_estate_retrieve::BLAST_DEPTH_CEILING + 1).to_string();
    for args in [
        vec!["blast-radius", "f", "--depth", over.as_str(), "--db", db],
        vec!["blast-radius", "f", "--depth=100000", "--db", db],
    ] {
        let out = run(&dir, &args);
        assert!(
            !out.status.success(),
            "{args:?} must be refused, got stdout={}",
            String::from_utf8_lossy(&out.stdout)
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains(&format!("maximum of {ceiling}")),
            "the error must name the ceiling: {err}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// wicked-estate#194: `blast-radius --json` carries `confidence: {min, avg, edge_count}` over the
/// DEPENDENCY edges inside the answer. Here that is exactly two: `g → f` (calls) and
/// `b.ts → a.ts` (imports) — both endpoints of each are returned rows. The two parse-certain
/// 1.0 `contains` edges (`a.ts → f`, `b.ts → g`) are structure, not dependency: counting them
/// would make it 4 and lift the average toward "verified".
#[test]
fn json_confidence_envelope_counts_dependency_edges_only() {
    let dir = scratch("conf");
    fs::write(
        dir.join("src/a.ts"),
        "export function f(): number { return 1; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/b.ts"),
        "import { f } from './a';\nexport function g(): number { return f(); }\n",
    )
    .unwrap();
    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    let src = dir.join("src");
    let out = run(&dir, &["index", src.to_str().unwrap(), "--db", db]);
    assert!(out.status.success(), "index failed: {out:?}");

    let out = run(&dir, &["blast-radius", "f", "--json", "--db", db]);
    assert!(out.status.success(), "{out:?}");
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let conf = &doc["confidence"];
    assert_eq!(conf["edge_count"], serde_json::json!(2), "{doc}");

    let out = run(&dir, &["path", "g", "f", "--json", "--db", db]);
    let path: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let hop_conf = &path["hops"][0]["confidence"];
    // The weakest edge in the answer is the name-resolved call, as `path` reports it.
    assert_eq!(&conf["min"], hop_conf, "{doc}");
    // The #190 honesty keys survive the new key.
    for key in [
        "searched_depth",
        "depth_horizon_reached",
        "node_cap_reached",
    ] {
        assert!(doc.get(key).is_some(), "{key} lost: {doc}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// wicked-estate#194: `cross-graph` prints one `evidence [<db>]:` line PER REPO — never a pooled
/// figure, since each repo is resolved by its own tiers.
#[test]
fn cross_graph_prints_evidence_per_repo() {
    let dir = scratch("xgraph");
    let mut dbs = Vec::new();
    for repo in ["one", "two"] {
        let src = dir.join(repo);
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("a.ts"),
            "export function f(): number { return 1; }\n",
        )
        .unwrap();
        fs::write(
            src.join("b.ts"),
            "import { f } from './a';\nexport function g(): number { return f(); }\n",
        )
        .unwrap();
        let db = dir
            .join(format!("{repo}.db"))
            .to_string_lossy()
            .into_owned();
        let out = run(&dir, &["index", src.to_str().unwrap(), "--db", &db]);
        assert!(out.status.success(), "index {repo} failed: {out:?}");
        dbs.push(db);
    }

    let out = run(
        &dir,
        &["cross-graph", "f", "--db", &dbs[0], "--db", &dbs[1]],
    );
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("evidence ["))
        .collect();
    assert_eq!(lines.len(), 2, "one evidence line per repo:\n{stdout}");
    for (line, db) in lines.iter().zip(&dbs) {
        assert!(line.contains(db.as_str()), "{line} should name {db}");
        assert!(
            line.contains("dependency edge(s); confidence min"),
            "{line}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
