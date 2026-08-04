//! Semantic annotations that link code symbols to requirements.
//!
//! Three columns hang off every node, set via [`GraphWrite::set_node_semantics`](crate::GraphWrite)
//! and read via [`GraphRead::node_semantics`](crate::GraphRead) / `find_by_requirement`:
//!   * `description` — what the symbol *is* (human/LLM prose),
//!   * `requirement` — the requirement it matches/fulfils,
//!   * `requirement_validated` — whether that match has been validated as actually true, and BY
//!     WHOM (see [`ValidationClaim`]).
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
    /// WHO asserted that, free-form, mirroring [`crate::Annotation::author`]. `None` on rows written
    /// before authorship was recorded — an unattributable claim, which is the state this field
    /// exists to make visible.
    pub requirement_validated_by: Option<String>,
    /// When the assertion was made (unix seconds). `None` for the same reason as above.
    pub requirement_validated_at: Option<i64>,
}

/// An assertion that a symbol's `requirement` really is satisfied — and who is making it.
///
/// # Why this is a struct and not a `bool`
///
/// It used to be `Option<bool>`, so a caller could set `requirement_validated = true` with nothing
/// recording who decided that. A consuming platform observed an agent write 46 distinct strings as
/// the `requirement` of 34,897 nodes, every one self-validated: coverage computed 1.0, the pinned
/// validator passed, and the resulting requirements were file-name titles over reference lists
/// (wicked-core#131). Its own governance rule is evaluator≠creator — "structurally can't
/// self-grade" — and it could not be applied here, because the data model had nowhere to put the
/// distinction.
///
/// Pairing the flag with its author makes the unattributed claim unrepresentable: you cannot assert
/// validation without saying who is asserting it. Same posture as `Edge`, which never carries a
/// resolution without `{confidence, provenance, resolved_by}`.
///
/// This records the claim. Whether a self-validated requirement should then be rejected, downgraded
/// or merely reported is the CONSUMER's policy (tracked in wicked-core#131) — the store's job is to
/// make the question answerable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationClaim {
    /// Whether the requirement is asserted satisfied. `false` retracts a previous claim.
    pub validated: bool,
    /// Who/what is asserting it — a username, an agent id, `"system"`. Never blank: a claim
    /// attributed to nobody is the thing this type exists to prevent.
    pub by: String,
}

impl ValidationClaim {
    /// A claim, rejecting a blank author — the one way this type could still carry nothing.
    pub fn new(validated: bool, by: impl Into<String>) -> Result<Self, &'static str> {
        let by = by.into();
        if by.trim().is_empty() {
            return Err("a validation claim needs an author; pass the actor asserting it");
        }
        Ok(Self { validated, by })
    }
}
