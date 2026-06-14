//! Cross-language + cross-domain SEMANTIC CONNECTION receipts (permanent regression tests).
//!
//! The headline value of wicked_estate is one graph where a reference in language A resolves to a
//! definition in language B. This test indexes a polyglot mainframe estate through the full
//! `index_path` pipeline and asserts — via the user-facing `blast_radius_by_name` query path —
//! that each cross-boundary edge actually resolves. It pins the joins that were previously only
//! hand-validated:
//!
//! - JCL `EXEC PGM=PAYROLL`        → the COBOL program PAYROLL          (cross-language, Calls)
//! - HLASM `CALL PAYROLL`          → the COBOL program PAYROLL          (cross-language, Calls)
//! - COBOL `CALL 'TAXSUB'`         → the COBOL program TAXSUB           (string-literal call target)
//! - JCL `DD DSN=` + RACF `ADDSD`  → the dataset, used + protected      (cross-domain estate join)
//! - RACF `RDEFINE MQQUEUE` + MQSC → the MQ queue, protected            (cross-domain, generic class)
//! - MQSC `QALIAS TARGET(...)`     → the target queue                   (resolves_to)
//! - IMS `DBD`/`SEGM PARENT=`      → segment hierarchy                  (Contains + parent)

use wicked_estate_core::{Node, NodeKind};
use wicked_estate_store::SqliteStore;
use std::fs;
use std::path::PathBuf;

fn fresh_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_xlang_estate_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// The polyglot estate: COBOL + JCL + RACF + IMS DBD/PSB + MQSC, wired by name across files.
fn fixtures() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "PAYROLL.cbl",
            "       IDENTIFICATION DIVISION.\n\
             \x20      PROGRAM-ID. PAYROLL.\n\
             \x20      DATA DIVISION.\n\
             \x20      WORKING-STORAGE SECTION.\n\
             \x20      01  WS-NET-PAY        PIC S9(7)V99 COMP-3.\n\
             \x20      PROCEDURE DIVISION.\n\
             \x20      MAIN-PARA.\n\
             \x20          CALL 'TAXSUB' USING WS-NET-PAY.\n\
             \x20          STOP RUN.\n",
        ),
        (
            "TAXSUB.cbl",
            "       IDENTIFICATION DIVISION.\n\
             \x20      PROGRAM-ID. TAXSUB.\n\
             \x20      PROCEDURE DIVISION.\n\
             \x20      MAIN-PARA.\n\
             \x20          GOBACK.\n",
        ),
        (
            "RUNPAY.jcl",
            "//RUNPAY   JOB (ACCT),'PAYROLL RUN',CLASS=A\n\
             //STEP1    EXEC PGM=PAYROLL\n\
             //CUSTFILE DD DSN=PAYROLL.MASTER.KSDS,DISP=SHR\n",
        ),
        (
            "BATCH.hlasm",
            "BATCHPGM CSECT\n\
             \x20        CALL  PAYROLL\n\
             \x20        BR    14\n\
             \x20        END\n",
        ),
        (
            "security.racf",
            "ADDSD 'PAYROLL.MASTER.KSDS' UACC(NONE) OWNER(PAYADM)\n\
             RDEFINE MQQUEUE PAYROLL.IN UACC(NONE) OWNER(PAYADM)\n",
        ),
        (
            "queues.mqsc",
            "DEFINE QLOCAL('PAYROLL.IN') MAXDEPTH(5000)\n\
             DEFINE QALIAS('PAY.ALIAS') TARGET('PAYROLL.IN')\n",
        ),
        (
            "custdb.dbd",
            "CUSTDBD  DBD   NAME=CUSTDB,ACCESS=(HDAM,OSAM)\n\
             \x20        SEGM  NAME=CUSTOMER,PARENT=0,BYTES=200\n\
             \x20        SEGM  NAME=ORDER,PARENT=CUSTOMER,BYTES=150\n",
        ),
    ]
}

/// True when `name`'s blast radius (all dependents, the user-facing query) contains a node of the
/// given kind + name — i.e. the cross-boundary edge resolved and is traversable.
fn blast_contains(store: &SqliteStore, query: &str, kind: NodeKind, dep_name: &str) -> bool {
    let deps: Vec<Node> = wicked_estate::blast_radius_by_name(store, query, 12).expect("blast radius");
    deps.iter().any(|n| n.kind == kind && n.name == dep_name)
}

fn other(k: &str) -> NodeKind {
    NodeKind::Other(k.to_string())
}

#[test]
fn polyglot_estate_resolves_cross_language_and_cross_domain() {
    let dir = fresh_dir();
    for (name, src) in fixtures() {
        fs::write(dir.join(name), src).unwrap();
    }
    let mut store = SqliteStore::in_memory().expect("sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    // 1) Cross-language: JCL `EXEC PGM=PAYROLL` resolves to the COBOL program → the step is a
    //    dependent of PAYROLL.
    assert!(
        blast_contains(&store, "PAYROLL", other("step"), "STEP1"),
        "JCL EXEC PGM=PAYROLL must resolve to the COBOL program (step STEP1 in PAYROLL's blast radius)"
    );

    // 1b) Cross-language: HLASM `CALL PAYROLL` resolves to the same COBOL program → the assembler
    //     CSECT is a dependent of PAYROLL.
    assert!(
        blast_contains(&store, "PAYROLL", NodeKind::Module, "BATCHPGM"),
        "HLASM CALL PAYROLL must resolve to the COBOL program (CSECT BATCHPGM in PAYROLL's blast radius)"
    );

    // 2) Cross-language: COBOL `CALL 'TAXSUB'` (quoted literal) resolves to the COBOL program
    //    TAXSUB → the caller PAYROLL is a dependent of TAXSUB.
    assert!(
        blast_contains(&store, "TAXSUB", NodeKind::Module, "PAYROLL"),
        "COBOL CALL 'TAXSUB' must resolve to program TAXSUB (caller PAYROLL in TAXSUB's blast radius)"
    );

    // 3) Cross-domain: the dataset is both USED by the JCL step and PROTECTED by the RACF profile.
    assert!(
        blast_contains(&store, "PAYROLL.MASTER.KSDS", other("step"), "STEP1"),
        "JCL step must `use` the dataset"
    );
    assert!(
        blast_contains(
            &store,
            "PAYROLL.MASTER.KSDS",
            other("racf_dataset_profile"),
            "PAYROLL.MASTER.KSDS"
        ),
        "RACF dataset profile must `protect` the dataset (estate cross-domain join)"
    );

    // 4) Cross-domain: RACF `RDEFINE MQQUEUE` protects the MQ queue (generic-class match), and the
    //    MQSC alias resolves_to it — both surface in the queue's blast radius.
    assert!(
        blast_contains(&store, "PAYROLL.IN", other("racf_profile"), "PAYROLL.IN"),
        "RACF MQQUEUE profile must `protect` the MQ queue"
    );
    assert!(
        blast_contains(&store, "PAYROLL.IN", other("mq_queue"), "PAY.ALIAS"),
        "MQSC QALIAS must `resolve_to` PAYROLL.IN"
    );

    // 5) IMS hierarchy: CUSTOMER is contained by CUSTDB and is the parent of ORDER.
    assert!(
        blast_contains(&store, "CUSTOMER", other("ims_database"), "CUSTDB"),
        "IMS DBD CUSTDB must `contain` segment CUSTOMER"
    );
    assert!(
        blast_contains(&store, "CUSTOMER", other("ims_segment"), "ORDER"),
        "IMS segment ORDER must point to its `parent` CUSTOMER"
    );

    let _ = fs::remove_dir_all(&dir);
}
