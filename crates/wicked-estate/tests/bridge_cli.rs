//! The RetrievalTool→CLI bridge commands (`traverse`, `rank`/`hotspots`, `rules-inventory`,
//! `rules-recall`) through the real binary.
//!
//! These test the BRIDGE: strict flags, operand and `--seeds` resolution, aliases, the
//! stdout/stderr split, exactly one document under `--json`, clamp reporting. They
//! deliberately do not pin `TraverseGraph`'s payload shape — that envelope is owned by the tool
//! and is being reworked by the TypeScript-semantics wave; a snapshot here would break under it
//! for no bridge reason. `rank` reads only `RankHotspots`' row array, outside that rework.
//!
//! The bespoke arms get the same strictness from `cli_flags` (#197, #206); the tests at the end
//! pin it for `nodes` and `source`, the two commands those issues reproduced on.

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

/// An empty scratch directory: no repo, no graph.
fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_travcli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

/// A `--db` that is not there must fail CLOSED, as `lineage` does (#241): opening a missing
/// SQLite path creates an empty graph, and an empty graph answers `rank` with an empty ranking,
/// exit 0 — a typo reads as "this repo has no hotspots". A bare path and `sqlite://<path>` are
/// both file specs, and a zero-length file is not a graph either.
#[test]
fn a_missing_graph_fails_closed_for_bridged_commands() {
    let s = scratch("missing_db");
    for spec in ["typo.db", "sqlite://typo.db"] {
        for args in [
            vec!["rank", "--json", "--db", spec],
            vec!["hotspots", "--limit", "2", "--db", spec],
            vec!["traverse", "f0", "--json", "--db", spec],
            vec!["rules-inventory", "--json", "--db", spec],
            vec!["rules-recall", "--severity", "error", "--db", spec],
        ] {
            let out = run(&s, &args);
            assert!(!out.status.success(), "{args:?} must fail");
            assert!(
                out.stdout.is_empty(),
                "{args:?} printed to stdout: {}",
                stdout(&out)
            );
            let err = stderr(&out);
            assert!(
                err.contains(&format!("no graph at {spec}")),
                "{args:?}: {err}"
            );
            assert!(
                !s.join("typo.db").exists(),
                "{args:?} must never create a graph"
            );
        }
    }
    fs::write(s.join("empty.db"), b"").unwrap();
    for args in [
        ["rank", "--json", "--db", "empty.db"],
        ["traverse", "f0", "--db", "empty.db"],
    ] {
        let out = run(&s, &args);
        assert!(
            !out.status.success(),
            "{args:?}: a zero-length file must fail"
        );
        assert!(out.stdout.is_empty(), "{args:?}: {}", stdout(&out));
        assert!(stderr(&out).contains("no graph at empty.db"), "{args:?}");
        assert_eq!(
            fs::metadata(s.join("empty.db")).unwrap().len(),
            0,
            "{args:?} grew the zero-length file into an empty graph"
        );
    }
}

// ── rules-inventory / rules-recall (#196) ────────────────────────────────────────────────────

