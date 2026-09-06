//! `wicked-estate-memory-core` — the differentiated core of wicked-estate memory.
//!
//! Memory rides the `wicked-estate` graph types (`Node`/`Edge`/`SymbolId`): a memory is a `Node`
//! with `kind = Other("memory")` and its fields in `metadata`; relationships are `Edge`s with
//! `kind = Other(<rel>)`. This crate owns ONLY the memory-native logic that estate cannot
//! provide — tiers, hierarchical scope, the deterministic distillation floor (DEC-R: model-free;
//! the agent is the reasoner), salience/decay math, the recall rerank + token-budget, and the
//! extended MemoryApi trait (DES-001 §4.2–4.3).
//!
//! Store/retrieval wiring lives in sibling crates (L0 integration).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wicked_estate_core::{Language, Location, Node, NodeKind, Span, Symbol, SymbolId};

pub mod facets;
pub mod fuzzy;
pub mod reason;
pub mod recall;
pub mod salience;
pub mod scope;

pub use facets::{Facets, facet_admits};
pub use fuzzy::{fuzzy_candidates, jaccard, normalize};
pub use reason::{Extracted, heuristic_extract, heuristic_same_entity, heuristic_summary};
pub use recall::{Candidate, budget_pack, rrf_fuse};
pub use salience::{Salience, decay, p50, salience, wilson_lower_bound};
pub use scope::{Scope, ScopeParseError};

/// The five memory tiers. Regions of one graph + lifecycle policy (not separate stores).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// T0 working — current session scratchpad, volatile, capacity-bounded.
    Working,
    /// T1 episodic — timestamped turns/events (raw experience).
    Episodic,
    /// T2 semantic — distilled facts/entities/relations (the knowledge graph).
    Semantic,
    /// T3 procedural — learned skills/patterns, reinforced by feedback.
    Procedural,
    /// T4 archival — consolidated/compressed cold storage.
    Archival,
}

impl Tier {
    /// Recall tier-weight (recall §9; defaults — tuned post-benchmark at L5, never magic-frozen).
    pub fn weight(self) -> f64 {
        match self {
            Tier::Working => 1.0,
            Tier::Semantic => 1.0,
            Tier::Procedural => 0.95,
            Tier::Episodic => 0.9,
            Tier::Archival => 0.6,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Working => "working",
            Tier::Episodic => "episodic",
            Tier::Semantic => "semantic",
            Tier::Procedural => "procedural",
            Tier::Archival => "archival",
        }
    }
}

/// What a memory node *is* (rides `metadata.mem_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemKind {
    Working,
    Episode,
    Entity,
    Fact,
    Skill,
    Archive,
}

impl MemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MemKind::Working => "working",
            MemKind::Episode => "episode",
            MemKind::Entity => "entity",
            MemKind::Fact => "fact",
            MemKind::Skill => "skill",
            MemKind::Archive => "archive",
        }
    }
}

/// The node-kind label all memory nodes carry (so estate stores them generically).
pub const MEMORY_NODE_KIND: &str = "memory";
/// Metadata keys (the opaque ExtensionData slot — estate-core never reads these).
pub mod meta_keys {
    pub const MEM_KIND: &str = "mem_kind";
    pub const TIER: &str = "tier";
    pub const SCOPE: &str = "scope";
    pub const CONTENT: &str = "content";
    pub const CREATED_AT: &str = "created_at";
    pub const VALID_AT: &str = "valid_at";
    pub const INVALID_AT: &str = "invalid_at";
    pub const LAST_ACCESS: &str = "last_access";
    pub const ACCESS_COUNT: &str = "access_count";
    pub const REINFORCE_POS: &str = "reinforce_pos";
    pub const REINFORCE_TOTAL: &str = "reinforce_total";
    pub const SALIENCE: &str = "salience";
    /// Orthogonal facet map (DES-MEM-FACETED-001 §4.1) — a JSON object `{axis:value}`, written
    /// ONLY when non-empty so legacy (unfaceted) nodes stay byte-identical.
    pub const FACETS: &str = "facets";
}

