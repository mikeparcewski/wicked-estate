//! Symbol-name misses through the real binary (wicked-estate#358).
//!
//! Every name-addressed command, handed a name that resolves to no symbol, must (1) say so —
//! distinct from an empty answer for a resolved symbol — (2) offer ranked did-you-mean
//! candidates and never substitute one, and (3) log the miss to `search_misses`, which
//! `misses` reads back. The fixture is a small indexed Rust repo; every probe is a perturbation
//! of a real name (case flip, typo, qualified form).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

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

const SRC: &str = "\
pub struct OrderService;

impl OrderService {
    pub fn create(&self) {}
}

pub fn create_order() {}

pub fn cancel_order() {
    create_order();
}
";

/// The fixture repo, committed and indexed into `<dir>/graph.db`.
fn indexed(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_namemiss_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let s = Scratch(d);
    fs::create_dir_all(s.join("src")).unwrap();
    fs::write(s.join("src/lib.rs"), SRC).unwrap();
    git(&s, &["init", "-q", "."]);
    git(&s, &["add", "-A"]);
    git(&s, &["commit", "-qm", "fx"]);
    let out = run(&s, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "index failed: {}", stderr(&out));
    s
}

/// Git, hermetically (as `bridge_cli.rs` does).
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

/// The binary inside `dir`, hermetically (no bus, no telemetry, no store override).
fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(dir)
        .args(args)
        .env("WICKED_ESTATE_EMIT_PROGRAM", "wicked-bus-absent-name-miss")
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

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn with_db<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut v = args.to_vec();
    v.extend_from_slice(&["--db", "graph.db"]);
    v
}

/// Rows in the miss log, read straight from the file.
fn logged_misses(dir: &Path) -> Vec<String> {
    let store = wicked_estate_store::SqliteStore::open_readonly(dir.join("graph.db")).unwrap();
    store
        .search_misses(1_000, None)
        .unwrap()
        .into_iter()
        .map(|m| m.query)
        .collect()
}

/// The R3 failure #358 retires: `blast-radius <miss>` must not read as "nothing depends on it".
#[test]
fn blast_radius_miss_is_labelled_not_a_leaf() {
    let s = indexed("br");
    let out = run(&s, &with_db(&["blast-radius", "Create_Order"]));
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("no symbol named 'Create_Order' — did you mean:"),
        "{text}"
    );
    assert!(text.contains("create_order"), "{text}");
    assert!(
        !text.contains("no resolved dependents"),
        "a miss must not read as a leaf: {text}"
    );
    assert!(
        !text.contains("coverage:"),
        "no coverage line for a name nothing resolved to and nothing calls: {text}"
    );

    // --json: the usual keys stay, the miss is flagged, the candidate is ranked first.
    let out = run(&s, &with_db(&["blast-radius", "Create_Order", "--json"]));
    assert!(out.status.success(), "{}", stderr(&out));
    let doc: Value = serde_json::from_str(&stdout(&out)).expect("one JSON document");
    assert_eq!(doc["resolved"], json!(false), "{doc}");
    assert_eq!(doc["query"], json!("Create_Order"));
    assert_eq!(doc["dependents"], json!([]), "usual keys kept: {doc}");
    assert_eq!(
        doc["suggestions"][0]["name"],
        json!("create_order"),
        "{doc}"
    );
    assert_eq!(
        doc["suggestions"][0]["match_reason"],
        json!("case_insensitive")
    );

    // A resolved name: `resolved: true`, no suggestions, and the coverage line is stated.
    let out = run(&s, &with_db(&["blast-radius", "create_order", "--json"]));
    let doc: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(doc["resolved"], json!(true), "{doc}");
    assert!(doc.get("suggestions").is_none(), "{doc}");
    let out = run(&s, &with_db(&["blast-radius", "cancel_order"]));
    let text = stdout(&out);
    assert!(text.contains("coverage:"), "{text}");
    assert!(
        !text.contains("did you mean"),
        "a resolved name is not a miss: {text}"
    );
}

