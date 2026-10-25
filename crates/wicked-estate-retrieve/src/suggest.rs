//! Did-you-mean suggestions for a symbol-name miss (wicked-estate#358).
//!
//! Every name-addressed entry point resolves by exact, case-sensitive name. A wrong casing, a
//! qualified form (`OrderService.create`) or a typo returns nothing — and before #358 that empty
//! answer read like "nothing depends on this". [`suggest`] ranks candidates for the miss so the
//! caller can say `resolved: false` **and** offer the symbol the agent most likely meant. A
//! suggestion is a CANDIDATE: no answer is ever computed from one (R7); the caller re-queries by
//! symbol id.
//!
//! Candidates come only from [`GraphRead::find_symbols`] with [`SymbolQuery::name_prefix`] — at
//! most three calls per miss, never one per candidate:
//!
//! 1. `name_prefix = probe` — serves tier 1 (case-insensitive exact) and tier 3 (prefix).
//! 2. `name_prefix = last segment` — tier 2, only for a qualified probe (`a.b`, `a::b`, `a#b`,
//!    `a/b`): the last segment matches the name case-insensitively and every earlier segment
//!    appears, case-folded and in order, in the candidate's `SymbolId`.
//! 3. `name_prefix = first char` (bounded by [`SUGGEST_POOL_MAX`]) — tier 4: Damerau-Levenshtein
//!    distance ≤ `max(1, len/4)`, case-folded.
//!
//! Tiers are ranked in that order (case-insensitive, qualified segment, prefix, edit): every hit
//! of an earlier tier outranks every hit of a later one. Within a tier: distance, then name, then
//! symbol id — so the same graph and probe always give the same order. Synthetic value-flow slots
//! are excluded (the wicked-estate#207 lesson): a bare name never suggests a local slot.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use wicked_estate_core::{GraphRead, Node, Result, SymbolQuery, is_structural_symbol};

/// Default number of suggestions returned on a miss (R4: one row is ~200 chars).
pub const SUGGEST_K_DEFAULT: usize = 5;
/// Hard ceiling on suggestions per miss.
pub const SUGGEST_K_MAX: usize = 10;
/// Cap on any one candidate pool fetched from the store.
pub const SUGGEST_POOL_MAX: usize = 2_000;
/// A probe longer than this is no symbol name anyone mistyped; it gets no suggestions (and the
/// edit-distance tier stays bounded). Echoes of the probe are cut to [`ECHO_MAX_CHARS`].
pub const PROBE_MAX_CHARS: usize = 256;
/// The longest echo of a probe in a reply or diagnostic (R4: a 30 000-char id must not be
/// echoed back whole).
pub const ECHO_MAX_CHARS: usize = 200;

/// `probe` cut to [`ECHO_MAX_CHARS`] for an echo.
pub fn echo(probe: &str) -> String {
    if probe.chars().count() > ECHO_MAX_CHARS {
        format!(
            "{}…",
            probe.chars().take(ECHO_MAX_CHARS).collect::<String>()
        )
    } else {
        probe.to_string()
    }
}

/// Why a candidate was suggested. Serialized snake_case on the wire (`match_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchReason {
    CaseInsensitive,
    QualifiedSegment,
    Prefix,
    Edit,
}

impl MatchReason {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchReason::CaseInsensitive => "case_insensitive",
            MatchReason::QualifiedSegment => "qualified_segment",
            MatchReason::Prefix => "prefix",
            MatchReason::Edit => "edit",
        }
    }
    fn label(self) -> &'static str {
        match self {
            MatchReason::CaseInsensitive => "case-insensitive",
            MatchReason::QualifiedSegment => "qualified segment",
            MatchReason::Prefix => "prefix",
            MatchReason::Edit => "edit",
        }
    }
}

/// One ranked candidate for a missed name.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub node: Node,
    pub reason: MatchReason,
    /// Case-folded edit distance between the probe (or its last segment) and the name.
    pub distance: u32,
}

