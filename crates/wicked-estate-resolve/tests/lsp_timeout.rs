//! Wedged-server timeout test — phase-0 of W3.6 (ADR-009).
//!
//! Registers a `sleep` binary masquerading as a language server and asserts that
//! `LspTier::definition` returns `Err` within the injected budget instead of hanging, and that
//! the masquerading child does not survive. This exercises the **spawn/initialize** timeout
//! path only — the client never enters the tier's cache (insertion happens after a successful
//! handshake); the eviction path has its own test in `src/lsp.rs`
//! (`tier_evicts_and_kills_the_client_when_a_request_times_out`).
//!
//! **Graceful degradation**: probe-and-skip on the `sleep` binary (absent on native Windows —
//! there the pure-std mechanism unit tests in `src/lsp.rs` are the coverage). No `#[ignore]`.

use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

use wicked_estate_resolve::lsp::{LspTier, ServerRegistry, path_to_file_uri};

fn binary_on_path(bin: &str) -> bool {
    let result = if cfg!(windows) {
        Command::new("where").arg(bin).output()
    } else {
        Command::new("which").arg(bin).output()
    };
    result.map(|o| o.status.success()).unwrap_or(false)
}

/// Distinctive sleep duration so the surviving-child check can pgrep for exactly this
/// masquerade and nothing else.
const MASQUERADE_SLEEP_SECS: &str = "63971";

#[test]
fn wedged_server_times_out_within_budget_and_leaves_no_child() {
    if !binary_on_path("sleep") {
        println!("[lsp_timeout] SKIP: `sleep` not found on PATH; wedged-server path not exercised");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("main.wl");
    fs::write(&src, "hello\n").unwrap();

    let mut reg = ServerRegistry::standard();
    reg.register(
        "wedgelang",
        "sleep",
        vec![MASQUERADE_SLEEP_SECS.to_string()],
    );

    let budget = Duration::from_millis(500);
    let mut tier = LspTier::with_registry(tmp.path().to_str().unwrap(), reg).with_timeout(budget);
    let uri = path_to_file_uri(src.to_str().unwrap());

    let start = Instant::now();
    let err = tier.definition("wedgelang", &uri, 0, 0).unwrap_err();
    let elapsed = start.elapsed();

    assert!(
        err.to_string().contains("timed out"),
        "expected a timeout error from the wedged initialize handshake, got: {err}"
    );
    assert!(
        elapsed >= budget,
        "returned before the deadline: {elapsed:?}"
    );
    assert!(
        elapsed < budget + Duration::from_secs(8),
        "timeout not honored — took {elapsed:?} for a {budget:?} budget (a hang here means the \
         read-timeout fix is broken)"
    );

    // The masquerading child must have been killed and reaped (spawn kills on handshake
    // failure). pgrep is guarded — absent on some minimal environments.
    if binary_on_path("pgrep") {
        let pattern = format!("sleep {MASQUERADE_SLEEP_SECS}");
        let out = Command::new("pgrep")
            .args(["-f", &pattern])
            .output()
            .expect("pgrep runs");
        assert!(
            !out.status.success(),
            "a wedged `{pattern}` child survived the timeout: pids {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    println!("[lsp_timeout] PASS — wedged server errored in {elapsed:?} (budget {budget:?})");
}