/// A memory item, before it is written as an estate `Node`. The builder owns the field layout so
/// capture/recall/consolidation agree on it in one place.
#[derive(Debug, Clone)]
pub struct Memory {
    pub id: String,
    pub kind: MemKind,
    pub tier: Tier,
    pub scope: Scope,
    pub content: String,
    pub created_at: i64,
    pub valid_at: Option<i64>,
    pub invalid_at: Option<i64>,
    pub last_access: i64,
    pub access_count: u64,
    pub reinforce_pos: u64,
    pub reinforce_total: u64,
    /// Orthogonal, intent-matching facets (DES-MEM-FACETED-001). Empty ⇒ specificity 0 ⇒ always
    /// admitted (legacy behavior). Agent-declared at capture as the learning's natural axis.
    pub facets: Facets,
}

impl Memory {
    /// New captured memory (a fresh observation/turn). `now` is unix-seconds (caller-supplied so the
    /// crate stays deterministic/testable — no hidden clock).
    pub fn new(
        kind: MemKind,
        tier: Tier,
        scope: Scope,
        content: impl Into<String>,
        now: i64,
    ) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            kind,
            tier,
            scope,
            content: content.into(),
            created_at: now,
            valid_at: Some(now),
            invalid_at: None,
            last_access: now,
            access_count: 0,
            reinforce_pos: 0,
            reinforce_total: 0,
            facets: Facets::default(),
        }
    }

    /// Builder: attach the orthogonal facets this memory is tagged with (its natural axis).
    pub fn with_facets(mut self, facets: Facets) -> Self {
        self.facets = facets;
        self
    }

    /// Stable estate symbol id for this memory (`Synthetic{scheme:"mem", id:<uuid-v7>}`).
    pub fn symbol(&self) -> SymbolId {
        Symbol::synthetic("mem", self.id.clone()).id()
    }

    /// Current confidence — Wilson lower bound of the reinforcement ratio (calibrated at low n).
    pub fn confidence(&self) -> f64 {
        wilson_lower_bound(self.reinforce_pos, self.reinforce_total)
    }

    /// Reconstruct a `Memory` from an estate `Node` written by [`Memory::to_node`].
    /// Returns `None` if the node is not a memory node / is missing required fields.
    pub fn from_node(node: &Node) -> Option<Memory> {
        match &node.kind {
            NodeKind::Other(k) if k == MEMORY_NODE_KIND => {}
            _ => return None,
        }
        let m = &node.metadata;
        let s = |key: &str| m.get(key).and_then(|v| v.as_str()).map(|s| s.to_string());
        let i = |key: &str| m.get(key).and_then(|v| v.as_i64());
        let u = |key: &str| m.get(key).and_then(|v| v.as_u64());
        let kind = match s(meta_keys::MEM_KIND)?.as_str() {
            "working" => MemKind::Working,
            "episode" => MemKind::Episode,
            "entity" => MemKind::Entity,
            "fact" => MemKind::Fact,
            "skill" => MemKind::Skill,
            "archive" => MemKind::Archive,
            _ => return None,
        };
        let tier = match s(meta_keys::TIER)?.as_str() {
            "working" => Tier::Working,
            "episodic" => Tier::Episodic,
            "semantic" => Tier::Semantic,
            "procedural" => Tier::Procedural,
            "archival" => Tier::Archival,
            _ => return None,
        };
        // id: strip the "mem synthetic <id>:" rendering back to the uuid (or fall back to symbol str).
        let id = node
            .symbol
            .as_str()
            .rsplit(' ')
            .next()
            .unwrap_or("")
            .trim_end_matches(':')
            .to_string();
        // Facets: an ABSENT key ⇒ empty (legacy nodes — universally visible, unchanged behavior).
        // A PRESENT-but-undecodable/invalid key ⇒ fail CLOSED: skip this memory (`None`) rather
        // than default it to empty facets (= universally visible), which would strip a malformed
        // memory's constraints and risk cross-scope/cross-user leakage. Validated deserialization
        // (`Facets: TryFrom`) means an invalid axis/value here also yields `None`.
        let facets = match m.get(meta_keys::FACETS) {
            None => Facets::default(),
            Some(v) => serde_json::from_value::<Facets>(v.clone()).ok()?,
        };
        Some(Memory {
            id,
            kind,
            tier,
            scope: Scope::parse(&s(meta_keys::SCOPE).unwrap_or_default()),
            content: s(meta_keys::CONTENT).unwrap_or_else(|| node.name.clone()),
            created_at: i(meta_keys::CREATED_AT).unwrap_or(0),
            valid_at: i(meta_keys::VALID_AT),
            invalid_at: i(meta_keys::INVALID_AT),
            last_access: i(meta_keys::LAST_ACCESS).unwrap_or(0),
            access_count: u(meta_keys::ACCESS_COUNT).unwrap_or(0),
            reinforce_pos: u(meta_keys::REINFORCE_POS).unwrap_or(0),
            reinforce_total: u(meta_keys::REINFORCE_TOTAL).unwrap_or(0),
            facets,
        })
    }

    /// Materialize as an estate `Node` (kind `Other("memory")`, fields in `metadata`).
    pub fn to_node(&self) -> Node {
        let mut node = Node::new(
            self.symbol(),
            NodeKind::Other(MEMORY_NODE_KIND.to_string()),
            self.content.clone(),
            Language::new("memory"),
            Location::new("mem", Span::ZERO),
        );
        let m = &mut node.metadata;
        m.insert(meta_keys::MEM_KIND.into(), self.kind.as_str().into());
        m.insert(meta_keys::TIER.into(), self.tier.as_str().into());
        m.insert(meta_keys::SCOPE.into(), self.scope.as_path().into());
        m.insert(meta_keys::CONTENT.into(), self.content.clone().into());
        m.insert(meta_keys::CREATED_AT.into(), self.created_at.into());
        if let Some(v) = self.valid_at {
            m.insert(meta_keys::VALID_AT.into(), v.into());
        }
        if let Some(v) = self.invalid_at {
            m.insert(meta_keys::INVALID_AT.into(), v.into());
        }
        m.insert(meta_keys::LAST_ACCESS.into(), self.last_access.into());
        m.insert(meta_keys::ACCESS_COUNT.into(), self.access_count.into());
        m.insert(meta_keys::REINFORCE_POS.into(), self.reinforce_pos.into());
        m.insert(
            meta_keys::REINFORCE_TOTAL.into(),
            self.reinforce_total.into(),
        );
        // Facets: written ONLY when non-empty so legacy (unfaceted) nodes stay byte-identical.
        if !self.facets.is_empty() {
            if let Ok(v) = serde_json::to_value(&self.facets) {
                m.insert(meta_keys::FACETS.into(), v);
            }
        }
        node
    }
}

