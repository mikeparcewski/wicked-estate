//! W5.1 FTS5 tests for `SqliteStore`.
//!
//! Covers: BM25-ranked results, `limit` respected, special-character escaping, and the
//! `capabilities().full_text_search` flag.  MemStore is NOT tested here — it intentionally
//! does not advertise FTS, and its substring fallback is exercised by the conformance suite.

use wicked_estate_core::{
    GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, SymbolId, SymbolQuery,
};
use wicked_estate_store::SqliteStore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn loc() -> Location {
    Location::new("src/lib.rs", Span::ZERO)
}

fn make_node(id: &str, name: &str, sig: Option<&str>, doc: Option<&str>) -> Node {
    let mut n = Node::new(
        SymbolId(id.to_string()),
        NodeKind::Function,
        name,
        Language::new("rust"),
        loc(),
    );
    n.signature = sig.map(str::to_string);
    n.doc = doc.map(str::to_string);
    n
}

fn open() -> SqliteStore {
    SqliteStore::in_memory().expect("open in-memory sqlite")
}

// ---------------------------------------------------------------------------
// 1. capability flag
// ---------------------------------------------------------------------------

#[test]
fn sqlite_advertises_full_text_search() {
    let store = open();
    assert!(
        store.capabilities().full_text_search,
        "SqliteStore must report full_text_search=true after W5.1"
    );
}

#[test]
fn memstore_does_not_advertise_full_text_search() {
    let store = wicked_estate_store::MemStore::new();
    assert!(
        !store.capabilities().full_text_search,
        "MemStore must not claim FTS — it only does substring fallback"
    );
}

// ---------------------------------------------------------------------------
// 2. basic FTS match
// ---------------------------------------------------------------------------

#[test]
fn fts_returns_matching_nodes() {
    let mut store = open();
    let nodes = vec![
        make_node(
            "sym::alpha",
            "alpha_function",
            Some("fn alpha_function() -> i32"),
            Some("alpha doc"),
        ),
        make_node(
            "sym::beta",
            "beta_routine",
            Some("fn beta_routine()"),
            Some("completely unrelated"),
        ),
        make_node("sym::gamma", "gamma_proc", None, Some("unrelated content")),
    ];
    store.upsert_nodes(&nodes).expect("upsert");

    let q = SymbolQuery {
        text: Some("alpha".to_string()),
        ..Default::default()
    };
    let results = store.find_symbols(&q).expect("find");
    assert_eq!(results.len(), 1, "only 'alpha_function' matches 'alpha'");
    assert_eq!(results[0].symbol.0, "sym::alpha");
}

// ---------------------------------------------------------------------------
// 3. doc-comment content is searchable
// ---------------------------------------------------------------------------

#[test]
fn fts_searches_doc_field() {
    let mut store = open();
    let nodes = vec![
        make_node(
            "sym::one",
            "do_something",
            None,
            Some("Computes the zeta transform"),
        ),
        make_node("sym::two", "other_fn", None, Some("ordinary function")),
    ];
    store.upsert_nodes(&nodes).expect("upsert");

    let q = SymbolQuery {
        text: Some("zeta".to_string()),
        ..Default::default()
    };
    let results = store.find_symbols(&q).expect("find");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].symbol.0, "sym::one");
}

// ---------------------------------------------------------------------------
// 4. BM25 ordering: more-relevant result comes first
// ---------------------------------------------------------------------------

#[test]
fn fts_bm25_best_match_first() {
    let mut store = open();
    // "process" appears only in name of sym::proc; sym::util only mentions it in doc.
    // The name-field hit should score higher — we just verify the most-relevant one is first.
    let nodes = vec![
        make_node(
            "sym::util",
            "utility_helper",
            None,
            Some("helper that calls process internally"),
        ),
        make_node(
            "sym::proc",
            "process_request",
            Some("fn process_request(req: Req)"),
            None,
        ),
    ];
    store.upsert_nodes(&nodes).expect("upsert");

    let q = SymbolQuery {
        text: Some("process".to_string()),
        ..Default::default()
    };
    let results = store.find_symbols(&q).expect("find");

    // Both match; the one whose name contains "process" should rank first.
    assert!(results.len() >= 2, "both nodes should match");
    assert_eq!(
        results[0].symbol.0, "sym::proc",
        "process_request should be the BM25-best match for 'process'"
    );
}

