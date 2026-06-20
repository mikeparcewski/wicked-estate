//! Query + result types for the read side of the [`crate::GraphStore`] and retrieval tools.

use crate::edge::{Direction, Edge, EdgeKind};
use crate::node::{Language, Node, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A symbol search. Free-text ranking (BM25) is the store's job; this is the request shape.
#[derive(Debug, Clone, Default)]
pub struct SymbolQuery {
    /// Free-text query (BM25 over name/signature/doc in stores that support it).
    pub text: Option<String>,
    /// Exact simple-name match.
    pub exact_name: Option<String>,
    /// Restrict to these node kinds (empty = any).
    pub kinds: Vec<NodeKind>,
    pub language: Option<Language>,
    pub limit: Option<usize>,
    /// Restrict results to a scope subtree by canonical path prefix (e.g. `"org:acme"`), matching
    /// that scope and its descendants. `None` = all scopes. The predicate is pushed into the store
    /// SQL **before** any `LIMIT`, so top-k ranking never leaks across scopes (multi-tenant isolation).
    pub scope_prefix: Option<String>,
}

/// A **bounded** traversal request. We deliberately support only bounded reverse-reachability /
/// k-hop (the actual agent workload; see the design notes).
/// `max_depth` and `max_nodes` are required guard rails — unbounded whole-graph walks are out.
#[derive(Debug, Clone)]
pub struct TraversalSpec {
    pub direction: Direction,
    /// Edge kinds to follow (empty = all).
    pub edge_kinds: Vec<EdgeKind>,
    pub max_depth: u32,
    pub max_nodes: usize,
    /// Ignore edges below this confidence.
    pub min_confidence: f32,
}

impl TraversalSpec {
    /// Blast-radius preset: walk `Dependents` along `Calls` edges up to `max_depth`.
    pub fn blast_radius(max_depth: u32) -> Self {
        Self {
            direction: Direction::Dependents,
            // ALL edge kinds, not just Calls. The contract invariant is source=dependent /
            // target=dependency for EVERY edge, so a complete blast radius follows every
            // dependency kind backwards — `uses` (JCL step → dataset), `protects` (RACF profile →
            // asset), `accesses`, imports, references, type/heritage edges. Calls-only silently
            // under-reported estate + non-call dependents (an asset read as "nothing depends on
            // me"), the exact failure mode the engine must never have.
            edge_kinds: vec![],
            max_depth,
            max_nodes: 5_000,
            min_confidence: 0.0,
        }
    }
}

/// A subgraph returned by a traversal, with per-node distance from the start.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Subgraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Distance from the start node, keyed by `SymbolId` string.
    pub depths: BTreeMap<String, u32>,
    /// True if a cap (`max_depth` / `max_nodes`) truncated the result.
    pub truncated: bool,
}

/// Aggregate counts for health / staleness / coverage reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStats {
    pub node_count: u64,
    pub edge_count: u64,
    pub file_count: u64,
    pub unresolved_ref_count: u64,
    pub nodes_by_kind: BTreeMap<String, u64>,
    pub edges_by_kind: BTreeMap<String, u64>,
    /// On-disk database size in bytes. Zero for in-memory stores.
    #[serde(default)]
    pub db_size_bytes: u64,
}

/// Result envelope for a [`crate::RetrievalTool`] invocation. `diagnostics` carries the
/// agent-behavior signals (staleness, coverage warnings, `GRAPH-FALLBACK:` markers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub content: serde_json::Value,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl RetrievalResult {
    pub fn new(content: serde_json::Value) -> Self {
        Self {
            content,
            diagnostics: Vec::new(),
        }
    }
}
