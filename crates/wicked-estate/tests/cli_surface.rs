//! The CLI's machine-output and UX surface (#191, #199, #200, #202, #234, #244, #246, #247):
//! `--version` is one line; the usage banner is a complete inventory of the dispatch arms; every
//! read command fails closed on a missing `--db` and never creates it; the JSON arms survive a
//! reader that went away; `query --json` is `resolve --json`'s shape; `resolve` hides synthetic
//! value slots unless asked.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use wicked_estate_core::GraphRead;
use wicked_estate_store::SqliteStore;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_surface_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn wicked-estate")
}

/// A three-file TypeScript repo (`f` ← `g` ← `h`), indexed into `<dir>/g.db`.
fn indexed_chain(tag: &str) -> (PathBuf, String) {
    let dir = scratch(tag);
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
    fs::write(
        dir.join("src/c.ts"),
        "import { g } from './b';\nexport function h(): number { return g(); }\n",
    )
    .unwrap();
    let db = dir.join("g.db").to_str().unwrap().to_string();
    let src = dir.join("src");
    let out = run(&dir, &["index", src.to_str().unwrap(), "--db", &db]);
    assert!(out.status.success(), "index failed: {out:?}");
    (dir, db)
}

#[test]
fn version_flag_prints_one_line_and_exits_zero() {
    let dir = scratch("version");
    for flag in ["--version", "-V", "version"] {
        let out = run(&dir, &[flag]);
        assert!(out.status.success(), "{flag}: {out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            stdout,
            format!("wicked-estate {}\n", env!("CARGO_PKG_VERSION")),
            "{flag} must print exactly one line, not the banner"
        );
        assert!(out.stderr.is_empty(), "{flag}: {:?}", out.stderr);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The quoted command names a `match cmd { "name" => {` arm in `main.rs` (eight-space indent,
/// `|`-joined aliases included) and the bridged tools' `name:` entries in `tool_bridge.rs`.
fn dispatch_arms() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut arms = Vec::new();
    for line in fs::read_to_string(root.join("src/main.rs"))
        .unwrap()
        .lines()
    {
        let Some(rest) = line.strip_prefix("        \"") else {
            continue;
        };
        let Some((names, tail)) = rest.split_once(" => ") else {
            continue;
        };
        if tail.trim() != "{" {
            continue;
        }
        for n in names.split(" | ") {
            let n = n.trim_matches('"');
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                arms.push(n.to_string());
            }
        }
    }
    for line in fs::read_to_string(root.join("src/tool_bridge.rs"))
        .unwrap()
        .lines()
    {
        if let Some(rest) = line.trim().strip_prefix("name: \"") {
            if let Some((n, _)) = rest.split_once('"') {
                if !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                    arms.push(n.to_string());
                }
            }
        }
    }
    arms.sort();
    arms.dedup();
    assert!(arms.len() > 30, "the arm scan found only {arms:?}");
    arms
}

