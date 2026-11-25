//! `wicked-estate changed-since <sha>` maps git's paths onto the graph's stored paths (#352).
//!
//! git names a changed file relative to the repository top level; the graph stores it relative
//! to the indexed root, under `<label>/` when the repo was indexed with `--repo`. Looking git's
//! path up verbatim found nothing on a labelled graph, or on a graph of a subdirectory, and the
//! command reported "0 symbols": a caller (test impact) read that as "nothing changed".

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn estate(dir: &Path, args: &[&str]) -> Output {
    estate_from(dir, dir.parent().unwrap(), args)
}

/// [`estate`] run from `cwd` (a subdirectory, say), with git stopped above `ceiling`.
fn estate_from(cwd: &Path, ceiling: &Path, args: &[&str]) -> Output {
    let out = Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-changed-since",
        )
        .env("WICKED_ESTATE_EMIT_DEADLETTER", cwd.join("emit.ndjson"))
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .output()
        .expect("spawn wicked-estate");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// A git repo whose `svc/` directory holds the code; the first commit is returned.
fn repo(tmp: &Path) -> (std::path::PathBuf, String) {
    let dir = tmp.join("checkout");
    fs::create_dir_all(dir.join("svc/src")).unwrap();
    fs::write(
        dir.join("svc/src/orders.ts"),
        "export function placeOrder() { return 1; }\n",
    )
    .unwrap();
    fs::write(dir.join("README.md"), "docs\n").unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    let first = git(&dir, &["rev-parse", "HEAD"]);
    fs::write(
        dir.join("svc/src/orders.ts"),
        "export function placeOrder() { return 2; }\nexport function cancelOrder() {}\n",
    )
    .unwrap();
    fs::write(dir.join("README.md"), "more docs\n").unwrap();
    git(&dir, &["commit", "-q", "-am", "two"]);
    (dir, first)
}

fn names(json: &[u8]) -> Vec<String> {
    let rows: serde_json::Value = serde_json::from_slice(json).expect("a JSON array");
    let mut names: Vec<String> = rows
        .as_array()
        .expect("array")
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn a_labelled_graph_reports_the_changed_symbols() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", ".", "--repo", "shop", "--db", db]);
    let out = estate(&dir, &["changed-since", &first, "--json", "--db", db]);
    let found = names(&out.stdout);
    for want in ["cancelOrder", "placeOrder"] {
        assert!(found.iter().any(|n| n == want), "{want}: {found:?}");
    }
    // Both changed files are inside the indexed root, so each is reported under the label:
    // orders.ts's symbols and the README's file node, and nothing else.
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: std::collections::BTreeSet<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        ["shop/README.md", "shop/svc/src/orders.ts"].into(),
        "{rows}"
    );

    let text =
        String::from_utf8_lossy(&estate(&dir, &["changed-since", &first, "--db", db]).stdout)
            .into_owned();
    assert!(!text.starts_with("0 symbol(s)"), "{text}");
    assert!(text.contains("placeOrder"), "{text}");
}

#[test]
fn a_graph_of_a_subdirectory_reports_the_changed_symbols() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let out = estate(&dir, &["changed-since", &first, "--json", "--db", db]);
    let found = names(&out.stdout);
    for want in ["cancelOrder", "placeOrder"] {
        assert!(found.iter().any(|n| n == want), "{want}: {found:?}");
    }
    // README.md changed too, outside the indexed root: said, by name, not silently dropped. With
    // --json it goes to stderr, so stdout stays the one array parsed above.
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        err.contains("1 changed file(s) outside the indexed root(s), not in this graph: README.md"),
        "{err}"
    );
    let text =
        String::from_utf8_lossy(&estate(&dir, &["changed-since", &first, "--db", db]).stdout)
            .into_owned();
    assert!(
        text.contains(
            "1 changed file(s) outside the indexed root(s), not in this graph: README.md"
        ),
        "{text}"
    );
}

/// `diff.relative` makes `git diff` name paths relative to the working directory, and leave out
/// the rest; run from `svc/`, the mapping still gets top-level paths.
#[test]
fn diff_relative_config_does_not_change_the_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    git(&dir, &["config", "diff.relative", "true"]);
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let out = estate_from(
        &dir.join("svc"),
        tmp.path(),
        &["changed-since", &first, "--json", "--db", db],
    );
    let found = names(&out.stdout);
    for want in ["cancelOrder", "placeOrder"] {
        assert!(found.iter().any(|n| n == want), "{want}: {found:?}");
    }
}

/// A backslash in a unix file name: the indexer stores it as `/`, and so does the mapping.
#[cfg(unix)]
#[test]
fn a_backslash_in_a_file_name_maps_like_the_indexer() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, _) = repo(tmp.path());
    let odd = dir.join("svc/src/a\\b.ts");
    fs::write(&odd, "export function oddOne() { return 1; }\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "three"]);
    let before = git(&dir, &["rev-parse", "HEAD"]);
    fs::write(&odd, "export function oddOne() { return 2; }\n").unwrap();
    git(&dir, &["commit", "-q", "-am", "four"]);
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let out = estate(&dir, &["changed-since", &before, "--json", "--db", db]);
    let found = names(&out.stdout);
    assert!(found.iter().any(|n| n == "oddOne"), "{found:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("outside the indexed root"), "{err}");
}

/// On a case-insensitive filesystem a case-only directory rename leaves git with the old case.
/// The graph of the renamed directory still gets the change (skipped where names are
/// case-sensitive).
#[test]
fn a_case_only_rename_on_a_case_insensitive_filesystem_still_maps() {
    let tmp = tempfile::tempdir().unwrap();
    let probe = tmp.path().join("CaseProbe");
    fs::create_dir(&probe).unwrap();
    if !tmp.path().join("caseprobe").exists() {
        return;
    }
    let (dir, _) = repo(tmp.path());
    let before = git(&dir, &["rev-parse", "HEAD"]);
    fs::rename(dir.join("svc"), dir.join("SVC")).unwrap();
    fs::write(
        dir.join("SVC/src/orders.ts"),
        "export function placeOrder() { return 3; }\nexport function refundOrder() {}\n",
    )
    .unwrap();
    git(&dir, &["commit", "-q", "-am", "three"]);
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "SVC", "--db", db]);
    let out = estate(&dir, &["changed-since", &before, "--json", "--db", db]);
    let found = names(&out.stdout);
    assert!(found.iter().any(|n| n == "refundOrder"), "{found:?}");
}
