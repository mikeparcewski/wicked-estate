//! `wicked-estate-memory-api` — the **interface seam** between a host (the estate binary / MCP) and
//! any agent-memory implementation (PR-2; architecture B′ in `wicked-memory/docs/phases/`).
//!
//! This crate is deliberately tiny and dependency-light (only `serde`): it defines the contract so
//! the estate side depends on a TRAIT, never on a concrete memory crate. **`wicked-estate-core` must
//! never depend on this crate** (enforced by `scripts/check-core-firewall.sh`); `wicked-memory`
//! implements [`MemoryApi`], and the estate binary wires a concrete impl behind a cargo feature.
//!
//! Types use plain `String`s (not estate/memory enums) so the seam stays free of either side's
//! internal types — anyone can implement their own memory behind it.

use serde::{Deserialize, Serialize};

/// Capture a new memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// code/infra symbol ids this memory is `about` (cross-edges)
    #[serde(default)]
    pub about: Vec<String>,
}

/// Conversational recall request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallQuery {
    pub query: String,
    pub scope: String,
    /// code/infra seed symbol ids to expand from via `about` edges
    #[serde(default)]
    pub seeds: Vec<String>,
    pub token_budget: usize,
    pub now: i64,
}

/// One recalled item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecalledItem {
    pub id: String,
    pub content: String,
    pub tier: String,
    pub score: f64,
}

/// The memory contract a host calls. Implemented by `wicked-memory` (or any alternative).
///
/// `Error` is associated so implementations keep their own error type without this crate depending
/// on it. Methods take `&mut self` where they write.
pub trait MemoryApi {
    type Error;

    /// Capture a memory (+ optional `about` cross-edges); returns the new memory id.
    fn capture(&mut self, req: CaptureRequest) -> Result<String, Self::Error>;

    /// Conversational recall — the most relevant, token-budgeted slice for `query` in scope.
    fn recall(&self, q: &RecallQuery) -> Result<Vec<RecalledItem>, Self::Error>;

    /// Distill episodic → semantic memory in `scope` (extraction + entity-merge). Returns new facts.
    fn reflect(&mut self, scope: &str, now: i64) -> Result<usize, Self::Error>;
}
