//! `wicked-estate supports` end to end, through the real binary (TS-S2A CLI parity).
//!
//! The command is a frontend over `GraphRead::{support_owners, edge_supports}` and
//! `GraphWrite::replace_edge_supports`, so the oracle is the store itself: every `--json`
//! document is compared with what the same SQLite file answers through the trait. Graphs are
//! built through `SqliteStore` directly (support has no producer yet — TS-S2/TS-S3 add them).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use wicked_estate_core::{
    Direction, Edge, EdgeKind, GraphRead, GraphWrite, Language, Location, Node, NodeKind,
    ResolutionTier, Span, SupportFact, SupportOwner, SymbolId,
};
use wicked_estate_store::SqliteStore;

const R4_CHAR_BUDGET: usize = 25_000;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_supportscli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let s = Scratch(d);
    fs::create_dir_all(&*s).unwrap();
    s
}

/// The binary, run inside `dir`, hermetically. Every inherited `WICKED_*` (store selection,
/// runtime profile, event emitter, plugins, OTel), `OTEL_*` and `GIT_*` variable is removed; the
/// emitter points at a missing program with a spool inside `dir`; git discovery stops at `dir`'s
/// parent and reads no global/system config (no signing, no hooks).
fn run(dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(bin());
    cmd.current_dir(dir).args(args);
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy().into_owned();
        if k.starts_with("WICKED_") || k.starts_with("OTEL_") || k.starts_with("GIT_") {
            cmd.env_remove(&k);
        }
    }
    cmd.env(
        "WICKED_ESTATE_EMIT_PROGRAM",
        "wicked-bus-absent-supports-cli",
    )
    .env("WICKED_ESTATE_EMIT_DEADLETTER", dir.join("emit.ndjson"))
    .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
    .env("GIT_CONFIG_NOSYSTEM", "1")
    .env("GIT_CONFIG_GLOBAL", dir.join("no-gitconfig"))
    .output()
    .expect("spawn wicked-estate")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json_out(o: &Output) -> Value {
    assert!(o.status.success(), "failed: {}", stderr(o));
    let text = stdout(o);
    assert_eq!(
        text.trim_end().lines().count(),
        1,
        "exactly one JSON document: {text}"
    );
    serde_json::from_str(&text).expect("valid JSON")
}

fn sid(s: &str) -> SymbolId {
    SymbolId(s.into())
}

fn node(id: &str) -> Node {
    Node::new(
        sid(id),
        NodeKind::Function,
        id,
        Language::new("typescript"),
        Location::new("src/nodes.ts", Span::ZERO),
    )
}

fn calls(by: &str, tier: ResolutionTier, file: &str) -> Edge {
    Edge::new(sid("app:a"), sid("app:b"), EdgeKind::Calls, tier, by)
        .with_location(Location::new(file, Span::ZERO))
}

fn owner(p: &str, s: &str) -> SupportOwner {
    SupportOwner::new(p, s).unwrap()
}

/// A graph with a base `Calls` edge a → b and two owners supporting it, plus an empty owner.
fn build(dir: &Path) -> (PathBuf, Edge) {
    let db = dir.join("graph.db");
    let mut store = SqliteStore::open(&db).unwrap();
    store.upsert_nodes(&[node("app:a"), node("app:b")]).unwrap();
    let base = calls("name-resolver", ResolutionTier::Heuristic, "src/a.ts");
    store.upsert_edges(std::slice::from_ref(&base)).unwrap();
    store
        .replace_edge_supports(
            &owner("scip-typescript", "apps/web"),
            3,
            &[
                SupportFact::new(
                    "occ:2",
                    calls("scip-typescript", ResolutionTier::Scip, "src/a.ts"),
                )
                .unwrap(),
                SupportFact::new(
                    "occ:1",
                    calls("scip-typescript", ResolutionTier::Scip, "src/a2.ts"),
                )
                .unwrap(),
            ],
        )
        .unwrap();
    store
        .replace_edge_supports(
            &owner("angular-compiler", "apps/web"),
            1,
            &[SupportFact::new(
                "tmpl:7",
                calls("angular-compiler", ResolutionTier::Tsg, "src/a.html"),
            )
            .unwrap()],
        )
        .unwrap();
    store
        .replace_edge_supports(
            &owner("emptied", "x"),
            1,
            &[SupportFact::new("f", calls("e", ResolutionTier::Tags, "src/e.ts")).unwrap()],
        )
        .unwrap();
    store
        .replace_edge_supports(&owner("emptied", "x"), 2, &[])
        .unwrap();
    (db, base)
}

