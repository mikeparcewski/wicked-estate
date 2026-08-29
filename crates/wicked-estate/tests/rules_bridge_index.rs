//! End-to-end proof that `RulesBridgeResolver` is wired into the `index_path` slice (engine
//! defect #6, D02-6/D02-8): a repo with a rules-engine bridge rule
//! (`.wicked-estate-extractors/odm.toml`), a Java call site (`IlrContext.execute()`), and a real
//! DRL RuleSet indexes into InvokedBy edges carrying `resolved_by = "rules-bridge-resolver"`.
//! Before the wiring, the bridge refs sat in `unresolved_refs` forever and the DRL RuleSet was
//! never linked to the call site.
//!
//! Falsifier (run manually, recorded in docs/recon/resolver-precision.md §6 M6): remove the
//! `&RulesBridgeResolver,` line from the slice in `crates/wicked-estate/src/lib.rs` — this test
//! FAILS on the java→DRL edge assertion and on the unresolved-refs assertion.

use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead, NodeKind, Symbol};
use wicked_estate_store::SqliteStore;

/// The W15.13 bridge rule (same text as the extract-crate unit fixture `ODM_BRIDGE_RULE`).
const ODM_BRIDGE_RULE: &str = r#"
[[rule]]
name       = "ibm-odm-invoke"
file_glob  = "**/*.java"
pattern    = 'IlrContext\.execute\(\)|RulesRunner\.run\(\)|IlrSession\.execute\(\)'

[rule.emit_node]
id_template   = "odm:pricing-rules"
label_capture = ""
kind          = "rule_set"
node_scheme   = "ibm-odm"

[rule.emit_edge]
kind               = "invoked_by"
target_id_template = "odm:pricing-rules"
target_node_scheme = "ibm-odm"
"#;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_rulesbridge_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join(".wicked-estate-extractors")).unwrap();
    fs::create_dir_all(d.join("src")).unwrap();
    fs::create_dir_all(d.join("rules")).unwrap();
    d
}

#[test]
fn index_path_produces_rules_bridge_invoked_by_edges() {
    let dir = fresh_dir("e2e");
    fs::write(
        dir.join(".wicked-estate-extractors/odm.toml"),
        ODM_BRIDGE_RULE,
    )
    .unwrap();
    fs::write(
        dir.join("src/PricingService.java"),
        "public class PricingService {\n\
         \x20 public void price() {\n\
         \x20   IlrContext.execute();\n\
         \x20 }\n\
         }\n",
    )
    .unwrap();
    // A real DRL ruleset the TOML rule does not know about — DrlExtractor mints the RuleSet node.
    fs::write(
        dir.join("rules/pricing.drl"),
        "package com.example.pricing\n\
         \n\
         rule \"Base price\"\n\
         when\n\
         \x20   $o : Order()\n\
         then\n\
         \x20   $o.setPrice(100);\n\
         end\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    let nodes = GraphRead::all_nodes(&store).unwrap();
    let java_file_sym = Symbol::file("src/PricingService.java").id();
    let drl_ruleset = nodes
        .iter()
        .find(|n| n.kind == NodeKind::RuleSet && n.location.file == "rules/pricing.drl")
        .expect("DrlExtractor must mint a RuleSet node for the package");
    let synthetic_ruleset_sym = Symbol::synthetic("ibm-odm", "odm:pricing-rules").id();
    let synthetic_ruleset = nodes
        .iter()
        .find(|n| n.kind == NodeKind::RuleSet && n.symbol == synthetic_ruleset_sym)
        .expect("the bridge rule must mint the synthetic RuleSet node");

    let invoked_by: Vec<_> = GraphRead::all_edges(&store)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::InvokedBy)
        .collect();

    // (a) The NEW capability: java call site → the REAL DRL RuleSet, from the wired resolver.
    let to_drl = invoked_by
        .iter()
        .find(|e| e.source == java_file_sym && e.target == drl_ruleset.symbol)
        .expect("wired RulesBridgeResolver must link the java call site to the DRL RuleSet");
    assert_eq!(to_drl.resolved_by, "rules-bridge-resolver");
    assert!(
        (to_drl.confidence.get() - 0.5).abs() < 1e-6,
        "Heuristic tier confidence 0.5, got {}",
        to_drl.confidence.get()
    );

    // (b) The D7 overwrite, asserted explicitly so a future change is deliberate: the extractor's
    // own file→synthetic-RuleSet edge (Heuristic 0.5, Provenance::Extractor(rule)) is overwritten
    // by the resolver's equal-confidence edge (sqlite upsert uses >=; resolved edges land after
    // local edges), flipping resolved_by from the rule name to the resolver id.
    let to_synth = invoked_by
        .iter()
        .find(|e| e.source == java_file_sym && e.target == synthetic_ruleset.symbol)
        .expect("the synthetic-RuleSet InvokedBy edge must exist");
    assert_eq!(
        to_synth.resolved_by, "rules-bridge-resolver",
        "the resolver's equal-confidence edge overwrites the extractor's (documented D7 semantics)"
    );

    // (c) The bridge ref must NOT be parked: before the wiring, `rules-engine:ibm-odm` sat in
    // unresolved_refs.
    let parked = store
        .unresolved_refs_for_name("rules-engine:ibm-odm")
        .unwrap();
    assert!(
        parked.is_empty(),
        "rules-engine:* refs must be consumed by the wired resolver, found {} parked",
        parked.len()
    );

    // (d) RulesInventory-level view asserted via all_edges (the retrieve-crate constructor is not
    // reachable from this test crate): both InvokedBy edges above ARE the inventory's rows —
    // RulesInventory filters EdgeKind::InvokedBy only.
    assert!(
        invoked_by.len() >= 2,
        "expected at least the DRL and synthetic InvokedBy edges, got {}",
        invoked_by.len()
    );

    let _ = fs::remove_dir_all(&dir);
}
