//! Estate cross-domain join — RACF security profiles → the assets they protect.
//!
//! RACF is the mainframe's access-control database. A *dataset profile* (`ADDSD`) or a
//! *general-resource profile* (`RDEFINE <class>`) names — often via a GENERIC PATTERN — the
//! assets it governs. The concrete assets (a JCL `DD DSN=`, an MQ `DEFINE QLOCAL`) are extracted
//! from OTHER files, so the link is a cross-node derivation over the whole graph, not a per-file
//! reference resolution. This mirrors [`crate::scip_edges`]: a pass over the node population that
//! emits edges, rather than a [`Resolver`](wicked_estate_core::Resolver) over `UnresolvedRef`s — the
//! `SymbolIndex` seam offers only exact-name lookup, which cannot match a generic profile against
//! the dataset population.
//!
//! ## Edge direction
//! `source = profile` (dependent), `target = asset` (dependency) — per the engine contract. So
//! `blast-radius <dataset>` surfaces both the JCL step that USES it and the RACF profile that
//! PROTECTS it: deleting the dataset orphans the profile, hence the profile depends on the asset.
//!
//! ## RACF generic matching (a documented approximation, not a bit-exact reimplementation)
//! Names are qualifiers separated by `.`; profiles use generic characters:
//! - `%` — exactly one non-period character.
//! - `*` — within a qualifier, zero or more characters (never crosses `.`); as a whole qualifier,
//!   exactly one qualifier.
//! - `**` — zero or more whole qualifiers (RACF Enhanced Generic Naming).
//!
//! When several profiles match one asset, RACF applies the MOST SPECIFIC. We approximate RACF's
//! specificity ordering with a deterministic score (exact ≫ generic; then fewer `**`, more
//! literal characters, fewer `*`, fewer `%`, more qualifiers). We only see the profiles that were
//! actually indexed, so the governing profile MAY differ from a live RACF database. Exact
//! (non-generic, equal) matches are emitted at [`ResolutionTier::Parsed`] (confidence 1.0);
//! generic-pattern matches at [`ResolutionTier::Heuristic`] (0.5) — never present an inferred
//! protection as a hard fact.

use wicked_estate_core::{Edge, EdgeKind, Node, NodeKind, ResolutionTier};

/// Recorded on every edge this pass emits (the `resolved_by` provenance string).
const RESOLVED_BY: &str = "estate-racf";

/// RACF general-resource class → the node `kind` it protects. Only precise resource classes are
/// mapped; broad admin classes (e.g. `MQADMIN`) are intentionally omitted to avoid over-linking.
fn class_to_kind(class: &str) -> Option<&'static str> {
    match class {
        "MQQUEUE" | "GMQQUEUE" => Some("mq_queue"),
        "MQTOPIC" | "MXTOPIC" => Some("mq_topic"),
        "MQCHANNEL" | "MQCHAN" => Some("mq_channel"),
        _ => None,
    }
}

