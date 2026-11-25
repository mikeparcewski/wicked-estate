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

/// On a case-insensitive filesystem a case-only rename of the indexed root's directory leaves git
/// with the old case. The graph of the renamed directory still gets the change. It runs where the
/// filesystem is case-insensitive (macOS and Windows) and is reported ignored elsewhere.
#[test]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    ignore = "needs a case-insensitive filesystem"
)]
fn a_case_only_rename_on_a_case_insensitive_filesystem_still_maps() {
    let tmp = tempfile::tempdir().unwrap();
    let probe = tmp.path().join("CaseProbe");
    fs::create_dir(&probe).unwrap();
    assert!(
        tmp.path().join("caseprobe").exists(),
        "the temp directory's filesystem is case-sensitive; this test needs a case-insensitive one"
    );
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

/// A graph used from another worktree of its repository: the root is rebased onto this checkout
/// at its position in the work tree, so the changed symbols are found, a subdirectory graph keeps
/// its scope, and a file outside it is still named outside (#606 round 2).
#[test]
fn a_graph_used_from_another_worktree_still_reports_the_changed_symbols() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let other = tmp.path().join("other");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            other.to_str().unwrap(),
            "HEAD",
        ],
    );
    for (scope, outside) in [(".", false), ("svc", true)] {
        let db = tmp.path().join(format!("graph-{}.db", scope.len()));
        let db = db.to_str().unwrap();
        estate(&dir, &["index", scope, "--db", db]);
        let out = estate(&other, &["changed-since", &first, "--json", "--db", db]);
        let found = names(&out.stdout);
        for want in ["cancelOrder", "placeOrder"] {
            assert!(
                found.iter().any(|n| n == want),
                "{scope}: {want}: {found:?}"
            );
        }
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            err.contains("outside the indexed root(s), not in this graph: README.md"),
            outside,
            "{scope}: {err}"
        );
    }
}

/// A moved checkout: the recorded root is gone, so the root is placed in this checkout at the
/// position recorded at index time (#606 round 2).
#[test]
fn a_graph_used_from_a_moved_checkout_still_reports_the_changed_symbols() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let moved = tmp.path().join("moved");
    fs::rename(&dir, &moved).unwrap();
    // Then again with a plain directory made where the root was.
    for remade in [false, true] {
        if remade {
            fs::create_dir_all(dir.join("svc")).unwrap();
        }
        let out = estate(&moved, &["changed-since", &first, "--json", "--db", db]);
        let found = names(&out.stdout);
        for want in ["cancelOrder", "placeOrder"] {
            assert!(
                found.iter().any(|n| n == want),
                "remade: {remade}: {want}: {found:?}"
            );
        }
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains(
                "1 changed file(s) outside the indexed root(s), not in this graph: README.md"
            ),
            "remade: {remade}: {err}"
        );
    }
}

/// Another repository with the same file names is not this graph's checkout, even when it holds
/// the graph's commit in a side ref: it shares no history with its HEAD, so nothing is rebased.
/// The root is said to be not placed in this checkout, whether it still exists or is gone.
#[test]
fn an_unrelated_repository_is_not_read_as_a_moved_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, _) = repo(tmp.path());
    let indexed = git(&dir, &["rev-parse", "HEAD"]);
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", ".", "--db", db]);
    let elsewhere = tempfile::tempdir().unwrap();
    let stranger = elsewhere.path().join("stranger");
    fs::create_dir_all(stranger.join("svc/src")).unwrap();
    let orders = stranger.join("svc/src/orders.ts");
    fs::write(&orders, "export function strangerOrder() { return 1; }\n").unwrap();
    git(&stranger, &["init", "-q", "-b", "main"]);
    git(&stranger, &["add", "-A"]);
    git(&stranger, &["commit", "-q", "-m", "one"]);
    let first = git(&stranger, &["rev-parse", "HEAD"]);
    fs::write(&orders, "export function strangerOrder() { return 2; }\n").unwrap();
    git(&stranger, &["commit", "-q", "-am", "two"]);
    git(
        &stranger,
        &[
            "fetch",
            "-q",
            dir.to_str().unwrap(),
            "+refs/heads/main:refs/side/graph",
        ],
    );
    let held = git(&stranger, &["rev-parse", "refs/side/graph"]);
    assert_eq!(held, indexed, "the side ref holds the graph's commit");
    for gone in [false, true] {
        if gone {
            fs::remove_dir_all(&dir).unwrap();
        }
        let out = estate(&stranger, &["changed-since", &first, "--json", "--db", db]);
        assert_eq!(names(&out.stdout), Vec::<String>::new(), "gone: {gone}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("1 indexed root(s) not placed in this checkout"),
            "gone: {gone}: {err}"
        );
        assert!(
            err.contains("1 changed file(s) under no root placed in this checkout"),
            "gone: {gone}: {err}"
        );
        assert!(!err.contains("not in this graph"), "gone: {gone}: {err}");
    }
}

