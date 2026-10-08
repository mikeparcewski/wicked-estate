//! `wicked-estate path` end to end, through the real binary.
//!
//! These assertions are about exit codes, usage lines, `--help` text and the stdout
//! document — all properties of `main.rs`, which an in-process test cannot reach. So this
//! spawns the built binary, following the same pattern as `repo_flag_cli.rs`.
//!
//! The argument cases are not incidental. The binary's shared parser pushes every token it
//! does not recognise into `positional`, so a two-operand command that merely scans for
//! `--json` (the way the one-operand `blast-radius` arm does) resolves `to` to `"--json"`
//! on `path A --json`. Each case below fails against that implementation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

/// Owns a scratch directory and removes it on drop, so a CI run does not leave a dozen
/// indexed repositories and databases behind in the temp dir.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A Rust source tree with a call chain `f0 → f1 → … → fN`, indexed into a scratch db.
/// The db lives at `<dir>/graph.db`; the directory is removed when the handle drops.
fn indexed_chain(tag: &str, depth: usize) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_pathcli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    // Own the directory from the moment its path exists, so EVERY fallible step below is
    // covered — the round-2 repair guarded only the `index` spawn, leaving the two
    // filesystem calls able to panic and leak the very directory this handle exists to
    // clean up.
    let scratch = Scratch(d);
    fs::create_dir_all(scratch.join("src")).unwrap();

    // f0 calls f1 calls f2 … so the dependency direction runs f0 → fN.
    let mut src = String::new();
    for i in 0..=depth {
        if i == depth {
            src.push_str(&format!("fn f{i}() {{}}\n"));
        } else {
            src.push_str(&format!("fn f{i}() {{ f{}(); }}\n", i + 1));
        }
    }
    fs::write(scratch.join("src/a.rs"), src).unwrap();

    let out = Command::new(bin())
        .current_dir(&*scratch)
        .args(["index", ".", "--db", "graph.db"])
        .output()
        .expect("spawn index");
    assert!(
        out.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    scratch
}

/// Run `git` in `dir`, failing loudly. Used to make a fixture that can actually go stale.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .unwrap_or_else(|e| panic!("spawn git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A chain fixture that is a git repo with a commit made AFTER indexing, so the staleness
/// notice genuinely fires.
///
/// Without this, `json_mode_emits_no_staleness_notice` asserted the absence of a string the
/// fixture could never produce — a plain temp dir makes `commits_behind` return `None`, so
/// deleting the `if !json_out` guard left every test green.
fn indexed_stale_chain(tag: &str, depth: usize) -> Scratch {
    let scratch = indexed_chain(tag, depth);
    git(&scratch, &["init", "-q"]);
    git(&scratch, &["add", "-A"]);
    git(&scratch, &["commit", "-qm", "base"]);
    // A commit strictly after the db's mtime is what `commits_behind` counts.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(scratch.join("src/later.rs"), "fn later() {}\n").unwrap();
    git(&scratch, &["add", "-A"]);
    git(&scratch, &["commit", "-qm", "after indexing"]);
    scratch
}

fn path_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(dir)
        .arg("path")
        .args(args)
        .args(["--db", "graph.db"])
        .output()
        .expect("spawn wicked-estate path")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn json_of(out: &Output) -> serde_json::Value {
    let s = stdout_of(out);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!("stdout must be exactly one JSON document; parse failed: {e}\n--- stdout ---\n{s}")
    })
}

// ── the route itself ─────────────────────────────────────────────────────────

