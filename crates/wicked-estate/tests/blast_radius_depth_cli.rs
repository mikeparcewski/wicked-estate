//! `blast-radius --depth` is bounded by the same ceiling as the MCP `BlastRadius` tool.
//!
//! The recursive walk grows with depth on a cyclic graph: on estate's own graph `--depth 100000`
//! ran past 20 s and 397 MB without answering. The CUT line invites the user to raise `--depth`,
//! so an unbounded flag was a hang one suggestion away. Above the ceiling the CLI refuses with a
//! message that names it.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_brdepth_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
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
fn depth_above_the_ceiling_is_refused_and_the_ceiling_itself_is_accepted() {
    let dir = scratch("cap");
    fs::write(
        dir.join("src/a.ts"),
        "export function f(): number { return 1; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/b.ts"),
        "import { f } from './a';\nexport function g(): number { return f(); }\n",
    )
    .unwrap();
    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    let src = dir.join("src");
    let out = run(&dir, &["index", src.to_str().unwrap(), "--db", db]);
    assert!(out.status.success(), "index failed: {out:?}");

    let ceiling = wicked_estate_retrieve::BLAST_DEPTH_CEILING.to_string();
    let ok = run(
        &dir,
        &["blast-radius", "f", "--depth", &ceiling, "--db", db],
    );
    assert!(
        ok.status.success(),
        "--depth at the ceiling must run: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let stdout = String::from_utf8_lossy(&ok.stdout);
    assert!(
        stdout.contains("depend on 'f'"),
        "expected g as a dependent: {stdout}"
    );

    let over = (wicked_estate_retrieve::BLAST_DEPTH_CEILING + 1).to_string();
    for args in [
        vec!["blast-radius", "f", "--depth", over.as_str(), "--db", db],
        vec!["blast-radius", "f", "--depth=100000", "--db", db],
    ] {
        let out = run(&dir, &args);
        assert!(
            !out.status.success(),
            "{args:?} must be refused, got stdout={}",
            String::from_utf8_lossy(&out.stdout)
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains(&format!("maximum of {ceiling}")),
            "the error must name the ceiling: {err}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