#[test]
fn owners_json_is_the_store_answer() {
    let dir = scratch("owners");
    let (db, _) = build(&dir);
    let doc = json_out(&run(
        &dir,
        &["supports", "owners", "--json", "--db", "graph.db"],
    ));
    let store = SqliteStore::open_readonly(&db).unwrap();
    let want = serde_json::to_value(store.support_owners().unwrap()).unwrap();
    assert_eq!(doc, json!({"owners": want, "total": 3, "truncated": false}));
    // Ordered (producer, snapshot), and an owner whose set is empty is still listed.
    let producers: Vec<&str> = doc["owners"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["owner"]["producer"].as_str().unwrap())
        .collect();
    assert_eq!(
        producers,
        ["angular-compiler", "emptied", "scip-typescript"]
    );

    let text = run(&dir, &["supports", "owners", "--db", "graph.db"]);
    assert!(text.status.success(), "{}", stderr(&text));
    assert_eq!(
        stdout(&text),
        "angular-compiler\tapps/web\tgeneration 1\nemptied\tx\tgeneration 2\nscip-typescript\tapps/web\tgeneration 3\n"
    );
}

#[test]
fn edge_json_is_the_store_answer() {
    let dir = scratch("edge");
    let (db, _) = build(&dir);
    let doc = json_out(&run(
        &dir,
        &[
            "supports", "edge", "--source", "app:a", "--target", "app:b", "--kind", "calls",
            "--json", "--db", "graph.db",
        ],
    ));
    let store = SqliteStore::open_readonly(&db).unwrap();
    let rows = store
        .edge_supports(&sid("app:a"), &sid("app:b"), &EdgeKind::Calls)
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        doc,
        json!({
            "source": "app:a", "target": "app:b", "kind": "calls",
            "supports": serde_json::to_value(&rows).unwrap(),
            "total": 3, "truncated": false,
        })
    );
    // fact ids come back exactly as the producers wrote them, ordered by (owner, fact_id).
    let ids: Vec<&str> = doc["supports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["fact_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["tmpl:7", "occ:1", "occ:2"]);

    // A tag kind (`EdgeKind::Other`) and an unsupported key: an honest empty answer, exit 0.
    let none = json_out(&run(
        &dir,
        &[
            "supports", "edge", "--source", "app:a", "--target", "app:b", "--kind", "flows_to",
            "--json", "--db", "graph.db",
        ],
    ));
    assert_eq!(none["kind"], json!({"other": "flows_to"}));
    assert_eq!(
        (none["total"].clone(), none["supports"].clone()),
        (json!(0), json!([]))
    );

    let text = run(
        &dir,
        &[
            "supports", "edge", "--source", "app:a", "--target", "app:b", "--kind", "calls",
            "--db", "graph.db",
        ],
    );
    let t = stdout(&text);
    assert!(
        t.starts_with("support for app:a -[calls]-> app:b: 3 fact(s)\n"),
        "{t}"
    );
    assert!(
        t.contains("scip-typescript/apps/web gen 3  fact_id \"occ:1\""),
        "{t}"
    );
}

#[test]
fn retract_clears_one_owner_and_restores_the_base_edge() {
    let dir = scratch("retract");
    let (db, base) = build(&dir);
    for producer in ["scip-typescript", "angular-compiler"] {
        let doc = json_out(&run(
            &dir,
            &[
                "supports",
                "retract",
                "--producer",
                producer,
                "--snapshot",
                "apps/web",
                "--json",
                "--db",
                "graph.db",
            ],
        ));
        assert_eq!(doc["retracted"]["owner"]["producer"], json!(producer));
        assert_eq!(doc["retracted"]["retained"], json!(0));
    }
    let store = SqliteStore::open_readonly(&db).unwrap();
    assert_eq!(
        store
            .support_generation(&owner("scip-typescript", "apps/web"))
            .unwrap(),
        Some(4)
    );
    assert_eq!(
        store
            .support_generation(&owner("angular-compiler", "apps/web"))
            .unwrap(),
        Some(2)
    );
    assert!(
        store
            .edge_supports(&sid("app:a"), &sid("app:b"), &EdgeKind::Calls)
            .unwrap()
            .is_empty()
    );
    let public: Vec<Edge> = store
        .neighbors(&sid("app:a"), Direction::Dependencies)
        .unwrap();
    assert_eq!(public, vec![base], "the base edge is restored exactly");

    // Retracting an empty owner bumps its generation and retracts nothing; an unknown owner is an
    // error, and nothing is created for it.
    let again = json_out(&run(
        &dir,
        &[
            "supports",
            "retract",
            "--producer",
            "emptied",
            "--snapshot",
            "x",
            "--json",
            "--db",
            "graph.db",
        ],
    ));
    assert_eq!(
        (
            again["retracted"]["generation"].clone(),
            again["retracted"]["retracted"].clone()
        ),
        (json!(3), json!(0))
    );
    let unknown = run(
        &dir,
        &[
            "supports",
            "retract",
            "--producer",
            "nobody",
            "--snapshot",
            "x",
            "--db",
            "graph.db",
        ],
    );
    assert!(!unknown.status.success());
    assert!(
        stderr(&unknown).contains("no support owner"),
        "{}",
        stderr(&unknown)
    );
    assert_eq!(
        store.support_generation(&owner("nobody", "x")).unwrap(),
        None
    );
}

