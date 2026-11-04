//! Document-owned links (DES-PM-001 §3.4): typed edges a source's document owns, written by
//! [`KnowledgeEngine::replace_source_linked`] as part of the source's generation.
//!
//! A tracker (one project in one tenant of a project-management system) is a catalog of its own,
//! `source:<tracker id>`. Its documents carry **links**: `tracked_in` to a crew project or repo in
//! the `control` store, and an `issue_ref` document's `addresses` / `closes` from a run or a PR. A
//! link is written as a typed edge **owned by its document at one generation**: the owner is
//! `<doc node id>@<generation>`, so the rows of two generations are distinct and a re-put of an
//! unchanged link never overwrites the live row.
//!
//! A reader keeps a link only while its owner document is live, visible, and **at that
//! generation**. Every edge's `meta.deps` holds the document's deps plus the far end's
//! (`tracked_in project:P` carries `[project:P, source:<tracker id>]`), and the far end must be
//! visible on its own.
//!
//! The write order (each step's crash leaves only hidden rows):
//!
//! 1. validate every link (no reads);
//! 2. refuse an older generation, or the same one with different content, before any row is
//!    touched; the committed generation again (same content) is a retry that rewrites no row, and
//!    a newer one resolves its links (reads only);
//! 3. delete this source's rows of a generation above the committed one (an interrupted attempt's
//!    leftovers, so they can never surface when that generation number later commits);
//! 4. write the new rows in one xedge transaction, hidden because the owner is still at the old
//!    generation;
//! 5. commit the knowledge generation: the flip;
//! 6. delete this source's rows of every other generation.
//!
//! The knowledge generation and the link rows live in two files, so steps 3-6 are separate
//! transactions: a failed call commits no knowledge generation, but may leave hidden link rows,
//! which the next generation of the source deletes.
//!
//! **One writer per source.** The sequence assumes one writer of a given source at a time (crew's
//! projector is the only one). Two concurrent writers of the same tracker could interleave a
//! pre-clean with the other's staged rows; if a second writer ever appears, serialize the whole
//! sequence per source (an interprocess lock or one writer service).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use wicked_estate_core::{GraphRead, SymbolId};
use wicked_estate_overlay::{Endpoint, TYPED_RELS, TypedXEdge, is_typed_rel};

use crate::engine::KnowledgeEngine;
use crate::plane::{
    MAX_DEPS, RUN_KIND, SOURCE_KIND, control_deps, effective_deps, normalize_deps, shown,
};
use crate::shape::{CONTROL_STORE, KNOWLEDGE_STORE};
use crate::source::{ReplaceReport, SourceGeneration, doc_symbol};

/// At most this many links per document.
pub const MAX_LINKS_PER_DOC: usize = 200;
/// At most this many links per generation.
pub const MAX_GENERATION_LINKS: usize = 20_000;
/// Longest end id (a knowledge node id with two percent-encoded 512-byte keys fits).
const MAX_END_ID_BYTES: usize = 4096;

/// Which way a link points: from its document (`Out`), or into it (`In`, e.g. `run:<id> addresses
/// <issue_ref>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkDir {
    Out,
    In,
}

impl LinkDir {
    /// The wire spelling: `"out"` or `"in"`.
    pub fn as_str(self) -> &'static str {
        match self {
            LinkDir::Out => "out",
            LinkDir::In => "in",
        }
    }

    /// Parse the wire spelling.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "out" => Some(LinkDir::Out),
            "in" => Some(LinkDir::In),
            _ => None,
        }
    }
}

/// A link's far end: `store` is `"control"` (a crew project, repo, PR or run) or `"knowledge"` (a
/// live knowledge node outside the link's own source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEnd {
    pub store: String,
    pub id: String,
}

/// One link a source document owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocLink {
    /// One of the typed relations.
    pub rel: String,
    pub dir: LinkDir,
    pub end: LinkEnd,
    /// Only for a `run:` end: the run's filing (`project:<id>`, or `repo:<id>` for an unfiled run).
    /// Estate derives every other end's deps, so it is refused there.
    pub deps: Option<Vec<String>>,
}

/// What a linked `replace_source` did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LinkedReplaceReport {
    /// The generation's own report (documents, sections, tombstones).
    pub report: ReplaceReport,
    /// The link rows this generation holds.
    pub links: usize,
    /// Rows of other generations, and of interrupted attempts, deleted.
    pub stale_removed: u64,
    /// The cleanup after the flip failed: the generation is committed and its old rows are hidden;
    /// the next generation of this source deletes them.
    pub cleanup_error: Option<String>,
}

impl LinkedReplaceReport {
    /// A report for a generation written without the link store (no links, nothing cleaned).
    pub fn without_links(report: ReplaceReport) -> Self {
        Self {
            report,
            links: 0,
            stale_removed: 0,
            cleanup_error: None,
        }
    }
}

/// The owner of a document's links at one generation: `<doc node id>@<generation>`.
pub fn link_owner(doc: &SymbolId, generation: u64) -> String {
    format!("{}@{generation}", doc.0)
}

/// The owner prefix of every link a source's documents own (an index range).
pub fn source_owner_prefix(source_id: &str) -> String {
    let probe = doc_symbol(source_id, "_").0;
    probe
        .strip_suffix("_:")
        .map(str::to_string)
        .unwrap_or(probe)
}

/// Split a link owner into its document and generation; `None` for any other owner (a data
/// shape's node id, a malformed string).
pub fn parse_link_owner(owner: &str) -> Option<(SymbolId, u64)> {
    let (doc, generation) = owner.rsplit_once('@')?;
    let doc_prefix = source_owner_prefix("x");
    let kind_prefix = doc_prefix.strip_suffix("x/")?; // `kdoc synthetic source:`
    if !doc.starts_with(kind_prefix) || !doc.ends_with(':') {
        return None;
    }
    if generation.is_empty()
        || generation.len() > 20
        || !generation.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let generation = generation.parse::<u64>().ok().filter(|g| *g > 0)?;
    Some((SymbolId(doc.to_string()), generation))
}

fn end_id_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= MAX_END_ID_BYTES && !s.chars().any(char::is_control)
}

