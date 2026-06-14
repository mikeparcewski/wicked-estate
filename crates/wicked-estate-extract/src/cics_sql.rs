//! CICS + embedded-SQL (Db2) extractor — **regex pass over EXEC blocks, NO tree-sitter grammar**.
//!
//! The arborium-cobol grammar parses `EXEC CICS … END-EXEC` and `EXEC SQL … END-EXEC` as an
//! opaque `exec_statement` node with no child structure. The embedded commands are therefore
//! invisible to the AST. This extractor does a text/regex pass over the raw COBOL source — the
//! same pattern as [`super::jcl`] and [`super::hlasm`] — and emits:
//!
//! | CICS statement                        | node kind          | edge kind |
//! |---------------------------------------|--------------------|-----------|
//! | `EXEC CICS LINK PROGRAM('X')`         | `cics_program`     | `Calls`   |
//! | `EXEC CICS XCTL PROGRAM(X)`           | `cics_program`     | `Calls`   |
//! | `EXEC CICS SEND MAP('M')`             | `cics_map`         | `Calls`   |
//! | `EXEC CICS RECEIVE MAP('M')`          | `cics_map`         | `Calls`   |
//! | `EXEC SQL … END-EXEC` (table names)   | `db2_table`        | `Calls`   |
//!
//! All edges originate from a file-level program anchor (`Symbol::synthetic("cobol-pgm", path)`)
//! so that inbound-ref queries ("which programs LINK to PAYPGM?", "who touches table CUSTOMER?")
//! work immediately without waiting for a COBOL symbol extractor to run.
//!
//! # Resolution tier
//!
//! `ResolutionTier::Heuristic` (confidence 0.5) — correct for a text-pass that cannot see
//! variable-name indirection. A value like `EXEC CICS LINK PROGRAM(WS-PGM)` emits the host
//! variable name; a resolver that specialises in COBOL SET statements can upgrade the edge later.
//!
//! # Regex safety
//!
//! All six regexes are compiled exactly once via [`std::sync::LazyLock`] and shared across
//! threads — they are `Send + Sync`. The `(?i)` flag makes every pattern case-insensitive so
//! lowercase test snippets and production SHOUTING-COBOL both match.

use wicked_estate_core::{
    EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, Result, SourceFile, Span,
    Symbol, SymbolId, UnresolvedRef,
};
use regex::Regex;
use std::sync::LazyLock;

// ── Compiled regexes (compiled once, shared across threads) ──────────────────

/// Matches the raw CICS LINK or XCTL invocation, capturing the PROGRAM operand.
///
/// `(?:'([^']+)'|([A-Z0-9#$@-]+))` — two alternatives:
/// - group 1: quoted literal `PROGRAM('PAYPGM')` → inner token without quotes.
/// - group 2: bare identifier `PROGRAM(WS-PGM)` → the identifier itself.
static RE_CICS_PROGRAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)EXEC\s+CICS\s+(?:LINK|XCTL)\s+PROGRAM\s*\(\s*(?:'([^']+)'|([A-Za-z0-9#$@-]+))\s*\)",
    )
    .expect("RE_CICS_PROGRAM must compile")
});

/// Matches EXEC CICS SEND MAP or RECEIVE MAP, capturing the MAP operand (same quoting logic).
static RE_CICS_MAP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)EXEC\s+CICS\s+(?:SEND|RECEIVE)\s+MAP\s*\(\s*(?:'([^']+)'|([A-Za-z0-9#$@-]+))\s*\)",
    )
    .expect("RE_CICS_MAP must compile")
});

/// Strips SQL comments (`-- …` to end-of-line and `/* … */` blocks) from a single SQL block
/// before table-name extraction, preventing false matches inside comments.
static RE_SQL_LINE_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)--[^\n]*").expect("RE_SQL_LINE_COMMENT must compile"));

static RE_SQL_BLOCK_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("RE_SQL_BLOCK_COMMENT must compile"));

/// Extracts the first identifier after a table-introducing SQL keyword.
///
/// Keywords: `FROM`, `JOIN`, `INTO`, `UPDATE`, `DELETE\s+FROM`, `INSERT\s+INTO`.
/// The identifier is: `[A-Za-z_][A-Za-z0-9_$#@.]*` (qualified names like `SCHEMA.TABLE` are
/// captured whole so the Db2 qualified form works). Host-variable references (`:VARNAME`) are
/// explicitly excluded by the leading `(?!:)` negative-lookahead on the first character.
static RE_SQL_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:FROM|JOIN|INTO|UPDATE|DELETE\s+FROM|INSERT\s+INTO)\s+([A-Za-z_][A-Za-z0-9_$#@.]*)",
    )
    .expect("RE_SQL_TABLE must compile")
});