/// A missing or zero-length graph is refused by every subcommand; nothing is created or grown.
#[test]
fn missing_or_empty_graph_fails_closed() {
    let dir = scratch("closed");
    fs::write(dir.join("empty.db"), b"").unwrap();
    let modes: [&[&str]; 3] = [
        &["supports", "owners"],
        &[
            "supports", "edge", "--source", "a", "--target", "b", "--kind", "calls",
        ],
        &["supports", "retract", "--producer", "p", "--snapshot", "s"],
    ];
    for mode in modes {
        for db in [
            "missing.db",
            "sqlite://missing.db",
            "empty.db",
            "sqlite://empty.db",
        ] {
            let mut args = mode.to_vec();
            args.extend_from_slice(&["--db", db]);
            let out = run(&dir, &args);
            assert!(!out.status.success(), "{args:?} must fail");
            assert!(
                stderr(&out).contains("no graph at"),
                "{args:?}: {}",
                stderr(&out)
            );
        }
        assert!(!dir.join("missing.db").exists(), "{mode:?} created a graph");
        assert_eq!(
            fs::metadata(dir.join("empty.db")).unwrap().len(),
            0,
            "{mode:?} grew the empty file"
        );
    }
}

/// Argument errors fail with usage BEFORE any store is opened (the missing db is never touched).
#[test]
fn strict_arguments() {
    let dir = scratch("argv");
    let bad: [&[&str]; 13] = [
        &["supports"],
        &["supports", "everything"],
        &["supports", "owners", "--bogus"],
        &["supports", "owners", "--depth", "3"],
        &["supports", "owners", "stray"],
        &["supports", "owners", "--json", "--json"],
        &["supports", "edge", "--source", "a", "--target", "b"],
        &[
            "supports", "edge", "--source", "a", "--source", "a", "--target", "b", "--kind",
            "calls",
        ],
        &[
            "supports", "edge", "--source", "--target", "b", "--kind", "calls",
        ],
        &[
            "supports",
            "edge",
            "--source=",
            "--target",
            "b",
            "--kind",
            "calls",
        ],
        &[
            "supports",
            "edge",
            "--producer",
            "p",
            "--source",
            "a",
            "--target",
            "b",
            "--kind",
            "calls",
        ],
        &["supports", "retract", "--producer", "p"],
        &[
            "supports",
            "retract",
            "--producer",
            "p",
            "--snapshot",
            "s",
            "--kind",
            "calls",
        ],
    ];
    for args in bad {
        let mut full = args.to_vec();
        full.extend_from_slice(&["--db", "never.db"]);
        let out = run(&dir, &full);
        assert!(!out.status.success(), "{full:?} must fail");
        assert!(
            stderr(&out).contains("usage: wicked-estate supports"),
            "{full:?}: {}",
            stderr(&out)
        );
        assert!(
            !stderr(&out).contains("no graph at"),
            "{full:?}: argv is checked first"
        );
    }
    assert!(!dir.join("never.db").exists());
}

/// The whole document stays inside the one R4 budget; `total` stays exact and `truncated` says
/// rows were dropped.
#[test]
fn output_is_bounded_by_the_r4_budget() {
    let dir = scratch("r4");
    let db = dir.join("graph.db");
    let mut store = SqliteStore::open(&db).unwrap();
    store.upsert_nodes(&[node("app:a"), node("app:b")]).unwrap();
    let facts: Vec<SupportFact> = (0..300)
        .map(|i| {
            let mut e = calls(
                "scip-typescript",
                ResolutionTier::Scip,
                &format!("src/f{i}.ts"),
            );
            e.metadata.insert("note".into(), json!("x".repeat(200)));
            SupportFact::new(format!("occ:{i:04}"), e).unwrap()
        })
        .collect();
    store
        .replace_edge_supports(&owner("scip-typescript", "big"), 1, &facts)
        .unwrap();
    drop(store);
    let out = run(
        &dir,
        &[
            "supports", "edge", "--source", "app:a", "--target", "app:b", "--kind", "calls",
            "--json", "--db", "graph.db",
        ],
    );
    let text = stdout(&out);
    assert!(
        text.len() < R4_CHAR_BUDGET,
        "document is {} chars",
        text.len()
    );
    let doc = json_out(&out);
    assert_eq!(doc["total"], json!(300));
    assert_eq!(doc["truncated"], json!(true));
    let shown = doc["supports"].as_array().unwrap();
    assert!(!shown.is_empty() && shown.len() < 300);
    assert_eq!(
        shown[0]["fact_id"],
        json!("occ:0000"),
        "rows are dropped from the end, in order"
    );
    let t = stdout(&run(
        &dir,
        &[
            "supports", "edge", "--source", "app:a", "--target", "app:b", "--kind", "calls",
            "--db", "graph.db",
        ],
    ));
    assert!(
        t.contains(&format!("truncated: showing {} of 300", shown.len())),
        "{t}"
    );
}

