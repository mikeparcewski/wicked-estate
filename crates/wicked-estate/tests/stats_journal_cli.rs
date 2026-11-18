//! `wicked-estate stats --journal delete` (wicked-crew#1497, wicked-estate#491): the graph's owner
//! leaves a REAL indexed graph (estate's FTS triggers and all) in rollback-journal mode with
//! estate's own SQLite, so a reader that may not write its directory can open it. Spawns the real
//! binary on a graph the real indexer built.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_stats_journal_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run(cwd: &Path, db: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .args(["--db", db.to_str().unwrap()])
        .output()
        .expect("spawn wicked-estate")
}

fn header_mode(db: &Path) -> u8 {
    fs::read(db).unwrap()[18]
}

#[test]
fn stats_journal_delete_leaves_a_real_labelled_graph_in_delete_mode() {
    let dir = fresh_dir("delete");
    for label in ["a", "b"] {
        fs::create_dir_all(dir.join(label).join("src")).unwrap();
        fs::write(
            dir.join(label).join("src/lib.ts"),
            format!("export function in_{label}() {{ return 1; }}\n"),
        )
        .unwrap();
    }
    let db = dir.join("code-graph.db");
    for label in ["a", "b"] {
        let out = run(&dir, &db, &["index", label, "--repo", label]);
        assert!(out.status.success(), "index {label}: {out:?}");
    }
    assert_eq!(header_mode(&db), 2, "the indexer leaves a WAL graph");

    let out = run(&dir, &db, &["stats", "--journal", "delete"]);
    assert!(out.status.success(), "stats --journal delete: {out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("nodes="));
    assert_eq!(header_mode(&db), 1, "the graph is in rollback-journal mode");
    for side in ["code-graph.db-wal", "code-graph.db-shm"] {
        assert!(!dir.join(side).exists(), "{side} left behind");
    }
    // The JSON form does the same, and the graph still answers.
    let out = run(&dir, &db, &["stats", "--json", "--journal", "delete"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(header_mode(&db), 1);
    let out = run(&dir, &db, &["resolve", "in_a", "--json"]);
    assert!(out.status.success(), "{out:?}");

    // Only `delete` is a journal this command sets.
    let out = run(&dir, &db, &["stats", "--journal", "wal"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--journal takes `delete`"));
    let _ = fs::remove_dir_all(&dir);
}
