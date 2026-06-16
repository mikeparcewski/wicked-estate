//! CLI integration tests for the typed-annotation surface (Chunks 2 + 4).
//!
//! These drive the *compiled `wicked-estate` binary* as a subprocess against a temp on-disk DB,
//! so they exercise the real wiring: flag parsing → `GraphWrite::annotate` → `GraphRead::annotations`
//! → JSON shaping. Unit-level shape/cap/ordering coverage lives in `src/source_bundle.rs`; this
//! file proves the end-to-end CLI contract from `docs/recon/annotation-consumer-spec.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the compiled `wicked-estate` binary (Cargo sets this for integration tests).
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

/// A fresh temp dir with a `src/` subdir; the on-disk DB lives at `<dir>/g.db`.
fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_anncli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

/// Run `wicked-estate <args> --db <db>` from `cwd`; assert success; return stdout.
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

/// Index a single-function source file and return (dir, db_path).
fn index_one_fn(tag: &str, src: &str) -> (PathBuf, PathBuf) {
    let dir = fresh_dir(tag);
    fs::write(dir.join("src/a.rs"), src).unwrap();
    let db = dir.join("g.db");
    run(&dir, &db, &["index", dir.to_str().unwrap()]);
    (dir, db)
}

#[test]
fn annotate_typed_roundtrips_via_annotations_json() {
    let (dir, db) = index_one_fn("rt", "fn target() {}\n");

    // Write a typed assumption with explicit confidence/provenance/author.
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "assumption",
            "--key",
            "thread-safety",
            "--value",
            "assumed Send+Sync",
            "--confidence",
            "0.7",
            "--provenance",
            "manual",
            "--author",
            "alice",
        ],
    );

    let stdout = run(&dir, &db, &["annotations", "target", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("annotations --json is JSON");
    // <name> path → array of {symbol, annotations:[...]} objects.
    let arr = v.as_array().expect("array under <name>");
    // Exactly one symbol named `target`, carrying one annotation.
    let with_ann: Vec<&serde_json::Value> = arr
        .iter()
        .filter(|o| !o["annotations"].as_array().unwrap().is_empty())
        .collect();
    assert_eq!(
        with_ann.len(),
        1,
        "exactly one symbol carries the annotation"
    );
    let obj = with_ann[0];
    assert!(obj["symbol"].is_string(), "per-symbol object has `symbol`");
    let anns = obj["annotations"].as_array().unwrap();
    assert_eq!(anns.len(), 1);
    let a = &anns[0];
    assert_eq!(a["type"], "assumption");
    assert_eq!(a["key"], "thread-safety");
    assert_eq!(a["value"], "assumed Send+Sync");
    assert_eq!(a["confidence"], 0.7);
    assert_eq!(a["provenance"], "manual");
    assert_eq!(a["author"], "alice");
    assert!(a["ts"].as_i64().unwrap() > 0, "store stamped ts");
    assert_eq!(a["advisory"], true, "assumption is advisory");
}

#[test]
fn annotate_defaults_to_note_type() {
    let (dir, db) = index_one_fn("dflt", "fn target() {}\n");
    // No --type → defaults to note (back-compat).
    run(
        &dir,
        &db,
        &["annotate", "target", "--key", "owner", "--value", "team-x"],
    );
    let stdout = run(&dir, &db, &["annotations", "target", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let a = v
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|o| o["annotations"].as_array().unwrap().clone())
        .next()
        .expect("one annotation");
    assert_eq!(a["type"], "note", "default type is note");
    assert_eq!(a["advisory"], false, "note is not advisory");
}

#[test]
fn type_filter_narrows_annotations() {
    let (dir, db) = index_one_fn("filter", "fn target() {}\n");
    // Two annotations of different types on the same symbol.
    run(
        &dir,
        &db,
        &[
            "annotate", "target", "--type", "note", "--key", "k1", "--value", "v1",
        ],
    );
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "assumption",
            "--key",
            "k2",
            "--value",
            "v2",
        ],
    );

    // No filter → both.
    let all: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["annotations", "target", "--json"])).unwrap();
    let all_count: usize = all
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["annotations"].as_array().unwrap().len())
        .sum();
    assert_eq!(all_count, 2, "both annotations without a filter");

    // --type assumption → only the assumption.
    let only: serde_json::Value = serde_json::from_str(&run(
        &dir,
        &db,
        &["annotations", "target", "--type", "assumption", "--json"],
    ))
    .unwrap();
    let only_anns: Vec<serde_json::Value> = only
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|o| o["annotations"].as_array().unwrap().clone())
        .collect();
    assert_eq!(only_anns.len(), 1, "filter keeps only the assumption");
    assert_eq!(only_anns[0]["type"], "assumption");
    assert_eq!(only_anns[0]["key"], "k2");
}

