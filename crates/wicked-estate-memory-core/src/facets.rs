//! Faceted memory partitioning (DES-MEM-FACETED-001 §4.3) — the orthogonal, intent-matching
//! dimension layered ON TOP of the hierarchical `Scope` (tenancy/write-isolation stays unchanged).
//!
//! A memory is tagged at capture with the natural axis its learning is *about* (a CLI quirk →
//! `cli:codex`, a repo gotcha → `repo:x`); recall reads the session's **intent** and admits the
//! unique combination it needs — "include what is needed, nothing else". This ports the faceted
//! model estate already proves for `rules.recall` (`rules_recall.rs`), with ONE deliberate
//! divergence in the admission predicate (see [`facet_admits`]).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An ordered set of `axis → value` facet bindings on a memory (or on a recall intent tuple).
///
/// `BTreeMap` gives deterministic iteration/serialization (mirrors rules' `Targets`), so a memory's
/// facets round-trip byte-stably through `node.metadata`. Serializes transparently as a JSON object
/// (`{"cli":"codex","repo":"x"}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Facets(BTreeMap<String, String>);

/// An axis is a lowercase token `^[a-z][a-z0-9_-]*$` (aligned to the intent axes: `cli`, `repo`,
/// `user`, `project`, `tool`, …). Hand-rolled to avoid a regex dependency in the core crate.
fn valid_axis(axis: &str) -> bool {
    let mut chars = axis.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl Facets {
    /// Insert a **validated** `axis → value` binding. Fail-loud (`Err`) when the axis is not a
    /// lowercase token or the value is empty — a malformed facet silently mis-routes recall, so it
    /// is rejected at the door rather than stored.
    pub fn insert(
        &mut self,
        axis: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), String> {
        let axis = axis.into();
        let value = value.into();
        if !valid_axis(&axis) {
            return Err(format!(
                "invalid facet axis {axis:?}: must match ^[a-z][a-z0-9_-]*$"
            ));
        }
        if value.is_empty() {
            return Err(format!("facet value for axis {axis:?} must be non-empty"));
        }
        self.0.insert(axis, value);
        Ok(())
    }

    /// Build a validated `Facets` from an arbitrary `axis → value` map, failing loud on the first
    /// invalid binding (same rules as [`Facets::insert`]).
    pub fn try_from_map<I, K, V>(map: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut f = Facets::default();
        for (axis, value) in map {
            f.insert(axis, value)?;
        }
        Ok(f)
    }

    /// The value bound to `axis`, if any.
    pub fn get(&self, axis: &str) -> Option<&str> {
        self.0.get(axis).map(String::as_str)
    }

    /// No facets constrained (⇒ specificity 0, always admitted).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Count of bound axes (the specificity of a fully-matched memory).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Deterministic (`axis`-ordered) iteration over the bindings.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }
}

/// The faceted-recall admission predicate (DES-MEM-FACETED-001 §4.3).
///
/// For EVERY `(axis, value)` present on `mem`, `intent` must carry that axis with the SAME value.
/// An axis ABSENT on `mem` is a wildcard (the memory does not constrain it). A memory that
/// constrains an axis the intent does NOT carry is **EXCLUDED** (`None`).
///
/// This is the deliberate divergence from rules' `facet_matches`
/// (`wicked-estate-retrieve/src/rules_recall.rs`), which *includes*-on-None: here an unsatisfiable
/// constraint EXCLUDES, to prevent cross-user / cross-repo leakage — "include what is needed,
/// nothing else". Example: a `user:bob` memory with no session `user` in the intent must NOT leak.
///
/// Returns `Some(specificity)` when admitted, where `specificity` = the count of the memory's
/// present facets (all of which matched). Empty `mem` ⇒ `Some(0)` — always admitted, so legacy
/// (unfaceted) memories behave exactly as before.
pub fn facet_admits(mem: &Facets, intent: &Facets) -> Option<usize> {
    for (axis, val) in mem.iter() {
        match intent.get(axis) {
            Some(iv) if iv == val => {}
            _ => return None,
        }
    }
    Some(mem.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `Facets` from validated pairs (test helper — panics on an invalid axis/value).
    fn facets(pairs: &[(&str, &str)]) -> Facets {
        Facets::try_from_map(pairs.iter().map(|(a, v)| (*a, *v))).expect("valid facets")
    }

    #[test]
    fn empty_mem_always_admits() {
        // Empty memory facets ⇒ Some(0), regardless of intent (legacy memories always surface).
        assert_eq!(
            facet_admits(&Facets::default(), &Facets::default()),
            Some(0)
        );
        assert_eq!(
            facet_admits(
                &Facets::default(),
                &facets(&[("cli", "codex"), ("repo", "z")])
            ),
            Some(0)
        );
    }

    #[test]
    fn cli_learning_travels_across_repos() {
        // {cli:codex} vs intent {cli:codex, repo:z} ⇒ Some(1): a CLI learning is reusable in every
        // repo (the memory does not constrain `repo`, so the intent's repo is a wildcard match).
        assert_eq!(
            facet_admits(
                &facets(&[("cli", "codex")]),
                &facets(&[("cli", "codex"), ("repo", "z")])
            ),
            Some(1)
        );
    }

    #[test]
    fn wrong_repo_value_is_excluded() {
        // {repo:x} vs intent {repo:z} ⇒ None: a repo gotcha does not cross to another repo.
        assert_eq!(
            facet_admits(&facets(&[("repo", "x")]), &facets(&[("repo", "z")])),
            None
        );
    }

    #[test]
    fn constrained_axis_absent_from_intent_is_excluded() {
        // {user:bob} vs intent with NO user ⇒ None (the divergence from rules' include-on-None):
        // no cross-user leakage.
        assert_eq!(
            facet_admits(&facets(&[("user", "bob")]), &facets(&[("cli", "codex")])),
            None
        );
        assert_eq!(
            facet_admits(&facets(&[("user", "bob")]), &Facets::default()),
            None
        );
    }

    #[test]
    fn full_match_returns_specificity() {
        // {cli:codex, repo:x} vs {cli:codex, repo:x, user:u} ⇒ Some(2): every present facet matched.
        assert_eq!(
            facet_admits(
                &facets(&[("cli", "codex"), ("repo", "x")]),
                &facets(&[("cli", "codex"), ("repo", "x"), ("user", "u")])
            ),
            Some(2)
        );
    }

    #[test]
    fn partial_match_is_excluded() {
        // A 2-facet memory where only one axis matches ⇒ None (AND semantics).
        assert_eq!(
            facet_admits(
                &facets(&[("cli", "codex"), ("repo", "x")]),
                &facets(&[("cli", "codex"), ("repo", "z")])
            ),
            None
        );
    }

    #[test]
    fn invalid_axis_and_empty_value_rejected_fail_loud() {
        let mut f = Facets::default();
        assert!(f.insert("CLI", "codex").is_err(), "uppercase axis rejected");
        assert!(
            f.insert("1cli", "codex").is_err(),
            "leading-digit axis rejected"
        );
        assert!(
            f.insert("cli.name", "codex").is_err(),
            "dot not in the token class"
        );
        assert!(f.insert("", "codex").is_err(), "empty axis rejected");
        assert!(f.insert("cli", "").is_err(), "empty value rejected");
        assert!(f.is_empty(), "no invalid binding was stored");
        // Valid axes: lowercase, digits, `_`/`-` after the first letter.
        assert!(f.insert("cli", "codex").is_ok());
        assert!(f.insert("estate-mcp", "on").is_ok());
        assert!(f.insert("a_b2", "v").is_ok());
        assert_eq!(f.len(), 3);
        // try_from_map fails loud on the first invalid pair.
        assert!(Facets::try_from_map([("ok", "v"), ("BAD", "v")]).is_err());
    }

    #[test]
    fn round_trips_as_json_object() {
        let f = facets(&[("cli", "codex"), ("repo", "x")]);
        let v = serde_json::to_value(&f).unwrap();
        assert!(v.is_object(), "Facets serializes as a JSON object, got {v}");
        assert_eq!(v["cli"], "codex");
        let back: Facets = serde_json::from_value(v).unwrap();
        assert_eq!(back, f);
    }
}
