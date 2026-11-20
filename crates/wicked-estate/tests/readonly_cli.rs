//! Read commands must work on a current graph the process cannot write (#519).

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(dir)
        .args(args)
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-readonly-cli",
        )
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

/// Restore permissions even if an assertion fails, so the scratch directory can be removed.
struct RestorePermissions {
    dir: PathBuf,
    db: PathBuf,
    dir_permissions: fs::Permissions,
    db_permissions: fs::Permissions,
}

impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.dir, self.dir_permissions.clone());
        let _ = fs::set_permissions(&self.db, self.db_permissions.clone());
    }
}

fn entries(dir: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    names
}

#[test]
fn read_commands_accept_an_unwritable_graph_without_changing_it() {
    let uid = Command::new("id").arg("-u").output().expect("id -u");
    assert!(uid.status.success(), "id -u failed");
    if String::from_utf8(uid.stdout).unwrap().trim() == "0" {
        eprintln!("skipping permission test: root can write despite chmod");
        return;
    }

    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path();
    fs::write(
        dir.join("fixture.ts"),
        "export function caller() { return callee(); }\n\
         export function callee() { return 1; }\n",
    )
    .unwrap();
    for args in [
        vec!["index", ".", "--db", "graph.db"],
        vec!["stats", "--journal", "delete", "--db", "graph.db"],
    ] {
        let out = run(dir, &args);
        assert!(
            out.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let copy_dir = dir.join("readonly");
    fs::create_dir(&copy_dir).unwrap();
    let db = copy_dir.join("graph.db");
    fs::copy(dir.join("graph.db"), &db).unwrap();
    let _restore = RestorePermissions {
        dir: copy_dir.clone(),
        db: db.clone(),
        dir_permissions: fs::metadata(&copy_dir).unwrap().permissions(),
        db_permissions: fs::metadata(&db).unwrap().permissions(),
    };
    fs::set_permissions(&db, fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(&copy_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let before = fs::read(&db).unwrap();
    let files = entries(&copy_dir);

    for args in [
        vec!["query", "callee"],
        vec!["rank"],
        vec!["blast-radius", "callee"],
        vec!["traverse", "caller"],
        vec!["path", "caller", "callee"],
        vec!["lineage", "caller"],
        vec!["stats"],
    ] {
        let mut argv = args.clone();
        argv.extend(["--db", "readonly/graph.db"]);
        let out = run(dir, &argv);
        assert!(
            out.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.stdout.is_empty(), "{args:?} returned no output");
        assert_eq!(fs::read(&db).unwrap(), before, "{args:?} changed the graph");
        assert_eq!(entries(&copy_dir), files, "{args:?} created sidecar files");
    }
}
