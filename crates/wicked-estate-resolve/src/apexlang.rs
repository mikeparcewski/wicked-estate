//! APEXlang reference resolution (estate#313).
//!
//! The APEXlang extractor writes every `@slug` as
//! [`apexlang-ref:<kind>:<slug>:<scope>`](wicked_estate_core::refs::apexlang_ref). This resolver
//! binds those refs, and only those:
//! - candidates are the APEXlang nodes named `<slug>` that sit strictly under `<scope>` (the
//!   application, page or region the reference is scoped to) — never another application, never
//!   another language;
//! - a typed ref (`<kind>` from the extractor's property table) keeps candidates of that kind and
//!   binds at [`ResolutionTier::Parsed`]: the reference is explicit syntax and its target kind is
//!   data;
//! - an untyped ref ([`ANY_KIND`], a property not in the table) binds only when the slug names
//!   exactly one component in the scope, at [`ResolutionTier::Heuristic`];
//! - [`NO_BIND`] refs never bind.
//!
//! Exactly one candidate binds; zero or several leave the ref unresolved (never a guess). Every
//! input is in the persisted `raw_name`, so a stored ref re-resolves to the same edge without
//! re-extraction.

use wicked_estate_core::refs::apexlang_ref::{
    self, ANY_KIND, BINDABLE, BINDABLE_KEY, HEURISTIC, NO_BIND,
};
use wicked_estate_core::{Edge, ResolutionTier, Resolver, Result, SymbolIndex, UnresolvedRef};

/// Language name of APEXlang nodes (`wicked-estate-extract`'s `apexlang::LANGUAGE`).
const LANGUAGE: &str = "apexlang";
/// Node metadata key holding the APEXlang component kind.
const KIND_KEY: &str = "apexlang_kind";

/// Binds `apexlang-ref:` refs. See the module docs.
#[derive(Debug, Default, Clone, Copy)]
pub struct ApexlangRefResolver;

impl Resolver for ApexlangRefResolver {
    fn id(&self) -> &str {
        "apexlang-ref"
    }

    fn tier(&self) -> ResolutionTier {
        ResolutionTier::Parsed
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
        let mut out = Vec::new();
        for r in refs {
            // A top-level component's link to its holder (application / container directory):
            // `Contains` from the holder, once the holder is indexed.
            if let Some(holder) = apexlang_ref::parse_container(&r.raw_name) {
                let holder = wicked_estate_core::SymbolId(holder.to_string());
                if index
                    .get(&holder)
                    .is_some_and(|n| n.language.as_str() == LANGUAGE)
                    && holder != r.from
                {
                    let mut edge = Edge::new(
                        holder,
                        r.from.clone(),
                        r.kind.clone(),
                        ResolutionTier::Parsed,
                        self.id(),
                    )
                    .with_location(r.location.clone());
                    edge.metadata.insert("via".into(), "layout".into());
                    out.push(edge);
                }
                continue;
            }
            let Some(parsed) = apexlang_ref::parse(&r.raw_name) else {
                continue;
            };
            if parsed.kind == NO_BIND || parsed.scope.is_empty() {
                continue;
            }
            // `~kind`: typed, but found by a text scan (a bind variable in an unparsed block).
            let (kind, scanned) = match parsed.kind.strip_prefix(HEURISTIC) {
                Some(k) => (k.to_string(), true),
                None => (parsed.kind.clone(), false),
            };
            let typed = kind != ANY_KIND;
            let candidates: Vec<_> = index
                .by_name(&parsed.slug)
                .into_iter()
                .filter(|n| n.language.as_str() == LANGUAGE)
                .filter(wicked_estate_core::is_structural_symbol)
                .filter(|n| {
                    let s = n.symbol.as_str();
                    s.len() > parsed.scope.len() && s.starts_with(&parsed.scope)
                })
                .filter(|n| {
                    if !typed {
                        return true;
                    }
                    if kind == BINDABLE {
                        return n.metadata.get(BINDABLE_KEY).and_then(|v| v.as_bool())
                            == Some(true);
                    }
                    n.metadata.get(KIND_KEY).and_then(|v| v.as_str()) == Some(kind.as_str())
                })
                .collect();
            let [only] = candidates.as_slice() else {
                continue;
            };
            if only.symbol == r.from {
                continue;
            }
            let tier = if typed && !scanned {
                ResolutionTier::Parsed
            } else {
                ResolutionTier::Heuristic
            };
            let mut edge = Edge::new(
                r.from.clone(),
                only.symbol.clone(),
                r.kind.clone(),
                tier,
                self.id(),
            )
            .with_location(r.location.clone());
            let via = match (typed, scanned) {
                (true, false) if kind == BINDABLE => "bind-variable",
                (true, false) => "kind-table",
                (true, true) => "text-scan",
                (false, _) => "unique-slug",
            };
            edge.metadata.insert("via".into(), via.into());
            if typed {
                edge.metadata.insert("target_kind".into(), kind.into());
            }
            out.push(edge);
        }
        Ok(out)
    }
}

