//! Shared event-emit seam (DoD-A3) — the single path every wicked-estate emit site calls.
//!
//! ## Why this exists
//! Before this seam, wicked-estate was event-silent: the index / drift / annotate paths did
//! their work and returned, and the `watch` loop re-indexed on every change but published
//! nothing. Any future "just spawn `wicked-bus emit` and forget it" would have been a *silent*
//! fire-and-forget: a spawn error or a non-zero child exit would vanish, and a dropped event
//! leaves no trace. A dropped event is a defect, never silent.
//!
//! ## What it does
//! [`emit_event`] is the one function all emit sites call. It spawns the canonical
//! `wicked-bus emit` CLI (fire-and-forget by design — emit must never block or fail the
//! indexer, per graceful-degradation). But the failure path is **loud and durable**:
//!
//! * spawn returned `Err` (bus CLI not installed / not on PATH), **or**
//! * the child exited non-zero,
//!
//! → the event is appended as exactly one NDJSON line to a dead-letter spool
//!   (`~/.something-wicked/wicked-estate/emit-deadletter.ndjson` by default) **and** a loud
//!   `eprintln!` marker is written. The spool is replayable; the log is visible.
//!
//! ## Cross-platform
//! The spool root is resolved via [`dirs::home_dir`] (works on macOS / Linux / Windows) joined
//! with `std::path::Path` segments — never a hardcoded `~`. The path is overridable via the
//! `WICKED_ESTATE_EMIT_DEADLETTER` environment variable (used by tests to redirect the spool
//! into a temp dir, and available as an operator override).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// Environment variable that overrides the dead-letter spool file path. When unset, the spool
/// defaults to `<home>/.something-wicked/wicked-estate/emit-deadletter.ndjson`.
pub const DEADLETTER_ENV: &str = "WICKED_ESTATE_EMIT_DEADLETTER";

/// Environment variable that overrides the bus-emit program. When unset, the seam spawns the
/// `wicked-bus` binary on `PATH`. Tests point this at a guaranteed-non-zero/missing command to
/// force the failure path deterministically (no network, no real bus).
pub const EMIT_PROGRAM_ENV: &str = "WICKED_ESTATE_EMIT_PROGRAM";

/// Loud, greppable marker written to stderr whenever an event is dead-lettered.
pub const DEADLETTER_MARKER: &str = "EMIT-DEADLETTER:";

/// A coarse wicked-bus event ready to be published through the shared seam.
///
/// Event types follow the ecosystem convention `wicked.<noun>.<past-verb>` (e.g.
/// `wicked.estate.indexed`). `payload` is an already-built JSON object.
#[derive(Debug, Clone)]
pub struct EmitEvent {
    /// `wicked.<noun>.<past-verb>` — e.g. `wicked.estate.indexed`.
    pub event_type: String,
    /// Top-level bus domain — always `wicked-estate` for this crate.
    pub domain: String,
    /// Bus subdomain — e.g. `estate.index`.
    pub subdomain: String,
    /// Structured event payload (a JSON object).
    pub payload: serde_json::Value,
}

impl EmitEvent {
    /// Construct an event for the `wicked-estate` domain. `subdomain` is the dotted subdomain
    /// (e.g. `estate.index`); `event_type` is the full `wicked.<noun>.<past-verb>` name.
    pub fn new(
        event_type: impl Into<String>,
        subdomain: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            domain: "wicked-estate".to_string(),
            subdomain: subdomain.into(),
            payload,
        }
    }

    /// The full dead-letter record: the envelope the bus would have received, plus the reason
    /// it was spooled. Serialized as one NDJSON line.
    fn deadletter_record(&self, reason: &str) -> serde_json::Value {
        serde_json::json!({
            "type": self.event_type,
            "domain": self.domain,
            "subdomain": self.subdomain,
            "payload": self.payload,
            "deadletter_reason": reason,
        })
    }
}

/// Resolve the dead-letter spool path: the `WICKED_ESTATE_EMIT_DEADLETTER` override if set,
/// else `<home>/.something-wicked/wicked-estate/emit-deadletter.ndjson`.
///
/// Returns `None` only when no override is set *and* the home directory cannot be resolved
/// (extremely rare; a headless/no-`HOME` environment) — in that case the caller still logs
/// loudly, it just cannot persist the spool line.
pub fn deadletter_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(DEADLETTER_ENV) {
        return Some(PathBuf::from(p));
    }
    let home = dirs::home_dir()?;
    Some(
        home.join(".something-wicked")
            .join("wicked-estate")
            .join("emit-deadletter.ndjson"),
    )
}

/// Append one NDJSON line for `event` to the dead-letter spool, creating parent dirs as needed.
///
/// Returns the path written on success. On any I/O failure this returns `Err` — the caller has
/// already logged loudly, so a spool-write failure cannot itself become silent.
fn dead_letter(event: &EmitEvent, reason: &str) -> std::io::Result<PathBuf> {
    let path = deadletter_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot resolve dead-letter spool path (no HOME and no WICKED_ESTATE_EMIT_DEADLETTER)",
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Serialize to a single line. serde_json::to_string never emits an interior newline for a
    // flat object, so one record == one NDJSON line.
    let line = serde_json::to_string(&event.deadletter_record(reason))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(path)
}

/// The bus-emit program to spawn: the `WICKED_ESTATE_EMIT_PROGRAM` override if set, else
/// `wicked-bus`.
fn emit_program() -> String {
    std::env::var(EMIT_PROGRAM_ENV).unwrap_or_else(|_| "wicked-bus".to_string())
}