/// `--kind` is checked with the rest of argv, before the graph is looked for: a case variant of a
/// built-in spelling (`Calls`) is a usage error with exit 1 and nothing on stdout, never an
/// `Other("Calls")` tag that answers an honest-looking empty result. A lowercase tag the graph
/// does not define (`nonsense`) is still matched as `Other`, exit 0, and says so on stderr.
#[test]
fn a_case_variant_kind_is_refused() {
    let dir = scratch("kindcase");
    let (_db, _) = build(&dir);
    let edge = |kind: &str, db: &str, json: bool| {
        let mut args = vec![
            "supports", "edge", "--source", "app:a", "--target", "app:b", "--kind", kind,
        ];
        if json {
            args.push("--json");
        }
        args.extend_from_slice(&["--db", db]);
        args.into_iter().map(str::to_owned).collect::<Vec<_>>()
    };
    let run_s = |args: &[String]| run(&dir, &args.iter().map(String::as_str).collect::<Vec<_>>());
    for kind in ["Calls", "CALLS", "Imports"] {
        let out = run_s(&edge(kind, "graph.db", true));
        assert!(!out.status.success(), "--kind {kind} must fail");
        assert_eq!(out.stdout.len(), 0, "--kind {kind}: {}", stdout(&out));
        let want = format!("did you mean {:?}", kind.to_lowercase());
        assert!(
            stderr(&out).contains(&want),
            "--kind {kind}: {}",
            stderr(&out)
        );
    }
    // The check is part of argv parsing: usage, and the missing graph is never looked for.
    let never = run_s(&edge("Calls", "never.db", false));
    assert!(!never.status.success());
    let e = stderr(&never);
    assert!(
        e.contains("usage: wicked-estate supports") && !e.contains("no graph at"),
        "{e}"
    );
    assert!(!dir.join("never.db").exists());
    // A built-in spelling: no note. An unknown lowercase tag: the `Other` tag, with a note.
    let ok = run_s(&edge("calls", "graph.db", true));
    assert!(ok.status.success(), "{}", stderr(&ok));
    assert!(!stderr(&ok).contains("note:"), "{}", stderr(&ok));
    let tag = run_s(&edge("nonsense", "graph.db", true));
    let doc = json_out(&tag);
    assert_eq!(doc["kind"], json!({"other": "nonsense"}));
    assert_eq!(doc["total"], json!(0));
    assert!(
        stderr(&tag).contains("not a built-in kind"),
        "{}",
        stderr(&tag)
    );
}

/// Text mode prints a fact's site with a 1-based line, like every other text path of the CLI;
/// `--json` carries the raw 0-based span unchanged.
#[test]
fn text_site_line_is_one_based() {
    let dir = scratch("line");
    let db = dir.join("graph.db");
    let mut store = SqliteStore::open(&db).unwrap();
    store.upsert_nodes(&[node("app:a"), node("app:b")]).unwrap();
    let mut fact = calls("scip-typescript", ResolutionTier::Scip, "src/forty_two.ts");
    fact.location = Some(Location::new(
        "src/forty_two.ts",
        Span {
            start_line: 41,
            end_line: 41,
            ..Span::ZERO
        },
    ));
    store
        .replace_edge_supports(
            &owner("scip-typescript", "apps/web"),
            1,
            &[SupportFact::new("occ:42", fact).unwrap()],
        )
        .unwrap();
    drop(store);
    let args = [
        "supports", "edge", "--source", "app:a", "--target", "app:b", "--kind", "calls", "--db",
        "graph.db",
    ];
    let t = stdout(&run(&dir, &args));
    let row = t
        .lines()
        .find(|l| l.contains("fact_id \"occ:42\""))
        .unwrap_or_else(|| panic!("no row for occ:42 in {t}"));
    assert!(row.ends_with(" at src/forty_two.ts:42"), "{row}");
    let mut json_args = args.to_vec();
    json_args.insert(8, "--json");
    let doc = json_out(&run(&dir, &json_args));
    assert_eq!(
        doc["supports"][0]["fact"]["location"]["span"]["start_line"],
        json!(41)
    );
}
