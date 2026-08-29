//! Admissibility residual F-A (closure finding 2): a JSON key whose name collides with a method
//! name (`optional`, `parse` — every zod call site) must NOT receive Calls edges from TS call
//! sites. The 402-edge class on command_iq (374 ts + 21 tsx + 7 bash, all `name-resolver`@0.6)
//! reduced to a fixture. The fix is DATA: a `json` row in `languages.toml` (own-name family)
//! makes the existing D5 cross-family guard block the bind — `json.scm` mints zero call refs,
//! so no legitimate Calls edge can target a json node.
//!
//! Survival asserts (D-10 blast-radius honesty): the same-family TS call still resolves, and the
//! evidence-based json-import path — `RelativeImportResolver`'s exact-path File→File bind at 0.9,
//! which carries NO family guard — is untouched. What is lost is only the 0.6 name-coincidence
//! bind (plus the ts→json Imports name-bind, recorded and accepted per docs/recon/admissibility.md
//! D-10).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead, NodeKind};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_admdata_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn json_keys_are_never_calls_targets_and_evidence_paths_survive() {
    let dir = fresh_dir("json");
    // TS side: `.optional()` / `.parse()` call sites (the zod shape), a same-family call that
    // MUST resolve, and a relative import of the json file that MUST keep its File→File edge.
    fs::write(
        dir.join("schema.ts"),
        "import cfg from './config.json';\n\
         import { helper } from './util';\n\
         export function build(x: { optional: () => unknown; parse: () => unknown }) {\n\
           x.optional();\n\
           x.parse();\n\
           helper();\n\
           return cfg;\n\
         }\n",
    )
    .unwrap();
    fs::write(
        dir.join("util.ts"),
        "export function helper(): number {\n  return 1;\n}\n",
    )
    .unwrap();
    // JSON side: top-level keys colliding with the method names (minted as Struct nodes).
    fs::write(
        dir.join("config.json"),
        "{\n  \"optional\": {\"a\": 1},\n  \"parse\": {\"b\": 2}\n}\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    let nodes = GraphRead::all_nodes(&store).unwrap();
    let by_sym: HashMap<_, _> = nodes.iter().map(|n| (n.symbol.clone(), n)).collect();

    // Non-vacuity: the colliding JSON keys must exist as json Struct nodes.
    for key in ["optional", "parse"] {
        assert!(
            nodes.iter().any(|n| n.name == key
                && n.language.as_str() == "json"
                && n.kind == NodeKind::Struct),
            "fixture must produce a json Struct node named {key}"
        );
    }

    let edges = GraphRead::all_edges(&store).unwrap();

    // The finding: ZERO Calls edges may target a json-language node.
    let calls_to_json: Vec<_> = edges
        .iter()
        .filter(|e| {
            e.kind == EdgeKind::Calls
                && by_sym
                    .get(&e.target)
                    .is_some_and(|t| t.language.as_str() == "json")
        })
        .collect();
    assert!(
        calls_to_json.is_empty(),
        "Calls edges must never target json nodes; found {}: {:?}",
        calls_to_json.len(),
        calls_to_json
            .iter()
            .map(|e| format!("{} -> {}", e.source.0, e.target.0))
            .collect::<Vec<_>>()
    );

    // Survival 1: the same-family TS call still resolves (guard must not over-block).
    assert!(
        edges.iter().any(|e| {
            e.kind == EdgeKind::Calls
                && by_sym
                    .get(&e.target)
                    .is_some_and(|t| t.name == "helper" && t.language.as_str() == "typescript")
        }),
        "the ts→ts helper() call must still resolve"
    );

    // Survival 2 (D-10): the relative import of ./config.json keeps its File→File edge —
    // RelativeImportResolver is evidence-based (exact path) and carries no family guard.
    assert!(
        edges.iter().any(|e| {
            e.kind == EdgeKind::Imports
                && by_sym.get(&e.source).is_some_and(|s| {
                    s.kind == NodeKind::File && s.location.file.ends_with("schema.ts")
                })
                && by_sym.get(&e.target).is_some_and(|t| {
                    t.kind == NodeKind::File && t.location.file.ends_with("config.json")
                })
        }),
        "the File→File relative-import edge to config.json must survive"
    );

    let _ = fs::remove_dir_all(&dir);
}
