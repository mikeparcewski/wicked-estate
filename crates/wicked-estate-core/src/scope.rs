//! Hierarchical ownership / partition scope — a general estate primitive (not memory-specific).
//!
//! A [`Scope`] is an ordered path of `kind:id` segments, e.g.
//! `org:acme / unit:payments / project:checkout / sprint:24 / agent:claude`. Kinds are
//! caller-defined. It partitions a graph so a multi-tenant / multi-repo / multi-agent deployment can
//! store everything in one store and **filter by ownership** without leaking across tenants.
//!
//! Identity note (ADR-002): scope is an **additive attribute** on nodes; it is deliberately NOT part
//! of [`crate::SymbolId`], so stable code identity is unchanged and the default (root) scope keeps
//! every existing graph behaving exactly as before.

use serde::{Deserialize, Serialize};

/// One `kind:id` segment of a scope path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopeSeg {
    pub kind: String,
    pub id: String,
}

/// An ordered, hierarchical ownership/partition path. Empty = root (matches everything).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Scope(pub Vec<ScopeSeg>);

impl Scope {
    /// The root scope.
    pub fn root() -> Self {
        Scope(Vec::new())
    }

    /// Parse `"org:acme/unit:pay"`. Empty / malformed segments are skipped; `""` → root.
    pub fn parse(path: &str) -> Self {
        let segs = path
            .split('/')
            .filter(|s| !s.is_empty())
            .filter_map(|s| {
                let (kind, id) = s.split_once(':')?;
                if kind.is_empty() || id.is_empty() {
                    return None;
                }
                Some(ScopeSeg {
                    kind: kind.to_string(),
                    id: id.to_string(),
                })
            })
            .collect();
        Scope(segs)
    }

    /// Canonical string form (`"org:acme/unit:pay"`; root → `""`). This is what the store persists
    /// and what [`crate::SymbolQuery::scope_prefix`] is matched against.
    pub fn as_path(&self) -> String {
        self.0
            .iter()
            .map(|s| format!("{}:{}", s.kind, s.id))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Is `self` an ancestor of (or equal to) `other`? (segment prefix — the inheritance rule.)
    pub fn is_ancestor_of(&self, other: &Scope) -> bool {
        other.0.len() >= self.0.len() && other.0[..self.0.len()] == self.0[..]
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Allocation-free equivalent of `path_in_prefix(&self.as_path(), prefix)` — does this scope
    /// fall within `prefix`'s subtree? Walks the prefix against the segments' virtual `kind:id/…`
    /// rendering instead of materializing the path, so per-candidate checks (e.g. the recall
    /// rerank loop) don't allocate.
    ///
    /// Equivalence holds for parse-normalized scopes ([`Scope::parse`] splits on `/`, so segment
    /// kinds/ids can never contain one) — which is every scope decoded from a store. A prefix that
    /// is not itself a canonical rendering (partial segment, trailing `/`, missing `:`) matches
    /// nothing, exactly as the string predicate behaves against canonical paths.
    pub fn path_in_prefix(&self, prefix: &str) -> bool {
        if prefix.is_empty() {
            return true; // root subtree — matches everything
        }
        let mut rem = prefix;
        for seg in &self.0 {
            // The segment's virtual rendering is "{kind}:{id}"; consume it from the prefix.
            rem = match rem.strip_prefix(seg.kind.as_str()) {
                Some(r) => r,
                None => return false,
            };
            rem = match rem.strip_prefix(':') {
                Some(r) => r,
                None => return false,
            };
            rem = match rem.strip_prefix(seg.id.as_str()) {
                Some(r) => r,
                None => return false,
            };
            if rem.is_empty() {
                return true; // prefix ends exactly at a segment boundary — subtree hit
            }
            rem = match rem.strip_prefix('/') {
                Some(r) => r,
                None => return false, // prefix continues mid-segment ("org:acme" vs "org:acme2")
            };
        }
        false // prefix has segments beyond this scope — deeper than the candidate
    }
}

/// Does a node's canonical scope path fall within `prefix`'s subtree? (the `scope_prefix` predicate.)
/// An empty prefix (root) matches everything; otherwise matches the exact scope or any descendant.
/// Segment-aware so `"org:acme"` does NOT match `"org:acme2"`.
pub fn path_in_prefix(node_path: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    match node_path.strip_prefix(prefix) {
        Some(rest) => rest.is_empty() || rest.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod prefix_tests {
    use super::{Scope, path_in_prefix};

    #[test]
    fn prefix_is_segment_aware_isolation() {
        assert!(path_in_prefix("org:acme", "org:acme"));
        assert!(path_in_prefix("org:acme/unit:pay", "org:acme"));
        assert!(path_in_prefix("anything", "")); // root prefix
        assert!(!path_in_prefix("org:acme2", "org:acme")); // not a sibling-prefix leak
        assert!(!path_in_prefix("org:other", "org:acme")); // ISOLATION
    }

    /// `Scope::path_in_prefix` (allocation-free walk) must agree with the string predicate on
    /// every parse-normalized scope × prefix pair — including non-canonical prefixes.
    #[test]
    fn scope_walk_is_equivalent_to_string_predicate() {
        let scopes = [
            "",
            "org:acme",
            "org:acme2",
            "org:acme/unit:pay",
            "org:acme/unit:pay/agent:claude",
            "brain:wicked-garden/doc:mem%2Fabc.md",
        ];
        let prefixes = [
            "",                                     // root subtree
            "org:acme",                             // exact + descendant
            "org:acme/",                            // trailing slash (non-canonical)
            "org:acme/unit:pay",                    // deeper exact
            "org:acme/unit:pay/agent:claude/x:y",   // deeper than any scope
            "org:acm",                              // partial id
            "org:acme2",                            // sibling
            "org",                                  // missing `:` (non-canonical)
            "brain:wicked-garden",                  // migrated-brain subtree
            "brain:wicked-garden/doc:mem%2Fabc.md", // migrated-brain leaf
        ];
        for s in scopes {
            let scope = Scope::parse(s);
            let path = scope.as_path();
            for p in prefixes {
                assert_eq!(
                    scope.path_in_prefix(p),
                    path_in_prefix(&path, p),
                    "walk vs string diverged: scope={path:?} prefix={p:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_roundtrip() {
        let s = Scope::parse("org:acme/unit:pay/agent:claude");
        assert_eq!(s.0.len(), 3);
        assert_eq!(s.as_path(), "org:acme/unit:pay/agent:claude");
        assert!(Scope::parse("").is_root());
        assert_eq!(Scope::root().as_path(), "");
    }

    #[test]
    fn ancestor_prefix_and_isolation() {
        let org = Scope::parse("org:acme");
        let agent = Scope::parse("org:acme/unit:pay/agent:claude");
        let other = Scope::parse("org:other");
        assert!(org.is_ancestor_of(&agent));
        assert!(agent.is_ancestor_of(&agent));
        assert!(!org.is_ancestor_of(&other)); // ISOLATION
        assert!(Scope::root().is_ancestor_of(&agent));
    }
}