#[test]
fn banner_lists_every_dispatch_arm() {
    let dir = scratch("banner");
    let out = run(&dir, &["help"]);
    assert!(out.status.success(), "{out:?}");
    let banner = String::from_utf8_lossy(&out.stdout);
    let missing: Vec<String> = dispatch_arms()
        .into_iter()
        .filter(|arm| {
            let needle = format!("wicked-estate {arm}");
            !banner.match_indices(&needle).any(|(i, _)| {
                banner[i + needle.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-'))
            })
        })
        .collect();
    assert!(
        missing.is_empty(),
        "dispatched but not in the usage banner (#202): {missing:?}"
    );
    assert!(
        banner.contains("wicked-estate --version"),
        "the banner advertises --version (#200)"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_commands_fail_closed_on_a_missing_db() {
    let dir = scratch("missing");
    let table: &[&[&str]] = &[
        &["query", "f"],
        &["blast-radius", "f"],
        &["path", "f", "g"],
        &["resolve", "f"],
        &["stats"],
        &["nodes"],
        &["graph-view"],
        &["annotations"],
        &["source", "f"],
        &["entrypoints"],
        &["export"],
    ];
    for (i, args) in table.iter().enumerate() {
        let db = dir.join(format!("missing-{i}.db"));
        let spec = db.to_str().unwrap().to_string();
        let mut argv: Vec<&str> = args.to_vec();
        argv.push("--db");
        argv.push(&spec);
        let out = run(&dir, &argv);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "{args:?} must fail on a missing db, got stdout={}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            stderr.contains(&format!("no graph at {spec}")),
            "{args:?}: {stderr}"
        );
        assert!(
            !db.exists(),
            "{args:?} must not create the db it was asked to read"
        );
    }
    // `index` still creates: writers are not read commands.
    let created = dir.join("created.db");
    let out = run(
        &dir,
        &[
            "index",
            dir.join("src").to_str().unwrap(),
            "--db",
            created.to_str().unwrap(),
        ],
    );
    assert!(out.status.success() && created.exists(), "{out:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn query_json_emits_resolve_rows() {
    let (dir, db) = indexed_chain("queryjson");
    let q = run(&dir, &["query", "f", "--json", "--db", &db]);
    assert!(q.status.success(), "{q:?}");
    let rows: serde_json::Value = serde_json::from_slice(&q.stdout).expect("query --json is JSON");
    let rows = rows.as_array().expect("an array of rows");
    assert_eq!(rows.len(), 1, "{rows:?}");
    for key in ["symbol_id", "name", "kind", "file", "line"] {
        assert!(rows[0].get(key).is_some(), "row lacks {key}: {rows:?}");
    }
    let r = run(&dir, &["resolve", "f", "--json", "--db", &db]);
    let resolved: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(
        rows[0], resolved[0],
        "query --json is resolve --json's shape"
    );
    let text = run(&dir, &["query", "f", "--db", &db]);
    assert!(
        String::from_utf8_lossy(&text.stdout).contains("match(es) for 'f'"),
        "text mode unchanged"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_arms_survive_a_closed_stdout() {
    let (dir, db) = indexed_chain("pipe");
    let r = run(&dir, &["resolve", "f", "--json", "--db", &db]);
    let resolved: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    let id = resolved[0]["symbol_id"].as_str().unwrap().to_string();
    let table: Vec<Vec<&str>> = vec![
        vec!["lineage", "--symbol", &id, "--json"],
        vec!["lineage", "--symbol", &id],
        vec!["resolve", "f", "--json"],
        vec!["query", "f", "--json"],
        vec!["blast-radius", "f", "--json"],
        vec!["resolve", "f"],
        vec!["query", "f"],
        vec!["blast-radius", "f"],
    ];
    for args in table {
        let mut child = Command::new(bin())
            .current_dir(&dir)
            .args(&args)
            .args(["--db", &db])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The reader goes away before the document is written (`| head -1` after the index).
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("panicked") && out.status.code() != Some(101),
            "{args:?} must not panic on a closed stdout: {stderr}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolve_hides_value_slots_unless_include_values() {
    let dir = scratch("slots");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    let db = dir.join("g.db").to_str().unwrap().to_string();
    let out = run(&dir, &["index", fixture.to_str().unwrap(), "--db", &db]);
    assert!(out.status.success(), "index failed: {out:?}");
    let store = SqliteStore::open(&db).unwrap();
    let slot = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .find(|n| n.is_value_flow_node())
        .expect("the fixture has synthetic value slots");
    let name = slot.name.clone();
    let default = run(&dir, &["resolve", &name, "--json", "--db", &db]);
    assert!(default.status.success(), "{default:?}");
    let rows: serde_json::Value = serde_json::from_slice(&default.stdout).unwrap();
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| r["symbol_id"] != slot.symbol.as_str()),
        "the slot {} must be hidden by default: {rows}",
        slot.symbol.as_str()
    );
    let with = run(
        &dir,
        &["resolve", &name, "--include-values", "--json", "--db", &db],
    );
    assert!(with.status.success(), "{with:?}");
    let rows: serde_json::Value = serde_json::from_slice(&with.stdout).unwrap();
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .any(|r| r["symbol_id"] == slot.symbol.as_str()),
        "--include-values is the way back in: {rows}"
    );
    let text = run(&dir, &["resolve", &name, "--include-values", "--db", &db]);
    assert!(text.status.success(), "{text:?}");
    let _ = std::io::stderr().flush();
    let _ = fs::remove_dir_all(&dir);
}
