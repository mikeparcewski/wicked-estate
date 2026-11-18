//! `blast-radius --id <symbol-id>` answers for ONE symbol; the name form still unions (#451).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn run(cwd: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

fn json(out: &Output) -> serde_json::Value {
    assert!(out.status.success(), "failed: {out:?}");
    serde_json::from_slice(&out.stdout).expect("one JSON document")
}

/// The caller FUNCTIONS among the dependents (File rows ride along and are not the claim).
fn dependent_names(v: &serde_json::Value) -> Vec<String> {
    let mut n: Vec<String> = v["dependents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .filter(|n| n.starts_with("caller"))
        .collect();
    n.sort();
    n
}

#[test]
fn id_returns_one_symbols_dependents_and_the_name_form_still_unions() {
    let dir = std::env::temp_dir().join(format!("ci_brid_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    for (f, body) in [
        ("a.ts", "export function dup(): number { return 1; }\n"),
        ("b.ts", "export function dup(): number { return 2; }\n"),
        (
            "ca.ts",
            "import { dup } from './a';\nexport function callerA(): number { return dup(); }\n",
        ),
        (
            "cb.ts",
            "import { dup } from './b';\nexport function callerB(): number { return dup(); }\n",
        ),
    ] {
        fs::write(dir.join("src").join(f), body).unwrap();
    }
    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    let src = dir.join("src");
    assert!(
        run(&dir, &["index", src.to_str().unwrap(), "--db", db])
            .status
            .success()
    );

    let union = json(&run(&dir, &["blast-radius", "dup", "--json", "--db", db]));
    assert_eq!(dependent_names(&union), ["callerA", "callerB"]);

    let rows = json(&run(&dir, &["query", "dup", "--json", "--db", db]));
    let ids: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["symbol_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2, "two symbols share the name: {rows}");

    let mut seen = Vec::new();
    for id in &ids {
        let one = json(&run(
            &dir,
            &["blast-radius", "--id", id, "--json", "--db", db],
        ));
        assert_eq!(one["resolved"], true);
        assert_eq!(one["target"], *id);
        let names = dependent_names(&one);
        assert_eq!(names.len(), 1, "--id must not union: {one}");
        seen.extend(names);
    }
    seen.sort();
    assert_eq!(seen, ["callerA", "callerB"]);

    // An unknown id is a miss, not a leaf; the id and the name together are refused.
    let miss = json(&run(
        &dir,
        &["blast-radius", "--id", "nope", "--json", "--db", db],
    ));
    assert_eq!(miss["resolved"], false);
    assert!(
        !run(&dir, &["blast-radius", "dup", "--id", ids[0], "--db", db])
            .status
            .success()
    );
    let _ = fs::remove_dir_all(&dir);
}
