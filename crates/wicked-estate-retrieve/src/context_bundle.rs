//! `ContextBundle` — one-shot agent context retrieval (W12).
//!
//! An agent answering "give me context on X" otherwise has to orchestrate four tools:
//! `SearchEntity`/`RetrieveEntity` (resolve the seed) → `TraverseGraph` or `BlastRadius`
//! (gather neighbours) → `ContextPack` (budget-pack stubs). `ContextBundle` collapses that
//! into a single MCP call: resolve the seed, gather its graph neighbours (callers + callees),
//! rank them with personalised PageRank, and render the seed + top neighbours as elided stubs
//! packed within a character budget.
//!
//! # Reuse, not reinvention
//!
//! The budgeting machinery is **not** re-implemented here. The packed `rendered` text comes from
//! [`crate::render_context`] (W4.2 — token-budgeted, ranked, elided-stub renderer) and each
//! per-neighbour `stub` comes from [`crate::render_stub`]. This module only adds the seed
//! resolution + neighbour gathering + bundle assembly on top of the existing renderer.
//!
//! # Agent-behavior rules honored
//!
//! * R1 — never `isError`; a missing/unresolvable seed returns empty `content` + a diagnostic.
//! * R4 — output is budget-packed: the char budget is hard-capped below the ~25K agent limit and
//!   the rendered text is produced by the token-budgeted [`crate::render_context`].
//! * R5 — a staleness note is always emitted (the MCP layer fills in `commits_behind`).
//! * R7 — low-confidence neighbour edges are flagged in diagnostics.

use serde_json::{Value, json};
use std::collections::HashSet;
use wicked_estate_core::{
    Direction, GraphRead, Node, Result, RetrievalResult, RetrievalTool, SymbolId, SymbolQuery,
};

/// Default character budget when the caller supplies none (~8000 chars).
const DEFAULT_CHAR_BUDGET: usize = 8_000;

/// Hard cap on the character budget — R4 (output < ~25K chars). Kept comfortably below 25K so
/// the rendered block plus the small JSON envelope still clears the agent output limit.
const MAX_CHAR_BUDGET: usize = 24_000;

/// 4 chars ≈ 1 token — the same ratio [`crate::render_context`] uses internally. Used to convert
/// the caller's *character* budget into the *token* budget that the renderer expects, so a single
/// budget knob drives both the per-neighbour stub packing and the rendered block.
const CHARS_PER_TOKEN: usize = 4;

/// Render a node as the JSON object used for the seed and each neighbour entry.
/// `stub` reuses the crate's [`crate::render_stub`] (signature + first doc line + `// file:line`).
fn node_entry(node: &Node) -> Value {
    json!({
        "symbol": node.symbol.as_str(),
        "name": node.name,
        "kind": serde_json::to_value(&node.kind).unwrap_or(Value::Null),
        "file": node.location.file,
        "line": node.location.span.start_line,
        "stub": crate::render_stub(node),
    })
}

/// One-shot context bundle: seed + ranked neighbours + budget-packed stubs.
///
/// **Request shape** (one of `symbol` or `query` is required)
/// ```json
/// { "symbol": "<symbol-id>", "budget": 8000 }
/// ```
/// ```json
/// { "query": "<symbol name / FTS text>", "budget": 8000 }
/// ```
/// * `symbol` — resolve the seed directly by stable [`SymbolId`].
/// * `query`  — resolve the seed by name / FTS search; the top hit becomes the seed.
/// * `budget` — optional character budget, default [`DEFAULT_CHAR_BUDGET`], hard-capped at
///   [`MAX_CHAR_BUDGET`] (< 25K for R4).
///
/// **Response `content` shape**
/// ```json
/// { "seed": { "symbol": "…", "name": "…", "kind": "…", "file": "…", "line": 0, "stub": "…" },
///   "neighbors": [ { "symbol": "…", "name": "…", "kind": "…", "file": "…", "line": 0,
///                    "stub": "…" }, … ],
///   "rendered": "<budget-packed elided-stub text>",
///   "truncated": true }
/// ```
/// * `seed` — the resolved seed node (or `null` when nothing resolved — R1, no error).
/// * `neighbors` — ranked callers + callees whose stubs fit within `budget`.
/// * `rendered` — the packed stub block from [`crate::render_context`], ready to paste into a
///   prompt.
/// * `truncated` — `true` when at least one ranked neighbour did not fit in `budget`.
#[derive(Debug, Default)]
pub struct ContextBundle;

