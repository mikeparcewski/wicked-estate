//! The knowledge plane's dependency model (DES-KNOWLEDGE-DATA-001 §4.3).
//!
//! Every node the plane writes carries **`deps`**: the authoritative set of sources
//! (`source:<id>`) and repos (`repo:<id>`) it was derived from. A source catalog has one dep; an
//! approved data shape has every source and repo it connects. Estate DERIVES deps — no proposer
//! supplies them — and deps never narrow.
//!
//! A third dep kind, **`project:<id>`** (a crew project, DES-PM-001 §3.4), is carried by the typed
//! links a tracker's catalog owns (`tracked_in project:P`), never by a node. The **`control`** store
//! names crew's projects, repos, PRs and runs as link endpoints; [`control_deps`] derives each id's
//! deps.
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
/// The dep kind of a crew project: `project:<projectId>` (DES-PM-001 §3.4).
pub const PROJECT_KIND: &str = "project";
/// The `control` store's id kind for a pull request: `pr:<repo id>#<n>`.
pub const PR_KIND: &str = "pr";
/// The `control` store's id kind for a run: `run:<run id>`.
pub const RUN_KIND: &str = "run";

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

/// A caller-supplied value as an error message shows it: at most 64 characters, so a diagnostic
/// never carries an oversized input back (R4).
pub fn shown(s: &str) -> String {
    const SHOWN_CHARS: usize = 64;
    if s.chars().count() <= SHOWN_CHARS {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(SHOWN_CHARS).collect();
        out.push('…');
        out
    }
}

/// Is `id` a valid crew project (or run) id? 1-128 of `[A-Za-z0-9._-]` (crew's `proj_…`).
pub fn valid_project_id(id: &str) -> bool {
    valid_source_id(id)
}

/// Is `dep` a well-formed dependency: `source:<source id>`, `repo:<repo id>` or
/// `project:<project id>`?
pub fn valid_dep(dep: &str) -> bool {
    match dep.split_once(':') {
        Some((SOURCE_KIND, id)) => valid_source_id(id),
        Some((REPO_KIND, id)) => valid_repo_id(id),
        Some((PROJECT_KIND, id)) => valid_project_id(id),
        _ => false,
    }
}

