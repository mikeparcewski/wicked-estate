//! COBOL graph completeness: the parse reaches the end of the file, PERFORM THRU and GO TO
//! become calls, and calls and data uses start from the paragraph that makes them.
//!
//! The parse-stop fixtures are the minimal lines from AWS CardDemo (Apache-2.0) programs whose
//! graph used to end partway through the file: arborium-cobol's external scanner turned any one
//! statement the grammar cannot parse into an ERROR node holding the rest of the file. Each
//! fixture is: a paragraph, the trigger, then two paragraphs that must still get nodes.

use wicked_estate_core::completeness;
use wicked_estate_core::{EdgeKind, Extraction, Extractor, Language, Node, NodeKind, SourceFile};
use wicked_estate_extract::CicsSqlExtractor;

/// The index pipeline's COBOL extraction: the tree-sitter extractor plus the COBOL text supplement.
fn extract(name: &str, text: &str) -> Extraction {
    CicsSqlExtractor::new()
        .extract(&SourceFile {
            path: format!("{name}.cbl"),
            language: Language::new("cobol"),
            text: text.to_string(),
        })
        .expect("cobol extraction")
}

/// A fixed-format program: `body` lines are the PROCEDURE DIVISION, written from column 8.
fn program(id: &str, data: &[&str], body: &[&str]) -> String {
    let mut out = vec![
        "       IDENTIFICATION DIVISION.".to_string(),
        format!("       PROGRAM-ID. {id}."),
        "       DATA DIVISION.".to_string(),
        "       WORKING-STORAGE SECTION.".to_string(),
    ];
    out.extend(data.iter().map(|l| format!("       {l}")));
    out.push("       PROCEDURE DIVISION.".to_string());
    out.extend(body.iter().map(|l| format!("       {l}")));
    out.join("\n") + "\n"
}

const DATA: &[&str] = &[
    "01  WS-A          PIC X(10).",
    "01  WS-B          PIC X(10).",
    "01  WS-FLAG       PIC X VALUE 'N'.",
    "01  WS-DATE       PIC X(10).",
];

/// Comment lines (indicator `*` or `/` in column 7) are trivia for the completeness check.
fn cobol_trivia(line: &str) -> bool {
    matches!(line.as_bytes().get(6), Some(b'*') | Some(b'/'))
}

fn function_names(ex: &Extraction) -> Vec<&str> {
    ex.nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect()
}

fn node<'a>(ex: &'a Extraction, name: &str) -> &'a Node {
    ex.nodes
        .iter()
        .find(|n| n.name == name && n.kind != NodeKind::File)
        .unwrap_or_else(|| panic!("no node {name}; nodes: {:?}", function_names(ex)))
}

/// Every paragraph has a node, the paragraphs tile the PROCEDURE DIVISION, and the program
/// module ends on the last line.
fn assert_covered_to_eof(name: &str, text: &str, paragraphs: &[&str]) -> Extraction {
    let ex = extract(name, text);
    assert_eq!(function_names(&ex), paragraphs, "{name}: paragraph nodes");
    let last_line = text.lines().count() as u32 - 1;
    assert_eq!(
        node(&ex, name).location.span.end_line,
        last_line,
        "{name}: the program module must end on the last line"
    );
    let paras = ex.nodes.iter().filter(|n| n.kind == NodeKind::Function);
    let c = completeness::check(text, paras, cobol_trivia);
    assert!(c.is_complete(), "{name}: {c:?}");
    ex
}

/// The distinct targets of `kind` refs from a node, sorted (edges are a set per source).
fn targets(ex: &Extraction, from: &str, kind: EdgeKind) -> Vec<String> {
    let sym = &node(ex, from).symbol;
    let mut out: Vec<String> = ex
        .refs
        .iter()
        .filter(|r| &r.from == sym && r.kind == kind)
        .map(|r| r.raw_name.clone())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The names a node `Contains` (local edges), sorted.
fn contained_by(ex: &Extraction, from: &str) -> Vec<String> {
    let sym = &node(ex, from).symbol;
    let mut out: Vec<String> = ex
        .local_edges
        .iter()
        .filter(|e| &e.source == sym && e.kind == EdgeKind::Contains)
        .filter_map(|e| ex.nodes.iter().find(|n| n.symbol == e.target))
        .map(|n| n.name.clone())
        .collect();
    out.sort();
    out
}

fn calls_from(ex: &Extraction, from: &str) -> Vec<String> {
    targets(ex, from, EdgeKind::Calls)
}

fn refs_from(ex: &Extraction, from: &str) -> Vec<String> {
    targets(ex, from, EdgeKind::References)
}

// ── parse stops: one fixture per trigger shape ───────────────────────────────────────────────

#[test]
fn not_equal_without_a_space_does_not_stop_the_parse() {
    // CBACT04C.cbl:194, CBTRN03C.cbl:181. The grammar's NOT-EQUAL token needs whitespace.
    let text = program(
        "CBACT04C",
        DATA,
        &[
            "MAIN-PARA.",
            "    IF  WS-A NOT= WS-B",
            "        PERFORM 1050-UPDATE",
            "    END-IF",
            "    GOBACK.",
            "1050-UPDATE.",
            "    MOVE WS-A TO WS-B.",
            "9999-ABEND.",
            "    DISPLAY 'ABENDING PROGRAM'.",
        ],
    );
    let ex = assert_covered_to_eof(
        "CBACT04C",
        &text,
        &["MAIN-PARA", "1050-UPDATE", "9999-ABEND"],
    );
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["1050-UPDATE"]);
}

