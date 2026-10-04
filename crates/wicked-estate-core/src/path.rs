//! Path queries — the ordered hops from one symbol to another.
//!
//! `blast-radius` answers *which* symbols are reachable; this answers *how* you get there, so
//! an agent can name the intermediate functions and read only those files. See
//! `docs/specs/path-query/spec.md`.
//!
//! The seam lives in core rather than in the `wicked-estate` binary crate because both
//! consumers must reach it and `wicked-estate-retrieve` does not depend on `wicked-estate`
//! (the dependency runs the other way). It needs only core types.

use crate::edge::Edge;
use crate::node::Node;
use crate::query::{Subgraph, SymbolQuery, TraversalSpec};
use crate::symbol::SymbolId;
use crate::traits::GraphRead;
use crate::{Direction, Result};

/// Which endpoint of a path request could not be resolved.
///
/// `#[non_exhaustive]` so a new failure mode can be added without a breaking release; match it
/// with a wildcard arm outside this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unresolved {
    From,
    To,
}

impl Unresolved {
    /// The wire form both surfaces report (`"from"` / `"to"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Unresolved::From => "from",
            Unresolved::To => "to",
        }
    }
}

/// The result of a path query.
///
/// `found == false` with both bound flags clear is a **proven** absence — no route exists in
/// the graph. `found == false` with either flag set is a **bounded** absence: the search was
/// cut off and a route may lie beyond it. Conflating the two is the R3 failure the engine
/// contract forbids, which is why both flags are reported rather than one "truncated" bit.
///
/// `#[non_exhaustive]` so a new field (a further bound cause, say) is not a breaking change.
/// Outside this crate, build one from `PathResult::default()` and assign the fields.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PathResult {
    /// The route, in order from `from` to `to`. Empty when `found` is false, and also when
    /// the two endpoints resolve to the same symbol (a zero-hop route is still `found`).
    pub hops: Vec<Edge>,
    /// Every node appearing as a hop endpoint, from the traversal that found the route.
    ///
    /// Both surfaces render endpoints from this instead of calling `get_node` per endpoint —
    /// up to 17 store queries on a 16-hop path, which the no-per-node-query rule forbids.
    /// The edge-admission rule guarantees both endpoints of every admitted hop are nodes of
    /// the traversal, so this is always complete for `hops`.
    pub endpoints: Vec<Node>,
    /// A route was found.
    pub found: bool,
    /// Some candidate traversal reached its depth frontier — a node sits at exactly
    /// `max_depth` in its `depths` map.
    ///
    /// Derived here from `depths`, never read off [`Subgraph::truncated`], which folds the node
    /// cap in as well. With a multi-match `from` this is the **disjunction** over every candidate
    /// traversal: reporting the winning candidate's flag alone would let a query whose other
    /// candidate walk was cut off return a proven-absence signal for a bounded search.
    ///
    /// Deliberately conservative — true whenever the frontier is *touched*, including when a
    /// route was found and when nothing remained to expand. `false` means the walk saw its
    /// whole reachable set; `true` does not mean a route exists further out.
    pub depth_bounded: bool,
    /// Some candidate traversal exhausted its node budget ([`Subgraph::node_cap_reached`]).
    ///
    /// Read from the node-cap cause alone: [`Subgraph::truncated`] is also true when only the
    /// depth horizon cut the walk, which would report a depth cut as a node-budget cut.
    ///
    /// Also a disjunction across candidates, for the same reason as `depth_bounded`.
    /// Backend-approximate: the stores budget on different populations (nodes with a stored
    /// `Node` vs every reached interned symbol vs a `max_nodes + 1` fencepost) and retain
    /// different node sets once the cap binds, so results are comparable across backends
    /// only while it does not.
    pub node_bounded: bool,
    /// Which endpoint failed to resolve, if either did. `Some(_)` always implies
    /// `found == false`, and distinguishes "you named something that isn't here" from
    /// "these two exist but nothing connects them".
    pub unresolved: Option<Unresolved>,
}

