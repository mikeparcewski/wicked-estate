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

/// The name of the node with symbol `sym`.
fn name_of<'a>(ex: &'a Extraction, sym: &wicked_estate_core::SymbolId) -> &'a str {
    ex.nodes
        .iter()
        .find(|n| &n.symbol == sym)
        .map_or("?", |n| n.name.as_str())
}

/// The distinct targets of `kind` refs and local edges from a node, sorted (edges are a set per
/// source). A ref names its target; a local edge (a program running its paragraph) binds it.
fn targets(ex: &Extraction, from: &str, kind: EdgeKind) -> Vec<String> {
    let sym = &node(ex, from).symbol;
    let mut out: Vec<String> = ex
        .refs
        .iter()
        .filter(|r| &r.from == sym && r.kind == kind)
        .map(|r| r.raw_name.clone())
        .chain(
            ex.local_edges
                .iter()
                .filter(|e| &e.source == sym && e.kind == kind)
                .map(|e| name_of(ex, &e.target).to_string()),
        )
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
    // 1000-A's GO TO is under an IF, so 1000-A also falls into 1000-B (#585).
    assert_eq!(calls_from(&ex, "1000-A"), ["1000-B", "1000-EXIT"]);
    assert_eq!(calls_from(&ex, "2000-B"), ["2000-EXIT", "2000-X"]);
}

// ── attribution to the paragraph ─────────────────────────────────────────────────────────────

#[test]
fn calls_and_data_uses_start_from_the_paragraph_that_makes_them() {
    let ex = extract("THRUPGM", &program("THRUPGM", DATA, THRU));
    // The program's one call is its entry: it runs its first paragraph.
    assert_eq!(calls_from(&ex, "THRUPGM"), ["MAIN-PARA"]);
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
    // The LINK is the paragraph's only: the program's one call is its entry.
    assert_eq!(calls_from(&ex, "LINKPGM"), ["MAIN-PARA"]);
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

// ── round-1 review folds ─────────────────────────────────────────────────────────────────────

#[test]
fn a_copy_without_its_own_period_hides_only_copy_and_member() {
    // A COPY with no period of its own (a compile error) must not run on to the next period: that
    // one ends the next paragraph's header, and blanking up to it lost the paragraph and gave its
    // CALL to the paragraph before.
    let text = program(
        "COPYNOP",
        DATA,
        &[
            "MAIN-PARA.",
            "    MOVE WS-A TO WS-B",
            "    COPY CSUTLDPY",
            "AFTER-PARA.",
            "    CALL 'SUBPGM'.",
        ],
    );
    let ex = extract("COPYNOP", &text);
    assert_eq!(function_names(&ex), ["MAIN-PARA", "AFTER-PARA"]);
    assert_eq!(calls_from(&ex, "AFTER-PARA"), ["SUBPGM"]);
    // No call from the hidden COPY; MAIN-PARA falls into AFTER-PARA (#585).
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["AFTER-PARA"]);
}

#[test]
fn a_copy_statement_ends_at_its_own_period_through_of_suppress_and_replacing() {
    let text = program(
        "COPYOPS",
        DATA,
        &[
            "MAIN-PARA.",
            "    COPY CSUTLDPY OF SYSLIB SUPPRESS",
            "         REPLACING ==:TAG:== BY ==WS==",
            "                   LEADING ==X== BY ==Y==",
            "                   OLD-NAME BY NEW-NAME.",
            "NEXT-PARA.",
            "    CALL 'SUBPGM'.",
        ],
    );
    let ex = extract("COPYOPS", &text);
    assert_eq!(function_names(&ex), ["MAIN-PARA", "NEXT-PARA"]);
    assert_eq!(calls_from(&ex, "NEXT-PARA"), ["SUBPGM"]);
}

/// Two sections that both have paragraphs `A-PARA` and `Z-PARA` (legal: a paragraph name is
/// unique within its section), with a different paragraph between them in each.
fn two_sections(perform: &str) -> String {
    program(
        "QUALPGM",
        DATA,
        &[
            "FIRST-SECTION SECTION.",
            "MAIN-PARA.",
            perform,
            "    GOBACK.",
            "A-PARA.",
            "    EXIT.",
            "M1-PARA.",
            "    EXIT.",
            "Z-PARA.",
            "    EXIT.",
            "SECOND-SECTION SECTION.",
            "A-PARA.",
            "    EXIT.",
            "M2-PARA.",
            "    EXIT.",
            "Z-PARA.",
            "    EXIT.",
        ],
    )
}

#[test]
fn a_qualified_perform_thru_runs_the_range_in_the_named_section() {
    let ex = extract(
        "QUALPGM",
        &two_sections("    PERFORM A-PARA OF SECOND-SECTION THRU Z-PARA IN SECOND-SECTION."),
    );
    assert_eq!(
        calls_from(&ex, "MAIN-PARA"),
        ["A-PARA", "M2-PARA", "Z-PARA"]
    );
}

#[test]
fn an_unqualified_perform_thru_runs_the_range_in_its_own_section() {
    // COBOL: an unqualified paragraph name means the one in the referring paragraph's section.
    let ex = extract("QUALPGM", &two_sections("    PERFORM A-PARA THRU Z-PARA."));
    assert_eq!(
        calls_from(&ex, "MAIN-PARA"),
        ["A-PARA", "M1-PARA", "Z-PARA"]
    );
}

#[test]
fn a_perform_thru_with_an_ambiguous_endpoint_runs_no_guessed_range() {
    // From SECOND-SECTION's own paragraph the endpoints are its own; from a paragraph outside
    // every section, `A-PARA` names two paragraphs, so no range is guessed: only the named ones.
    let text = program(
        "AMBIGPGM",
        DATA,
        &[
            "MAIN-PARA.",
            "    PERFORM A-PARA THRU Z-PARA.",
            "    GOBACK.",
            "ONE SECTION.",
            "A-PARA.",
            "    EXIT.",
            "M1-PARA.",
            "    EXIT.",
            "Z-PARA.",
            "    EXIT.",
            "TWO SECTION.",
            "A-PARA.",
            "    EXIT.",
            "M2-PARA.",
            "    EXIT.",
            "Z-PARA.",
            "    EXIT.",
        ],
    );
    let ex = extract("AMBIGPGM", &text);
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["A-PARA", "Z-PARA"]);
}

#[test]
fn a_jump_to_its_own_paragraph_is_no_call() {
    // Estate stores no self-edges (the resolvers drop self-candidates), so a GO TO or PERFORM of
    // the paragraph it sits in would park, or bind to a same-named paragraph in another program.
    let text = program(
        "LOOPPGM",
        DATA,
        &[
            "LOOP-PARA.",
            "    MOVE WS-A TO WS-B",
            "    IF WS-FLAG = 'Y'",
            "        GO TO LOOP-PARA",
            "    END-IF",
            "    PERFORM DONE-PARA.",
            "DONE-PARA.",
            "    EXIT.",
        ],
    );
    let ex = extract("LOOPPGM", &text);
    assert_eq!(calls_from(&ex, "LOOP-PARA"), ["DONE-PARA"]);
    // The only call to LOOP-PARA is the program's entry, not the GO TO inside it.
    assert_eq!(callers_of(&ex, "LOOP-PARA"), ["LOOPPGM"]);
}

#[test]
fn sort_input_and_output_procedures_call_every_paragraph_of_their_range() {
    let text = program(
        "SORTPGM",
        DATA,
        &[
            "MAIN-PARA.",
            "    SORT SORT-FILE ON ASCENDING KEY WS-A",
            "        INPUT PROCEDURE IS 1000-IN THRU 1000-IN-EXIT",
            "        OUTPUT PROCEDURE 2000-OUT",
            "    GOBACK.",
            "1000-IN.",
            "    MOVE WS-A TO WS-B.",
            "1000-MID.",
            "    MOVE WS-B TO WS-A.",
            "1000-IN-EXIT.",
            "    EXIT.",
            "2000-OUT.",
            "    EXIT.",
        ],
    );
    let ex = extract("SORTPGM", &text);
    assert_eq!(
        calls_from(&ex, "MAIN-PARA"),
        ["1000-IN", "1000-IN-EXIT", "1000-MID", "2000-OUT"]
    );
}

/// Every `Calls` source to `target` (refs by name, local edges by target node name), sorted.
fn callers_of(ex: &Extraction, target: &str) -> Vec<String> {
    let mut out: Vec<String> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls && r.raw_name == target)
        .map(|r| name_of(ex, &r.from).to_string())
        .chain(
            ex.local_edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Calls && name_of(ex, &e.target) == target)
                .map(|e| name_of(ex, &e.source).to_string()),
        )
        .collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn one_call_site_is_one_caller_the_paragraph_that_makes_it() {
    // Corroboration (wicked-core) counts each distinct caller of a program as an independent
    // source: a call stored from the paragraph AND from its program would be one site counted as
    // two. Static CALL, CICS XCTL and a dynamic CALL each start from the paragraph only.
    let text = program(
        "CALLPGM",
        &["01  WS-PGM        PIC X(8) VALUE 'DYNPGM'."],
        &[
            "MAIN-PARA.",
            "    PERFORM 1000-CALL.",
            "    GOBACK.",
            "1000-CALL.",
            "    CALL 'SUBPGM' USING WS-PGM",
            "    CALL WS-PGM",
            "    EXEC CICS XCTL PROGRAM('NEXTPGM') END-EXEC.",
        ],
    );
    let ex = extract("CALLPGM", &text);
    for target in ["SUBPGM", "NEXTPGM", "cobol-dynamic-call:DYNPGM"] {
        assert_eq!(callers_of(&ex, target), ["1000-CALL"], "{target}");
    }
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["1000-CALL"]);
    assert_eq!(calls_from(&ex, "CALLPGM"), ["MAIN-PARA"]);
}