#[test]
fn exec_cics_after_an_open_perform_thru_does_not_stop_the_parse() {
    // COPAUA0C.cbl:220-226: `PERFORM … THRU …` with no period, then EXEC CICS RETURN.
    let text = program(
        "COPAUA0C",
        DATA,
        &[
            "MAIN-PARA.",
            "    PERFORM 1000-INITIALIZE    THRU 1000-EXIT",
            "",
            "    EXEC CICS RETURN",
            "    END-EXEC.",
            "1000-INITIALIZE.",
            "    MOVE SPACES TO WS-A.",
            "1000-EXIT.",
            "    EXIT.",
        ],
    );
    assert_covered_to_eof(
        "COPAUA0C",
        &text,
        &["MAIN-PARA", "1000-INITIALIZE", "1000-EXIT"],
    );
}

#[test]
fn exec_cics_after_an_open_perform_and_a_comment_block_does_not_stop_the_parse() {
    // COACCT01.cbl:187-191 / CODATE01.cbl:136-140 (sequence numbers in columns 1-6 and 73-80),
    // COADM01C.cbl:279-280: `PERFORM X` with no period, then EXEC CICS.
    let cards = [
        "000100 IDENTIFICATION DIVISION.",
        "000200 PROGRAM-ID. COACCT01.",
        "000300 DATA DIVISION.",
        "000400 WORKING-STORAGE SECTION.",
        "000500 01  WS-A          PIC X(10).",
        "000600 PROCEDURE DIVISION.",
        "014500 1000-CONTROL.",
        "014600     PERFORM 2100-OPEN-ERROR-QUEUE",
        "015300******************************************************************",
        "015400* GET THE QUEUE NAME WHICH STARTED THE TRANSACTION               *",
        "015500******************************************************************",
        "015600     EXEC CICS RETRIEVE",
        "015700       INTO(WS-A)",
        "016000     END-EXEC",
        "016100     .",
        "016200 2100-OPEN-ERROR-QUEUE.",
        "016300     MOVE SPACES TO WS-A.",
        "016400 9000-TERMINATE.",
        "016500     DISPLAY 'END'.",
    ];
    let text = cards
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c:<72}{:08}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_covered_to_eof(
        "COACCT01",
        &text,
        &["1000-CONTROL", "2100-OPEN-ERROR-QUEUE", "9000-TERMINATE"],
    );
}

#[test]
fn copy_replacing_pseudo_text_in_the_procedure_division_does_not_stop_the_parse() {
    // COACTUPC.cbl:3208-3211, COTRTUPC.cbl:1358-1361.
    let text = program(
        "COACTUPC",
        DATA,
        &[
            "3300-SETUP-SCREEN-ATTRS.",
            "    COPY CSSETATY REPLACING",
            "      ==(TESTVAR1)== BY ==ACCT-STATUS==",
            "      ==(SCRNVAR2)== BY ==ACSTTUS==",
            "      ==(MAPNAME3)== BY ==CACTUPA== .",
            "    MOVE WS-A TO WS-B.",
            "3300-SETUP-SCREEN-ATTRS-EXIT.",
            "    EXIT.",
            "3400-SEND-SCREEN.",
            "    MOVE WS-B TO WS-A.",
        ],
    );
    assert_covered_to_eof(
        "COACTUPC",
        &text,
        &[
            "3300-SETUP-SCREEN-ATTRS",
            "3300-SETUP-SCREEN-ATTRS-EXIT",
            "3400-SEND-SCREEN",
        ],
    );
}

#[test]
fn entry_using_does_not_stop_the_parse() {
    // DBUNLDGS.CBL:164-167, PAUDBLOD.CBL:169-171, PAUDBUNL.CBL:157-158.
    let text = program(
        "DBUNLDGS",
        DATA,
        &[
            "MAIN-PARA.",
            "     ENTRY 'DLITCBL'                 USING WS-A",
            "                                           WS-B.",
            "     PERFORM 1000-INITIALIZE.",
            "     GOBACK.",
            "1000-INITIALIZE.",
            "     MOVE SPACES TO WS-A.",
            "9999-ABEND.",
            "     DISPLAY 'ABEND'.",
        ],
    );
    assert_covered_to_eof(
        "DBUNLDGS",
        &text,
        &["MAIN-PARA", "1000-INITIALIZE", "9999-ABEND"],
    );
}

