//! Crossings: how an answer says a walk left one repo for another (DES-S2 §5, slice X1).
//!
//! A crossing is a traversed edge written by the project linker (`resolved_by` [`LINK_RESOLVED_BY`])
//! whose two ends are in different member repos. Nothing else counts: an unresolved import
//! placeholder shared by two repos (DES-S1 F2) is never attributed to a repo and never ends a
//! crossing. Each crossing carries the link's evidence; each dependent reached through one names it
//! (`via_crossing`), derived from ONE predecessor chain per dependent (smallest depth, then edge
//! kind, then symbol id: deterministic).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{Edge, query::Subgraph};

/// `resolved_by` of every edge the project linker writes.
pub const LINK_RESOLVED_BY: &str = "wicked-link";

/// One end of a crossing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossingEnd {
    pub repo: String,
    pub symbol: String,
}

/// One hop of a crossing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossingHop {
    pub kind: String,
    pub resolved_by: String,
}

/// A walk's step from one repo into another, with the link's evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crossing {
    /// Stable id: a hash of the edge key.
    pub id: String,
    /// Depth of the dependent end (the consumer) in the walk.
    pub depth: u32,
    /// The dependent side (the consumer file).
    pub from: CrossingEnd,
    /// The dependency side (the provider's entry file).
    pub to: CrossingEnd,
    pub hops: Vec<CrossingHop>,
    /// The link's evidence: `identity`, `package`, `ecosystem`, `confidence`, `skew`, versions.
    pub evidence: serde_json::Map<String, serde_json::Value>,
}

fn is_link(e: &Edge) -> bool {
    e.resolved_by == LINK_RESOLVED_BY
}

fn crossing_id(e: &Edge) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in e
        .source
        .0
        .bytes()
        .chain([0])
        .chain(e.target.0.bytes())
        .chain([0])
        .chain(format!("{:?}", e.kind).bytes())
    {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("x{h:016x}")
}

