//! studio#206 (estate half): `memory.erase` by `id` removes exactly one memory.
//!
//! Before this, erase took only `scope_prefix`, so a management surface that wanted to delete one
//! row had to erase that row's whole scope, and every other memory sharing the scope went with it.

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
            json!({"content": content, "kind": "fact", "tier": "semantic", "scope": "project:wicked"}),
        );
        out["memory_id"].as_str().unwrap().to_string()
    }

    /// (memory_id as `memory.list` reports it, content), sorted by content.
    fn listed(&mut self) -> Vec<(String, String)> {
        let out = self.ok("memory.list", json!({"scope_prefix": "project:wicked"}));
        let mut items: Vec<(String, String)> = out["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["memory_id"].as_str().unwrap().to_string(),
                    i["content"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        items.sort_by(|a, b| a.1.cmp(&b.1));
        items
    }

    fn listed_contents(&mut self) -> Vec<String> {
        self.listed().into_iter().map(|(_, c)| c).collect()
    }
}

#[test]
fn erase_by_list_id_removes_exactly_one_memory_in_a_shared_scope() {
    let mut rig = Rig::new();
    for c in [
        "alpha uses sqlite",
        "beta uses postgres",
        "gamma uses surreal",
    ] {
        rig.capture(c);
    }
    let listed = rig.listed();
    assert_eq!(listed.len(), 3);
    let beta_id = listed[1].0.clone();

    let out = rig.ok("memory.erase", json!({"id": beta_id}));
    assert_eq!(out["deleted_count"], 1, "one memory erased: {out}");
    assert_eq!(
        rig.listed_contents(),
        vec!["alpha uses sqlite", "gamma uses surreal"],
        "the other memories in the scope survive"
    );

    // Idempotent: the id is gone, so a repeat erases nothing.
    let out = rig.ok("memory.erase", json!({"id": beta_id}));
    assert_eq!(out["deleted_count"], 0, "{out}");
    // An id that is not a memory erases nothing.
    let out = rig.ok("memory.erase", json!({"id": "no-such-memory"}));
    assert_eq!(out["deleted_count"], 0, "{out}");
    assert_eq!(rig.listed().len(), 2);
}

#[test]
fn erase_by_capture_id_removes_exactly_one_memory() {
    // capture and recall report the SymbolId form of the id; erase accepts it too.
    let mut rig = Rig::new();
    rig.capture("the deploy target is fly.io");
    let gone = rig.capture("the release branch is trunk");

    let out = rig.ok("memory.erase", json!({"id": gone}));
    assert_eq!(out["deleted_count"], 1, "{out}");
    assert_eq!(rig.listed_contents(), vec!["the deploy target is fly.io"]);
}

#[test]
fn erase_needs_exactly_one_of_id_or_scope_prefix() {
    let mut rig = Rig::new();
    let a = rig.capture("alpha");
    assert!(!a.is_empty());
    for args in [
        json!({}),
        json!({"id": ""}),
        json!({"id": a, "scope_prefix": "project:wicked"}),
    ] {
        let resp = rig.call("memory.erase", args.clone());
        assert_eq!(
            resp["error"]["code"], -32602,
            "{args} must be invalid params: {resp}"
        );
    }
    assert_eq!(rig.listed().len(), 1, "a rejected erase deletes nothing");

    // The subtree erase still works as before.
    rig.capture("beta");
    let out = rig.ok("memory.erase", json!({"scope_prefix": "project:wicked"}));
    assert_eq!(out["deleted_count"], 2, "{out}");
}