// ── MCP wire types + MemoryApi trait (DES-001 §4.2–4.3) ──────────────────────

/// Capture a new memory (MCP: memory.capture).
/// `#[non_exhaustive]` so callers use `..Default::default()` — DES-001 §4.5 breaking-change note.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CaptureRequest {
    pub content: String,
    /// `working|episode|entity|fact|skill|archive`
    pub kind: String,
    /// `working|episodic|semantic|procedural|archival`
    pub tier: String,
    /// canonical scope path, e.g. `"org:acme/agent:claude"` (empty = root)
    pub scope: String,
    /// unix-seconds (caller-owned clock → deterministic)
    pub now: i64,
    /// code/infra symbol ids this memory is `about` (cross-edges); None when no cross-edges needed
    #[serde(default)]
    pub about: Option<Vec<String>>,
    /// pre-fetched estate symbol_epochs for the `about` ids (DES-001 §4.5 ADR-ESTATE-010)
    #[serde(default)]
    pub about_epochs: Option<HashMap<String, u64>>,
    /// Agent-declared facets (DES-MEM-FACETED-001) — the natural axis this learning is about.
    /// Defaults empty; landed on the captured memory node.
    #[serde(default)]
    pub facets: Facets,
}

impl Default for CaptureRequest {
    fn default() -> Self {
        Self {
            content: String::new(),
            kind: "episode".to_string(),
            tier: "episodic".to_string(),
            scope: String::new(),
            now: 0,
            about: None,
            about_epochs: None,
            facets: Facets::default(),
        }
    }
}

