//! Boundary and definition contributions under one shared id, settled inside ONE extraction.
//!
//! The mainframe extractors mint shared synthetic ids for things that a referencing file names
//! and a different file may define: a Db2 table, a CICS map, a copybook, a JCL DD binding, a RACF
//! identity. The referencing file contributes a BOUNDARY record ([`Node::as_boundary`]); the
//! defining file contributes the definition. Across files, the store keeps one contribution per
//! `(symbol, file)` and surfaces the definition (wicked-estate#152). Inside one file, the store
//! keeps only the LAST record for a `(symbol, file)` pair, so a boundary written after the
//! definition would hide it. [`settle`] fixes that before the extraction leaves the extractor.

use std::collections::HashMap;
use wicked_estate_core::{Node, SymbolId};

/// Collapse records that share a symbol: the first DEFINITION wins; with no definition, the first
/// boundary/declaration record is kept. Records with unique symbols are untouched and keep order.
pub(crate) fn settle(nodes: &mut Vec<Node>) {
    let mut at: HashMap<SymbolId, usize> = HashMap::new();
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for n in nodes.drain(..) {
        match at.get(&n.symbol) {
            None => {
                at.insert(n.symbol.clone(), out.len());
                out.push(n);
            }
            Some(&i) => {
                if out[i].is_declaration() && !n.is_declaration() {
                    out[i] = n;
                }
            }
        }
    }
    *nodes = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{Language, Location, NodeKind, Span, Symbol};

    fn n(name: &str, boundary: bool) -> Node {
        let node = Node::new(
            Symbol::synthetic("x", "ID").id(),
            NodeKind::Other(name.into()),
            "ID",
            Language::new("x"),
            Location::new("f", Span::ZERO),
        );
        if boundary {
            node.as_boundary("why")
        } else {
            node
        }
    }

    #[test]
    fn definition_wins_regardless_of_order() {
        let mut a = vec![n("b1", true), n("def", false), n("b2", true)];
        settle(&mut a);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NodeKind::Other("def".into()));
        let mut b = vec![n("b1", true), n("b2", true)];
        settle(&mut b);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, NodeKind::Other("b1".into()));
    }
}
