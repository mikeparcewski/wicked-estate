//! Bulk **source bundle** — full bodies for an entire file / cluster / symbol-set in one call.
//!
//! Consuming agents asked for one call that returns the exact source of a whole unit of work
//! (a file, a detected community, or an explicit symbol list) so they don't fan out N
//! `FetchContent` round-trips. The selector + budget parsing lives in `main.rs`'s `"source"`
//! arm; the bundle assembly + budget/truncation policy lives here as **pure functions** so it
//! is unit-testable against a `MemStore` fixture without going through the CLI.
//!
//! ## Budget semantics (reconciled — the owner overrode the spec's 12K/0 defaults)
//! * **Omitted budget = UNBOUNDED.** The caller owns its context window; the engine imposes no
//!   default cap. `--max-total-chars` / `--max-node-chars` only constrain when explicitly set to
//!   `N > 0`.
//! * `--signatures-only` is an explicit flag (`signatures_only`), never modelled as
//!   `max_total_chars: 0` — overloading `0` to mean two things is the footgun we refuse.
//! * **Loud truncation, never silent.** A per-node cap cuts the body to a prefix and sets
//!   `source_truncated: true`. When the total budget is exhausted, remaining nodes get
//!   `source: null` + `source_truncated: true` but keep FULL metadata (`symbol_id` / `byte_range`
//!   / `blob_sha` / `signature`) — the escape hatch is "re-fetch by byte_range/blob_sha". A node
//!   is **never dropped**, only its body.
//! * **Deterministic fill order.** Nodes are sorted by `(file, start_byte)` so the same selection
//!   truncates identically every run; the same db + selector + budget → byte-identical JSON.

use serde_json::{Value, json};
use wicked_estate_core::Node;

/// Explicit budget / shaping options for a source bundle. All caps are opt-in: `None` (and the
/// CLI's "flag omitted") means UNBOUNDED. `0` is treated as unbounded too — a cap of zero would
/// truncate every body to empty, which is what `signatures_only` already expresses cleanly.
#[derive(Debug, Clone, Copy, Default)]
pub struct BudgetOpts {
    /// Cap on the summed `source` characters across all nodes. `None` = unbounded.
    pub max_total_chars: Option<usize>,
    /// Cap on any single node's `source` characters. `None` = unbounded.
    pub max_node_chars: Option<usize>,
    /// Omit bodies entirely (`source: null`), keeping all metadata.
    pub signatures_only: bool,
}

impl BudgetOpts {
    /// Normalise a raw `--max-*` value: the CLI parses to `Option<usize>`, and a literal `0`
    /// (an explicitly-passed cap of zero) collapses to `None` (unbounded) so `0` never silently
    /// means "no bodies" — `signatures_only` owns that meaning.
    fn norm(cap: Option<usize>) -> Option<usize> {
        match cap {
            Some(0) | None => None,
            some => some,
        }
    }
}

/// Truncate `s` to at most `max` characters on a UTF-8 boundary, returning `(prefix, truncated)`.
/// Counts by `char` (not byte) so a cap is a stable, human-meaningful character budget and we
/// never split a multi-byte codepoint.
fn truncate_chars(s: &str, max: usize) -> (String, bool) {
    // The byte offset of the `max`-th char boundary, if the string is longer than `max` chars.
    // `enumerate()` yields (chars-before-this-position, byte_idx), so the entry with index `max`
    // sits exactly `max` chars in — `s[..byte_idx]` is then a `max`-char prefix on a valid
    // boundary.
    match s
        .char_indices()
        .enumerate()
        .find(|(count, _)| *count == max)
    {
        Some((_, (byte_idx, _))) => (s[..byte_idx].to_string(), true),
        None => (s.to_string(), false), // s has <= max chars
    }
}

