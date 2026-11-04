//! The knowledge plane over JSON-RPC (DES-KNOWLEDGE-DATA-001 §4, §10.1, build brief B3a):
//! `knowledge.replace_source`, `knowledge.source_catalog`, `visible`-filtered `knowledge.recall`,
//! `knowledge.neighbors`, and knowledge proposals with `relate[]` / `relate_code[]` whose approval
//! derives the deps and is invisible until complete (with the start-up sweep).
//!
//! Driven through the unified dispatch against REAL engines on lane-local in-memory stores.

use std::sync::Arc;

use serde_json::{Value, json};
use wicked_estate_core::RetrievalTool;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::tools::proposals::sweep_knowledge_approvals;
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified_ro};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::{MemoryApi, ProposalState};
use wicked_estate_overlay::XedgeStore;
use wicked_estate_store::SqliteStore;

struct Rig {
    graph: SqliteStore,
    mem: MemoryEngine,
    know: KnowledgeEngine,
    xedge: Arc<XedgeStore>,
}

fn rig() -> Rig {
    let xedge = Arc::new(XedgeStore::in_memory().unwrap());
    Rig {
        graph: SqliteStore::in_memory().unwrap(),
        mem: MemoryEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::clone(&xedge)),
        know: KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::clone(&xedge)),
        xedge,
    }
}

impl Rig {
    fn call_ro(&mut self, tool: &str, args: Value, read_only: bool) -> Value {
        let req = json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        });
        let mut handles = DomainHandles {
            memory: &mut self.mem as &mut dyn MemoryApi<Error = anyhow::Error>,
            knowledge: &mut self.know as &mut dyn KnowledgeApi,
        };
        handle_request_unified_ro(
            &self.graph,
            &req,
            &McpContext::default(),
            Some(&mut handles),
            None::<&dyn RetrievalTool>,
            read_only,
        )
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        self.call_ro(tool, args, false)
    }

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let resp = self.call(tool, args);
        assert_eq!(
            resp["result"]["isError"], false,
            "{tool} must succeed: {resp}"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }
}

fn generation(n: u64, tables: &[&str]) -> Value {
    json!({
        "sourceId": "3f0c-support",
        "generation": n,
        "label": "support-pg",
        "catalog": {"dialect": "postgres"},
        "docs": tables.iter().map(|t| json!({
            "key": t,
            "entry": {"kind": "table"},
            "sections": [{"key": "columns", "text": format!("{t} columns: id int8, customer_id int8, region text, status text enum(open, closed)")}],
        })).collect::<Vec<_>>(),
    })
}

const SEE: &str = "source:3f0c-support";

