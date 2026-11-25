//! The COBOL classifier as it was before wicked-estate#582 (TsNode walks), kept as the test
//! oracle: under `cfg(test)`, `classify_in` checks every COBOL answer against it, so every unit
//! test that extracts COBOL also proves the array-based classifier gives the same answer. The one
//! exception is the counted-work test, which turns the oracle off on its thread (it is quadratic).

use super::{
    Behaviour, COBOL_AREA_B_COLUMN, WALK_BUDGET, cobol_area_exit_only, cobol_header_is_verb,
};
use tree_sitter::Node as TsNode;

/// The old `treesitter::cobol_body_end`.
fn cobol_body_end(header: TsNode<'_>) -> Option<usize> {
    let parent = header.parent()?;
    if !crate::treesitter::COBOL_PROC_PARENTS.contains(&parent.kind()) {
        return None;
    }
    let mut next = header.next_sibling();
    while let Some(sib) = next {
        if crate::treesitter::COBOL_HEADERS.contains(&sib.kind())
            && crate::treesitter::is_cobol_header(sib)
        {
            return Some(sib.start_byte());
        }
        next = sib.next_sibling();
    }
    Some(parent.end_byte())
}

pub(super) fn cobol(header: TsNode<'_>, src: &[u8]) -> Behaviour {
    let Some(end) = cobol_body_end(header) else {
        return Behaviour::Real;
    };
    if header.start_position().column >= COBOL_AREA_B_COLUMN || cobol_header_is_verb(header, src) {
        return Behaviour::Real;
    }
    let mut statements = 0usize;
    let mut ended_by_header = false;
    let mut next = header.next_sibling();
    while let Some(n) = next {
        next = n.next_sibling();
        if crate::treesitter::is_cobol_header(n) {
            // A header the grammar built from a verb (`COMMIT.`, `ROLLBACK.`) is a statement it
            // misread, in any column. Any other header ends the span, whatever column it starts in.
            if cobol_header_is_verb(n, src) {
                return Behaviour::Real;
            }
            ended_by_header = true;
            break;
        }
        if n.start_byte() > end {
            break;
        }
        // Every child, named or not, zero-width at the parent's end included.
        if n.is_error() || n.is_missing() || n.has_error() {
            return Behaviour::Real;
        }
        if !n.is_named() {
            continue;
        }
        match n.kind() {
            "period" | "comment" => {}
            "exit_statement" | "continue_statement" | "goback_statement" => statements += 1,
            _ => return Behaviour::Real,
        }
    }
    // A declaratives paragraph's errors are only those inside its own `procedure_declaratives`
    // (a MISSING period on `END DECLARATIVES`, say); an ERROR or MISSING in the main procedure
    // code that follows a valid `END DECLARATIVES.` does not make it real.
    let scan_root = header
        .parent()
        .filter(|p| p.kind() == "procedure_declaratives")
        .unwrap_or_else(|| {
            let mut root = header;
            while let Some(p) = root.parent() {
                root = p;
            }
            root
        });
    if !ended_by_header && error_after_division(scan_root, end) {
        return Behaviour::Real;
    }
    if statements > 0 {
        let Ok(source) = std::str::from_utf8(src) else {
            return Behaviour::Real;
        };
        // The grammar reads fixed columns and silently drops code that starts in column 1 (free
        // format, continuations): read the paragraph's own source and keep it real unless every
        // line of it is fixed-format shape and holds only exit-only statements.
        if crate::cobol_links::source_is_free_format(source)
            || !cobol_exit_only_source_ok(header, end, ended_by_header, src)
        {
            return Behaviour::Real;
        }
        Behaviour::Stub("exit_only")
    } else {
        Behaviour::Real
    }
}

