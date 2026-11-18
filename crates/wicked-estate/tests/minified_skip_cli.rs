// SPDX-License-Identifier: MIT
//! #478: a file is skipped as minified only on minification signals, never on size, and every
//! skipped file is named in the index output.
//!
//! A 1.7 MB hand-written Rust file (many short lines) was skipped as "minified", its symbols never
//! reached the graph, and blast radius silently missed its callers. The output carried only a count.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_minskip_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

fn run(dir: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(dir)
        .args(args)
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-minified-skip",
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

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn large_hand_written_file_is_indexed_and_queryable() {
    let dir = scratch("big");
    let mut src = String::new();
    // ~1.7 MB of short lines, each function distinct.
    for i in 0..30_000 {
        src.push_str(&format!(
            "pub fn handler_{i}() {{\n    let _ = {i};\n}}\n\n"
        ));
    }
    assert!(
        src.len() > 1_048_576,
        "fixture must exceed 1 MiB: {}",
        src.len()
    );
    fs::write(dir.join("src/actor.rs"), &src).unwrap();

    let out = run(&dir, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "index failed: {}", stderr(&out));
    assert!(
        !stderr(&out).contains("SKIPPED_MINIFIED"),
        "hand-written source must not be skipped: {}",
        stderr(&out)
    );

    let q = run(
        &dir,
        &["query", "handler_29999", "--db", "graph.db", "--json"],
    );
    assert!(q.status.success(), "query failed: {}", stderr(&q));
    assert!(
        stdout(&q).contains("handler_29999"),
        "symbol from the large file must be in the graph: {}",
        stdout(&q)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn real_minified_file_is_still_skipped_and_named() {
    let dir = scratch("min");
    // One enormous line: the longest-line and average signals both fire.
    let mut min = String::new();
    while min.len() < 120_000 {
        min.push_str("function a(b){return b+1}var c=a(2);");
    }
    fs::write(dir.join("src/bundle.js"), &min).unwrap();
    fs::write(dir.join("src/lib.rs"), "pub fn kept() {}\n").unwrap();

    let out = run(&dir, &["index", ".", "--db", "graph.db"]);
    assert!(out.status.success(), "index failed: {}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("SKIPPED_MINIFIED: 1 file(s)"),
        "the count line stays: {err}"
    );
    assert!(
        err.lines().any(|l| l == "SKIPPED_MINIFIED: src/bundle.js"),
        "the skipped file must be named: {err}"
    );
    assert!(
        !err.contains("src/lib.rs"),
        "a normal file is not named: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}
