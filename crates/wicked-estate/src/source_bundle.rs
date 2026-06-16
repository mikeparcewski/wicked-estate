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
use std::collections::BTreeMap;
use wicked_estate_core::{Annotation, Node, is_advisory};

/// R4 payload cap: at most this many annotations are inlined per entity in a structured payload
/// (`nodes --json`, `source --json`, `RetrieveEntity`). `annotation_summary.count` always carries
/// the TRUE total so a consumer can tell it was capped (the spec's "summary is always exact" rule).
pub const MAX_ANNOTATIONS_PER_ENTITY: usize = 20;

/// Render a single annotation as the spec's payload object:
/// `{type, key, value, confidence, provenance, author, ts, advisory}`. `advisory` is computed from
/// the `type` via [`is_advisory`] (assumption / question) — a consumer gates "is this a fact?" off
/// this field, never the type string (spec build-ahead note 1).
pub fn annotation_json(a: &Annotation) -> Value {
    json!({
        "type": a.r#type,
        "key": a.key,
        "value": a.value,
        "confidence": a.confidence,
        "provenance": a.provenance,
        "author": a.author,
        "ts": a.ts,
        "advisory": is_advisory(&a.r#type),
    })
}

/// Apply the R4 cap to a symbol's annotations for inlining in a payload.
///
/// Ordering: **advisory-class (`assumption`/`question`) first, then the rest by recency (`ts`
/// desc)**, then truncated to [`MAX_ANNOTATIONS_PER_ENTITY`]. This keeps the trust-relevant rows
/// (the ones a consumer must surface as not-a-fact, R7) when an entity exceeds the cap. The input
/// is taken by value and reordered in place; the TRUE total is captured by the caller via
/// [`annotation_summary`] BEFORE capping. Sort is stable so equal-`ts` rows keep their relative
/// (insertion / oldest-first) order from the store.
pub fn cap_annotations_for_payload(mut anns: Vec<Annotation>) -> Vec<Annotation> {
    anns.sort_by(|a, b| {
        let adv = is_advisory(&b.r#type).cmp(&is_advisory(&a.r#type)); // advisory (true) first
        adv.then_with(|| b.ts.cmp(&a.ts)) // then newest first
    });
    anns.truncate(MAX_ANNOTATIONS_PER_ENTITY);
    anns
}

/// Build the `annotation_summary` object — `{count, by_type, has_advisory}` — over the FULL
/// annotation set (before any R4 cap). `count` is the true total; `by_type` is a per-`type` tally
/// (deterministic key order via `BTreeMap`); `has_advisory` is true when any annotation is
/// advisory-class. This is the cheap-triage field a consumer reads instead of pulling every value
/// (spec build-ahead note 2).
pub fn annotation_summary(anns: &[Annotation]) -> Value {
    let mut by_type: BTreeMap<&str, u64> = BTreeMap::new();
    let mut has_advisory = false;
    for a in anns {
        *by_type.entry(a.r#type.as_str()).or_insert(0) += 1;
        if is_advisory(&a.r#type) {
            has_advisory = true;
        }
    }
    json!({
        "count": anns.len(),
        "by_type": by_type,
        "has_advisory": has_advisory,
    })
}

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
///
/// `annotations_of` yields a node's FULL typed-annotation set (the CLI passes
/// `store.annotations`). Each node gains `annotation_summary` (always exact, over the full set)
/// and, when non-empty, an `annotations` array capped per the R4 rule
/// ([`cap_annotations_for_payload`]) — advisory-class first, then `ts` desc, ≤ 20.
pub fn build_bundle<S, H, A>(
    mut nodes: Vec<Node>,
    selector: Value,
    opts: BudgetOpts,
    source_of: S,
    sha_of: H,
    annotations_of: A,
) -> Value
where
    S: Fn(&Node) -> Option<String>,
    H: Fn(&str) -> Option<String>,
    A: Fn(&Node) -> Vec<Annotation>,
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

        // Typed annotations: summary is exact over the FULL set; the inlined array is R4-capped
        // (advisory-first, then ts desc, ≤ 20) and omitted entirely when the symbol has none.
        let all_anns = annotations_of(n);
        let summary = annotation_summary(&all_anns);
        let mut node_obj = json!({
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
            "annotation_summary": summary,
        });
        if !all_anns.is_empty() {
            let capped: Vec<Value> = cap_annotations_for_payload(all_anns)
                .iter()
                .map(annotation_json)
                .collect();
            node_obj["annotations"] = Value::Array(capped);
        }
        out_nodes.push(node_obj);
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
        Annotation, GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, SymbolId,
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

    /// Wrap a store as the (source_of, sha_of, annotations_of) closures the CLI passes.
    fn bundle(store: &MemStore, nodes: Vec<Node>, sel: Value, opts: BudgetOpts) -> Value {
        build_bundle(
            nodes,
            sel,
            opts,
            |n| store.symbol_source(n).unwrap(),
            |f| store.file_git_sha(f).unwrap(),
            |n| store.annotations(&n.symbol).unwrap(),
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
        let before_anns_sym = SymbolId("sym::alpha".to_string());

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
        // annotations is a read-only fetch — building a bundle must not write any.
        assert!(
            store.annotations(&before_anns_sym).unwrap().is_empty(),
            "no annotations written by bundle build"
        );
        assert_eq!(
            store.file_content("src/a.rs").unwrap(),
            before_a,
            "content unchanged"
        );
    }

    // 8. annotation_json shape: every spec field present; `advisory` computed from `type`.
    #[test]
    fn annotation_json_carries_advisory_flag() {
        let assume = Annotation::new("assumption", "k", "v")
            .with_confidence(0.7)
            .with_provenance("manual")
            .with_author("alice");
        let j = annotation_json(&assume);
        assert_eq!(j["type"], json!("assumption"));
        assert_eq!(j["key"], json!("k"));
        assert_eq!(j["value"], json!("v"));
        assert_eq!(j["confidence"], json!(0.7));
        assert_eq!(j["provenance"], json!("manual"));
        assert_eq!(j["author"], json!("alice"));
        assert_eq!(j["advisory"], json!(true), "assumption is advisory");

        // A note is NOT advisory; a custom type is NOT advisory.
        assert_eq!(
            annotation_json(&Annotation::note("k", "v"))["advisory"],
            json!(false)
        );
        assert_eq!(
            annotation_json(&Annotation::new("adr-ref", "k", "v"))["advisory"],
            json!(false),
            "custom type is not advisory"
        );
        // A question IS advisory.
        assert_eq!(
            annotation_json(&Annotation::new("question", "k", "v"))["advisory"],
            json!(true)
        );
    }

    // 9. annotation_summary: exact count, per-type tally, has_advisory.
    #[test]
    fn annotation_summary_is_exact() {
        let anns = vec![
            Annotation::note("a", "1"),
            Annotation::note("b", "2"),
            Annotation::new("assumption", "c", "3"),
        ];
        let s = annotation_summary(&anns);
        assert_eq!(s["count"], json!(3), "count is the true total");
        assert_eq!(s["by_type"]["note"], json!(2));
        assert_eq!(s["by_type"]["assumption"], json!(1));
        assert_eq!(s["has_advisory"], json!(true), "an assumption is present");

        let none = annotation_summary(&[]);
        assert_eq!(none["count"], json!(0));
        assert_eq!(none["has_advisory"], json!(false));
    }

    // 10. R4 cap: advisory-class first, then ts desc, truncated to 20; summary count stays TRUE.
    #[test]
    fn cap_orders_advisory_first_then_ts_desc_and_truncates() {
        // 25 annotations: indices 0..25, ts = index. Make a few advisory at LOW ts so the
        // ordering rule (advisory-first) is observable distinct from pure recency.
        let mut anns: Vec<Annotation> = Vec::new();
        for i in 0..25i64 {
            // ts = i; types: i==0,1 are questions (advisory, oldest); rest are notes.
            let ty = if i < 2 { "question" } else { "note" };
            let mut a = Annotation::new(ty, format!("k{i}"), format!("v{i}"));
            a.ts = i;
            anns.push(a);
        }
        let total = anns.len();
        let summary = annotation_summary(&anns);
        assert_eq!(
            summary["count"],
            json!(total as u64),
            "summary is true total (25)"
        );

        let capped = cap_annotations_for_payload(anns);
        assert_eq!(capped.len(), MAX_ANNOTATIONS_PER_ENTITY, "capped to 20");
        // The two advisory (question, ts 0 and 1) must survive the cap despite being the OLDEST,
        // and appear FIRST. Among the two advisory rows, ts desc → ts=1 before ts=0.
        assert!(capped[0].is_advisory(), "first row is advisory");
        assert!(capped[1].is_advisory(), "second row is advisory");
        assert_eq!(
            capped[0].ts, 1,
            "advisory ordered ts desc within advisory class"
        );
        assert_eq!(capped[1].ts, 0);
        // After the two advisory rows, the remaining 18 are the NEWEST notes (ts 24..7), ts desc.
        assert_eq!(capped[2].ts, 24, "newest note after advisory rows");
        assert!(
            capped.iter().all(|a| a.is_advisory() || a.ts >= 7),
            "the oldest notes (ts 2..6) were dropped by the cap"
        );
    }

    // 11. End-to-end: a node WITH annotations gets `annotations` (capped) + `annotation_summary`;
    //     a node WITHOUT annotations omits `annotations` but still carries the summary (count 0).
    #[test]
    fn bundle_inlines_annotations_and_summary() {
        let (mut store, nodes) = fixture();
        // Annotate alpha with 1 note + 1 assumption; leave beta/gamma bare.
        store
            .annotate(
                &SymbolId("sym::alpha".to_string()),
                Annotation::note("owner", "team-x"),
            )
            .unwrap();
        store
            .annotate(
                &SymbolId("sym::alpha".to_string()),
                Annotation::new("assumption", "thread-safe", "assumed"),
            )
            .unwrap();

        let b = bundle(&store, nodes, json!({"file": "all"}), BudgetOpts::default());
        let out = b["nodes"].as_array().unwrap();

        // alpha sorts first (src/a.rs, byte 0).
        let alpha = &out[0];
        assert_eq!(alpha["symbol_id"], json!("sym::alpha"));
        assert_eq!(alpha["annotation_summary"]["count"], json!(2));
        assert_eq!(alpha["annotation_summary"]["has_advisory"], json!(true));
        let alpha_anns = alpha["annotations"]
            .as_array()
            .expect("annotations present");
        assert_eq!(alpha_anns.len(), 2);
        // Advisory (assumption) must be inlined first.
        assert_eq!(alpha_anns[0]["type"], json!("assumption"));
        assert_eq!(alpha_anns[0]["advisory"], json!(true));

        // beta (src/a.rs byte 18) has NO annotations: summary present (count 0), array omitted.
        let beta = &out[1];
        assert_eq!(beta["symbol_id"], json!("sym::beta"));
        assert_eq!(beta["annotation_summary"]["count"], json!(0));
        assert!(
            beta.get("annotations").is_none(),
            "annotations array omitted when empty"
        );
    }
}
