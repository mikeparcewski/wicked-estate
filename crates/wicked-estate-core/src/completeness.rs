//! Span completeness: does a file's extraction cover the file, with no hole between neighbours?
//!
//! A parser that gives up partway through a file (a grammar that swallows the rest of the file
//! into one error node) leaves no trace an extraction-count test sees: the nodes it did make are
//! correct, there are just no nodes after the stop. Two span facts expose it, for any language:
//!
//! - **tail:** the last node ends at or near the end of the file. The lines after it that are not
//!   trivia (blank, or whatever the caller's `is_trivia` says) are [`Completeness::uncovered_tail`].
//! - **siblings:** a node's span ends where its next sibling starts. Siblings are nodes with the
//!   same innermost containing node (or none). The non-trivia lines strictly between one sibling's
//!   end line and the next one's start line are a [`SiblingGap`].
//!
//! Neither is an invariant of every language (a TypeScript file can end with top-level statements
//! that are no node), so this reports and the caller asserts: pick the nodes that must tile the
//! file (COBOL paragraphs, JCL steps) and the trivia rule of the language. This is a test helper
//! (`wicked_estate_core::completeness::check`), not a stored statistic.

use crate::{Node, NodeKind};

/// Non-trivia lines between two neighbouring nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiblingGap {
    /// Name of the node before the gap.
    pub before: String,
    /// Name of the node after the gap.
    pub after: String,
    /// 0-based numbers of the non-trivia lines strictly between them.
    pub lines: Vec<u32>,
}

/// The result of [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completeness {
    /// Lines in the source (a final line without a newline counts).
    pub total_lines: u32,
    /// 0-based line the last-ending checked node ends on; `None` when no node was checked.
    pub last_end_line: Option<u32>,
    /// 0-based numbers of the non-trivia lines after `last_end_line` (every non-trivia line when
    /// no node was checked).
    pub uncovered_tail: Vec<u32>,
    /// Neighbouring siblings with non-trivia lines between them, in source order.
    pub sibling_gaps: Vec<SiblingGap>,
}

impl Completeness {
    /// No uncovered tail and no sibling gap.
    pub fn is_complete(&self) -> bool {
        self.uncovered_tail.is_empty() && self.sibling_gaps.is_empty()
    }
}

/// Check `nodes` (one file's; `File` nodes are ignored) against `source`. A line is trivia when it
/// is blank or `is_trivia(line)` is true.
pub fn check<'a>(
    source: &str,
    nodes: impl IntoIterator<Item = &'a Node>,
    is_trivia: impl Fn(&str) -> bool,
) -> Completeness {
    let lines: Vec<&str> = source.lines().collect();
    let trivia = |i: u32| -> bool {
        lines
            .get(i as usize)
            .is_none_or(|l| l.trim().is_empty() || is_trivia(l))
    };
    let mut spans: Vec<(&str, u32, u32, u32, u32)> = nodes
        .into_iter()
        .filter(|n| n.kind != NodeKind::File)
        .map(|n| {
            let s = n.location.span;
            (
                n.name.as_str(),
                s.start_byte,
                s.end_byte,
                s.start_line,
                s.end_line,
            )
        })
        .collect();
    spans.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)));
    let total_lines = lines.len() as u32;
    let last_end_line = spans.iter().map(|s| s.4).max();
    let tail_from = last_end_line.map_or(0, |l| l + 1);
    let uncovered_tail = (tail_from..total_lines).filter(|i| !trivia(*i)).collect();
    // Innermost container of each span: the smallest other span holding it.
    let container = |i: usize| -> Option<usize> {
        let (_, s, e, _, _) = spans[i];
        (0..spans.len())
            .filter(|&j| j != i && spans[j].1 <= s && e <= spans[j].2)
            .filter(|&j| (spans[j].1, spans[j].2) != (s, e) || j < i)
            .min_by_key(|&j| spans[j].2 - spans[j].1)
    };
    let parents: Vec<Option<usize>> = (0..spans.len()).map(container).collect();
    let mut sibling_gaps = Vec::new();
    for i in 0..spans.len() {
        let next = (i + 1..spans.len()).find(|&j| parents[j] == parents[i]);
        let Some(j) = next else { continue };
        let gap: Vec<u32> = (spans[i].4 + 1..spans[j].3)
            .filter(|l| !trivia(*l))
            .collect();
        if !gap.is_empty() {
            sibling_gaps.push(SiblingGap {
                before: spans[i].0.to_string(),
                after: spans[j].0.to_string(),
                lines: gap,
            });
        }
    }
    Completeness {
        total_lines,
        last_end_line,
        uncovered_tail,
        sibling_gaps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, Location, Span, SymbolId};

    fn node(name: &str, src: &str, first: u32, last: u32) -> Node {
        let starts: Vec<u32> = std::iter::once(0)
            .chain(src.match_indices('\n').map(|(i, _)| i as u32 + 1))
            .collect();
        let span = Span {
            start_byte: starts[first as usize],
            end_byte: starts[last as usize + 1] - 1,
            start_line: first,
            start_col: 0,
            end_line: last,
            end_col: 0,
        };
        Node::new(
            SymbolId::from(format!("t {name}")),
            NodeKind::Function,
            name,
            Language::new("t"),
            Location::new("f", span),
        )
    }

    const SRC: &str = "a\nb\n\nc\nd\n# note\ne\n";

    #[test]
    fn tiling_nodes_are_complete() {
        let nodes = [node("A", SRC, 0, 2), node("B", SRC, 3, 6)];
        let c = check(SRC, &nodes, |_| false);
        assert!(c.is_complete(), "{c:?}");
        assert_eq!((c.total_lines, c.last_end_line), (7, Some(6)));
    }

    #[test]
    fn a_stop_shows_as_an_uncovered_tail() {
        let nodes = [node("A", SRC, 0, 1)];
        let c = check(SRC, &nodes, |l| l.starts_with('#'));
        assert_eq!(
            c.uncovered_tail,
            vec![3, 4, 6],
            "blank and trivia lines are not counted"
        );
        assert!(!c.is_complete());
    }

    #[test]
    fn a_hole_between_siblings_is_a_gap_but_nested_nodes_are_not() {
        // OUTER holds A and B; A ends on line 1, B starts on line 4: line 3 is uncovered.
        let nodes = [
            node("OUTER", SRC, 0, 6),
            node("A", SRC, 0, 1),
            node("B", SRC, 4, 6),
        ];
        let c = check(SRC, &nodes, |_| false);
        assert!(c.uncovered_tail.is_empty());
        assert_eq!(
            c.sibling_gaps,
            vec![SiblingGap {
                before: "A".into(),
                after: "B".into(),
                lines: vec![3],
            }]
        );
        // The File node is ignored, whatever its span.
        let mut file = node("f", SRC, 0, 0);
        file.kind = NodeKind::File;
        let c2 = check(SRC, nodes.iter().chain([&file]), |_| false);
        assert_eq!(c2.sibling_gaps, c.sibling_gaps);
    }
}
