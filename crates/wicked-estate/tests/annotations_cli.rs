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

/// Collect every annotation across the `annotations <name> --json` (array-of-symbol) shape.
fn anns_for_name(dir: &Path, db: &Path, name: &str) -> Vec<serde_json::Value> {
    let out = run(dir, db, &["annotations", name, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("annotations --json is JSON");
    v.as_array()
        .expect("array under <name>")
        .iter()
        .flat_map(|o| o["annotations"].as_array().unwrap().clone())
        .collect()
}

#[test]
fn annotate_replace_upserts_by_type_key() {
    // --replace twice with the SAME (type, key) but a different value → exactly ONE row,
    // carrying the latest value (idempotent upsert, not append).
    let (dir, db) = index_one_fn("replace_upsert", "fn target() {}\n");

    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "cache",
            "--key",
            "k",
            "--value",
            "first",
            "--replace",
        ],
    );
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "cache",
            "--key",
            "k",
            "--value",
            "second",
            "--replace",
        ],
    );

    let anns = anns_for_name(&dir, &db, "target");
    let cache: Vec<&serde_json::Value> = anns.iter().filter(|a| a["type"] == "cache").collect();
    assert_eq!(
        cache.len(),
        1,
        "--replace upserts by (type,key): exactly one row, got {anns:?}"
    );
    assert_eq!(cache[0]["key"], "k");
    assert_eq!(cache[0]["value"], "second", "latest value wins");
}

#[test]
fn annotate_without_replace_appends() {
    // Two plain annotate calls with the same (type, key) → TWO rows (append unchanged).
    let (dir, db) = index_one_fn("no_replace_append", "fn target() {}\n");

    run(
        &dir,
        &db,
        &[
            "annotate", "target", "--type", "cache", "--key", "k", "--value", "first",
        ],
    );
    run(
        &dir,
        &db,
        &[
            "annotate", "target", "--type", "cache", "--key", "k", "--value", "second",
        ],
    );

    let anns = anns_for_name(&dir, &db, "target");
    let cache: Vec<&serde_json::Value> = anns.iter().filter(|a| a["type"] == "cache").collect();
    assert_eq!(
        cache.len(),
        2,
        "append is the default: two rows survive, got {anns:?}"
    );
}

#[test]
fn annotate_replace_only_affects_matching_type_key() {
    // --replace on (cache, k1) must NOT touch a different key (cache, k2) NOR a different
    // type under the same key (note, k1) on the same symbol.
    let (dir, db) = index_one_fn("replace_scoped", "fn target() {}\n");

    // A different key, same type — must survive.
    run(
        &dir,
        &db,
        &[
            "annotate", "target", "--type", "cache", "--key", "k2", "--value", "keepme",
        ],
    );
    // Same key, different type — must survive.
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "note",
            "--key",
            "k1",
            "--value",
            "keepme-note",
        ],
    );
    // Seed (cache, k1), then replace it.
    run(
        &dir,
        &db,
        &[
            "annotate", "target", "--type", "cache", "--key", "k1", "--value", "old",
        ],
    );
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--type",
            "cache",
            "--key",
            "k1",
            "--value",
            "new",
            "--replace",
        ],
    );

    let anns = anns_for_name(&dir, &db, "target");

    // (cache, k1) collapsed to one row with the new value.
    let cache_k1: Vec<&serde_json::Value> = anns
        .iter()
        .filter(|a| a["type"] == "cache" && a["key"] == "k1")
        .collect();
    assert_eq!(cache_k1.len(), 1, "replaced (cache,k1) is a single row");
    assert_eq!(cache_k1[0]["value"], "new");

    // (cache, k2) — different key — untouched.
    let cache_k2: Vec<&serde_json::Value> = anns
        .iter()
        .filter(|a| a["type"] == "cache" && a["key"] == "k2")
        .collect();
    assert_eq!(cache_k2.len(), 1, "different key survives the replace");
    assert_eq!(cache_k2[0]["value"], "keepme");

    // (note, k1) — same key, different type — untouched.
    let note_k1: Vec<&serde_json::Value> = anns
        .iter()
        .filter(|a| a["type"] == "note" && a["key"] == "k1")
        .collect();
    assert_eq!(
        note_k1.len(),
        1,
        "same key under a different type survives the replace"
    );
    assert_eq!(note_k1[0]["value"], "keepme-note");
}

