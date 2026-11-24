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
    assert_eq!(calls_from(&ex, "1000-A"), ["1000-EXIT"]);
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
    assert!(calls_from(&ex, "MAIN-PARA").is_empty());
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
fn a_paragraph_control_falls_into_is_run_by_the_program() {
    // No statement names NEXT-PARA: control falls into it from MAIN-PARA. Its CALL must stay in
    // the program's lineage.
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
    assert_eq!(calls_from(&ex, "FALLPGM"), ["MAIN-PARA", "NEXT-PARA"]);
    assert_eq!(callers_of(&ex, "SUBPGM"), ["NEXT-PARA"]);
}

#[test]
fn using_comments_declaratives_and_sections_are_run_by_the_program() {
    let using = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. USINGPGM.\n       \
                 DATA DIVISION.\n       LINKAGE SECTION.\n       01  LK-A PIC X.\n       \
                 PROCEDURE DIVISION USING LK-A.\n      * the entry\n       MAIN-PARA.\n           \
                 GOBACK.\n";
    assert_eq!(
        calls_from(&extract("USINGPGM", using), "USINGPGM"),
        ["MAIN-PARA"]
    );
    // A DECLARATIVES handler runs on an I/O condition, which no statement names.
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
    assert_eq!(
        calls_from(&ex, "DECLPGM"),
        ["ERR-PARA", "ERR-SECTION", "MAIN-PARA"]
    );
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
    // WORK-SECTION and its paragraph are performed; the rest is the program's.
    assert_eq!(
        calls_from(&extract("SECTPGM", &section), "SECTPGM"),
        ["MAIN-PARA", "MAIN-SECTION"]
    );
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
    // 2000-B by the PERFORM; 1000-A is named by nothing (dead here, but the graph cannot tell).
    assert_eq!(calls_from(&ex, "TOPPGM"), ["1000-A", "2000-B"]);
    assert_eq!(callers_of(&ex, "2000-B"), ["TOPPGM"]);
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
    assert!(
        calls_from(&ex, "MAIN-PARA").is_empty(),
        "replacement text is not code"
    );
    assert_eq!(calls_from(&ex, "AFTER-PARA"), ["SUBPGM"]);
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
    assert_eq!(calls_from(&ex, "MAIN-PARA"), ["SUBPGM"]);
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
    // A loop that control falls into: the program runs 0000-INIT, and 1000-READ starts the loop.
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
    assert_eq!(calls_from(&ex, "READPGM"), ["0000-INIT", "1000-READ"]);
    assert_eq!(callers_of(&ex, "2000-PROC"), ["1000-READ"]);
}

#[test]
fn a_qualified_perform_of_a_same_named_paragraph_is_not_a_self_jump() {
    // FIRST-SECTION's A-PARA performs SECOND-SECTION's A-PARA: a call, and FIRST-SECTION's A-PARA
    // (named by no statement) is still the program's.
    let text = program(
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
    );
    let ex = extract("QUALPGM", &text);
    let paras: std::collections::BTreeSet<_> = ex
        .nodes
        .iter()
        .filter(|n| n.name == "A-PARA" && n.kind == NodeKind::Function)
        .map(|n| n.symbol.clone())
        .collect();
    // Paragraph ids carry no section (#580), so the two may share one id: then the call is not
    // stored (no self-edges). With two ids, the PERFORM is a call, never a jump to itself.
    assert!(ex.local_edges.iter().all(|e| e.source != e.target));
    if paras.len() == 2 {
        let line = text
            .lines()
            .position(|l| l.trim() == "PERFORM A-PARA OF SECOND-SECTION")
            .unwrap() as u32;
        assert!(
            ex.local_edges.iter().any(|e| e.kind == EdgeKind::Calls
                && e.location
                    .as_ref()
                    .is_some_and(|l| l.span.start_line == line)),
            "{:?}",
            ex.local_edges
        );
    }
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