#[test]
fn replace_source_then_recall_catalog_and_tombstone_over_jsonrpc() {
    let mut r = rig();
    // A malformed generation is invalid params and writes nothing.
    let mut bad = generation(1, &["support.tickets"]);
    bad["docs"][0]["sections"] = json!([]);
    let resp = r.call("knowledge.replace_source", bad);
    assert_eq!(resp["error"]["code"], -32602, "{resp}");

    let out = r.ok(
        "knowledge.replace_source",
        generation(1, &["support.tickets", "support.refunds"]),
    );
    assert_eq!(out["docs"], 2);
    assert_eq!(out["tombstoned"]["nodes"], 0);

    // recall without visible: the catalog is hidden; with it: items carry scope/deps/untrusted.
    let hidden = r.ok(
        "knowledge.recall",
        json!({"query": "tickets region status"}),
    );
    assert!(hidden["items"].as_array().unwrap().is_empty(), "{hidden}");
    let seen = r.ok(
        "knowledge.recall",
        json!({"query": "tickets region status", "visible": [SEE]}),
    );
    let item = &seen["items"][0];
    assert_eq!(item["scope"], "source:3f0c-support");
    assert_eq!(item["deps"], json!([SEE]));
    assert_eq!(item["untrusted"], true);
    assert_eq!(item["class"], "section");
    // A malformed visible entry is invalid params, never a silently smaller set.
    let resp = r.call(
        "knowledge.recall",
        json!({"query": "tickets", "visible": ["support-pg"]}),
    );
    assert_eq!(resp["error"]["code"], -32602, "{resp}");

    let cat = r.ok(
        "knowledge.source_catalog",
        json!({"sourceId": "3f0c-support", "visible": [SEE]}),
    );
    assert_eq!(cat["generation"], 1);
    assert_eq!(cat["total_docs"], 2);
    assert_eq!(cat["untrusted"], true);
    assert_eq!(cat["docs"][1]["key"], "support.tickets");
    let resp = r.call(
        "knowledge.source_catalog",
        json!({"sourceId": "3f0c-support"}),
    );
    assert_eq!(
        resp["result"]["isError"], true,
        "hidden without visible: {resp}"
    );

    // Generation 2 drops a table: it is tombstoned.
    let out = r.ok(
        "knowledge.replace_source",
        generation(2, &["support.tickets"]),
    );
    assert_eq!(out["tombstoned"]["docs"], json!(["support.refunds"]));
    // An older generation is a tool error that writes nothing.
    let resp = r.call("knowledge.replace_source", generation(1, &[]));
    assert_eq!(resp["result"]["isError"], true, "{resp}");
    let cat = r.ok(
        "knowledge.source_catalog",
        json!({"sourceId": "3f0c-support", "visible": [SEE]}),
    );
    assert_eq!(
        (cat["generation"].as_u64(), cat["total_docs"].as_u64()),
        (Some(2), Some(1))
    );

    // A read-only server refuses the writer and keeps the readers.
    let resp = r.call_ro("knowledge.replace_source", generation(3, &[]), true);
    assert!(resp.get("error").is_some(), "{resp}");
    let resp = r.call_ro(
        "knowledge.source_catalog",
        json!({"sourceId": "3f0c-support", "visible": [SEE]}),
        true,
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
}

fn tickets_node(r: &mut Rig) -> String {
    let seen = r.ok(
        "knowledge.recall",
        json!({"query": "support.tickets columns", "visible": [SEE]}),
    );
    seen["items"][0]["node_id"].as_str().unwrap().to_string()
}

fn shape(tickets: &str, deps: Option<Value>) -> Value {
    let mut payload = json!({
        "content": "Open tickets per region: support.tickets.region groups tickets; each ticket is a Ticket",
        "source": "data-shapes:support-pg",
        "class": "concept",
        "relate": [{"rel": "derived_from", "to": tickets}],
        "relate_code": [{
            "rel": "describes", "from": tickets, "code_id": "rust::support::Ticket",
            "repo": "support-app", "evidence": {"kind": "name_type"}
        }],
    });
    if let Some(d) = deps {
        payload["deps"] = d;
    }
    json!({"kind_type": "knowledge", "payload": payload})
}

#[test]
fn shape_proposal_derives_deps_and_is_visible_only_where_all_deps_are() {
    let mut r = rig();
    r.ok(
        "knowledge.replace_source",
        generation(1, &["support.tickets"]),
    );
    let tickets = tickets_node(&mut r);

    // A malformed link is refused at submit (the proposer can still fix it).
    let mut bad = shape(&tickets, None);
    bad["payload"]["relate"][0]["rel"] = json!("about");
    let resp = r.call("proposal.submit", bad);
    assert_eq!(resp["error"]["code"], -32602, "{resp}");

    // A proposal filed under a repo facet (the relay stamps the unit's own) may only link that
    // repo's code: naming another is refused at submit.
    let mut other_repo = shape(&tickets, None);
    other_repo["facets"] = json!({"repo": "billing-app"});
    let resp = r.call("proposal.submit", other_repo);
    assert_eq!(resp["error"]["code"], -32602, "{resp}");
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("is not this proposal's repo"),
        "{resp}"
    );

    // NEGATIVE CONTROL: declaring fewer deps than the endpoints derive is refused at approve.
    let pid = r.ok("proposal.submit", shape(&tickets, Some(json!([SEE]))))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = r.call("proposal.approve", json!({"id": pid}));
    assert_eq!(resp["error"]["code"], -32603, "{resp}");
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("differ from the deps its endpoints derive"),
        "{resp}"
    );
    assert_eq!(
        r.mem
            .list_proposals(Some("knowledge"), Some(ProposalState::Pending))
            .unwrap()
            .len(),
        1,
        "the refused proposal stays pending"
    );
    assert!(r.know.applying_proposals().unwrap().is_empty());
    assert_eq!(r.xedge.typed_len().unwrap(), 0, "nothing written");

    // The same shape without a declaration: approve derives source + repo.
    let pid = r.ok("proposal.submit", shape(&tickets, None))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let approved = r.ok("proposal.approve", json!({"id": pid}));
    assert_eq!(approved["outcome"], "promoted");
    assert_eq!(
        approved["unverified_code_endpoints"], 1,
        "the attached graph does not hold the code symbol: {approved}"
    );
    let shape_id = approved["active_id"].as_str().unwrap().to_string();
    assert!(
        r.know.applying_proposals().unwrap().is_empty(),
        "journal dropped"
    );

    let both = json!([SEE, "repo:support-app"]);
    let hits = r.ok(
        "knowledge.recall",
        json!({"query": "open tickets per region", "visible": both}),
    );
    let item = hits["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["node_id"] == shape_id.as_str())
        .unwrap_or_else(|| panic!("the shape is visible with both deps: {hits}"));
    assert_eq!(item["deps"], json!(["repo:support-app", SEE]));
    assert_eq!(item["untrusted"], false);
    let partial = r.ok(
        "knowledge.recall",
        json!({"query": "open tickets per region", "visible": [SEE]}),
    );
    assert!(
        partial["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["node_id"] != shape_id.as_str()),
        "a reader without the repo never sees the shape: {partial}"
    );

    let n = r.ok(
        "knowledge.neighbors",
        json!({"id": tickets, "depth": 2, "visible": both}),
    );
    let rels: Vec<&str> = n["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["rel"].as_str().unwrap())
        .collect();
    assert!(
        rels.contains(&"describes") && rels.contains(&"derived_from"),
        "{n}"
    );
    let from_code = r.ok(
        "knowledge.neighbors",
        json!({"id": "rust::support::Ticket", "store": "estate", "visible": both}),
    );
    assert!(
        from_code["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == tickets.as_str()),
        "a code seed reaches the table that describes it: {from_code}"
    );
    let resp = r.call(
        "knowledge.neighbors",
        json!({"id": tickets, "depth": 9, "visible": both}),
    );
    assert_eq!(resp["error"]["code"], -32602, "depth is bounded: {resp}");
}

/// NEGATIVE CONTROL: a crash between the node and its edges leaves nothing visible after the
/// sweep; the proposal stays pending and approves cleanly afterwards.
#[test]
fn the_start_up_sweep_rolls_back_a_half_done_approval() {
    let mut r = rig();
    r.ok(
        "knowledge.replace_source",
        generation(1, &["support.tickets"]),
    );
    let tickets = tickets_node(&mut r);
    let pid = r.ok("proposal.submit", shape(&tickets, None))["id"]
        .as_str()
        .unwrap()
        .to_string();
    // A process that died after writing the node, before the queue marked it approved.
    let proposal = r
        .mem
        .list_proposals(Some("knowledge"), Some(ProposalState::Pending))
        .unwrap()
        .into_iter()
        .find(|p| p.id == pid)
        .unwrap();
    let shape_id = r.know.write_proposal(&proposal, 2).unwrap();
    assert!(r.xedge.typed_len().unwrap() > 0);

    // A young journal may be another process's approval in progress: the sweep leaves it (hidden).
    assert!(
        sweep_knowledge_approvals(&mut r.mem, &mut r.know, 3)
            .unwrap()
            .is_empty()
    );
    assert!(r.xedge.typed_len().unwrap() > 0);
    let log = sweep_knowledge_approvals(&mut r.mem, &mut r.know, 10_000).unwrap();
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].contains("rolled back"), "{log:?}");
    assert_eq!(r.xedge.typed_len().unwrap(), 0);
    let both = json!([SEE, "repo:support-app"]);
    let hits = r.ok(
        "knowledge.recall",
        json!({"query": "open tickets per region", "visible": both}),
    );
    assert!(
        hits["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["node_id"] != shape_id.as_str()),
        "nothing of the half-done approval is visible after the sweep: {hits}"
    );
    // Still pending; a clean approve now lands it, and a second sweep finds nothing to do.
    let approved = r.ok("proposal.approve", json!({"id": pid}));
    assert_eq!(approved["active_id"], shape_id.as_str());
    assert!(
        sweep_knowledge_approvals(&mut r.mem, &mut r.know, 20_000)
            .unwrap()
            .is_empty()
    );
}

/// An approval interrupted AFTER the queue marked it approved is completed, never undone.
#[test]
fn the_sweep_completes_an_approved_proposal() {
    let mut r = rig();
    let pid = r.ok(
        "proposal.submit",
        json!({"kind_type": "knowledge", "payload": {"content": "refunds over 500 need a manager", "source": "docs/refunds.md"}}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    // Approve through the queue but keep the journal (as if the process died before step 5).
    let mut writer =
        |p: &wicked_estate_memory_core::Proposal, now: i64| r.know.write_proposal(p, now);
    r.mem
        .approve_proposal_with_knowledge(&pid, 2, &mut writer)
        .unwrap();
    let journals = r.know.applying_proposals().unwrap();
    assert_eq!(
        (journals.len(), journals[0].proposal_id.as_str()),
        (1, pid.as_str())
    );
    // Hidden while its journal exists, even though the queue already approved it.
    let hits = r.ok("knowledge.recall", json!({"query": "refunds manager"}));
    assert!(hits["items"].as_array().unwrap().is_empty(), "{hits}");
    // An approved one is completed at any age.
    let log = sweep_knowledge_approvals(&mut r.mem, &mut r.know, 3).unwrap();
    assert!(log[0].contains("completed"), "{log:?}");
    assert!(r.know.applying_proposals().unwrap().is_empty());
    let hits = r.ok("knowledge.recall", json!({"query": "refunds manager"}));
    assert_eq!(hits["items"].as_array().unwrap().len(), 1, "{hits}");
}

const TRK: &str = "trk_0123456789abcdef";

fn tracker(n: u64, targets: &[&str]) -> Value {
    json!({
        "sourceId": TRK,
        "generation": n,
        "label": "PROJA",
        "catalog": {"system": "jira", "tenant": "site-1"},
        "docs": [{
            "key": "tracker",
            "title": "PROJA tracker",
            "entry": {"kind": "tracker"},
            "sections": [{"key": "workflow", "text": "PROJA workflow states: To Do todo, In Progress doing, Done done"}],
            "links": targets.iter().map(|t| json!({
                "rel": "tracked_in", "dir": "out", "end": {"store": "control", "id": t},
            })).collect::<Vec<_>>(),
        }],
    })
}

/// DES-PM-001 §3.4 over JSON-RPC: a tracker generation with links; `project:` in `visible`;
/// `knowledge.neighbors` from a `control` start; hidden and malformed starts; links refused under
/// `--readonly` and when malformed; a later generation drops a link.
#[test]
fn tracker_links_over_jsonrpc() {
    let mut r = rig();
    let out = r.ok(
        "knowledge.replace_source",
        tracker(1, &["project:proj_P", "repo:r"]),
    );
    assert_eq!(
        out["links"],
        json!({"written": 2, "stale_removed": 0, "cleanup": null})
    );
    assert_eq!(out["orphaned_edges"]["count"], 0);
    let see = json!([format!("source:{TRK}"), "project:proj_P", "repo:r"]);

    let items = r.ok(
        "knowledge.recall",
        json!({"query": "PROJA workflow states", "visible": see}),
    )["items"]
        .clone();
    assert!(
        items.as_array().unwrap().iter().any(|i| i["source"]
            .as_str()
            .unwrap()
            .starts_with(&format!("source:{TRK}/"))),
        "{items}"
    );

    let n = r.ok(
        "knowledge.neighbors",
        json!({"store": "control", "id": "project:proj_P", "visible": see}),
    );
    let nodes = n["nodes"].as_array().unwrap();
    assert_eq!(nodes[0]["store"], "control");
    assert_eq!(nodes[0]["deps"], json!(["project:proj_P"]));
    assert_eq!(nodes[0]["hydrated"], false);
    assert!(nodes.iter().any(|x| x["store"] == "knowledge"));
    let edge = &n["edges"][0];
    assert_eq!(edge["rel"], "tracked_in");
    assert_eq!(
        edge["to"],
        json!({"store": "control", "id": "project:proj_P"})
    );
    assert_eq!(
        edge["meta"]["deps"],
        json!(["project:proj_P", format!("source:{TRK}")])
    );
    assert_eq!(edge["meta"]["generation"], 1);

    // A hidden start answers like an unknown one; a malformed control id is invalid params.
    let hidden = r.call(
        "knowledge.neighbors",
        json!({"store": "control", "id": "project:proj_P", "visible": [format!("source:{TRK}")]}),
    );
    assert_eq!(hidden["result"]["isError"], true, "{hidden}");
    for bad in ["project:a/b", "issue:x", "pr:r#0", "run:"] {
        let resp = r.call(
            "knowledge.neighbors",
            json!({"store": "control", "id": bad}),
        );
        assert_eq!(resp["error"]["code"], -32602, "{bad}: {resp}");
    }
    let resp = r.call("knowledge.neighbors", json!({"store": "nope", "id": "x"}));
    assert_eq!(resp["error"]["code"], -32602);

    // Malformed links are invalid params and write nothing.
    let mut bad = tracker(2, &["project:proj_P"]);
    bad["docs"][0]["links"][0]["deps"] = json!(["project:proj_P"]);
    let resp = r.call("knowledge.replace_source", bad);
    assert_eq!(resp["error"]["code"], -32602, "{resp}");
    let mut bad = tracker(2, &["run:run_1"]);
    bad["docs"][0]["links"][0]["rel"] = json!("addresses");
    assert_eq!(
        r.call("knowledge.replace_source", bad)["error"]["code"],
        -32602,
        "a run end needs its filing"
    );
    let mut bad = tracker(2, &["project:proj_P"]);
    bad["docs"][0]["links"][0]["dir"] = json!("sideways");
    assert_eq!(
        r.call("knowledge.replace_source", bad)["error"]["code"],
        -32602
    );
    assert_eq!(r.xedge.typed_len().unwrap(), 2, "nothing written");

    // A writer surface: refused under --readonly.
    let resp = r.call_ro("knowledge.replace_source", tracker(2, &["repo:r"]), true);
    assert!(resp.get("error").is_some(), "{resp}");

    // Generation 2 drops the link to P: P's start then shows no tracker.
    let out = r.ok("knowledge.replace_source", tracker(2, &["repo:r"]));
    assert_eq!(out["links"]["written"], 1);
    assert_eq!(out["links"]["stale_removed"], 2);
    let n = r.ok(
        "knowledge.neighbors",
        json!({"store": "control", "id": "project:proj_P", "visible": see}),
    );
    assert_eq!(n["nodes"].as_array().unwrap().len(), 1);
    assert!(n["edges"].as_array().unwrap().is_empty());
    assert_eq!(r.xedge.typed_len().unwrap(), 1);
}
