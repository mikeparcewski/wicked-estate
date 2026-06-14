//! COBOL data-item / copybook-structure coverage (mainframe stack: "Advanced Data Formats" +
//! "Complex Copybook Structures"). Verifies that COMP-3 / signed items, REDEFINES overlays, and
//! OCCURS DEPENDING ON arrays are extracted from WORKING-STORAGE into the graph: each data item
//! becomes a node, and the REDEFINES / DEPENDING-ON relationships become reference edges.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

// Fixed-format COBOL (Area A at col 8, Area B at col 12). 01 group + COMP-3/signed elementary
// items, a REDEFINES overlay, and an OCCURS ... DEPENDING ON table.
const PROG: &str = concat!(
    "       IDENTIFICATION DIVISION.\n",
    "       PROGRAM-ID. CUSTPROG.\n",
    "       DATA DIVISION.\n",
    "       WORKING-STORAGE SECTION.\n",
    "       01  CUSTOMER-RECORD.\n",
    "           05  CUST-ID            PIC 9(7) COMP-3.\n",
    "           05  CUST-BALANCE       PIC S9(9)V99 COMP-3.\n",
    "           05  CUST-FLAGS         PIC X(4).\n",
    "           05  CUST-FLAGS-R       REDEFINES CUST-FLAGS PIC 9(4).\n",
    "           05  ITEM-COUNT         PIC 9(3) COMP.\n",
    "           05  ITEM-TABLE OCCURS 1 TO 100 TIMES DEPENDING ON ITEM-COUNT.\n",
    "               10  ITEM-CODE      PIC X(5).\n",
    "       PROCEDURE DIVISION.\n",
    "       MAIN-PARA.\n",
    "           MOVE 0 TO ITEM-COUNT.\n",
    "           STOP RUN.\n",
);

fn extract() -> wicked_estate_core::Extraction {
    TreeSitterExtractor::for_language("cobol")
        .expect("cobol registered")
        .extract(&SourceFile {
            path: "CUSTPROG.cbl".to_string(),
            language: Language::new("cobol"),
            text: PROG.to_string(),
        })
        .expect("cobol extraction")
}

#[test]
fn cobol_data_items_become_nodes() {
    let ex = extract();
    let fields: Vec<&str> = ex
        .nodes
        .iter()
        .filter(|n| {
            matches!(n.kind, NodeKind::Field) || n.kind == NodeKind::Other("field".to_string())
        })
        .map(|n| n.name.as_str())
        .collect();
    // The COMP-3, signed, REDEFINES and OCCURS items must all surface as data-item nodes.
    for expected in [
        "CUST-ID",
        "CUST-BALANCE",
        "CUST-FLAGS",
        "ITEM-COUNT",
        "ITEM-TABLE",
    ] {
        assert!(
            fields.contains(&expected),
            "data item {expected} must be a node; got {fields:?}",
        );
    }
}

#[test]
fn cobol_redefines_and_occurs_depending_emit_refs() {
    let ex = extract();
    let refs: Vec<&str> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    // REDEFINES CUST-FLAGS → a reference to CUST-FLAGS.
    assert!(
        refs.contains(&"CUST-FLAGS"),
        "REDEFINES target CUST-FLAGS must be a reference; got {refs:?}",
    );
    // OCCURS ... DEPENDING ON ITEM-COUNT → a reference to ITEM-COUNT.
    assert!(
        refs.contains(&"ITEM-COUNT"),
        "OCCURS DEPENDING ON counter ITEM-COUNT must be a reference; got {refs:?}",
    );
}
