//! Conformance tests: L2.1–L2.4 — v0.13.0 tools/list vs frozen v0.12.x golden schemas.
//!
//! Covers:
//! - L2.1: All 24 tool names from golden files are present in the live tools/list response.
//! - L2.2: Estate tool required fields and property keys match their golden schemas.
//! - L2.3: Memory tool required fields and property keys match their golden schemas.
//! - L2.4: Knowledge tool required fields and property keys match their golden schemas.
//! - L2.4 (count): tools/list with all domains returns exactly 11 estate + 6 memory + 7 knowledge.
//!
//! (Counts raised from 23/10 when arch-R2 added `rules.recall` as the 11th estate tool; the
//! knowledge.recall / knowledge.coverage goldens gained `scope_prefix` with arch-R5 — both
//! ADDITIVE, optional-param changes re-frozen in the golden files.)
//!
//! Golden files live at `tests/conformance/schemas/<ToolName>.json`.
//! The v0.12.x fixture DBs live at `tests/fixtures/`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use wicked_estate_core::RetrievalTool;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::MemoryApi;
use wicked_estate_overlay::XedgeStore;
use wicked_estate_store::SqliteStore;

// ── Tool name constants ────────────────────────────────────────────────────────

const ESTATE_TOOLS: &[&str] = &[
    "SearchEntity",
    "RetrieveEntity",
    "TraverseGraph",
    "BlastRadius",
    "FetchContent",
    "ContextBundle",
    "RulesInventory",
    "rules.recall",
    "RankHotspots",
    "Communities",
    "Lineage",
];

const MEMORY_TOOLS: &[&str] = &[
    "memory.capture",
    "memory.recall",
    "memory.reflect",
    "memory.erase",
    "memory.learn",
    "memory.coverage",
];

const KNOWLEDGE_TOOLS: &[&str] = &[
    "knowledge.ingest",
    "knowledge.write",
    "knowledge.relate",
    "knowledge.recall",
    "knowledge.coverage",
    "knowledge.relate_code",
    "knowledge.recall_about_code",
];

// ── Path helpers ──────────────────────────────────────────────────────────────

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/schemas")
}

// ── Fixture helpers ───────────────────────────────────────────────────────────

/// Copy a fixture file to the temp dir. Silently skips if the source does not exist
/// (used for optional sidecars such as `.memext`). Returns the destination path regardless.
fn copy_fixture(name: &str, dir: &tempfile::TempDir) -> PathBuf {
    let src = fixture_dir().join(name);
    let dst = dir.path().join(name);
    if src.exists() {
        std::fs::copy(&src, &dst)
            .unwrap_or_else(|e| panic!("failed to copy fixture '{name}': {e}"));
    }
    dst
}

// ── Golden schema loader ──────────────────────────────────────────────────────

/// Load and parse a golden schema file from `tests/conformance/schemas/<name>.json`.
fn load_golden(name: &str) -> Value {
    let path = schema_dir().join(format!("{name}.json"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read golden schema '{name}.json' at {path:?}: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse golden schema '{name}.json': {e}"))
}

// ── Live tools/list helper ────────────────────────────────────────────────────

/// Open all 4 fixture stores and issue a `tools/list` request with all domain handles active.
/// Returns a map from tool name → the full tool Value (with `name`, `description`, `inputSchema`).
///
/// Uses `McpContext::default()` — `embedder_meta_id` is `None` — so the dim-guard always fails
/// and SemanticSearch is never advertised, giving the stable 24-tool set.
fn tools_list_map_with_domains() -> HashMap<String, Value> {
    let dir = tempfile::tempdir().expect("create tempdir");

    let estate_path = copy_fixture("estate_v0120.db", &dir);
    let memory_path = copy_fixture("memory_v0121.db", &dir);
    let knowledge_path = copy_fixture("knowledge_v0121.db", &dir);
    let xedge_path = copy_fixture("xedge_v0121.db", &dir);
    // Optional sidecar — copy if present so MemoryEngine::open reconciles it.
    copy_fixture("memory_v0121.db.memext", &dir);

    let store = SqliteStore::open(&estate_path).expect("estate_v0120.db: open must succeed");
    let xedge = Arc::new(
        XedgeStore::open(xedge_path.to_str().unwrap()).expect("xedge_v0121.db: open must succeed"),
    );
    let mut memory = MemoryEngine::open(memory_path.to_str().unwrap())
        .expect("memory_v0121.db: open must succeed")
        .with_xedge_store(Arc::clone(&xedge));
    let mut knowledge = KnowledgeEngine::open(knowledge_path.to_str().unwrap())
        .expect("knowledge_v0121.db: open must succeed")
        .with_xedge_store(Arc::clone(&xedge));

    let ctx = McpContext::default();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    });

    let resp = {
        let mut handles = DomainHandles {
            memory: &mut memory as &mut dyn MemoryApi<Error = anyhow::Error>,
            knowledge: &mut knowledge as &mut dyn KnowledgeApi,
        };
        handle_request_unified(
            &store,
            &req,
            &ctx,
            Some(&mut handles),
            None::<&dyn RetrievalTool>,
        )
    };

    let tools = resp["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list: result.tools must be an array; got: {resp}"));

    tools
        .iter()
        .map(|t| {
            let name = t["name"].as_str().unwrap_or("<?>").to_string();
            (name, t.clone())
        })
        .collect()
}

