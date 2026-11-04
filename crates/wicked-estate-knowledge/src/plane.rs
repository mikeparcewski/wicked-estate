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

use wicked_estate_core::Node;

use crate::engine::meta_keys;

/// The scope kind (and dep kind) of a live source's catalog: `source:<sourceId>`.
pub const SOURCE_KIND: &str = "source";
/// The dep kind of a code repository: `repo:<repoId>`.
pub const REPO_KIND: &str = "repo";

/// Longest id a dep may carry (a registry uuid is 36; a repo id is a short slug).
const MAX_DEP_ID: usize = 128;
/// A node may carry at most this many deps (a data shape connecting more sources and repos than
/// this is refused at approve), so an item's `deps` always fits beside its content (R4).
pub const MAX_DEPS: usize = 32;

/// Is `id` a valid source id? The registry's immutable id: 1-128 of `[A-Za-z0-9._-]`. Strict,
/// because it becomes a scope segment and part of every catalog node id.
pub fn valid_source_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Is `id` a valid repo id? Any 1-128 printable ASCII characters except whitespace and `,`.
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

/// The node's scope as written (the knowledge metadata field `KNode::from_node` hydrates).
fn meta_scope(node: &Node) -> &str {
    node.metadata
        .get(meta_keys::SCOPE)
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

/// A node's STORED deps (the `deps` metadata array). A malformed entry is kept as a dep no reader
/// holds (fail closed), never dropped.
pub fn stored_deps(node: &Node) -> Vec<String> {
    match node.metadata.get(meta_keys::DEPS) {
        None => Vec::new(),
        Some(serde_json::Value::Array(xs)) => xs
            .iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("invalid:{x}"))
            })
            .collect(),
        Some(other) => vec![format!("invalid:{other}")],
    }
}

/// A node's effective deps: its stored deps plus the deps its scope implies, sorted and unique.
pub fn effective_deps(node: &Node) -> Vec<String> {
    let mut set: BTreeSet<String> = stored_deps(node).into_iter().collect();
    set.extend(scope_deps(meta_scope(node)));
    set.into_iter().collect()
}

/// Does the node's text come from a source the person does not control? True for anything in a
/// `source:` scope and for a node written with the flag. Consumers fence it as data.
pub fn is_untrusted(node: &Node) -> bool {
    node.metadata
        .get(meta_keys::UNTRUSTED)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || !scope_deps(meta_scope(node)).is_empty()
}

/// Stamp `deps` (sorted, unique) and the untrusted flag onto a knowledge node's metadata — the
/// plane's write side of [`stored_deps`] / [`is_untrusted`].
pub fn stamp(node: &mut Node, deps: &[String], untrusted: bool) {
    let deps = normalize_deps(deps.iter().cloned());
    if deps.is_empty() {
        node.metadata.remove(meta_keys::DEPS);
    } else {
        node.metadata.insert(meta_keys::DEPS.into(), deps.into());
    }
    if untrusted {
        node.metadata
            .insert(meta_keys::UNTRUSTED.into(), true.into());
    } else {
        node.metadata.remove(meta_keys::UNTRUSTED);
    }
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
    use crate::engine::{KClass, KNode};

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
        let node = KNode::new(KClass::Section, "t", "source:s1", "x", 1).to_node();
        assert_eq!(effective_deps(&node), vec!["source:s1".to_string()]);
        assert!(is_untrusted(&node));
        let mut node = KNode::new(KClass::Section, "t", "wiki:arch", "x", 1).to_node();
        stamp(&mut node, &["source:s2".into(), "repo:r1".into()], false);
        assert_eq!(
            effective_deps(&node),
            vec!["repo:r1".to_string(), "source:s2".to_string()]
        );
        assert!(!is_untrusted(&node));
        node.metadata
            .insert(meta_keys::DEPS.into(), serde_json::json!([42]));
        assert_eq!(stored_deps(&node), vec!["invalid:42".to_string()]);
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
