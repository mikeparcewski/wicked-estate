//! SLC-001 — Store connection lifecycle: WAL mode + clean drop.
//!
//! Verifies two properties of `SqliteStore`:
//!
//! (a) `open()` enables WAL journal mode — the prerequisite for safe concurrent access
//!     (multiple readers, serialized writers, no full-file locking on reads).
//!
//! (b) After `drop(store)`, all connection handles held by the store are released:
//!     `PRAGMA wal_checkpoint(TRUNCATE)` returns `busy = 0`, meaning no external reader
//!     is blocking the checkpoint.  A store that opened N > 1 connections internally but
//!     only closed one of them on `drop` would leave N−1 handles live, causing the
//!     checkpoint to return `busy > 0` and failing assertion (b).

use rusqlite::Connection;
use wicked_estate_store::SqliteStore;

#[test]
fn slc_001_single_connection_handle_per_store_file() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("slc001.db");
    let path_str = path.to_str().unwrap();

    // Phase 1: open and drop the store to initialise the DB in WAL mode.
    {
        let _store = SqliteStore::open(path_str).expect("open store phase-1");
    }

    // Phase 2: open an independent monitoring connection and verify WAL mode.
    let monitor = Connection::open(&path).expect("open monitor connection");
    let journal_mode: String = monitor
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("PRAGMA journal_mode");
    assert_eq!(
        journal_mode, "wal",
        "SqliteStore must enable WAL mode on open; got '{journal_mode}'"
    );

    // Phase 3: re-open the store (simulates server startup).
    let store = SqliteStore::open(path_str).expect("open store phase-3");

    // Phase 4: drop the store and confirm WAL is fully checkpointable.
    //
    // `PRAGMA wal_checkpoint(TRUNCATE)` returns (busy, log, checkpointed).
    // `busy` is the number of reader connections — other than the one running
    // the checkpoint — that prevented some frames from being checkpointed.
    // If `SqliteStore` leaked additional connection handles, they would show
    // here as `busy > 0`.
    drop(store);
    let (busy, _log, _ckpt): (i64, i64, i64) = monitor
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .expect("wal_checkpoint");

    assert_eq!(
        busy, 0,
        "WAL checkpoint blocked after SqliteStore::drop (busy={busy}) — \
         indicates more than one connection handle was opened to '{path_str}'"
    );
}