#[test]
fn clusters_annotate_is_idempotent() {
    // Re-running `clusters --annotate` must REPLACE (not duplicate) each member's community
    // annotation: after two runs every annotated symbol has exactly ONE `community` row.
    let src = "\
fn a() { b(); c(); }
fn b() { c(); a(); }
fn c() { a(); b(); }
";
    let (dir, db) = index_one_fn("clusters_idem", src);

    run(&dir, &db, &["clusters", "--annotate"]);
    run(&dir, &db, &["clusters", "--annotate"]);

    let after = run(
        &dir,
        &db,
        &["nodes", "--annotated-with", "community", "--json"],
    );
    let after_v: serde_json::Value = serde_json::from_str(&after).unwrap();
    let annotated = after_v.as_array().unwrap();
    assert!(
        !annotated.is_empty(),
        "at least one member annotated with community after two runs"
    );

    // Each annotated symbol carries exactly one `community` annotation (no duplicate).
    for node in annotated {
        let sym = node["symbol_id"].as_str().unwrap();
        let detail = run(&dir, &db, &["annotations", "--symbol", sym, "--json"]);
        let dv: serde_json::Value = serde_json::from_str(&detail).unwrap();
        let community_rows: Vec<&serde_json::Value> = dv["annotations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["type"] == "community")
            .collect();
        assert_eq!(
            community_rows.len(),
            1,
            "symbol {sym} has exactly one community annotation after two runs, got {community_rows:?}"
        );
    }
}

/// Run `wicked-estate <args> --db <db>` from `cwd`; assert it FAILS; return stderr.
fn run_fail(cwd: &Path, db: &Path, args: &[&str]) -> String {
    let out = Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .args(["--db", db.to_str().unwrap()])
        .output()
        .expect("spawn wicked-estate");
    assert!(
        !out.status.success(),
        "command {args:?} unexpectedly succeeded: stdout={}",
        String::from_utf8_lossy(&out.stdout),
    );
    String::from_utf8(out.stderr).expect("utf8 stderr")
}

/// #204: every `--json` arm that renders an annotation carries the evidence envelope, so a reader
/// never has `ts` (write time) as the only clock and infers "verified just now". The decisive case
/// is `stale-annotations`: its human and JSON renderings must agree the row was never verified.
#[test]
fn every_json_arm_carries_the_evidence_envelope() {
    let (dir, db) = index_one_fn("envelope", "fn target() {}\n");
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--key",
            "owner",
            "--value",
            "payments-team",
        ],
    );
    let by_name: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["annotations", "target", "--json"])).unwrap();
    let id = by_name[0]["symbol"].as_str().unwrap().to_string();

    let assert_envelope = |arm: &str, a: &serde_json::Value| {
        assert_eq!(a["key"], "owner", "{arm}: wrong annotation");
        assert!(a["ts"].as_i64().unwrap() > 0, "{arm}: ts is the write time");
        assert_eq!(a["last_verified"], 0, "{arm}: never verified is explicit 0");
        assert_eq!(a["source_type"], "unspecified", "{arm}: source_type");
        assert_eq!(a["extraction_method"], "manual", "{arm}: extraction_method");
    };

    let human = run(&dir, &db, &["stale-annotations", "9999999999"]);
    assert!(human.contains("last_verified=0"), "human line: {human}");
    let stale: serde_json::Value = serde_json::from_str(&run(
        &dir,
        &db,
        &["stale-annotations", "9999999999", "--json"],
    ))
    .unwrap();
    assert_envelope("stale-annotations", &stale[0]["annotation"]);

    assert_envelope("annotations <name>", &by_name[0]["annotations"][0]);
    let by_id: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["annotations", "--symbol", &id, "--json"])).unwrap();
    assert_envelope("annotations --symbol", &by_id["annotations"][0]);

    let nodes: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["nodes", "--json"])).unwrap();
    let node = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["annotations"].is_array())
        .expect("annotated node in nodes --json");
    assert_envelope("nodes", &node["annotations"][0]);

    let bundle: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["source", "--symbols", &id, "--json"])).unwrap();
    assert_envelope("source", &bundle["nodes"][0]["annotations"][0]);
}

