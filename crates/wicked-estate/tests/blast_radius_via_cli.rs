//! #450: the CLI's rows describe real edges to the immediately inner ring, even when paged.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use wicked_estate_core::{
    Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, ResolutionTier, Span, SymbolId,
};
use wicked_estate_store::SqliteStore;

fn fixture(db: &Path, ids: &[String], edges: &[(&str, &str, EdgeKind)]) {
    let mut store = SqliteStore::open(db).unwrap();
    store.begin_batch().unwrap();
    let nodes: Vec<Node> = ids
        .iter()
        .map(|id| {
            Node::new(
                SymbolId(id.clone()),
                if id == "file" {
                    NodeKind::File
                } else {
                    NodeKind::Function
                },
                id,
                Language::new("rust"),
                Location::new("fixture.rs", Span::ZERO),
            )
        })
        .collect();
    store.upsert_nodes(&nodes).unwrap();
    let edges: Vec<Edge> = edges
        .iter()
        .map(|(s, t, kind)| {
            Edge::new(
                SymbolId((*s).into()),
                SymbolId((*t).into()),
                kind.clone(),
                ResolutionTier::Parsed,
                "via-fixture",
            )
        })
        .collect();
    store.upsert_edges(&edges).unwrap();
    store.commit_batch().unwrap();
}

fn blast(db: &Path, cursor: Option<&str>) -> Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate"));
    cmd.args(["blast-radius", "t", "--json", "--db"]).arg(db);
    if let Some(cursor) = cursor {
        cmd.args(["--cursor", cursor]);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.len() <= 25_001, "one reply stays within R4");
    serde_json::from_slice(&out.stdout).unwrap()
}

fn row<'a>(doc: &'a Value, id: &str) -> &'a Value {
    doc["dependents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("missing {id}: {doc}"))
}

#[test]
fn diamond_lists_both_predecessors_and_contains_edges() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("graph.db");
    fixture(
        &db,
        &["t", "a", "b", "c", "file"].map(str::to_string),
        &[
            ("a", "t", EdgeKind::Calls),
            ("b", "t", EdgeKind::Calls),
            ("c", "a", EdgeKind::Calls),
            ("c", "b", EdgeKind::Calls),
            ("file", "c", EdgeKind::Contains),
        ],
    );
    let doc = blast(&db, None);
    assert!(doc["next_cursor"].is_null(), "small answer is unpaged");
    assert_eq!(
        row(&doc, "c")["via"],
        json!([{"id": "a", "kind": "calls"}, {"id": "b", "kind": "calls"}])
    );
    for id in ["a", "b"] {
        assert_eq!(row(&doc, id)["via"], json!([{"id": "t", "kind": "calls"}]));
        assert_eq!(row(&doc, id)["via_omitted"], 0);
    }
    assert_eq!(
        row(&doc, "file")["via"],
        json!([{"id": "c", "kind": "contains"}])
    );
}

#[test]
fn cycle_never_points_to_the_same_or_an_outer_ring() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("graph.db");
    fixture(
        &db,
        &["t", "a", "b"].map(str::to_string),
        &[
            ("a", "t", EdgeKind::Calls),
            ("b", "a", EdgeKind::Calls),
            ("a", "b", EdgeKind::Calls),
        ],
    );
    let doc = blast(&db, None);
    assert_eq!(row(&doc, "a")["via"], json!([{"id": "t", "kind": "calls"}]));
    assert_eq!(row(&doc, "b")["via"], json!([{"id": "a", "kind": "calls"}]));
    for r in doc["dependents"].as_array().unwrap() {
        for via in r["via"].as_array().unwrap() {
            let inner_depth = if via["id"] == "t" {
                0
            } else {
                row(&doc, via["id"].as_str().unwrap())["depth"]
                    .as_u64()
                    .unwrap()
            };
            assert_eq!(inner_depth + 1, r["depth"].as_u64().unwrap());
        }
    }
}

#[test]
fn via_caps_three_edges_and_counts_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("graph.db");
    let mids = ["a", "b", "c", "d", "e"];
    let mut edges = Vec::new();
    for mid in mids {
        edges.push((mid, "t", EdgeKind::Calls));
        edges.push(("outer", mid, EdgeKind::Calls));
    }
    fixture(
        &db,
        &["t", "a", "b", "c", "d", "e", "outer"].map(str::to_string),
        &edges,
    );
    let doc = blast(&db, None);
    assert_eq!(
        row(&doc, "outer")["via"],
        json!([
            {"id": "a", "kind": "calls"},
            {"id": "b", "kind": "calls"},
            {"id": "c", "kind": "calls"},
        ])
    );
    assert_eq!(row(&doc, "outer")["via_omitted"], 2);
}

#[test]
fn pages_join_with_the_unpaged_answers_via_row_for_row() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("graph.db");
    let mut ids = vec!["t".to_string()];
    ids.extend((0..600).map(|i| format!("caller_with_a_long_descriptive_name_{i:04}")));
    let edges: Vec<(&str, &str, EdgeKind)> = ids[1..]
        .iter()
        .map(|id| (id.as_str(), "t", EdgeKind::Calls))
        .collect();
    fixture(&db, &ids, &edges);
    let store = SqliteStore::open(&db).unwrap();
    // The library returns the entire answer before the CLI applies its per-reply budget.
    let all = wicked_estate::blast_radius_by_name(&store, "t", 12).unwrap();
    let expected: BTreeMap<String, Value> = all
        .dependents
        .iter()
        .map(|n| {
            (
                n.symbol.0.clone(),
                json!({"via": [{"id": "t", "kind": "calls"}], "via_omitted": 0}),
            )
        })
        .collect();
    assert_eq!(expected.len(), 600);
    let mut seen = BTreeSet::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let doc = blast(&db, cursor.as_deref());
        assert_eq!(doc["page"]["from"], json!(seen.len()));
        assert_eq!(doc["page"]["total"], json!(expected.len()));
        for r in doc["dependents"].as_array().unwrap() {
            let id = r["id"].as_str().unwrap();
            assert!(seen.insert(id.to_string()), "duplicate row {id}");
            assert_eq!(r["via"], json!([{"id": "t", "kind": "calls"}]));
            assert_eq!(
                json!({"via": r["via"], "via_omitted": r["via_omitted"]}),
                expected[id]
            );
        }
        pages += 1;
        cursor = doc["next_cursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            break;
        }
        assert!(pages < 100, "paging must terminate");
    }
    assert!(pages > 1);
    assert_eq!(seen.len(), expected.len());
}