// ── the paragraphs a program runs itself ──────────────────────────────────────────────────────

#[test]
fn a_program_runs_its_first_paragraph_and_only_paragraphs_nothing_else_calls() {
    // The program calls what no flow statement of it names; a PERFORMed paragraph keeps exactly
    // its performer as caller (no call site counts twice).
    let ex = extract(
        "CALLPGM",
        &program(
            "CALLPGM",
            DATA,
            &[
                "MAIN-PARA.",
                "    PERFORM 1000-CALL",
                "    GOBACK.",
                "1000-CALL.",
                "    CALL 'SUBPGM'.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "CALLPGM"), ["MAIN-PARA"]);
    assert_eq!(callers_of(&ex, "MAIN-PARA"), ["CALLPGM"]);
    assert_eq!(callers_of(&ex, "1000-CALL"), ["MAIN-PARA"]);
}

#[test]
fn using_comments_declaratives_and_sections_start_where_the_program_starts() {
    let using = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. USINGPGM.\n       \
                 DATA DIVISION.\n       LINKAGE SECTION.\n       01  LK-A PIC X.\n       \
                 PROCEDURE DIVISION USING LK-A.\n      * the entry\n       MAIN-PARA.\n           \
                 GOBACK.\n";
    assert_eq!(
        calls_from(&extract("USINGPGM", using), "USINGPGM"),
        ["MAIN-PARA"]
    );
    // A DECLARATIVES handler runs on an I/O condition: with no I/O statement on INFILE, nothing
    // runs it, and the program starts after END DECLARATIVES (#585).
    let decl = program(
        "DECLPGM",
        DATA,
        &[
            "DECLARATIVES.",
            "ERR-SECTION SECTION.",
            "    USE AFTER STANDARD ERROR PROCEDURE ON INFILE.",
            "ERR-PARA.",
            "    CALL 'LOGGER'.",
            "END DECLARATIVES.",
            "MAIN-PARA.",
            "    GOBACK.",
        ],
    );
    let ex = extract("DECLPGM", &decl);
    assert_eq!(calls_from(&ex, "DECLPGM"), ["MAIN-PARA"]);
    assert_eq!(callers_of(&ex, "ERR-SECTION"), Vec::<String>::new());
    assert_eq!(callers_of(&ex, "ERR-PARA"), ["ERR-SECTION"]);
    assert_eq!(callers_of(&ex, "LOGGER"), ["ERR-PARA"]);
    let section = program(
        "SECTPGM",
        DATA,
        &[
            "MAIN-SECTION SECTION.",
            "MAIN-PARA.",
            "    PERFORM WORK-SECTION",
            "    GOBACK.",
            "WORK-SECTION SECTION.",
            "WORK-PARA.",
            "    EXIT.",
        ],
    );
    // WORK-SECTION and its paragraph are performed; the program starts at MAIN-SECTION, which
    // falls into MAIN-PARA.
    let ex = extract("SECTPGM", &section);
    assert_eq!(calls_from(&ex, "SECTPGM"), ["MAIN-SECTION"]);
    assert_eq!(callers_of(&ex, "MAIN-PARA"), ["MAIN-SECTION"]);
}

#[test]
fn statements_before_the_first_paragraph_keep_their_own_calls() {
    let ex = extract(
        "TOPPGM",
        &program(
            "TOPPGM",
            DATA,
            &[
                "    PERFORM 2000-B",
                "    GOBACK.",
                "1000-A.",
                "    EXIT.",
                "2000-B.",
                "    EXIT.",
            ],
        ),
    );
    // 2000-B by the PERFORM. The GOBACK ends the run before 1000-A, which nothing names: dead
    // code, so no caller (#585). 1000-A falls into 2000-B.
    assert_eq!(calls_from(&ex, "TOPPGM"), ["2000-B"]);
    assert_eq!(callers_of(&ex, "1000-A"), Vec::<String>::new());
    assert_eq!(callers_of(&ex, "2000-B"), ["1000-A", "TOPPGM"]);
}

#[test]
fn two_programs_in_one_file_each_run_their_own_paragraph() {
    // Bound by symbol, not by name: both programs have a MAIN-PARA.
    let text = [
        "       IDENTIFICATION DIVISION.",
        "       PROGRAM-ID. PGMONE.",
        "       PROCEDURE DIVISION.",
        "       MAIN-PARA.",
        "           CALL 'SUBONE'.",
        "       END PROGRAM PGMONE.",
        "       IDENTIFICATION DIVISION.",
        "       PROGRAM-ID. PGMTWO.",
        "       PROCEDURE DIVISION.",
        "       MAIN-PARA.",
        "           CALL 'SUBTWO'.",
        "       END PROGRAM PGMTWO.",
    ]
    .join("\n")
        + "\n";
    let ex = extract("TWOPGM", &text);
    for (program, sub) in [("PGMONE", "SUBONE"), ("PGMTWO", "SUBTWO")] {
        let sym = &node(&ex, program).symbol;
        let runs: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| &e.source == sym && e.kind == EdgeKind::Calls)
            .map(|e| e.target.clone())
            .collect();
        assert_eq!(runs.len(), 1, "{program}: {runs:?}");
        assert!(
            ex.refs
                .iter()
                .any(|r| r.from == runs[0] && r.raw_name == sub),
            "{program} runs the MAIN-PARA that calls {sub}"
        );
    }
}

#[test]
fn an_unterminated_copy_replacing_does_not_take_the_next_paragraph() {
    let text = program(
        "COPYREP",
        DATA,
        &[
            "MAIN-PARA.",
            "    COPY CSSETATY REPLACING ==A== BY ==B==",
            "AFTER-PARA.",
            "    CALL 'SUBPGM'.",
        ],
    );
    let ex = extract("COPYREP", &text);
    assert_eq!(function_names(&ex), ["MAIN-PARA", "AFTER-PARA"]);
    assert_eq!(calls_from(&ex, "AFTER-PARA"), ["SUBPGM"]);
}

#[test]
fn copy_operands_that_are_literals_end_at_the_copy_period() {
    let text = program(
        "COPYLIT",
        DATA,
        &[
            "MAIN-PARA.",
            "    COPY 'CSUTLDPY' OF 'SYSLIB'.",
            "    COPY CSSETATY REPLACING 'OLD' BY 'NEW'.",
            "NEXT-PARA.",
            "    CALL 'SUBPGM'.",
        ],
    );
    assert_covered_to_eof("COPYLIT", &text, &["MAIN-PARA", "NEXT-PARA"]);
    assert_eq!(
        calls_from(&extract("COPYLIT", &text), "NEXT-PARA"),
        ["SUBPGM"]
    );
}

#[test]
fn an_unterminated_copy_split_over_lines_keeps_every_line_break() {
    let text = program(
        "COPYNL",
        DATA,
        &[
            "MAIN-PARA.",
            "    COPY",
            "        CSUTLDPY",
            "NEXT-PARA.",
            "    CALL 'SUBPGM'.",
        ],
    );
    let ex = extract("COPYNL", &text);
    let line = text.lines().position(|l| l.trim() == "NEXT-PARA.").unwrap() as u32;
    assert_eq!(node(&ex, "NEXT-PARA").location.span.start_line, line);
    let call = ex.refs.iter().find(|r| r.raw_name == "SUBPGM").unwrap();
    assert_eq!(call.location.span.start_line, line + 1);
}

/// MAIN-PARA runs `perform`; sections SEC-A (A-PARA), SEC-B (B1-PARA, B2-PARA), SEC-C (C-PARA).
fn sections(perform: &str) -> Extraction {
    extract(
        "SECTHRU",
        &program(
            "SECTHRU",
            DATA,
            &[
                "MAIN-PARA.",
                perform,
                "    GOBACK.",
                "SEC-A SECTION.",
                "A-PARA.",
                "    EXIT.",
                "SEC-B SECTION.",
                "B1-PARA.",
                "    EXIT.",
                "B2-PARA.",
                "    EXIT.",
                "SEC-C SECTION.",
                "C-PARA.",
                "    EXIT.",
            ],
        ),
    )
}

#[test]
fn a_section_runs_its_paragraphs_however_it_is_performed() {
    // Each statement on its own, so one expansion cannot hide another's miss.
    for (perform, calls) in [
        ("    PERFORM SEC-B", &["B1-PARA", "B2-PARA", "SEC-B"][..]),
        (
            "    PERFORM SEC-B THRU SEC-B",
            &["B1-PARA", "B2-PARA", "SEC-B"][..],
        ),
        (
            "    PERFORM A-PARA THRU SEC-B",
            &["A-PARA", "B1-PARA", "B2-PARA", "SEC-B"][..],
        ),
        ("    PERFORM B1-PARA THRU B1-PARA", &["B1-PARA"][..]),
    ] {
        assert_eq!(
            calls_from(&sections(perform), "MAIN-PARA"),
            calls,
            "{perform}"
        );
    }
}

#[test]
fn a_paragraph_named_like_the_program_it_calls_keeps_its_call() {
    let text = program(
        "ORDPGM",
        DATA,
        &[
            "MAIN-PARA.",
            "    PERFORM SHIPPGM.",
            "SHIPPGM.",
            "    CALL 'SHIPPGM' USING WS-A.",
        ],
    );
    let ex = extract("ORDPGM", &text);
    let para = ex
        .nodes
        .iter()
        .find(|n| n.name == "SHIPPGM" && n.kind == NodeKind::Function)
        .unwrap();
    assert!(
        ex.refs
            .iter()
            .any(|r| r.from == para.symbol && r.raw_name == "SHIPPGM"),
        "the paragraph's CALL is not a self-jump"
    );
    assert_eq!(calls_from(&ex, "ORDPGM"), ["MAIN-PARA"]);
}

#[test]
fn a_dynamic_call_through_an_item_named_like_the_program_keeps_its_ref() {
    let text = program(
        "DRIVER",
        &["01  DRIVER        PIC X(8) VALUE 'SUBPGM'."],
        &["MAIN-PARA.", "    CALL DRIVER."],
    );
    let ex = extract("DRIVER", &text);
    let program = ex
        .nodes
        .iter()
        .find(|n| n.name == "DRIVER" && n.kind == NodeKind::Module)
        .unwrap();
    let from_program: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.from == program.symbol && r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(from_program.is_empty(), "{from_program:?}");
    assert_eq!(
        calls_from(&ex, "DRIVER"),
        ["MAIN-PARA"],
        "the paragraph it runs"
    );
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["cobol-dynamic-call:SUBPGM"]);
}

#[test]
fn a_call_or_xctl_of_the_program_itself_starts_from_its_paragraph_only() {
    let text = program(
        "SELFPGM",
        DATA,
        &[
            "MAIN-PARA.",
            "    CALL 'SELFPGM'",
            "    EXEC CICS XCTL PROGRAM('SELFPGM') END-EXEC.",
        ],
    );
    let ex = extract("SELFPGM", &text);
    let program = ex
        .nodes
        .iter()
        .find(|n| n.name == "SELFPGM" && n.kind == NodeKind::Module)
        .unwrap();
    let from_program: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.from == program.symbol && r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        from_program.is_empty(),
        "no program → itself ref: {from_program:?}"
    );
    assert_eq!(
        calls_from(&ex, "SELFPGM"),
        ["MAIN-PARA"],
        "the paragraph it runs"
    );
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["SELFPGM"]);
}