impl ContextBundle {
    /// Resolve the seed [`SymbolId`] from either an explicit `symbol` id or a `query` to search.
    /// Returns the resolved seed node, or `None` (with a diagnostic pushed) when nothing matches.
    fn resolve_seed(
        store: &dyn GraphRead,
        request: &Value,
        diag: &mut Vec<String>,
    ) -> Result<Option<Node>> {
        // 1. Explicit symbol id.
        if let Some(s) = request.get("symbol").and_then(|v| v.as_str()) {
            if !s.is_empty() {
                let id = SymbolId(s.to_string());
                match store.get_node(&id)? {
                    Some(node) => return Ok(Some(node)),
                    None => {
                        diag.push(format!("ContextBundle: symbol '{s}' not found in graph"));
                        return Ok(None);
                    }
                }
            }
        }

        // 2. Name / FTS query — the top hit becomes the seed.
        if let Some(q) = request.get("query").and_then(|v| v.as_str()) {
            if !q.is_empty() {
                let query = SymbolQuery {
                    text: Some(q.to_string()),
                    limit: Some(20),
                    ..Default::default()
                };
                let hits = store.find_symbols(&query)?;
                match hits.into_iter().next() {
                    Some(node) => return Ok(Some(node)),
                    None => {
                        diag.push(format!(
                            "ContextBundle: no symbols found matching query '{q}'"
                        ));
                        return Ok(None);
                    }
                }
            }
        }

        diag.push(
            "ContextBundle: provide 'symbol' (a stable symbol id) or 'query' (a name to resolve)"
                .to_string(),
        );
        Ok(None)
    }
}

impl RetrievalTool for ContextBundle {
    fn name(&self) -> &str {
        "ContextBundle"
    }

    fn description(&self) -> &str {
        "One-shot context bundle for a symbol: resolves the seed (by id or by name/FTS query), \
         gathers its graph neighbours (callers + callees), ranks them by personalised PageRank, \
         and returns the seed plus the top neighbours as token-budgeted elided stubs packed \
         within a character budget. Replaces orchestrating SearchEntity + TraverseGraph + \
         ContextPack by hand — use it to fill an agent's context window in a single call."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let mut diag: Vec<String> = Vec::new();

        // ── parse char budget (R4: hard-capped below the ~25K agent limit) ──────
        let char_budget = request
            .get("budget")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_CHAR_BUDGET)
            .clamp(1, MAX_CHAR_BUDGET);

        // ── resolve the seed (R1: missing/unknown → empty content + diagnostic) ──
        let seed = match Self::resolve_seed(store, request, &mut diag)? {
            Some(node) => node,
            None => {
                diag.push(crate::staleness_note());
                return Ok(RetrievalResult {
                    content: json!({
                        "seed": Value::Null,
                        "neighbors": [],
                        "rendered": "",
                        "truncated": false,
                    }),
                    diagnostics: diag,
                });
            }
        };
        let seed_id = seed.symbol.clone();

        // ── gather neighbours: direct callers (in-edges) + callees (out-edges) ───
        let mut neighbour_ids: HashSet<SymbolId> = HashSet::new();
        let mut low_conf = 0usize;
        for edge in store.neighbors(&seed_id, Direction::Dependencies)? {
            if edge.confidence.get() < 0.5 {
                low_conf += 1;
            }
            if edge.target != seed_id {
                neighbour_ids.insert(edge.target.clone());
            }
        }
        for edge in store.neighbors(&seed_id, Direction::Dependents)? {
            if edge.confidence.get() < 0.5 {
                low_conf += 1;
            }
            if edge.source != seed_id {
                neighbour_ids.insert(edge.source.clone());
            }
        }

        // ── rank neighbours via personalised PageRank seeded on the seed ─────────
        // ranked_symbols returns ALL store nodes ranked; keep neighbours in rank order, then
        // append any neighbour the ranker did not surface so none are silently dropped.
        let ranked =
            wicked_estate_rank::ranked_symbols(store, std::slice::from_ref(&seed_id), 500)?;
        let mut ordered: Vec<SymbolId> = ranked
            .into_iter()
            .filter_map(|(id, _)| neighbour_ids.contains(&id).then_some(id))
            .collect();
        let seen: HashSet<SymbolId> = ordered.iter().cloned().collect();
        for id in &neighbour_ids {
            if !seen.contains(id) {
                ordered.push(id.clone());
            }
        }

        // ── pack neighbour entries within the char budget ────────────────────────
        // The seed's own stub is charged against the budget first (it is the centre of the
        // bundle); neighbours fill the remainder. `truncated` is set when a ranked neighbour
        // is dropped for space.
        let seed_entry = node_entry(&seed);
        let mut chars_used = crate::render_stub(&seed).len();
        let mut neighbors: Vec<Value> = Vec::new();
        let mut truncated = false;

