//! SLC-001 — Store connection lifecycle: WAL mode + clean drop.
//!
//! Verifies two properties of `SqliteStore`:
//!
//! (a) `open()` enables WAL journal mode — the prerequisite for safe concurrent access
//!     (multiple readers, serialized writers, no full-file locking on reads).
//!
//! (b) After `drop(store)`, no connection owned by the store is still blocking a WAL
//!     checkpoint: `PRAGMA wal_checkpoint(TRUNCATE)` returns `busy = 0`.  `busy > 0`
//!     means at least one connection with an active read transaction prevented some WAL
//!     frames from being checkpointed — a signal that the store did not fully release its
//!     connections on drop.

use rusqlite::Connection;
use wicked_estate_store::SqliteStore;

#[test]
fn slc_001_wal_mode_and_clean_drop() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join("slc001.db");

    // Phase 1: open and drop the store to initialise the DB in WAL mode.
    {
        let _store = SqliteStore::open(&path).expect("open store phase-1");
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

    // Phase 3: re-open the store (simulates server startup after a clean shutdown).
    let store = SqliteStore::open(&path).expect("open store phase-3");

    // Phase 4: drop the store and confirm no store-owned connection is still blocking
    // a WAL checkpoint.  `PRAGMA wal_checkpoint(TRUNCATE)` returns (busy, log, ckpt):
    // `busy > 0` means a connection with an active read transaction prevented some frames
    // from being checkpointed — indicating the store did not fully close its connections.
    drop(store);
    let (busy, _log, _ckpt): (i64, i64, i64) = monitor
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .expect("wal_checkpoint");

    assert_eq!(
        busy, 0,
        "WAL checkpoint blocked after SqliteStore::drop (busy={busy}) — \
         a store-owned connection is still holding an active read transaction; \
         SqliteStore may not have fully released its connections on drop"
    );
}
