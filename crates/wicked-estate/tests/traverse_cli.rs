//! `wicked-estate traverse` — the first RetrievalTool→CLI bridge command — through the real
//! binary.
//!
//! These test the BRIDGE: strict flags, operand resolution, the stdout/stderr split, exactly
//! one document under `--json`, clamp reporting. They deliberately do not pin
//! `TraverseGraph`'s payload shape — that envelope is owned by the tool and is being reworked
//! by the TypeScript-semantics wave; a snapshot here would break under it for no bridge reason.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

/// Owns a scratch directory and removes it on drop.
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

/// A git repo with a Rust call chain `f0 → f1 → … → f{depth}`, indexed into `<dir>/graph.db`.
fn indexed_chain(tag: &str, depth: usize) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_travcli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let scratch = Scratch(d);
    fs::create_dir_all(scratch.join("src")).unwrap();
    let mut src = String::new();
    for i in 0..=depth {
        if i == depth {
            src.push_str(&format!("fn f{i}() {{}}\n"));
        } else {
            src.push_str(&format!("fn f{i}() {{ f{}(); }}\n", i + 1));
        }
    }
    fs::write(scratch.join("src/a.rs"), src).unwrap();
    git(&scratch, &["init", "-q", "."]);
    git(&scratch, &["add", "-A"]);
    git(&scratch, &["commit", "-qm", "fx"]);
    let out = run(&scratch, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "index failed: {}", stderr(&out));
    scratch
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

fn traverse(dir: &Path, args: &[&str]) -> Output {
    let mut full = vec!["traverse"];
    full.extend_from_slice(args);
    full.extend_from_slice(&["--db", "graph.db"]);
    run(dir, &full)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Every JSON document on stdout. The `--json` contract is exactly one.
fn documents(o: &Output) -> Vec<serde_json::Value> {
    serde_json::Deserializer::from_slice(&o.stdout)
        .into_iter::<serde_json::Value>()
        .collect::<Result<_, _>>()
        .unwrap_or_else(|e| panic!("stdout is not clean JSON ({e}): {}", stdout(o)))
}

#[test]
fn json_is_exactly_one_document_and_diagnostics_go_to_stderr() {
    let fx = indexed_chain("json", 3);
    let out = traverse(&fx, &["f0", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let docs = documents(&out);
    assert_eq!(docs.len(), 1, "stdout: {}", stdout(&out));
    assert!(docs[0].is_object());
    // The tool always emits at least its staleness diagnostic; it must be on stderr, not in
    // the document.
    let err = stderr(&out);
    assert!(err.contains("STALENESS"), "stderr: {err}");
    assert!(!stdout(&out).contains("STALENESS"));
}

#[test]
fn name_operand_resolves_to_the_symbol_id_the_tool_needs() {
    // TraverseGraph keys on a SymbolId; handed the bare name it would return an empty set.
    let fx = indexed_chain("resolve", 3);
    let out = traverse(&fx, &["f0", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let doc = documents(&out).remove(0).to_string();
    for reached in ["f1", "f2", "f3"] {
        assert!(doc.contains(reached), "{reached} not reached: {doc}");
    }
    let out = traverse(&fx, &["no_such_fn"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no symbol named \"no_such_fn\""));
}

#[test]
fn unknown_and_foreign_flags_exit_non_zero_with_nothing_on_stdout() {
    let fx = indexed_chain("flags", 2);
    for bad in [
        &["f0", "--bogus-flag", "x"][..],
        // Owned by other commands; main's shared parser would swallow it silently.
        &["f0", "--top", "5"][..],
        &["f0", "--depth", "four"][..],
        &["f0", "--direction", "sideways"][..],
        &["f0", "--edge-kinds", "cals"][..],
    ] {
        let out = traverse(&fx, bad);
        assert!(!out.status.success(), "{bad:?} exited 0");
        assert!(
            out.stdout.is_empty(),
            "{bad:?} wrote stdout: {}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("usage: wicked-estate traverse"),
            "{bad:?}"
        );
    }
}

#[test]
fn depth_above_the_ceiling_reports_the_clamp() {
    let fx = indexed_chain("clamp", 2);
    let out = traverse(&fx, &["f0", "--depth", "99", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(documents(&out).len(), 1);
    assert!(
        stderr(&out).contains("CLAMPED: depth=99"),
        "stderr: {}",
        stderr(&out)
    );
    // Human mode: diagnostics ride stdout after the rendered content.
    let out = traverse(&fx, &["f0", "--depth", "99"]);
    assert!(
        stdout(&out).contains("CLAMPED: depth=99"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn real_staleness_is_reported_on_stderr_under_json() {
    // The tool's own staleness line is a placeholder for the transport; the bridge computes
    // the real commits-behind count as the MCP server does (wicked-estate#198 for this surface).
    let fx = indexed_chain("stale", 2);
    // `commits_behind` compares at whole-second resolution against the db mtime; a commit in
    // the same second as the index can count either way. Step past it, and assert presence
    // rather than an exact count (the precedent in path_cli.rs).
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(fx.join("src/b.rs"), "fn g() {}\n").unwrap();
    git(&fx, &["add", "-A"]);
    git(&fx, &["commit", "-qm", "more"]);
    let out = traverse(&fx, &["f0", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(documents(&out).len(), 1);
    // The tool's placeholder line never says "commit(s) since last index"; only the bridge's
    // computed one does.
    assert!(
        stderr(&out).contains("commit(s) since last index"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(!stdout(&out).contains("STALENESS"));
}

#[test]
fn ambiguous_name_exits_non_zero_listing_candidates() {
    let fx = indexed_chain("ambig", 1);
    // A second `f0` in another file: the name now matches two symbols.
    fs::write(fx.join("src/b.rs"), "fn f0() {}\n").unwrap();
    let out = run(&fx, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = traverse(&fx, &["f0", "--json"]);
    assert!(!out.status.success(), "ambiguous name must not pick one");
    assert!(out.stdout.is_empty(), "stdout: {}", stdout(&out));
    let err = stderr(&out);
    assert!(err.contains("names 2 symbols"), "{err}");
    assert!(
        err.contains("src/a/f0") && err.contains("src/b/f0"),
        "{err}"
    );
}

#[test]
fn symbol_id_operand_disambiguates() {
    // The ids come from the bridge's own ambiguity error, not from TraverseGraph's payload,
    // whose layout this file does not pin.
    let fx = indexed_chain("byid", 1);
    fs::write(fx.join("src/b.rs"), "fn f0() {}\n").unwrap();
    let out = run(&fx, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&traverse(&fx, &["f0"]));
    let ids: Vec<&str> = err.lines().filter_map(|l| l.strip_prefix("  ")).collect();
    assert_eq!(ids.len(), 2, "{err}");
    for id in ids {
        let out = traverse(&fx, &[id, "--json"]);
        assert!(out.status.success(), "{id}: {}", stderr(&out));
        assert_eq!(documents(&out).len(), 1);
    }
}

#[test]
fn help_comes_from_the_tool_and_the_banner_lists_the_command() {
    let fx = indexed_chain("help", 1);
    let out = run(&fx, &["traverse", "--help"]);
    assert!(out.status.success());
    let help = stdout(&out);
    assert!(help.contains("Bounded multi-hop walk"), "{help}");
    assert!(help.contains("--edge-kinds"));

    let banner = stdout(&run(&fx, &["help"]));
    assert!(
        banner.contains("wicked-estate traverse <symbol>"),
        "{banner}"
    );
}