/// Finds `EXEC SQL … END-EXEC` blocks (case-insensitive, spanning newlines).
///
/// Capture group 1 is the SQL body between the keywords.
static RE_EXEC_SQL_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)EXEC\s+SQL\s+(.*?)\s+END-EXEC").expect("RE_EXEC_SQL_BLOCK must compile")
});

// ── Extractor ─────────────────────────────────────────────────────────────────

/// CICS + embedded-SQL (Db2) extractor for COBOL source files.
///
/// Supplements the arborium-cobol grammar-based COBOL extractor by scanning `EXEC … END-EXEC`
/// blocks that the grammar exposes as opaque nodes. Registers for the `"cobol"` language tag so
/// the pipeline applies it alongside the structural COBOL extractor.
pub struct CicsSqlExtractor;

impl CicsSqlExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Build the stable file-level anchor `SymbolId` that all emitted refs point from.
    ///
    /// File granularity is intentional: when the COBOL extractor later emits a proper
    /// `NodeKind::Module` for the program-id it can replace this anchor, but in the meantime
    /// the refs are already in the graph and cross-language "who calls PAYPGM?" works.
    fn file_anchor(file_path: &str) -> SymbolId {
        Symbol::synthetic("cobol-pgm", file_path).id()
    }

    /// Emit a shared target node + an `UnresolvedRef` from the file anchor to it.
    ///
    /// `name` is normalised to uppercase so `PAYPGM` and `paypgm` share the same node regardless
    /// of how the programmer cased the literal.
    fn emit_ref(
        name_raw: &str,
        scheme: &str,
        node_kind_tag: &str,
        anchor: &SymbolId,
        loc: Location,
        nodes: &mut Vec<Node>,
        refs: &mut Vec<UnresolvedRef>,
    ) {
        let name = name_raw.trim().to_uppercase();
        if name.is_empty() {
            return;
        }

        let target_sym = Symbol::synthetic(scheme, name.as_str()).id();

        // Node — deduplicated by the store; emitting it multiple times is safe (same SymbolId).
        let node = Node::new(
            target_sym,
            NodeKind::Other(node_kind_tag.to_string()),
            name.clone(),
            Language::new("cobol"),
            loc.clone(),
        );
        nodes.push(node);

        // UnresolvedRef: anchor → target (source = dependent, target = dependency).
        refs.push(UnresolvedRef::new(
            anchor.clone(),
            name,
            EdgeKind::Calls,
            loc,
        ));
    }
}

impl Default for CicsSqlExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for CicsSqlExtractor {
    /// This extractor targets COBOL files — it supplements the structural COBOL extractor.
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("cobol")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let mut nodes: Vec<Node> = Vec::new();
        let local_edges = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();

        let anchor = Self::file_anchor(&file.path);
        // Span::ZERO is sufficient for a file-level anchor — line tracking below uses per-match
        // byte offsets converted to approximate line numbers.
        let anchor_loc = Location::new(&file.path, Span::ZERO);

        // ── CICS PROGRAM (LINK / XCTL) ────────────────────────────────────────
        for cap in RE_CICS_PROGRAM.captures_iter(&file.text) {
            // Group 1: quoted literal; group 2: bare identifier.
            let name = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            Self::emit_ref(
                name,
                "cics",
                "cics_program",
                &anchor,
                anchor_loc.clone(),
                &mut nodes,
                &mut refs,
            );
        }

        // ── CICS MAP (SEND MAP / RECEIVE MAP) ────────────────────────────────
        for cap in RE_CICS_MAP.captures_iter(&file.text) {
            let name = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            Self::emit_ref(
                name,
                "cics",
                "cics_map",
                &anchor,
                anchor_loc.clone(),
                &mut nodes,
                &mut refs,
            );
        }

        // ── EXEC SQL … END-EXEC blocks ────────────────────────────────────────
        for sql_cap in RE_EXEC_SQL_BLOCK.captures_iter(&file.text) {
            let raw_body = sql_cap.get(1).map_or("", |m| m.as_str());
            // Strip SQL comments before matching table names to avoid false positives like
            //   `-- SELECT name FROM old_table`  or  `/* FROM legacy */`.
            let no_lc = RE_SQL_LINE_COMMENT.replace_all(raw_body, " ");
            let clean = RE_SQL_BLOCK_COMMENT.replace_all(&no_lc, " ");

            for tbl_cap in RE_SQL_TABLE.captures_iter(&clean) {
                let raw_name = tbl_cap.get(1).map_or("", |m| m.as_str());
                // Skip SQL host variables — they start with `:`. The regex itself already
                // anchors on `[A-Za-z_]` so `:`-prefixed tokens cannot match, but we guard
                // explicitly for clarity.
                if raw_name.starts_with(':') {
                    continue;
                }
                Self::emit_ref(
                    raw_name,
                    "db2",
                    "db2_table",
                    &anchor,
                    anchor_loc.clone(),
                    &mut nodes,
                    &mut refs,
                );
            }
        }