#[test]
fn function_reference_modification_does_not_stop_the_parse() {
    // CBIMPORT.cbl:178.
    let text = program(
        "CBIMPORT",
        DATA,
        &[
            "1000-INITIALIZE.",
            "    MOVE FUNCTION CURRENT-DATE(1:4) TO WS-DATE(1:4)",
            "    MOVE '-' TO WS-DATE(5:1).",
            "2000-PROCESS.",
            "    MOVE WS-A TO WS-B.",
            "4000-FINALIZE.",
            "    DISPLAY 'DONE'.",
        ],
    );
    assert_covered_to_eof(
        "CBIMPORT",
        &text,
        &["1000-INITIALIZE", "2000-PROCESS", "4000-FINALIZE"],
    );
}

#[test]
fn a_copy_of_a_quoted_member_between_paragraphs_does_not_stop_the_parse() {
    // COACTVWC.cbl:913-916, COCRDLIC.cbl:1416-1422, COTRTLIC.cbl:2064-2070.
    let text = program(
        "COACTVWC",
        DATA,
        &[
            "SEND-LONG-TEXT-EXIT.",
            "    EXIT",
            "    .",
            "*****************************************************************",
            "*Common code to store PFKey",
            "******************************************************************",
            "COPY 'CSSTRPFY'",
            "    .",
            "",
            "ABEND-ROUTINE.",
            "    MOVE WS-A TO WS-B.",
            "ABEND-ROUTINE-EXIT.",
            "    EXIT.",
        ],
    );
    let ex = extract("COACTVWC", &text);
    // The COPY sits between paragraphs, so the paragraph before it holds it: the check is that
    // both paragraphs after it get nodes and the module reaches the last line.
    assert_eq!(
        function_names(&ex),
        ["SEND-LONG-TEXT-EXIT", "ABEND-ROUTINE", "ABEND-ROUTINE-EXIT"]
    );
    assert_eq!(
        node(&ex, "COACTVWC").location.span.end_line,
        text.lines().count() as u32 - 1
    );
}

// ── PERFORM THRU and GO TO ───────────────────────────────────────────────────────────────────

const THRU: &[&str] = &[
    "MAIN-PARA.",
    "    PERFORM 1000-A THRU 1000-EXIT",
    "    PERFORM 2000-B",
    "       THRU 2000-EXIT",
    "    GOBACK.",
    "1000-A.",
    "    IF WS-FLAG = 'Y'",
    "        GO TO 1000-EXIT",
    "    END-IF",
    "    MOVE WS-A TO WS-B.",
    "1000-B.",
    "    MOVE WS-B TO WS-A.",
    "1000-EXIT.",
    "    EXIT.",
    "2000-B.",
    "    GO TO 2000-X 2000-EXIT DEPENDING ON WS-FLAG.",
    "2000-X.",
    "    MOVE SPACES TO WS-A.",
    "2000-EXIT.",
    "    EXIT.",
];

#[test]
fn perform_thru_calls_each_paragraph_from_the_first_through_the_last_in_order() {
    let ex = extract("THRUPGM", &program("THRUPGM", DATA, THRU));
    assert_eq!(
        calls_from(&ex, "MAIN-PARA"),
        // `THRU` on the next line is the same statement.
        [
            "1000-A",
            "1000-B",
            "1000-EXIT",
            "2000-B",
            "2000-EXIT",
            "2000-X"
        ],
    );
    // Nothing parks under the old "X THRU Y" name.
    assert!(
        !ex.refs.iter().any(|r| r.raw_name.contains("THRU")),
        "{:?}",
        ex.refs.iter().map(|r| &r.raw_name).collect::<Vec<_>>()
    );
}

