//! Semantic annotations that link code symbols to requirements.
//!
//! Three columns hang off every node, set via [`GraphWrite::set_node_semantics`](crate::GraphWrite)
//! and read via [`GraphRead::node_semantics`](crate::GraphRead) / `find_by_requirement`:
//!   * `description` — what the symbol *is* (human/LLM prose),
//!   * `requirement` — the requirement it matches/fulfils,
//!   * `requirement_validated` — whether that match has been validated as actually true.
//!
//! These power semantic linking: "which functionality satisfies requirement R?", "what is still
//! unvalidated?", "describe this symbol" — the requirement↔functionality graph alongside the
//! structural call/import graph.

use serde::{Deserialize, Serialize};

/// Semantic annotations attached to a single code symbol (node).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSemantics {
    /// What is this symbol? (free-text description)
    pub description: Option<String>,
    /// The requirement this symbol matches / fulfils.
    pub requirement: Option<String>,
    /// The matched requirement has been validated as actually satisfied by this symbol.
    pub requirement_validated: bool,
}
