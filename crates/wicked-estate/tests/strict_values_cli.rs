//! W8.6: the bespoke CLI arms reject malformed values, surplus or missing operands, and
//! accidental repeats, through the real binary.
//!
//! #197/#206 closed the *flag* half of accept-and-ignore (`cli_flags::check`). These pin the
//! two siblings that remained: a malformed value that fell back to a default (`--top abc` → 20,
//! `--cluster abc` → no selector) and an operand or repeat the arm never read (`stats foo`,
//! `--kind A --kind B` → last wins). Every invalid case must exit non-zero with usage and the
//! offending field on stderr, write nothing to stdout, and leave the directory untouched — no
//! store created, none grown — because the contract is checked before any I/O.

use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
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

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_strictvals_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let s = Scratch(d);
    fs::create_dir_all(&*s).unwrap();
    s
}

/// The binary, run inside `dir`, hermetically. Every inherited `WICKED_*` (store selection,
/// runtime profile, event emitter, plugins, OTel), `OTEL_*` and `GIT_*` variable is removed; the
/// emitter points at a missing program with a spool inside `dir`; git discovery stops at `dir`'s
/// parent and reads no global/system config.
fn run(dir: &Path, args: &[&str]) -> Output {
    run_with_env(dir, args, &[])
}

/// [`run`], then `env` set on top of the hermetic environment.
fn run_with_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(bin());
    cmd.current_dir(dir).args(args);
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy().into_owned();
        if k.starts_with("WICKED_") || k.starts_with("OTEL_") || k.starts_with("GIT_") {
            cmd.env_remove(&k);
        }
    }
    cmd.env("WICKED_ESTATE_EMIT_PROGRAM", "wicked-bus-absent-strict-cli")
        .env("WICKED_ESTATE_EMIT_DEADLETTER", dir.join("emit.ndjson"))
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dir.join("no-gitconfig"))
        .envs(env.iter().copied())
        .output()
        .expect("spawn wicked-estate")
}