#[test]
fn perform_thru_with_an_unknown_end_calls_the_named_paragraphs_only() {
    let ex = extract(
        "THRUBAD",
        &program(
            "THRUBAD",
            DATA,
            &[
                "MAIN-PARA.",
                "    PERFORM 1000-A THRU 9999-NOWHERE.",
                "1000-A.",
                "    EXIT.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["1000-A", "9999-NOWHERE"]);
}

#[test]
fn go_to_targets_become_calls_from_the_paragraph() {
    let ex = extract("THRUPGM", &program("THRUPGM", DATA, THRU));
    assert_eq!(calls_from(&ex, "1000-A"), ["1000-EXIT"]);
    assert_eq!(calls_from(&ex, "2000-B"), ["2000-EXIT", "2000-X"]);
}

// ── attribution to the paragraph ─────────────────────────────────────────────────────────────

#[test]
fn calls_and_data_uses_start_from_the_paragraph_that_makes_them() {
    let ex = extract("THRUPGM", &program("THRUPGM", DATA, THRU));
    assert!(
        calls_from(&ex, "THRUPGM").is_empty(),
        "no call starts at the program"
    );
    assert_eq!(refs_from(&ex, "1000-A"), ["WS-A", "WS-B", "WS-FLAG"]);
    assert_eq!(refs_from(&ex, "1000-B"), ["WS-A", "WS-B"]);
    assert_eq!(refs_from(&ex, "2000-B"), ["WS-FLAG"]);
    // Uses in the DATA DIVISION stay where they were.
    assert!(refs_from(&ex, "THRUPGM").is_empty());
    // The program contains its paragraphs, so it stays in their callees' and fields' blast radius.
    assert_eq!(
        contained_by(&ex, "THRUPGM"),
        [
            "1000-A",
            "1000-B",
            "1000-EXIT",
            "2000-B",
            "2000-EXIT",
            "2000-X",
            "MAIN-PARA"
        ]
    );
}

#[test]
fn a_section_spans_its_paragraphs_and_the_paragraph_still_owns_its_calls() {
    let text = program(
        "SECTPGM",
        DATA,
        &[
            "MAIN-SECTION SECTION.",
            "    PERFORM WORK-PARA.",
            "WORK-PARA.",
            "    MOVE WS-A TO WS-B.",
            "WORK-EXIT.",
            "    EXIT.",
            "LAST-SECTION SECTION.",
            "LAST-PARA.",
            "    DISPLAY 'DONE'.",
        ],
    );
    let ex = extract("SECTPGM", &text);
    let line = |name: &str| text.lines().position(|l| l.trim() == name).unwrap() as u32;
    let main = node(&ex, "MAIN-SECTION").location.span;
    assert_eq!(main.start_line, line("MAIN-SECTION SECTION."));
    assert_eq!(main.end_line, line("LAST-SECTION SECTION.") - 1);
    assert_eq!(
        node(&ex, "LAST-SECTION").location.span.end_line,
        text.lines().count() as u32 - 1
    );
    // Statements before the first paragraph belong to the section; inside a paragraph, to it.
    assert_eq!(calls_from(&ex, "MAIN-SECTION"), ["WORK-PARA"]);
    assert_eq!(refs_from(&ex, "WORK-PARA"), ["WS-A", "WS-B"]);
    // Program → section → paragraph.
    assert_eq!(
        contained_by(&ex, "SECTPGM"),
        ["LAST-SECTION", "MAIN-SECTION"]
    );
    assert_eq!(
        contained_by(&ex, "MAIN-SECTION"),
        ["WORK-EXIT", "WORK-PARA"]
    );
    assert_eq!(contained_by(&ex, "LAST-SECTION"), ["LAST-PARA"]);
}

#[test]
fn cics_and_sql_calls_and_data_uses_start_from_the_paragraph() {
    let text = program(
        "LINKPGM",
        DATA,
        &[
            "MAIN-PARA.",
            "    PERFORM 1000-LINK.",
            "    GOBACK.",
            "1000-LINK.",
            "    EXEC CICS LINK PROGRAM('SUBPGM') COMMAREA(WS-A) END-EXEC",
            "    EXEC SQL SELECT C INTO :WS-B FROM T1 END-EXEC.",
        ],
    );
    let ex = extract("LINKPGM", &text);
    assert_eq!(calls_from(&ex, "1000-LINK"), ["SUBPGM"]);
    assert_eq!(refs_from(&ex, "1000-LINK"), ["WS-A", "WS-B"]);
    assert!(calls_from(&ex, "LINKPGM").is_empty());
    assert!(refs_from(&ex, "LINKPGM").is_empty());
}

#[test]
fn recovery_built_headers_are_not_paragraphs_but_a_header_without_a_period_is() {
    // DBUNLDGS.CBL:165-167: error recovery made headers named `ENTRY` and `PASFLPCB` out of the
    // ENTRY statement. COTRTLIC.cbl:1274-1275: `2000-SEND-MAP` has its period on the next line
    // and is a paragraph.
    let text = program(
        "DBUNLDGS",
        DATA,
        &[
            "MAIN-PARA.",
            "     ENTRY 'DLITCBL'                 USING WS-A",
            "                                           WS-B",
            "                                           WS-FLAG.",
            "     PERFORM 2000-SEND-MAP.",
            "2000-SEND-MAP",
            "     .",
            "     MOVE WS-A TO WS-B.",
            "2000-SEND-MAP-EXIT.",
            "     EXIT.",
        ],
    );
    assert_covered_to_eof(
        "DBUNLDGS",
        &text,
        &["MAIN-PARA", "2000-SEND-MAP", "2000-SEND-MAP-EXIT"],
    );
}