/// The ranked suggestions for `probe` (up to `k`, clamped to [`SUGGEST_K_MAX`]) plus whether more
/// qualified than were returned.
pub fn suggest(store: &dyn GraphRead, probe: &str, k: usize) -> Result<(Vec<Suggestion>, bool)> {
    let k = k.clamp(1, SUGGEST_K_MAX);
    let probe = probe.trim();
    if probe.is_empty() || probe.chars().count() > PROBE_MAX_CHARS {
        return Ok((vec![], false));
    }
    let folded = probe.to_ascii_lowercase();
    let segments = split_qualified(probe);
    let last = segments.last().copied().unwrap_or(probe);
    let last_folded = last.to_ascii_lowercase();

    let query = |prefix: &str| SymbolQuery {
        name_prefix: Some(prefix.to_string()),
        limit: Some(SUGGEST_POOL_MAX),
        ..Default::default()
    };
    // Tiers 1–3 (a whole probe or segment as the prefix — a narrow set): value-flow slots are
    // excluded BEFORE the cap, so slots can never crowd a real symbol out of the pool.
    let fetch =
        |prefix: &str| -> Result<Vec<Node>> { store.find_structural_symbols(&query(prefix)) };

    let mut ranked: Vec<Suggestion> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |ranked: &mut Vec<Suggestion>, node: &Node, reason, distance| {
        if seen.insert(node.symbol.0.clone()) {
            ranked.push(Suggestion {
                node: node.clone(),
                reason,
                distance,
            });
        }
    };

    // Call 1: tiers 1 + 3.
    let by_probe = fetch(probe)?;
    for n in &by_probe {
        if n.name.eq_ignore_ascii_case(probe) {
            push(&mut ranked, n, MatchReason::CaseInsensitive, 0);
        }
    }
    // Call 2: tier 2, qualified probes only.
    if segments.len() > 1 && !last.is_empty() {
        let earlier: Vec<String> = segments[..segments.len() - 1]
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        for n in &fetch(last)? {
            if n.name.eq_ignore_ascii_case(last) && segments_in_order(&n.symbol.0, last, &earlier) {
                push(&mut ranked, n, MatchReason::QualifiedSegment, 0);
            }
        }
    }
    let probe_len = probe.chars().count();
    for n in &by_probe {
        // `name_prefix` guarantees `name` starts with `probe` (case-folded): distance = extra chars.
        let d = n.name.chars().count().saturating_sub(probe_len) as u32;
        push(&mut ranked, n, MatchReason::Prefix, d);
    }
    // Call 3 (+ call 2's slot when the probe is unqualified): tier 4, edit distance over a
    // bounded pool. The pool shares the target's first character, so an edit anywhere but the
    // first character is in reach. When that pool hits the cap (a common first letter on a large
    // graph) the two-character pool is fetched too, so the nearest names are never cut by the cap.
    let target = if segments.len() > 1 {
        last_folded.as_str()
    } else {
        folded.as_str()
    };
    let max_d = (target.chars().count() / 4).max(1) as u32;
    let mut pools: Vec<Vec<Node>> = Vec::new();
    if let Some(first) = target.chars().next() {
        // The one-character pool is the wide one: fetched capped, and whether the CAP bit is
        // read from the raw rows (slots included) — filtering first would hide a full window.
        let raw = store.find_symbols(&query(&first.to_string()))?;
        let capped = raw.len() >= SUGGEST_POOL_MAX;
        pools.push(raw.into_iter().filter(is_structural_symbol).collect());
        let two: String = target.chars().take(2).collect();
        if capped && segments.len() <= 1 && two.chars().count() == 2 {
            pools.push(fetch(&two)?);
        }
    }
    for n in pools.iter().flatten() {
        let d = damerau_levenshtein(target, &n.name.to_ascii_lowercase());
        if d <= max_d {
            push(&mut ranked, n, MatchReason::Edit, d);
        }
    }

    ranked.sort_by(|a, b| {
        (a.reason, a.distance, &a.node.name, &a.node.symbol.0).cmp(&(
            b.reason,
            b.distance,
            &b.node.name,
            &b.node.symbol.0,
        ))
    });
    let truncated = ranked.len() > k;
    ranked.truncate(k);
    Ok((ranked, truncated))
}