fn md_str(e: &Edge, k: &str) -> String {
    e.metadata
        .get(k)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// The crossing an edge is, at `depth`: a link edge whose consumer and provider repos differ.
fn crossing_of(e: &Edge, depth: u32) -> Option<Crossing> {
    if !is_link(e) {
        return None;
    }
    let from_repo = md_str(e, "link.consumer_label");
    let to_repo = md_str(e, "link.provider_label");
    if from_repo == to_repo {
        return None; // a link inside one repo is a contract hop, not a crossing
    }
    let mut evidence = serde_json::Map::new();
    for (k, v) in &e.metadata {
        let Some(short) = k
            .strip_prefix("link.")
            .filter(|s| !matches!(*s, "consumer_label" | "provider_label"))
        else {
            continue;
        };
        evidence.insert(short.to_string(), v.clone());
    }
    Some(Crossing {
        id: crossing_id(e),
        depth,
        from: CrossingEnd {
            repo: from_repo,
            symbol: e.source.0.clone(),
        },
        to: CrossingEnd {
            repo: to_repo,
            symbol: e.target.0.clone(),
        },
        hops: vec![CrossingHop {
            kind: "imports".into(),
            resolved_by: e.resolved_by.clone(),
        }],
        evidence,
    })
}

/// The crossings on a `path` route, in route order (`depth` = the hop's 1-based position).
pub fn path_crossings(hops: &[Edge]) -> Vec<Crossing> {
    hops.iter()
        .enumerate()
        .filter_map(|(i, e)| crossing_of(e, u32::try_from(i + 1).unwrap_or(u32::MAX)))
        .collect()
}

/// The crossings `sub` traversed, and for each dependent reached through one, that crossing's id.
/// `sub` is a dependents walk (edges point dependent -> dependency).
pub fn crossings(sub: &Subgraph) -> (Vec<Crossing>, BTreeMap<String, String>) {
    let mut out: BTreeMap<String, Crossing> = BTreeMap::new();
    // The start carries no depth row: depth 0.
    let depth_of = |s: &str| sub.depths.get(s).copied().unwrap_or(0);
    for e in sub.edges.iter().filter(|e| is_link(e)) {
        // Only an edge the walk ADMITTED: its dependent end was reached (has a depth) and its
        // dependency end is the start or was reached. A frontier edge past the node or depth cap
        // is not part of the answer.
        let Some(ds) = sub.depths.get(e.source.as_str()).copied() else {
            continue;
        };
        if !(ds == 1 || sub.depths.contains_key(e.target.as_str())) {
            continue;
        }
        if let Some(c) = crossing_of(e, ds) {
            out.insert(c.id.clone(), c);
        }
    }
    // One predecessor per node: the edge to a node one hop closer to the start (a target with no
    // depth row is the start, depth 0), the same deterministic tie-break everywhere.
    let mut parent: HashMap<&str, &Edge> = HashMap::new();
    for e in &sub.edges {
        let Some(ds) = sub.depths.get(e.source.as_str()).copied() else {
            continue;
        };
        if ds != depth_of(e.target.as_str()) + 1 {
            continue;
        }
        let keep = match parent.get(e.source.as_str()) {
            None => true,
            Some(p) => {
                (format!("{:?}", e.kind), e.target.as_str())
                    < (format!("{:?}", p.kind), p.target.as_str())
            }
        };
        if keep {
            parent.insert(e.source.as_str(), e);
        }
    }
    let mut via = BTreeMap::new();
    for sym in sub.depths.keys() {
        let mut cur: &str = sym;
        let mut seen = 0usize;
        let mut found: Option<String> = None;
        while let Some(e) = parent.get(cur) {
            if is_link(e) && out.contains_key(&crossing_id(e)) {
                found = Some(crossing_id(e));
            }
            cur = e.target.as_str();
            seen += 1;
            if seen > sub.depths.len() {
                break; // a cycle guard; depths strictly decrease, so this never fires on a sane walk
            }
        }
        if let Some(id) = found {
            via.insert(sym.clone(), id);
        }
    }
    (out.into_values().collect(), via)
}

/// Crossings folded over several walks (a name that resolves to several starts): each crossing
/// once, and each dependent's `via_crossing` from the walk that reached it at the smallest depth
/// (ties: the smaller crossing id), so it describes the same walk as the dependent's depth.
#[derive(Debug, Default)]
pub struct CrossingSet {
    crossings: BTreeMap<String, Crossing>,
    /// Per reached symbol: its smallest depth over the walks, and that walk's crossing (if any).
    via: BTreeMap<String, (u32, Option<String>)>,
}

/// Most crossings an answer lists; the rest are counted (`crossings_total`).
pub const CROSSINGS_CAP: usize = 20;

impl CrossingSet {
    pub fn add_walk(&mut self, sub: &Subgraph) {
        let (cs, via) = crossings(sub);
        for c in cs {
            match self.crossings.get(&c.id) {
                Some(x) if x.depth <= c.depth => {}
                _ => {
                    self.crossings.insert(c.id.clone(), c);
                }
            }
        }
        for (sym, &d) in &sub.depths {
            let id = via.get(sym).cloned();
            // Smaller depth wins; at equal depth a walk through a crossing wins (it is the
            // cross-repo fact), then the smaller id: deterministic.
            let better = match self.via.get(sym) {
                None => true,
                Some((bd, bid)) => (d, id.is_none(), &id) < (*bd, bid.is_none(), bid),
            };
            if better {
                self.via.insert(sym.clone(), (d, id));
            }
        }
    }

    /// `(crossings by depth then id, capped at `cap`; the total; dependent -> crossing id)`.
    pub fn finish(self, cap: usize) -> (Vec<Crossing>, usize, BTreeMap<String, String>) {
        let mut all: Vec<Crossing> = self.crossings.into_values().collect();
        all.sort_by(|a, b| (a.depth, &a.id).cmp(&(b.depth, &b.id)));
        let total = all.len();
        all.truncate(cap);
        let via = self
            .via
            .into_iter()
            .filter_map(|(k, (_, id))| id.map(|i| (k, i)))
            .collect();
        (all, total, via)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgeKind, ResolutionTier, SymbolId};

    fn edge(s: &str, t: &str, link: bool) -> Edge {
        let mut e = Edge::new(
            SymbolId(s.into()),
            SymbolId(t.into()),
            EdgeKind::Imports,
            ResolutionTier::ImportMap,
            if link { LINK_RESOLVED_BY } else { "import-map" },
        );
        if link {
            e.metadata
                .insert("link.consumer_label".into(), "studio".into());
            e.metadata
                .insert("link.provider_label".into(), "crew".into());
            e.metadata
                .insert("link.identity".into(), "registry-source".into());
        }
        e
    }

    /// G1.7 / G2.1: the crossing names the link edge and its evidence; dependents past it carry
    /// `via_crossing`; a non-link import (a shared placeholder) never is a crossing (G2.10).
    #[test]
    fn crossings_name_link_edges_and_dependents_through_them() {
        let mut sub = Subgraph {
            edges: vec![
                edge("file studio/a.ts", "file crew/index.d.ts", true),
                edge("fn studio/a#f", "file studio/a.ts", false),
                edge("file crew/other.ts", "file crew/index.d.ts", false),
            ],
            ..Default::default()
        };
        sub.depths.insert("file studio/a.ts".into(), 1);
        sub.depths.insert("fn studio/a#f".into(), 2);
        sub.depths.insert("file crew/other.ts".into(), 1);
        let (cs, via) = crossings(&sub);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].from.repo, "studio");
        assert_eq!(cs[0].to.repo, "crew");
        assert_eq!(
            cs[0].evidence.get("identity").and_then(|v| v.as_str()),
            Some("registry-source")
        );
        assert_eq!(via.get("fn studio/a#f"), Some(&cs[0].id));
        assert_eq!(via.get("file studio/a.ts"), Some(&cs[0].id));
        assert!(!via.contains_key("file crew/other.ts"));
    }

    /// C20: a link edge whose dependent end the walk did not admit (a frontier edge past the cap)
    /// is not a crossing.
    #[test]
    fn frontier_link_edges_are_not_crossings() {
        let mut sub = Subgraph {
            edges: vec![
                edge("file studio/a.ts", "file crew/index.d.ts", true),
                edge("file studio/far.ts", "file studio/mid.ts", true),
            ],
            ..Default::default()
        };
        sub.depths.insert("file studio/a.ts".into(), 1);
        // `far.ts` was never admitted (no depth row); `mid.ts` was never reached either.
        let (cs, _) = crossings(&sub);
        assert_eq!(cs.len(), 1, "{cs:?}");
        assert_eq!(cs[0].from.symbol, "file studio/a.ts");
    }

    /// C21: over two walks, a dependent's `via_crossing` comes from the walk that reached it at the
    /// smallest depth; a walk reaching it in-repo first leaves no stale crossing behind.
    #[test]
    fn merged_walks_keep_the_shallowest_chain() {
        let mut deep = Subgraph {
            edges: vec![
                edge("file studio/a.ts", "file crew/index.d.ts", true),
                edge("fn studio/x#f", "file studio/a.ts", false),
            ],
            ..Default::default()
        };
        deep.depths.insert("file studio/a.ts".into(), 1);
        deep.depths.insert("fn studio/x#f".into(), 2);
        let mut set = CrossingSet::default();
        set.add_walk(&deep);
        let (cs, total, via) = set.finish(CROSSINGS_CAP);
        assert_eq!((cs.len(), total), (1, 1));
        assert_eq!(via.get("fn studio/x#f"), Some(&cs[0].id));

        // A second walk reaches the same dependent in-repo, shallower: no crossing for it.
        let mut shallow = Subgraph {
            edges: vec![edge("fn studio/x#f", "file studio/b.ts", false)],
            ..Default::default()
        };
        shallow.depths.insert("fn studio/x#f".into(), 1);
        let mut set = CrossingSet::default();
        set.add_walk(&deep);
        set.add_walk(&shallow);
        let (_, _, via) = set.finish(CROSSINGS_CAP);
        assert_eq!(via.get("fn studio/x#f"), None, "{via:?}");

        let mut cap = CrossingSet::default();
        cap.add_walk(&deep);
        let (cs, total, _) = cap.finish(0);
        assert!(cs.is_empty());
        assert_eq!(total, 1, "the cap counts what it drops");
    }

    /// K4: a `path` route names the hop that crosses repos, at its position.
    #[test]
    fn path_route_names_its_crossing() {
        let hops = vec![
            edge("fn studio/x#f", "file studio/a.ts", false),
            edge("file studio/a.ts", "file crew/index.d.ts", true),
        ];
        let cs = path_crossings(&hops);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].depth, 2);
        assert_eq!(
            (cs[0].from.repo.as_str(), cs[0].to.repo.as_str()),
            ("studio", "crew")
        );
    }

    #[test]
    fn no_link_edges_no_crossings() {
        let mut sub = Subgraph {
            edges: vec![edge("a", "b", false)],
            ..Default::default()
        };
        sub.depths.insert("a".into(), 1);
        let (cs, via) = crossings(&sub);
        assert!(cs.is_empty() && via.is_empty());
    }
}
