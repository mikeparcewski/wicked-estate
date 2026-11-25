//! `wicked-estate convert <file>` (wicked-estate#556): a binary document to Markdown on stdout, in
//! the isolated conversion child (#442), storing nothing. A refusal leaves stdout empty, prints one
//! `refused <kind>: <reason>` line on stderr and exits with that kind's code, so a caller (crew's
//! chat attachments, wicked-crew#1542) can branch on it without parsing prose.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../wicked-estate-extract/tests/fixtures/docs"
    ))
    .join(name)
}

/// Run `convert` in an empty scratch directory with no store configured, and check it left the
/// directory empty: the command opens no graph and writes nothing.
fn convert(file: &Path, extra: &[&str]) -> Output {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(tmp.path())
        .arg("convert")
        .arg(file)
        .args(extra)
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .env("WICKED_ESTATE_EMIT_PROGRAM", "wicked-bus-absent-convert-cli")
        .output()
        .expect("spawn wicked-estate");
    let left: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "convert wrote into its cwd: {left:?}");
    out
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 markdown")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_converted(out: &Output) -> String {
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={} stderr={}",
        stdout(out),
        stderr(out)
    );
    stdout(out)
}

/// A refusal: the documented exit code, nothing on stdout, and exactly one stderr line naming the
/// kind.
fn assert_refused(out: &Output, kind: &str, code: i32) -> String {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(code), "{kind}: stderr={err}");
    assert!(out.stdout.is_empty(), "{kind}: stdout={}", stdout(out));
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 1, "{kind}: one stderr line: {err}");
    let prefix = format!("refused {kind}: ");
    assert!(lines[0].starts_with(&prefix), "{kind}: {err}");
    lines[0][prefix.len()..].to_string()
}

#[test]
fn docx_converts_to_markdown_with_the_secret_masked() {
    let md = assert_converted(&convert(&fixture("spec.docx"), &[]));
    assert!(md.contains("Ledger reconciliation spec"), "{md}");
    assert!(md.contains("reconcile_ledger"), "{md}");
    // The fixture's fake AWS key canary is masked, as knowledge.ingest masks it.
    assert!(!md.contains("AKIAABCDEFGHIJKLMNOP"), "{md}"); // ggignore: fake canary key
}

#[test]
fn pptx_xlsx_and_text_pdf_convert() {
    let md = assert_converted(&convert(&fixture("deck.pptx"), &[]));
    assert!(md.contains("settle_invoice"), "{md}");
    let md = assert_converted(&convert(&fixture("budget.xlsx"), &[]));
    for cell in ["cost_center", "retry_budget", "ledger_sync", "1250"] {
        assert!(md.contains(cell), "{cell}: {md}");
    }
    let md = assert_converted(&convert(&fixture("runbook.pdf"), &[]));
    assert!(md.contains("settle_invoice"), "{md}");
}

#[test]
fn a_scanned_pdf_says_it_needs_ocr() {
    let why = assert_refused(&convert(&fixture("scanned.pdf"), &[]), "needs_ocr", 6);
    assert!(why.contains("OCR"), "{why}");
}

#[test]
fn a_file_over_max_bytes_is_refused_before_it_is_read() {
    let len = std::fs::metadata(fixture("spec.docx")).unwrap().len();
    let cap = (len - 1).to_string();
    let why = assert_refused(
        &convert(&fixture("spec.docx"), &["--max-bytes", &cap]),
        "too_large",
        3,
    );
    assert!(why.contains(&format!("{len} bytes")), "{why}");
    assert!(why.contains(&format!("limit {cap}")), "{why}");
}

#[test]
fn a_conversion_past_its_deadline_is_killed() {
    let why = assert_refused(
        &convert(&fixture("long.docx"), &["--deadline-ms", "1"]),
        "deadline",
        7,
    );
    assert!(why.contains("1 ms"), "{why}");
}

#[test]
fn an_unknown_format_is_unsupported() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("notes.docx");
    std::fs::write(&path, b"plain words, not a document").unwrap();
    assert_refused(&convert(&path, &[]), "unsupported", 4);
}

#[test]
fn a_missing_file_is_unreadable() {
    let tmp = tempfile::tempdir().unwrap();
    assert_refused(
        &convert(&tmp.path().join("gone.pdf"), &[]),
        "unreadable",
        10,
    );
}

/// The crafted PDF from #442 (a FlateDecode predictor with 2^32 columns): the child's memory
/// budget refuses it, and the parent reports it.
#[test]
fn a_hostile_pdf_stays_inside_the_memory_budget() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
        concat!(
            "<< /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /DecodeParms ",
            "<< /Predictor 12 /Columns 4294967295 >> /Length 0 >>\n",
            "stream\nendstream"
        ),
    ];
    let mut pdf = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        writeln!(pdf, "{} 0 obj\n{object}\nendobj", i + 1).unwrap();
    }
    let xref = pdf.len();
    writeln!(pdf, "xref\n0 5\n0000000000 65535 f ").unwrap();
    for offset in offsets {
        writeln!(pdf, "{offset:010} 00000 n ").unwrap();
    }
    writeln!(
        pdf,
        "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF"
    )
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("crafted.pdf");
    std::fs::write(&path, pdf).unwrap();
    assert_refused(&convert(&path, &[]), "over_budget", 8);
}

#[test]
fn limits_are_checked_before_any_work() {
    // Above the converter's own limits, and zero, are usage errors, not silent clamps.
    for (flag, value) in [
        ("--max-bytes", "0"),
        ("--max-bytes", "33554433"),
        ("--deadline-ms", "0"),
        ("--deadline-ms", "120001"),
    ] {
        let out = convert(&fixture("spec.docx"), &[flag, value]);
        assert_eq!(out.status.code(), Some(1), "{flag} {value}: {}", stderr(&out));
        assert!(out.stdout.is_empty(), "{flag} {value}");
        assert!(
            stderr(&out).contains("usage: wicked-estate convert"),
            "{flag} {value}: {}",
            stderr(&out)
        );
    }
}
