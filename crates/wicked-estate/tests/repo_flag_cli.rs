//! `--repo` is the one value flag on `index` whose missing value CANNOT degrade quietly.
//!
//! Every other value flag falls back to a visible default — `--db` to the default path, `--budget`
//! to its number. `--repo` falls back to "index un-labelled", which is a DIFFERENT WRITE to the
//! graph than the caller asked for: bare paths instead of `<label>/…`, a graph-wide delete sweep
//! instead of a scoped one, and the singular `repo_*` provenance keys instead of the per-repo
//! ones. It also reports success while doing it.
//!
//! `--repo --force` fell into that hole twice over: the parser consumed `--force` as the LABEL
//! (`--repo --force` → label `"--force"`) and the flag itself was dropped, so the run was neither
//! labelled the way it was asked nor forced.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_repoflag_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::create_dir_all(d.join("repo/src")).unwrap();
    fs::write(d.join("repo/src/index.ts"), "export function only() {}\n").unwrap();
    d
}

fn index_in(cwd: &PathBuf, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .arg("index")
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

fn refuses(dir: &PathBuf, args: &[&str], why: &str) {
    let out = index_in(dir, args);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "{why}\nargs={args:?}\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        stderr.contains("--repo") || stderr.contains("--as"),
        "the error must name the flag the caller got wrong: {stderr}"
    );
    assert!(
        !stdout.contains("indexed "),
        "it reported an index it must not have run: {stdout}"
    );
}

#[test]
fn repo_with_no_value_is_refused() {
    let dir = scratch("noval");
    let db = dir.join("g.db");
    refuses(
        &dir,
        &["repo", "--db", db.to_str().unwrap(), "--repo"],
        "`--repo` with nothing after it silently indexed un-labelled",
    );
    assert!(
        !db.exists(),
        "a refused run wrote a database; the next reader cannot tell it from a real one"
    );
}

#[test]
fn repo_followed_by_a_flag_is_refused_rather_than_labelling_the_repo_force() {
    let dir = scratch("flagval");
    let db = dir.join("g.db");
    refuses(
        &dir,
        &["repo", "--db", db.to_str().unwrap(), "--repo", "--force"],
        "`--force` was swallowed as the repo LABEL and dropped as a flag",
    );
    refuses(
        &dir,
        &["repo", "--repo", "--db", db.to_str().unwrap()],
        "`--db` was swallowed as the repo LABEL, sending the graph to the default path",
    );
    // `--as` is the same flag by another name and must not be the way around the check.
    refuses(
        &dir,
        &["repo", "--db", db.to_str().unwrap(), "--as", "--force"],
        "the `--as` alias skipped the check",
    );
}

/// The check must not cost the flag its job: a real label still labels, and a flag AFTER the label
/// is still a flag.
#[test]
fn a_real_label_still_indexes_and_the_following_flag_still_applies() {
    let dir = scratch("ok");
    let db = dir.join("g.db");
    let out = index_in(
        &dir,
        &[
            "repo",
            "--db",
            db.to_str().unwrap(),
            "--repo",
            "mine",
            "--force",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stdout={stdout} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("as 'mine'"),
        "the label did not reach the index run: {stdout}"
    );

    let q = Command::new(bin())
        .current_dir(&dir)
        .args(["query", "only", "--db", db.to_str().unwrap()])
        .output()
        .expect("spawn");
    let q = String::from_utf8_lossy(&q.stdout);
    assert!(
        q.contains("mine/src/index.ts"),
        "rows are not in the label's namespace: {q}"
    );
}