fn cobol_exit_only_source_ok(
    header: TsNode<'_>,
    end: usize,
    ended_by_header: bool,
    src: &[u8],
) -> bool {
    let line_start = |at: usize| {
        src[..at.min(src.len())]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1)
    };
    let line_begin = line_start(header.start_byte());
    let end = end.min(src.len());
    // A following header starts its own span on its own line. The last paragraph's span reaches
    // at least the end of the physical line its division ends on, then on to the line of the next
    // non-extra node after the division (or EOF): lines the grammar dropped there (a column-1
    // statement read as a comment, a statement past column 72) are read too.
    let region_end = if ended_by_header || end == 0 || src[end - 1] == b'\n' {
        line_start(end)
    } else {
        src[end..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(src.len(), |i| end + i)
    };
    let region_end = if ended_by_header {
        region_end
    } else {
        let mut ancestor = header.parent();
        let mut next_token = None;
        'ancestors: while let Some(node) = ancestor {
            let mut next = node.next_sibling();
            while let Some(sibling) = next {
                if !sibling.is_extra() {
                    next_token = Some(sibling.start_byte());
                    break 'ancestors;
                }
                next = sibling.next_sibling();
            }
            ancestor = node.parent();
        }
        region_end.max(next_token.map_or(src.len(), line_start))
    };
    let region_end = region_end.max(line_begin);
    // When the span ends at the next header, that header's own line is dropped from the scan. A
    // free-format statement the grammar misread as a header (e.g. `CALL   SUBPROG.` in column 1,
    // where it drops `CALL` and reads `SUBPROG.` as a header) leaves real text in the boundary
    // line's sequence area; a genuine fixed-format following header has blank or digit columns 1-6
    // there. Require blank-or-digit columns 1-6 on the boundary line, else keep the paragraph real.
    if ended_by_header {
        let boundary = line_start(end);
        let boundary_end = src[boundary..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(src.len(), |i| boundary + i);
        let prefix = String::from_utf8_lossy(&src[boundary..boundary_end]);
        if !prefix
            .chars()
            .take(6)
            .all(|c| c == ' ' || c.is_ascii_digit())
        {
            return false;
        }
    }
    let header_end_col = header.end_byte().min(src.len()) - line_begin;
    let region = String::from_utf8_lossy(&src[line_begin..region_end]);
    for (idx, raw) in region.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let body = line.as_bytes();
        // Fixed-format shape: columns 1-6 are blank or digits (sequence area).
        if !body
            .iter()
            .take(6)
            .all(|&c| c == b' ' || c.is_ascii_digit())
        {
            return false;
        }
        // Comment line: indicator column 7 is `*` or `/`.
        if matches!(body.get(6), Some(b'*') | Some(b'/')) {
            continue;
        }
        // Any other indicator (`D` debug, `-` continuation) is not plain exit-only source.
        if body.get(6).is_some_and(|&c| c != b' ') {
            return false;
        }
        // Columns 8-72; on the header line, only what follows the header itself.
        let from = if idx == 0 { header_end_col.max(7) } else { 7 };
        let to = 72.min(body.len());
        // A floating `*>` comment in columns 8-72 consumes the tail too.
        let comment = body[from.min(to)..body.len().min(73)]
            .windows(2)
            .position(|pair| pair == b"*>");
        // Columns 73+: blank, or one label word on a card with an identification area. A label
        // never holds a blank, so `GO TO X` there is a statement the grammar dropped.
        let tail = String::from_utf8_lossy(body.get(72..).unwrap_or_default());
        let tail = tail.trim();
        if comment.is_none()
            && !tail.is_empty()
            && (tail.contains(char::is_whitespace)
                || !crate::cobol_links::has_identification_area(body))
        {
            return false;
        }
        let area_end = comment.map_or(to, |p| from.min(to) + p);
        let area = String::from_utf8_lossy(&body[from.min(to)..area_end]);
        let area_norm = area
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_ascii_uppercase();
        if area_norm == "END DECLARATIVES" {
            break;
        }
        if !cobol_area_exit_only(&area) {
            return false;
        }
    }
    true
}

fn error_after_division(scan_root: TsNode<'_>, division_end: usize) -> bool {
    let root = scan_root;
    let mut stop = usize::MAX;
    let mut errors = Vec::new();
    let mut cursor = root.walk();
    let mut seen = 0usize;
    'walk: loop {
        let n = cursor.node();
        seen += 1;
        if seen > WALK_BUDGET * 64 {
            return true;
        }
        if n.end_byte() >= division_end {
            if n.kind() == "program_definition" && n.start_byte() >= division_end {
                stop = stop.min(n.start_byte());
            }
            if (n.is_error() || n.is_missing()) && n.start_byte() >= division_end {
                errors.push(n.start_byte());
            }
            if cursor.goto_first_child() {
                continue;
            }
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
    errors.into_iter().any(|e| e < stop)
}