/// #205: the cutoff is given exactly one way. The old `find_map` took the first parseable token
/// anywhere, so a stray or misordered operand silently set the cutoff.
#[test]
fn stale_annotations_cutoff_takes_exactly_one_spelling() {
    let (dir, db) = index_one_fn("cutoff", "fn target() {}\n");
    for args in [
        &["stale-annotations"][..],
        &["stale-annotations", "2026-02-30"],
        &["stale-annotations", "soon", "100"],
        &["stale-annotations", "100", "200"],
        &["stale-annotations", "100", "--older-than", "90d"],
        &["stale-annotations", "--older-than", "90"],
    ] {
        let err = run_fail(&dir, &db, args);
        assert!(
            err.contains("<cutoff-unix-seconds | YYYY-MM-DD>") && err.contains("--older-than"),
            "{args:?} names every accepted spelling: {err}"
        );
    }
}

/// #205: a date and a window are real cutoffs, not decoration. A never-verified row
/// (`last_verified = 0`) is stale for any cutoff after the epoch and for none at or before it.
#[test]
fn stale_annotations_accepts_a_date_and_a_window() {
    let (dir, db) = index_one_fn("cutoff_forms", "fn target() {}\n");
    run(
        &dir,
        &db,
        &["annotate", "target", "--key", "owner", "--value", "x"],
    );
    let rows = |args: &[&str]| -> usize {
        let mut a = vec!["stale-annotations"];
        a.extend_from_slice(args);
        a.push("--json");
        let v: serde_json::Value = serde_json::from_str(&run(&dir, &db, &a)).unwrap();
        v.as_array().unwrap().len()
    };
    assert_eq!(rows(&["2026-01-01"]), 1, "a date after the epoch");
    assert_eq!(
        rows(&["1970-01-01"]),
        0,
        "1970-01-01 is cutoff 0: last_verified 0 is not < 0"
    );
    assert_eq!(rows(&["--older-than", "1s"]), 1, "a window ending now");
    assert_eq!(
        rows(&["--older-than", "3000w"]),
        0,
        "a window reaching before 1970"
    );

    let human = run(&dir, &db, &["stale-annotations", "2026-01-01"]);
    assert!(
        human.contains("cutoff 1767225600 (2026-01-01T00:00:00Z)"),
        "human line echoes the resolved instant: {human}"
    );
}

/// #205: the banner names the unit the parser demands.
#[test]
fn help_banner_states_the_cutoff_unit() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("stale-annotations <cutoff-unix-seconds | YYYY-MM-DD>")
            && help.contains("stale-annotations --older-than <N>{s,m,h,d,w}"),
        "banner: {help}"
    );
}

/// `annotate` writes the evidence envelope, and `stale-annotations` judges the row by the clock
/// it was written with — before this a CLI-written row was always `last_verified = 0`, so no
/// CLI user could ever record a re-verification.
#[test]
fn annotate_writes_the_evidence_envelope() {
    let (dir, db) = index_one_fn("ann_envelope", "fn target() {}\n");
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--key",
            "owner",
            "--value",
            "payments-team",
            "--source-type",
            "sme-answer",
            "--extraction-method",
            "interview@2026-01",
            "--last-verified",
            "2026-01-01",
        ],
    );
    let v: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["annotations", "target", "--json"])).unwrap();
    let a = &v[0]["annotations"][0];
    assert_eq!(a["source_type"], "sme-answer");
    assert_eq!(a["extraction_method"], "interview@2026-01");
    assert_eq!(a["last_verified"], 1_767_225_600_i64);

    let stale = |cutoff: &str| -> usize {
        let out = run(&dir, &db, &["stale-annotations", cutoff, "--json"]);
        serde_json::from_str::<serde_json::Value>(&out)
            .unwrap()
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!(stale("2025-06-01"), 0, "verified after this cutoff → fresh");
    assert_eq!(
        stale("2026-06-01"),
        1,
        "verified before this cutoff → stale"
    );

    // `now` is fresh for any window that ends now.
    run(
        &dir,
        &db,
        &[
            "annotate",
            "target",
            "--key",
            "owner",
            "--value",
            "p",
            "--last-verified",
            "now",
            "--replace",
        ],
    );
    let out = run(
        &dir,
        &db,
        &["stale-annotations", "--older-than", "1d", "--json"],
    );
    assert_eq!(out.trim(), "[]", "just verified → not stale: {out}");
}

