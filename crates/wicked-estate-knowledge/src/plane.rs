//! The knowledge plane's dependency model (DES-KNOWLEDGE-DATA-001 §4.3).
//!
//! Every node the plane writes carries **`deps`**: the authoritative set of sources
//! (`source:<id>`) and repos (`repo:<id>`) it was derived from. A source catalog has one dep; an
//! approved data shape has every source and repo it connects. Estate DERIVES deps — no proposer
//! supplies them — and deps never narrow.
//!
//! A reader declares what it may see with a [`Visible`] set in the same spelling. An item is
//! returned only when **every** one of its effective deps is in that set; a node with no deps (all
//! knowledge written before this plane) is visible to every reader, and a reader that declares
//! nothing sees exactly those.

use std::collections::BTreeSet;

use wicked_estate_core::Scope;

use crate::engine::KNode;

/// The scope kind (and dep kind) of a live source's catalog: `source:<sourceId>`.
pub const SOURCE_KIND: &str = "source";
/// The dep kind of a code repository: `repo:<repoId>`.
pub const REPO_KIND: &str = "repo";

/// Longest id a dep may carry (a registry uuid is 36; a repo id is a short slug).
const MAX_DEP_ID: usize = 256;

/// Is `id` a valid source id? The registry's immutable id: 1-128 of `[A-Za-z0-9._-]`. Strict,
/// because it becomes a scope segment and part of every catalog node id.
pub fn valid_source_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Is `id` a valid repo id? Any 1-256 printable ASCII characters except whitespace and `,`.
pub fn valid_repo_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_DEP_ID
        && id.bytes().all(|b| b.is_ascii_graphic() && b != b',')
}

/// Is `dep` a well-formed dependency: `source:<source id>` or `repo:<repo id>`?
pub fn valid_dep(dep: &str) -> bool {
    match dep.split_once(':') {
        Some((SOURCE_KIND, id)) => valid_source_id(id),
        Some((REPO_KIND, id)) => valid_repo_id(id),
        _ => false,
    }
}

/// The deps a scope implies: `source:<id>` for every `source` segment in it. Anything written
/// into a source's scope is gated by that source, whoever wrote it (a segment that is not a valid
/// source id still gates: its literal dep is in no reader's set).
pub fn scope_deps(scope: &str) -> Vec<String> {
    if !scope.contains("source:") {
        return Vec::new(); // the common case: no parse, no allocation
    }
    Scope::parse(scope)
        .0
        .iter()
        .filter(|seg| seg.kind == SOURCE_KIND)
        .map(|seg| format!("{SOURCE_KIND}:{}", seg.id))
        .collect()
}

/// A node's effective deps: its stored deps plus the deps its scope implies, sorted and unique.
pub fn effective_deps(kn: &KNode) -> Vec<String> {
    let mut set: BTreeSet<String> = kn.deps.iter().cloned().collect();
    set.extend(scope_deps(&kn.scope));
    set.into_iter().collect()
}

/// Does the node's text come from a source the person does not control? True for anything in a
/// `source:` scope and for a node written with the flag. Consumers fence it as data.
pub fn is_untrusted(kn: &KNode) -> bool {
    kn.untrusted || !scope_deps(&kn.scope).is_empty()
}

/// The set of sources and repos a reader may see (`source:<id>` / `repo:<id>`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visible(BTreeSet<String>);

impl Visible {
    /// The empty set: only dep-free knowledge is visible.
    pub fn none() -> Self {
        Self::default()
    }

    /// Build from deps, refusing a malformed one (a reader that mistypes a dep must hear it, not
    /// silently see less).
    pub fn from_deps<I, S>(deps: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = BTreeSet::new();
        for d in deps {
            let d = d.as_ref();
            if !valid_dep(d) {
                return Err(format!(
                    "invalid visible entry {d:?}: expected source:<id> or repo:<id>"
                ));
            }
            set.insert(d.to_string());
        }
        Ok(Self(set))
    }

    /// Is `dep` in the set?
    pub fn contains(&self, dep: &str) -> bool {
        self.0.contains(dep)
    }

    /// May the reader see something with these deps? Every one must be in the set.
    pub fn allows<S: AsRef<str>>(&self, deps: &[S]) -> bool {
        deps.iter().all(|d| self.0.contains(d.as_ref()))
    }
}

/// Sort and de-duplicate a dep list (the stored form).
pub fn normalize_deps<I, S>(deps: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    deps.into_iter()
        .map(Into::into)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::KClass;

    #[test]
    fn dep_grammar() {
        assert!(valid_dep("source:3f0c-11ee.a_b"));
        assert!(valid_dep("repo:mikeparcewski/wicked-estate"));
        assert!(!valid_dep("source:a/b"), "a source id is one scope segment");
        assert!(!valid_dep("source:"));
        assert!(!valid_dep("repo:has space"));
        assert!(!valid_dep("table:x"));
        assert!(!valid_dep("source"));
    }

    #[test]
    fn effective_deps_add_the_source_scope_and_untrusted_follows_it() {
        let mut kn = KNode::new(KClass::Section, "t", "source:s1", "x", 1);
        assert_eq!(effective_deps(&kn), vec!["source:s1".to_string()]);
        assert!(is_untrusted(&kn));
        kn.scope = "wiki:arch".into();
        kn.deps = vec!["repo:r1".into(), "source:s2".into()];
        assert_eq!(
            effective_deps(&kn),
            vec!["repo:r1".to_string(), "source:s2".to_string()]
        );
        assert!(!is_untrusted(&kn));
    }

    #[test]
    fn every_dep_must_be_visible() {
        let v = Visible::from_deps(["source:s1", "repo:r1"]).unwrap();
        assert!(v.allows::<&str>(&[]), "dep-free is always visible");
        assert!(v.allows(&["source:s1"]));
        assert!(v.allows(&["source:s1", "repo:r1"]));
        assert!(!v.allows(&["source:s1", "source:s2"]));
        assert!(!Visible::none().allows(&["source:s1"]));
        assert!(Visible::from_deps(["bogus"]).is_err());
    }
}