#[test]
fn an_exec_block_without_end_exec_stops_at_the_next_exec() {
    // The scan for END-EXEC used to run on to the next block's END-EXEC, swallowing the header
    // between them (and to EOF for each later EXEC when none followed: quadratic).
    let text = program(
        "EXECNOEND",
        DATA,
        &[
            "MAIN-PARA.",
            "    EXEC CICS RETURN.",
            "NEXT-PARA.",
            "    EXEC SQL SELECT A INTO :WS-A FROM T END-EXEC",
            "    CALL 'SUBPGM'.",
        ],
    );
    let ex = extract("EXECNOEND", &text);
    assert_eq!(function_names(&ex), ["MAIN-PARA", "NEXT-PARA"]);
    assert_eq!(calls_from(&ex, "NEXT-PARA"), ["SUBPGM"]);
}

#[test]
fn the_last_paragraph_covers_the_last_line_without_a_trailing_newline() {
    let text = program(
        "NOEOL",
        DATA,
        &[
            "MAIN-PARA.",
            "    PERFORM LAST-PARA.",
            "LAST-PARA.",
            "    EXEC CICS LINK PROGRAM('SUBPGM') COMMAREA(WS-A) END-EXEC.",
        ],
    );
    let text = text.trim_end_matches('\n').to_string();
    let ex = assert_covered_to_eof("NOEOL", &text, &["MAIN-PARA", "LAST-PARA"]);
    assert_eq!(
        node(&ex, "LAST-PARA").location.span.end_line,
        text.lines().count() as u32 - 1
    );
    assert_eq!(refs_from(&ex, "LAST-PARA"), ["WS-A"]);
}

// ── round-1 second re-review folds ───────────────────────────────────────────────────────────

#[test]
fn an_unterminated_copy_hides_its_whole_last_pseudo_text_operand() {
    let text = program(
        "COPYPSE",
        DATA,
        &[
            "MAIN-PARA.",
            "    COPY CSSETATY REPLACING ==A== BY ==PERFORM 1000-WORK==",
            "AFTER-PARA.",
            "    CALL 'SUBPGM'.",
            "1000-WORK.",
            "    EXIT.",
        ],
    );
    let ex = extract("COPYPSE", &text);
    assert_eq!(
        function_names(&ex),
        ["MAIN-PARA", "AFTER-PARA", "1000-WORK"]
    );
    // Replacement text is not code: no PERFORM 1000-WORK. Each paragraph falls into the next
    // (#585).
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["AFTER-PARA"]);
    assert_eq!(calls_from(&ex, "AFTER-PARA"), ["1000-WORK", "SUBPGM"]);
}

#[test]
fn copy_and_exec_words_are_never_copy_operands() {
    // `COPY BOOK1 OF` (no library) then another COPY: the second COPY is its own statement, so
    // `BOOK2.` does not become a paragraph. `EXEC CICS ISSUE COPY END-EXEC` is no COPY at all.
    let text = program(
        "COPYKW",
        DATA,
        &[
            "MAIN-PARA.",
            "    COPY BOOK1 OF",
            "    COPY BOOK2.",
            "    EXEC CICS ISSUE COPY END-EXEC",
            "    CALL 'SUBPGM'.",
            "NEXT-PARA.",
            "    EXIT.",
        ],
    );
    let ex = extract("COPYKW", &text);
    assert_eq!(function_names(&ex), ["MAIN-PARA", "NEXT-PARA"]);
    // SUBPGM, and NEXT-PARA by fall-through (#585).
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["NEXT-PARA", "SUBPGM"]);
}

#[test]
fn exec_words_in_comments_and_literals_do_not_end_an_exec_block() {
    // The block text is not COBOL: header-shaped lines in it (GHOST-A, GHOST-B) are no paragraphs.
    let text = program(
        "EXECCMT",
        DATA,
        &[
            "MAIN-PARA.",
            "    PERFORM 1000-INIT",
            "    EXEC CICS",
            "@COMMENT@",
            "GHOST-A.",
            "         RETURN",
            "    END-EXEC",
            "    EXEC SQL SELECT A INTO :WS-A FROM T",
            "         WHERE B = 'EXEC SQL' -- EXEC SQL",
            "GHOST-B.",
            "    END-EXEC.",
            "1000-INIT.",
            "    EXIT.",
        ],
    )
    .replace("       @COMMENT@", "      *    EXEC CICS OLD-CALL");
    let ex = extract("EXECCMT", &text);
    assert_eq!(function_names(&ex), ["MAIN-PARA", "1000-INIT"]);
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["1000-INIT"]);
    assert_eq!(refs_from(&ex, "MAIN-PARA"), ["WS-A"]);
}

#[test]
fn declaratives_sections_end_at_end_declaratives() {
    let text = program(
        "DECLPGM",
        DATA,
        &[
            "DECLARATIVES.",
            "ERR-SECTION SECTION.",
            "    USE AFTER STANDARD ERROR PROCEDURE ON INFILE.",
            "ERR-PARA.",
            "    EXIT.",
            "END DECLARATIVES.",
            "MAIN-PARA.",
            "    PERFORM ERR-SECTION",
            "    GOBACK.",
            "WORK-PARA.",
            "    EXIT.",
        ],
    );
    let ex = extract("DECLPGM", &text);
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["ERR-PARA", "ERR-SECTION"]);
}

#[test]
fn before_the_first_paragraph_only_a_literal_self_call_is_dropped() {
    let text = program(
        "PAYROLL",
        DATA,
        &[
            "    PERFORM PAYROLL",
            "    CALL 'PAYROLL'",
            "    EXEC CICS XCTL PROGRAM('PAYROLL') END-EXEC",
            "    GOBACK.",
            "PAYROLL.",
            "    EXIT.",
        ],
    );
    let ex = extract("PAYROLL", &text);
    let program = ex
        .nodes
        .iter()
        .find(|n| n.name == "PAYROLL" && n.kind == NodeKind::Module)
        .unwrap();
    // The CALL and the XCTL of the program itself make no ref; the PERFORM binds to the
    // paragraph PAYROLL.
    assert!(
        !ex.refs
            .iter()
            .any(|r| r.from == program.symbol && r.kind == EdgeKind::Calls),
        "{:?}",
        ex.refs
    );
    let para = ex
        .nodes
        .iter()
        .find(|n| n.name == "PAYROLL" && n.kind == NodeKind::Function)
        .unwrap();
    let perform = text
        .lines()
        .position(|l| l.trim() == "PERFORM PAYROLL")
        .unwrap() as u32;
    assert!(ex.local_edges.iter().any(|e| {
        e.source == program.symbol
            && e.target == para.symbol
            && e.kind == EdgeKind::Calls
            && e.location
                .as_ref()
                .is_some_and(|l| l.span.start_line == perform)
    }));
}

