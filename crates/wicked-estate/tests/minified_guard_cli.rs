// SPDX-License-Identifier: MIT
//! #478: a large hand-written file is indexed (size is never a minification signal), and a real
//! minified file is skipped and named in the index output.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_minguard_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run(cwd: &PathBuf, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

#[test]
fn large_handwritten_rust_is_indexed_and_minified_is_skipped_and_named() {
    let dir = scratch("both");
    let repo = dir.join("repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    // ~1.5 MiB of short lines, 3 per function: well past 1 MiB, nothing minified about it.
    let mut big = String::new();
    for i in 0..30_000 {
        big.push_str(&format!(
            "pub fn handwritten_{i}() -> u32 {{\n    {i}\n}}\n"
        ));
    }
    assert!(big.len() > 1_048_576);
    fs::write(repo.join("src/actor.rs"), &big).unwrap();
    // A real minified file: one enormous line.
    let min = format!("var a=[{}];", "1,".repeat(40_000));
    fs::write(repo.join("bundle.min.js"), &min).unwrap();

    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    let out = run(&dir, &["index", repo.to_str().unwrap(), "--db", db]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "index failed: {err}");
    assert!(
        err.contains("SKIPPED_MINIFIED: 1 file(s)"),
        "count line missing: {err}"
    );
    assert!(
        err.contains("SKIPPED_MINIFIED: bundle.min.js"),
        "the skipped file must be named: {err}"
    );
    assert!(
        !err.contains("actor.rs"),
        "the hand-written file must not be skipped: {err}"
    );

    let q = run(&dir, &["query", "handwritten_29999", "--db", db]);
    let stdout = String::from_utf8_lossy(&q.stdout);
    assert!(
        stdout.contains("handwritten_29999"),
        "symbol from the large file is not queryable: {stdout}"
    );
}
