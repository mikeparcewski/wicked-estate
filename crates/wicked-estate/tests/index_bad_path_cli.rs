//! FINDING-076 (#78): `index` must not report success for work it did not do.
//!
//! Walking a path that is not there yields zero files, and the old behaviour reported that as
//! `indexed <path> → 0 nodes, 0 edges, 0 files` with exit 0 — while creating the database. The
//! caller was handed a real, queryable, EMPTY graph and a success code, and every query against it
//! answered "nothing found", which is indistinguishable from a repository that genuinely contains
//! nothing.
//!
//! Two findings in the consuming platform ran on exactly that:
//!
//!   * wicked-core#170 — the indexed graph and the graph the worker queried were different files.
//!     Every query returned nothing, for months, looking like an empty repo.
//!   * wicked-crew#196 — concurrent registrations indexed the wrong repo. Caught only because three
//!     SQLite writers collided; without that, each run reports success for a repo it never opened.
//!
//! "Indexed a repo with no code" and "was handed a path that does not exist" are different answers
//! and need different exit codes.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_idxbad_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Run from a scratch cwd so a stray default-path database cannot land in the repo.
fn index_in(cwd: &PathBuf, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .arg("index")
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

#[test]
fn a_path_that_does_not_exist_fails_and_names_itself() {
    let dir = scratch("missing");
    let db = dir.join("g.db");
    let out = index_in(
        &dir,
        &["/no/such/directory/at/all", "--db", db.to_str().unwrap()],
    );

    assert!(
        !out.status.success(),
        "indexing a missing path reported SUCCESS — the failure mode this test exists for.\n\
         stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("/no/such/directory/at/all"),
        "the error must name the path the caller passed, since the caller's bug IS the path: {err}"
    );
    assert!(
        !db.exists(),
        "a failed index left a database behind; the next reader cannot tell it from a real empty repo"
    );
}

/// The shape that motivated this: an unsubstituted template value reaching the CLI as a path.
#[test]
fn an_unsubstituted_placeholder_fails_rather_than_indexing_nothing() {
    let dir = scratch("placeholder");
    let db = dir.join("g.db");
    let out = index_in(&dir, &["{repo_root}", "--db", db.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "`{{repo_root}}` was treated as a directory name and 'indexed' successfully"
    );
}

/// A real path with no indexable content is a legitimate answer, and must stay exit 0 — otherwise
/// the fix above just moves the confusion to the other side.
#[test]
fn an_empty_but_real_directory_still_succeeds() {
    let dir = scratch("empty");
    let target = dir.join("nothing-here");
    fs::create_dir_all(&target).unwrap();
    let db = dir.join("g.db");
    let out = index_in(
        &dir,
        &[target.to_str().unwrap(), "--db", db.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "an existing directory with no code must still index cleanly: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// `--help` is a flag. It used to fall through to the positional catch-all, so `index --help`
/// indexed a directory named `--help` — and wrote a database into the caller's cwd doing it.
#[test]
fn help_prints_usage_instead_of_indexing_a_directory_called_help() {
    let dir = scratch("help");
    let out = index_in(&dir, &["--help"]);
    assert!(out.status.success(), "`index --help` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("usage:"),
        "expected usage output, got: {stdout}"
    );
    assert!(
        !stdout.contains("indexed --help"),
        "`--help` was treated as a path: {stdout}"
    );
    assert!(
        !dir.join(".wicked-estate").exists(),
        "`--help` wrote a database into the caller's working directory"
    );
}
