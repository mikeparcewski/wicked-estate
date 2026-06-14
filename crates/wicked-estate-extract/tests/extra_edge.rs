//! Integration tests for `ExtraEdgeExtractor`.
//!
//! Uses fixture files under `tests/fixtures/event_bus_{producer,consumer}.js` plus the shared
//! TOML config at `tests/fixtures/event_bus_rules.toml`. Key invariants checked:
//!
//! 1. Correct node kinds + provenance per rule.
//! 2. Two files that emit/consume the SAME topic share the SAME synthetic `SymbolId` — so
//!    blast-radius can cross the event-bus boundary.
//! 3. Distinct topics produce distinct node ids.
//! 4. `ExtraExtraction::edges` carry the right `EdgeKind`.
//! 5. Glob filtering drops non-matching files.

use wicked_estate_core::{EdgeKind, Language, NodeKind, Provenance, SourceFile, Symbol};
use wicked_estate_extract::ExtraEdgeExtractor;

// ── helpers ───────────────────────────────────────────────────────────────────

fn load_fixture(name: &str) -> String {
    let path = format!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/{}"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read fixture {name}: {e}"))
}

fn js_file(path: &str, text: String) -> SourceFile {
    SourceFile {
        path: path.into(),
        language: Language::new("javascript"),
        text,
    }
}

fn load_extractor() -> ExtraEdgeExtractor {
    let toml = load_fixture("event_bus_rules.toml");
    ExtraEdgeExtractor::from_toml(&toml).expect("event_bus_rules.toml must parse cleanly")
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn fixture_toml_parses_cleanly() {
    // Just ensure the fixture TOML is well-formed. If this fails the extractor is broken.
    load_extractor();
}

#[test]
fn producer_emits_synthetic_topic_nodes() {
    let ex = load_extractor();
    let text = load_fixture("event_bus_producer.js");
    let sf = js_file("src/orders.js", text);
    let out = ex.extract_extra(&sf);

    // Fixture emits: orders.created (×2 in file), orders.fulfilled.
    // Nodes are deduped — expect exactly 2 distinct topic nodes.
    assert_eq!(
        out.nodes.len(),
        2,
        "producer: expected 2 distinct topic nodes, got {:?}",
        out.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );

    let names: Vec<&str> = out.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"orders.created"),
        "must have orders.created node"
    );
    assert!(
        names.contains(&"orders.fulfilled"),
        "must have orders.fulfilled node"
    );

    // All nodes must be Synthetic kind.
    for n in &out.nodes {
        assert_eq!(
            n.kind,
            NodeKind::Synthetic,
            "topic nodes must be Synthetic, got {:?}",
            n.kind
        );
    }
}

#[test]
fn producer_emits_correct_edges() {
    let ex = load_extractor();
    let text = load_fixture("event_bus_producer.js");
    let sf = js_file("src/orders.js", text);
    let out = ex.extract_extra(&sf);

    // Fixture has 3 emit() calls: orders.created, orders.fulfilled, orders.created again.
    assert_eq!(out.edges.len(), 3, "producer: expected 3 emits edges");

    for e in &out.edges {
        assert_eq!(
            e.kind,
            EdgeKind::Other("emits".to_string()),
            "all producer edges must be emits"
        );
        assert_eq!(
            e.provenance,
            Provenance::Extractor("event-bus-emit".to_string()),
            "provenance must be Extractor(event-bus-emit)"
        );
        assert_eq!(
            e.source,
            Symbol::file("src/orders.js").id(),
            "source must be the file node"
        );
    }
}

#[test]
fn consumer_emits_synthetic_topic_nodes() {
    let ex = load_extractor();
    let text = load_fixture("event_bus_consumer.js");
    let sf = js_file("src/notifications.js", text);
    let out = ex.extract_extra(&sf);

    // Fixture subscribes to: orders.created, orders.fulfilled, payments.processed.
    assert_eq!(
        out.nodes.len(),
        3,
        "consumer: expected 3 distinct topic nodes"
    );

    let names: Vec<&str> = out.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"orders.created"));
    assert!(names.contains(&"orders.fulfilled"));
    assert!(names.contains(&"payments.processed"));
}

#[test]
fn consumer_emits_correct_edges() {
    let ex = load_extractor();
    let text = load_fixture("event_bus_consumer.js");
    let sf = js_file("src/notifications.js", text);
    let out = ex.extract_extra(&sf);

    assert_eq!(out.edges.len(), 3, "consumer: expected 3 consumes edges");

    for e in &out.edges {
        assert_eq!(
            e.kind,
            EdgeKind::Other("consumes".to_string()),
            "all consumer edges must be consumes"
        );
        assert_eq!(
            e.provenance,
            Provenance::Extractor("event-bus-consume".to_string()),
            "provenance must be Extractor(event-bus-consume)"
        );
    }
}

/// THE KEY INVARIANT: producer and consumer files that reference the same topic share
/// the identical synthetic SymbolId. This is what lets blast-radius cross the event-bus boundary.
#[test]
fn shared_topic_has_same_symbol_id_across_files() {
    let ex = load_extractor();

    let producer_text = load_fixture("event_bus_producer.js");
    let consumer_text = load_fixture("event_bus_consumer.js");

    let producer_out = ex.extract_extra(&js_file("src/orders.js", producer_text));
    let consumer_out = ex.extract_extra(&js_file("src/notifications.js", consumer_text));

    // Both files reference "orders.created" and "orders.fulfilled".
    let producer_ids: std::collections::HashSet<_> =
        producer_out.nodes.iter().map(|n| &n.symbol).collect();
    let consumer_ids: std::collections::HashSet<_> =
        consumer_out.nodes.iter().map(|n| &n.symbol).collect();

    let shared: Vec<_> = producer_ids.intersection(&consumer_ids).collect();
    assert_eq!(
        shared.len(),
        2,
        "orders.created and orders.fulfilled must share ids; producer={:?} consumer={:?}",
        producer_out
            .nodes
            .iter()
            .map(|n| n.symbol.as_str())
            .collect::<Vec<_>>(),
        consumer_out
            .nodes
            .iter()
            .map(|n| n.symbol.as_str())
            .collect::<Vec<_>>(),
    );

    // Verify the ids are derived from Symbol::synthetic("event-bus-topic", "topic:orders.created").
    // Both rules set node_scheme = "event-bus-topic" so they converge on the same SymbolId.
    let expected_created = Symbol::synthetic("event-bus-topic", "topic:orders.created").id();
    assert!(
        producer_ids.contains(&expected_created) && consumer_ids.contains(&expected_created),
        "orders.created synthetic id must be present in both; expected={}",
        expected_created
    );
}

#[test]
fn glob_filter_drops_non_js_file() {
    let ex = load_extractor();
    // Same text as producer but with a .py extension — the glob should filter it out.
    let text = load_fixture("event_bus_producer.js");
    let sf = SourceFile {
        path: "src/orders.py".into(),
        language: Language::new("python"),
        text,
    };
    let out = ex.extract_extra(&sf);
    assert!(out.nodes.is_empty(), "glob **/*.js must reject .py files");
    assert!(out.edges.is_empty());
}

#[test]
fn no_matches_produces_empty_extraction() {
    let ex = load_extractor();
    let sf = js_file(
        "src/noop.js",
        "// This file has no emit or subscribe calls.\nconsole.log('hello');\n".to_string(),
    );
    let out = ex.extract_extra(&sf);
    assert!(out.nodes.is_empty());
    assert!(out.edges.is_empty());
}
