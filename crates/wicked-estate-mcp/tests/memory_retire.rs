//! estate#376: soft retirement, restoration, and permanent erasure.

use serde_json::{Value, json};
use wicked_estate_core::RetrievalTool;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::MemoryApi;
use wicked_estate_store::SqliteStore;

struct Rig {
    store: SqliteStore,
    memory: MemoryEngine,
    knowledge: KnowledgeEngine,
}

impl Rig {
    fn new() -> Self {
        Rig {
            store: SqliteStore::in_memory().unwrap(),
            memory: MemoryEngine::in_memory().unwrap(),
            knowledge: KnowledgeEngine::in_memory().unwrap(),
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        });
        let mut handles = DomainHandles {
            memory: &mut self.memory as &mut dyn MemoryApi<Error = anyhow::Error>,
            knowledge: &mut self.knowledge as &mut dyn KnowledgeApi,
        };
        handle_request_unified(
            &self.store,
            &req,
            &McpContext::default(),
            Some(&mut handles),
            None::<&dyn RetrievalTool>,
        )
    }

    /// Parse the text payload of a successful tools/call.
    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let resp = self.call(tool, args);
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool}: expected a result, got {resp}"));
        serde_json::from_str(text).unwrap()
    }

    /// Capture into the shared scope; returns the id `memory.capture` (and `memory.recall`)
    /// report, which is the memory's graph `SymbolId`.
    fn capture(&mut self, content: &str) -> String {
        let out = self.ok(
            "memory.capture",
            json!({
                "content": content,
                "kind": "fact",
                "tier": "semantic",
                "scope": "project:wicked"
            }),
        );
        out["memory_id"].as_str().unwrap().to_string()
    }
}

#[test]
fn retire_lists_separately_and_unretire_restores_the_same_recall_id() {
    let mut rig = Rig::new();
    let id = rig.capture("retire quartz");
    let out = rig.ok("memory.retire", json!({"id": id}));
    assert_eq!(out["retired_count"], 1);
    assert_eq!(rig.ok("memory.list", json!({}))["items"], json!([]));
    assert_eq!(
        rig.ok("memory.list", json!({"retired": false}))["items"],
        json!([])
    );
    assert_eq!(rig.ok("memory.coverage", json!({}))["total"], 0);
    let query = json!({"query": "retire quartz", "scope": "project:wicked"});
    assert_eq!(rig.ok("memory.recall", query.clone())["items"], json!([]));
    let out = rig.ok("memory.list", json!({"retired": true}));
    let items = out["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let retired_id = items[0]["memory_id"].as_str().unwrap().to_string();
    assert_eq!(items[0]["content"], "retire quartz");
    assert_eq!(items[0]["scope"], "project:wicked");
    assert_eq!(items[0]["tier"], "semantic");
    assert_eq!(items[0]["facets"], json!({}));
    assert!(items[0]["created_at"].is_i64());
    assert!(items[0]["retired_at"].is_i64());
    let other = rig.ok(
        "memory.list",
        json!({"retired": true, "scope_prefix": "project:other"}),
    );
    assert_eq!(other["items"], json!([]));
    for target in [id.as_str(), "unknown"] {
        assert_eq!(
            rig.ok("memory.retire", json!({"id": target}))["retired_count"],
            0
        );
    }
    let out = rig.ok("memory.unretire", json!({"id": retired_id}));
    assert_eq!(out["restored_count"], 1);
    assert_eq!(rig.ok("memory.recall", query)["items"][0]["memory_id"], id);
    assert_eq!(
        rig.ok("memory.list", json!({"retired": true}))["items"],
        json!([])
    );
    assert!(
        rig.ok("memory.list", json!({}))["items"][0]
            .get("retired_at")
            .is_none()
    );
    assert_eq!(
        rig.ok("memory.unretire", json!({"id": id}))["restored_count"],
        0
    );
}

#[test]
fn erase_by_id_and_scope_remove_retired_memories_and_their_xedges() {
    use std::sync::Arc;
    use wicked_estate_overlay::{XEdge, XedgeStore};
    for by_scope in [false, true] {
        let mut rig = Rig::new();
        let xedge = Arc::new(XedgeStore::in_memory().unwrap());
        rig.memory = rig.memory.with_xedge_store(xedge.clone());
        let id = rig.capture("retire quartz");
        xedge
            .put_edge(&XEdge::about(id.clone(), "code-seed", 0))
            .unwrap();
        rig.capture("surviving quartz");
        assert_eq!(
            rig.ok("memory.retire", json!({"id": id}))["retired_count"],
            1
        );
        assert_eq!(
            xedge.len().unwrap(),
            1,
            "retirement keeps the xedge for undo"
        );
        let args = if by_scope {
            json!({"scope_prefix": "project:wicked"})
        } else {
            json!({"id": id})
        };
        let out = rig.ok("memory.erase", args);
        assert_eq!(out["deleted_count"], if by_scope { 2 } else { 1 });
        assert_eq!(
            rig.ok("memory.list", json!({"retired": true}))["items"],
            json!([])
        );
        assert_eq!(
            rig.ok("memory.unretire", json!({"id": id}))["restored_count"],
            0
        );
        assert!(
            xedge
                .reader()
                .out_edges("memory", &id, &["about"])
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn retire_and_unretire_require_a_nonempty_string_id_and_list_requires_boolean() {
    let mut rig = Rig::new();
    rig.capture("quartz");
    for tool in ["memory.retire", "memory.unretire"] {
        for args in [
            json!({}),
            json!({"id": ""}),
            json!({"id": 1}),
            json!({"id": null}),
        ] {
            let resp = rig.call(tool, args);
            assert_eq!(resp["error"]["code"], -32602, "{resp}");
        }
    }
    for retired in [json!("true"), json!(1)] {
        let resp = rig.call("memory.list", json!({"retired": retired}));
        assert_eq!(resp["error"]["code"], -32602, "{resp}");
    }
    // A null `retired` is treated as omitted: the live list, not an error.
    let resp = rig.call("memory.list", json!({"retired": Value::Null}));
    assert!(resp["error"].is_null(), "{resp}");
}