/// Resolve one endpoint to its candidate symbols: exact name first, then `SymbolId`.
///
/// The name query matches the one `blast-radius` issues, including its rule that a synthetic
/// value-flow node (wicked-estate#207, `Node::is_value_flow_node`) is never resolved by NAME:
/// those slots are addressable only by their exact `SymbolId`, which the fallback below still
/// accepts. The same semantics hold by construction, not by sharing a helper (that helper
/// lives in a crate core cannot depend on). Candidates are sorted by `SymbolId` string so the
/// winner is a property of the data: `MemStore` sorts `find_symbols` by symbol string while
/// `SqliteStore` orders by an autoincrement row id, and taking either store's order would
/// pick a different winner on the same graph.
///
/// Public so the CLI's RetrievalTool bridge resolves a `traverse` operand under exactly this
/// rule — one name-vs-id visibility policy for `path` and every bridged command, not a copy
/// per surface (CLAUDE.md §11).
pub fn resolve_operand(store: &dyn GraphRead, value: &str) -> Result<Vec<SymbolId>> {
    let query = SymbolQuery {
        exact_name: Some(value.to_string()),
        ..Default::default()
    };
    let mut ids: Vec<SymbolId> = store
        .find_symbols(&query)?
        .into_iter()
        .filter(|n| !n.is_value_flow_node())
        .map(|n| n.symbol)
        .collect();
    if ids.is_empty() {
        // Not a symbol name — the caller may be passing an id straight back from
        // SearchEntity or TraverseGraph, which is the form an agent actually holds.
        let as_id = SymbolId(value.to_string());
        if store.get_node(&as_id)?.is_some() {
            ids.push(as_id);
        }
    }
    ids.sort_by(|a, b| a.0.cmp(&b.0));
    ids.dedup();
    Ok(ids)
}

/// True when this traversal reached its depth frontier — some node sits at exactly
/// `max_depth`. Deliberately coarser than [`Subgraph::depth_horizon_reached`]: it is true
/// whenever the frontier is touched, whether or not anything lay beyond it.
fn touched_depth_frontier(subgraph: &Subgraph, max_depth: u32) -> bool {
    subgraph.depths.values().any(|d| *d >= max_depth)
}

/// The route from `from` to `to` following dependency edges, within `max_depth` hops and
/// `max_nodes` visited nodes.
///
/// Each endpoint is an exact symbol name or, failing that, a `SymbolId`. Issues exactly one
/// [`GraphRead::traverse`] per resolved `from` candidate and no per-node query; the route is
/// reconstructed in memory from the returned subgraph.
pub fn path_between(
    store: &dyn GraphRead,
    from: &str,
    to: &str,
    max_depth: u32,
    max_nodes: usize,
) -> Result<PathResult> {
    let from_ids = resolve_operand(store, from)?;
    if from_ids.is_empty() {
        return Ok(PathResult {
            unresolved: Some(Unresolved::From),
            ..Default::default()
        });
    }
    let to_ids = resolve_operand(store, to)?;
    if to_ids.is_empty() {
        return Ok(PathResult {
            unresolved: Some(Unresolved::To),
            ..Default::default()
        });
    }

    let spec = TraversalSpec {
        direction: Direction::Dependencies,
        edge_kinds: vec![],
        max_depth,
        max_nodes,
        // 0.0 deliberately: a confidence filter here would drop exactly the low-confidence
        // hops the R7 contract requires the response to count and flag.
        min_confidence: 0.0,
    };

    let mut best: Option<(Vec<Edge>, Subgraph)> = None;
    // Disjunctions, not the winning candidate's values — see the field docs.
    let mut depth_bounded = false;
    let mut node_bounded = false;

    for start in &from_ids {
        let subgraph = store.traverse(start, &spec)?;
        depth_bounded |= touched_depth_frontier(&subgraph, max_depth);
        node_bounded |= subgraph.node_cap_reached;

        if let Some(hops) = subgraph.shortest_path(start, &to_ids) {
            let shorter = match &best {
                Some((current, _)) => hops.len() < current.len(),
                None => true,
            };
            if shorter {
                best = Some((hops, subgraph));
            }
        }
    }

    let Some((hops, subgraph)) = best else {
        return Ok(PathResult {
            depth_bounded,
            node_bounded,
            ..Default::default()
        });
    };

    // Endpoint nodes come from the traversal that found the route, so neither surface needs
    // a store lookup to denormalize a hop.
    // A set, not a Vec: this is scanned once per subgraph node, and a subgraph may hold
    // `max_nodes` of them while the route has at most 17 endpoints.
    let wanted: std::collections::HashSet<&SymbolId> = hops
        .iter()
        .flat_map(|hop| [&hop.source, &hop.target])
        .collect();
    let endpoints: Vec<Node> = subgraph
        .nodes
        .iter()
        .filter(|n| wanted.contains(&n.symbol))
        .cloned()
        .collect();

    Ok(PathResult {
        hops,
        endpoints,
        found: true,
        depth_bounded,
        node_bounded,
        unresolved: None,
    })
}
