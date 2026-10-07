//! `wicked-estate stats --json` (#201, #198): ONE JSON document carrying the graph's identity
//! (full commit SHA, branch, dirty, canonical `indexed_root`, `indexed_at`, versions) and its
//! freshness (`staleness.commits_behind`, measured from the indexed commit — #243), with one
//! provenance block per co-located repo (#245). Spawns the real binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_stats_json_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A git repo with one committed TypeScript file. Returns HEAD's full SHA.
fn committed_repo(root: &Path, unique: &str) -> String {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/index.ts"),
        format!("export function {unique}() {{ return 1; }}\n"),
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.invalid"]);
    git(root, &["config", "user.name", "wicked-test"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    git(root, &["rev-parse", "HEAD"])
}

fn commit_one(root: &Path, name: &str) {
    fs::write(root.join(format!("src/{name}.ts")), "export const x = 1;\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", name]);
}

fn run(cwd: &Path, db: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new(bin());
    cmd.current_dir(cwd);
    cmd.args(args);
    cmd.args(["--db", db.to_str().unwrap()]);
    let out = cmd.output().expect("spawn wicked-estate");
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

/// The whole of stdout parses as ONE JSON document — a `STALENESS:` line beside it would fail
/// this parse, which is exactly the one-document rule machine callers rely on.
fn stats_json(cwd: &Path, db: &Path) -> serde_json::Value {
    let out = run(cwd, db, &["stats", "--json"]);
    serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("stats --json is not one JSON document ({e}): {out}"))
}

#[test]
fn stats_json_is_one_document_with_full_identity_and_freshness() {
    let dir = fresh_dir("identity");
    let repo = dir.join("repo");
    let head = committed_repo(&repo, "ident");
    let db = dir.join("graph.db");
    // Indexed from a DIFFERENT working directory with a relative path, the #248 shape.
    run(&dir, &db, &["index", "repo"]);

    let doc = stats_json(&dir, &db);
    let prov = &doc["provenance"];
    assert_eq!(prov["commit"].as_str(), Some(head.as_str()), "{doc}");
    assert_eq!(head.len(), 40, "the full SHA, not the 8-char human form");
    assert_eq!(prov["dirty"], serde_json::json!(false), "{doc}");
    assert!(prov["branch"].is_string(), "{doc}");
    let root = prov["indexed_root"].as_str().expect("indexed_root");
    assert!(
        Path::new(root).is_absolute(),
        "canonical root, not 'repo': {root}"
    );
    assert_eq!(
        PathBuf::from(root),
        fs::canonicalize(&repo).unwrap(),
        "{doc}"
    );
    let at = prov["indexed_at"].as_str().expect("indexed_at");
    assert!(
        at.len() == 20 && at.ends_with('Z') && &at[10..11] == "T",
        "RFC 3339 UTC: {at}"
    );
    assert_eq!(
        prov["indexed_version"].as_str(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert!(
        prov["id_scheme"].is_string() && prov["graph_version"].is_string(),
        "{doc}"
    );
    assert_eq!(doc["files"], serde_json::json!(1), "{doc}");
    assert_eq!(
        doc["staleness"]["commits_behind"],
        serde_json::json!(0),
        "{doc}"
    );
    assert_eq!(doc["staleness"]["checked"], serde_json::json!(1), "{doc}");
    assert_eq!(
        doc["repos"],
        serde_json::json!([]),
        "a single-repo graph has no repo blocks"
    );

    // One commit later: behind by one, from ANY working directory (#248), measured from the
    // indexed commit even though reading the graph just wrote to the db file (#243).
    commit_one(&repo, "later");
    let elsewhere = fresh_dir("elsewhere");
    let doc = stats_json(&elsewhere, &db);
    assert_eq!(
        doc["staleness"]["commits_behind"],
        serde_json::json!(1),
        "{doc}"
    );
    assert_eq!(
        doc["provenance"]["commit"].as_str(),
        Some(head.as_str()),
        "identity is the INDEXED commit"
    );
    // The human form still says so, as a line.
    let human = run(&elsewhere, &db, &["stats"]);
    assert!(human.contains("STALENESS: 1 commit(s)"), "{human}");

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&elsewhere);
}

#[test]
fn stats_json_has_one_provenance_block_per_repo() {
    let dir = fresh_dir("per_repo");
    let a = dir.join("a");
    let b = dir.join("b");
    let head_a = committed_repo(&a, "alpha");
    let head_b = committed_repo(&b, "beta");
    let db = dir.join("shared.db");
    // Indexed by RELATIVE paths from `dir`; read back from another cwd below (#248 for the
    // registry's roots — codex round 1 on #264).
    run(&dir, &db, &["index", "a", "--repo", "a"]);
    run(&dir, &db, &["index", "b", "--repo", "b"]);

    let elsewhere = fresh_dir("per_repo_elsewhere");
    let doc = stats_json(&elsewhere, &db);
    let repos = doc["repos"].as_array().expect("repos");
    assert_eq!(repos.len(), 2, "{doc}");
    let by_label = |l: &str| {
        repos
            .iter()
            .find(|r| r["label"] == l)
            .unwrap_or_else(|| panic!("no block for {l}: {doc}"))
            .clone()
    };
    let (ra, rb) = (by_label("a"), by_label("b"));
    assert_eq!(
        PathBuf::from(ra["root"].as_str().unwrap()),
        fs::canonicalize(&a).unwrap(),
        "the registry root is canonical, not 'a': {doc}"
    );
    assert_eq!(ra["commit"].as_str(), Some(head_a.as_str()));
    assert_eq!(rb["commit"].as_str(), Some(head_b.as_str()));
    assert_eq!(ra["files"], serde_json::json!(1));
    assert_eq!(ra["commits_behind"], serde_json::json!(0));
    assert_eq!(rb["commits_behind"], serde_json::json!(0));
    assert_eq!(doc["staleness"]["commits_behind"], serde_json::json!(0));
    assert_eq!(doc["staleness"]["checked"], serde_json::json!(2));
    assert!(
        doc["provenance"]["commit"].is_null(),
        "a labelled index never writes the singular repo_* keys: {doc}"
    );

    // Two commits in `b`, none in `a`: the graph is as stale as its stalest repo, and each
    // block says which (#245).
    commit_one(&b, "b1");
    commit_one(&b, "b2");
    let doc = stats_json(&elsewhere, &db);
    let repos = doc["repos"].as_array().unwrap();
    let behind =
        |l: &str| repos.iter().find(|r| r["label"] == l).unwrap()["commits_behind"].clone();
    assert_eq!(behind("a"), serde_json::json!(0), "{doc}");
    assert_eq!(behind("b"), serde_json::json!(2), "{doc}");
    assert_eq!(
        doc["staleness"]["commits_behind"],
        serde_json::json!(2),
        "{doc}"
    );

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&elsewhere);
}