#[test]
fn every_name_addressed_command_offers_candidates_on_a_miss() {
    let s = indexed("sites");
    let typo = "creat_order";
    for args in [
        vec!["query", typo],
        vec!["source", typo],
        vec!["annotations", typo],
        vec!["fingerprint", typo],
    ] {
        let out = run(&s, &with_db(&args));
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        let text = stdout(&out);
        assert!(
            text.contains(&format!("no symbol named '{typo}' — did you mean:"))
                && text.contains("create_order"),
            "{args:?}: {text}"
        );
    }
    // annotate: a miss annotates nothing and says why — no "annotated 0 symbol(s)".
    let out = run(
        &s,
        &with_db(&[
            "annotate", typo, "--type", "note", "--key", "k", "--value", "v",
        ]),
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("did you mean") && text.contains("create_order"),
        "{text}"
    );
    assert!(!text.contains("annotated 0"), "{text}");
    // source --json: `resolved` on the bundle, both ways.
    let doc: Value =
        serde_json::from_str(&stdout(&run(&s, &with_db(&["source", typo, "--json"])))).unwrap();
    assert_eq!(doc["resolved"], json!(false), "{doc}");
    let doc: Value = serde_json::from_str(&stdout(&run(
        &s,
        &with_db(&["source", "create_order", "--json"]),
    )))
    .unwrap();
    assert_eq!(doc["resolved"], json!(true), "{doc}");
    // path: the unresolved operand is a miss, not "no route".
    let out = run(&s, &with_db(&["path", "cancel_order", typo]));
    let text = stdout(&out);
    assert!(text.contains("'to' operand matched no symbol"), "{text}");
    assert!(
        text.contains("did you mean") && text.contains("create_order"),
        "{text}"
    );
    let out = run(&s, &with_db(&["path", "cancel_order", typo, "--json"]));
    let doc: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(doc["resolved"], json!(false), "{doc}");
    assert_eq!(doc["query"], json!(typo));
    assert_eq!(
        doc["suggestions"][0]["name"],
        json!("create_order"),
        "{doc}"
    );
    // graph-view --focus and the bridged commands fail, naming the candidates.
    for args in [vec!["graph-view", "--focus", typo], vec!["traverse", typo]] {
        let out = run(&s, &with_db(&args));
        assert!(!out.status.success(), "{args:?} must fail on a miss");
        let err = stderr(&out);
        assert!(
            err.contains("did you mean") && err.contains("create_order"),
            "{args:?}: {err}"
        );
    }
    // cross-graph: a miss in every database, with candidates per repo.
    let out = run(&s, &["cross-graph", typo, "--db", "graph.db"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("in any of the specified databases — did you mean:")
            && text.contains("create_order"),
        "{text}"
    );
    assert!(text.contains("(a miss, not a leaf)"), "{text}");
    assert!(!text.contains("no resolved dependents"), "{text}");
}

/// `query --json` keeps its array on stdout; the miss document and `NAME-MISS:` go to stderr.
#[test]
fn query_json_keeps_the_array_and_reports_the_miss_on_stderr() {
    let s = indexed("qjson");
    let out = run(&s, &with_db(&["query", "orderservice", "--json"]));
    assert!(out.status.success(), "{}", stderr(&out));
    let rows: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(rows, json!([]));
    let err = stderr(&out);
    assert!(err.contains("NAME-MISS: 'orderservice'"), "{err}");
    let doc: Value = err
        .lines()
        .find_map(|l| serde_json::from_str::<Value>(l).ok())
        .expect("the miss document on stderr");
    assert_eq!(doc["resolved"], json!(false));
    assert_eq!(
        doc["suggestions"][0]["name"],
        json!("OrderService"),
        "{doc}"
    );
}

/// One miss writes one `search_misses` row; `misses --json` reads it back with today's best
/// suggestion. A hit writes nothing.
#[test]
fn a_miss_is_logged_once_and_misses_reads_it_back() {
    let s = indexed("log");
    assert!(logged_misses(&s).is_empty());
    let out = run(&s, &with_db(&["query", "create_order"]));
    assert!(out.status.success());
    assert!(logged_misses(&s).is_empty(), "a hit is not a miss");

    let out = run(&s, &with_db(&["query", "cancel_ordr"]));
    assert!(out.status.success());
    assert_eq!(logged_misses(&s), vec!["cancel_ordr".to_string()]);
    let out = run(&s, &with_db(&["blast-radius", "cancel_ordr"]));
    assert!(out.status.success());
    assert_eq!(logged_misses(&s).len(), 2);

    let out = run(&s, &with_db(&["misses", "--json"]));
    assert!(out.status.success(), "{}", stderr(&out));
    let doc: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(doc["total_queries"], json!(1), "{doc}");
    let row = &doc["misses"][0];
    assert_eq!(row["query"], json!("cancel_ordr"));
    assert_eq!(row["count"], json!(2));
    assert_eq!(
        row["best_suggestion"]["name"],
        json!("cancel_order"),
        "{doc}"
    );
    // `misses` itself reads read-only: it logs nothing.
    assert_eq!(logged_misses(&s).len(), 2);

    let text = stdout(&run(&s, &with_db(&["misses"])));
    assert!(
        text.contains("2x  'cancel_ordr'") && text.contains("did you mean"),
        "{text}"
    );
}