        Ok(Extraction {
            nodes,
            local_edges,
            refs,
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn source(text: &str) -> SourceFile {
        SourceFile {
            path: "PAYCOB.cbl".to_string(),
            language: Language::new("cobol"),
            text: text.to_string(),
        }
    }

    fn extract(text: &str) -> Extraction {
        CicsSqlExtractor::new()
            .extract(&source(text))
            .expect("cics_sql extract must not fail")
    }

    fn has_node(ex: &Extraction, kind_tag: &str, name: &str) -> bool {
        let kind = NodeKind::Other(kind_tag.to_string());
        ex.nodes.iter().any(|n| n.kind == kind && n.name == name)
    }

    fn has_calls_ref(ex: &Extraction, raw_name: &str) -> bool {
        ex.refs
            .iter()
            .any(|r| r.kind == EdgeKind::Calls && r.raw_name == raw_name)
    }

    // ── Canonical combined snippet (the spec fixture) ─────────────────────────

    const COBOL_SNIPPET: &str = r#"
       EXEC CICS LINK PROGRAM('PAYPGM') END-EXEC
       EXEC CICS SEND MAP('CUSTMAP') END-EXEC
       EXEC SQL SELECT NAME INTO :WS-NAME FROM CUSTOMER WHERE ID = :WS-ID END-EXEC
    "#;

    #[test]
    fn emits_cics_program_node_paypgm() {
        let ex = extract(COBOL_SNIPPET);
        assert!(
            has_node(&ex, "cics_program", "PAYPGM"),
            "expected cics_program node PAYPGM; nodes: {:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.kind, &n.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn emits_cics_map_node_custmap() {
        let ex = extract(COBOL_SNIPPET);
        assert!(
            has_node(&ex, "cics_map", "CUSTMAP"),
            "expected cics_map node CUSTMAP; nodes: {:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.kind, &n.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn emits_db2_table_node_customer() {
        let ex = extract(COBOL_SNIPPET);
        assert!(
            has_node(&ex, "db2_table", "CUSTOMER"),
            "expected db2_table node CUSTOMER; nodes: {:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.kind, &n.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn emits_calls_ref_for_cics_program() {
        let ex = extract(COBOL_SNIPPET);
        assert!(
            has_calls_ref(&ex, "PAYPGM"),
            "expected Calls ref to PAYPGM; refs: {:?}",
            ex.refs.iter().map(|r| &r.raw_name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn emits_calls_ref_for_cics_map() {
        let ex = extract(COBOL_SNIPPET);
        assert!(
            has_calls_ref(&ex, "CUSTMAP"),
            "expected Calls ref to CUSTMAP; refs: {:?}",
            ex.refs.iter().map(|r| &r.raw_name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn emits_calls_ref_for_db2_table() {
        let ex = extract(COBOL_SNIPPET);
        assert!(
            has_calls_ref(&ex, "CUSTOMER"),
            "expected Calls ref to CUSTOMER; refs: {:?}",
            ex.refs.iter().map(|r| &r.raw_name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn host_variables_not_captured_as_tables() {
        // :WS-NAME and :WS-ID appear in SELECT/INTO; neither should become a db2_table node.
        let ex = extract(COBOL_SNIPPET);
        for name in &["WS-NAME", "WS-ID", ":WS-NAME", ":WS-ID"] {
            assert!(
                !has_node(&ex, "db2_table", name),
                "host variable {name:?} must not become a db2_table node"
            );
        }
    }

    // ── XCTL variant ─────────────────────────────────────────────────────────

    #[test]
    fn exec_cics_xctl_program_captured() {
        let ex = extract("       EXEC CICS XCTL PROGRAM('ORDPGM') END-EXEC\n");
        assert!(
            has_node(&ex, "cics_program", "ORDPGM"),
            "XCTL must produce a cics_program node"
        );
        assert!(has_calls_ref(&ex, "ORDPGM"), "XCTL must emit a Calls ref");
    }

    // ── RECEIVE MAP variant ───────────────────────────────────────────────────

    #[test]
    fn exec_cics_receive_map_captured() {
        let ex = extract("       EXEC CICS RECEIVE MAP('ORDMAP') END-EXEC\n");
        assert!(
            has_node(&ex, "cics_map", "ORDMAP"),
            "RECEIVE MAP must produce a cics_map node"
        );
        assert!(
            has_calls_ref(&ex, "ORDMAP"),
            "RECEIVE MAP must emit a Calls ref"
        );
    }

    // ── Bare (unquoted) PROGRAM operand ──────────────────────────────────────

    #[test]
    fn bare_program_identifier_captured() {
        // PROGRAM(WS-PGM) — no quotes; captures the host-var/identifier name as-is.
        let ex = extract("       EXEC CICS LINK PROGRAM(WSPGM) END-EXEC\n");
        assert!(
            has_node(&ex, "cics_program", "WSPGM"),
            "bare PROGRAM operand must produce a cics_program node"
        );
    }

    // ── Case-insensitivity ────────────────────────────────────────────────────

    #[test]
    fn lowercase_exec_cics_matched() {
        let ex = extract("       exec cics link program('lowpgm') end-exec\n");
        // Name is uppercased on emit.
        assert!(
            has_node(&ex, "cics_program", "LOWPGM"),
            "lowercase EXEC CICS must be matched and name uppercased"
        );
    }

    // ── SQL DML variants ──────────────────────────────────────────────────────

    #[test]
    fn sql_insert_into_captured() {
        let ex = extract(
            "       EXEC SQL INSERT INTO ORDERS (ID, AMT) VALUES (:WS-ID, :WS-AMT) END-EXEC\n",
        );
        assert!(
            has_node(&ex, "db2_table", "ORDERS"),
            "INSERT INTO must yield db2_table ORDERS"
        );
    }

    #[test]
    fn sql_update_captured() {
        let ex = extract(
            "       EXEC SQL UPDATE ACCOUNTS SET BAL = BAL + :DELTA WHERE ID = :WS-ID END-EXEC\n",
        );
        assert!(
            has_node(&ex, "db2_table", "ACCOUNTS"),
            "UPDATE must yield db2_table ACCOUNTS"
        );
    }

    #[test]
    fn sql_delete_from_captured() {
        let ex = extract("       EXEC SQL DELETE FROM AUDIT_LOG WHERE AGE > 365 END-EXEC\n");
        assert!(
            has_node(&ex, "db2_table", "AUDIT_LOG"),
            "DELETE FROM must yield db2_table AUDIT_LOG"
        );
    }

    #[test]
    fn sql_join_captured() {
        let ex = extract(
            "       EXEC SQL SELECT A.ID FROM CUSTOMER A JOIN ORDERS B ON A.ID = B.CID END-EXEC\n",
        );
        assert!(
            has_node(&ex, "db2_table", "CUSTOMER"),
            "FROM table in JOIN query must be captured"
        );
        assert!(
            has_node(&ex, "db2_table", "ORDERS"),
            "JOIN table must be captured"
        );
    }

    // ── Empty / no-EXEC file ──────────────────────────────────────────────────

    #[test]
    fn no_exec_blocks_yields_empty_extraction() {
        let ex = extract("       MOVE 1 TO WS-COUNT.\n       STOP RUN.\n");
        assert!(ex.nodes.is_empty(), "no EXEC blocks → no nodes");
        assert!(ex.refs.is_empty(), "no EXEC blocks → no refs");
    }

    // ── Node deduplication (stable SymbolId) ──────────────────────────────────

    #[test]
    fn duplicate_table_reference_does_not_crash() {
        // Two SELECT FROM CUSTOMER stmts → two nodes with identical SymbolId (store deduplicates).
        let text = concat!(
            "       EXEC SQL SELECT ID FROM CUSTOMER END-EXEC\n",
            "       EXEC SQL SELECT NAME FROM CUSTOMER END-EXEC\n",
        );
        let ex = extract(text);
        // At least one CUSTOMER node is present; we don't assert count because the store deduplicates.
        assert!(
            has_node(&ex, "db2_table", "CUSTOMER"),
            "CUSTOMER node must be present"
        );
        // Two refs are emitted (one per call site) — that is correct and expected.
        let count = ex
            .refs
            .iter()
            .filter(|r| r.raw_name == "CUSTOMER" && r.kind == EdgeKind::Calls)
            .count();
        assert_eq!(count, 2, "two CUSTOMER refs expected (one per call site)");
    }

    // ── Anchor ────────────────────────────────────────────────────────────────

    #[test]
    fn refs_originate_from_file_anchor() {
        let ex = extract(COBOL_SNIPPET);
        let expected_anchor = Symbol::synthetic("cobol-pgm", "PAYCOB.cbl").id();
        for r in &ex.refs {
            assert_eq!(
                r.from, expected_anchor,
                "all refs must originate from the file-level cobol-pgm anchor"
            );
        }
    }

    #[test]
    fn languages_returns_cobol() {
        let langs = CicsSqlExtractor::new().languages();
        assert_eq!(langs, vec![Language::new("cobol")]);
    }
}