/// A committed repo with one Python file and one Drools package (a `RuleSet` + two `Rule` nodes
/// through the real `index` path), plus three conformance rules seeded straight into the graph:
/// those are minted only by `wicked-core rules ingest`, which this repo does not ship.
fn indexed_rules(tag: &str) -> Scratch {
    let s = scratch(tag);
    fs::write(s.join("alpha.py"), "def f(p): return p\n").unwrap();
    fs::write(
        s.join("lending.drl"),
        "package com.example.lending;\nrule \"CheckScore\"\n  when\n    $c : Customer( score >= 700 )\n  then\n    $c.approve();\nend\n",
    )
    .unwrap();
    git(&s, &["init", "-q", "."]);
    git(&s, &["add", "-A"]);
    git(&s, &["commit", "-qm", "fx"]);
    index(&s);

    use wicked_estate_core::{Language, Location, Node, NodeKind, Span, Symbol};
    let conformance = |id: &str, severity: &str, language: Option<&str>| {
        let path = format!("conformance_rule/{id}");
        let mut node = Node::new(
            Symbol::synthetic("wicked-apps", path.clone()).id(),
            NodeKind::Rule,
            id,
            Language::new("wicked-apps"),
            Location::new(path, Span::ZERO),
        );
        let targets = match language {
            Some(l) => serde_json::json!({ "language": l }),
            None => serde_json::json!({}),
        };
        let serde_json::Value::Object(meta) = serde_json::json!({
            "id": id, "rule_type": "pattern", "severity": severity, "targets": targets,
        }) else {
            unreachable!()
        };
        node.metadata = meta;
        node
    };
    let mut store = wicked_estate_store::open_store(s.join("graph.db").to_str().unwrap()).unwrap();
    store.begin_batch().unwrap();
    store
        .upsert_nodes(&[
            conformance("PAT-1", "error", Some("python")),
            conformance("PAT-3", "error", Some("java")),
            conformance("POL-2", "warn", None),
        ])
        .unwrap();
    store.commit_batch().unwrap();
    s
}

/// The tool's own `content` for `request` against the fixture's graph — what MCP returns.
fn direct(
    dir: &Path,
    tool: &dyn wicked_estate_core::RetrievalTool,
    request: serde_json::Value,
) -> serde_json::Value {
    let store = wicked_estate_store::open_store(dir.join("graph.db").to_str().unwrap()).unwrap();
    tool.invoke(&*store, &request).unwrap().content
}