/// Conversational recall request (MCP: memory.recall).
///
/// `#[non_exhaustive]` (DES-MEM-FACETED-001 §4.4): adding a field is otherwise a compile break for
/// every cross-crate constructor. External crates build one via [`RecallQuery::new`] + field
/// assignment, never a struct literal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RecallQuery {
    pub query: String,
    pub scope: String,
    /// Subtree-inclusive scope filter. When present, candidate memories are those whose scope
    /// falls WITHIN this prefix's subtree — the same `path_in_prefix` predicate `memory.erase`
    /// and `memory.coverage` use — REPLACING the default ancestor-visible (inheritance) filter
    /// derived from `scope`. `Some("")` = the root subtree = every memory. `None` = the existing
    /// inheritance behavior, exactly. `#[serde(default)]` keeps older serialized queries valid.
    #[serde(default)]
    pub scope_prefix: Option<String>,
    /// code/infra seed symbol ids to expand from via `about` edges
    #[serde(default)]
    pub seeds: Vec<String>,
    pub token_budget: usize,
    pub now: i64,
    /// The session's intent tuple (DES-MEM-FACETED-001 §4.3). AND-composed with `scope` at the
    /// recall rerank gate: a candidate passes iff `facet_admits(mem.facets, intent).is_some()`.
    /// Empty (the default) admits every **unfaceted** memory (specificity 0); a faceted memory is
    /// EXCLUDED unless the intent carries its axes with matching values. Legacy nodes hydrate to no
    /// facets, so legacy recall is preserved exactly. `#[serde(default)]` keeps old queries valid.
    #[serde(default)]
    pub intent: Facets,
}

impl RecallQuery {
    /// Construct a recall query with the required fields; `scope_prefix`/`seeds`/`intent` default
    /// (None / empty / no-facets). This is the constructor cross-crate callers use — a
    /// `#[non_exhaustive]` struct cannot be built with a struct literal from another crate.
    pub fn new(
        query: impl Into<String>,
        scope: impl Into<String>,
        token_budget: usize,
        now: i64,
    ) -> Self {
        Self {
            query: query.into(),
            scope: scope.into(),
            scope_prefix: None,
            seeds: Vec::new(),
            token_budget,
            now,
            intent: Facets::default(),
        }
    }
}

impl Default for RecallQuery {
    fn default() -> Self {
        Self::new(String::new(), String::new(), 0, 0)
    }
}

/// One recalled item returned by memory.recall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecalledItem {
    pub id: String,
    pub content: String,
    pub tier: String,
    pub score: f64,
    /// The memory node's own hierarchical scope (e.g. `"org:acme/agent:claude"`). Always present
    /// on the wire (S4 attribution requirement). Empty string when scope was not recorded at
    /// capture time, or when the node could not be re-hydrated from the store at recall time.
    /// `#[serde(default)]` keeps deserialization backward-compatible: older responses
    /// that predate this field deserialize to `""` (the documented fallback) rather than erroring.
    #[serde(default)]
    pub scope: String,
}

/// Memory counts, optionally scoped (MCP: memory.coverage). HC-007 frozen schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCoverage {
    pub total: u32,
    pub by_tier: HashMap<String, u32>,
    pub by_kind: HashMap<String, u32>,
}

/// Return type of MemoryApi::reflect — carries distilled facts as required by REQ-003 §2.2.
/// The wire format exposes `{ scope, distilled_facts: Vec<String>, node_count: u32 }`.
/// HC-007 frozen field names: `distilled_facts` and `node_count` must NOT be renamed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectResult {
    pub scope: String,
    pub distilled_facts: Vec<String>,
    pub node_count: u32,
}

/// The extended memory contract (DES-001 §4.2). Implemented by `wicked-estate-memory`.
///
/// `Error` is associated so implementations keep their own error type without this crate depending
/// on it. Methods take `&mut self` where they write.
pub trait MemoryApi {
    type Error;

