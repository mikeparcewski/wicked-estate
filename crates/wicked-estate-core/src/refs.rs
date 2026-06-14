//! The two-phase pipeline staging types (EXTRACT → RESOLVE).
//!
//! Extraction is per-file and parallelizable and emits [`UnresolvedRef`]s — raw references by
//! *name*, not yet bound to a target [`crate::SymbolId`]. The resolver pass runs once the whole
//! project's symbols are known and turns refs into [`Edge`]s. Decoupling the two phases means
//! resolution can be swapped without re-parsing (Wave 1.1) and avoids file-ordering bugs
//! (the `unresolved_refs` pattern; see the design notes).

use crate::edge::{Edge, EdgeKind};
use crate::node::{Location, Metadata, Node};
use crate::symbol::SymbolId;
use serde::{Deserialize, Serialize};

/// A reference captured during extraction but not yet resolved to a target symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnresolvedRef {
    /// The symbol making the reference (the dependent → future edge `source`).
    pub from: SymbolId,
    /// The name as written at the reference site (may be unqualified).
    pub raw_name: String,
    /// The edge kind this reference becomes once resolved.
    pub kind: EdgeKind,
    pub location: Location,
    /// Resolution hints: imports in scope, receiver-type guesses, arity, etc.
    #[serde(default)]
    pub hints: Metadata,
}

impl UnresolvedRef {
    pub fn new(
        from: SymbolId,
        raw_name: impl Into<String>,
        kind: EdgeKind,
        location: Location,
    ) -> Self {
        Self {
            from,
            raw_name: raw_name.into(),
            kind,
            location,
            hints: Metadata::new(),
        }
    }
}

/// The output of an [`crate::Extractor`] for one file: nodes, intra-file edges known at parse
/// time (e.g. `contains`, `defines`), and unresolved refs for the resolver pass.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Extraction {
    pub nodes: Vec<Node>,
    pub local_edges: Vec<Edge>,
    pub refs: Vec<UnresolvedRef>,
}