        for id in &ordered {
            let Some(node) = store.get_node(id)? else {
                continue; // stale neighbour id; skip silently
            };
            let stub = crate::render_stub(&node);
            if chars_used + stub.len() > char_budget {
                truncated = true;
                continue;
            }
            chars_used += stub.len();
            neighbors.push(node_entry(&node));
        }

        // ── budget-packed rendered block — reuse render_context (W4.2) ───────────
        // Convert the char budget to the token budget render_context expects (chars / 4).
        // render_context itself caps tokens at MAX_TOKEN_BUDGET (≈24K chars), so the rendered
        // text is independently bounded below the R4 limit.
        let token_budget = char_budget / CHARS_PER_TOKEN;
        let rendered = crate::render_context(store, std::slice::from_ref(&seed_id), token_budget)?;

        // ── diagnostics ──────────────────────────────────────────────────────────
        if neighbors.is_empty() {
            diag.push(format!(
                "ContextBundle: seed '{}' has no graph neighbours (leaf or not yet indexed)",
                seed_id.as_str()
            ));
        }
        if truncated {
            diag.push(format!(
                "ContextBundle: neighbour set truncated to fit the {char_budget}-char budget"
            ));
        }
        if low_conf > 0 {
            diag.push(format!(
                "R7-CONFIDENCE: {low_conf} neighbour edge(s) below 0.5 confidence"
            ));
        }
        diag.push(crate::staleness_note());

