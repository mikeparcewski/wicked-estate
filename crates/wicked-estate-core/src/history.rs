//! Read-only history log of superseded edges (Wave 7 — the brain remembers old connections).
//!
//! When a file is re-indexed, the edges its *previous* version produced are appended here, tagged
//! with that version's git blob SHA, BEFORE the live edges are replaced. This log is **read-only
//! and never traversed** — it can never affect blast-radius, ranking, or live queries — and is
//! bounded by a retention window pruned during `compact()`. It answers "what did this file call at
//! version X?" without bloating the live graph.
//!
//! This is the deliberate reconciliation of two requirements that pull opposite ways: "keep old
//! connections keyed by git file sha" vs. the fragmentation/cleanup discipline. History is
//! *segregated and prunable*, not inlined into the live edge set.

use crate::edge::Edge;
use serde::{Deserialize, Serialize};

/// One superseded edge, preserved exactly as it was resolved at a past file version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalEdge {
    /// Git blob SHA of the file version this edge belonged to (`git hash-object` of that content).
    pub git_sha: String,
    /// Append order — the highest `archived_seq` is the most recently superseded version.
    pub archived_seq: u64,
    /// The edge as it stood at that version (full `{confidence, provenance, resolved_by}` retained).
    pub edge: Edge,
}
