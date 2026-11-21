//! Atomic CLI writes and explicit refusal of ids that are not nodes (garden#1453, garden#1470).

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Fixture {
    dir: PathBuf,
    db: PathBuf,
    spool: PathBuf,
    first: String,
    second: String,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ci_write_batch_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/a.rs"), "fn first() {}\nfn second() {}\n").unwrap();
        let mut fixture = Self {
            db: dir.join("graph.db"),
            spool: dir.join("events.ndjson"),
            dir,
            first: String::new(),
            second: String::new(),
        };
        fixture.success(&["index", "."]);
        fs::remove_file(&fixture.spool).unwrap();
        fixture.first = fixture.resolve("first");
        fixture.second = fixture.resolve("second");
        fixture
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate"));
        cmd.current_dir(&self.dir)
            .args(args)
            .arg("--db")
            .arg(&self.db)
            .env_remove("WICKED_ESTATE_DB")
            .env_remove("WICKED_STORE_URL")
            .env_remove("WICKED_RUNTIME")
            .env("WICKED_ESTATE_EMIT_DEADLETTER", &self.spool)
            .env(
                "WICKED_ESTATE_EMIT_PROGRAM",
                self.dir.join("nonexistent-wicked-bus"),
            );
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("WICKED_OTEL_") {
                cmd.env_remove(key);
            }
        }
        cmd
    }

    fn success(&self, args: &[&str]) -> String {
        let out = self.command(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    fn resolve(&self, name: &str) -> String {
        let out = self.success(&["resolve", name, "--json"]);
        let rows: Value = serde_json::from_str(&out).unwrap();
        rows[0]["symbol_id"].as_str().unwrap().to_string()
    }

    fn annotations(&self, symbol: &str) -> Vec<Value> {
        let out = self.success(&["annotations", "--symbol", symbol, "--json"]);
        let rows: Value = serde_json::from_str(&out).unwrap();
        rows["annotations"].as_array().unwrap().clone()
    }

    fn batch(&self, text: &str, json_out: bool) -> Output {
        let path = self.dir.join("writes.ndjson");
        fs::write(&path, text).unwrap();
        let mut cmd = self.command(&["write-batch", path.to_str().unwrap()]);
        if json_out {
            cmd.arg("--json");
        }
        cmd.output().unwrap()
    }

    fn events(&self) -> Vec<Value> {
        fs::read_to_string(&self.spool)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn batch_writes_in_order_replaces_and_emits_once() {
    let f = Fixture::new("roundtrip");
    let rows = [
        json!({"op":"annotate", "symbol":f.first, "key":"owner", "value":"old"}),
        json!({"op":"annotate", "symbol":f.second, "key":"team", "value":"estate",
            "type":"assumption", "confidence":0.7, "provenance":"manual", "author":"alice"}),
        json!({"op":"annotate", "symbol":f.first, "key":"owner", "value":"new",
            "replace":true}),
        json!({"op":"semantics", "symbol":f.second, "description":"Second function",
            "requirement":"REQ-1", "validated":true, "validated_by":"reviewer"}),
    ];
    let input = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let out = f.batch(&input, true);
    assert!(out.status.success(), "{out:?}");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report,
        json!({"rows":4, "annotations":3, "replaced":1, "semantics":1})
    );
    let first = f.annotations(&f.first);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["value"], "new");
    assert_eq!(first[0]["type"], "note");
    assert_eq!(first[0]["confidence"], 1.0);
    assert_eq!(first[0]["provenance"], "");
    assert_eq!(first[0]["author"], "");
    let second = f.annotations(&f.second);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["confidence"], 0.7);
    assert_eq!(second[0]["author"], "alice");
    let semantics = f.success(&["semantics", &f.second]);
    for expected in ["Second function", "REQ-1", "true", "reviewer"] {
        assert!(semantics.contains(expected), "{semantics}");
    }
    let events = f.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["type"], "wicked.estate.annotated");
    assert_eq!(events[0]["subdomain"], "estate.write-batch");
    assert_eq!(events[0]["payload"]["count"], 4);
    assert_eq!(events[0]["payload"]["annotations"], 3);
    assert_eq!(events[0]["payload"]["semantics"], 1);
    assert_eq!(events[0]["payload"]["replaced"], 1);
}