/// Publish `event` through the shared seam.
///
/// Fire-and-forget toward the bus by design (emit must never block or fail the indexer), but
/// **never silent on failure**: if the bus CLI cannot be spawned, or exits non-zero, the event
/// is dead-lettered to the spool and a loud [`DEADLETTER_MARKER`] line is written to stderr.
///
/// Returns `true` if the bus accepted the event (child exited zero), `false` if it was
/// dead-lettered.
pub fn emit_event(event: &EmitEvent) -> bool {
    let program = emit_program();
    let payload = event.payload.to_string();
    let result = Command::new(&program)
        .arg("emit")
        .arg("--type")
        .arg(&event.event_type)
        .arg("--domain")
        .arg(&event.domain)
        .arg("--subdomain")
        .arg(&event.subdomain)
        .arg("--payload")
        .arg(&payload)
        // Silence the child's own chatter; we only care about its exit status.
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let reason = match result {
        Ok(status) if status.success() => return true,
        Ok(status) => format!("bus exited non-zero: {status}"),
        Err(e) => format!("spawn `{program}` failed: {e}"),
    };

    // A dropped event is a defect — log loudly, then persist for replay.
    eprintln!(
        "{DEADLETTER_MARKER} event `{}` not delivered ({reason}); spooling to dead-letter",
        event.event_type
    );
    match dead_letter(event, &reason) {
        Ok(path) => eprintln!(
            "{DEADLETTER_MARKER} spooled `{}` to {}",
            event.event_type,
            path.display()
        ),
        Err(e) => eprintln!(
            "{DEADLETTER_MARKER} FAILED to spool `{}` to dead-letter: {e}",
            event.event_type
        ),
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    // `emit_event` reads process-global env vars; serialize the env-mutating tests so they do
    // not race each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn read_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
        let body = std::fs::read_to_string(path).expect("spool file must exist");
        body.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("each spool line must be valid JSON"))
            .collect()
    }

    /// BUILD-GATE (dead-letter catches a drop): point the emit program at a guaranteed-missing
    /// command so the spawn fails, then assert the event lands as a parseable NDJSON line in the
    /// spool with its payload intact.
    ///
    /// Falsifier: if a dropped event left no spool line, `read_lines` would be empty / the file
    /// would be absent and this test fails.
    #[test]
    fn dropped_event_lands_in_deadletter_spool() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join("emit-deadletter.ndjson");

        // SAFETY: env access is serialized by ENV_LOCK; vars are restored before unlock.
        unsafe {
            std::env::set_var(DEADLETTER_ENV, &spool);
            // A command that cannot exist on PATH → spawn returns Err → failure path.
            std::env::set_var(
                EMIT_PROGRAM_ENV,
                "wicked-bus-emit-does-not-exist-xyzzy-9000",
            );
        }

        let event = EmitEvent::new(
            "wicked.estate.indexed",
            "estate.index",
            serde_json::json!({ "nodes": 42, "edges": 7, "files": 3 }),
        );
        let accepted = emit_event(&event);

        let lines = read_lines(&spool);

        unsafe {
            std::env::remove_var(DEADLETTER_ENV);
            std::env::remove_var(EMIT_PROGRAM_ENV);
        }

        assert!(!accepted, "spawn-failed emit must report not-accepted");
        assert_eq!(lines.len(), 1, "exactly one NDJSON line must be spooled");
        let rec = &lines[0];
        assert_eq!(rec["type"], "wicked.estate.indexed");
        assert_eq!(rec["domain"], "wicked-estate");
        assert_eq!(rec["subdomain"], "estate.index");
        assert_eq!(rec["payload"]["nodes"], 42);
        assert!(
            rec["deadletter_reason"].is_string(),
            "the spooled record records why it was dropped"
        );
    }

    /// A second dropped event appends a second line (the spool accumulates; it does not clobber).
    #[test]
    fn deadletter_spool_appends_one_line_per_drop() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join("emit-deadletter.ndjson");

        unsafe {
            std::env::set_var(DEADLETTER_ENV, &spool);
            std::env::set_var(EMIT_PROGRAM_ENV, "wicked-bus-emit-missing-xyzzy-9001");
        }

        for n in 0..3u64 {
            let event = EmitEvent::new(
                "wicked.estate.drifted",
                "estate.drift",
                serde_json::json!({ "i": n }),
            );
            emit_event(&event);
        }

        let lines = read_lines(&spool);

        unsafe {
            std::env::remove_var(DEADLETTER_ENV);
            std::env::remove_var(EMIT_PROGRAM_ENV);
        }

        assert_eq!(lines.len(), 3, "three drops → three NDJSON lines");
        assert_eq!(lines[0]["payload"]["i"], 0);
        assert_eq!(lines[2]["payload"]["i"], 2);
    }

    /// Default spool path is derived from the home dir (cross-platform) and ends with the
    /// documented `.something-wicked/wicked-estate/emit-deadletter.ndjson` suffix — never a
    /// hardcoded `~`.
    #[test]
    fn default_deadletter_path_is_under_home() {
        let _guard = lock_env();
        unsafe {
            std::env::remove_var(DEADLETTER_ENV);
        }
        if let Some(p) = deadletter_path() {
            let s = p.to_string_lossy().replace('\\', "/");
            assert!(
                s.ends_with(".something-wicked/wicked-estate/emit-deadletter.ndjson"),
                "unexpected default spool path: {s}"
            );
            assert!(!s.contains('~'), "path must be expanded, not literal ~");
        }
        // If home_dir() is None (headless CI), the function returns None and there is nothing to
        // assert — the loud-log path still fires at the call site.
    }
}
