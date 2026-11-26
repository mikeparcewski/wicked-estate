//! estate#641 step 1, a MEASUREMENT branch that is never merged: a full `CicsSqlExtractor` extract
//! at N, 2N and 4N of three shapes, release profile, timed. It runs only when `MEASURE_641` is
//! set (the measurement job sets it), so the gate's debug test run skips its sizes.

use std::time::Instant;
use wicked_estate_core::{Extractor, Language, SourceFile};
use wicked_estate_extract::CicsSqlExtractor;

fn program(id: &str, data: &[String], body: &[String]) -> String {
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

/// A paragraph per CICS `LINK`.
fn cics_links(n: usize) -> String {
    let mut body = Vec::new();
    for i in 0..n {
        body.push(format!("P{i}."));
        body.push(format!("    EXEC CICS LINK PROGRAM('PGM{i}') END-EXEC."));
    }
    body.push("    GOBACK.".to_string());
    program("LINKS", &["01  WS-A PIC X(10).".to_string()], &body)
}

/// Many sequential programs, each with a field and a data use.
fn many_programs(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        out.push_str(&program(
            &format!("PG{i}"),
            &[format!("01  F{i} PIC X(10).")],
            &[
                "MAIN-PARA.".to_string(),
                format!("    MOVE 'A' TO F{i}."),
                "    GOBACK.".to_string(),
            ],
        ));
        out.push_str(&format!("       END PROGRAM PG{i}.\n"));
    }
    out
}

/// A flat procedure division of PERFORMs.
fn flat_performs(n: usize) -> String {
    let mut body = vec!["MAIN-PARA.".to_string()];
    for i in 0..n {
        body.push(format!("    PERFORM P{i}."));
    }
    body.push("    GOBACK.".to_string());
    for i in 0..n {
        body.push(format!("P{i}."));
        body.push("    CONTINUE.".to_string());
    }
    program("FLAT", &["01  WS-A PIC X(10).".to_string()], &body)
}

fn time(text: &str) -> (f64, usize, usize) {
    let start = Instant::now();
    let ex = CicsSqlExtractor::new()
        .extract(&SourceFile {
            path: "MEASURE.cbl".to_string(),
            language: Language::new("cobol"),
            text: text.to_string(),
        })
        .unwrap();
    (start.elapsed().as_secs_f64(), ex.nodes.len(), ex.refs.len())
}

#[test]
fn measure_cics_sql_extract_scaling_641() {
    if std::env::var_os("MEASURE_641").is_none() {
        return;
    }
    let shapes: [(&str, fn(usize) -> String, usize); 3] = [
        ("a paragraph per CICS LINK", cics_links, 4_000),
        (
            "sequential programs, a field and a data use each",
            many_programs,
            1_000,
        ),
        (
            "a flat procedure division of PERFORMs",
            flat_performs,
            4_000,
        ),
    ];
    for (shape, make, n) in shapes {
        let mut prev = None;
        for k in [1, 2, 4] {
            let text = make(n * k);
            let lines = text.lines().count();
            let (secs, nodes, refs) = time(&text);
            let ratio = prev.map_or(String::new(), |p: f64| format!(" ratio {:.2}", secs / p));
            println!(
                "MEASURE641 | {shape} | N={} | lines {lines} | {secs:.3} s | nodes {nodes} refs {refs}{ratio}",
                n * k
            );
            prev = Some(secs);
        }
    }
}