fn rule_ids(doc: &serde_json::Value) -> Vec<&str> {
    doc["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect()
}

#[test]
fn rules_inventory_json_is_the_tool_document_unchanged() {
    let fx = indexed_rules("rules_inv");
    let out = run(&fx, &["rules-inventory", "--json", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let docs = documents(&out);
    assert_eq!(docs.len(), 1, "stdout: {}", stdout(&out));
    assert_eq!(
        docs[0],
        direct(
            &fx,
            &wicked_estate_retrieve::RulesInventory,
            serde_json::json!({})
        )
    );
    // Populated, not vacuously equal: the Drools package and its rule, plus the seeded rules.
    assert_eq!(docs[0]["engines"][0]["name"], "com.example.lending");
    assert_eq!(docs[0]["rule_nodes"]["total"], 4);
    assert!(stderr(&out).contains("STALENESS:"), "{}", stderr(&out));
}

#[test]
fn rules_recall_json_is_the_tool_document_and_str_facets_reach_the_tool() {
    let fx = indexed_rules("rules_recall");
    let recall = |args: &[&str]| {
        let mut full = vec!["rules-recall"];
        full.extend_from_slice(args);
        full.extend_from_slice(&["--json", "--db", "graph.db"]);
        let out = run(&fx, &full);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        let mut docs = documents(&out);
        assert_eq!(docs.len(), 1, "{args:?} stdout: {}", stdout(&out));
        docs.remove(0)
    };

    let doc = recall(&["--severity", "error", "--language", "python"]);
    assert_eq!(
        doc,
        direct(
            &fx,
            &wicked_estate_retrieve::RulesRecall,
            serde_json::json!({ "severity": "error", "language": "python" })
        )
    );
    // Exact severity drops POL-2 (warn); the language facet drops PAT-3 (java). Were either
    // flag lost or mistyped on the way, the tool would read it as absent and return more.
    assert_eq!(rule_ids(&doc), ["PAT-1"]);

    // `language` is a wildcard facet: POL-2 names no language, so it applies to python too.
    // Severity-first order: error before warn.
    assert_eq!(
        rule_ids(&recall(&["--language=python"])),
        ["PAT-1", "POL-2"]
    );
    // U64 reaches the tool as a number: a string would fall back to the default cap of 100.
    assert_eq!(rule_ids(&recall(&["--limit", "1"])), ["PAT-1"]);
    // FND-EST-02: `--steering-type` reaches the tool. These rules predate steering types, so
    // they are `architecture` (core's default) and no other page.
    assert_eq!(
        rule_ids(&recall(&["--steering-type", "architecture"])),
        rule_ids(&recall(&[]))
    );
    assert!(rule_ids(&recall(&["--steering-type", "security"])).is_empty());
}

#[test]
fn rules_commands_on_a_graph_without_rules_are_empty_not_errors() {
    // R1: an empty graph is an empty result and exit 0, never an error.
    let fx = indexed_chain("rules_empty", 1);
    let out = run(&fx, &["rules-inventory", "--json", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let doc = documents(&out).remove(0);
    assert_eq!(doc["total"], 0);
    assert_eq!(doc["rule_nodes"]["total"], 0);

    let out = run(&fx, &["rules-recall", "--json", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        documents(&out),
        [serde_json::json!({"returned": 0, "rules": [], "total": 0})]
    );
    assert!(stderr(&out).contains("no active conformance rules matched"));
}

#[test]
fn rules_commands_reject_unknown_foreign_and_malformed_flags() {
    let fx = indexed_chain("rules_flags", 1);
    for bad in [
        &["rules-inventory", "--bogus", "x"][..],
        &["rules-inventory", "--limit", "5"][..],
        &["rules-inventory", "positional"][..],
        &["rules-recall", "--top", "5"][..],
        &["rules-recall", "--language", ""][..],
        &["rules-recall", "--language="][..],
        &["rules-recall", "--severity"][..],
        &["rules-recall", "--layer", "a", "--layer", "b"][..],
        &["rules-recall", "--limit", "many"][..],
        &["rules-recall", "--projects", ","][..],
    ] {
        let mut full = bad.to_vec();
        full.extend_from_slice(&["--db", "graph.db"]);
        let out = run(&fx, &full);
        assert!(!out.status.success(), "{bad:?} exited 0");
        assert!(
            out.stdout.is_empty(),
            "{bad:?} wrote stdout: {}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains(&format!("usage: wicked-estate {}", bad[0])),
            "{bad:?}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn rules_commands_have_help_and_appear_in_the_banner() {
    let fx = scratch("rules_help");
    let help = stdout(&run(&fx, &["rules-recall", "--help"]));
    assert!(help.contains("wildcard facets"), "{help}");
    for flag in [
        "--severity",
        "--rule-type",
        "--language",
        "--layer",
        "--framework",
        "--scope",
        "--projects",
        "--limit",
    ] {
        assert!(help.contains(flag), "{flag} missing: {help}");
    }
    let banner = stdout(&run(&fx, &["help"]));
    assert!(
        banner.contains("wicked-estate rules-inventory [--json]"),
        "{banner}"
    );
    assert!(
        banner.contains("wicked-estate rules-recall [--severity S]"),
        "{banner}"
    );
}

// ── Strict flags on the bespoke arms (#197, #206) ────────────────────────────────────────────

/// Two Python functions sharing one name — the #206 fixture: a name alone cannot pin either.
fn duplicate_names(tag: &str) -> Scratch {
    let s = scratch(tag);
    for (file, root) in [("alpha.py", "alpha"), ("beta.py", "beta")] {
        fs::write(
            s.join(file),
            format!(
                "def validate_confined_directory(path):\n    return path.startswith(\"/safe/{root}\")\n"
            ),
        )
        .unwrap();
    }
    git(&s, &["init", "-q", "."]);
    git(&s, &["add", "-A"]);
    git(&s, &["commit", "-qm", "fx"]);
    index(&s);
    s
}

fn symbol_id_in(dir: &Path, file: &str) -> String {
    let out = run(
        dir,
        &[
            "resolve",
            "validate_confined_directory",
            "--json",
            "--db",
            "graph.db",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    documents(&out)
        .remove(0)
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["file"] == file)
        .unwrap_or_else(|| panic!("no match in {file}: {}", stdout(&out)))["symbol_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn assert_usage_error(out: &Output, cmd: &str, why: &str) {
    assert!(!out.status.success(), "exited 0: {}", stdout(out));
    assert!(out.stdout.is_empty(), "wrote stdout: {}", stdout(out));
    let err = stderr(out);
    assert!(
        err.contains(&format!("usage: wicked-estate {cmd}")),
        "{err}"
    );
    assert!(err.contains(why), "{err}");
}

#[test]
fn nodes_rejects_flags_it_does_not_read_197() {
    let fx = duplicate_names("strict_nodes");
    let id = symbol_id_in(&fx, "alpha.py");
    // Before: every node in the graph, exit 0.
    let out = run(&fx, &["nodes", "--bogus-flag", "zzz", "--db", "graph.db"]);
    assert_usage_error(&out, "nodes", "unknown flag \"--bogus-flag\"");
    // `--symbol` is a real flag, of `annotate`/`annotations`; `nodes` never filtered by it.
    let out = run(&fx, &["nodes", "--symbol", &id, "--db", "graph.db"]);
    assert_usage_error(&out, "nodes", "accepted by: annotate, annotations");
    // Its own flags still work.
    let out = run(
        &fx,
        &["nodes", "--kind", "Function", "--json", "--db", "graph.db"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(documents(&out).remove(0).as_array().unwrap().len(), 2);
}

/// `--help`/`-h` wins only where a flag is read. In a value slot it is a refused value: before,
/// any `--help` token anywhere skipped the check, the shared parser took it as the value, and
/// `nodes --db --help --bogus-flag zzz` ran on a store literally named `--help` with the bogus
/// flag ignored — the accept-and-ignore path #197 closes, open for that one token.
#[test]
fn help_in_a_value_slot_is_a_value_not_a_help_request_197() {
    let fx = duplicate_names("strict_help");
    // Before: exit 0, "0 node(s)", a 200 KB empty store created at `<fx>/--help`.
    let out = run(&fx, &["nodes", "--db", "--help", "--bogus-flag", "zzz"]);
    assert_usage_error(
        &out,
        "nodes",
        "--db requires a value, got the flag \"--help\"",
    );
    assert!(
        !fx.join("--help").exists(),
        "a store named `--help` was created"
    );
    // Before: exit 0, `k=--help` written to both symbols, the bogus flag ignored.
    let before = run(
        &fx,
        &[
            "annotations",
            "validate_confined_directory",
            "--json",
            "--db",
            "graph.db",
        ],
    );
    assert!(before.status.success(), "{}", stderr(&before));
    let out = run(
        &fx,
        &[
            "annotate",
            "validate_confined_directory",
            "--key",
            "k",
            "--value",
            "--help",
            "--bogus-flag",
            "zzz",
            "--db",
            "graph.db",
        ],
    );
    assert_usage_error(
        &out,
        "annotate",
        "--value requires a value, got the flag \"--help\"",
    );
    let after = run(
        &fx,
        &[
            "annotations",
            "validate_confined_directory",
            "--json",
            "--db",
            "graph.db",
        ],
    );
    assert_eq!(stdout(&before), stdout(&after), "annotation written");
    // Control: in flag position help still wins, even over an earlier unknown flag.
    let out = run(&fx, &["nodes", "--bogus", "--help", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("wicked-estate nodes [--kind K]"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn source_text_honours_the_selector_instead_of_dropping_it_206() {
    let fx = duplicate_names("strict_source");
    let id = symbol_id_in(&fx, "alpha.py");

    // Before: "2 match(es)", both bodies — the selector that pinned one was dropped.
    let out = run(
        &fx,
        &[
            "source",
            "validate_confined_directory",
            "--symbols",
            &id,
            "--db",
            "graph.db",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("1 match(es) for --symbols "), "{text}");
    assert!(
        text.contains("/safe/alpha") && !text.contains("/safe/beta"),
        "{text}"
    );

    // The JSON path resolves the same selector to the same single node.
    let out = run(
        &fx,
        &[
            "source",
            "validate_confined_directory",
            "--symbols",
            &id,
            "--json",
            "--db",
            "graph.db",
        ],
    );
    let bundle = documents(&out).remove(0);
    assert_eq!(bundle["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(bundle["nodes"][0]["symbol_id"], id.as_str());

    // `--file` too, and `--signatures-only` drops the body in text mode as it does in JSON.
    let out = run(
        &fx,
        &[
            "source",
            "--file",
            "beta.py",
            "--signatures-only",
            "--db",
            "graph.db",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("match(es) for --file beta.py:"), "{text}");
    assert!(
        !text.contains("startswith"),
        "body printed under --signatures-only: {text}"
    );

    // A bare <name> keeps its legacy output.
    let out = run(
        &fx,
        &["source", "validate_confined_directory", "--db", "graph.db"],
    );
    let text = stdout(&out);
    assert!(
        text.starts_with("2 match(es) for 'validate_confined_directory':"),
        "{text}"
    );
}

#[test]
fn source_rejects_what_its_text_path_cannot_honour_206() {
    let fx = duplicate_names("strict_source_rej");
    for (args, why) in [
        (
            &[
                "source",
                "validate_confined_directory",
                "--max-total-chars",
                "5",
            ][..],
            "max-total-chars applies only with --json",
        ),
        (&["source", "--file=alpha.py"][..], "write --file <value>"),
        (
            &["source", "validate_confined_directory", "--top", "1"][..],
            "unknown flag \"--top\"",
        ),
        (
            &["source", "validate_confined_directory", "--symbols"][..],
            "--symbols requires a value, got the flag \"--db\"",
        ),
    ] {
        let mut full = args.to_vec();
        full.extend_from_slice(&["--db", "graph.db"]);
        assert_usage_error(&run(&fx, &full), "source", why);
    }
}

/// `source` with no selector and no `<name>` is a usage error, raised before `--db` is opened:
/// opening a missing SQLite path creates an empty store, so `source --db typo2.db` used to exit 1
/// AND leave `typo2.db` behind. The `--max-*-chars` guard above the open shows the right order.
#[test]
fn source_usage_error_does_not_create_the_store_206() {
    let s = scratch("strict_source_nodb");
    let out = run(&s, &["source", "--db", "typo2.db"]);
    assert_usage_error(&out, "source", "usage: wicked-estate source");
    assert!(
        !s.join("typo2.db").exists(),
        "typo2.db created by a usage error"
    );
}

/// W8.5: `traverse` addresses a value-flow node by its exact `SymbolId` — a value slot that
/// name search hides — and walks its `flows_to` neighbourhood like any other node.
#[test]
fn traverse_accepts_a_value_flow_node_by_exact_symbol_id() {
    let d = std::env::temp_dir().join(format!("ci_travcli_valueid_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let scratch = Scratch(d);
    fs::create_dir_all(&*scratch).unwrap();
    fs::write(
        scratch.join("v.ts"),
        "export function f(a: string): string {\n  const b = a;\n  return b;\n}\n",
    )
    .unwrap();
    index(&scratch);
    let store = wicked_estate_store::SqliteStore::open(scratch.join("graph.db")).unwrap();
    let slot = |suffix: &str| {
        use wicked_estate_core::GraphRead;
        let hits: Vec<_> = store
            .all_nodes()
            .unwrap()
            .into_iter()
            .filter(|n| n.is_value_flow_node() && n.symbol.0.ends_with(suffix))
            .collect();
        assert_eq!(hits.len(), 1, "{suffix}: {hits:?}");
        hits[0].symbol.0.clone()
    };
    let (a, b) = (slot("f().:param:a:"), slot("f().:local:b:"));
    let out = traverse(&scratch, &[&a, "--direction", "dependents", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let docs = documents(&out);
    assert_eq!(docs.len(), 1);
    assert!(
        docs[0].to_string().contains(&b),
        "the slot's flows_to consumer must be reached: {}",
        docs[0]
    );
}
