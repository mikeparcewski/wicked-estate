//! The RetrievalTool→CLI bridge commands (`traverse`, `rank`/`hotspots`) through the real
//! binary.
//!
//! These test the BRIDGE: strict flags, operand and `--seeds` resolution, aliases, the
//! stdout/stderr split, exactly one document under `--json`, clamp reporting. They
//! deliberately do not pin `TraverseGraph`'s payload shape — that envelope is owned by the tool
//! and is being reworked by the TypeScript-semantics wave; a snapshot here would break under it
//! for no bridge reason. `rank` reads only `RankHotspots`' row array, outside that rework.

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
    let scratch = chain_repo(tag, depth);
    index(&scratch);
    scratch
}

fn index(dir: &Path) {
    let out = run(dir, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "index failed: {}", stderr(&out));
}

/// [`indexed_chain`] without the index: the committed git repo alone.
fn chain_repo(tag: &str, depth: usize) -> Scratch {
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
    scratch
}

/// Git, hermetically: a developer's `commit.gpgsign` or hooks must not reach the fixture, and
/// repo discovery stops at the scratch dir's parent.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
}

/// The binary, run inside `dir`, hermetically (as `lineage_cli.rs` does): the `index` event
/// emitter points at a missing program and a spool inside `dir` (no `wicked-bus`, no write to
/// `$HOME`); the store-selection and telemetry variables the binary reads are cleared, so a
/// developer's `WICKED_RUNTIME=team` cannot fail every command; and git discovery stops at
/// `dir`'s parent, so a scratch dir under a checkout cannot change the freshness verdict.
fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(dir)
        .args(args)
        .env("WICKED_ESTATE_EMIT_PROGRAM", "wicked-bus-absent-bridge-cli")
        .env("WICKED_ESTATE_EMIT_DEADLETTER", dir.join("emit.ndjson"))
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env_remove("WICKED_OTEL_ENDPOINT")
        .env_remove("WICKED_OTEL_HEADERS")
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
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

fn rank(dir: &Path, args: &[&str]) -> Output {
    let mut full = vec!["rank"];
    full.extend_from_slice(args);
    full.extend_from_slice(&["--db", "graph.db"]);
    run(dir, &full)
}

fn hotspot_rows(o: &Output) -> Vec<serde_json::Value> {
    documents(o).remove(0)["hotspots"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("no hotspots array: {}", stdout(o)))
}

/// wicked-estate#193: `--json` and `--limit` were accepted and silently dropped; the count was
/// fixed at 25 and the output was human text either way.
#[test]
fn rank_honours_json_and_limit() {
    let fx = indexed_chain("rank_limit", 6);
    let out = rank(&fx, &["--json", "--limit", "3"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(documents(&out).len(), 1);
    assert_eq!(hotspot_rows(&out).len(), 3);
    assert!(stderr(&out).contains("STALENESS"), "{}", stderr(&out));

    let out = rank(&fx, &["--limit", "999", "--json"]);
    assert!(
        stderr(&out).contains("CLAMPED: limit=999"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn hotspots_is_the_same_command_as_rank() {
    let fx = indexed_chain("rank_alias", 4);
    let a = rank(&fx, &["--json"]);
    let mut full = vec!["hotspots", "--json", "--db", "graph.db"];
    let b = run(&fx, &full);
    assert!(a.status.success() && b.status.success());
    assert_eq!(a.stdout, b.stdout);
    full.push("--bogus");
    assert!(!run(&fx, &full).status.success(), "the alias is strict too");
}

/// `--seeds` personalizes the ranking — and a seed that resolves to nothing is an error, not
/// a silent fall-back to the global ranking.
#[test]
fn rank_seeds_resolve_and_personalize() {
    let fx = indexed_chain("rank_seeds", 6);
    let global = hotspot_rows(&rank(&fx, &["--json"]));
    let seeded = rank(&fx, &["--json", "--seeds", "f0"]);
    assert!(seeded.status.success(), "{}", stderr(&seeded));
    assert_ne!(
        global,
        hotspot_rows(&seeded),
        "seeds must change the ranking"
    );

    let out = rank(&fx, &["--seeds", "f0,no_such_fn"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        stderr(&out).contains("no symbol named \"no_such_fn\""),
        "{}",
        stderr(&out)
    );
}

#[test]
fn rank_takes_no_operand_and_keeps_its_text_listing() {
    let fx = indexed_chain("rank_text", 3);
    let out = rank(&fx, &["f0"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("takes no positional argument"),
        "{}",
        stderr(&out)
    );

    let out = rank(&fx, &["--limit", "2"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    // Every row renders kind, name and file:line — a renamed tool key would print `? ? (?:null)`.
    let rows: Vec<&str> = text.lines().skip(1).take(2).collect();
    for row in &rows {
        assert!(
            row.contains(" function f") && row.contains("(src/a.rs:"),
            "row lost a field: {row:?}\n{text}"
        );
    }
    assert!(text.starts_with("top 2 symbols by PageRank:\n"), "{text}");
}

/// R5 through the bridge: exactly one freshness statement, never the tool's transport-addressed
/// placeholder — "0 commits" on a current git index, "unknown" without git history.
#[test]
fn staleness_is_stated_once_and_the_placeholder_never_leaks() {
    const PLACEHOLDER: &str = "commits_behind not available at this layer";
    // `commits_behind` counts commits at or after the db mtime's whole second, so a commit in
    // the same second as the index counts as one behind. Index in a LATER second so "current"
    // is exact regardless of timing.
    let fx = chain_repo("fresh", 2);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    index(&fx);
    let out = rank(&fx, &["--json"]);
    let err = stderr(&out);
    assert!(
        err.contains("STALENESS: 0 commits since last index"),
        "{err}"
    );
    assert!(!err.contains(PLACEHOLDER), "{err}");
    assert_eq!(err.matches("STALENESS").count(), 1, "{err}");

    // No git history: freshness is unknown, and says so.
    fs::remove_dir_all(fx.join(".git")).unwrap();
    let out = traverse(&fx, &["f0"]);
    let text = stdout(&out);
    assert!(text.contains("STALENESS: unknown"), "{text}");
    assert!(!text.contains(PLACEHOLDER), "{text}");
}

/// A multi-repo graph where one repo has no git history: the bridge must NOT claim the graph is
/// current (R3 — partial coverage presented as complete). It names the unchecked repo instead.
#[test]
fn mixed_git_and_non_git_repos_are_not_reported_current() {
    let fx = chain_repo("mixed", 2);
    // Outside any git work tree — inside `fx` git would answer for it.
    let plain = Scratch(
        std::env::temp_dir().join(format!("ci_travcli_mixed_plain_{}", std::process::id())),
    );
    let _ = fs::remove_dir_all(&*plain);
    fs::create_dir_all(&*plain).unwrap();
    fs::write(plain.join("b.rs"), "fn g() {}\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let plain_path = plain.to_string_lossy().into_owned();
    for (path, label) in [("src", "chain"), (plain_path.as_str(), "plain")] {
        let out = run(&fx, &["index", path, "--repo", label, "--db", "graph.db"]);
        assert!(out.status.success(), "index {label}: {}", stderr(&out));
    }
    let out = rank(&fx, &["--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(!err.contains("0 commits since last index"), "{err}");
    assert!(err.contains("STALENESS: unknown for repo 'plain'"), "{err}");
}
