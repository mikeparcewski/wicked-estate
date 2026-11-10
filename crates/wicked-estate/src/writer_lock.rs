//! One estate writer per database file (DES-S2 §4, slice X1).
//!
//! crew's project refresh coalesces its own refreshes in-process, but nothing stopped a second
//! process (a CLI run, a second daemon on the same state home) from writing the same database
//! between the linker's read and its write. Every estate write command takes the writer lock
//! ([`wicked_estate_store::writer_lock`]) before it opens the store. A second writer waits up to a
//! bound and then refuses with `WRITER-BUSY`; it never interleaves.

use std::time::Duration;

use wicked_estate_core::Result;
pub use wicked_estate_store::writer_lock::WriterLock;

/// Environment override for how long a writer waits for the lock, in seconds (default 120).
pub const WAIT_ENV: &str = "WICKED_ESTATE_WRITER_WAIT_SECS";

fn wait_budget() -> Duration {
    std::env::var(WAIT_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(120))
}

/// Take the writer lock for the SQLite store at `db_path` (`None` for `:memory:` or a non-file
/// store, which have no second writer to race). `command` names the refused command in the error.
pub fn acquire(db_path: &str, command: &str) -> Result<Option<WriterLock>> {
    if db_path == ":memory:" || (db_path.contains("://") && !db_path.starts_with("sqlite://")) {
        return Ok(None);
    }
    let path = db_path.strip_prefix("sqlite://").unwrap_or(db_path);
    if let Some(dir) = std::path::Path::new(path)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        std::fs::create_dir_all(dir)?;
    }
    wicked_estate_store::writer_lock::acquire(path, command, wait_budget()).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_and_server_stores_take_no_lock() {
        assert!(acquire(":memory:", "index").unwrap().is_none());
        assert!(acquire("postgres://h/db", "index").unwrap().is_none());
    }
}