/// Build the JSON bundle from an already-selected node set.
///
/// Pure: all store access is injected. `source_of` yields the exact byte slice for a node (the
/// CLI passes `store.symbol_source`), `sha_of` yields a file's content hash (the CLI passes
/// `store.file_git_sha`). Returns the `{ "nodes": [...], "summary": {...} }` object described in
/// the spec.
///
/// `selector` is the already-built JSON descriptor of how the nodes were chosen (e.g.
/// `{"file": "src/a.rs"}`); it is echoed verbatim into `summary.selector`. `requested` is the
/// count the selector resolved to *before* any shaping — it always equals `returned` here because
/// a node is never dropped, only its body; we surface both so a future "drop" policy stays
/// observable.
pub fn build_bundle<S, H>(
    mut nodes: Vec<Node>,
    selector: Value,
    opts: BudgetOpts,
    source_of: S,
    sha_of: H,
) -> Value
where
    S: Fn(&Node) -> Option<String>,
    H: Fn(&str) -> Option<String>,
{
    // Deterministic fill order: (file, start_byte). Same selection truncates identically every run.
    nodes.sort_by(|a, b| {
        a.location
            .file
            .cmp(&b.location.file)
            .then_with(|| a.location.span.start_byte.cmp(&b.location.span.start_byte))
            .then_with(|| a.symbol.0.cmp(&b.symbol.0))
    });

    let requested = nodes.len();
    let max_total = BudgetOpts::norm(opts.max_total_chars);
    let max_node = BudgetOpts::norm(opts.max_node_chars);

    let mut total_source_chars = 0usize;
    let mut truncated_count = 0usize;
    let mut out_nodes: Vec<Value> = Vec::with_capacity(nodes.len());

    for n in &nodes {
        let span = &n.location.span;
        let blob_sha = sha_of(&n.location.file);

        // Decide this node's `source` + `source_truncated` under the budget.
        let (source, truncated): (Option<String>, bool) = if opts.signatures_only {
            // Bodies omitted entirely; metadata kept.
            (None, false)
        } else {
            match source_of(n) {
                // No stored source for this node (zero-span synthetic, or content not indexed).
                None => (None, false),
                Some(body) => {
                    // Remaining room under the total budget (if any).
                    let total_room = max_total.map(|cap| cap.saturating_sub(total_source_chars));
                    if total_room == Some(0) {
                        // Total budget exhausted → drop the BODY (not the node).
                        (None, true)
                    } else {
                        // Effective per-node cap = min(per-node cap, remaining total room).
                        let cap = match (max_node, total_room) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (Some(a), None) => Some(a),
                            (None, Some(b)) => Some(b),
                            (None, None) => None,
                        };
                        match cap {
                            Some(cap) => {
                                let (prefix, was_cut) = truncate_chars(&body, cap);
                                total_source_chars += prefix.chars().count();
                                (Some(prefix), was_cut)
                            }
                            None => {
                                total_source_chars += body.chars().count();
                                (Some(body), false)
                            }
                        }
                    }
                }
            }
        };

        if truncated {
            truncated_count += 1;
        }

        out_nodes.push(json!({
            "symbol_id": n.symbol.0,
            "name": n.name,
            "kind": format!("{:?}", n.kind),
            "language": n.language.as_str(),
            "file": n.location.file,
            "line_1based": span.start_line + 1,
            "end_line_1based": span.end_line + 1,
            "byte_range": [span.start_byte, span.end_byte],
            "blob_sha": blob_sha,
            "signature": n.signature,
            "doc": n.doc,
            "source": source,
            "source_truncated": truncated,
        }));
    }

    json!({
        "nodes": out_nodes,
        "summary": {
            "selector": selector,
            "requested": requested,
            "returned": out_nodes.len(),
            "total_source_chars": total_source_chars,
            "truncated_count": truncated_count,
            "budget": {
                "max_total_chars": max_total,
                "max_node_chars": max_node,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, SymbolId,
    };
    use wicked_estate_store::MemStore;

    /// A node whose span covers `[start_byte, end_byte)` of `file`, on a given 0-based start line.
    fn node_at(
        id: &str,
        name: &str,
        file: &str,
        start_byte: u32,
        end_byte: u32,
        start_line: u32,
        end_line: u32,
    ) -> Node {
        let span = Span {
            start_byte,
            end_byte,
            start_line,
            start_col: 0,
            end_line,
            end_col: 0,
        };
        let mut n = Node::new(
            SymbolId(id.to_string()),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new(file, span),
        );
        n.signature = Some(format!("fn {name}()"));
        n
    }

    /// Two files, three functions. `a.rs` holds `alpha` (0..18) and `beta` (19..36); `b.rs`
    /// holds `gamma` (0..18). Bodies are 18 chars each so budgets are easy to reason about.
    fn fixture() -> (MemStore, Vec<Node>) {
        let a_src = "fn alpha() { 1 }\n\nfn beta() { 22 }\n"; // alpha 0..16, beta 18..34
        let b_src = "fn gamma() { 333 }\n";

        // Compute exact byte ranges from the source so symbol_source slices cleanly.
        let alpha_end = "fn alpha() { 1 }".len() as u32; // 16
        let beta_start = a_src.find("fn beta").unwrap() as u32; // 18
        let beta_end = beta_start + "fn beta() { 22 }".len() as u32; // 34
        let gamma_end = "fn gamma() { 333 }".len() as u32; // 18

        let alpha = node_at("sym::alpha", "alpha", "src/a.rs", 0, alpha_end, 0, 0);
        let beta = node_at("sym::beta", "beta", "src/a.rs", beta_start, beta_end, 2, 2);
        let gamma = node_at("sym::gamma", "gamma", "src/b.rs", 0, gamma_end, 0, 0);

        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[alpha.clone(), beta.clone(), gamma.clone()])
            .unwrap();
        store.commit_batch().unwrap();
        store.set_file_content("src/a.rs", a_src).unwrap();
        store.set_file_content("src/b.rs", b_src).unwrap();

        (store, vec![alpha, beta, gamma])
    }

    /// Wrap a store as the (source_of, sha_of) closures the CLI passes.
    fn bundle(store: &MemStore, nodes: Vec<Node>, sel: Value, opts: BudgetOpts) -> Value {
        build_bundle(
            nodes,
            sel,
            opts,
            |n| store.symbol_source(n).unwrap(),
            |f| store.file_git_sha(f).unwrap(),
        )
    }

    // 1. Bodies full by default (no budget) — every node has full `source`, not truncated.
    #[test]
    fn full_bodies_by_default() {
        let (store, nodes) = fixture();
        let b = bundle(
            &store,
            nodes,
            json!({"file": "src/a.rs"}),
            BudgetOpts::default(),
        );

        let out = b["nodes"].as_array().unwrap();
        assert_eq!(out.len(), 3, "all three nodes returned");
        for nd in out {
            assert!(nd["source"].is_string(), "every node has full source: {nd}");
            assert_eq!(nd["source_truncated"], json!(false));
        }
        // Sorted (file, start_byte): a.rs/alpha(0), a.rs/beta(18), b.rs/gamma(0).
        assert_eq!(out[0]["source"], json!("fn alpha() { 1 }"));
        assert_eq!(out[1]["source"], json!("fn beta() { 22 }"));
        assert_eq!(out[2]["source"], json!("fn gamma() { 333 }"));
        assert_eq!(b["summary"]["truncated_count"], json!(0));
        assert_eq!(b["summary"]["returned"], json!(3));
        assert_eq!(b["summary"]["budget"]["max_total_chars"], Value::Null);
        assert_eq!(b["summary"]["budget"]["max_node_chars"], Value::Null);
    }

    // 2. --max-total-chars over a larger selection → total ≤ cap, some truncated, NONE dropped.
    #[test]
    fn total_budget_caps_without_dropping() {
        let (store, nodes) = fixture();
        // 3 bodies of 16/16/18 chars = 50 total. Cap at 20 → first body fits, the rest are cut /
        // body-dropped, but all three nodes remain with full metadata.
        let opts = BudgetOpts {
            max_total_chars: Some(20),
            ..Default::default()
        };
        let b = bundle(&store, nodes, json!({"file": "all"}), opts);

        assert_eq!(b["summary"]["requested"], json!(3));
        assert_eq!(b["summary"]["returned"], json!(3), "no node dropped");
        let total = b["summary"]["total_source_chars"].as_u64().unwrap();
        assert!(total <= 20, "total {total} must be ≤ cap 20");
        assert!(
            b["summary"]["truncated_count"].as_u64().unwrap() > 0,
            "something must be truncated under a tight budget"
        );

        // Every node — including body-dropped ones — keeps full metadata.
        for nd in b["nodes"].as_array().unwrap() {
            assert!(nd["symbol_id"].is_string());
            assert!(nd["byte_range"].is_array());
            assert!(nd["signature"].is_string());
            assert!(nd["line_1based"].is_number());
            // A truncated node either has a prefix string or null, and is flagged.
            if nd["source_truncated"] == json!(true) {
                assert!(nd["source"].is_string() || nd["source"].is_null());
            }
        }
        // Over-budget tail must be source:null + truncated:true (body dropped, not the node).
        let last = b["nodes"].as_array().unwrap().last().unwrap();
        assert_eq!(last["source"], Value::Null);
        assert_eq!(last["source_truncated"], json!(true));
    }

    // 3. Metadata always present even when source is null/truncated.
    #[test]
    fn metadata_present_when_body_absent() {
        let (store, nodes) = fixture();
        let opts = BudgetOpts {
            max_node_chars: Some(4), // every body gets cut to 4 chars
            ..Default::default()
        };
        let b = bundle(&store, nodes, json!({"file": "all"}), opts);
        for nd in b["nodes"].as_array().unwrap() {
            assert!(nd["symbol_id"].is_string(), "symbol_id present");
            assert_eq!(nd["byte_range"].as_array().unwrap().len(), 2);
            assert!(
                nd["blob_sha"].is_string(),
                "blob_sha present (file has one)"
            );
            assert!(nd["line_1based"].is_number(), "line_1based present");
            assert_eq!(nd["source_truncated"], json!(true));
            assert_eq!(nd["source"].as_str().unwrap().chars().count(), 4);
        }
    }

    // 4. --signatures-only → all source null, metadata present, nothing flagged truncated.
    #[test]
    fn signatures_only_omits_bodies() {
        let (store, nodes) = fixture();
        let opts = BudgetOpts {
            signatures_only: true,
            ..Default::default()
        };
        let b = bundle(&store, nodes, json!({"file": "all"}), opts);
        for nd in b["nodes"].as_array().unwrap() {
            assert_eq!(nd["source"], Value::Null, "body omitted");
            assert!(nd["signature"].is_string(), "metadata kept");
            assert_eq!(nd["source_truncated"], json!(false), "not a truncation");
        }
        assert_eq!(b["summary"]["total_source_chars"], json!(0));
        assert_eq!(b["summary"]["truncated_count"], json!(0));
    }

    // 5. Determinism — same call twice → byte-identical JSON.
    #[test]
    fn deterministic_output() {
        let (store, nodes) = fixture();
        let opts = BudgetOpts {
            max_total_chars: Some(25),
            ..Default::default()
        };
        let one = bundle(&store, nodes.clone(), json!({"file": "all"}), opts);
        let two = bundle(&store, nodes, json!({"file": "all"}), opts);
        assert_eq!(
            serde_json::to_string(&one).unwrap(),
            serde_json::to_string(&two).unwrap(),
            "same db + selector + budget → byte-identical JSON"
        );
    }

    // 6. --symbols precision — two same-named symbols selected by id → exactly those two.
    #[test]
    fn symbols_selector_is_precise() {
        // Two distinct symbols both named "dup" — a fuzzy name match would over-select.
        let src = "fn dup() { 1 }\nfn dup() { 2 }\n";
        let d1 = node_at("sym::dup1", "dup", "src/d.rs", 0, 14, 0, 0);
        let d2 = node_at("sym::dup2", "dup", "src/d.rs", 15, 29, 1, 1);
        let other = node_at("sym::dup3", "dup", "src/e.rs", 0, 14, 0, 0);

        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[d1.clone(), d2.clone(), other.clone()])
            .unwrap();
        store.commit_batch().unwrap();
        store.set_file_content("src/d.rs", src).unwrap();
        store
            .set_file_content("src/e.rs", "fn dup() { 9 }\n")
            .unwrap();

        // Caller selects exactly two of the three "dup" symbols by id.
        let selected: Vec<Node> = vec![d1, d2];
        let b = bundle(
            &store,
            selected,
            json!({"symbols": ["sym::dup1", "sym::dup2"]}),
            BudgetOpts::default(),
        );
        let ids: Vec<&str> = b["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["symbol_id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["sym::dup1", "sym::dup2"],
            "exactly the two selected ids"
        );
        assert_eq!(b["summary"]["returned"], json!(2));
    }

    // 7. Read-only — building a bundle does not mutate the store.
    #[test]
    fn build_is_read_only() {
        let (store, nodes) = fixture();
        let before = store.all_nodes().unwrap().len();
        let before_a = store.file_content("src/a.rs").unwrap();

        let _ = bundle(
            &store,
            nodes,
            json!({"file": "src/a.rs"}),
            BudgetOpts {
                max_total_chars: Some(10),
                ..Default::default()
            },
        );

        assert_eq!(
            store.all_nodes().unwrap().len(),
            before,
            "node count unchanged"
        );
        assert_eq!(
            store.file_content("src/a.rs").unwrap(),
            before_a,
            "content unchanged"
        );
    }
}