// ── round-2 delta folds: cycles, sections, symbols ─────────────────────────────────────────────

#[test]
fn a_cycle_no_unnamed_paragraph_reaches_is_run_from_its_first_paragraph() {
    // MAIN-PARA and WORK-PARA jump to each other, so a statement names both. The program still
    // runs the first, and SUBPGM's one caller stays MAIN-PARA.
    let ex = extract(
        "LOOPPGM",
        &program(
            "LOOPPGM",
            DATA,
            &[
                "MAIN-PARA.",
                "    CALL 'SUBPGM'",
                "    GO TO WORK-PARA.",
                "WORK-PARA.",
                "    GO TO MAIN-PARA.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "LOOPPGM"), ["MAIN-PARA"]);
    assert_eq!(callers_of(&ex, "SUBPGM"), ["MAIN-PARA"]);
    // A loop that control falls into: the program starts at 0000-INIT, which falls into
    // 1000-READ (#585).
    let ex = extract(
        "READPGM",
        &program(
            "READPGM",
            DATA,
            &[
                "0000-INIT.",
                "    DISPLAY 'START'.",
                "1000-READ.",
                "    GO TO 2000-PROC.",
                "2000-PROC.",
                "    CALL 'SUBPGM'",
                "    GO TO 1000-READ.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "READPGM"), ["0000-INIT"]);
    assert_eq!(callers_of(&ex, "1000-READ"), ["0000-INIT", "2000-PROC"]);
    assert_eq!(callers_of(&ex, "2000-PROC"), ["1000-READ"]);
}

/// The nodes named `name`, in source order.
fn nodes_named<'a>(ex: &'a Extraction, name: &str) -> Vec<&'a Node> {
    let mut out: Vec<&Node> = ex.nodes.iter().filter(|n| n.name == name).collect();
    out.sort_by_key(|n| n.location.span.start_line);
    out
}

/// The `kind` local edges from `from`, as targets.
fn edges_from<'a>(
    ex: &'a Extraction,
    from: &wicked_estate_core::SymbolId,
    kind: EdgeKind,
) -> Vec<&'a wicked_estate_core::SymbolId> {
    ex.local_edges
        .iter()
        .filter(|e| &e.source == from && e.kind == kind)
        .map(|e| &e.target)
        .collect()
}

#[test]
fn same_named_paragraphs_of_two_sections_are_two_nodes_580() {
    // FIRST-SECTION's A-PARA performs SECOND-SECTION's A-PARA. A paragraph name that repeats in
    // two sections of one program is qualified by its section, `A-PARA(FIRST-SECTION).` (#580):
    // two nodes, and the qualified PERFORM is a call from one to the other, bound by identity.
    let ex = extract(
        "QUALPGM",
        &program(
            "QUALPGM",
            DATA,
            &[
                "FIRST-SECTION SECTION.",
                "A-PARA.",
                "    PERFORM A-PARA OF SECOND-SECTION",
                "    GOBACK.",
                "SECOND-SECTION SECTION.",
                "A-PARA.",
                "    CALL 'SUBPGM'.",
            ],
        ),
    );
    assert!(ex.local_edges.iter().all(|e| e.source != e.target));
    let paras = nodes_named(&ex, "A-PARA");
    assert_eq!(paras.len(), 2, "{:?}", function_names(&ex));
    let (first, second) = (&paras[0].symbol, &paras[1].symbol);
    assert!(
        first.as_str().ends_with("QUALPGM#A-PARA(FIRST-SECTION)."),
        "{first}"
    );
    assert!(
        second.as_str().ends_with("QUALPGM#A-PARA(SECOND-SECTION)."),
        "{second}"
    );
    assert_eq!(edges_from(&ex, first, EdgeKind::Calls), [second]);
    // Each section contains its own A-PARA only.
    for (section, para) in [("FIRST-SECTION", first), ("SECOND-SECTION", second)] {
        let held: Vec<_> = edges_from(&ex, &node(&ex, section).symbol, EdgeKind::Contains)
            .into_iter()
            .filter(|t| name_of(&ex, t) == "A-PARA")
            .collect();
        assert_eq!(held, [para], "{section}");
    }
    // The CALL is the second A-PARA's alone. The program starts at FIRST-SECTION (#585), which
    // falls into the first A-PARA; the second is reached only by the qualified PERFORM.
    let calls_of = |sym| {
        ex.refs
            .iter()
            .filter(|r| &r.from == sym && r.kind == EdgeKind::Calls)
            .map(|r| r.raw_name.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(calls_of(first), Vec::<&str>::new());
    assert_eq!(calls_of(second), ["SUBPGM"]);
    assert_eq!(calls_from(&ex, "QUALPGM"), ["FIRST-SECTION"]);
    assert_eq!(
        edges_from(&ex, &node(&ex, "FIRST-SECTION").symbol, EdgeKind::Calls),
        [first]
    );

    // A differently named paragraph of FIRST-SECTION performs A-PARA OF SECOND-SECTION: a local
    // call from it on the PERFORM line to SECOND-SECTION's A-PARA, though FIRST-SECTION has an
    // A-PARA too.
    let text = program(
        "QUALTWO",
        DATA,
        &[
            "FIRST-SECTION SECTION.",
            "MAIN-PARA.",
            "    PERFORM A-PARA OF SECOND-SECTION",
            "    GOBACK.",
            "A-PARA.",
            "    DISPLAY 'FIRST'.",
            "SECOND-SECTION SECTION.",
            "A-PARA.",
            "    CALL 'SUBPGM'.",
        ],
    );
    let ex = extract("QUALTWO", &text);
    let line = text
        .lines()
        .position(|l| l.trim() == "PERFORM A-PARA OF SECOND-SECTION")
        .unwrap() as u32;
    let main = &node(&ex, "MAIN-PARA").symbol;
    let performed: Vec<&wicked_estate_core::SymbolId> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls && &e.source == main)
        .filter(|e| {
            e.location
                .as_ref()
                .is_some_and(|l| l.span.start_line == line)
        })
        .map(|e| &e.target)
        .collect();
    assert_eq!(
        performed,
        [&nodes_named(&ex, "A-PARA")[1].symbol],
        "{:?}",
        ex.local_edges
    );
}

#[test]
fn a_paragraph_name_used_once_keeps_its_unqualified_id_580() {
    // Only a name that repeats across sections is qualified: MAIN-PARA and M1-PARA are unique,
    // so their ids carry no section and do not change; A-PARA and Z-PARA repeat.
    let ex = extract("QUALPGM", &two_sections("    PERFORM M1-PARA."));
    for unique in ["MAIN-PARA", "M1-PARA", "M2-PARA"] {
        let sym = node(&ex, unique).symbol.as_str();
        assert!(
            sym.ends_with(&format!("QUALPGM#{unique}().")),
            "{unique}: {sym}"
        );
    }
    for repeated in ["A-PARA", "Z-PARA"] {
        let found: Vec<&str> = nodes_named(&ex, repeated)
            .iter()
            .map(|n| n.symbol.as_str())
            .collect();
        assert_eq!(found.len(), 2, "{repeated}: {found:?}");
        assert!(
            found[0].ends_with(&format!("QUALPGM#{repeated}(FIRST-SECTION).")),
            "{found:?}"
        );
        assert!(
            found[1].ends_with(&format!("QUALPGM#{repeated}(SECOND-SECTION).")),
            "{found:?}"
        );
    }
    // Sections themselves keep their ids.
    let section = node(&ex, "FIRST-SECTION").symbol.as_str();
    assert!(section.ends_with("QUALPGM#FIRST-SECTION()."), "{section}");

    // A section named like a paragraph is no repeated paragraph name: the paragraph keeps its id.
    let ex = extract(
        "SECTPGM",
        &program(
            "SECTPGM",
            DATA,
            &[
                "S SECTION.",
                "P.",
                "    PERFORM Q.",
                "P SECTION.",
                "Q.",
                "    EXIT.",
            ],
        ),
    );
    let p = nodes_named(&ex, "P");
    assert!(
        p.iter()
            .all(|n| n.symbol.as_str().ends_with("SECTPGM#P().")),
        "{:?}",
        p.iter().map(|n| n.symbol.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn repeated_paragraphs_in_integer_named_sections_are_two_nodes_580() {
    // A section named by an integer emits no node of its own; its paragraphs are still qualified.
    let ex = extract(
        "NUMPGM",
        &program(
            "NUMPGM",
            DATA,
            &[
                "100 SECTION.",
                "P.",
                "    PERFORM P OF 200.",
                "200 SECTION.",
                "P.",
                "    EXIT.",
            ],
        ),
    );
    let p: Vec<&str> = nodes_named(&ex, "P")
        .iter()
        .map(|n| n.symbol.as_str())
        .collect();
    assert_eq!(p.len(), 2, "{p:?}");
    assert!(
        p[0].ends_with("NUMPGM#P(100).") && p[1].ends_with("NUMPGM#P(200)."),
        "{p:?}"
    );
}

#[test]
fn a_paragraph_before_any_section_keeps_its_id_and_its_twin_is_qualified_580() {
    // P before any section and P in section S: two nodes. The one in no section keeps its id; the
    // one in S is qualified.
    let ex = extract(
        "PREPGM",
        &program(
            "PREPGM",
            DATA,
            &[
                "P.",
                "    PERFORM P OF S.",
                "    GOBACK.",
                "S SECTION.",
                "P.",
                "    EXIT.",
            ],
        ),
    );
    let p: Vec<&str> = nodes_named(&ex, "P")
        .iter()
        .map(|n| n.symbol.as_str())
        .collect();
    assert_eq!(p.len(), 2, "{p:?}");
    assert!(
        p[0].ends_with("PREPGM#P().") && p[1].ends_with("PREPGM#P(S)."),
        "{p:?}"
    );
}

#[test]
fn a_section_qualified_paragraph_never_takes_a_nested_programs_id_580() {
    // OUTER repeats P in sections INNER and OTHER; the nested program INNER has its own P. The
    // section is in the paragraph's own descriptor, not a container, so the three ids differ.
    let text = [
        "       IDENTIFICATION DIVISION.",
        "       PROGRAM-ID. OUTER.",
        "       PROCEDURE DIVISION.",
        "       INNER SECTION.",
        "       P.",
        "           EXIT.",
        "       OTHER SECTION.",
        "       P.",
        "           EXIT.",
        "       IDENTIFICATION DIVISION.",
        "       PROGRAM-ID. INNER.",
        "       PROCEDURE DIVISION.",
        "       P.",
        "           EXIT.",
        "       END PROGRAM INNER.",
        "       END PROGRAM OUTER.",
    ]
    .join("\n")
        + "\n";
    let ex = extract("OUTER", &text);
    let mut p: Vec<&str> = nodes_named(&ex, "P")
        .iter()
        .map(|n| n.symbol.as_str())
        .collect();
    assert_eq!(p.len(), 3, "{p:?}");
    p.sort_unstable();
    p.dedup();
    assert_eq!(p.len(), 3, "three distinct ids: {p:?}");
}

#[test]
fn a_paragraph_a_cycle_performs_is_not_run_by_the_program() {
    // LOOP-A and LOOP-B jump to each other, and LOOP-A performs HELPER, defined before them. The
    // program starts at INIT, which falls into HELPER, which falls into the cycle (#585).
    let ex = extract(
        "HELPPGM",
        &program(
            "HELPPGM",
            DATA,
            &[
                "INIT.",
                "    DISPLAY 'START'.",
                "HELPER.",
                "    CALL 'SUBPGM'.",
                "LOOP-A.",
                "    PERFORM HELPER",
                "    GO TO LOOP-B.",
                "LOOP-B.",
                "    GO TO LOOP-A.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "HELPPGM"), ["INIT"]);
    assert_eq!(callers_of(&ex, "HELPER"), ["INIT", "LOOP-A"]);
    assert_eq!(callers_of(&ex, "LOOP-A"), ["HELPER", "LOOP-B"]);
    assert_eq!(callers_of(&ex, "SUBPGM"), ["HELPER"]);
}

#[test]
fn a_thru_range_to_an_unknown_name_still_runs_the_named_sections_paragraphs() {
    // The range end names no header, so the range is unknown: the PERFORM runs WORK-SECTION
    // (and so its paragraphs), and NO-SUCH-PARA stays a name.
    let ex = extract(
        "THRUSECT",
        &program(
            "THRUSECT",
            DATA,
            &[
                "MAIN-PARA.",
                "    PERFORM WORK-SECTION THRU NO-SUCH-PARA",
                "    GOBACK.",
                "WORK-SECTION SECTION.",
                "WORK-1.",
                "    CALL 'SUBPGM'.",
                "WORK-2.",
                "    EXIT.",
            ],
        ),
    );
    assert_eq!(
        calls_from(&ex, "MAIN-PARA"),
        ["NO-SUCH-PARA", "WORK-1", "WORK-2", "WORK-SECTION"]
    );
    assert_eq!(calls_from(&ex, "THRUSECT"), ["MAIN-PARA"]);
}

#[test]
fn a_go_to_a_section_runs_its_paragraphs() {
    let ex = extract(
        "GOSECT",
        &program(
            "GOSECT",
            DATA,
            &[
                "MAIN-PARA.",
                "    GO TO ERR-SECTION.",
                "ERR-SECTION SECTION.",
                "ERR-1.",
                "    CALL 'ERRPGM'.",
                "ERR-2.",
                "    EXIT.",
            ],
        ),
    );
    assert_eq!(
        calls_from(&ex, "MAIN-PARA"),
        ["ERR-1", "ERR-2", "ERR-SECTION"]
    );
}

#[test]
fn two_programs_in_one_file_bind_a_performed_shared_name_to_their_own_paragraph() {
    let text = [
        "       IDENTIFICATION DIVISION.",
        "       PROGRAM-ID. PGMONE.",
        "       PROCEDURE DIVISION.",
        "       MAIN-ONE.",
        "           PERFORM WORK-PARA.",
        "       WORK-PARA.",
        "           CALL 'SUBONE'.",
        "       END PROGRAM PGMONE.",
        "       IDENTIFICATION DIVISION.",
        "       PROGRAM-ID. PGMTWO.",
        "       PROCEDURE DIVISION.",
        "       MAIN-TWO.",
        "           PERFORM WORK-PARA.",
        "       WORK-PARA.",
        "           CALL 'SUBTWO'.",
        "       END PROGRAM PGMTWO.",
    ]
    .join("\n")
        + "\n";
    let ex = extract("TWOWORK", &text);
    for (main, sub) in [("MAIN-ONE", "SUBONE"), ("MAIN-TWO", "SUBTWO")] {
        let sym = &node(&ex, main).symbol;
        let work: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| &e.source == sym && e.kind == EdgeKind::Calls)
            .map(|e| e.target.clone())
            .collect();
        assert_eq!(work.len(), 1, "{main}: {work:?}");
        assert!(
            ex.refs
                .iter()
                .any(|r| r.from == work[0] && r.raw_name == sub),
            "{main} performs the WORK-PARA that calls {sub}"
        );
    }
}

// ── cost guards ──────────────────────────────────────────────────────────────────────────────

/// Extracts `text` as `name.cbl` and fails when that takes longer than a generous bound. The
/// bounds sit far above the fixed code's run (a few seconds in a debug build) and below the
/// quadratic one each guards against, so a loaded runner does not trip them.
fn assert_extracts_within(name: &str, text: &str, bound: std::time::Duration) {
    let started = std::time::Instant::now();
    let _ = extract(name, text);
    let took = started.elapsed();
    assert!(took < bound, "{name}: {took:?} (bound {bound:?})");
}

#[test]
fn a_rust_source_saved_as_cbl_does_not_walk_error_children_quadratically() {
    // Error recovery gives one ERROR node a child per token in text the grammar rejects; walking a
    // node's children by index (`Node::child(i)` is O(i)) was quadratic: about a minute for these
    // 8,000 lines in a debug build. (The parse itself still costs more on such text than it did
    // when recovery swallowed whole lines: wicked-estate#583.)
    let text: String = include_str!("../src/treesitter.rs")
        .lines()
        .take(8_000)
        .map(|l| format!("{l}\n"))
        .collect();
    assert_extracts_within("RUSTTEXT", &text, std::time::Duration::from_secs(30));
}

#[test]
fn many_exec_blocks_without_end_exec_extract_in_linear_time() {
    // Each scan for END-EXEC stops at the next EXEC (scanner patch 3); without that, each one
    // reads to the end of the file: quadratic.
    let body: Vec<&str> = std::iter::once("MAIN-PARA.")
        .chain(std::iter::repeat_n("    EXEC CICS RETURN.", 16_000))
        .collect();
    assert_extracts_within(
        "EXECPGM",
        &program("EXECPGM", DATA, &body),
        std::time::Duration::from_secs(30),
    );
}

#[test]
fn many_paragraphs_extract_in_linear_time() {
    // Each flow call and each paragraph looks up its unit and program once: 6,000 paragraphs,
    // each performing the next.
    let mut body = Vec::new();
    for i in 0..6_000 {
        body.push(format!("P{i}."));
        body.push(format!("    PERFORM P{}.", i + 1));
    }
    body.push("P6000.".to_string());
    body.push("    GOBACK.".to_string());
    let body: Vec<&str> = body.iter().map(String::as_str).collect();
    assert_extracts_within(
        "MANYPGM",
        &program("MANYPGM", DATA, &body),
        std::time::Duration::from_secs(30),
    );
}

/// The behaviour each named paragraph was given (wicked-estate#582's guards check the fast path
/// still classifies at full size; the lib test `cobol_classifier_work_is_linear_in_the_file_582`
/// counts its work).
fn behaviour_of<'a>(ex: &'a Extraction, name: &str) -> Vec<&'a str> {
    ex.nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function && n.name == name)
        .filter_map(|n| n.metadata.get("behaviour").and_then(|v| v.as_str()))
        .collect()
}

#[test]
fn many_exit_paragraphs_classify_582() {
    // 6,000 PERFORM ... THRU ranges, each ending in an EXIT paragraph (about 24k lines). Each
    // exit-only paragraph re-read the whole file (UTF-8 and free-format checks) and walked its
    // siblings with tree-sitter's O(index) `next_sibling`: about 16 s for 25k lines in a debug
    // build, growing with the square of the size.
    let mut body = Vec::new();
    for i in 0..6_000 {
        body.push(format!("P{i}."));
        body.push(format!("    PERFORM Q{i} THRU Q{i}-EXIT."));
        body.push(format!("Q{i}."));
        body.push("    MOVE 'Y' TO WS-FLAG.".to_string());
        body.push(format!("Q{i}-EXIT."));
        body.push("    EXIT.".to_string());
    }
    let body: Vec<&str> = body.iter().map(String::as_str).collect();
    let text = program("EXITPGM", DATA, &body);
    let ex = extract("EXITPGM", &text);
    assert_eq!(behaviour_of(&ex, "Q0-EXIT"), ["stub"]);
    assert_eq!(behaviour_of(&ex, "Q5999-EXIT"), ["stub"]);
    assert_eq!(behaviour_of(&ex, "Q3000"), ["real"]);
}

#[test]
fn many_programs_in_one_file_classify_582() {
    // 4,000 small programs in one file: the error-after-division check walked from the file's
    // root once per program, about 135 s in a debug build.
    let mut text = String::new();
    for i in 0..4_000 {
        let mut one = program(
            &format!("PG{i}"),
            &[],
            &["A-PARA.", "    PERFORM B-PARA.", "B-PARA.", "    EXIT."],
        );
        one.push_str(&format!("       END PROGRAM PG{i}.\n"));
        text.push_str(&one);
    }
    let ex = extract("MANYPROGS", &text);
    // Every program's B-PARA is classified. An early program's last paragraph may stay real: the
    // error check keeps the old walk's budget, which the programs after it can pass.
    let classes = behaviour_of(&ex, "B-PARA");
    assert_eq!(classes.len(), 4_000);
    let stubs = classes.iter().filter(|b| **b == "stub").count();
    assert!(stubs >= 2_000, "{stubs} of 4,000 B-PARA are stubs");
}

/// The 1-based first and last line of the parse error the File node reports (scanner patch 4),
/// read as the index reads it. Checks the report's claim: no paragraph node lies in those lines.
fn unparsed_lines(ex: &Extraction) -> Option<(u32, u32)> {
    let file = ex.nodes.iter().find(|n| n.kind == NodeKind::File)?;
    let v = file
        .metadata
        .get(wicked_estate_extract::treesitter::COBOL_UNPARSED_SPAN)?;
    let s: wicked_estate_core::Span = serde_json::from_value(v.clone()).expect("a Span");
    let last = if s.end_col == 0 && s.end_line > s.start_line {
        s.end_line
    } else {
        s.end_line + 1
    };
    for n in ex.nodes.iter().filter(|n| n.kind == NodeKind::Function) {
        let line = n.location.span.start_line + 1;
        assert!(
            !(s.start_line + 1..=last).contains(&line),
            "{} at line {line} lies in the reported error {}..={last}",
            n.name,
            s.start_line + 1
        );
    }
    Some((s.start_line + 1, last))
}

#[test]
fn generated_text_the_grammar_rejects_extracts_in_bounded_time() {
    // Patch 1's recovery walked ever longer runs of tree-sitter's stack on such text: 10,000 lines
    // took about 13 s in a debug build, and 8,000 word lines about 30 s (wicked-estate#583). Once
    // recovery has taken over a parse, scanner patch 4 recovers as upstream does.
    let text: String = (0..10_000)
        .map(|i| format!("let value_{i} = compute(&inputs[{i}], |x| x + {i})?;\n"))
        .collect();
    assert_extracts_within("JUNKTEXT", &text, std::time::Duration::from_secs(8));
    // The File node says which lines are one parse error, for the index to report.
    let (from, to) = unparsed_lines(&extract("JUNKTEXT", &text)).expect("a report");
    assert!(from <= 1_000 && to == 10_000, "{from}..={to}");
    let words: String = (0..8_000)
        .map(|i| format!("alpha beta gamma delta epsilon zeta eta theta {i}\n"))
        .collect();
    assert_extracts_within("WORDTEXT", &words, std::time::Duration::from_secs(8));
    let (from, to) = unparsed_lines(&extract("WORDTEXT", &words)).expect("a report");
    assert!(from <= 1_000 && to == 8_000, "{from}..={to}");
}

/// `count` lines of generated text the grammar cannot read, each written from column 8.
fn junk_lines(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("let value_{i} = compute(&inputs[{i}], |x| x + {i})?;"))
        .collect()
}

#[test]
fn a_program_with_an_unreadable_tail_reports_the_tail_lines() {
    // 40 readable paragraphs, then 3,000 lines the grammar cannot read: the report starts at the
    // tail (tree-sitter's error takes in the last readable line, measured), ends at the last line,
    // and the paragraphs before it keep their nodes.
    let mut body = Vec::new();
    for i in 0..40 {
        body.push(format!("PARA-{i:02}."));
        body.push(format!("    CALL 'SUB{i:02}'."));
    }
    let head = 4 + DATA.len() + 1 + body.len();
    body.extend(junk_lines(3_000));
    let body: Vec<&str> = body.iter().map(String::as_str).collect();
    let ex = extract("TAILPGM", &program("TAILPGM", DATA, &body));
    assert!(function_names(&ex).contains(&"PARA-39"));
    let (from, to) = unparsed_lines(&ex).expect("a report");
    let total = (head + 3_000) as u32;
    assert!(
        from >= head as u32 && from <= head as u32 + 1_000 && to == total,
        "{from}..={to}, readable head {head}, total {total}"
    );
}

#[test]
fn a_free_format_file_reports_its_own_lines() {
    // Free format is parsed as fixed-format cards, three cards per line here; the reported lines
    // are the file's, not the cards'.
    let mut text = String::from(">>SOURCE FORMAT FREE\n");
    for i in 0..2_000 {
        text.push_str(&format!(
            "let value_{i} = compute(&inputs[{i}], |x| x + {i})?; let other_{i} = combine(value_{i}, \
             &weights[{i}]); let third_{i} = finish(other_{i}, &limits[{i}], {i});\n"
        ));
    }
    let (from, to) = unparsed_lines(&extract("FREEJUNK", &text)).expect("a report");
    assert!(from >= 2 && to == 2_001, "{from}..={to}");
}

#[test]
fn a_program_after_an_unreadable_block_is_never_reported_as_unparsed() {
    // Once patch 4 is off, upstream recovery can still resync at a division header. The report
    // names the largest error, not "everything after the trip", so a paragraph the graph has is
    // never inside the reported lines (`unparsed_lines` checks that).
    let mut text: String = junk_lines(3_000)
        .iter()
        .map(|l| format!("       {l}\n"))
        .collect();
    text.push_str(&program(
        "AFTERPGM",
        DATA,
        &["MAIN-PARA.", "    CALL 'SUBPGM'.", "    GOBACK."],
    ));
    let ex = extract("AFTERPGM", &text);
    let (from, to) = unparsed_lines(&ex).expect("a report");
    assert!(from <= 1_000 && to <= 3_000 + 12, "{from}..={to}");
}

#[test]
fn a_block_of_unreadable_lines_inside_a_program_still_resyncs() {
    // 100 lines the grammar cannot read between two paragraphs stay below patch 4's floor: the
    // parse resyncs after them (patch 1), and the paragraphs after the block get nodes.
    let mut body = vec!["MAIN-PARA.".to_string(), "    DISPLAY 'START'.".to_string()];
    for i in 0..100 {
        body.push(format!(
            "    let value_{i} = compute(&inputs[{i}], |x| x + {i})?;"
        ));
    }
    body.extend([
        "NEXT-PARA.".to_string(),
        "    CALL 'SUBPGM'.".to_string(),
        "LAST-PARA.".to_string(),
        "    GOBACK.".to_string(),
    ]);
    let body: Vec<&str> = body.iter().map(String::as_str).collect();
    let ex = extract("JUNKPGM", &program("JUNKPGM", DATA, &body));
    let names = function_names(&ex);
    assert!(
        names.contains(&"NEXT-PARA") && names.contains(&"LAST-PARA"),
        "{names:?}"
    );
    assert_eq!(callers_of(&ex, "SUBPGM"), ["NEXT-PARA"]);
    assert_eq!(unparsed_lines(&ex), None);
}

// ── control flow no statement names (#585) ────────────────────────────────────────────────────

/// The one local `Calls` edge from `from` to `to`, by node name.
fn flow_edge<'a>(ex: &'a Extraction, from: &str, to: &str) -> &'a wicked_estate_core::Edge {
    let (from, to) = (&node(ex, from).symbol, &node(ex, to).symbol);
    let edges: Vec<_> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls && &e.source == from && &e.target == to)
        .collect();
    assert_eq!(edges.len(), 1, "{edges:?}");
    edges[0]
}

/// `(fall-through, conditional)` metadata of the edge from `from` to `to`.
fn flow_tags(ex: &Extraction, from: &str, to: &str) -> (bool, bool) {
    let edge = flow_edge(ex, from, to);
    let tag = |key: &str| edge.metadata.get(key).and_then(|v| v.as_bool()) == Some(true);
    (tag("cobol_fall_through"), tag("cobol_conditional"))
}

#[test]
fn control_falls_from_a_paragraph_into_the_next_and_the_program_starts_only_its_first() {
    let ex = extract(
        "FALLPGM",
        &program(
            "FALLPGM",
            DATA,
            &[
                "MAIN-PARA.",
                "    DISPLAY 'START'.",
                "NEXT-PARA.",
                "    CALL 'SUBPGM'.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "FALLPGM"), ["MAIN-PARA"]);
    assert_eq!(callers_of(&ex, "NEXT-PARA"), ["MAIN-PARA"]);
    assert_eq!(flow_tags(&ex, "MAIN-PARA", "NEXT-PARA"), (true, false));
    assert_eq!(flow_tags(&ex, "FALLPGM", "MAIN-PARA"), (false, false));
    assert_eq!(callers_of(&ex, "SUBPGM"), ["NEXT-PARA"]);
}

/// MAIN-PARA ends with `last` (and whatever comes before it); NEXT-PARA follows and nothing
/// names it. Its callers: MAIN-PARA when control falls into it, else none.
fn next_callers(last: &[&str]) -> Vec<String> {
    let mut body = vec!["MAIN-PARA.", "    DISPLAY 'START'"];
    body.extend(last);
    body.extend(["NEXT-PARA.", "    CALL 'SUBPGM'."]);
    let ex = extract("ENDPGM", &program("ENDPGM", DATA, &body));
    assert_eq!(calls_from(&ex, "ENDPGM"), ["MAIN-PARA"], "{last:?}");
    callers_of(&ex, "NEXT-PARA")
}

#[test]
fn goback_stop_run_and_an_unconditional_go_to_end_a_run() {
    let none: Vec<String> = Vec::new();
    for last in [
        &["    GOBACK."][..],
        &["    STOP RUN."],
        // After a closed IF, the GOBACK runs whatever the condition.
        &[
            "    IF WS-FLAG = 'Y'",
            "        DISPLAY 'Y'",
            "    END-IF",
            "    GOBACK.",
        ],
        // In a later sentence of the paragraph.
        &["    IF WS-FLAG = 'Y' DISPLAY 'Y'.", "    GOBACK."],
        // After a READ's AT END phrase, closed by END-READ.
        &[
            "    READ INFILE AT END MOVE 'Y' TO WS-FLAG END-READ",
            "    GOBACK.",
        ],
    ] {
        assert_eq!(next_callers(last), none, "{last:?}");
    }
    // An unconditional GO TO: NEXT-PARA right after MAIN-PARA gets no fall-through caller.
    let ex = extract(
        "GOTOPGM",
        &program(
            "GOTOPGM",
            DATA,
            &[
                "MAIN-PARA.",
                "    GO TO OTHER-PARA.",
                "NEXT-PARA.",
                "    CALL 'SUBPGM'.",
                "OTHER-PARA.",
                "    GOBACK.",
            ],
        ),
    );
    assert_eq!(callers_of(&ex, "NEXT-PARA"), none);
    assert_eq!(flow_tags(&ex, "MAIN-PARA", "OTHER-PARA"), (false, false));
}

#[test]
fn a_run_ending_statement_under_a_condition_or_unknown_does_not_end_the_run() {
    let main = vec!["MAIN-PARA".to_string()];
    for last in [
        // Flat IFs: the terminator is inside the IF, until END-IF or the period.
        &["    IF WS-FLAG = 'Y'", "        GOBACK", "    END-IF."][..],
        &["    IF WS-FLAG = 'Y' GOBACK."],
        &["    IF WS-FLAG = 'Y' DISPLAY 'Y' ELSE STOP RUN END-IF."],
        &[
            "    EVALUATE WS-FLAG",
            "        WHEN 'Y' GOBACK",
            "    END-EVALUATE.",
        ],
        &["    READ INFILE AT END GOBACK END-READ."],
        &["    READ INFILE AT END DISPLAY 'E' NOT AT END GOBACK."],
        &[
            "    PERFORM UNTIL WS-FLAG = 'Y'",
            "        GOBACK",
            "    END-PERFORM.",
        ],
        // GO TO DEPENDING falls through when the index is out of range.
        &["    GO TO OTHER-PARA DEPENDING ON WS-FLAG."],
        // EXIT PROGRAM is ignored in a program nobody called; CICS RETURN with RESP returns.
        &["    EXIT PROGRAM."],
        &["    EXEC CICS RETURN END-EXEC."],
        // The rest of a sentence after NEXT SENTENCE may be skipped.
        &["    IF WS-FLAG = 'Y' NEXT SENTENCE END-IF", "    GOBACK."],
        // STOP with a literal is not STOP RUN.
        &["    STOP 'END'."],
    ] {
        if last[0].contains("DEPENDING") {
            // NEXT-PARA right after MAIN-PARA; the DEPENDING target comes after it.
            let mut body = vec!["MAIN-PARA.", "    DISPLAY 'START'"];
            body.extend(last);
            body.extend(["NEXT-PARA.", "    GOBACK.", "OTHER-PARA.", "    GOBACK."]);
            let ex = extract("ENDPGM", &program("ENDPGM", DATA, &body));
            assert_eq!(flow_tags(&ex, "MAIN-PARA", "NEXT-PARA"), (true, false));
            assert_eq!(flow_tags(&ex, "MAIN-PARA", "OTHER-PARA"), (false, true));
            continue;
        }
        assert_eq!(next_callers(last), main, "{last:?}");
    }
}

#[test]
fn stop_run_split_by_a_sequence_number_still_ends_the_run() {
    let text = [
        "000100 IDENTIFICATION DIVISION.",
        "000200 PROGRAM-ID. SEQPGM.",
        "000300 PROCEDURE DIVISION.",
        "000400 MAIN-PARA.",
        "000500     DISPLAY 'START'",
        "000600     STOP",
        "000700     RUN.",
        "000800 NEXT-PARA.",
        "000900     CALL 'SUBPGM'.",
    ]
    .join("\n")
        + "\n";
    let ex = extract("SEQPGM", &text);
    assert_eq!(calls_from(&ex, "SEQPGM"), ["MAIN-PARA"]);
    assert_eq!(callers_of(&ex, "NEXT-PARA"), Vec::<String>::new());
}

#[test]
fn go_to_depending_targets_are_conditional_unless_also_reached_another_way() {
    let ex = extract(
        "DEPPGM",
        &program(
            "DEPPGM",
            DATA,
            &[
                "MAIN-PARA.",
                "    PERFORM B-PARA",
                "    GO TO A-PARA B-PARA DEPENDING ON WS-FLAG",
                "    GOBACK.",
                "A-PARA.",
                "    GOBACK.",
                "B-PARA.",
                "    GOBACK.",
            ],
        ),
    );
    assert_eq!(flow_tags(&ex, "MAIN-PARA", "A-PARA"), (false, true));
    assert_eq!(flow_tags(&ex, "MAIN-PARA", "B-PARA"), (false, false));
    // In the THRU fixture, 2000-B's DEPENDING target 2000-X is also where it falls: not
    // conditional, since falling is not; 2000-EXIT is reached only by the DEPENDING.
    let ex = extract("THRUPGM", &program("THRUPGM", DATA, THRU));
    assert_eq!(flow_tags(&ex, "2000-B", "2000-X"), (true, false));
    assert_eq!(flow_tags(&ex, "2000-B", "2000-EXIT"), (false, true));
}

#[test]
fn nested_ifs_owned_phrases_and_exits_are_read_as_scopes() {
    let main = vec!["MAIN-PARA".to_string()];
    let none: Vec<String> = Vec::new();
    for (last, falls) in [
        // ELSE IF opens a nested IF: the inner END-IF leaves the GOBACK under the outer one.
        (
            &[
                "    IF WS-FLAG = 'Y' CONTINUE",
                "    ELSE IF WS-A = 'B' CONTINUE END-IF",
                "    GOBACK END-IF.",
            ][..],
            true,
        ),
        // END-CALL ends the CALL inside AT END, not the READ's phrase.
        (
            &["    READ INFILE AT END CALL 'X' END-CALL GOBACK END-READ."],
            true,
        ),
        // An inner READ with no phrase ends with its own END-READ.
        (
            &["    READ INFILE AT END READ OUTFILE END-READ GOBACK END-READ."],
            true,
        ),
        // NOT AT END after the AT END statements is the same scope: END-READ closes it.
        (
            &[
                "    READ INFILE AT END DISPLAY 'EOF' NOT AT END CONTINUE END-READ",
                "    GOBACK.",
            ],
            false,
        ),
        // EXIT PARAGRAPH (taken under an IF) skips the GOBACK after it.
        (
            &["    IF WS-FLAG = 'Y' EXIT PARAGRAPH END-IF", "    GOBACK."],
            true,
        ),
    ] {
        let want = if falls { &main } else { &none };
        assert_eq!(&next_callers(last), want, "{last:?}");
    }
}

#[test]
fn a_procedure_copy_or_an_integer_named_paragraph_keeps_the_flow() {
    // The member of a procedure COPY may hold any statements: the run is not known to end.
    let ex = extract(
        "COPYFALL",
        &program(
            "COPYFALL",
            DATA,
            &[
                "MAIN-PARA.",
                "    IF WS-FLAG = 'Y' COPY BODY.",
                "    GOBACK.",
                "NEXT-PARA.",
                "    CALL 'SUBPGM'.",
            ],
        ),
    );
    assert_eq!(callers_of(&ex, "NEXT-PARA"), ["MAIN-PARA"]);
    // An integer-named paragraph has no node: control falls through it to NEXT-PARA, and a
    // program that starts there starts at NEXT-PARA.
    let ex = extract(
        "NUMFALL",
        &program(
            "NUMFALL",
            DATA,
            &[
                "1000.",
                "    DISPLAY 'X'.",
                "NEXT-PARA.",
                "    CALL 'SUBPGM'.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "NUMFALL"), ["NEXT-PARA"]);
}

#[test]
fn a_cycle_nothing_reaches_is_dead_code() {
    let ex = extract(
        "DEADPGM",
        &program(
            "DEADPGM",
            DATA,
            &[
                "MAIN-PARA.",
                "    GOBACK.",
                "LOOP-A.",
                "    GO TO LOOP-B.",
                "LOOP-B.",
                "    CALL 'SUBPGM'",
                "    GO TO LOOP-A.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "DEADPGM"), ["MAIN-PARA"]);
    assert_eq!(callers_of(&ex, "LOOP-A"), ["LOOP-B"]);
    assert_eq!(callers_of(&ex, "LOOP-B"), ["LOOP-A"]);
}

#[test]
fn a_section_falls_into_its_paragraphs_and_into_the_next_section() {
    let ex = extract(
        "SECFALL",
        &program(
            "SECFALL",
            DATA,
            &[
                "A-SECTION SECTION.",
                "A1.",
                "    DISPLAY 'A1'.",
                "B-SECTION SECTION.",
                "B1.",
                "    GOBACK.",
            ],
        ),
    );
    assert_eq!(calls_from(&ex, "SECFALL"), ["A-SECTION"]);
    assert_eq!(flow_tags(&ex, "A-SECTION", "A1"), (true, false));
    assert_eq!(flow_tags(&ex, "A1", "B-SECTION"), (true, false));
    assert_eq!(flow_tags(&ex, "B-SECTION", "B1"), (true, false));
}

/// A program with two files (INFILE with record IN-REC, OUTFILE with OUT-REC) and DECLARATIVES
/// `handlers` before MAIN-PARA's `main` statements.
fn with_files(id: &str, handlers: &[&str], main: &[&str]) -> String {
    let mut out: Vec<String> = [
        "IDENTIFICATION DIVISION.",
        &format!("PROGRAM-ID. {id}."),
        "ENVIRONMENT DIVISION.",
        "INPUT-OUTPUT SECTION.",
        "FILE-CONTROL.",
        "    SELECT INFILE ASSIGN TO 'IN'.",
        "    SELECT OUTFILE ASSIGN TO 'OUT'.",
        "DATA DIVISION.",
        "FILE SECTION.",
        "FD  INFILE.",
        "01  IN-REC        PIC X(80).",
        "FD  OUTFILE.",
        "01  OUT-REC       PIC X(80).",
        "WORKING-STORAGE SECTION.",
        "01  WS-REC        PIC X(80).",
        "PROCEDURE DIVISION.",
        "DECLARATIVES.",
    ]
    .iter()
    .map(|l| l.to_string())
    .collect();
    out.extend(handlers.iter().map(|l| l.to_string()));
    out.push("END DECLARATIVES.".to_string());
    out.push("MAIN-SECTION SECTION.".to_string());
    out.push("MAIN-PARA.".to_string());
    out.extend(main.iter().map(|l| l.to_string()));
    out.iter()
        .map(|l| format!("       {l}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn a_use_handler_is_run_by_the_io_statements_on_its_files() {
    let text = with_files(
        "USEPGM",
        &[
            "IN-ERR SECTION.",
            "    USE AFTER STANDARD ERROR PROCEDURE ON INFILE.",
            "IN-ERR-PARA.",
            "    CALL 'LOGGER'.",
            "OUT-ERR SECTION.",
            "    USE AFTER STANDARD ERROR PROCEDURE ON OUTFILE.",
            "OUT-ERR-PARA.",
            "    DISPLAY 'OUT'.",
        ],
        &[
            "    OPEN INPUT INFILE",
            "    PERFORM READ-PARA",
            "    PERFORM WRITE-PARA",
            "    GOBACK.",
            "READ-PARA.",
            "    READ INFILE.",
            "WRITE-PARA.",
            "    WRITE OUT-REC.",
        ],
    );
    let ex = extract("USEPGM", &text);
    // The program starts at MAIN-SECTION, never at a handler.
    assert_eq!(calls_from(&ex, "USEPGM"), ["MAIN-SECTION"]);
    assert_eq!(callers_of(&ex, "IN-ERR"), ["MAIN-PARA", "READ-PARA"]);
    assert_eq!(flow_tags(&ex, "READ-PARA", "IN-ERR"), (false, true));
    // WRITE names OUT-REC, OUTFILE's record.
    assert_eq!(callers_of(&ex, "OUT-ERR"), ["WRITE-PARA"]);
    // A handler section falls into its paragraph, never into the next handler or past END
    // DECLARATIVES.
    // DECLARATIVES (OUT-ERR's one caller above is WRITE-PARA, not IN-ERR-PARA).
    assert_eq!(flow_tags(&ex, "IN-ERR", "IN-ERR-PARA"), (true, false));
    assert_eq!(callers_of(&ex, "MAIN-SECTION"), ["USEPGM"]);
    assert_eq!(callers_of(&ex, "LOGGER"), ["IN-ERR-PARA"]);
}

#[test]
fn a_use_on_an_open_mode_or_on_a_record_of_no_fd_is_run_by_every_matching_io_statement() {
    let text = with_files(
        "MODEPGM",
        &[
            "ANY-ERR SECTION.",
            "    USE AFTER ERROR PROCEDURE ON INPUT.",
            "ANY-ERR-PARA.",
            "    DISPLAY 'ERR'.",
            "IN-ERR SECTION.",
            "    USE AFTER ERROR PROCEDURE ON INFILE.",
            "IN-ERR-PARA.",
            "    DISPLAY 'IN'.",
        ],
        &[
            "    PERFORM WRITE-PARA",
            "    PERFORM CLOSE-PARA",
            "    GOBACK.",
            "WRITE-PARA.",
            "    WRITE WS-REC.",
            "    GOBACK.",
            "CLOSE-PARA.",
            "    CLOSE OUTFILE.",
        ],
    );
    let ex = extract("MODEPGM", &text);
    assert_eq!(callers_of(&ex, "ANY-ERR"), ["CLOSE-PARA", "WRITE-PARA"]);
    // WS-REC is no FD's record, so it may be any file's: INFILE's handler may run.
    assert_eq!(callers_of(&ex, "IN-ERR"), ["WRITE-PARA"]);
}

#[test]
fn sort_files_declaratives_io_and_global_handlers_run_handlers() {
    let text = with_files(
        "SORTPGM",
        &[
            "IN-ERR SECTION.",
            "    USE GLOBAL AFTER STANDARD ERROR PROCEDURE ON INFILE.",
            "IN-ERR-PARA.",
            "    WRITE OUT-REC.",
            "OUT-ERR SECTION.",
            "    USE AFTER STANDARD ERROR PROCEDURE ON OUTFILE.",
            "OUT-ERR-PARA.",
            "    DISPLAY 'OUT'.",
        ],
        &[
            "    PERFORM SORT-PARA",
            "    GOBACK.",
            "SORT-PARA.",
            "    SORT WS-REC ON ASCENDING KEY WS-REC",
            "        USING INFILE GIVING OUTFILE.",
        ],
    );
    let ex = extract("SORTPGM", &text);
    // SORT reads INFILE and writes OUTFILE.
    assert_eq!(callers_of(&ex, "OUT-ERR"), ["IN-ERR-PARA", "SORT-PARA"]);
    // GLOBAL: a contained program's I/O may run it, so the program calls it too.
    assert_eq!(callers_of(&ex, "IN-ERR"), ["SORT-PARA", "SORTPGM"]);
    assert_eq!(flow_tags(&ex, "SORTPGM", "IN-ERR"), (false, true));
}

#[test]
fn a_use_for_debugging_handler_is_run_by_the_program_conditionally() {
    let text = with_files(
        "DBGPGM",
        &[
            "DBG SECTION.",
            "    USE FOR DEBUGGING ON ALL PROCEDURES.",
            "DBG-PARA.",
            "    DISPLAY 'DBG'.",
        ],
        &["    GOBACK."],
    );
    let ex = extract("DBGPGM", &text);
    assert_eq!(calls_from(&ex, "DBGPGM"), ["DBG", "MAIN-SECTION"]);
    assert_eq!(flow_tags(&ex, "DBGPGM", "DBG"), (false, true));
}
