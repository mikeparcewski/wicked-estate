//! Change log for reactive delta subscription (Wave 7.1; prior art's graph-query-subscribe pattern).
//!
//! Logged at **file granularity** (one delta per changed/removed file, NOT per node/edge) so the
//! log never explodes during bulk indexing. Subscribers resume from the last `seq` they saw.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    /// A file was indexed or re-indexed.
    Upsert,
    /// A file was removed from the graph.
    Remove,
}

/// One delta in the change log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// Monotonic cursor — a subscriber resumes from the last `seq` it processed.
    pub seq: u64,
    pub op: ChangeOp,
    /// What changed — a repo-relative file path.
    pub target: String,
}
