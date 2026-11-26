//! estate#619: `memory.learn` honours its `scope` and `tier` arguments, or refuses them. It never
//! silently stores a project fact at the root scope, where every project's recall inherits it.

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
        Self {
            store: SqliteStore::in_memory().unwrap(),
            memory: MemoryEngine::with_backend(
                Box::new(SqliteStore::in_memory().unwrap()),
                ":memory:",
            )
            .unwrap(),
            knowledge: KnowledgeEngine::in_memory().unwrap(),
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": tool, "arguments": args}
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

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let response = self.call(tool, args);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool}: {response}"));
        serde_json::from_str(text).unwrap()
    }

    fn listed(&mut self, scope_prefix: &str) -> Vec<Value> {
        self.ok("memory.list", json!({"scope_prefix": scope_prefix}))["items"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn recalled(&mut self, scope: &str) -> Vec<Value> {
        self.ok(
            "memory.recall",
            json!({"query": "clamp guard bounds", "scope": scope, "token_budget": 2000}),
        )["items"]
            .as_array()
            .unwrap()
            .clone()
    }
}

const FACT: &str = "clamp is the guard every caller relies on for bounds";

#[test]
fn learn_stores_at_the_given_scope_and_tier_and_stays_out_of_other_projects_619() {
    let mut rig = Rig::new();
    let learned = rig.ok(
        "memory.learn",
        json!({"content": FACT, "symbols": [], "scope": "project:a", "tier": "procedural"}),
    );
    let learned_id = learned["memory_id"].as_str().unwrap().to_string();

    let in_a = rig.listed("project:a");
    let item = in_a
        .iter()
        .find(|m| m["memory_id"].as_str() == Some(learned_id.as_str()))
        .unwrap_or_else(|| panic!("learned memory not listed under project:a: {in_a:?}"));
    assert_eq!(item["scope"], "project:a", "{item}");
    assert_eq!(item["tier"], "procedural", "{item}");

    // Recall inherits from ancestor scopes, so a root-scoped memory would surface here.
    let from_b = rig.recalled("project:b");
    assert!(
        from_b.iter().all(|m| m["content"].as_str() != Some(FACT)),
        "a fact learned for project:a leaked into project:b's recall: {from_b:?}"
    );
    let from_a = rig.recalled("project:a");
    assert!(
        from_a.iter().any(|m| m["content"].as_str() == Some(FACT)),
        "project:a's own recall misses its learned fact: {from_a:?}"
    );
}

#[test]
fn learn_without_scope_or_tier_keeps_the_root_semantic_default_619() {
    let mut rig = Rig::new();
    let learned = rig.ok("memory.learn", json!({"content": FACT, "symbols": []}));
    let learned_id = learned["memory_id"].as_str().unwrap().to_string();
    let all = rig.listed("");
    let item = all
        .iter()
        .find(|m| m["memory_id"].as_str() == Some(learned_id.as_str()))
        .unwrap_or_else(|| panic!("learned memory not listed: {all:?}"));
    assert_eq!(item["scope"], "", "{item}");
    assert_eq!(item["tier"], "semantic", "{item}");
}

#[test]
fn learn_refuses_a_scope_or_tier_it_cannot_honour_and_writes_nothing_619() {
    let mut rig = Rig::new();
    for args in [
        json!({"content": FACT, "symbols": [], "scope": "no-colon"}),
        json!({"content": FACT, "symbols": [], "scope": "project:"}),
        json!({"content": FACT, "symbols": [], "scope": 7}),
        json!({"content": FACT, "symbols": [], "tier": "working"}),
        json!({"content": FACT, "symbols": [], "tier": "nonsense"}),
        json!({"content": FACT, "symbols": [], "tier": 2}),
    ] {
        let response = rig.call("memory.learn", args.clone());
        assert_eq!(response["error"]["code"], -32602, "{args} -> {response}");
    }
    assert!(rig.listed("").is_empty(), "a refused learn wrote a memory");
}
