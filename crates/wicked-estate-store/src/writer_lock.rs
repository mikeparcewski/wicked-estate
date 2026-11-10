//! One estate writer per database file (DES-S2 §4, slice X1).
//!
//! The lock is a small SQLite database next to the store, `<db>.writer.lock`, held in an
//! `EXCLUSIVE` transaction for as long as the [`WriterLock`] lives. SQLite's own file locking makes
//! it cross-platform and cross-process (and correct between two connections in one process), and
//! the operating system releases it when the process exits, so a crashed writer never leaves it
//! stuck. A second writer waits up to its budget, then is refused.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode};
use wicked_estate_core::{Error, Result};

/// Held for as long as a write command runs; dropping it releases the lock.
#[derive(Debug)]
pub struct WriterLock {
    _conn: Connection,
}

/// Take the writer lock for the store file `db_path`, waiting up to `wait`. `command` names the
/// refused command in the `WRITER-BUSY` error.
pub fn acquire(db_path: &str, command: &str, wait: Duration) -> Result<WriterLock> {
    let conn = Connection::open(lock_path(db_path))
        .map_err(|e| Error::Storage(format!("writer lock: {e}")))?;
    conn.busy_timeout(wait)
        .map_err(|e| Error::Storage(format!("writer lock: {e}")))?;
    match conn.execute_batch("BEGIN EXCLUSIVE") {
        Ok(()) => Ok(WriterLock { _conn: conn }),
        Err(rusqlite::Error::SqliteFailure(f, _))
            if matches!(f.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) =>
        {
            Err(Error::Invalid(format!(
                "WRITER-BUSY: another estate writer holds {db_path} (waited {}s); `{command}` \
                 refused instead of interleaving writes",
                wait.as_secs()
            )))
        }
        Err(e) => Err(Error::Storage(format!("writer lock: {e}"))),
    }
}

/// `<db>.writer.lock` for the CANONICAL database path, so two spellings of one database (a
/// symlink, `./g.db` vs an absolute path) take the same lock. A database that does not exist yet
/// is placed under its canonical parent directory.
fn lock_path(db_path: &str) -> std::path::PathBuf {
    let p = Path::new(db_path);
    let canon = std::fs::canonicalize(p).ok().or_else(|| {
        let parent = p
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let dir = std::fs::canonicalize(parent).ok()?;
        Some(dir.join(p.file_name()?))
    });
    let mut s = canon.unwrap_or_else(|| p.to_path_buf()).into_os_string();
    s.push(".writer.lock");
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two spellings of one database (a symlink) take one lock.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_spelling_takes_the_same_lock() {
        let dir = std::env::temp_dir().join(format!("wl-ln-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.db");
        std::fs::write(&real, b"").unwrap();
        let alias = dir.join("alias.db");
        let _ = std::fs::remove_file(&alias);
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let _first = acquire(real.to_str().unwrap(), "index", Duration::from_millis(50)).unwrap();
        let err = acquire(alias.to_str().unwrap(), "link", Duration::from_millis(50))
            .unwrap_err()
            .to_string();
        assert!(err.contains("WRITER-BUSY"), "{err}");
    }

    /// G2.8: a second writer is refused while the first holds the lock; released on drop.
    #[test]
    fn a_second_writer_is_refused_while_the_first_holds_the_lock() {
        let dir = std::env::temp_dir().join(format!("wl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("g.db");
        let db = db.to_str().unwrap();
        let first = acquire(db, "index", Duration::from_millis(50)).unwrap();
        let err = acquire(db, "link", Duration::from_millis(50))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("WRITER-BUSY") && err.contains("`link`"),
            "{err}"
        );
        drop(first);
        assert!(
            acquire(db, "link", Duration::from_millis(50)).is_ok(),
            "released on drop"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