/// Validate a generation's links before anything is read or written (pure): each names a document
/// of the generation, uses a typed relation and a well-formed end, gives deps only for a `run:`,
/// and appears once.
pub fn validate_links(
    g: &SourceGeneration,
    links: &BTreeMap<String, Vec<DocLink>>,
) -> Result<(), String> {
    let keys: BTreeSet<&str> = g.docs.iter().map(|d| d.key.as_str()).collect();
    let mut total = 0usize;
    for (key, doc_links) in links {
        if doc_links.is_empty() {
            continue;
        }
        if !keys.contains(key.as_str()) {
            return Err(format!(
                "links for doc {:?}, which this generation does not hold",
                shown(key)
            ));
        }
        if doc_links.len() > MAX_LINKS_PER_DOC {
            return Err(format!(
                "doc {key:?}: {} links is over {MAX_LINKS_PER_DOC}",
                doc_links.len()
            ));
        }
        total += doc_links.len();
        if total > MAX_GENERATION_LINKS {
            return Err(format!(
                "the generation's links are over {MAX_GENERATION_LINKS}"
            ));
        }
        let mut seen = BTreeSet::new();
        for (i, l) in doc_links.iter().enumerate() {
            let at = format!("doc {:?} links[{i}]", shown(key));
            if !is_typed_rel(&l.rel) {
                return Err(format!(
                    "{at}.rel must be EXACTLY one of {}",
                    TYPED_RELS.join("|")
                ));
            }
            match l.end.store.as_str() {
                CONTROL_STORE => {
                    control_deps(&l.end.id, l.deps.as_deref()).map_err(|e| format!("{at}: {e}"))?;
                }
                KNOWLEDGE_STORE => {
                    if l.deps.is_some() {
                        return Err(format!(
                            "{at}: deps are given only for a run: end (estate derives a knowledge end's deps)"
                        ));
                    }
                    if !end_id_ok(&l.end.id) {
                        return Err(format!(
                            "{at}.end.id must be a knowledge node_id of at most {MAX_END_ID_BYTES} bytes"
                        ));
                    }
                }
                other => {
                    return Err(format!(
                        "{at}.end.store {:?}: expected control or knowledge",
                        shown(other)
                    ));
                }
            }
            if !seen.insert((
                l.rel.as_str(),
                l.dir,
                l.end.store.as_str(),
                l.end.id.as_str(),
            )) {
                return Err(format!(
                    "{at}: duplicate link ({}, {}, {}:{})",
                    l.rel,
                    l.dir.as_str(),
                    l.end.store,
                    shown(&l.end.id)
                ));
            }
        }
    }
    Ok(())
}