/// A clone of the repository, with the indexed checkout still in place, is placed by shared
/// history; a shallow clone that lacks the indexed commit cannot be, and says so.
#[test]
fn a_clone_is_placed_by_shared_history_and_a_shallow_one_without_the_commit_is_said_unplaced() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, _) = repo(tmp.path());
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let orders = dir.join("svc/src/orders.ts");
    let mut since = String::new();
    for (i, body) in ["3", "4"].into_iter().enumerate() {
        fs::write(
            &orders,
            format!("export function placeOrder() {{ return {body}; }}\n"),
        )
        .unwrap();
        git(&dir, &["commit", "-q", "-am", body]);
        if i == 0 {
            since = git(&dir, &["rev-parse", "HEAD"]);
        }
    }
    for (name, depth, placed) in [("full", None, true), ("shallow", Some("2"), false)] {
        let clone = tmp.path().join(name);
        // `--no-local`: git's own transport, so `--depth` applies to a path (no URL to encode).
        let mut args = vec!["clone", "-q", "--no-local"];
        if let Some(depth) = depth {
            args.extend(["--depth", depth]);
        }
        args.extend([dir.to_str().unwrap(), clone.to_str().unwrap()]);
        git(tmp.path(), &args);
        let out = estate(&clone, &["changed-since", &since, "--json", "--db", db]);
        let err = String::from_utf8_lossy(&out.stderr);
        if placed {
            let found = names(&out.stdout);
            for want in ["cancelOrder", "placeOrder"] {
                assert!(found.iter().any(|n| n == want), "{name}: {want}: {found:?}");
            }
            assert!(!err.contains("not placed"), "{name}: {err}");
        } else {
            assert_eq!(names(&out.stdout), Vec::<String>::new(), "{name}");
            assert!(
                err.contains("1 indexed root(s) not placed in this checkout"),
                "{name}: {err}"
            );
        }
    }
}

/// Two labelled scopes indexed from two linked worktrees of one repository: from either
/// worktree, both labels read this checkout's changes.
#[test]
fn labelled_scopes_from_two_worktrees_both_read_this_checkouts_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let other = tmp.path().join("other");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            other.to_str().unwrap(),
            "HEAD",
        ],
    );
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--repo", "api", "--db", db]);
    estate(&other, &["index", ".", "--repo", "all", "--db", db]);
    let out = estate(&dir, &["changed-since", &first, "--json", "--db", db]);
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: std::collections::BTreeSet<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            "all/README.md",
            "all/svc/src/orders.ts",
            "api/src/orders.ts"
        ]
        .into(),
        "{rows}"
    );
}

/// A labelled graph used from a moved checkout keeps its label and scope.
#[test]
fn a_labelled_graph_used_from_a_moved_checkout_keeps_its_label() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--repo", "shop", "--db", db]);
    let moved = tmp.path().join("moved");
    fs::rename(&dir, &moved).unwrap();
    let out = estate(&moved, &["changed-since", &first, "--json", "--db", db]);
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: std::collections::BTreeSet<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["shop/src/orders.ts"].into(), "{rows}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("1 changed file(s) outside the indexed root(s), not in this graph: README.md"),
        "{err}"
    );
}

/// A linked worktree nested inside the indexed checkout is another checkout, not a directory of
/// the indexed one: its files map at the same position.
#[test]
fn a_graph_used_from_a_nested_worktree_still_reports_the_changed_symbols() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let nested = dir.join("nested");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            nested.to_str().unwrap(),
            "HEAD",
        ],
    );
    // git may look above the worktree (the root is in the checkout holding it).
    let out = estate_from(
        &nested,
        tmp.path(),
        &["changed-since", &first, "--json", "--db", db],
    );
    let found = names(&out.stdout);
    for want in ["cancelOrder", "placeOrder"] {
        assert!(found.iter().any(|n| n == want), "{want}: {found:?}");
    }
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("1 changed file(s) outside the indexed root(s), not in this graph: README.md"),
        "{err}"
    );
}

/// A graph that recorded no position (written before it was recorded, or where git could not
/// answer) and whose root is gone cannot be placed: said so, not reported as empty.
#[test]
fn a_moved_checkout_of_a_graph_without_a_recorded_position_is_said_unplaced() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let db_path = tmp.path().join("graph.db");
    let db = db_path.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let recorded: String = conn
        .query_row("SELECT v FROM meta WHERE k = 'indexed_subpath'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(recorded, "\"svc\"");
    conn.execute("DELETE FROM meta WHERE k = 'indexed_subpath'", [])
        .unwrap();
    drop(conn);
    let moved = tmp.path().join("moved");
    fs::rename(&dir, &moved).unwrap();
    let out = estate(&moved, &["changed-since", &first, "--json", "--db", db]);
    assert_eq!(names(&out.stdout), Vec::<String>::new());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("1 indexed root(s) not placed in this checkout"),
        "{err}"
    );
    assert!(
        err.contains("2 changed file(s) under no root placed in this checkout"),
        "{err}"
    );
}

/// The recorded root replaced by a symlink into another repository is not followed: the root is
/// placed in this checkout at its recorded position.
#[cfg(unix)]
#[test]
fn a_root_replaced_by_a_symlink_is_placed_at_its_recorded_position() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, first) = repo(tmp.path());
    let db = tmp.path().join("graph.db");
    let db = db.to_str().unwrap();
    estate(&dir, &["index", "svc", "--db", db]);
    let moved = tmp.path().join("moved");
    fs::rename(&dir, &moved).unwrap();
    let other = tmp.path().join("other");
    // The symlink resolves to a directory where the root was: `svc` exists there too.
    fs::create_dir_all(other.join("svc")).unwrap();
    fs::write(other.join("svc/x.txt"), "x\n").unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "one"]);
    std::os::unix::fs::symlink(&other, &dir).unwrap();
    let out = estate(&moved, &["changed-since", &first, "--json", "--db", db]);
    let found = names(&out.stdout);
    for want in ["cancelOrder", "placeOrder"] {
        assert!(found.iter().any(|n| n == want), "{want}: {found:?}");
    }
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("1 changed file(s) outside the indexed root(s), not in this graph: README.md"),
        "{err}"
    );
}