/// Git, hermetically: no signing, no hooks, discovery stops at the scratch dir's parent.
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
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Every file under `dir` → (length, content hash). Equal snapshots = nothing created or grown.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (u64, u64)> {
    fn walk(d: &Path, out: &mut BTreeMap<PathBuf, (u64, u64)>) {
        for e in fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                let bytes = fs::read(&p).unwrap();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                bytes.hash(&mut h);
                out.insert(p, (bytes.len() as u64, h.finish()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, &mut out);
    out
}

/// `args` must fail before I/O: non-zero, empty stdout, usage + `field` on stderr, and `dir`
/// byte-for-byte unchanged.
fn assert_rejected(dir: &Path, args: &[&str], field: &str) {
    let before = snapshot(dir);
    let out = run(dir, args);
    let err = stderr(&out);
    assert!(!out.status.success(), "{args:?} exited 0: {}", stdout(&out));
    assert!(
        out.stdout.is_empty(),
        "{args:?} wrote stdout: {}",
        stdout(&out)
    );
    assert!(
        err.contains(&format!("usage: wicked-estate {}", args[0])),
        "{args:?}: no usage in {err}"
    );
    assert!(err.contains(field), "{args:?}: {field:?} not in {err}");
    assert_eq!(before, snapshot(dir), "{args:?} touched the directory");
}

/// [`assert_rejected`] with `--db probe.db` appended: a store that does not exist, so creating
/// it would show in the snapshot.
fn rejected_with_db(dir: &Path, args: &[&str], field: &str) {
    let mut full = args.to_vec();
    full.extend_from_slice(&["--db", "probe.db"]);
    assert_rejected(dir, &full, field);
}

// ── (a) malformed values ────────────────────────────────────────────────────────────────────

/// Coercion is one function per type, so the full matrix of malformed forms runs through the
/// binary for one integer flag; every other W8.6 integer flag gets one malformed form each
/// (cycling through the matrix). `cli_flags`' unit tests cover every form for every type.
#[test]
fn malformed_integers_are_rejected_not_defaulted() {
    let s = scratch("ints");
    let huge = "99999999999999999999999999";
    let forms = ["abc", "-1", "", "1.5", "+3", huge];
    for bad in forms {
        rejected_with_db(&s, &["graph-view", "--limit", bad], "--limit");
    }
    let cases: [&[&str]; 9] = [
        &["subscribe", "--since"],
        &["source", "--cluster"],
        &["context", "f", "--budget"],
        &["clusters", "--weight", "semantic", "--k"],
        &["clusters", "--min-pts"],
        &["source", "f", "--json", "--max-total-chars"],
        &["source", "f", "--json", "--max-node-chars"],
        &["blast-radius", "f", "--depth"],
        &["path", "a", "b", "--max-depth"],
    ];
    for (i, case) in cases.iter().enumerate() {
        let mut args = case.to_vec();
        args.push(forms[i % forms.len()]);
        rejected_with_db(&s, &args, case.last().unwrap());
    }
    assert_rejected(
        &s,
        &[
            "correspond",
            "--db-a",
            "a.db",
            "--db-b",
            "b.db",
            "--top",
            "abc",
        ],
        "--top",
    );
    // The inline form is typed the same way.
    rejected_with_db(&s, &["blast-radius", "f", "--depth=abc"], "--depth");
    rejected_with_db(&s, &["blast-radius", "f", "--depth="], "--depth");
    // Overflow names the field and says it is out of range rather than "not a number".
    rejected_with_db(&s, &["graph-view", "--limit", huge], "out of range");
}

#[test]
fn malformed_and_non_finite_floats_are_rejected_not_defaulted() {
    let s = scratch("floats");
    let forms = ["x", "", "NaN", "inf", "-inf", "infinity", "1e999", "-0.5"];
    for bad in forms {
        rejected_with_db(&s, &["clusters", "--resolution", bad], "--resolution");
    }
    let cases: [&[&str]; 4] = [
        &[
            "correspond",
            "--db-a",
            "a.db",
            "--db-b",
            "b.db",
            "--min-score",
        ],
        &[
            "annotate",
            "f",
            "--key",
            "k",
            "--value",
            "v",
            "--confidence",
        ],
        &["clusters", "--package-bias"],
        &["clusters", "--weight", "semantic", "--eps"],
    ];
    for (i, case) in cases.iter().enumerate() {
        let mut args = case.to_vec();
        args.push(forms[(i * 2 + 2) % forms.len()]);
        if case[0] == "correspond" {
            assert_rejected(&s, &args, case.last().unwrap());
        } else {
            rejected_with_db(&s, &args, case.last().unwrap());
        }
    }
    // Documented domains: confidence 0.0–1.0, eps 0.0–2.0.
    rejected_with_db(
        &s,
        &[
            "annotate",
            "f",
            "--key",
            "k",
            "--value",
            "v",
            "--confidence",
            "1.5",
        ],
        "--confidence",
    );
    rejected_with_db(
        &s,
        &["clusters", "--weight", "semantic", "--eps", "2.5"],
        "--eps",
    );
}

#[test]
fn closed_set_values_are_rejected_not_defaulted() {
    let s = scratch("oneof");
    // Anything but `semantic` used to mean graph mode; anything but `json` meant ndjson; anything
    // but true/1/yes meant "not validated".
    rejected_with_db(&s, &["clusters", "--weight", "semantik"], "--weight");
    rejected_with_db(&s, &["export", "--format", "xml"], "--format");
    rejected_with_db(
        &s,
        &["semantics", "sym", "--validated", "maybe"],
        "--validated",
    );
    assert_rejected(&s, &["plugins", "lsit"], "lsit");
}

#[test]
fn empty_values_are_rejected() {
    let s = scratch("empty");
    assert_rejected(&s, &["stats", "--db", ""], "--db");
    rejected_with_db(&s, &["nodes", "--kind", ""], "--kind");
    rejected_with_db(&s, &["source", "--file", ""], "--file");
    rejected_with_db(&s, &["annotate", "f", "--key", "", "--value", "v"], "--key");
    rejected_with_db(&s, &["index", ".", "--repo="], "--repo");
    rejected_with_db(&s, &["query", ""], "<name>");
}

// ── (b) operands ────────────────────────────────────────────────────────────────────────────

#[test]
fn zero_operand_commands_reject_extras() {
    let s = scratch("zero_ops");
    for cmd in [
        "stats",
        "drift",
        "compact",
        "subscribe",
        "entrypoints",
        "leaves",
        "dead-code",
        "nodes",
        "graph-view",
        "export",
    ] {
        rejected_with_db(&s, &[cmd, "extra"], "\"extra\"");
    }
    assert_rejected(
        &s,
        &["correspond", "extra", "--db-a", "a.db", "--db-b", "b.db"],
        "\"extra\"",
    );
}

#[test]
fn required_operands_reject_missing_and_surplus_values() {
    let s = scratch("req_ops");
    for (cmd, placeholder) in [
        ("query", "<name>"),
        ("blast-radius", "<name>"),
        ("tfstate", "<file>"),
        ("import-telemetry", "<file.json>"),
        ("semantic", "<query>"),
        ("context", "<name>"),
        ("fingerprint", "<name>"),
        ("changed-since", "<sha>"),
        ("resolve", "<name>"),
        (
            "stale-annotations",
            "<cutoff-unix-seconds | YYYY-MM-DD> or --older-than",
        ),
        ("semantics", "<symbol>"),
        ("by-requirement", "<requirement>"),
    ] {
        rejected_with_db(&s, &[cmd], placeholder);
        rejected_with_db(&s, &[cmd, "1", "2"], "\"2\"");
    }
    rejected_with_db(&s, &["cross-graph", "a", "b"], "\"b\"");
    assert_rejected(&s, &["cross-graph", "--db", "a.db"], "<name>");
    rejected_with_db(&s, &["path", "a"], "<to>");
    rejected_with_db(&s, &["path", "a", "b", "c"], "\"c\"");
    // Optional single operands keep only their documented shape.
    rejected_with_db(&s, &["index", ".", "extra"], "\"extra\"");
    rejected_with_db(&s, &["watch", ".", "extra"], "\"extra\"");
    rejected_with_db(&s, &["source", "a", "b"], "\"b\"");
    rejected_with_db(&s, &["clusters", "3", "4"], "\"4\"");
    assert_rejected(&s, &["plugins", "list", "extra"], "\"extra\"");
    // `<name>` and `--symbol` are alternatives: both, or neither, is a usage error.
    rejected_with_db(
        &s,
        &[
            "annotate", "f", "--symbol", "id", "--key", "k", "--value", "v",
        ],
        "--symbol",
    );
    rejected_with_db(&s, &["annotate", "--key", "k", "--value", "v"], "<name>");
    rejected_with_db(&s, &["annotations", "f", "--symbol", "id"], "--symbol");
    rejected_with_db(&s, &["annotations"], "<name>");
}

/// `query a b` used to search for `a` and drop `b`; `stats foo` ignored `foo`.
#[test]
fn stats_and_query_extras_are_rejected_197_followup() {
    let s = scratch("stats_query");
    rejected_with_db(&s, &["stats", "foo"], "unexpected operand \"foo\"");
    rejected_with_db(&s, &["query", "a", "b"], "unexpected operand \"b\"");
}

/// The landed token classes: `-` and `-1` are operands, `-x` is a flag.
#[test]
fn dash_negative_and_single_dash_tokens_keep_their_classes() {
    let s = scratch("dashes");
    // `-x` is a flag no command owns.
    rejected_with_db(&s, &["query", "-x"], "unknown flag \"-x\"");
    rejected_with_db(&s, &["clusters", "-x"], "unknown flag \"-x\"");
    // `-1` is an operand: typed, so a negative min-size is refused as a value, not as a flag.
    rejected_with_db(&s, &["clusters", "-1"], "<min-size>");
    // Cutoff seconds are digits only (#259), so `-1` is a refused cutoff — still not a flag.
    rejected_with_db(
        &s,
        &["stale-annotations", "-1"],
        "<cutoff-unix-seconds | YYYY-MM-DD>: cutoff \"-1\"",
    );
    rejected_with_db(
        &s,
        &["stale-annotations", "abc"],
        "<cutoff-unix-seconds | YYYY-MM-DD>",
    );

    let fx = indexed("dashes_ok");
    // `-` is an ordinary name; a cutoff may be the epoch itself.
    let out = run(&fx, &["query", "-", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("match(es) for '-'"),
        "{}",
        stdout(&out)
    );
    let out = run(&fx, &["stale-annotations", "0", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("cutoff 0"), "{}", stdout(&out));
}

// ── (c) repeats ─────────────────────────────────────────────────────────────────────────────

#[test]
fn accidental_repeats_are_rejected_before_io() {
    let s = scratch("repeats");
    rejected_with_db(
        &s,
        &["nodes", "--kind", "Function", "--kind", "Class"],
        "--kind given more than once",
    );
    assert_rejected(
        &s,
        &["query", "x", "--db", "a.db", "--db", "b.db"],
        "--db given more than once",
    );
    rejected_with_db(
        &s,
        &["nodes", "--json", "--json"],
        "--json given more than once",
    );
    rejected_with_db(
        &s,
        &["blast-radius", "f", "--depth", "2", "--depth=3"],
        "--depth given more than once",
    );
    // `--as` is `--repo` by another name.
    rejected_with_db(&s, &["index", ".", "--repo", "a", "--as", "b"], "--as");
    assert_rejected(
        &s,
        &[
            "correspond",
            "--db-a",
            "a.db",
            "--db-b",
            "b.db",
            "--top",
            "1",
            "--top",
            "2",
        ],
        "--top given more than once",
    );
    rejected_with_db(
        &s,
        &["cross-graph", "f", "--dbs", "a.db", "--dbs", "b.db"],
        "--dbs given more than once",
    );
}

/// `cross-graph --db` repeats by design: every value is consulted, in the order given.
#[test]
fn cross_graph_db_repeats_keep_every_value_in_order() {
    let s = scratch("xgraph");
    for repo in ["ra", "rb", "rc"] {
        let d = s.join(repo);
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("m.py"),
            "def shared_target():\n    return 1\n\ndef caller():\n    return shared_target()\n",
        )
        .unwrap();
        let out = run(&s, &["index", repo, "--db", &format!("{repo}.db")]);
        assert!(out.status.success(), "{}", stderr(&out));
    }
    let order = |o: &Output| -> Vec<String> {
        let text = stdout(o);
        let head = text.split("=== cross-graph blast-radius").next().unwrap();
        head.lines()
            .filter_map(|l| l.trim().strip_prefix("[repo: "))
            .map(|l| l.trim_end_matches(']').to_string())
            .collect()
    };
    let out = run(
        &s,
        &[
            "cross-graph",
            "shared_target",
            "--db",
            "rc.db",
            "--db",
            "ra.db",
            "--db",
            "rb.db",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("across 3 repo(s)"),
        "{}",
        stdout(&out)
    );
    assert_eq!(order(&out), ["rc.db", "ra.db", "rb.db"], "{}", stdout(&out));
    // `--dbs` contributes in argv position too.
    let out = run(
        &s,
        &[
            "cross-graph",
            "shared_target",
            "--db",
            "rb.db",
            "--dbs",
            "rc.db,ra.db",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(order(&out), ["rb.db", "rc.db", "ra.db"], "{}", stdout(&out));
}

/// `graph-view --ignore` repeats by design: every pattern applies.
#[test]
fn graph_view_ignore_repeats_apply_every_pattern() {
    let s = scratch("gv_ignore");
    for dir in ["alpha", "beta", "gamma"] {
        fs::create_dir_all(s.join(dir)).unwrap();
        fs::write(
            s.join(dir).join("work.py"),
            format!(
                "def {dir}_compute():\n    return {dir}_helper()\n\ndef {dir}_helper():\n    return 2\n"
            ),
        )
        .unwrap();
    }
    let out = run(&s, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let files = |args: &[&str]| -> Vec<String> {
        let mut full = vec!["graph-view"];
        full.extend_from_slice(args);
        full.extend_from_slice(&["--db", "graph.db"]);
        let out = run(&s, &full);
        assert!(out.status.success(), "{}", stderr(&out));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let mut f: Vec<String> = doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["file"].as_str().unwrap().to_string())
            .collect();
        f.sort();
        f.dedup();
        f
    };
    assert_eq!(
        files(&[]),
        ["alpha/work.py", "beta/work.py", "gamma/work.py"]
    );
    assert_eq!(
        files(&["--ignore", "alpha/", "--ignore", "gamma/"]),
        ["beta/work.py"]
    );
}

// ── (d) required values and help ────────────────────────────────────────────────────────────

#[test]
fn a_value_flag_needs_a_real_value() {
    let s = scratch("needs_value");
    assert_rejected(&s, &["nodes", "--db"], "--db requires a value");
    assert_rejected(
        &s,
        &["nodes", "--db", "--help"],
        "--db requires a value, got the flag \"--help\"",
    );
    assert_rejected(
        &s,
        &["nodes", "--db", "--bogus"],
        "--db requires a value, got the flag \"--bogus\"",
    );
    assert!(!s.join("--help").exists() && !s.join("--bogus").exists());
}

#[test]
fn inline_values_are_refused_where_only_the_spaced_form_is_owned() {
    let s = scratch("inline");
    rejected_with_db(&s, &["graph-view", "--limit=5"], "write --limit <value>");
    rejected_with_db(&s, &["clusters", "--k=3"], "write --k <value>");
    assert_rejected(&s, &["nodes", "--db=probe.db"], "write --db <value>");
}

/// Help is a help request in flag position — standalone, after an inline value — and nowhere
/// else. It never opens or creates a store.
#[test]
fn help_is_recognised_only_in_flag_position() {
    let s = scratch("help");
    for args in [
        &["nodes", "--help"][..],
        &["nodes", "-h"][..],
        &["index", "--repo=x", "--help"][..],
        &["blast-radius", "f", "--depth=3", "--help"][..],
        // Help wins over an earlier mistake, as it did before W8.6.
        &["query", "a", "b", "--top", "x", "--help"][..],
        &["lineage", "--help"][..],
    ] {
        let before = snapshot(&s);
        let out = run(&s, args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(
            stdout(&out).contains("usage:"),
            "{args:?}: {}",
            stdout(&out)
        );
        assert_eq!(before, snapshot(&s), "{args:?} touched the directory");
    }
    // In a bridged command's value slot, `--help` is that flag's (refused) value too.
    let out = run(&s, &["lineage", "x", "--depth", "--help"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(stderr(&out).contains("--depth"), "{}", stderr(&out));
}

// ── (e) combinations the arm would ignore ───────────────────────────────────────────────────

/// A flag that means nothing without another — or that another silently overrides — is
/// refused before I/O instead of being dropped.
#[test]
fn flag_combinations_the_arm_would_ignore_are_refused_before_io() {
    let s = scratch("combos");
    for (args, why) in [
        (
            &["semantics", "sym", "--validated-by", "me"][..],
            "--validated-by applies only with --validated",
        ),
        (
            &["semantics", "sym", "--validated", "true"][..],
            "--validated applies only with --validated-by",
        ),
        (
            &["clusters", "--k", "3"][..],
            "--k applies only with --weight semantic",
        ),
        (
            &["clusters", "--weight", "semantic", "--resolution", "2"][..],
            "--resolution does not apply with --weight semantic",
        ),
        (
            &[
                "clusters",
                "--weight",
                "semantic",
                "--k",
                "3",
                "--min-pts",
                "2",
            ][..],
            "--min-pts does not apply with --k",
        ),
        (
            &["clusters", "--summary"][..],
            "--summary applies only with --json",
        ),
        (
            &["nodes", "--kind", "Function", "--annotated-with", "k"][..],
            "--kind does not apply with --annotated-with",
        ),
        (
            &["nodes", "--semantics"][..],
            "--semantics applies only with --json",
        ),
        (
            &["export", "--nodes-only", "--edges-only"][..],
            "--nodes-only does not apply with --edges-only",
        ),
        (
            &["graph-view", "--focus", "f", "--limit", "0"][..],
            "--focus does not apply with --limit 0",
        ),
        (&["annotate", "f", "--value", "v"][..], "--key is required"),
        (
            &["cross-graph", "f", "--dbs", "a.db,,b.db"][..],
            "--dbs has an empty item",
        ),
        (
            &["source", "--symbols", "a,"][..],
            "--symbols has an empty item",
        ),
    ] {
        rejected_with_db(&s, args, why);
    }
    assert_rejected(&s, &["cross-graph", "f"], "one of --db/--dbs is required");
    assert_rejected(&s, &["correspond", "--db-a", "a.db"], "--db-b is required");
    // An explicit in-memory store has nowhere to keep history or embeddings.
    assert_rejected(
        &s,
        &["index", ".", "--db", ":memory:", "--embeddings"],
        "--embeddings does not apply with --db :memory:",
    );
}

/// The in-memory rule sees the store the run would open, including a default the environment
/// sets: `WICKED_ESTATE_DB=:memory: index . --history` used to skip history and exit 0.
#[test]
fn an_in_memory_default_from_the_environment_is_seen_before_io() {
    let s = scratch("env_memory");
    fs::write(s.join("m.py"), "def f():\n    return 1\n").unwrap();
    let mem = [("WICKED_ESTATE_DB", ":memory:")];
    for args in [
        &["index", ".", "--history"][..],
        &["index", ".", "--embeddings"][..],
        &["watch", ".", "--history"][..],
    ] {
        let before = snapshot(&s);
        let out = run_with_env(&s, args, &mem);
        let err = stderr(&out);
        assert!(!out.status.success(), "{args:?} exited 0: {}", stdout(&out));
        assert!(out.stdout.is_empty(), "{args:?}: {}", stdout(&out));
        assert!(
            err.contains(&format!("usage: wicked-estate {}", args[0])),
            "{args:?}: {err}"
        );
        assert!(
            err.contains("does not apply with the default store :memory: (set by the environment"),
            "{args:?}: {err}"
        );
        assert_eq!(before, snapshot(&s), "{args:?} touched the directory");
    }
    // An explicit file store overrides the in-memory default, and history is kept there.
    let out = run_with_env(&s, &["index", ".", "--history", "--db", "g.db"], &mem);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.join("g.db").exists());
}

/// The combinations each arm does read keep working.
#[test]
fn flag_combinations_the_arm_reads_still_run() {
    let fx = indexed("combos_ok");
    for args in [
        &["nodes", "--annotated-with", "k", "--json", "--semantics"][..],
        &["export", "--nodes-only"][..],
        &[
            "clusters",
            "--weight",
            "graph",
            "--resolution",
            "1.5",
            "--summary",
            "--json",
        ][..],
        &["clusters", "--weight", "semantic", "--k", "2", "--json"][..],
        &["graph-view", "--focus", "f0", "--limit", "5"][..],
    ] {
        let mut full = args.to_vec();
        full.extend_from_slice(&["--db", "graph.db"]);
        let out = run(&fx, &full);
        assert!(out.status.success(), "{full:?}: {}", stderr(&out));
    }
}

// ── source: selectors and JSON-only budgets stay as documented ──────────────────────────────

#[test]
fn source_selector_precedence_and_json_only_budgets_are_unchanged() {
    let fx = duplicate_names("src_prec");
    let alpha = symbol_id_in(&fx, "alpha.py");
    // --symbols > --file > <name>: the symbol in alpha wins over a --file naming beta.
    let out = run(
        &fx,
        &[
            "source",
            "validate_confined_directory",
            "--file",
            "beta.py",
            "--symbols",
            &alpha,
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
    // --file > <name>.
    let out = run(
        &fx,
        &[
            "source",
            "validate_confined_directory",
            "--file",
            "beta.py",
            "--db",
            "graph.db",
        ],
    );
    let text = stdout(&out);
    assert!(
        text.contains("/safe/beta") && !text.contains("/safe/alpha"),
        "{text}"
    );
    // The budgets are JSON-only; with --json they are honoured, typed.
    assert_rejected(
        &fx,
        &[
            "source",
            "validate_confined_directory",
            "--max-node-chars",
            "10",
            "--db",
            "graph.db",
        ],
        "--max-node-chars applies only with --json",
    );
    let out = run(
        &fx,
        &[
            "source",
            "validate_confined_directory",
            "--json",
            "--max-node-chars",
            "10",
            "--db",
            "graph.db",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["nodes"].as_array().unwrap().len(), 2, "{doc}");
}

/// A write command with a malformed value must not touch an existing graph.
#[test]
fn a_rejected_write_leaves_an_existing_graph_byte_identical() {
    let fx = indexed("write_untouched");
    assert_rejected(
        &fx,
        &[
            "annotate",
            "f0",
            "--key",
            "k",
            "--value",
            "v",
            "--confidence",
            "high",
            "--db",
            "graph.db",
        ],
        "--confidence",
    );
    assert_rejected(
        &fx,
        &[
            "clusters",
            "--annotate",
            "--resolution",
            "fine",
            "--db",
            "graph.db",
        ],
        "--resolution",
    );
    assert_rejected(
        &fx,
        &[
            "semantics",
            "f0",
            "--validated",
            "maybe",
            "--db",
            "graph.db",
        ],
        "--validated",
    );
}

// ── fixtures ────────────────────────────────────────────────────────────────────────────────

/// A committed git repo with a Python chain `f0 → f1`, indexed into `<dir>/graph.db`.
fn indexed(tag: &str) -> Scratch {
    let s = scratch(tag);
    fs::write(
        s.join("m.py"),
        "def f0():\n    return f1()\n\ndef f1():\n    return 1\n",
    )
    .unwrap();
    git(&s, &["init", "-q", "."]);
    git(&s, &["add", "-A"]);
    git(&s, &["commit", "-qm", "fx"]);
    let out = run(&s, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    s
}

/// Two Python functions sharing one name (the #206 fixture).
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
    let out = run(&s, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
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
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    rows.as_array()
        .unwrap()
        .iter()
        .find(|r| r["file"] == file)
        .unwrap_or_else(|| panic!("no match in {file}: {}", stdout(&out)))["symbol_id"]
        .as_str()
        .unwrap()
        .to_string()
}
