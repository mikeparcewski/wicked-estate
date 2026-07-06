//! Hierarchical scope with inheritance (the user's "view of the IT estate" primitive).
//!
//! A `Scope` is an ordered path of `kind:id` segments, e.g.
//! `org:acme / unit:payments / project:checkout / sprint:24 / agent:claude`. Kinds are
//! custom/org-defined. Inheritance = prefix match: a recall at a leaf scope also sees its
//! ancestors. In L0 this lives in `wicked-memory` (a `metadata`/column value); at L3 it is
//! promoted into `wicked-estate-core` as a first-class query predicate.

use serde::{Deserialize, Serialize};

/// One `kind:id` segment of a scope path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Seg {
    pub kind: String,
    pub id: String,
}

/// An ordered, hierarchical ownership/partition path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Scope(pub Vec<Seg>);

impl Scope {
    /// The root scope (matches everything as an ancestor).
    pub fn root() -> Self {
        Scope(Vec::new())
    }

    /// Parse `"org:acme/unit:pay"`. Empty / malformed segments are skipped; an empty string is root.
    pub fn parse(path: &str) -> Self {
        let segs = path
            .split('/')
            .filter(|s| !s.is_empty())
            .filter_map(|s| {
                let (kind, id) = s.split_once(':')?;
                if kind.is_empty() || id.is_empty() {
                    return None;
                }
                Some(Seg {
                    kind: kind.to_string(),
                    id: id.to_string(),
                })
            })
            .collect();
        Scope(segs)
    }

    /// Canonical string form (`"org:acme/unit:pay"`; root → `""`).
    pub fn as_path(&self) -> String {
        self.0
            .iter()
            .map(|s| format!("{}:{}", s.kind, s.id))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Is `self` an ancestor of (or equal to) `other`? (prefix match — the inheritance rule.)
    pub fn is_ancestor_of(&self, other: &Scope) -> bool {
        other.0.len() >= self.0.len() && other.0[..self.0.len()] == self.0[..]
    }

    /// All ancestor paths (incl. self), leaf→root — what a scoped recall should also search.
    pub fn ancestors(&self) -> Vec<Scope> {
        (0..=self.0.len())
            .rev()
            .map(|n| Scope(self.0[..n].to_vec()))
            .collect()
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        let s = Scope::parse("org:acme/unit:pay/agent:claude");
        assert_eq!(s.0.len(), 3);
        assert_eq!(s.as_path(), "org:acme/unit:pay/agent:claude");
        assert_eq!(Scope::parse("").as_path(), "");
        assert!(Scope::parse("").is_root());
    }

    #[test]
    fn inheritance_is_prefix_match() {
        let org = Scope::parse("org:acme");
        let team = Scope::parse("org:acme/unit:pay");
        let agent = Scope::parse("org:acme/unit:pay/agent:claude");
        let other = Scope::parse("org:other/unit:pay");

        assert!(org.is_ancestor_of(&agent));
        assert!(team.is_ancestor_of(&agent));
        assert!(agent.is_ancestor_of(&agent)); // reflexive
        assert!(!agent.is_ancestor_of(&team)); // not a descendant
        assert!(!org.is_ancestor_of(&other)); // ISOLATION: acme is not ancestor of other
        assert!(Scope::root().is_ancestor_of(&agent)); // root sees all
    }

    #[test]
    fn ancestors_leaf_to_root() {
        let agent = Scope::parse("org:acme/unit:pay/agent:claude");
        let a = agent.ancestors();
        assert_eq!(a.len(), 4); // agent, team, org, root
        assert_eq!(a[0], agent);
        assert!(a[3].is_root());
    }
}
