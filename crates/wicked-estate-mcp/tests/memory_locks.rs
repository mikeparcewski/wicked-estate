//! estate#364 PR C: pack locks and recall-equivalent list admission.

use serde_json::{Value, json};
use wicked_estate_core::{GraphWrite, Node, RetrievalTool, Symbol};
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified};
use wicked_estate_memory::{MemoryEngine, PackMemory, PackRef, apply_pack_memories, pack_memories};
use wicked_estate_memory_core::{Facets, MemKind, Memory, MemoryApi, Scope, Tier, memory_lock};
use wicked_estate_store::SqliteStore;

fn memory(id: &str, scope: &str) -> Memory {
    Memory::new(
        MemKind::Fact,
        Tier::Semantic,
        Scope::parse(scope),
        "quartz policy",
        100,
    )
    .with_id(id)
}

fn locked(id: &str, scope: &str) -> Node {
    let mut node = memory(id, scope).to_node();
    node.metadata.insert(
        "lock".into(),
        json!({
            "pack": "fixture",
            "layer": if scope.starts_with("team:") { "team" } else { "org" },
            "version": "v1"
        }),
    );
    node
}

struct Rig {
    store: SqliteStore,
    memory: MemoryEngine,
    knowledge: KnowledgeEngine,
}

impl Rig {
    fn seeded(nodes: &[Node]) -> Self {
        let mut backend = SqliteStore::in_memory().unwrap();
        backend.upsert_nodes(nodes).unwrap();
        Self {
            store: SqliteStore::in_memory().unwrap(),
            memory: MemoryEngine::with_backend(Box::new(backend), ":memory:").unwrap(),
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
}

#[test]
fn locked_writes_refuse_at_mcp_and_preserve_the_complete_inventory() {
    let node = locked("fixture:policy", "org:fixture");
    let mut rig = Rig::seeded(std::slice::from_ref(&node));
    let before = rig.ok("memory.list", json!({}));
    for (tool, args) in [
        ("memory.erase", json!({"id": "fixture:policy"})),
        ("memory.erase", json!({"scope_prefix": "org:fixture"})),
        ("memory.retire", json!({"id": "fixture:policy"})),
        (
            "memory.capture",
            json!({"content": "overwrite", "scope": "org:fixture"}),
        ),
        (
            "memory.capture",
            json!({"content": "overwrite", "scope": "org:fixture", "about": ["code-seed"]}),
        ),
        (
            "memory.capture",
            json!({"content": "overwrite", "scope": "team:fixture"}),
        ),
    ] {
        let response = rig.call(tool, args);
        assert_eq!(response["error"]["code"], -32603, "{response}");
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("memory_locked")
        );
        assert_eq!(rig.ok("memory.list", json!({})), before);
        assert_eq!(
            serde_json::to_value(rig.memory.node(&node.symbol).unwrap()).unwrap(),
            serde_json::to_value(Some(&node)).unwrap(),
        );
    }
    let replacement = memory("fixture:policy", "project:x");
    let error = rig.memory.capture(&replacement).unwrap_err().to_string();
    assert!(
        error.contains("memory_locked: fixture:policy fixture@v1"),
        "{error}"
    );
    let learned = rig.ok(
        "memory.learn",
        json!({"content": "ordinary learning", "symbols": []}),
    );
    let learned_id = learned["memory_id"].as_str().unwrap();
    let learned_node = rig
        .memory
        .node(&wicked_estate_core::SymbolId(learned_id.into()))
        .unwrap()
        .unwrap();
    assert!(memory_lock(&learned_node).is_none());
    assert_eq!(
        rig.ok("memory.list", json!({}))["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        serde_json::to_value(rig.memory.node(&node.symbol).unwrap()).unwrap(),
        serde_json::to_value(Some(&node)).unwrap(),
    );
}

#[test]
fn mixed_subtree_erase_is_wholly_refused_and_names_every_lock() {
    let mut rig = Rig::seeded(&[
        locked("fixture:first", "org:fixture"),
        locked("fixture:second", "org:fixture"),
        memory("ordinary", "org:fixture/unit:y").to_node(),
    ]);
    let before = rig.ok("memory.list", json!({}));
    let response = rig.call("memory.erase", json!({"scope_prefix": "org:fixture"}));
    assert_eq!(response["error"]["code"], -32603, "{response}");
    let message = response["error"]["message"].as_str().unwrap();
    for id in ["fixture:first", "fixture:second"] {
        assert!(message.contains(id), "{message}");
    }
    assert_eq!(rig.ok("memory.list", json!({})), before);
}

#[test]
fn recall_inherits_only_locked_one_segment_layers() {
    let mut rig = Rig::seeded(&[
        locked("fixture:policy", "org:fixture"),
        locked("fixture:team", "team:fixture"),
        locked("fixture:deep", "org:fixture/unit:y"),
        memory("other", "org:other/unit:y").to_node(),
        memory("unlocked-org", "org:unlocked").to_node(),
    ]);
    let out = rig.ok(
        "memory.recall",
        json!({"query": "quartz policy", "scope": "project:x"}),
    );
    let scopes: Vec<&str> = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["scope"].as_str().unwrap())
        .collect();
    assert_eq!(scopes.len(), 2, "{out}");
    assert!(scopes.contains(&"org:fixture"));
    assert!(scopes.contains(&"team:fixture"));
}

#[test]
fn admitted_list_matches_recall_scope_and_facet_gates_without_ranking() {
    let mut nodes = vec![
        locked("fixture:policy", "org:fixture"),
        memory("operator", "operator:local").to_node(),
    ];
    for project in ["p1", "p2"] {
        nodes.push(
            memory(project, "")
                .with_facets(Facets::try_from_map([("project", project)]).unwrap())
                .to_node(),
        );
    }
    let mut rig = Rig::seeded(&nodes);
    let out = rig.ok(
        "memory.list",
        json!({"admit_for": {"scope": "project:x", "intent": {"project": "p1"}}}),
    );
    let ids: Vec<&str> = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["memory_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3, "{out}");
    for id in ["p1", "operator", "fixture:policy"] {
        assert!(ids.contains(&id), "{out}");
    }
    let recalled = rig.ok(
        "memory.recall",
        json!({"query": "quartz policy", "scope": "project:x", "intent": {"project": "p1"}}),
    );
    assert_eq!(recalled["items"].as_array().unwrap().len(), ids.len());
}

#[test]
fn admitted_list_rejects_conflicting_or_malformed_arguments() {
    let mut rig = Rig::seeded(&[]);
    for args in [
        json!({"admit_for": {"scope": "project:x"}, "scope_prefix": ""}),
        json!({"admit_for": {"scope": "project:x"}, "retired": true}),
        json!({"admit_for": []}),
        json!({"admit_for": {"scope": "broken"}}),
        json!({"admit_for": {"scope": 1}}),
        json!({"admit_for": {"scope": "project:x", "intent": {"project": 1}}}),
        json!({"admit_for": {"scope": "project:x", "intent": []}}),
    ] {
        let response = rig.call("memory.list", args);
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
}

#[test]
fn pack_apply_hashes_projection_and_reconciles_versions_and_removals() {
    let mut engine = MemoryEngine::in_memory().unwrap();
    let pack = PackRef::new("fixture", "org", "v1");
    let first = PackMemory::new("first", MemKind::Fact, Tier::Semantic, "quartz policy")
        .with_facets(Facets::try_from_map([("project", "p1")]).unwrap());
    let second = PackMemory::new(
        "second",
        MemKind::Skill,
        Tier::Procedural,
        "quartz procedure",
    );
    let report = apply_pack_memories(&mut engine, &pack, &[first.clone(), second]).unwrap();
    assert_eq!((report.written, report.unchanged, report.erased), (2, 0, 0));
    let sym = Symbol::synthetic("mem", "fixture:first").id();
    let original = engine.node(&sym).unwrap().unwrap();
    let lock = memory_lock(&original).unwrap();
    assert_eq!(
        (
            lock.pack.as_str(),
            lock.layer.as_str(),
            lock.version.as_str()
        ),
        ("fixture", "org", "v1")
    );
    assert_eq!(original.metadata["scope"], "org:fixture");
    assert!(original.metadata["created_at"].as_i64().unwrap() > 0);
    let hash = original.metadata["lock"]["hash"].as_str().unwrap();
    assert_eq!(hash.len(), 16);
    // Pack identity and version are outside the content projection.
    let mut other = MemoryEngine::in_memory().unwrap();
    apply_pack_memories(
        &mut other,
        &PackRef::new("other", "team", "v9"),
        std::slice::from_ref(&first),
    )
    .unwrap();
    let other_sym = Symbol::synthetic("mem", "other:first").id();
    assert_eq!(
        other.node(&other_sym).unwrap().unwrap().metadata["lock"]["hash"],
        hash
    );
    // Every member of the content projection affects the hash independently.
    for field in ["id", "kind", "tier", "content", "facets"] {
        let mut changed = first.clone();
        match field {
            "id" => changed.id = "renamed".into(),
            "kind" => changed.kind = MemKind::Entity,
            "tier" => changed.tier = Tier::Procedural,
            "content" => changed.content = "different content".into(),
            "facets" => {
                changed.facets = Facets::try_from_map([("project", "p2")]).unwrap();
            }
            _ => unreachable!(),
        }
        let sym = Symbol::synthetic("mem", format!("other:{}", changed.id)).id();
        apply_pack_memories(&mut other, &PackRef::new("other", "team", "v9"), &[changed]).unwrap();
        assert_ne!(
            other.node(&sym).unwrap().unwrap().metadata["lock"]["hash"],
            hash
        );
    }
    let read = pack_memories(&engine, "fixture").unwrap();
    assert_eq!(read.len(), 2);
    assert!(
        read.iter()
            .any(|m| m.id == "first" && m.facets == first.facets)
    );
    let report = apply_pack_memories(&mut engine, &pack, &read).unwrap();
    assert_eq!((report.written, report.unchanged, report.erased), (0, 2, 0));
    assert_eq!(
        serde_json::to_value(engine.node(&sym).unwrap()).unwrap(),
        serde_json::to_value(Some(&original)).unwrap()
    );
    let report = apply_pack_memories(
        &mut engine,
        &PackRef::new("fixture", "org", "v2"),
        std::slice::from_ref(&first),
    )
    .unwrap();
    assert_eq!((report.written, report.unchanged, report.erased), (1, 0, 1));
    assert_eq!(
        memory_lock(&engine.node(&sym).unwrap().unwrap())
            .unwrap()
            .version,
        "v2"
    );
    assert_eq!(pack_memories(&engine, "fixture").unwrap().len(), 1);
    let mut changed = first;
    changed.content = "changed quartz policy".into();
    let report = apply_pack_memories(
        &mut engine,
        &PackRef::new("fixture", "org", "v2"),
        &[changed],
    )
    .unwrap();
    assert_eq!(report.written, 1);
    assert_ne!(
        engine.node(&sym).unwrap().unwrap().metadata["lock"]["hash"],
        hash
    );
    assert_eq!(
        apply_pack_memories(&mut engine, &pack, &[]).unwrap().erased,
        1
    );
    assert!(pack_memories(&engine, "fixture").unwrap().is_empty());
}

#[test]
fn pack_apply_validates_all_inputs_before_writing() {
    let mut engine = MemoryEngine::in_memory().unwrap();
    let good = PackMemory::new("good", MemKind::Fact, Tier::Semantic, "quartz");
    for tier in [Tier::Working, Tier::Episodic, Tier::Archival] {
        let bad = PackMemory::new("bad", MemKind::Fact, tier, "quartz");
        assert!(
            apply_pack_memories(
                &mut engine,
                &PackRef::new("fixture", "org", "v1"),
                &[good.clone(), bad],
            )
            .is_err()
        );
        assert_eq!(engine.count().unwrap(), 0);
    }
    assert!(
        apply_pack_memories(
            &mut engine,
            &PackRef::new("fixture", "user", "v1"),
            std::slice::from_ref(&good),
        )
        .is_err()
    );
    assert_eq!(engine.count().unwrap(), 0);
    let pack = PackRef::new("fixture", "org", "v1");
    assert!(apply_pack_memories(&mut engine, &pack, &[good.clone(), good.clone()]).is_err());
    assert_eq!(engine.count().unwrap(), 0);
    let existing = memory("fixture:good", "project:x");
    engine.capture(&existing).unwrap();
    let fresh = PackMemory::new("fresh", MemKind::Fact, Tier::Semantic, "fresh quartz");
    assert!(apply_pack_memories(&mut engine, &pack, &[fresh, good.clone()]).is_err());
    assert_eq!(
        engine.count().unwrap(),
        1,
        "ownership failures must not partially apply"
    );
    assert!(memory_lock(&engine.node(&existing.symbol()).unwrap().unwrap()).is_none());

    let mut corrupt = locked("fixture:good", "org:fixture");
    corrupt
        .metadata
        .insert("lock".into(), json!({"pack": "fixture"}));
    let mut rig = Rig::seeded(&[corrupt]);
    assert!(pack_memories(&rig.memory, "fixture").is_err());
    let error = apply_pack_memories(&mut rig.memory, &pack, &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("memory_locked"), "{error}");
}