/// Split a qualified probe on the separators symbol ids already use (`::`, `.`, `#`, `/`).
/// Empty segments (a trailing `.`, a leading `::`) are dropped.
fn split_qualified(probe: &str) -> Vec<&str> {
    probe
        .split("::")
        .flat_map(|p| p.split(['.', '#', '/']))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Every earlier (case-folded) segment appears in `symbol_id`'s QUALIFIER — the text before the
/// last occurrence of the leaf name — case-folded and in order. Searching the whole id would let
/// `order` match inside `#cancelOrder().` and rank `InvoiceService#cancelOrder` for
/// `Order.cancelOrder` (agy review of #367).
fn segments_in_order(symbol_id: &str, leaf: &str, earlier: &[String]) -> bool {
    let full = symbol_id.to_ascii_lowercase();
    let cut = full.rfind(&leaf.to_ascii_lowercase()).unwrap_or(full.len());
    let hay = &full[..cut];
    let mut from = 0usize;
    for seg in earlier {
        match hay[from..].find(seg.as_str()) {
            Some(i) => from += i + seg.len(),
            None => return false,
        }
    }
    true
}

/// Optimal-string-alignment Damerau-Levenshtein distance over chars.
fn damerau_levenshtein(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m as u32;
    }
    if m == 0 {
        return n as u32;
    }
    let mut d = vec![vec![0u32; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i as u32;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j as u32;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = u32::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

/// The `suggestions` rows for a miss: `{symbol, name, kind, file, line_1based, match_reason,
/// distance}` — one shape for MCP and CLI `--json`.
pub fn suggestions_json(suggestions: &[Suggestion]) -> Value {
    Value::Array(
        suggestions
            .iter()
            .map(|s| {
                json!({
                    "symbol": s.node.symbol.as_str(),
                    "name": s.node.name,
                    "kind": serde_json::to_value(&s.node.kind).unwrap_or(Value::Null),
                    "file": s.node.location.file,
                    "line_1based": s.node.location.span.start_line + 1,
                    "match_reason": s.reason.as_str(),
                    "distance": s.distance,
                })
            })
            .collect(),
    )
}

/// The miss keys a name-addressed reply carries (merged into the tool's usual keys, which stay):
/// `resolved: false`, the `query`, the ranked `suggestions` and `suggestions_truncated`.
pub fn miss_fields(probe: &str, suggestions: &[Suggestion], truncated: bool) -> Value {
    json!({
        "resolved": false,
        "query": echo(probe),
        "suggestions": suggestions_json(suggestions),
        "suggestions_truncated": truncated,
    })
}

/// Merge [`miss_fields`] into a reply's `content` object (a no-op for a non-object).
pub fn mark_miss(content: &mut Value, probe: &str, suggestions: &[Suggestion], truncated: bool) {
    if let (Some(obj), Value::Object(extra)) = (
        content.as_object_mut(),
        miss_fields(probe, suggestions, truncated),
    ) {
        obj.extend(extra);
    }
}

/// The one `NAME-MISS:` diagnostic every miss site emits.
pub fn name_miss_diagnostic(probe: &str, suggestions: &[Suggestion]) -> String {
    let probe = echo(probe);
    match suggestions.first() {
        Some(s) => format!(
            "NAME-MISS: '{probe}' resolved to no symbol; did you mean {} ({}:{})? Re-query with \
             the symbol id.",
            s.node.symbol.as_str(),
            s.node.location.file,
            s.node.location.span.start_line + 1
        ),
        None => format!(
            "NAME-MISS: '{probe}' resolved to no symbol and no close candidate was found; this is \
             an unresolved name, not an empty answer."
        ),
    }
}

/// The human (CLI text) rendering of a miss — printed INSTEAD of an empty-answer line.
pub fn miss_text(probe: &str, suggestions: &[Suggestion]) -> Vec<String> {
    let probe = echo(probe);
    if suggestions.is_empty() {
        return vec![format!(
            "no symbol named '{probe}', and no close candidate was found"
        )];
    }
    let mut lines = vec![format!("no symbol named '{probe}' — did you mean:")];
    for s in suggestions {
        lines.push(format!(
            "  {:?}  {}  ({}:{})   [{}]",
            s.node.kind,
            s.node.symbol.as_str(),
            s.node.location.file,
            s.node.location.span.start_line + 1,
            s.reason.label()
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        Descriptor, GraphWrite, Language, Location, NodeKind, Span, Suffix, Symbol,
        VALUE_ROLE_METADATA_KEY,
    };
    use wicked_estate_store::{MemStore, SqliteStore};

    fn method(class: &str, name: &str, file: &str) -> Node {
        let id = Symbol::global(
            "scip-ts",
            None,
            vec![
                Descriptor::new("src", Suffix::Namespace),
                Descriptor::new(class, Suffix::Type),
                Descriptor::method(name, None),
            ],
        )
        .id();
        Node::new(
            id,
            NodeKind::Method,
            name,
            Language::new("typescript"),
            Location::new(file, Span::ZERO),
        )
    }

    fn class(name: &str, file: &str) -> Node {
        let id = Symbol::global(
            "scip-ts",
            None,
            vec![
                Descriptor::new("src", Suffix::Namespace),
                Descriptor::new(name, Suffix::Type),
            ],
        )
        .id();
        Node::new(
            id,
            NodeKind::Class,
            name,
            Language::new("typescript"),
            Location::new(file, Span::ZERO),
        )
    }

    /// A value-flow slot with a bare source name — must never be suggested (#207 lesson).
    fn value_slot(name: &str) -> Node {
        let mut n = Node::new(
            wicked_estate_core::SymbolId(format!("slot:{name}")),
            NodeKind::Variable,
            name,
            Language::new("typescript"),
            Location::new("src/order.ts", Span::ZERO),
        );
        n.metadata
            .insert(VALUE_ROLE_METADATA_KEY.into(), serde_json::json!("local"));
        n
    }

    fn fixture(store: &mut dyn wicked_estate_core::GraphStore) {
        GraphWrite::begin_batch(store).unwrap();
        GraphWrite::upsert_nodes(
            store,
            &[
                class("OrderService", "src/order.ts"),
                method("OrderService", "create", "src/order.ts"),
                method("InvoiceService", "create", "src/invoice.ts"),
                method("OrderService", "cancelOrder", "src/order.ts"),
                method("OrderService", "createOrderLine", "src/order.ts"),
                class("OrderServiceFactory", "src/factory.ts"),
                value_slot("orderService"),
                value_slot("createOrdr"),
            ],
        )
        .unwrap();
        GraphWrite::commit_batch(store).unwrap();
    }

    fn mem() -> MemStore {
        let mut s = MemStore::new();
        fixture(&mut s);
        s
    }

    fn names(found: &[Suggestion]) -> Vec<(String, MatchReason, u32)> {
        found
            .iter()
            .map(|s| (s.node.name.clone(), s.reason, s.distance))
            .collect()
    }

    #[test]
    fn tier1_case_insensitive_exact_ranks_first() {
        let (found, _) = suggest(&mem(), "orderservice", 5).unwrap();
        assert_eq!(found[0].node.name, "OrderService");
        assert_eq!(found[0].reason, MatchReason::CaseInsensitive);
        assert_eq!(found[0].distance, 0);
        // The longer name sharing the prefix follows as a prefix hit, never above the exact one.
        assert!(
            found
                .iter()
                .any(|s| s.node.name == "OrderServiceFactory" && s.reason == MatchReason::Prefix),
            "{:?}",
            names(&found)
        );
    }

    #[test]
    fn tier2_qualified_segment_needs_the_earlier_segments_in_the_id() {
        for probe in [
            "orderservice.create",
            "OrderService::create",
            "OrderService#create",
            "src/OrderService.create",
        ] {
            let (found, _) = suggest(&mem(), probe, 5).unwrap();
            assert_eq!(found[0].reason, MatchReason::QualifiedSegment, "{probe}");
            assert!(
                found[0]
                    .node
                    .symbol
                    .as_str()
                    .contains("OrderService#create"),
                "{probe}: {}",
                found[0].node.symbol
            );
            // InvoiceService#create shares the last segment but not the qualifier.
            assert!(
                !found
                    .iter()
                    .any(|s| s.reason == MatchReason::QualifiedSegment
                        && s.node.symbol.as_str().contains("InvoiceService")),
                "{probe}: {:?}",
                names(&found)
            );
        }
    }

    #[test]
    fn tier3_prefix_ranks_by_extra_length() {
        let (found, _) = suggest(&mem(), "createorder", 5).unwrap();
        assert_eq!(found[0].node.name, "createOrderLine");
        assert_eq!(found[0].reason, MatchReason::Prefix);
        assert_eq!(found[0].distance, 4);
    }

    #[test]
    fn tier4_edit_distance_catches_typos_but_not_the_first_letter_pool_outsiders() {
        // One transposition, one deletion.
        for (probe, want) in [
            ("cancleOrder", "cancelOrder"),
            ("cancelOrdr", "cancelOrder"),
        ] {
            let (found, _) = suggest(&mem(), probe, 5).unwrap();
            assert_eq!(found[0].node.name, want, "{probe}: {:?}", names(&found));
            assert_eq!(found[0].reason, MatchReason::Edit, "{probe}");
            assert_eq!(found[0].distance, 1, "{probe}");
        }
        // Beyond max(1, len/4) edits: no suggestion rather than a wild guess.
        let (found, _) = suggest(&mem(), "cxxxxxOrder", 5).unwrap();
        assert!(found.is_empty(), "{:?}", names(&found));
    }

    #[test]
    fn value_flow_slots_are_never_suggested() {
        for probe in ["orderService", "createOrdr", "orderservic"] {
            let (found, _) = suggest(&mem(), probe, 10).unwrap();
            assert!(
                found
                    .iter()
                    .all(|s| !s.node.symbol.as_str().starts_with("slot:")),
                "{probe}: {:?}",
                names(&found)
            );
        }
    }

    #[test]
    fn ranking_is_deterministic_and_k_is_clamped() {
        let store = mem();
        let a = suggest(&store, "orderservic", 10).unwrap();
        let b = suggest(&store, "orderservic", 10).unwrap();
        assert_eq!(a, b);
        let (found, truncated) = suggest(&store, "o", 1).unwrap();
        assert_eq!(found.len(), 1);
        assert!(truncated, "more than one candidate qualified");
        let (found, _) = suggest(&store, "o", 99).unwrap();
        assert!(found.len() <= SUGGEST_K_MAX);
        assert!(suggest(&store, "   ", 5).unwrap().0.is_empty());
    }

    /// Store parity: MemStore and SqliteStore loaded with the same fixture give the same
    /// suggestions — candidate generation is `find_symbols(name_prefix)` on both.
    #[test]
    fn mem_and_sqlite_suggest_the_same() {
        let mem = mem();
        let mut sqlite = SqliteStore::in_memory().unwrap();
        fixture(&mut sqlite);
        for probe in [
            "orderservice",
            "orderservice.create",
            "OrderService::create",
            "createorder",
            "cancleOrder",
            "nothing_like_this",
        ] {
            let m = suggest(&mem, probe, 5).unwrap();
            let s = suggest(&sqlite, probe, 5).unwrap();
            assert_eq!(names(&m.0), names(&s.0), "{probe}");
            assert_eq!(m.1, s.1, "{probe}");
        }
    }

    #[test]
    fn miss_wire_shape_and_diagnostic() {
        let (found, truncated) = suggest(&mem(), "orderservice.create", 5).unwrap();
        let mut content = serde_json::json!({ "dependents": [] });
        mark_miss(&mut content, "orderservice.create", &found, truncated);
        assert_eq!(content["resolved"], serde_json::json!(false));
        assert_eq!(content["query"], serde_json::json!("orderservice.create"));
        assert_eq!(
            content["dependents"],
            serde_json::json!([]),
            "usual keys kept"
        );
        let row = &content["suggestions"][0];
        for key in [
            "symbol",
            "name",
            "kind",
            "file",
            "line_1based",
            "match_reason",
            "distance",
        ] {
            assert!(row.get(key).is_some(), "suggestion row lacks {key}: {row}");
        }
        assert_eq!(row["match_reason"], serde_json::json!("qualified_segment"));
        let d = name_miss_diagnostic("orderservice.create", &found);
        assert!(d.starts_with("NAME-MISS: 'orderservice.create'"), "{d}");
        assert!(d.contains("OrderService#create"), "{d}");
        let none = name_miss_diagnostic("zzz", &[]);
        assert!(none.contains("not an empty answer"), "{none}");
    }

    /// codex/agy review of #367: value-flow slots must not crowd a real symbol out of a capped
    /// pool, and a full raw window still triggers the two-character fallback.
    #[test]
    fn value_flow_slots_cannot_exhaust_the_candidate_pool() {
        let mut store = MemStore::new();
        // Slots with the SAME name whose ids sort before the real symbol's: a raw capped window
        // in name-prefix order holds only slots.
        let mut nodes: Vec<Node> = (0..SUGGEST_POOL_MAX + 10)
            .map(|i| {
                let mut n = value_slot("create_orderz");
                n.symbol = wicked_estate_core::SymbolId(format!("aaa slot {i:05}"));
                n
            })
            .collect();
        nodes.push(method("OrderService", "create_orderz", "src/order.ts"));
        GraphWrite::begin_batch(&mut store).unwrap();
        GraphWrite::upsert_nodes(&mut store, &nodes).unwrap();
        GraphWrite::commit_batch(&mut store).unwrap();
        let (found, _) = suggest(&store, "Create_Orderz", 5).unwrap();
        assert_eq!(
            found.first().map(|s| (s.node.name.as_str(), s.reason)),
            Some(("create_orderz", MatchReason::CaseInsensitive)),
            "{:?}",
            names(&found)
        );
        let (found, _) = suggest(&store, "create_ordrz", 5).unwrap();
        assert!(
            found.iter().any(|s| s.node.name == "create_orderz"),
            "edit tier must reach past a slot-filled window: {:?}",
            names(&found)
        );
    }

    /// agy review of #367: the qualifier is matched BEFORE the leaf name, so `order` inside
    /// `#cancelOrder().` does not make `InvoiceService#cancelOrder` a qualified match.
    #[test]
    fn qualified_segments_match_the_qualifier_not_the_leaf() {
        let mut store = MemStore::new();
        GraphWrite::begin_batch(&mut store).unwrap();
        GraphWrite::upsert_nodes(
            &mut store,
            &[
                method("InvoiceService", "cancelOrder", "src/invoice.ts"),
                method("OrderService", "cancelOrder", "src/order.ts"),
            ],
        )
        .unwrap();
        GraphWrite::commit_batch(&mut store).unwrap();
        let (found, _) = suggest(&store, "Order.cancelOrder", 5).unwrap();
        let qualified: Vec<&str> = found
            .iter()
            .filter(|s| s.reason == MatchReason::QualifiedSegment)
            .map(|s| s.node.symbol.as_str())
            .collect();
        assert_eq!(qualified.len(), 1, "{qualified:?}");
        assert!(qualified[0].contains("/OrderService#"), "{qualified:?}");
    }

    #[test]
    fn damerau_levenshtein_counts_a_transposition_once() {
        assert_eq!(damerau_levenshtein("cancel", "cancle"), 1);
        assert_eq!(damerau_levenshtein("abc", "abc"), 0);
        assert_eq!(damerau_levenshtein("", "abc"), 3);
        assert_eq!(damerau_levenshtein("kitten", "sitting"), 3);
    }

    #[test]
    fn split_qualified_handles_every_separator() {
        assert_eq!(split_qualified("a::b.c#d/e"), vec!["a", "b", "c", "d", "e"]);
        assert_eq!(split_qualified("plain"), vec!["plain"]);
        assert_eq!(split_qualified("trailing."), vec!["trailing"]);
    }
}