#[test]
fn question_is_advisory() {
    let (dir, db) = index_one_fn("q", "fn target() {}\n");
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "question",
            "--key",
            "why",
            "--value",
            "is this reachable?",
        ],
    );
    let v: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["annotations", "target", "--json"])).unwrap();
    let a = v
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|o| o["annotations"].as_array().unwrap().clone())
        .next()
        .unwrap();
    assert_eq!(a["type"], "question");
    assert_eq!(a["advisory"], true, "question is advisory");
}

#[test]
fn nodes_json_carries_annotation_summary_and_array() {
    let (dir, db) = index_one_fn("nodes", "fn target() {}\n");
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "assumption",
            "--key",
            "k",
            "--value",
            "v",
        ],
    );
    let v: serde_json::Value = serde_json::from_str(&run(&dir, &db, &["nodes", "--json"])).unwrap();
    let nodes = v.as_array().unwrap();
    // The `target` function node carries the annotation summary + array.
    let target = nodes
        .iter()
        .find(|n| n["name"] == "target")
        .expect("target node present");
    assert_eq!(target["annotation_summary"]["count"], 1);
    assert_eq!(target["annotation_summary"]["has_advisory"], true);
    assert_eq!(target["annotation_summary"]["by_type"]["assumption"], 1);
    let anns = target["annotations"].as_array().expect("annotations array");
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0]["advisory"], true);

    // A node WITHOUT annotations still has a summary (count 0) and omits the array.
    let bare = nodes
        .iter()
        .find(|n| n["name"] != "target" && n["annotation_summary"]["count"] == 0);
    if let Some(b) = bare {
        assert!(
            b.get("annotations").is_none(),
            "annotations omitted when empty: {b}"
        );
    }
}

#[test]
fn clusters_annotate_writes_community_annotations() {
    // A connected call graph so Louvain finds at least one community of size >= 2.
    let src = "\
fn a() { b(); c(); }
fn b() { c(); a(); }
fn c() { a(); b(); }
";
    let (dir, db) = index_one_fn("clusters", src);

    // Default (no --annotate) must NOT write any community annotation.
    run(&dir, &db, &["clusters"]);
    let before = run(
        &dir,
        &db,
        &["nodes", "--annotated-with", "community", "--json"],
    );
    let before_v: serde_json::Value = serde_json::from_str(&before).unwrap();
    assert_eq!(
        before_v.as_array().unwrap().len(),
        0,
        "clusters is read-only without --annotate"
    );

    // Opt-in: --annotate writes a `community`-type annotation on each member.
    let report = run(&dir, &db, &["clusters", "--annotate"]);
    assert!(
        report.contains("type=community"),
        "report mentions the community write: {report}"
    );

    // Every annotated node carries a community annotation authored by "system".
    let after = run(
        &dir,
        &db,
        &["nodes", "--annotated-with", "community", "--json"],
    );
    let after_v: serde_json::Value = serde_json::from_str(&after).unwrap();
    let annotated = after_v.as_array().unwrap();
    assert!(
        !annotated.is_empty(),
        "at least one member annotated with community"
    );
    // Inspect one annotated symbol's annotations to confirm type/author/key.
    let sym = annotated[0]["symbol_id"].as_str().unwrap();
    let detail = run(&dir, &db, &["annotations", "--symbol", sym, "--json"]);
    let dv: serde_json::Value = serde_json::from_str(&detail).unwrap();
    let community = dv["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["type"] == "community")
        .expect("a community annotation");
    assert_eq!(community["key"], "community");
    assert_eq!(community["author"], "system");
    assert_eq!(
        community["advisory"], false,
        "community is system-derived, not advisory"
    );
}