fn node_kind_str(kind: &NodeKind) -> Option<&str> {
    match kind {
        NodeKind::Other(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Uppercase + strip one layer of surrounding single quotes (MQSC object names arrive quoted).
fn normalize(name: &str) -> String {
    let t = name.trim();
    let t = t.strip_prefix('\'').unwrap_or(t);
    let t = t.strip_suffix('\'').unwrap_or(t);
    t.to_ascii_uppercase()
}

fn has_generic(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('%')
}

/// Glob a SINGLE qualifier (contains no `.`): `*` = zero+ chars, `%` = exactly one char, else
/// literal. Operates on bytes (RACF names are EBCDIC-derived ASCII identifiers).
fn qualifier_glob(p: &[u8], s: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|k| qualifier_glob(rest, &s[k..])),
        Some((b'%', rest)) => !s.is_empty() && qualifier_glob(rest, &s[1..]),
        Some((c, rest)) => !s.is_empty() && s[0] == *c && qualifier_glob(rest, &s[1..]),
    }
}

/// Match a normalized RACF generic profile against a normalized asset name, qualifier-aware.
fn racf_match(pattern: &str, name: &str) -> bool {
    let p: Vec<&str> = pattern.split('.').collect();
    let n: Vec<&str> = name.split('.').collect();
    match_quals(&p, &n)
}

fn match_quals(p: &[&str], n: &[&str]) -> bool {
    match p.split_first() {
        None => n.is_empty(),
        // `**` consumes zero or more whole qualifiers.
        Some((&"**", rest)) => (0..=n.len()).any(|k| match_quals(rest, &n[k..])),
        Some((head, rest)) => {
            !n.is_empty()
                && qualifier_glob(head.as_bytes(), n[0].as_bytes())
                && match_quals(rest, &n[1..])
        }
    }
}

/// Specificity key — larger = more specific (RACF most-specific-wins approximation).
fn specificity(pattern: &str) -> (u8, i32, i32, i32, i32, usize) {
    let dstar = pattern.matches("**").count() as i32;
    let star = pattern.matches('*').count() as i32 - 2 * dstar; // lone `*`, excluding `**`
    let pct = pattern.matches('%').count() as i32;
    let literal = pattern
        .chars()
        .filter(|c| !matches!(c, '*' | '%' | '.'))
        .count() as i32;
    let quals = pattern.split('.').count();
    (
        u8::from(!has_generic(pattern)), // exact beats every generic
        -dstar,                          // fewer `**`  → more specific
        literal,                         // more literal characters
        -star,                           // fewer lone `*`
        -pct,                            // fewer `%`
        quals,                           // more qualifiers
    )
}

/// Derive `protects` edges from the node population: every RACF profile → the single most-specific
/// asset binding it governs. See the module docs for the matching + specificity model.
///
/// Accepts any iterator of node references (the CLI passes its in-memory index's nodes, so the
/// graph is not loaded from the store a second time).
pub fn estate_edges<'a, I>(nodes: I) -> Vec<Edge>
where
    I: IntoIterator<Item = &'a Node>,
{
    let mut dataset_profiles: Vec<&Node> = Vec::new();
    let mut general_profiles: Vec<&Node> = Vec::new();
    let mut datasets: Vec<&Node> = Vec::new();
    let mut resources: Vec<&Node> = Vec::new(); // mq_queue / mq_topic / mq_channel

    for n in nodes {
        match node_kind_str(&n.kind) {
            Some("racf_dataset_profile") => dataset_profiles.push(n),
            Some("racf_profile") => general_profiles.push(n),
            Some("dataset") => datasets.push(n),
            Some("mq_queue" | "mq_topic" | "mq_channel") => resources.push(n),
            _ => {}
        }
    }

    let mut out = Vec::new();

    // Dataset protection: the most-specific dataset profile governs each dataset.
    for ds in datasets.iter().copied() {
        let dsn = normalize(&ds.name);
        if let Some(p) = dataset_profiles
            .iter()
            .copied()
            .filter(|p| racf_match(&normalize(&p.name), &dsn))
            .max_by_key(|p| specificity(&normalize(&p.name)))
        {
            push_protects(&mut out, p, ds, &dsn);
        }
    }

    // General-resource protection: RDEFINE <class> profiles → MQ objects of the mapped kind.
    for res in resources.iter().copied() {
        let Some(rkind) = node_kind_str(&res.kind) else {
            continue;
        };
        let rname = normalize(&res.name);
        if let Some(p) = general_profiles
            .iter()
            .copied()
            .filter(|p| {
                p.signature
                    .as_deref()
                    .and_then(class_to_kind)
                    .is_some_and(|k| k == rkind)
            })
            .filter(|p| racf_match(&normalize(&p.name), &rname))
            .max_by_key(|p| specificity(&normalize(&p.name)))
        {
            push_protects(&mut out, p, res, &rname);
        }
    }

    out
}

fn push_protects(out: &mut Vec<Edge>, profile: &Node, asset: &Node, asset_norm: &str) {
    if profile.symbol == asset.symbol {
        return;
    }
    let pname = normalize(&profile.name);
    // Exact (non-generic, equal name) is a hard fact; a generic-pattern match is inference.
    let tier = if !has_generic(&pname) && pname == asset_norm {
        ResolutionTier::Parsed
    } else {
        ResolutionTier::Heuristic
    };
    out.push(
        Edge::new(
            profile.symbol.clone(),
            asset.symbol.clone(),
            EdgeKind::Other("protects".to_string()),
            tier,
            RESOLVED_BY,
        )
        .with_location(profile.location.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{Language, Location, Span, Symbol};

    fn node(scheme: &str, kind: &str, name: &str) -> Node {
        Node::new(
            Symbol::synthetic(scheme, name).id(),
            NodeKind::Other(kind.to_string()),
            name.to_string(),
            Language::new("racf"),
            Location::new("x.racf", Span::ZERO),
        )
    }
    fn ds_profile(pattern: &str) -> Node {
        node("racf", "racf_dataset_profile", pattern)
    }
    fn dataset(name: &str) -> Node {
        node("jcl-dataset", "dataset", name)
    }
    fn gen_profile(pattern: &str, class: &str) -> Node {
        let mut n = node("racf", "racf_profile", pattern);
        n.signature = Some(class.to_string());
        n
    }
    fn mq_queue(name: &str) -> Node {
        node("mq", "mq_queue", name)
    }

    // ── RACF generic matching ─────────────────────────────────────────────────

    #[test]
    fn exact_match() {
        assert!(racf_match("PAYROLL.MASTER.KSDS", "PAYROLL.MASTER.KSDS"));
        assert!(!racf_match("PAYROLL.MASTER.KSDS", "PAYROLL.MASTER.ESDS"));
    }

    #[test]
    fn double_star_matches_zero_or_more_qualifiers() {
        assert!(racf_match("PROD.PAY.**", "PROD.PAY.MASTER"));
        assert!(racf_match("PROD.PAY.**", "PROD.PAY.MASTER.KSDS"));
        assert!(racf_match("PROD.PAY.**", "PROD.PAY")); // `**` matches zero qualifiers
        assert!(racf_match("PROD.**", "PROD.PAY.MASTER.KSDS"));
        assert!(!racf_match("PROD.PAY.**", "PROD.HR.MASTER"));
    }

    #[test]
    fn single_star_is_one_qualifier_or_intra_qualifier() {
        assert!(racf_match("PROD.*.MASTER", "PROD.PAY.MASTER")); // whole-qualifier `*`
        assert!(!racf_match("PROD.*.MASTER", "PROD.PAY.HR.MASTER")); // `*` ≠ two qualifiers
        assert!(racf_match("PROD.PAY*.KSDS", "PROD.PAYROLL.KSDS")); // intra-qualifier `*`
        assert!(!racf_match("PROD.PAY*.KSDS", "PROD.HR.KSDS"));
    }

    #[test]
    fn percent_matches_exactly_one_char() {
        assert!(racf_match("PROD.PAY%.KSDS", "PROD.PAY1.KSDS"));
        assert!(!racf_match("PROD.PAY%.KSDS", "PROD.PAY.KSDS")); // needs exactly one char
        assert!(!racf_match("PROD.PAY%.KSDS", "PROD.PAY12.KSDS"));
    }

    // ── specificity / most-specific-wins ───────────────────────────────────────

    #[test]
    fn specificity_orders_exact_above_generic() {
        assert!(specificity("PROD.PAY.MASTER") > specificity("PROD.PAY.**"));
        assert!(specificity("PROD.PAY.**") > specificity("PROD.**"));
        assert!(specificity("PROD.PAY.%") > specificity("PROD.PAY.*"));
    }

    #[test]
    fn most_specific_profile_wins() {
        // Both PROD.** and PROD.PAY.** match; only the most-specific governs the dataset.
        let nodes = [
            ds_profile("PROD.**"),
            ds_profile("PROD.PAY.**"),
            dataset("PROD.PAY.MASTER"),
        ];
        let edges = estate_edges(&nodes);
        assert_eq!(edges.len(), 1, "exactly one governing profile");
        let winner = Symbol::synthetic("racf", "PROD.PAY.**").id();
        assert_eq!(
            edges[0].source, winner,
            "PROD.PAY.** is more specific than PROD.**"
        );
    }

    // ── estate_edges end-to-end ────────────────────────────────────────────────

    #[test]
    fn exact_dataset_protection_is_parsed_tier() {
        let nodes = [
            ds_profile("PAYROLL.MASTER.KSDS"),
            dataset("PAYROLL.MASTER.KSDS"),
        ];
        let edges = estate_edges(&nodes);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::Other("protects".to_string()));
        assert_eq!(edges[0].resolved_by, "estate-racf");
        assert!(
            edges[0].confidence.get() > 0.99,
            "exact match is a hard fact (Parsed/1.0), got {}",
            edges[0].confidence.get()
        );
    }

    #[test]
    fn generic_dataset_protection_is_heuristic_tier() {
        let nodes = [ds_profile("PAYROLL.**"), dataset("PAYROLL.MASTER.KSDS")];
        let edges = estate_edges(&nodes);
        assert_eq!(edges.len(), 1);
        assert!(
            edges[0].confidence.get() < 0.6,
            "generic match is inference (Heuristic/0.5), got {}",
            edges[0].confidence.get()
        );
    }

    #[test]
    fn mqqueue_class_protects_mq_queue_quotes_normalized() {
        // RDEFINE MQQUEUE PAYROLL.IN  vs  DEFINE QLOCAL('PAYROLL.IN') — quotes differ, must match.
        let nodes = [
            gen_profile("PAYROLL.IN", "MQQUEUE"),
            mq_queue("'PAYROLL.IN'"),
        ];
        let edges = estate_edges(&nodes);
        assert_eq!(edges.len(), 1, "MQQUEUE profile must protect the mq_queue");
        assert_eq!(
            edges[0].source,
            Symbol::synthetic("racf", "PAYROLL.IN").id()
        );
    }

    #[test]
    fn unmapped_class_does_not_link() {
        // FACILITY is not a resource class we model → no protects edge to an MQ object.
        let nodes = [
            gen_profile("PAYROLL.IN", "FACILITY"),
            mq_queue("'PAYROLL.IN'"),
        ];
        assert!(estate_edges(&nodes).is_empty());
    }

    #[test]
    fn no_match_emits_no_edge() {
        let nodes = [ds_profile("HR.**"), dataset("PAYROLL.MASTER.KSDS")];
        assert!(estate_edges(&nodes).is_empty());
    }
}