#[test]
fn text_mode_prints_one_line_per_hop_with_kind_and_confidence() {
    let d = indexed_chain("text", 3);
    let out = path_in(&d, &["f0", "f3"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = stdout_of(&out);

    let hop_lines: Vec<&str> = s.lines().filter(|l| l.contains("->")).collect();
    assert_eq!(hop_lines.len(), 3, "one line per hop, got:\n{s}");
    for line in &hop_lines {
        assert!(
            line.contains("confidence"),
            "each hop names its confidence: {line}"
        );
        assert!(line.contains('['), "each hop names its edge kind: {line}");
    }
    // In order, f0 → f1 → f2 → f3.
    assert!(hop_lines[0].contains("f0") && hop_lines[0].contains("f1"));
    assert!(hop_lines[2].contains("f2") && hop_lines[2].contains("f3"));
}

#[test]
fn json_mode_emits_one_document_with_denormalized_endpoints() {
    let d = indexed_chain("json", 2);
    let out = path_in(&d, &["f0", "f2", "--json"]);
    assert!(out.status.success());
    let doc = json_of(&out);

    assert_eq!(doc["found"], true);
    assert!(doc["depth_bounded"].is_boolean());
    assert!(doc["node_bounded"].is_boolean());
    assert_eq!(doc["unresolved"], serde_json::Value::Null);

    let hops = doc["hops"].as_array().expect("hops array");
    assert_eq!(hops.len(), 2);
    for hop in hops {
        for end in ["source", "target"] {
            let e = &hop[end];
            for field in ["symbol", "name", "kind", "file", "line", "line_1based"] {
                assert!(
                    !e[field].is_null(),
                    "{end}.{field} must be present — a bare id would force the caller to \
                     run a second command per hop\n{e}"
                );
            }
        }
        assert!(hop["confidence"].is_number());
        assert!(!hop["provenance"].is_null());
        assert!(!hop["resolved_by"].is_null());
    }
}

/// The `--json` single-document contract, on a fixture that CAN go stale.
///
/// Text mode must print the notice (otherwise the fixture proves nothing); `--json` must
/// not, because a notice on stdout would corrupt the one-document guarantee a caller parses.
#[test]
fn json_mode_suppresses_a_staleness_notice_that_text_mode_shows() {
    let d = indexed_stale_chain("quiet", 2);

    let text = stdout_of(&path_in(&d, &["f0", "f2"]));
    assert!(
        text.contains("STALENESS") || text.to_lowercase().contains("stale"),
        "precondition: this fixture must actually be stale, or the --json half proves \
         nothing:\n{text}"
    );

    let out = path_in(&d, &["f0", "f2", "--json"]);
    let s = stdout_of(&out);
    assert!(
        !s.contains("STALENESS") && !s.to_lowercase().contains("stale"),
        "a notice on stdout would corrupt the single-document contract:\n{s}"
    );
    let _ = json_of(&out); // and it still parses as exactly one document
}

// ── honesty on absence ───────────────────────────────────────────────────────

#[test]
fn unresolvable_operand_is_distinguishable_from_a_proven_absence() {
    let d = indexed_chain("unres", 2);

    let missing = json_of(&path_in(&d, &["nope_not_here", "f2", "--json"]));
    assert_eq!(missing["found"], false);
    assert_eq!(
        missing["unresolved"], "from",
        "an unresolvable input must not look like a proven absence (R3)"
    );

    // f2 is the chain's tail, so nothing is reachable from it — a real absence.
    let real = json_of(&path_in(&d, &["f2", "f0", "--json"]));
    assert_eq!(real["found"], false);
    assert_eq!(real["unresolved"], serde_json::Value::Null);
    assert_ne!(
        missing.to_string(),
        real.to_string(),
        "the two cases must not serialize identically"
    );
}

#[test]
fn text_mode_states_the_depth_bound_when_the_walk_is_cut_off() {
    let d = indexed_chain("bound", 6);
    let out = path_in(&d, &["f0", "f6", "--max-depth", "2"]);
    let s = stdout_of(&out);
    assert!(s.contains("no path found"), "{s}");
    assert!(
        s.contains("depth frontier"),
        "text mode must say the absence is bounded, not proven:\n{s}"
    );
}

/// #230: a FOUND route prints no bound line — the bound lines are for the `no path found`
/// reader; and the exact cause means a route found AT the bound with nothing beyond it is not
/// bounded at all.
#[test]
fn a_found_route_prints_no_bound_line() {
    let d = indexed_chain("found_nobound", 6);
    let out = path_in(&d, &["f0", "f2", "--max-depth", "2"]);
    let s = stdout_of(&out);
    assert!(s.contains("2 hop(s)"), "{s}");
    assert!(
        !s.contains("depth frontier") && !s.contains("longer route"),
        "a found route is not an absence; no bound line:\n{s}"
    );
    let doc = json_of(&path_in(&d, &["f0", "f2", "--max-depth", "2", "--json"]));
    assert_eq!(doc["found"], serde_json::json!(true), "{doc}");
    assert_eq!(
        doc["depth_bounded"],
        serde_json::json!(true),
        "f3..f6 lie beyond depth 2, so the JSON flag stays honest: {doc}"
    );
}

/// #230: the same operand twice is a zero-hop route, worded as a shared CANDIDATE (names are
/// not unique), with no bound line.
#[test]
fn same_operand_twice_is_a_zero_hop_candidate_not_the_same_symbol() {
    let d = indexed_chain("same_operand", 3);
    let out = path_in(&d, &["f1", "f1", "--max-depth", "1"]);
    let s = stdout_of(&out);
    assert!(
        s.contains("resolve to a candidate for both endpoints") && s.contains("zero hops"),
        "{s}"
    );
    assert!(!s.contains("the same symbol"), "{s}");
    assert!(
        !s.contains("depth frontier"),
        "a zero-hop route walked nothing:\n{s}"
    );
}

// ── the argument contract ────────────────────────────────────────────────────

#[test]
fn flags_are_consumed_before_operands_in_either_position() {
    let d = indexed_chain("flagpos", 4);
    for args in [
        vec!["--max-depth", "4", "f0", "f4"],
        vec!["f0", "f4", "--max-depth", "4"],
    ] {
        let out = path_in(&d, &args);
        assert!(
            out.status.success(),
            "{args:?} must resolve from=f0 to=f4 depth=4: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(stdout_of(&out).contains("4 hop(s)"), "{args:?}");
    }
}

#[test]
fn one_operand_plus_a_flag_is_a_usage_error() {
    let d = indexed_chain("arity1", 2);
    // `--json` is consumed as a flag, leaving one operand — this must NOT resolve
    // `to` to "--json".
    let out = path_in(&d, &["f0", "--json"]);
    assert!(!out.status.success(), "expected non-zero exit");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("<from>") && err.contains("<to>"), "{err}");
}

#[test]
fn missing_operands_are_usage_errors() {
    let d = indexed_chain("arity0", 2);
    for args in [vec![], vec!["f0"]] {
        let out = path_in(&d, &args);
        assert!(!out.status.success(), "{args:?} must exit non-zero");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("<from>") && err.contains("<to>"),
            "{args:?}: {err}"
        );
    }
}

#[test]
fn an_unknown_double_dash_token_is_rejected_not_taken_as_an_operand() {
    let d = indexed_chain("unknownflag", 2);
    let out = path_in(&d, &["--depth", "2", "f0", "f2"]);
    assert!(
        !out.status.success(),
        "an unrecognised --flag must not be resolved as a symbol name"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown flag"));
}

/// `--max-depth` as the final token, with no value. Without this the branch could be
/// mutated to ignore the flag and silently run at the default depth on a user typo.
#[test]
fn max_depth_without_a_value_is_a_usage_error() {
    let d = indexed_chain("noval", 2);
    let out = path_in(&d, &["f0", "f2", "--max-depth"]);
    assert!(
        !out.status.success(),
        "a flag with no value must exit non-zero"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("requires a value"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn max_depth_rejects_out_of_range_values() {
    let d = indexed_chain("range", 3);
    for bad in ["0", "abc"] {
        let out = path_in(&d, &["f0", "f3", "--max-depth", bad]);
        assert!(
            !out.status.success(),
            "--max-depth {bad} must exit non-zero"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("1..=16"),
            "the error must name the accepted range"
        );
    }
}

/// The clamp must be OBSERVABLE. A short chain found at `--max-depth 17` proves nothing —
/// it is found whether 17 clamps to 16 or passes through raw. A 20-hop chain must NOT be
/// found, which holds only if the clamp applies; and a 16-hop chain must still be found,
/// so the ceiling is reachable rather than merely low.
#[test]
fn max_depth_above_the_ceiling_clamps_observably() {
    let deep = indexed_chain("clamp20", 20);
    let out = path_in(&deep, &["f0", "f20", "--max-depth", "17"]);
    assert!(out.status.success(), "17 must clamp, not error");
    assert!(
        stdout_of(&out).contains("no path found"),
        "clamped to 16, a 20-hop chain is out of reach:\n{}",
        stdout_of(&out)
    );

    let at_ceiling = indexed_chain("clamp16", 16);
    let ok = path_in(&at_ceiling, &["f0", "f16", "--max-depth", "17"]);
    assert!(
        stdout_of(&ok).contains("16 hop(s)"),
        "the ceiling itself must be reachable:\n{}",
        stdout_of(&ok)
    );
}

/// The range criterion's lower bound. The not-found half has no observation at N=1,
/// because `--max-depth 0` is a usage error rather than a search.
#[test]
fn max_depth_one_finds_a_single_hop() {
    let d = indexed_chain("n1", 2);
    assert!(
        stdout_of(&path_in(&d, &["f0", "f1", "--max-depth", "1"])).contains("1 hop(s)"),
        "N=1 must find a 1-hop route"
    );
    assert!(
        stdout_of(&path_in(&d, &["f0", "f2", "--max-depth", "1"])).contains("no path found"),
        "and must not reach the 2-hop target"
    );
}

#[test]
fn default_depth_is_twelve() {
    // A 12-hop chain is found by default; a 13-hop chain is not.
    let ok = indexed_chain("def12", 12);
    let out = path_in(&ok, &["f0", "f12"]);
    assert!(stdout_of(&out).contains("12 hop(s)"), "{}", stdout_of(&out));

    let too_deep = indexed_chain("def13", 13);
    let out = path_in(&too_deep, &["f0", "f13"]);
    assert!(
        stdout_of(&out).contains("no path found"),
        "a 13-hop chain must exceed the default depth of 12:\n{}",
        stdout_of(&out)
    );
}

#[test]
fn help_lists_path_with_its_flags() {
    let out = Command::new(bin()).arg("--help").output().expect("spawn");
    let s = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let line = s
        .lines()
        .find(|l| l.contains("wicked-estate path"))
        .unwrap_or_else(|| panic!("--help must list `path`:\n{s}"));
    assert!(line.contains("--max-depth"), "{line}");
    assert!(line.contains("--json"), "{line}");
}