/// Every `raw_name` under which a parked ref could name `node` (the back-fill probe set): the
/// node's own kind and [`ANY_KIND`], under each of its enclosing scopes. Empty for a node that is
/// not an APEXlang component.
pub fn backfill_probes(node: &wicked_estate_core::Node) -> Vec<String> {
    if node.language.as_str() != LANGUAGE {
        return Vec::new();
    }
    let Some(kind) = node.metadata.get(KIND_KEY).and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let sym = node.symbol.as_str();
    // Enclosing scopes are the `#`-terminated prefixes of the node's own id: the application
    // (`…/app#`) and every enclosing component, excluding the node itself.
    let mut scopes = Vec::new();
    for (i, _) in sym.match_indices('#') {
        let prefix = &sym[..=i];
        if prefix.len() < sym.len() {
            scopes.push(prefix);
        }
    }
    let mut out = Vec::with_capacity(scopes.len() * 2 + 1);
    // Components parked waiting for this node as their holder.
    out.push(apexlang_ref::container(sym));
    for scope in scopes {
        let scanned = format!("{HEURISTIC}{kind}");
        for k in [kind, ANY_KIND, scanned.as_str()] {
            out.push(apexlang_ref::format(k, &node.name, scope));
        }
        if node.metadata.get(BINDABLE_KEY).and_then(|v| v.as_bool()) == Some(true) {
            for k in [BINDABLE.to_string(), format!("{HEURISTIC}{BINDABLE}")] {
                out.push(apexlang_ref::format(&k, &node.name, scope));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wicked_estate_core::{
        EdgeKind, Language, Location, Node, NodeKind, Provenance, Span, SymbolId,
    };

    struct Idx(Vec<Node>);
    impl SymbolIndex for Idx {
        fn by_name(&self, name: &str) -> Vec<Node> {
            self.0.iter().filter(|n| n.name == name).cloned().collect()
        }
        fn get(&self, id: &SymbolId) -> Option<Node> {
            self.0.iter().find(|n| &n.symbol == id).cloned()
        }
    }

    const APP1: &str = "apexlang . . . apps/one/app#";
    const APP2: &str = "apexlang . . . apps/two/app#";

    fn comp(sym: &str, kind: &str, name: &str) -> Node {
        let mut n = Node::new(
            SymbolId(sym.into()),
            NodeKind::Other(kind.into()),
            name,
            Language::new(LANGUAGE),
            Location::new("f.apx", Span::ZERO),
        );
        n.metadata.insert(KIND_KEY.into(), kind.into());
        n
    }

    fn reference(from: &str, kind: &str, slug: &str, scope: &str) -> UnresolvedRef {
        UnresolvedRef::new(
            SymbolId(from.into()),
            apexlang_ref::format(kind, slug, scope),
            EdgeKind::References,
            Location::new("p.apx", Span::ZERO),
        )
    }

    /// `breadcrumb` is a region, a breadcrumb and a breadcrumbTemplate in one application
    /// (oracle/skills example-1): the kind decides, and an untyped ref stays unresolved.
    #[test]
    fn ambiguous_slug_binds_only_by_kind() {
        let page = format!("{APP1}page:5#");
        let idx = Idx(vec![
            comp(&format!("{page}region:breadcrumb#"), "region", "breadcrumb"),
            comp(
                &format!("{APP1}breadcrumb:breadcrumb#"),
                "breadcrumb",
                "breadcrumb",
            ),
            comp(
                &format!("{APP1}theme:ut#breadcrumbTemplate:breadcrumb#"),
                "breadcrumbTemplate",
                "breadcrumb",
            ),
        ]);
        let from = format!("{page}region:breadcrumb#");
        let refs = vec![
            reference(&from, "breadcrumb", "breadcrumb", APP1),
            reference(&from, "breadcrumbTemplate", "breadcrumb", APP1),
            reference(&from, ANY_KIND, "breadcrumb", APP1),
        ];
        let edges = ApexlangRefResolver.resolve(&refs, &idx).unwrap();
        let targets: Vec<&str> = edges.iter().map(|e| e.target.as_str()).collect();
        assert_eq!(
            targets,
            vec![
                format!("{APP1}breadcrumb:breadcrumb#"),
                format!("{APP1}theme:ut#breadcrumbTemplate:breadcrumb#"),
            ],
            "typed refs bind their kind; the untyped one is ambiguous and stays unresolved"
        );
        for e in &edges {
            assert_eq!(e.provenance, Provenance::Parsed);
            assert_eq!(e.resolved_by, "apexlang-ref");
            assert!(e.location.is_some());
        }
    }

    #[test]
    fn a_holder_contains_its_component_only_once_it_exists() {
        let page = format!("{APP1}page:1#");
        let r = UnresolvedRef::new(
            SymbolId(page.clone()),
            apexlang_ref::container(APP1),
            EdgeKind::Contains,
            Location::new("app/pages/p1.apx", Span::ZERO),
        );
        let without = Idx(vec![comp(&page, "page", "1")]);
        assert!(
            ApexlangRefResolver
                .resolve(std::slice::from_ref(&r), &without)
                .unwrap()
                .is_empty()
        );
        let with = Idx(vec![comp(&page, "page", "1"), comp(APP1, "app", "ONE")]);
        let edges = ApexlangRefResolver.resolve(&[r], &with).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            (edges[0].source.as_str(), edges[0].target.as_str()),
            (APP1, page.as_str())
        );
        assert_eq!(edges[0].kind, EdgeKind::Contains);
    }

    #[test]
    fn text_scanned_kinds_bind_at_heuristic_tier() {
        let page = format!("{APP1}page:9#");
        let mut item = comp(&format!("{page}pageItem:P9_X#"), "pageItem", "P9_X");
        item.metadata.insert(BINDABLE_KEY.into(), true.into());
        // Not bindable (a region named like an item) never competes.
        let idx = Idx(vec![
            item,
            comp(&format!("{APP1}page:2#region:P9_X#"), "region", "P9_X"),
        ]);
        let from = format!("{APP1}page:9#process:p#");
        let refs = vec![
            reference(&from, BINDABLE, "P9_X", APP1),
            reference(&from, &format!("{HEURISTIC}{BINDABLE}"), "P9_X", APP1),
        ];
        let edges = ApexlangRefResolver.resolve(&refs, &idx).unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].provenance, Provenance::Parsed);
        assert_eq!(edges[1].provenance, Provenance::Heuristic);
        assert_eq!(
            edges[1].metadata.get("via").and_then(|v| v.as_str()),
            Some("text-scan")
        );
    }

    /// A quoted APEXlang id can spell an encoded ref name; the name tiers must still skip it.
    #[test]
    fn name_tiers_never_bind_reserved_names_even_when_a_node_spells_them() {
        use crate::{NameResolver, ScopedNameResolver};
        let raw = apexlang_ref::format("authorization", "admin", APP1);
        let trap = comp(&format!("{APP1}installScript:x#"), "installScript", &raw);
        let idx = Idx(vec![trap]);
        let refs = vec![UnresolvedRef::new(
            SymbolId(format!("{APP1}page:1#")),
            raw,
            EdgeKind::References,
            Location::new("p.apx", Span::ZERO),
        )];
        assert!(NameResolver.resolve(&refs, &idx).unwrap().is_empty());
        assert!(ScopedNameResolver.resolve(&refs, &idx).unwrap().is_empty());
    }

    #[test]
    fn same_slug_in_two_applications_never_crosses() {
        let idx = Idx(vec![comp(
            &format!("{APP2}authorization:contribution-rights#"),
            "authorization",
            "contribution-rights",
        )]);
        let refs = vec![
            reference(
                &format!("{APP1}page:4#"),
                "authorization",
                "contribution-rights",
                APP1,
            ),
            reference(
                &format!("{APP1}page:4#"),
                ANY_KIND,
                "contribution-rights",
                APP1,
            ),
        ];
        assert!(ApexlangRefResolver.resolve(&refs, &idx).unwrap().is_empty());
    }

    #[test]
    fn page_scope_excludes_other_pages_and_untyped_is_heuristic() {
        let idx = Idx(vec![
            comp(&format!("{APP1}page:1#region:main#"), "region", "main"),
            comp(&format!("{APP1}page:2#region:main#"), "region", "main"),
            comp(&format!("{APP1}lov:only-one#"), "lov", "only-one"),
        ]);
        let refs = vec![
            reference(
                &format!("{APP1}page:2#pageItem:P2_X#"),
                "region",
                "main",
                &format!("{APP1}page:2#"),
            ),
            reference(
                &format!("{APP1}page:2#pageItem:P2_X#"),
                ANY_KIND,
                "only-one",
                APP1,
            ),
            reference(
                &format!("{APP1}page:2#pageItem:P2_X#"),
                NO_BIND,
                "main",
                APP1,
            ),
        ];
        let edges = ApexlangRefResolver.resolve(&refs, &idx).unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(
            edges[0].target.as_str(),
            format!("{APP1}page:2#region:main#")
        );
        assert_eq!(edges[1].provenance, Provenance::Heuristic);
        assert_eq!(
            edges[1].metadata.get("via").and_then(|v| v.as_str()),
            Some("unique-slug")
        );
    }

    #[test]
    fn non_apexlang_nodes_and_other_names_are_ignored() {
        let mut js = comp(&format!("{APP1}region:main#"), "region", "main");
        js.language = Language::new("javascript");
        let idx = Idx(vec![js]);
        let refs = vec![
            reference(&format!("{APP1}page:1#"), "region", "main", APP1),
            UnresolvedRef::new(
                SymbolId("x".into()),
                "main".to_string(),
                EdgeKind::References,
                Location::new("p.apx", Span::ZERO),
            ),
        ];
        assert!(ApexlangRefResolver.resolve(&refs, &idx).unwrap().is_empty());
    }

    #[test]
    fn backfill_probes_cover_every_enclosing_scope() {
        let n = comp(&format!("{APP1}page:2#region:main#"), "region", "main");
        let probes: HashMap<String, ()> =
            backfill_probes(&n).into_iter().map(|p| (p, ())).collect();
        for scope in [APP1.to_string(), format!("{APP1}page:2#")] {
            assert!(probes.contains_key(&apexlang_ref::format("region", "main", &scope)));
            assert!(probes.contains_key(&apexlang_ref::format(ANY_KIND, "main", &scope)));
        }
        assert!(probes.contains_key(&apexlang_ref::container(n.symbol.as_str())));
        assert!(probes.contains_key(&apexlang_ref::format("~region", "main", APP1)));
        // A bindable component is also probed as a bind-variable target.
        let mut item = comp(&format!("{APP1}page:2#pageItem:P2_X#"), "pageItem", "P2_X");
        item.metadata.insert(BINDABLE_KEY.into(), true.into());
        let item_probes = backfill_probes(&item);
        for k in [BINDABLE.to_string(), format!("{HEURISTIC}{BINDABLE}")] {
            assert!(item_probes.contains(&apexlang_ref::format(&k, "P2_X", APP1)));
        }
        assert_eq!(
            probes.len(),
            7,
            "the node's own id is not a scope of itself"
        );
    }
}