/// The links' canonical form for the generation's fingerprint, or `None` when it has none (so a
/// link-free generation keeps the fingerprint it had before links existed).
pub(crate) fn canonical_links(links: &BTreeMap<String, Vec<DocLink>>) -> Option<Value> {
    let mut out = serde_json::Map::new();
    for (key, doc_links) in links {
        if doc_links.is_empty() {
            continue;
        }
        let mut rows: Vec<Value> = doc_links
            .iter()
            .map(|l| {
                json!([
                    l.rel,
                    l.dir.as_str(),
                    l.end.store,
                    l.end.id,
                    l.deps.as_ref().map(|d| normalize_deps(d.iter().cloned()))
                ])
            })
            .collect();
        rows.sort_by_key(|r| r.to_string());
        out.insert(key.clone(), Value::Array(rows));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

impl KnowledgeEngine {
    /// Build the rows of a generation's links (reads only): resolve every knowledge end, derive
    /// every edge's deps, and refuse a link that cannot be written.
    pub(crate) fn plan_links(
        &self,
        g: &SourceGeneration,
        links: &BTreeMap<String, Vec<DocLink>>,
    ) -> wicked_estate_core::Result<Vec<TypedXEdge>> {
        let bad = |m: String| wicked_estate_core::Error::Invalid(m);
        let sid = g.source_id.as_str();
        let own_dep = format!("{SOURCE_KIND}:{sid}");
        let mut edges = Vec::new();
        for (key, doc_links) in links {
            let doc = doc_symbol(sid, key);
            let owner = link_owner(&doc, g.generation);
            for (i, l) in doc_links.iter().enumerate() {
                let at = format!("doc {:?} links[{i}]", shown(key));
                let (end, end_deps, run_deps) = if l.end.store == CONTROL_STORE {
                    let deps = control_deps(&l.end.id, l.deps.as_deref())
                        .map_err(|e| bad(format!("{at}: {e}")))?;
                    let run = l.end.id.starts_with(&format!("{RUN_KIND}:"));
                    (
                        Endpoint::new(CONTROL_STORE, l.end.id.clone(), 0),
                        deps.clone(),
                        run.then_some(deps),
                    )
                } else {
                    let Some((node, kn)) = self
                        .live_knowledge(&l.end.id)
                        .map_err(|e| bad(format!("{at}: {e}")))?
                    else {
                        return Err(bad(format!(
                            "{at}: end {:?} is not a live knowledge node",
                            shown(&l.end.id)
                        )));
                    };
                    // A node in this source's own scope is one this generation rewrites (it has no
                    // epoch to stamp yet). A node elsewhere that merely DEPENDS on this source (a
                    // data shape built from it) is a fine end; its deps join the edge's.
                    if kn.scope == own_dep || kn.scope.starts_with(&format!("{own_dep}/")) {
                        return Err(bad(format!(
                            "{at}: end {:?} belongs to this source; a link joins a document to a \
                             crew endpoint or to another source's knowledge",
                            shown(&l.end.id)
                        )));
                    }
                    let deps = effective_deps(&node);
                    let epoch = self
                        .store
                        .symbol_epoch(&SymbolId(l.end.id.clone()))?
                        .unwrap_or(0);
                    (
                        Endpoint::new(KNOWLEDGE_STORE, l.end.id.clone(), epoch),
                        deps,
                        None,
                    )
                };
                let deps = normalize_deps(end_deps.into_iter().chain([own_dep.clone()]));
                if deps.len() > MAX_DEPS {
                    return Err(bad(format!(
                        "{at}: the link connects {} sources, repos and projects; at most {MAX_DEPS}",
                        deps.len()
                    )));
                }
                let mut meta = json!({ "deps": deps, "generation": g.generation });
                if let Some(rd) = run_deps {
                    meta["run_deps"] = json!(rd);
                }
                let doc_end = Endpoint::new(KNOWLEDGE_STORE, doc.0.clone(), 0);
                let (source, target) = match l.dir {
                    LinkDir::Out => (doc_end, end),
                    LinkDir::In => (end, doc_end),
                };
                edges.push(TypedXEdge {
                    source,
                    target,
                    rel: l.rel.clone(),
                    owner: owner.clone(),
                    confidence: 1.0,
                    meta: meta.to_string(),
                });
            }
        }
        Ok(edges)
    }

    /// Delete this source's link owners whose generation `stale` selects; the row count removed.
    pub(crate) fn delete_link_owners(
        &self,
        source_id: &str,
        stale: impl Fn(u64) -> bool,
    ) -> wicked_estate_core::Result<u64> {
        let Some(xedge) = self.xedge.as_ref() else {
            return Ok(0);
        };
        let owners = xedge
            .reader()
            .typed_owners_with_prefix(&source_owner_prefix(source_id), usize::MAX)?;
        let doomed: Vec<String> = owners
            .into_iter()
            .filter(|o| parse_link_owner(o).is_some_and(|(_, generation)| stale(generation)))
            .collect();
        xedge.delete_typed_by_owners(&doomed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_round_trip_and_nothing_else_parses_as_one() {
        let doc = doc_symbol("trk_0123456789abcdef", "tracker");
        let owner = link_owner(&doc, 7);
        assert!(owner.starts_with(&source_owner_prefix("trk_0123456789abcdef")));
        assert_eq!(parse_link_owner(&owner), Some((doc, 7)));
        for bad in [
            "kconcept synthetic proposal:p1:",
            "kconcept synthetic proposal:p1:@3",
            "kdoc synthetic source:t/x:@0",
            "kdoc synthetic source:t/x:@",
            "kdoc synthetic source:t/x:@1a",
            "kdoc synthetic source:t/x@1",
        ] {
            assert_eq!(parse_link_owner(bad), None, "{bad}");
        }
    }
}

/// DES-PM-001 §7, the estate parts of link tests (i), (ii), (iv) and (vi). Trackers `PROJA`,
/// `PROJB` and `PROJX` are the catalogs `trk_A`, `trk_B` and `trk_X`; P and Q are crew projects that
/// share repo r. Each NEGATIVE CONTROL names the mutant that turns it red (`scripts/mutants.py` in
/// the lane).
#[cfg(test)]
mod pm_tests {
    use super::*;
    use crate::Visible;
    use crate::engine::{KClass, KNode};
    use crate::shape::{Neighbors, NeighborsQuery};
    use crate::source::{SourceDoc, SourceSection};
    use std::sync::Arc;
    use wicked_estate_core::GraphWrite;
    use wicked_estate_overlay::XedgeStore;
    use wicked_estate_store::SqliteStore;

    const A: &str = "trk_A";
    const B: &str = "trk_B";
    const X: &str = "trk_X";
    const P: &str = "project:proj_P";
    const Q: &str = "project:proj_Q";
    const R: &str = "repo:r";

    fn engine() -> KnowledgeEngine {
        KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()))
    }

    fn tracker_doc(label: &str) -> SourceDoc {
        SourceDoc {
            key: "tracker".into(),
            title: Some(format!("{label} tracker")),
            entry: Some(json!({"kind": "tracker", "system": "jira"})),
            sections: vec![SourceSection {
                key: "workflow".into(),
                text: format!(
                    "{label} issue types Bug Story; workflow states To Do todo, In Progress doing, Done done"
                ),
            }],
        }
    }

    fn generation(sid: &str, n: u64) -> SourceGeneration {
        let label = format!("PROJ{}", sid.trim_start_matches("trk_"));
        SourceGeneration {
            source_id: sid.into(),
            generation: n,
            label: Some(label.clone()),
            catalog: Some(json!({"system": "jira", "tenant": "site-1"})),
            docs: vec![tracker_doc(&label)],
        }
    }

    fn control(rel: &str, dir: LinkDir, id: &str, deps: Option<&[&str]>) -> DocLink {
        DocLink {
            rel: rel.into(),
            dir,
            end: LinkEnd {
                store: CONTROL_STORE.into(),
                id: id.into(),
            },
            deps: deps.map(|d| d.iter().map(|x| x.to_string()).collect()),
        }
    }

    fn tracked_in(targets: &[&str]) -> BTreeMap<String, Vec<DocLink>> {
        let links = targets
            .iter()
            .map(|t| control("tracked_in", LinkDir::Out, t, None))
            .collect();
        BTreeMap::from([("tracker".to_string(), links)])
    }

    fn link(e: &mut KnowledgeEngine, sid: &str, n: u64, targets: &[&str]) -> LinkedReplaceReport {
        e.replace_source_linked(&generation(sid, n), &tracked_in(targets), n as i64)
            .unwrap()
    }

    fn doc(sid: &str) -> String {
        doc_symbol(sid, "tracker").0
    }

    fn dep(sid: &str) -> String {
        format!("source:{sid}")
    }

    fn visible(deps: &[&str]) -> Visible {
        Visible::from_deps(deps).unwrap()
    }

    fn walk(
        e: &KnowledgeEngine,
        store: &str,
        id: &str,
        v: &Visible,
        depth: usize,
    ) -> Option<Neighbors> {
        e.neighbors(
            &NeighborsQuery {
                store: store.into(),
                id: id.into(),
                rels: vec![],
                depth,
                visible: v.clone(),
            },
            &SqliteStore::in_memory().unwrap(),
        )
        .unwrap()
    }

    /// The control ends a tracker doc's `tracked_in` edges reach, as a reader sees them.
    fn tracked(e: &KnowledgeEngine, sid: &str, v: &Visible) -> BTreeSet<String> {
        walk(e, KNOWLEDGE_STORE, &doc(sid), v, 1)
            .map(|n| {
                n.edges
                    .iter()
                    .filter(|x| x.rel == "tracked_in" && x.to.0 == CONTROL_STORE)
                    .map(|x| x.to.1.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    fn recalled_sources(e: &mut KnowledgeEngine, v: &Visible) -> BTreeSet<String> {
        e.recall_visible("issue types workflow states", 8000, None, v, 9)
            .unwrap()
            .into_iter()
            .filter_map(|h| {
                h.source
                    .strip_prefix("source:")
                    .and_then(|s| s.split('/').next())
                    .map(str::to_string)
            })
            .collect()
    }

    fn owners(e: &KnowledgeEngine, sid: &str) -> Vec<String> {
        e.xedge
            .as_ref()
            .unwrap()
            .reader()
            .typed_owners_with_prefix(&source_owner_prefix(sid), 100)
            .unwrap()
    }

    /// (i) A project-linked tracker is visible in its project: recall, the catalog, and
    /// `tracked_in project:P` both ways. NEGATIVE CONTROL: `project:` dropped from the dep grammar
    /// (the reader cannot declare its project), or the far end's deps left off the edge.
    #[test]
    fn i_a_project_linked_tracker_is_visible_in_its_project() {
        let mut e = engine();
        let r = link(&mut e, A, 1, &[P]);
        assert_eq!(
            (r.links, r.stale_removed, r.cleanup_error.clone()),
            (1, 0, None)
        );
        let v = visible(&[&dep(A), P]);
        assert_eq!(recalled_sources(&mut e, &v), set(&[A]));
        assert!(e.source_catalog(A, 0, None, &v).unwrap().is_some());

        let n = walk(&e, KNOWLEDGE_STORE, &doc(A), &v, 1).unwrap();
        let edge = n.edges.iter().find(|x| x.rel == "tracked_in").unwrap();
        assert_eq!(edge.from, (KNOWLEDGE_STORE.to_string(), doc(A)));
        assert_eq!(edge.to, (CONTROL_STORE.to_string(), P.to_string()));
        assert_eq!(
            edge.meta["deps"],
            json!([P, dep(A)]),
            "the doc's deps and the far end's"
        );
        assert_eq!(edge.meta["generation"], 1);
        assert_eq!(edge.owner, link_owner(&doc_symbol(A, "tracker"), 1));
        let end = n.nodes.iter().find(|x| x.store == CONTROL_STORE).unwrap();
        assert_eq!(
            (
                end.id.as_str(),
                end.class.as_deref(),
                end.deps.clone(),
                end.hydrated
            ),
            (P, Some("project"), vec![P.to_string()], false)
        );
        assert!(end.scope.is_empty() && end.source.is_empty() && !end.untrusted);

        let back = walk(&e, CONTROL_STORE, P, &v, 1).unwrap();
        assert!(
            back.nodes.iter().any(|x| x.id == doc(A)),
            "P shows its tracker"
        );
    }

    /// (ii) The same tracker is invisible in another project. Q shares r with P; `PROJX` is linked
    /// to r and to P; Q is linked to `PROJB`. Q's unit (its run, and its run that targets r) holds
    /// `[source:trk_B, source:trk_X, repo:r, project:Q]`.
    #[test]
    fn ii_the_same_tracker_is_invisible_in_another_project() {
        let mut e = engine();
        link(&mut e, A, 1, &[P]);
        link(&mut e, B, 1, &[Q]);
        link(&mut e, X, 1, &[R, P]);
        let vq = visible(&[&dep(B), &dep(X), R, Q]);
        assert_eq!(
            recalled_sources(&mut e, &vq),
            set(&[B, X]),
            "no PROJA document"
        );
        assert!(e.source_catalog(A, 0, None, &vq).unwrap().is_none());
        assert_eq!(
            tracked(&e, X, &vq),
            set(&[R]),
            "tracked_in repo:r, never project:P"
        );
        let from_r = walk(&e, CONTROL_STORE, R, &vq, 3).unwrap();
        let ids: BTreeSet<&str> = from_r.nodes.iter().map(|x| x.id.as_str()).collect();
        assert!(ids.contains(doc(X).as_str()));
        assert!(
            !ids.contains(P) && !ids.contains(doc(A).as_str()),
            "{ids:?}"
        );
        assert!(
            walk(&e, CONTROL_STORE, P, &vq, 1).is_none(),
            "P's start is hidden from Q"
        );
        // P's own unit sees both of P's trackers.
        let vp = visible(&[&dep(A), &dep(X), R, P]);
        assert_eq!(tracked(&e, X, &vp), set(&[R, P]));
    }

    /// (ii) NEGATIVE CONTROL: a control far end is admitted only when ITS deps are visible, whatever
    /// the row's `meta.deps` say (a row that under-states them stays hidden). Mutant: the far end
    /// admitted without its own check.
    #[test]
    fn ii_a_control_end_whose_deps_are_hidden_is_left_out() {
        let mut e = engine();
        link(&mut e, A, 1, &[]);
        e.xedge
            .as_ref()
            .unwrap()
            .put_typed(&TypedXEdge {
                source: Endpoint::new(KNOWLEDGE_STORE, doc(A), 0),
                target: Endpoint::new(CONTROL_STORE, P, 0),
                rel: "tracked_in".into(),
                owner: link_owner(&doc_symbol(A, "tracker"), 1),
                confidence: 1.0,
                meta: json!({"deps": [dep(A)], "generation": 1}).to_string(),
            })
            .unwrap();
        assert!(tracked(&e, A, &visible(&[&dep(A)])).is_empty());
        assert_eq!(tracked(&e, A, &visible(&[&dep(A), P])), set(&[P]));
    }

    /// (ii) NEGATIVE CONTROL (estate unit test): an edge between two visible knowledge nodes whose
    /// `meta.deps` holds a hidden `project:` is left out. Mutant: edge deps ignored.
    #[test]
    fn ii_an_edge_whose_deps_hold_a_hidden_project_is_left_out() {
        let mut e = engine();
        let mut concept = |t: &str| {
            e.write(&KNode::new(KClass::Concept, t, "", "t.md", 1))
                .unwrap()
                .0
        };
        let (a, b, owner) = (
            concept("alpha concept"),
            concept("beta concept"),
            concept("the shape"),
        );
        e.xedge
            .as_ref()
            .unwrap()
            .put_typed(&TypedXEdge {
                source: Endpoint::new(KNOWLEDGE_STORE, a.clone(), 0),
                target: Endpoint::new(KNOWLEDGE_STORE, b.clone(), 0),
                rel: "derived_from".into(),
                owner,
                confidence: 1.0,
                meta: json!({"deps": [P]}).to_string(),
            })
            .unwrap();
        let hidden = walk(&e, KNOWLEDGE_STORE, &a, &visible(&[Q]), 1).unwrap();
        assert!(
            hidden.edges.is_empty() && hidden.nodes.len() == 1,
            "{hidden:?}"
        );
        let shown = walk(&e, KNOWLEDGE_STORE, &a, &visible(&[P]), 1).unwrap();
        assert_eq!(shown.edges.len(), 1);
    }

    /// (iv) Removing a link removes it, and no interrupted step shows a stale one. Generation 2
    /// drops P and adds Q. NEGATIVE CONTROLS: a reader that ignores the generation stamp (red
    /// before and after the flip); no cleanup after the flip (red at the retry).
    #[test]
    fn iv_removing_a_link_removes_it_and_no_interrupted_step_shows_a_stale_one() {
        let mut e = engine();
        link(&mut e, A, 1, &[P, R]);
        let v = visible(&[&dep(A), P, Q, R]);
        assert_eq!(tracked(&e, A, &v), set(&[P, R]));

        // Interrupted after the new rows are written, before the flip: still generation 1.
        e.fail_links_before_flip = true;
        assert!(
            e.replace_source_linked(&generation(A, 2), &tracked_in(&[R, Q]), 2)
                .is_err()
        );
        e.fail_links_before_flip = false;
        assert_eq!(owners(&e, A).len(), 2, "both generations' rows are on disk");
        assert_eq!(
            tracked(&e, A, &v),
            set(&[P, R]),
            "generation 2's rows are hidden"
        );
        assert_eq!(walk(&e, CONTROL_STORE, Q, &v, 1).unwrap().nodes.len(), 1);

        // Interrupted after the flip, before the cleanup: generation 2, and P's row is hidden.
        e.fail_links_after_flip = true;
        assert!(
            e.replace_source_linked(&generation(A, 2), &tracked_in(&[R, Q]), 3)
                .is_err()
        );
        e.fail_links_after_flip = false;
        assert_eq!(
            owners(&e, A).len(),
            2,
            "generation 1's rows are still on disk"
        );
        assert_eq!(tracked(&e, A, &v), set(&[R, Q]), "no stale link to P");
        assert_eq!(
            walk(&e, CONTROL_STORE, P, &v, 3).unwrap().nodes.len(),
            1,
            "nothing reaches P"
        );

        // The retry of generation 2 cleans up: only its own rows are left.
        let r = e
            .replace_source_linked(&generation(A, 2), &tracked_in(&[R, Q]), 4)
            .unwrap();
        assert_eq!((r.links, r.cleanup_error.clone()), (2, None));
        assert!(r.stale_removed >= 2, "{r:?}");
        assert_eq!(
            owners(&e, A),
            vec![link_owner(&doc_symbol(A, "tracker"), 2)]
        );

        // The last link goes: an empty generation tombstones the doc and every row it owned.
        let empty = SourceGeneration {
            docs: vec![],
            ..generation(A, 3)
        };
        let r = e
            .replace_source_linked(&empty, &BTreeMap::new(), 5)
            .unwrap();
        assert_eq!(r.report.tombstoned_doc_count, 1);
        assert_eq!(
            r.report.orphaned_edge_count, 0,
            "its own links are removed, not kept"
        );
        assert_eq!(e.xedge.as_ref().unwrap().typed_len().unwrap(), 0);
        assert!(walk(&e, KNOWLEDGE_STORE, &doc(A), &v, 1).is_none());
        assert_eq!(walk(&e, CONTROL_STORE, R, &v, 3).unwrap().nodes.len(), 1);
        assert!(recalled_sources(&mut e, &v).is_empty());
        // Re-linking restores it (a new generation, a new doc).
        link(&mut e, A, 4, &[P]);
        assert_eq!(tracked(&e, A, &v), set(&[P]));
    }

    /// (iv) NEGATIVE CONTROL: an interrupted attempt's rows never surface when its generation number
    /// later commits with other links (crew's registry changed in between). Mutant: no clean-up of
    /// uncommitted rows before the new ones are written.
    #[test]
    fn iv_an_interrupted_attempt_never_surfaces_when_its_generation_commits() {
        let mut e = engine();
        link(&mut e, A, 1, &[P]);
        e.fail_links_before_flip = true;
        assert!(
            e.replace_source_linked(&generation(A, 2), &tracked_in(&[Q]), 2)
                .is_err()
        );
        e.fail_links_before_flip = false;
        link(&mut e, A, 2, &[R]);
        assert_eq!(tracked(&e, A, &visible(&[&dep(A), P, Q, R])), set(&[R]));
    }

    /// (iv) The same generation number with other links is refused before any row is touched, and
    /// a generation without links drops the source's links.
    #[test]
    fn iv_a_changed_link_set_needs_a_new_generation() {
        let mut e = engine();
        link(&mut e, A, 1, &[P]);
        let err = e
            .replace_source_linked(&generation(A, 1), &tracked_in(&[Q]), 2)
            .unwrap_err()
            .to_string();
        assert!(err.contains("different content"), "{err}");
        assert_eq!(
            owners(&e, A),
            vec![link_owner(&doc_symbol(A, "tracker"), 1)]
        );
        let v = visible(&[&dep(A), P, Q]);
        assert_eq!(tracked(&e, A, &v), set(&[P]));
        // The same generation and links again: an idempotent retry.
        assert_eq!(link(&mut e, A, 1, &[P]).links, 1);
        // A plain generation (no links) drops them; an older generation is then refused.
        e.replace_source(&generation(A, 2), 4).unwrap();
        assert!(
            e.replace_source_linked(&generation(A, 1), &tracked_in(&[P]), 5)
                .is_err()
        );
        assert_eq!(
            e.xedge.as_ref().unwrap().typed_len().unwrap(),
            0,
            "no links in generation 2"
        );
        assert!(tracked(&e, A, &v).is_empty());
    }

    /// (vi) No project, every project. A run in no project whose own repo r is linked to `PROJA`
    /// holds `[source:trk_A, repo:r]`; `PROJB` is linked to P only. NEGATIVE CONTROL: a control
    /// start admitted without its deps check (P answers instead of looking unknown).
    #[test]
    fn vi_a_run_in_no_project_sees_only_its_own_repos_trackers() {
        let mut e = engine();
        link(&mut e, A, 1, &[R, P]);
        link(&mut e, B, 1, &[P]);
        let v0 = visible(&[&dep(A), R]);
        assert_eq!(recalled_sources(&mut e, &v0), set(&[A]));
        assert!(e.source_catalog(B, 0, None, &v0).unwrap().is_none());
        assert_eq!(tracked(&e, A, &v0), set(&[R]));
        let from_r = walk(&e, CONTROL_STORE, R, &v0, 3).unwrap();
        let ids: BTreeSet<&str> = from_r.nodes.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, BTreeSet::from([R, doc(A).as_str()]));
        assert!(
            walk(&e, CONTROL_STORE, P, &v0, 1).is_none(),
            "a hidden start looks unknown"
        );
        // A run in no project with no linked repo holds no tracker: it sees none.
        let bare = visible(&[R]);
        assert!(recalled_sources(&mut e, &bare).is_empty());
        assert_eq!(walk(&e, CONTROL_STORE, R, &bare, 3).unwrap().nodes.len(), 1);
    }

    /// The trail's links: an issue_ref owns `run:<id> addresses` (with the run's filing) and
    /// `pr:<repo>#<n> closes`, both `dir: in`. A run's start is visible only with its filing.
    #[test]
    fn run_and_pr_links_carry_their_filing() {
        let mut e = engine();
        let mut g = generation(A, 1);
        g.docs.push(SourceDoc {
            key: "PROJA-1".into(),
            title: Some("PROJA-1".into()),
            entry: Some(json!({"kind": "issue_ref", "key": "PROJA-1"})),
            sections: vec![SourceSection {
                key: "ref".into(),
                text: "issue_ref PROJA-1".into(),
            }],
        });
        let mut links = tracked_in(&[P]);
        links.insert(
            "PROJA-1".into(),
            vec![
                control("addresses", LinkDir::In, "run:run_1", Some(&[P])),
                control("closes", LinkDir::In, "pr:r#42", None),
            ],
        );
        assert_eq!(e.replace_source_linked(&g, &links, 1).unwrap().links, 3);
        let v = visible(&[&dep(A), P, R]);
        let issue = doc_symbol(A, "PROJA-1").0;
        let n = walk(&e, KNOWLEDGE_STORE, &issue, &v, 1).unwrap();
        let run = n.nodes.iter().find(|x| x.id == "run:run_1").unwrap();
        assert_eq!(
            (run.class.as_deref(), run.deps.clone()),
            (Some("run"), vec![P.to_string()])
        );
        let pr = n.nodes.iter().find(|x| x.id == "pr:r#42").unwrap();
        assert_eq!(pr.deps, vec![R.to_string()]);
        let addr = n.edges.iter().find(|x| x.rel == "addresses").unwrap();
        assert_eq!(addr.from.1, "run:run_1");
        assert_eq!(addr.meta["run_deps"], json!([P]));
        assert_eq!(addr.meta["deps"], json!([P, dep(A)]));
        // The run's start: visible with its filing, unknown without it or without a live link.
        assert!(walk(&e, CONTROL_STORE, "run:run_1", &v, 1).is_some());
        assert!(walk(&e, CONTROL_STORE, "run:run_1", &visible(&[&dep(A), R]), 1).is_none());
        assert!(walk(&e, CONTROL_STORE, "run:nope", &v, 1).is_none());
        // Without its filing a run link is refused, and nothing is written.
        let mut bad = links.clone();
        bad.insert(
            "PROJA-1".into(),
            vec![control("addresses", LinkDir::In, "run:run_2", None)],
        );
        let g2 = SourceGeneration {
            generation: 2,
            ..g.clone()
        };
        let err = e
            .replace_source_linked(&g2, &bad, 2)
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs deps"), "{err}");
        assert_eq!(owners(&e, A).len(), 2, "generation 1's two docs, untouched");
    }

    /// A knowledge end must be another source's live node; its deps join the edge's.
    #[test]
    fn a_knowledge_end_is_another_sources_live_node() {
        let mut e = engine();
        link(&mut e, B, 1, &[]);
        let mut links = tracked_in(&[]);
        links.insert(
            "tracker".into(),
            vec![DocLink {
                rel: "derived_from".into(),
                dir: LinkDir::Out,
                end: LinkEnd {
                    store: KNOWLEDGE_STORE.into(),
                    id: doc(B),
                },
                deps: None,
            }],
        );
        e.replace_source_linked(&generation(A, 1), &links, 1)
            .unwrap();
        let both = walk(
            &e,
            KNOWLEDGE_STORE,
            &doc(A),
            &visible(&[&dep(A), &dep(B)]),
            1,
        )
        .unwrap();
        let edge = both.edges.iter().find(|x| x.rel == "derived_from").unwrap();
        assert_eq!(edge.meta["deps"], json!([dep(A), dep(B)]));
        let only_a = walk(&e, KNOWLEDGE_STORE, &doc(A), &visible(&[&dep(A)]), 1).unwrap();
        assert!(only_a.edges.is_empty());
        // Its own source's doc, or a node that is not live, is refused.
        for end in [doc(A), "kdoc synthetic source:trk_Z/none:".to_string()] {
            let mut l = links.clone();
            l.get_mut("tracker").unwrap()[0].end.id = end;
            assert!(e.replace_source_linked(&generation(A, 2), &l, 2).is_err());
        }
    }

    #[test]
    fn links_are_validated_before_anything_is_read() {
        let g = generation(A, 1);
        let one = |l: DocLink| BTreeMap::from([("tracker".to_string(), vec![l])]);
        let ok = control("tracked_in", LinkDir::Out, P, None);
        assert!(validate_links(&g, &one(ok.clone())).is_ok());
        let mut about = ok.clone();
        about.rel = "about".into();
        let mut other_store = ok.clone();
        other_store.end.store = "estate".into();
        let deps_on_project = control("tracked_in", LinkDir::Out, P, Some(&[P]));
        let malformed = control("tracked_in", LinkDir::Out, "project:a/b", None);
        for bad in [about, other_store, deps_on_project, malformed] {
            assert!(validate_links(&g, &one(bad.clone())).is_err(), "{bad:?}");
        }
        let dup = BTreeMap::from([("tracker".to_string(), vec![ok.clone(), ok.clone()])]);
        assert!(validate_links(&g, &dup).unwrap_err().contains("duplicate"));
        let stray = BTreeMap::from([("nope".to_string(), vec![ok.clone()])]);
        assert!(validate_links(&g, &stray).is_err());
        let many = BTreeMap::from([("tracker".to_string(), vec![ok; MAX_LINKS_PER_DOC + 1])]);
        assert!(validate_links(&g, &many).is_err());
    }

    fn to_doc_of(sid: &str) -> BTreeMap<String, Vec<DocLink>> {
        BTreeMap::from([(
            "tracker".to_string(),
            vec![DocLink {
                rel: "derived_from".into(),
                dir: LinkDir::Out,
                end: LinkEnd {
                    store: KNOWLEDGE_STORE.into(),
                    id: doc(sid),
                },
                deps: None,
            }],
        )])
    }

    /// r1 (codex 1): the committed generation again is a retry that rewrites no link row, so a
    /// knowledge end deleted and re-added since stays unlinked (DEC-X6); a NEW generation rebinds.
    /// NEGATIVE CONTROL: a retry that re-resolves and rewrites its rows.
    #[test]
    fn r1_a_retry_never_rebinds_a_link_to_a_re_added_end() {
        let mut e = engine();
        link(&mut e, B, 1, &[]);
        e.replace_source_linked(&generation(A, 1), &to_doc_of(B), 1)
            .unwrap();
        let v = visible(&[&dep(A), &dep(B)]);
        let linked = |e: &KnowledgeEngine| {
            walk(e, KNOWLEDGE_STORE, &doc(A), &v, 1)
                .unwrap()
                .edges
                .len()
        };
        assert_eq!(linked(&e), 1);
        let empty_b = SourceGeneration {
            docs: vec![],
            ..generation(B, 2)
        };
        e.replace_source(&empty_b, 2).unwrap();
        e.replace_source(&generation(B, 3), 3).unwrap();
        assert_eq!(linked(&e), 0, "the re-added end does not inherit the link");
        let r = e
            .replace_source_linked(&generation(A, 1), &to_doc_of(B), 4)
            .unwrap();
        assert_eq!(r.links, 1);
        assert_eq!(linked(&e), 0, "a retry of generation 1 does not rebind it");
        e.replace_source_linked(&generation(A, 2), &to_doc_of(B), 5)
            .unwrap();
        assert_eq!(
            linked(&e),
            1,
            "generation 2 re-asserts it against the live end"
        );
    }

    /// r1 (codex 2): a node outside this source's scope that merely DEPENDS on it (a data shape
    /// built from the tracker) is a fine knowledge end. NEGATIVE CONTROL: ownership judged by deps.
    #[test]
    fn r1_a_node_that_depends_on_this_source_is_a_fine_end() {
        let mut e = engine();
        link(&mut e, A, 1, &[]);
        let mut node =
            KNode::new(KClass::Concept, "a shape over PROJA", "wiki:pm", "t.md", 1).to_node();
        crate::plane::stamp(&mut node, &[dep(A)], false);
        let id = node.symbol.0.clone();
        e.store.upsert_nodes(std::slice::from_ref(&node)).unwrap();
        let mut links = to_doc_of(A);
        links.get_mut("tracker").unwrap()[0].end.id = id.clone();
        e.replace_source_linked(&generation(A, 2), &links, 2)
            .unwrap();
        let n = walk(&e, KNOWLEDGE_STORE, &doc(A), &visible(&[&dep(A)]), 1).unwrap();
        assert!(n.nodes.iter().any(|x| x.id == id), "{n:?}");
    }

    /// r1 (codex 5): a run's deps are the same whether it is the start or a far end: the union of
    /// its live filings. A reader holding only the repo never reaches a run one of whose links is
    /// filed in a project it cannot see. NEGATIVE CONTROL: the first live filing taken as the run's.
    #[test]
    fn r1_a_run_is_judged_by_all_its_filings_as_a_start_and_as_a_far_end() {
        let mut e = engine();
        let mut g = generation(A, 1);
        let issue = |key: &str| SourceDoc {
            key: key.into(),
            title: None,
            entry: None,
            sections: vec![SourceSection {
                key: "ref".into(),
                text: format!("issue_ref {key}"),
            }],
        };
        g.docs.push(issue("PROJA-1"));
        g.docs.push(issue("PROJA-2"));
        let links = BTreeMap::from([
            // Sorts first: the run's repo filing (it was unfiled when PROJA-1 was projected).
            (
                "PROJA-1".to_string(),
                vec![control("addresses", LinkDir::In, "run:run_1", Some(&[R]))],
            ),
            (
                "PROJA-2".to_string(),
                vec![control("addresses", LinkDir::In, "run:run_1", Some(&[P]))],
            ),
        ]);
        e.replace_source_linked(&g, &links, 1).unwrap();
        let repo_only = visible(&[&dep(A), R]);
        let from_issue = walk(
            &e,
            KNOWLEDGE_STORE,
            &doc_symbol(A, "PROJA-1").0,
            &repo_only,
            3,
        )
        .unwrap();
        assert!(
            from_issue.nodes.iter().all(|x| x.id != "run:run_1"),
            "{from_issue:?}"
        );
        assert!(walk(&e, CONTROL_STORE, "run:run_1", &repo_only, 1).is_none());
        let both = visible(&[&dep(A), R, P]);
        let from_issue = walk(&e, KNOWLEDGE_STORE, &doc_symbol(A, "PROJA-1").0, &both, 1).unwrap();
        let run = from_issue
            .nodes
            .iter()
            .find(|x| x.id == "run:run_1")
            .unwrap();
        assert_eq!(run.deps, vec![P.to_string(), R.to_string()]);
    }

    /// r1 (codex 6): a run whose adjacency is over the read bound has no complete filing, so it is
    /// hidden. NEGATIVE CONTROL: the bound not checked (a partial union authorises it).
    #[test]
    fn r1_a_run_over_the_adjacency_bound_is_hidden() {
        let mut e = engine();
        let mut g = generation(A, 1);
        g.docs.push(SourceDoc {
            key: "PROJA-1".into(),
            title: None,
            entry: None,
            sections: vec![SourceSection {
                key: "ref".into(),
                text: "issue_ref PROJA-1".into(),
            }],
        });
        let links = BTreeMap::from([(
            "PROJA-1".to_string(),
            vec![control("addresses", LinkDir::In, "run:run_big", Some(&[P]))],
        )]);
        e.replace_source_linked(&g, &links, 1).unwrap();
        let v = visible(&[&dep(A), P]);
        assert!(walk(&e, CONTROL_STORE, "run:run_big", &v, 1).is_some());
        let filler: Vec<TypedXEdge> = (0..crate::shape::ADJACENCY_LIMIT)
            .map(|i| TypedXEdge {
                source: Endpoint::new(CONTROL_STORE, "run:run_big", 0),
                target: Endpoint::new(KNOWLEDGE_STORE, format!("n{i:05}"), 0),
                rel: "addresses".into(),
                owner: "dead-owner".into(),
                confidence: 1.0,
                meta: "{}".into(),
            })
            .collect();
        e.xedge.as_ref().unwrap().put_typed_batch(&filler).unwrap();
        assert!(walk(&e, CONTROL_STORE, "run:run_big", &v, 1).is_none());
    }

    /// r1 (pi): a cleanup that fails after the flip is reported, never returned as a failure: the
    /// generation is committed and its links are live; the next generation deletes the old rows.
    #[test]
    fn r1_a_failed_cleanup_is_reported_and_the_next_generation_finishes_it() {
        let mut e = engine();
        link(&mut e, A, 1, &[P]);
        e.fail_links_cleanup = true;
        let r = e
            .replace_source_linked(&generation(A, 2), &tracked_in(&[Q]), 2)
            .unwrap();
        e.fail_links_cleanup = false;
        assert!(r.cleanup_error.is_some(), "{r:?}");
        let v = visible(&[&dep(A), P, Q]);
        assert_eq!(tracked(&e, A, &v), set(&[Q]));
        assert_eq!(owners(&e, A).len(), 2);
        link(&mut e, A, 3, &[Q]);
        assert_eq!(
            owners(&e, A),
            vec![link_owner(&doc_symbol(A, "tracker"), 3)]
        );
    }

    /// r1 (pi): a `pr:` start is visible only with its repo.
    #[test]
    fn r1_a_pr_start_needs_its_repo() {
        let mut e = engine();
        let mut g = generation(A, 1);
        g.docs.push(SourceDoc {
            key: "PROJA-1".into(),
            title: None,
            entry: None,
            sections: vec![SourceSection {
                key: "ref".into(),
                text: "issue_ref PROJA-1".into(),
            }],
        });
        let links = BTreeMap::from([(
            "PROJA-1".to_string(),
            vec![control("closes", LinkDir::In, "pr:r#42", None)],
        )]);
        e.replace_source_linked(&g, &links, 1).unwrap();
        let n = walk(&e, CONTROL_STORE, "pr:r#42", &visible(&[&dep(A), R]), 1).unwrap();
        assert_eq!(n.nodes[0].deps, vec![R.to_string()]);
        assert!(n.nodes.iter().any(|x| x.id == doc_symbol(A, "PROJA-1").0));
        assert!(walk(&e, CONTROL_STORE, "pr:r#42", &visible(&[&dep(A)]), 1).is_none());
    }

    fn issues(n: usize) -> SourceGeneration {
        let mut g = generation(A, 1);
        for i in 0..n {
            g.docs.push(SourceDoc {
                key: format!("PROJA-{i}"),
                title: None,
                entry: None,
                sections: vec![SourceSection {
                    key: "ref".into(),
                    text: format!("issue_ref PROJA-{i}"),
                }],
            });
        }
        g
    }

    /// r2 (codex 3): a run filed under more projects and repos than an item may carry is hidden
    /// (its deps are never cut), so its record stays inside R4. NEGATIVE CONTROL: no cap.
    #[test]
    fn r2_a_run_filed_too_widely_is_hidden() {
        let repos: Vec<String> = (0..=MAX_DEPS).map(|i| format!("repo:r{i:02}")).collect();
        let run_links = |n: usize| {
            let mut l = BTreeMap::new();
            for (i, repo) in repos.iter().take(n).enumerate() {
                l.insert(
                    format!("PROJA-{i}"),
                    vec![control(
                        "addresses",
                        LinkDir::In,
                        "run:x",
                        Some(&[repo.as_str()]),
                    )],
                );
            }
            l
        };
        let mut all: Vec<&str> = repos.iter().map(String::as_str).collect();
        let a = dep(A);
        all.push(&a);
        let v = visible(&all);
        let mut e = engine();
        e.replace_source_linked(&issues(MAX_DEPS), &run_links(MAX_DEPS), 1)
            .unwrap();
        assert!(
            walk(&e, CONTROL_STORE, "run:x", &v, 1).is_some(),
            "32 filings fit"
        );
        let mut e = engine();
        e.replace_source_linked(&issues(MAX_DEPS + 1), &run_links(MAX_DEPS + 1), 1)
            .unwrap();
        assert!(
            walk(&e, CONTROL_STORE, "run:x", &v, 1).is_none(),
            "33 do not"
        );
    }

    /// r2 (codex 4): resolving runs' filings shares one row budget per walk, hidden runs included;
    /// a run past it is hidden and the answer says `truncated`. NEGATIVE CONTROL: no budget.
    #[test]
    fn r2_run_resolution_has_one_budget_per_walk() {
        let mut e = engine();
        let runs = 5;
        let mut l = BTreeMap::new();
        l.insert(
            "PROJA-0".to_string(),
            (0..runs)
                .map(|i| control("addresses", LinkDir::In, &format!("run:x{i}"), Some(&[P])))
                .collect(),
        );
        e.replace_source_linked(&issues(1), &l, 1).unwrap();
        let per_run = crate::shape::RUN_RESOLVE_ROWS / (runs - 1) - 500;
        let filler: Vec<TypedXEdge> = (0..runs)
            .flat_map(|r| {
                (0..per_run).map(move |i| TypedXEdge {
                    source: Endpoint::new(CONTROL_STORE, format!("run:x{r}"), 0),
                    target: Endpoint::new(KNOWLEDGE_STORE, format!("n{i:05}"), 0),
                    rel: "addresses".into(),
                    owner: "dead-owner".into(),
                    confidence: 1.0,
                    meta: "{}".into(),
                })
            })
            .collect();
        e.xedge.as_ref().unwrap().put_typed_batch(&filler).unwrap();
        let n = walk(
            &e,
            KNOWLEDGE_STORE,
            &doc_symbol(A, "PROJA-0").0,
            &visible(&[&dep(A), P]),
            1,
        )
        .unwrap();
        let shown_runs = n.nodes.iter().filter(|x| x.store == CONTROL_STORE).count();
        assert_eq!(shown_runs, runs - 1, "the run past the budget is hidden");
        assert!(n.truncated, "and the answer says so");
    }

    /// pi r2: a tracker's documents never take an `about` link, so `recall_about_code` can never
    /// surface one (another project's tracker included).
    #[test]
    fn r2_tracker_documents_take_no_about_link() {
        let mut e = engine();
        link(&mut e, A, 1, &[P]);
        assert!(
            e.check_about_link_target(&doc_symbol(A, "tracker"))
                .is_err()
        );
        assert!(
            crate::KnowledgeApi::recall_about_code_hits(
                &e,
                &["rust::app::Thing".to_string()],
                &visible(&[&dep(A), Q])
            )
            .unwrap()
            .is_empty()
        );
    }
}
