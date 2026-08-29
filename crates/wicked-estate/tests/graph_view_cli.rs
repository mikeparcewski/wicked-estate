//! graph-view must never surface File or Import nodes (round-1 R1-CORR-2).
//!
//! The `excluded` kind list in the graph-view arm is used twice: once to filter the
//! `important_symbols` seed/backfill (where File/Import are already impossible — the
//! ranked_symbols seam filters them) and once as the BFS EXPANSION gate. File nodes enter the
//! expansion frontier via file-scope Calls edges (a top-level call site's `from` is the File
//! symbol itself), then their Imports edges — including the lane's File→File edges — pull
//! Import nodes and more Files. Deleting File/Import from the list regressed graph-view on any
//! repo with a top-level call site; this drives the compiled binary against exactly that shape.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_gview_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

fn run(cwd: &Path, db: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new(bin());
    cmd.current_dir(cwd);
    cmd.args(args);
    cmd.args(["--db", db.to_str().unwrap()]);
    let out = cmd.output().expect("spawn wicked-estate");
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

#[test]
fn graph_view_expansion_never_yields_file_or_import_nodes() {
    let dir = fresh_dir("no_file_import");
    // a.ts has a FILE-SCOPE call site (a BARE top-level `coreThing();` statement — an assigned
    // call like `const x = coreThing()` attributes to the Constant, which is excluded anyway):
    // the ref's `from` is the File symbol, so the File node is one Calls hop from a ranked
    // function and enters the BFS frontier unless the expansion gate excludes it. The relative
    // imports give that File node Imports edges (synthetic Import node + the lane's File→File
    // edge) to pull more in. Without the gate this exact fixture yields 4 functions + 3 file +
    // 2 import nodes.
    fs::write(
        dir.join("src/a.ts"),
        "import { coreThing } from './b';\ncoreThing();\nexport function alpha() { return coreThing(); }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/b.ts"),
        "export function coreThing() { return 1; }\nexport function beta() { return coreThing(); }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/c.ts"),
        "import { alpha } from './a';\nexport function gamma() { return alpha(); }\n",
    )
    .unwrap();

    let db = dir.join("g.db");
    run(&dir, &db, &["index", dir.to_str().unwrap()]);
    let out = run(&dir, &db, &["graph-view", "--limit", "40"]);

    let v: serde_json::Value = serde_json::from_str(&out).expect("graph-view emits JSON");
    let nodes = v["nodes"].as_array().expect("nodes array");
    assert!(
        !nodes.is_empty(),
        "the fixture must produce ranked symbols: {out}"
    );
    let bad: Vec<&serde_json::Value> = nodes
        .iter()
        .filter(|n| matches!(n["kind"].as_str(), Some("file") | Some("import")))
        .collect();
    assert!(
        bad.is_empty(),
        "graph-view surfaced File/Import nodes via BFS expansion: {bad:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}