// ── Schema comparison helpers ─────────────────────────────────────────────────

/// Extract the sorted `required` field names from an `inputSchema` value.
/// Returns an empty vec when `required` is absent (schema has no required fields).
fn sorted_required(schema: &Value) -> Vec<String> {
    let mut req = schema["required"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    req.sort();
    req
}

/// Extract the sorted property key names from an `inputSchema` value.
/// Returns an empty vec when `properties` is absent.
fn sorted_properties(schema: &Value) -> Vec<String> {
    let mut props = schema["properties"]
        .as_object()
        .map(|obj| obj.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    props.sort();
    props
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// L2.1 — Every tool name that appears in a golden file must be present in tools/list.
///
/// This is the broad membership check: no golden tool may go missing from the live server.
#[test]
fn conf_all_24_tool_names_present_in_tools_list() {
    let all_golden_names: Vec<&str> = ESTATE_TOOLS
        .iter()
        .chain(MEMORY_TOOLS.iter())
        .chain(KNOWLEDGE_TOOLS.iter())
        .copied()
        .collect();

    let live = tools_list_map_with_domains();

    let missing: Vec<&&str> = all_golden_names
        .iter()
        .filter(|&&name| !live.contains_key(name))
        .collect();

    assert!(
        missing.is_empty(),
        "tools/list is missing {} golden tool name(s): {:?}\navailable tools: {:?}",
        missing.len(),
        missing,
        {
            let mut names: Vec<&str> = live.keys().map(|s| s.as_str()).collect();
            names.sort();
            names
        }
    );
}

/// L2.2 — For each estate tool, the live `inputSchema.required` array and
/// `inputSchema.properties` key set must match the frozen v0.12.x golden.
#[test]
fn conf_estate_tool_required_fields_match_goldens() {
    let live = tools_list_map_with_domains();

    for &name in ESTATE_TOOLS {
        let golden = load_golden(name);
        let golden_schema = &golden["inputSchema"];

        let live_tool = live
            .get(name)
            .unwrap_or_else(|| panic!("estate tool '{name}' is missing from tools/list"));
        let live_schema = &live_tool["inputSchema"];

        // required
        let golden_req = sorted_required(golden_schema);
        let live_req = sorted_required(live_schema);
        assert_eq!(
            live_req, golden_req,
            "estate tool '{name}': inputSchema.required mismatch\
             \n  live:   {live_req:?}\
             \n  golden: {golden_req:?}"
        );

        // properties keys
        let golden_props = sorted_properties(golden_schema);
        let live_props = sorted_properties(live_schema);
        assert_eq!(
            live_props, golden_props,
            "estate tool '{name}': inputSchema.properties key mismatch\
             \n  live:   {live_props:?}\
             \n  golden: {golden_props:?}"
        );
    }
}

/// L2.3 — For each memory tool, the live `inputSchema.required` array and
/// `inputSchema.properties` key set must match the frozen v0.12.x golden.
#[test]
fn conf_memory_tool_required_fields_match_goldens() {
    let live = tools_list_map_with_domains();

    for &name in MEMORY_TOOLS {
        let golden = load_golden(name);
        let golden_schema = &golden["inputSchema"];

        let live_tool = live
            .get(name)
            .unwrap_or_else(|| panic!("memory tool '{name}' is missing from tools/list"));
        let live_schema = &live_tool["inputSchema"];

        // required
        let golden_req = sorted_required(golden_schema);
        let live_req = sorted_required(live_schema);
        assert_eq!(
            live_req, golden_req,
            "memory tool '{name}': inputSchema.required mismatch\
             \n  live:   {live_req:?}\
             \n  golden: {golden_req:?}"
        );

        // properties keys
        let golden_props = sorted_properties(golden_schema);
        let live_props = sorted_properties(live_schema);
        assert_eq!(
            live_props, golden_props,
            "memory tool '{name}': inputSchema.properties key mismatch\
             \n  live:   {live_props:?}\
             \n  golden: {golden_props:?}"
        );
    }
}

/// L2.4 — For each knowledge tool, the live `inputSchema.required` array and
/// `inputSchema.properties` key set must match the frozen v0.12.x golden.
#[test]
fn conf_knowledge_tool_required_fields_match_goldens() {
    let live = tools_list_map_with_domains();

    for &name in KNOWLEDGE_TOOLS {
        let golden = load_golden(name);
        let golden_schema = &golden["inputSchema"];

        let live_tool = live
            .get(name)
            .unwrap_or_else(|| panic!("knowledge tool '{name}' is missing from tools/list"));
        let live_schema = &live_tool["inputSchema"];

        // required
        let golden_req = sorted_required(golden_schema);
        let live_req = sorted_required(live_schema);
        assert_eq!(
            live_req, golden_req,
            "knowledge tool '{name}': inputSchema.required mismatch\
             \n  live:   {live_req:?}\
             \n  golden: {golden_req:?}"
        );

        // properties keys
        let golden_props = sorted_properties(golden_schema);
        let live_props = sorted_properties(live_schema);
        assert_eq!(
            live_props, golden_props,
            "knowledge tool '{name}': inputSchema.properties key mismatch\
             \n  live:   {live_props:?}\
             \n  golden: {golden_props:?}"
        );
    }
}

/// L2.4 (count) — tools/list with all domain handles active must return exactly
/// 11 estate tools, 6 memory tools, and 7 knowledge tools.
///
/// Gated out when `fastembed` or `model2vec` features are active because those features
/// can enable SemanticSearch in the list, changing the total count.
#[test]
#[cfg(not(any(feature = "fastembed", feature = "model2vec")))]
fn conf_tool_count_11_estate_6_memory_7_knowledge() {
    let live = tools_list_map_with_domains();

    let estate_count = ESTATE_TOOLS
        .iter()
        .filter(|&&n| live.contains_key(n))
        .count();
    let memory_count = MEMORY_TOOLS
        .iter()
        .filter(|&&n| live.contains_key(n))
        .count();
    let knowledge_count = KNOWLEDGE_TOOLS
        .iter()
        .filter(|&&n| live.contains_key(n))
        .count();

    let all_live: Vec<&str> = {
        let mut names: Vec<&str> = live.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    };

    assert_eq!(
        estate_count, 11,
        "expected 11 estate tools, found {estate_count}\nall live tools: {all_live:?}"
    );
    assert_eq!(
        memory_count, 6,
        "expected 6 memory tools, found {memory_count}\nall live tools: {all_live:?}"
    );
    assert_eq!(
        knowledge_count, 7,
        "expected 7 knowledge tools, found {knowledge_count}\nall live tools: {all_live:?}"
    );
    assert_eq!(
        live.len(),
        24,
        "expected 24 total tools (11 estate + 6 memory + 7 knowledge), found {}\nall live tools: {all_live:?}",
        live.len()
    );
}