// ---------------------------------------------------------------------------
// 5. limit is applied after BM25 ordering
// ---------------------------------------------------------------------------

#[test]
fn fts_limit_respected() {
    let mut store = open();
    let nodes: Vec<Node> = (0..10)
        .map(|i| {
            make_node(
                &format!("sym::fn{i}"),
                &format!("widget_func_{i}"),
                None,
                Some("widget handler"),
            )
        })
        .collect();
    store.upsert_nodes(&nodes).expect("upsert");

    let q = SymbolQuery {
        text: Some("widget".to_string()),
        limit: Some(3),
        ..Default::default()
    };
    let results = store.find_symbols(&q).expect("find");
    assert_eq!(results.len(), 3, "limit=3 must truncate to 3 results");
}

// ---------------------------------------------------------------------------
// 6. special characters in the query don't crash (escaping)
// ---------------------------------------------------------------------------

#[test]
fn fts_special_chars_do_not_crash() {
    let mut store = open();
    let nodes = vec![make_node(
        "sym::safe",
        "safe_func",
        None,
        Some("nothing special"),
    )];
    store.upsert_nodes(&nodes).expect("upsert");

    // FTS5 operators / syntax characters: AND OR NOT * ^ " { } ( )
    let evil_inputs = [
        "AND OR NOT",
        r#"he said "hello""#,
        "proc*",
        "^anchor",
        "{phrase query}",
        "(parenthesised)",
        r#""quoted phrase""#,
    ];
    for term in &evil_inputs {
        let q = SymbolQuery {
            text: Some(term.to_string()),
            ..Default::default()
        };
        let res = store.find_symbols(&q);
        assert!(
            res.is_ok(),
            "query with special chars '{term}' must not error: {:?}",
            res.err()
        );
    }
}

// ---------------------------------------------------------------------------
// 7. upsert idempotence — re-upserting a node doesn't duplicate FTS rows
// ---------------------------------------------------------------------------

#[test]
fn fts_upsert_idempotent() {
    let mut store = open();
    let node = make_node(
        "sym::dup",
        "duplicate_func",
        None,
        Some("deduplication test"),
    );
    // Upsert the same node three times.
    store
        .upsert_nodes(std::slice::from_ref(&node))
        .expect("first upsert");
    store
        .upsert_nodes(std::slice::from_ref(&node))
        .expect("second upsert");
    store
        .upsert_nodes(std::slice::from_ref(&node))
        .expect("third upsert");

    let q = SymbolQuery {
        text: Some("deduplication".to_string()),
        ..Default::default()
    };
    let results = store.find_symbols(&q).expect("find");
    assert_eq!(
        results.len(),
        1,
        "re-upserting must not create duplicate FTS rows"
    );
}

// ---------------------------------------------------------------------------
// 8. text query combined with kind filter
// ---------------------------------------------------------------------------

#[test]
fn fts_combined_with_kind_filter() {
    let mut store = open();
    let mut fn_node = make_node("sym::fn_node", "render_widget", None, Some("render doc"));
    fn_node.kind = NodeKind::Function;
    let mut cls_node = make_node("sym::cls_node", "render_pipeline", None, Some("render doc"));
    cls_node.kind = NodeKind::Class;

    store.upsert_nodes(&[fn_node, cls_node]).expect("upsert");

    // Both match "render" via FTS; kind filter should retain only the Function.
    let q = SymbolQuery {
        text: Some("render".to_string()),
        kinds: vec![NodeKind::Function],
        ..Default::default()
    };
    let results = store.find_symbols(&q).expect("find");
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0].kind, NodeKind::Function));
}