/// The deps of a `control` endpoint (DES-PM-001 §3.4), derived by estate from the id:
///
/// | id | deps |
/// |---|---|
/// | `project:<id>` | itself |
/// | `repo:<id>` | itself |
/// | `pr:<repo id>#<n>` | `repo:<repo id>` |
/// | `run:<id>` | `given`: the run's filing from crew, 1-[`MAX_DEPS`] `project:` / `repo:` deps |
///
/// `given` is only for a `run:` (whose filing the id does not say); for any other id it is refused,
/// because estate derives those. A malformed id or dep is an error, never a narrower set.
pub fn control_deps(id: &str, given: Option<&[String]>) -> Result<Vec<String>, String> {
    let sid = shown(id);
    let (kind, rest) = id
        .split_once(':')
        .ok_or_else(|| format!("control id {sid:?}: expected project:, repo:, pr: or run:"))?;
    if kind != RUN_KIND && given.is_some() {
        return Err(format!(
            "control id {sid:?}: deps are given only for a run: end (estate derives a {}: end's deps)",
            shown(kind)
        ));
    }
    match kind {
        PROJECT_KIND if valid_project_id(rest) => Ok(vec![id.to_string()]),
        REPO_KIND if valid_repo_id(rest) => Ok(vec![id.to_string()]),
        PR_KIND => match rest.rsplit_once('#') {
            // The repo part holds no `#`, so `pr:<repo>#<n>` parses one way only.
            Some((repo, n))
                if valid_repo_id(repo)
                    && !repo.contains('#')
                    && !n.is_empty()
                    && n.len() <= 20
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && n.parse::<u64>().is_ok_and(|v| v > 0) =>
            {
                Ok(vec![format!("{REPO_KIND}:{repo}")])
            }
            _ => Err(format!(
                "control id {sid:?}: expected pr:<repo id>#<number>"
            )),
        },
        RUN_KIND if valid_project_id(rest) => {
            let given = given.ok_or_else(|| {
                format!(
                    "control id {sid:?}: a run: end needs deps (its project:<id>, or repo:<id> for an unfiled run)"
                )
            })?;
            if given.is_empty() || given.len() > MAX_DEPS {
                return Err(format!(
                    "control id {sid:?}: a run: end needs 1-{MAX_DEPS} deps"
                ));
            }
            for d in given {
                let ok = matches!(d.split_once(':'), Some((PROJECT_KIND | REPO_KIND, _)))
                    && valid_dep(d);
                if !ok {
                    return Err(format!(
                        "control id {sid:?}: run dep {:?} must be project:<id> or repo:<id>",
                        shown(d)
                    ));
                }
            }
            Ok(normalize_deps(given.iter().cloned()))
        }
        _ => Err(format!(
            "control id {sid:?}: expected project:<id>, repo:<id>, pr:<repo id>#<n> or run:<id>"
        )),
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

/// The set of sources, repos and projects a reader may see (`source:<id>` / `repo:<id>` /
/// `project:<id>`).
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
                    "invalid visible entry {:?}: expected source:<id>, repo:<id> or project:<id>",
                    shown(d)
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

    /// DES-PM-001 §3.4: `project:` is a dep kind a reader may declare.
    #[test]
    fn project_is_a_dep_kind() {
        assert!(valid_dep("project:proj_01HX.a-b"));
        assert!(!valid_dep("project:"));
        assert!(!valid_dep("project:a/b"), "one id segment");
        assert!(!valid_dep("run:r1"), "a run is an endpoint, never a dep");
        let v = Visible::from_deps(["source:trk_1", "project:proj_P"]).unwrap();
        assert!(v.allows(&["project:proj_P", "source:trk_1"]));
        assert!(!v.allows(&["project:proj_Q"]));
    }

    /// The control store's ids and their derived deps; a run's deps come from crew and are checked.
    #[test]
    fn control_ids_derive_their_deps() {
        let run_deps = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            control_deps("project:proj_P", None).unwrap(),
            vec!["project:proj_P"]
        );
        assert_eq!(
            control_deps("repo:billing-api", None).unwrap(),
            vec!["repo:billing-api"]
        );
        assert_eq!(
            control_deps("pr:billing-api#42", None).unwrap(),
            vec!["repo:billing-api"]
        );
        assert_eq!(
            control_deps("run:r1", Some(&run_deps(&["project:proj_P"]))).unwrap(),
            vec!["project:proj_P"]
        );
        assert_eq!(
            control_deps("run:r1", Some(&run_deps(&["repo:r", "repo:r"]))).unwrap(),
            vec!["repo:r"],
            "normalized"
        );
        for bad in [
            "project:",
            "project:a/b",
            "pr:r#0",
            "pr:r#x",
            "pr:r",
            "pr:#4",
            "pr:a#b#4",
            "issue:x",
            "x",
        ] {
            assert!(control_deps(bad, None).is_err(), "must refuse {bad}");
        }
        // A run needs its filing; only project:/repo: deps; nothing else is given deps.
        assert!(control_deps("run:r1", None).is_err());
        assert!(control_deps("run:r1", Some(&[])).is_err());
        assert!(control_deps("run:r1", Some(&run_deps(&["source:s1"]))).is_err());
        assert!(control_deps("run:r1", Some(&run_deps(&["project:"]))).is_err());
        assert!(control_deps("run:r1", Some(&run_deps(&["run:r2"]))).is_err());
        assert!(control_deps("project:proj_P", Some(&run_deps(&["project:proj_P"]))).is_err());
        let many: Vec<String> = (0..=MAX_DEPS).map(|i| format!("repo:r{i}")).collect();
        assert!(control_deps("run:r1", Some(&many)).is_err());
    }

    /// r1 (codex 7): an error never carries an oversized input back (R4).
    #[test]
    fn errors_show_a_bounded_excerpt() {
        let huge = format!("project:{}", "x".repeat(30_000));
        assert!(control_deps(&huge, None).unwrap_err().len() < 300);
        let huge_dep = vec![format!("source:{}", "y".repeat(30_000))];
        assert!(control_deps("run:r1", Some(&huge_dep)).unwrap_err().len() < 300);
        assert!(control_deps(&"z".repeat(30_000), None).unwrap_err().len() < 300);
        let huge_visible = format!("project:{}", "x".repeat(30_000));
        assert!(Visible::from_deps([huge_visible]).unwrap_err().len() < 300);
    }
}