    /// Capture a memory (+ optional `about` cross-edges); returns the new memory id.
    fn capture(&mut self, req: CaptureRequest) -> Result<String, Self::Error>;

    /// Conversational recall — the most relevant, token-budgeted slice for `query` in scope.
    fn recall(&self, q: &RecallQuery) -> Result<Vec<RecalledItem>, Self::Error>;

    /// Distil a scope into semantic facts and write them as T2-tier nodes.
    /// Returns a ReflectResult carrying the distilled text (REQ-003 §2.2 wire contract).
    fn reflect(&mut self, scope: &str, now: i64) -> Result<ReflectResult, Self::Error>;

    /// Hard-delete all memory nodes whose scope starts with `scope_prefix`.
    /// Returns `Err` if `scope_prefix` is empty (refuses total wipe).
    /// Implementations MUST also remove associated xedge entries (DES-001 §4.4).
    fn erase(&mut self, scope_prefix: &str, now: i64) -> Result<u32, Self::Error>;

    /// Store a T2/T3-tier fact and create about-edges to `symbols` atomically.
    /// `symbol_epochs` is pre-fetched by the dispatch layer (DES-001 §4.5 ADR-ESTATE-010).
    fn learn(
        &mut self,
        fact: &str,
        symbols: &[String],
        symbol_epochs: &HashMap<String, u64>,
        now: i64,
    ) -> Result<String, Self::Error>;

    /// Return memory counts, optionally scoped. `scope_prefix = None` returns global totals.
    fn coverage(&self, scope_prefix: Option<&str>) -> Result<MemoryCoverage, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_roundtrips_to_node() {
        let s = Scope::parse("org:acme/unit:pay/agent:claude");
        let mem = Memory::new(
            MemKind::Episode,
            Tier::Episodic,
            s.clone(),
            "user prefers oat milk",
            1000,
        );
        let node = mem.to_node();
        assert!(matches!(node.kind, NodeKind::Other(ref k) if k == MEMORY_NODE_KIND));
        assert_eq!(
            node.metadata[meta_keys::TIER],
            serde_json::Value::from("episodic")
        );
        assert_eq!(
            node.metadata[meta_keys::SCOPE],
            serde_json::Value::from("org:acme/unit:pay/agent:claude")
        );
        assert_eq!(node.symbol, Symbol::synthetic("mem", mem.id.clone()).id());
    }

