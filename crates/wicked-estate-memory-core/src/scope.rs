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

/// A scope segment that is not a `kind:id` pair (returned by [`Scope::parse_strict`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeParseError {
    /// The offending segment (empty, colonless, or with an empty kind/id).
    pub segment: String,
    /// The full path it appeared in.
    pub path: String,
}

impl std::fmt::Display for ScopeParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "scope segment '{}' in '{}' is not a 'kind:id' pair — scopes are \
             slash-separated kind:id segments (e.g. \"org:acme/agent:claude\"); \
             an empty scope is root",
            self.segment, self.path
        )
    }
}

impl std::error::Error for ScopeParseError {}

impl Scope {
    /// The root scope (matches everything as an ancestor).
    pub fn root() -> Self {
        Scope(Vec::new())
    }

    /// Parse `"org:acme/unit:pay"`. Empty / malformed segments are skipped; an empty string is root.
    ///
    /// LENIENT — reserved for re-reading values this crate itself persisted (always canonical).
    /// Free-form caller input (the MCP `memory.capture` path) must go through [`Scope::parse_strict`]:
    /// the silent skip here once swallowed a whole import's scope attribution (205 memories landed
    /// at root, un-erasable by their documented `scope_prefix`).
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

    /// FAIL-LOUD parse: every `/`-separated segment must be a `kind:id` pair with non-empty
    /// kind and id. An empty string is root. Where the lenient [`Scope::parse`] silently
    /// discards malformed segments (changing which scope a write lands in), this returns the
    /// offending segment so the caller can reject the request instead.
    pub fn parse_strict(path: &str) -> Result<Self, ScopeParseError> {
        if path.is_empty() {
            return Ok(Scope::root());
        }
        let mut segs = Vec::new();
        for raw in path.split('/') {
            match raw.split_once(':') {
                Some((kind, id)) if !kind.is_empty() && !id.is_empty() => segs.push(Seg {
                    kind: kind.to_string(),
                    id: id.to_string(),
                }),
                _ => {
                    return Err(ScopeParseError {
                        segment: raw.to_string(),
                        path: path.to_string(),
                    });
                }
            }
        }
        Ok(Scope(segs))
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

    /// Allocation-free equivalent of `path_in_prefix(&self.as_path(), prefix)` (the
    /// `wicked_estate_core::scope` subtree predicate erase/coverage/recall share) — does this
    /// scope fall within `prefix`'s subtree? Walks the prefix against the segments' virtual
    /// `kind:id/…` rendering instead of materializing the path, so per-candidate checks (the
    /// recall rerank loop, erase/coverage filters) don't allocate.
    ///
    /// Equivalence holds for parse-normalized scopes ([`Scope::parse`] splits on `/`, so segment
    /// kinds/ids can never contain one) — which is every scope decoded from a store. A prefix
    /// that is not itself a canonical rendering (partial segment, trailing `/`, missing `:`)
    /// matches nothing, exactly as the string predicate behaves against canonical paths.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strict_accepts_canonical_and_root() {
        let s = Scope::parse_strict("org:acme/unit:pay/agent:claude").unwrap();
        assert_eq!(s.as_path(), "org:acme/unit:pay/agent:claude");
        assert!(Scope::parse_strict("").unwrap().is_root());
        // ids may themselves contain later colons (split at the FIRST colon).
        let s = Scope::parse_strict("doc:a:b").unwrap();
        assert_eq!(s.0[0].kind, "doc");
        assert_eq!(s.0[0].id, "a:b");
    }

    #[test]
    fn parse_strict_rejects_what_lenient_parse_silently_drops() {
        // The brain-import regression shape: zero colons → lenient parse = root.
        let bad = "brain/wicked-garden/mem-123.md";
        assert!(
            Scope::parse(bad).is_root(),
            "precondition: lenient parse silently roots this"
        );
        let err = Scope::parse_strict(bad).unwrap_err();
        assert_eq!(err.segment, "brain");
        assert_eq!(err.path, bad);
        assert!(err.to_string().contains("kind:id"));

        // One good segment does not excuse a bad one.
        assert_eq!(
            Scope::parse_strict("org:acme/loose").unwrap_err().segment,
            "loose"
        );
        // Empty kind / empty id / empty segment (trailing slash) all fail loud.
        assert!(Scope::parse_strict(":id").is_err());
        assert!(Scope::parse_strict("kind:").is_err());
        assert_eq!(Scope::parse_strict("org:acme/").unwrap_err().segment, "");
    }

    #[test]
    fn parse_strict_agrees_with_lenient_parse_on_valid_input() {
        for path in ["", "org:acme", "org:acme/unit:pay/agent:claude"] {
            assert_eq!(Scope::parse_strict(path).unwrap(), Scope::parse(path));
        }
    }

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

    /// `Scope::path_in_prefix` (allocation-free walk) must agree with the shared string
    /// predicate on every parse-normalized scope × prefix pair — including non-canonical
    /// prefixes. This pins "a recall with a prefix previews exactly what erase would delete".
    #[test]
    fn path_in_prefix_walk_is_equivalent_to_string_predicate() {
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
                    wicked_estate_core::scope::path_in_prefix(&path, p),
                    "walk vs string diverged: scope={path:?} prefix={p:?}"
                );
            }
        }
    }
}