        Ok(RetrievalResult {
            content: json!({
                "seed": seed_entry,
                "neighbors": neighbors,
                "rendered": rendered,
                "truncated": truncated,
            }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wicked_estate_core::{
        Edge, EdgeKind, GraphWrite, Language, Location, NodeKind, ResolutionTier, Span, SymbolId,
    };
    use wicked_estate_store::MemStore;

    fn span(line: u32) -> Span {
        Span {
            start_byte: 0,
            end_byte: 0,
            start_line: line,
            start_col: 0,
            end_line: line,
            end_col: 0,
        }
    }

    fn make_node(id: &str, name: &str, file: &str, line: u32) -> Node {
        Node::new(
            SymbolId(id.to_string()),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new(file, span(line)),
        )
    }

    fn call_edge(src: &str, tgt: &str) -> Edge {
        Edge::new(
            SymbolId(src.to_string()),
            SymbolId(tgt.to_string()),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test-fixture",
        )
    }

    /// `caller` → `middle` → `leaf`. `middle` has both a caller and a callee.
    fn fixture() -> MemStore {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node("caller", "caller_fn", "src/a.rs", 1),
                make_node("middle", "middle_fn", "src/b.rs", 10),
                make_node("leaf", "leaf_fn", "src/c.rs", 20),
            ])
            .unwrap();
        store
            .upsert_edges(&[call_edge("caller", "middle"), call_edge("middle", "leaf")])
            .unwrap();
        store.commit_batch().unwrap();
        store
    }

    #[test]
    fn bundle_returns_seed_and_neighbors_by_symbol() {
        let store = fixture();
        let tool = ContextBundle;
        // middle has caller (dependent) and leaf (dependency) as neighbours.
        let res = tool
            .invoke(&store, &json!({"symbol": "middle", "budget": 8000}))
            .unwrap();

        assert_eq!(res.content["seed"]["name"].as_str().unwrap(), "middle_fn");
        assert_eq!(res.content["seed"]["symbol"].as_str().unwrap(), "middle");
        assert!(
            res.content["seed"]["stub"]
                .as_str()
                .unwrap()
                .contains("middle_fn"),
            "seed stub must render the seed symbol"
        );

        let neighbors = res.content["neighbors"].as_array().unwrap();
        assert!(
            !neighbors.is_empty(),
            "middle must have >= 1 neighbour (caller + leaf)"
        );
        let names: Vec<&str> = neighbors
            .iter()
            .map(|n| n["name"].as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"caller_fn") || names.contains(&"leaf_fn"),
            "a known caller/callee must appear, got {names:?}"
        );
        // The seed must not appear in its own neighbour list.
        assert!(
            !names.contains(&"middle_fn"),
            "seed excluded from neighbour list"
        );
        // Every neighbour carries a rendered stub.
        for n in neighbors {
            assert!(
                n["stub"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
                "each neighbour must carry a non-empty stub"
            );
        }
    }

    #[test]
    fn bundle_resolves_seed_by_query() {
        let store = fixture();
        let tool = ContextBundle;
        let res = tool.invoke(&store, &json!({"query": "middle_fn"})).unwrap();

        assert_eq!(
            res.content["seed"]["name"].as_str().unwrap(),
            "middle_fn",
            "query must resolve to the matching seed"
        );
    }

    #[test]
    fn bundle_respects_budget_and_sets_truncated_flag() {
        let store = fixture();
        let tool = ContextBundle;
        // A tiny budget: the seed stub alone exhausts it, so every neighbour is dropped.
        let res = tool
            .invoke(&store, &json!({"symbol": "middle", "budget": 1}))
            .unwrap();

        let neighbors = res.content["neighbors"].as_array().unwrap();
        assert!(
            neighbors.is_empty(),
            "a 1-char budget must drop all neighbours"
        );
        assert!(
            res.content["truncated"].as_bool().unwrap(),
            "truncated must be true when a neighbour is dropped for budget"
        );

        // The rendered block is independently bounded by render_context's token budget
        // (char_budget / 4), plus the small trailing summary line render_context appends.
        let rendered = res.content["rendered"].as_str().unwrap();
        assert!(
            rendered.len() <= (1 / CHARS_PER_TOKEN).max(1) * 4 + 200,
            "rendered block ({}) must stay within the converted token budget + summary overhead",
            rendered.len()
        );
    }

    #[test]
    fn bundle_rendered_within_budget_when_ample() {
        let store = fixture();
        let tool = ContextBundle;
        let budget = 8000usize;
        let res = tool
            .invoke(&store, &json!({"symbol": "middle", "budget": budget}))
            .unwrap();

        let rendered = res.content["rendered"].as_str().unwrap();
        // render_context packs to chars ≈ token_budget * 4 = (budget/4)*4 = budget; allow the
        // trailing summary line as overhead.
        assert!(
            rendered.len() <= budget + 200,
            "rendered block ({}) must stay within the char budget ({budget}) + summary overhead",
            rendered.len()
        );
        assert!(!rendered.is_empty(), "rendered block must be non-empty");
    }

    #[test]
    fn bundle_budget_hard_capped_for_r4() {
        let store = fixture();
        let tool = ContextBundle;
        // Request a budget far above the R4 ceiling.
        let res = tool
            .invoke(&store, &json!({"symbol": "middle", "budget": 9_999_999u64}))
            .unwrap();
        let rendered = res.content["rendered"].as_str().unwrap();
        // Even with an absurd request, the rendered block stays under the ~25K agent limit
        // (render_context caps tokens at MAX_TOKEN_BUDGET ≈ 24K chars).
        assert!(
            rendered.len() < 25_000,
            "rendered block must stay under the ~25K R4 limit, got {}",
            rendered.len()
        );
    }

    #[test]
    fn bundle_missing_symbol_no_error_empty_content() {
        // R1: an unknown symbol must NOT be an error — empty content + a diagnostic.
        let store = fixture();
        let tool = ContextBundle;
        let res = tool.invoke(&store, &json!({"symbol": "ghost_xyz"}));

        assert!(res.is_ok(), "ContextBundle must return Ok, never Err (R1)");
        let res = res.unwrap();
        assert!(res.content["seed"].is_null(), "seed must be null on miss");
        assert!(
            res.content["neighbors"].as_array().unwrap().is_empty(),
            "neighbors must be empty on miss"
        );
        assert_eq!(res.content["rendered"].as_str().unwrap(), "");
        assert!(
            !res.content["truncated"].as_bool().unwrap(),
            "truncated false on a clean miss"
        );
        assert!(
            res.diagnostics.iter().any(|d| d.contains("not found")),
            "diagnostic must explain the miss"
        );
    }

    #[test]
    fn bundle_missing_query_match_no_error() {
        let store = fixture();
        let tool = ContextBundle;
        let res = tool
            .invoke(&store, &json!({"query": "no_such_symbol_xyz"}))
            .unwrap();

        assert!(res.content["seed"].is_null());
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("no symbols found")),
            "diagnostic must explain the empty query result"
        );
    }

    #[test]
    fn bundle_missing_both_fields_returns_diagnostic() {
        let store = fixture();
        let tool = ContextBundle;
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert!(res.content["seed"].is_null());
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("'symbol'") && d.contains("'query'")),
            "diagnostic must name both accepted fields"
        );
    }

    #[test]
    fn bundle_staleness_note_always_present() {
        let store = fixture();
        let tool = ContextBundle;
        // Present on the success path…
        let ok = tool.invoke(&store, &json!({"symbol": "middle"})).unwrap();
        assert!(
            ok.diagnostics.iter().any(|d| d.contains("STALENESS")),
            "R5 staleness note must be present on success"
        );
        // …and on the miss path.
        let miss = tool.invoke(&store, &json!({"symbol": "ghost"})).unwrap();
        assert!(
            miss.diagnostics.iter().any(|d| d.contains("STALENESS")),
            "R5 staleness note must be present on a miss too"
        );
    }
}