#[test]
fn unknown_symbol_refuses_the_whole_batch() {
    let f = Fixture::new("unknown");
    let good = json!({"op":"annotate", "symbol":f.first, "key":"k", "value":"v"});
    let bad = json!({"op":"semantics", "symbol":"missing-node", "requirement":"R"});
    let out = f.batch(&format!("{good}\n{bad}\n{bad}\n"), false);
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    for expected in [
        "row 2",
        "missing-node",
        "1 other row(s)",
        "nothing was written",
    ] {
        assert!(err.contains(expected), "{err}");
    }
    assert!(f.annotations(&f.first).is_empty());
    assert!(f.events().is_empty());
}

#[test]
fn malformed_rows_refuse_before_writing() {
    let mut f = Fixture::new("invalid");
    let good = json!({"op":"annotate", "symbol":f.first, "key":"k", "value":"v"});
    let bad_rows = [
        "{".to_string(),
        json!({"op":"anotate", "symbol":f.first}).to_string(),
        json!({"op":"annotate", "symbol":f.first, "key":"k", "value":"v",
            "typo":true})
        .to_string(),
        json!({"op":"annotate", "symbol":f.first, "key":"k", "value":"v",
            "confidence":1.5})
        .to_string(),
        json!({"op":"semantics", "symbol":f.first, "validated":true}).to_string(),
    ];
    for bad in bad_rows {
        let out = f.batch(&format!("{good}\n{bad}\n"), false);
        assert!(!out.status.success(), "{bad}: {out:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.contains("row 2"), "{err}");
        assert!(err.contains("nothing was written"), "{err}");
        assert!(f.annotations(&f.first).is_empty());
        assert!(f.events().is_empty());
    }
    f.db = f.dir.join("absent/graph.db");
    let out = f.batch(&format!("{good}\n{{\n"), false);
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("row 2"),
        "validation must precede opening the graph: {err}"
    );
    assert!(!f.dir.join("absent").exists());
    assert!(f.events().is_empty());
}

#[test]
fn a_store_error_rolls_back_replacements_and_semantics() {
    let f = Fixture::new("rollback");
    f.success(&[
        "annotate", "--symbol", &f.first, "--key", "owner", "--value", "old",
    ]);
    fs::remove_file(&f.spool).unwrap();
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER refuse_batch_annotation BEFORE INSERT ON annotations
         WHEN NEW.key = 'refuse'
         BEGIN SELECT RAISE(ABORT, 'injected annotation failure'); END;",
    )
    .unwrap();
    drop(conn);
    let rows = [
        json!({"op":"annotate", "symbol":f.first, "key":"owner", "value":"new",
            "replace":true}),
        json!({"op":"semantics", "symbol":f.second, "requirement":"rolled-back"}),
        json!({"op":"annotate", "symbol":f.second, "key":"refuse", "value":"v"}),
    ];
    let input = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let out = f.batch(&input, false);
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    for expected in [
        "row 3",
        "injected annotation failure",
        "nothing was written",
    ] {
        assert!(err.contains(expected), "{err}");
    }
    let annotations = f.annotations(&f.first);
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["value"], "old");
    assert!(f.annotations(&f.second).is_empty());
    assert!(
        f.success(&["semantics", &f.second])
            .contains("no semantics set")
    );
    assert!(f.events().is_empty());
}

