//! Build-gate (DoD-A3 emit-once): a single `index` / `annotate` run through the real
//! `wicked-estate` binary emits exactly one coarse event of the right type — no duplicates.
//!
//! ## How the gate observes emits without a real bus
//! The shared emit seam spawns `WICKED_ESTATE_EMIT_PROGRAM` (default `wicked-bus`). We point it
//! at a command that cannot exist, which forces the seam's failure path: every emit is
//! dead-lettered to the spool at `WICKED_ESTATE_EMIT_DEADLETTER` as one NDJSON line. The spool
//! is therefore a faithful, ordered record of every emit the command made — one line per emit.
//! Counting lines of a given `type` counts emits of that event. No network, no bus install,
//! fully deterministic and cross-platform.
//!
//! Falsifier: if the index path emitted zero `wicked.estate.indexed` events, the spool would
//! have no matching line; if it emitted duplicates, the count would exceed one.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn read_spool(path: &std::path::Path) -> Vec<serde_json::Value> {
    let body = fs::read_to_string(path).unwrap_or_default();
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("spool line must be JSON"))
        .collect()
}

fn count_type(records: &[serde_json::Value], event_type: &str) -> usize {
    records
        .iter()
        .filter(|r| r["type"] == event_type)
        .count()
}

/// Build a throwaway working dir with one trivial source file.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ci_emit_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("a.rs"), "fn f() {}\nfn g() { f(); }\n").unwrap();
    d
}

#[test]
fn index_emits_exactly_one_indexed_event() {
    let work = scratch("index");
    let db = work.join("graph.db");
    let spool = work.join("dl.ndjson");

    let status = Command::new(bin())
        .arg("index")
        .arg(&work)
        .arg("--db")
        .arg(&db)
        .env("WICKED_ESTATE_EMIT_DEADLETTER", &spool)
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-emit-xyzzy-index",
        )
        .status()
        .expect("run wicked-estate index");
    assert!(status.success(), "index command itself must succeed");

    let records = read_spool(&spool);
    assert_eq!(
        count_type(&records, "wicked.estate.indexed"),
        1,
        "index must emit exactly one wicked.estate.indexed (got records: {records:?})"
    );
    // No stray coarse events of the other kinds leaked from an index run.
    assert_eq!(count_type(&records, "wicked.estate.drifted"), 0);
    assert_eq!(count_type(&records, "wicked.estate.annotated"), 0);

    // The payload carries the honest counts.
    let rec = records
        .iter()
        .find(|r| r["type"] == "wicked.estate.indexed")
        .unwrap();
    assert!(rec["payload"]["files"].as_u64().unwrap() >= 1);

    let _ = fs::remove_dir_all(&work);
}

#[test]
fn annotate_emits_exactly_one_annotated_event() {
    let work = scratch("annotate");
    let db = work.join("graph.db");
    let spool = work.join("dl.ndjson");

    // Index first (its own spool, discarded) so there is a symbol to annotate.
    let idx = Command::new(bin())
        .arg("index")
        .arg(&work)
        .arg("--db")
        .arg(&db)
        .env(
            "WICKED_ESTATE_EMIT_DEADLETTER",
            work.join("idx-dl.ndjson"),
        )
        .env("WICKED_ESTATE_EMIT_PROGRAM", "wicked-bus-absent-emit-pre")
        .status()
        .expect("run index");
    assert!(idx.success());

    // Now annotate symbol `f`, capturing only this command's emits in `spool`.
    let status = Command::new(bin())
        .arg("annotate")
        .arg("f")
        .arg("--key")
        .arg("owner")
        .arg("--value")
        .arg("team-a")
        .arg("--db")
        .arg(&db)
        .env("WICKED_ESTATE_EMIT_DEADLETTER", &spool)
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-emit-xyzzy-ann",
        )
        .status()
        .expect("run wicked-estate annotate");
    assert!(status.success(), "annotate command itself must succeed");

    let records = read_spool(&spool);
    assert_eq!(
        count_type(&records, "wicked.estate.annotated"),
        1,
        "annotate must emit exactly one wicked.estate.annotated (got: {records:?})"
    );
    assert_eq!(count_type(&records, "wicked.estate.indexed"), 0);

    let _ = fs::remove_dir_all(&work);
}