#[test]
fn annotate_refuses_a_degenerate_envelope() {
    let (dir, db) = index_one_fn("ann_envelope_bad", "fn target() {}\n");
    let base = ["annotate", "target", "--key", "k", "--value", "v"];
    for extra in [
        &["--source-type", ""][..],
        &["--extraction-method", ""],
        &["--last-verified", "yesterday"],
        &["--last-verified", "-1"],
        &["--last-verified", "2026-02-30"],
    ] {
        let mut args = base.to_vec();
        args.extend_from_slice(extra);
        let err = run_fail(&dir, &db, &args);
        assert!(err.contains(extra[0]), "{extra:?} names the flag: {err}");
    }
    // Nothing was written by any refused call.
    let v: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["annotations", "target", "--json"])).unwrap();
    assert!(v[0]["annotations"].as_array().unwrap().is_empty());
}

/// A malformed numeric flag value is a usage error, not the default. `--confidence high` used to
/// store 1.0 — a confident fact from a typo — and `source --max-total-chars 10k` ran unbounded.
#[test]
fn numeric_flag_values_are_strict() {
    let (dir, db) = index_one_fn("numeric", "fn target() {}\n");
    let base = [
        "annotate",
        "target",
        "--key",
        "k",
        "--value",
        "v",
        "--confidence",
    ];
    for bad in ["high", "1.5", "-0.1", "NaN"] {
        let mut args = base.to_vec();
        args.push(bad);
        let err = run_fail(&dir, &db, &args);
        assert!(err.contains("--confidence"), "{bad:?}: {err}");
    }
    let mut ok = base.to_vec();
    ok.push("0.4");
    run(&dir, &db, &ok);
    let v: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["annotations", "target", "--json"])).unwrap();
    let anns = v[0]["annotations"].as_array().unwrap();
    assert_eq!(anns.len(), 1, "only the valid call wrote a row");
    assert_eq!(anns[0]["confidence"], 0.4);

    for (args, flag) in [
        (
            &[
                "source",
                "--file",
                "src/a.rs",
                "--json",
                "--max-total-chars",
                "10k",
            ][..],
            "--max-total-chars",
        ),
        (
            &[
                "source",
                "--file",
                "src/a.rs",
                "--json",
                "--max-node-chars",
                "x",
            ],
            "--max-node-chars",
        ),
        (&["graph-view", "--limit", "lots"], "--limit"),
        (&["context", "target", "--budget", "x"], "--budget"),
        (&["clusters", "big"], "<min_size>"),
        (&["clusters", "1", "2"], "at most one operand"),
    ] {
        let err = run_fail(&dir, &db, args);
        assert!(err.contains(flag), "{args:?}: {err}");
    }
}

/// PR #259 review: `--older-than '90é'` panicked (exit 101) — `split_at` on a byte offset inside a
/// multibyte char. Malformed input is a usage error, exit 1, for every byte shape; a panic would
/// also "fail", so the exit code itself is asserted.
#[test]
fn malformed_cutoffs_exit_1_never_panic() {
    let (dir, db) = index_one_fn("cutoff_utf8", "fn target() {}\n");
    for args in [
        &["stale-annotations", "--older-than", "90é"][..],
        &["stale-annotations", "--older-than", "é"],
        &["stale-annotations", "--older-than", "9日"],
        &["stale-annotations", "+100"],
        &["stale-annotations", "2026-01-0é"],
        &[
            "annotate",
            "target",
            "--key",
            "k",
            "--value",
            "v",
            "--last-verified",
            "1é",
        ],
    ] {
        let out = Command::new(bin())
            .current_dir(&dir)
            .args(args)
            .args(["--db", db.to_str().unwrap()])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(!stderr.contains("panicked"), "{args:?}: {stderr}");
    }
}