#[test]
fn stdin_works_and_empty_input_emits_nothing() {
    let mut f = Fixture::new("stdin");
    let indexed_db = f.db.clone();
    f.db = f.dir.join("absent/graph.db");
    let empty = f.batch("\n  \n", false);
    assert!(empty.status.success(), "{empty:?}");
    assert_eq!(
        String::from_utf8(empty.stdout).unwrap().trim(),
        "wrote 0 row(s)"
    );
    assert!(f.events().is_empty());
    let empty_json = f.batch("", true);
    assert!(empty_json.status.success(), "{empty_json:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&empty_json.stdout).unwrap(),
        json!({"rows":0, "annotations":0, "replaced":0, "semantics":0})
    );
    assert!(f.events().is_empty());
    assert!(
        !f.dir.join("absent").exists(),
        "empty input must not take a writer lock"
    );
    f.db = indexed_db;
    let row = json!({"op":"annotate", "symbol":f.first, "key":"k", "value":"stdin"});
    let mut child = f
        .command(&["write-batch", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{row}").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "wrote 1 row(s): 1 annotation(s) (0 prior row(s) replaced), 0 semantics update(s)"
    );
    assert_eq!(f.annotations(&f.first)[0]["value"], "stdin");
    assert_eq!(f.events().len(), 1);
}

#[test]
fn interned_symbols_without_nodes_are_refused() {
    let f = Fixture::new("interned_only");
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute("INSERT INTO symbols(sym) VALUES (?1)", ["interned-only"])
        .unwrap();
    drop(conn);
    for args in [
        &[
            "annotate",
            "--symbol",
            "interned-only",
            "--key",
            "k",
            "--value",
            "v",
        ][..],
        &["semantics", "interned-only", "--requirement", "R"][..],
        &["semantics", "interned-only"][..],
    ] {
        let out = f.command(args).output().unwrap();
        assert!(!out.status.success(), "{args:?}: {out:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(
            err.contains("unknown symbol interned-only: not a node"),
            "{err}"
        );
    }
    let row = json!({"op":"annotate", "symbol":"interned-only", "key":"k", "value":"v"});
    let out = f.batch(&row.to_string(), false);
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("row 1"), "{err}");
    assert!(
        err.contains("unknown symbol interned-only: not a node"),
        "{err}"
    );
    assert!(f.annotations("interned-only").is_empty());
    assert!(f.events().is_empty());
}

#[test]
fn annotate_symbol_requires_a_node_and_real_ids_still_work() {
    let f = Fixture::new("annotate_node");
    let out = f
        .command(&[
            "annotate",
            "--symbol",
            "missing-node",
            "--key",
            "k",
            "--value",
            "v",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("annotate: unknown symbol missing-node: not a node"),
        "{err}"
    );
    assert!(err.contains("wicked-estate resolve <name> --json"), "{err}");
    assert!(f.annotations("missing-node").is_empty());
    assert!(f.annotations(&f.first).is_empty());
    assert!(f.events().is_empty());
    f.success(&[
        "annotate", "--symbol", &f.first, "--key", "k", "--value", "v",
    ]);
    assert_eq!(f.annotations(&f.first)[0]["value"], "v");
}

#[test]
fn semantics_read_and_write_require_a_node_and_real_ids_still_work() {
    let f = Fixture::new("semantics_node");
    for args in [
        &["semantics", "missing-node", "--requirement", "R"][..],
        &["semantics", "missing-node"][..],
    ] {
        let out = f.command(args).output().unwrap();
        assert!(!out.status.success(), "{out:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(
            err.contains("semantics: unknown symbol missing-node: not a node"),
            "{err}"
        );
        assert!(err.contains("wicked-estate resolve <name> --json"), "{err}");
    }
    assert!(f.events().is_empty());
    assert!(
        f.success(&["semantics", &f.first])
            .contains("no semantics set")
    );
    f.success(&["semantics", &f.first, "--requirement", "R"]);
    assert!(
        f.success(&["semantics", &f.first])
            .contains("requirement: R")
    );
}