    #[test]
    fn memory_node_roundtrip_both_ways() {
        let s = Scope::parse("org:acme/agent:claude");
        let mut mem = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            s,
            "Stripe is the billing provider",
            1234,
        );
        mem.reinforce_pos = 3;
        mem.reinforce_total = 4;
        mem.access_count = 7;
        let node = mem.to_node();
        let back = Memory::from_node(&node).expect("roundtrip");
        assert_eq!(back.id, mem.id);
        assert_eq!(back.kind, MemKind::Fact);
        assert_eq!(back.tier, Tier::Semantic);
        assert_eq!(back.scope.as_path(), "org:acme/agent:claude");
        assert_eq!(back.content, "Stripe is the billing provider");
        assert_eq!(back.created_at, 1234);
        assert_eq!((back.reinforce_pos, back.reinforce_total), (3, 4));
        assert_eq!(back.access_count, 7);
    }

    #[test]
    fn memory_facets_roundtrip_through_node() {
        let facets = Facets::try_from_map([("cli", "codex"), ("repo", "estate")]).unwrap();
        let mem = Memory::new(
            MemKind::Skill,
            Tier::Procedural,
            Scope::parse("org:acme"),
            "codex needs workspace-write",
            42,
        )
        .with_facets(facets.clone());
        let node = mem.to_node();
        // Persisted as a JSON object under the `facets` meta_key.
        assert!(
            node.metadata[meta_keys::FACETS].is_object(),
            "facets persist as a JSON object"
        );
        let back = Memory::from_node(&node).expect("roundtrip");
        assert_eq!(back.facets, facets, "facets survive to_node → from_node");
    }

    #[test]
    fn legacy_node_without_facets_hydrates_to_empty() {
        // A memory captured before facets existed writes NO `facets` key (to_node skips empty), and
        // must hydrate to empty facets with byte-identical legacy metadata (no new key added).
        let mem = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            Scope::parse("org:acme"),
            "legacy fact",
            7,
        );
        let node = mem.to_node();
        assert!(
            !node.metadata.contains_key(meta_keys::FACETS),
            "an unfaceted memory must NOT add the facets key (legacy nodes stay byte-identical)"
        );
        let back = Memory::from_node(&node).expect("roundtrip");
        assert!(
            back.facets.is_empty(),
            "legacy node hydrates to empty facets"
        );
    }

    #[test]
    fn invalid_facet_metadata_fails_closed() {
        // A node whose `facets` key is PRESENT but invalid must NOT hydrate to empty facets (which
        // would strip constraints and make a malformed memory universally visible). from_node fails
        // CLOSED → None, rather than defaulting to the most-permissive empty facet set.
        let mem = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            Scope::parse("org:acme"),
            "x",
            1,
        );
        let mut node = mem.to_node();
        node.metadata.insert(
            meta_keys::FACETS.to_string(),
            serde_json::json!({ "USER": "bob" }), // uppercase axis fails validation
        );
        assert!(
            Memory::from_node(&node).is_none(),
            "present-but-invalid facets fail closed (skip the memory), not default to empty"
        );
        // The SAME node with a VALID facet blob hydrates fine.
        node.metadata.insert(
            meta_keys::FACETS.to_string(),
            serde_json::json!({ "user": "bob" }),
        );
        let back = Memory::from_node(&node).expect("valid facets hydrate");
        assert_eq!(back.facets.get("user"), Some("bob"));
    }

    #[test]
    fn recall_query_new_and_non_exhaustive() {
        // The cross-crate constructor path: `new` + field assignment (no struct literal).
        let mut q = RecallQuery::new("find the codex quirk", "org:acme", 500, 100);
        assert!(q.scope_prefix.is_none());
        assert!(q.seeds.is_empty());
        assert!(q.intent.is_empty());
        q.intent = Facets::try_from_map([("cli", "codex")]).unwrap();
        assert_eq!(q.intent.get("cli"), Some("codex"));
        // Older serialized queries (no intent field) deserialize to empty intent.
        let de: RecallQuery = serde_json::from_value(
            serde_json::json!({"query":"x","scope":"","token_budget":10,"now":1}),
        )
        .unwrap();
        assert!(de.intent.is_empty());
    }

    #[test]
    fn tier_weights_rank_working_and_semantic_top() {
        assert!(Tier::Working.weight() >= Tier::Episodic.weight());
        assert!(Tier::Semantic.weight() > Tier::Archival.weight());
    }

    #[test]
    fn capture_request_default_and_non_exhaustive() {
        let req = CaptureRequest {
            content: "test".to_string(),
            ..CaptureRequest::default()
        };
        assert_eq!(req.tier, "episodic");
        assert!(req.about.is_none());
        assert!(req.about_epochs.is_none());
    }

    #[test]
    fn reflect_result_has_required_hc007_fields() {
        let r = ReflectResult {
            scope: "org:acme".to_string(),
            distilled_facts: vec!["fact one".to_string()],
            node_count: 1,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert!(json.get("distilled_facts").is_some());
        assert!(json.get("node_count").is_some());
        assert!(json.get("scope").is_some());
    }

    #[test]
    fn memory_coverage_has_by_kind() {
        let cov = MemoryCoverage {
            total: 5,
            by_tier: [
                ("episodic".to_string(), 3u32),
                ("semantic".to_string(), 2u32),
            ]
            .into_iter()
            .collect(),
            by_kind: [("fact".to_string(), 4u32)].into_iter().collect(),
        };
        let json = serde_json::to_value(&cov).unwrap();
        assert!(json.get("by_kind").is_some(), "by_kind is HC-007 required");
        assert!(json.get("by_tier").is_some());
        assert_eq!(json["total"], 5);
    }
}
