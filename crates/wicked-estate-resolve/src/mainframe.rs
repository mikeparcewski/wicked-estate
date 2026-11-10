//! COBOL dynamic-call resolution — `CALL WS-PGM` where `WS-PGM` holds exactly one literal.
//!
//! The COBOL text supplement (`wicked-estate-extract`'s `cobol_links`) reads each program's own
//! source for the literals a program-name variable can hold (`VALUE 'PRICEPGM'`, `MOVE 'PRICEPGM'
//! TO WS-PGM`, `SET WS-PTR TO ENTRY 'PRICEPGM'`). When there is exactly one, and no non-literal
//! assignment, it rewrites the call's ref to
//! [`COBOL_DYNAMIC_CALL_PREFIX`](wicked_estate_core::refs::COBOL_DYNAMIC_CALL_PREFIX)`PRICEPGM`.
//!
//! [`DynamicCallResolver`] binds those refs, and only those, to the UNIQUE program (a
//! [`NodeKind::Module`] of any language — COBOL, HLASM, a JCL PROC) with that name, at
//! [`ResolutionTier::Heuristic`]: the value is inferred from the program's text, not proven at run
//! time (an `ACCEPT`, a `READ … INTO`, a `CALL … USING` by reference, or a copybook can still change
//! it). The edge says so in `metadata.heuristic`. No name-based resolver can see the prefixed name,
//! so the edge is never minted at their higher `ImportMap` confidence. Ambiguity (two programs of
//! that name) or absence parks the ref, prefix intact, so a later back-fill binds it the same way.

use wicked_estate_core::refs::COBOL_DYNAMIC_CALL_PREFIX;
use wicked_estate_core::{
    Edge, NodeKind, ResolutionTier, Resolver, Result, SymbolIndex, UnresolvedRef,
};

/// Binds `cobol-dynamic-call:<NAME>` refs to the unique program named `<NAME>`. See the module docs.
#[derive(Debug, Default, Clone, Copy)]
pub struct DynamicCallResolver;

impl Resolver for DynamicCallResolver {
    fn id(&self) -> &str {
        "cobol-dynamic-call"
    }

    fn tier(&self) -> ResolutionTier {
        ResolutionTier::Heuristic
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
        let mut out = Vec::new();
        for r in refs {
            let Some(name) = r.raw_name.strip_prefix(COBOL_DYNAMIC_CALL_PREFIX) else {
                continue;
            };
            let candidates: Vec<_> = index
                .by_name(name)
                .into_iter()
                .filter(|n| n.kind == NodeKind::Module)
                .filter(wicked_estate_core::is_structural_symbol)
                .filter(|n| n.symbol != r.from)
                .collect();
            let [only] = candidates.as_slice() else {
                continue;
            };
            let mut edge = Edge::new(
                r.from.clone(),
                only.symbol.clone(),
                r.kind.clone(),
                self.tier(),
                self.id(),
            )
            .with_location(r.location.clone());
            edge.metadata.insert(
                "heuristic".to_string(),
                serde_json::Value::String("dynamic-call-literal".to_string()),
            );
            edge.metadata.insert(
                "program".to_string(),
                serde_json::Value::String(name.to_string()),
            );
            out.push(edge);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{EdgeKind, Language, Location, Node, Span, Symbol, SymbolId};

    struct Idx(Vec<Node>);
    impl SymbolIndex for Idx {
        fn by_name(&self, name: &str) -> Vec<Node> {
            self.0.iter().filter(|n| n.name == name).cloned().collect()
        }
        fn get(&self, id: &SymbolId) -> Option<Node> {
            self.0.iter().find(|n| &n.symbol == id).cloned()
        }
        fn all_nodes(&self) -> Result<Vec<Node>> {
            Ok(self.0.clone())
        }
    }

    fn module(tag: &str, name: &str, lang: &str) -> Node {
        Node::new(
            Symbol::synthetic(tag, name).id(),
            NodeKind::Module,
            name,
            Language::new(lang),
            Location::new(format!("{name}.src"), Span::ZERO),
        )
    }

    fn dyn_ref(name: &str) -> UnresolvedRef {
        UnresolvedRef::new(
            Symbol::synthetic("t", "ORDPGM").id(),
            format!("{COBOL_DYNAMIC_CALL_PREFIX}{name}"),
            EdgeKind::Calls,
            Location::new("cbl/ORDPGM.cbl", Span::ZERO),
        )
    }

    #[test]
    fn binds_unique_program_of_any_language_at_heuristic() {
        let idx = Idx(vec![module("hlasm", "PRICEPGM", "hlasm")]);
        let edges = DynamicCallResolver
            .resolve(&[dyn_ref("PRICEPGM")], &idx)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0].provenance,
            wicked_estate_core::Provenance::Heuristic
        );
        assert!((edges[0].confidence.get() - 0.5).abs() < 1e-6);
        assert_eq!(edges[0].kind, EdgeKind::Calls);
        assert_eq!(edges[0].metadata["heuristic"], "dynamic-call-literal");
    }

    #[test]
    fn ambiguous_absent_non_module_and_unprefixed_stay_unbound() {
        let field = Node::new(
            Symbol::synthetic("f", "PRICEPGM").id(),
            NodeKind::Field,
            "PRICEPGM",
            Language::new("cobol"),
            Location::new("x.cbl", Span::ZERO),
        );
        let two = Idx(vec![
            module("a", "PRICEPGM", "cobol"),
            module("b", "PRICEPGM", "hlasm"),
        ]);
        assert!(
            DynamicCallResolver
                .resolve(&[dyn_ref("PRICEPGM")], &two)
                .unwrap()
                .is_empty()
        );
        let only_field = Idx(vec![field]);
        assert!(
            DynamicCallResolver
                .resolve(&[dyn_ref("PRICEPGM")], &only_field)
                .unwrap()
                .is_empty()
        );
        let one = Idx(vec![module("a", "PRICEPGM", "cobol")]);
        let mut plain = dyn_ref("PRICEPGM");
        plain.raw_name = "PRICEPGM".into();
        assert!(
            DynamicCallResolver
                .resolve(&[plain], &one)
                .unwrap()
                .is_empty()
        );
        assert!(
            DynamicCallResolver
                .resolve(&[dyn_ref("NOPGM")], &one)
                .unwrap()
                .is_empty()
        );
    }
}
