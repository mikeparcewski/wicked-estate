//! wicked-estate#601 (codex-check on #691): a read-only refusal names the build that last wrote
//! the graph from a value in the graph file. A crafted graph can hold a huge value there, so the
//! refusal must not read it whole: under a SQLite heap limit far below the value's size, the
//! refusal still names it as not a version.
//!
//! Its own test binary: the heap limit is process-wide, so no other test may run beside it.

use rusqlite::Connection;
use wicked_estate_store::SqliteStore;

#[test]
fn a_huge_recorded_writer_is_refused_without_reading_it_601() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("graph.db");
    drop(SqliteStore::open(&db).unwrap());
    let huge = "9".repeat(48 << 20);
    let c = Connection::open(&db).unwrap();
    c.execute(
        "UPDATE meta SET v = ?1 WHERE k = 'writer_version'",
        [huge.as_str()],
    )
    .unwrap();
    assert_eq!(c.changes(), 1, "the writer open recorded a row to replace");
    c.execute_batch("DROP TABLE search_misses;").unwrap();
    drop(c);
    drop(huge);
    // SAFETY: a process-wide SQLite setting; this binary runs this one test.
    let before = unsafe { rusqlite::ffi::sqlite3_hard_heap_limit64(16 << 20) };
    let err = SqliteStore::open_readonly_current(&db)
        .err()
        .map(|e| e.to_string());
    // SAFETY: as above, restoring the previous limit.
    unsafe { rusqlite::ffi::sqlite3_hard_heap_limit64(before) };
    let err = err.expect("a graph without search_misses is refused");
    assert!(err.contains("search_misses"), "{err}");
    assert!(
        err.contains("last opened for writing by a build whose recorded version is not a version"),
        "{err}"
    );
}
