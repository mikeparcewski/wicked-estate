//! `wicked-estate convert <file>` (wicked-estate#556): a binary document to Markdown on stdout, in
//! the isolated conversion child (#442), storing nothing. A refusal leaves stdout empty, prints one
//! `refused <kind>: <reason>` line on stderr and exits with that kind's code, so a caller (crew's
//! chat attachments, wicked-crew#1542) can branch on it without parsing prose.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../wicked-estate-extract/tests/fixtures/docs"
    ))
    .join(name)
}

/// Run the binary under an outer limit: a hang fails the test instead of hanging it. Returns the
/// output and the wall time.
fn run_bounded(mut cmd: Command, limit: Duration) -> (Output, Duration) {
    let start = Instant::now();
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn wicked-estate");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = rx
        .recv_timeout(limit)
        .expect("wicked-estate did not return within the limit")
        .expect("wait for wicked-estate");
    (out, start.elapsed())
}

fn names(dir: &Path) -> Vec<std::ffi::OsString> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect()
}

/// Run `convert` in an empty cwd, with HOME, the XDG dirs and WICKED_HOME pointing at an empty
/// directory, and `store_env` as the store configuration (none by default). Afterwards the cwd,
/// the home and the directory a configured store would live in are all still empty.
fn convert_with(file: &Path, extra: &[&str], store_env: &[(&str, &str)]) -> (Output, Duration) {
    let cwd = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate"));
    cmd.current_dir(cwd.path())
        .arg("convert")
        .arg(file)
        .args(extra)
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .env("HOME", home.path())
        .env("WICKED_HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("XDG_DATA_HOME", home.path())
        .env("XDG_CACHE_HOME", home.path())
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-convert-cli",
        );
    for (k, v) in store_env {
        cmd.env(k, v);
    }
    let (out, took) = run_bounded(cmd, Duration::from_secs(120));
    assert!(
        names(cwd.path()).is_empty(),
        "convert wrote into its cwd: {:?}",
        names(cwd.path())
    );
    assert!(
        names(home.path()).is_empty(),
        "convert wrote into HOME: {:?}",
        names(home.path())
    );
    (out, took)
}

fn convert(file: &Path, extra: &[&str]) -> Output {
    convert_with(file, extra, &[]).0
}

/// An RTF document (RTF needs no zip container, so a test writes it inline).
fn rtf(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("{{\\rtf1\\ansi {body}}}")).unwrap();
    path
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

/// Secret-named assignments and bearer tokens are masked too: `knowledge.ingest` applies the
/// free-text rules on top of the Markdown rules, and so does `convert`.
#[test]
fn secret_assignments_are_masked_like_knowledge_ingest() {
    let tmp = tempfile::tempdir().unwrap();
    // ggignore: made-up secret values for the masking test
    let doc = rtf(
        tmp.path(),
        "secrets.rtf",
        "DB_PASSWORD=hunter2-example\\par password: marmoset\\par Authorization: Bearer innocentlookingsecret\\par API_KEY=\"shortsecret\"\\par",
    );
    let md = assert_converted(&convert(&doc, &[]));
    assert!(md.contains("DB_PASSWORD"), "{md}");
    for secret in [
        "hunter2-example",
        "marmoset",
        "innocentlookingsecret",
        "shortsecret",
    ] {
        assert!(!md.contains(secret), "{secret}: {md}");
    }
}

/// Masking runs in the isolated child, under its deadline and memory budget: the child's own
/// reply is already masked.
#[test]
fn the_isolated_child_masks_before_it_replies() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate"));
    cmd.args(["__doc-convert", "rtf", "mask"]);
    let child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // ggignore: made-up secret value for the masking test
    child
        .stdin
        .as_ref()
        .unwrap()
        .write_all(b"{\\rtf1\\ansi DB_PASSWORD=hunter2-example\\par}")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let reply = String::from_utf8(out.stdout).unwrap();
    assert!(reply.starts_with("OK\n"), "{reply}");
    assert!(reply.contains("DB_PASSWORD"), "{reply}");
    assert!(!reply.contains("hunter2-example"), "{reply}");
}

/// No store configuration reaches `convert`, valid or broken: a team runtime with no URL, an
/// unknown runtime, or a store path. The command and its help still work, and nothing appears
/// where that store would be.
#[test]
fn store_configuration_never_reaches_convert_or_its_help() {
    let elsewhere = tempfile::tempdir().unwrap();
    let db = elsewhere.path().join("graph.db");
    let db = db.to_str().unwrap();
    for store_env in [
        &[("WICKED_RUNTIME", "team")][..],
        &[("WICKED_RUNTIME", "no-such-runtime")],
        &[("WICKED_ESTATE_DB", db)],
        &[
            ("WICKED_RUNTIME", "team"),
            ("WICKED_STORE_URL", "postgres://127.0.0.1:1/none"),
        ],
    ] {
        let (out, _) = convert_with(&fixture("deck.pptx"), &[], store_env);
        assert!(
            assert_converted(&out).contains("settle_invoice"),
            "{store_env:?}"
        );
        let (help, _) = convert_with(Path::new("--help"), &[], store_env);
        assert_eq!(
            help.status.code(),
            Some(0),
            "{store_env:?}: {}",
            stderr(&help)
        );
        assert!(
            stdout(&help).contains("wicked-estate convert <file>"),
            "{store_env:?}: {}",
            stdout(&help)
        );
        assert!(names(elsewhere.path()).is_empty(), "{store_env:?}");
    }
}

/// A signatureless CSV is text, not a document: unsupported, like any extension outside the
/// document set.
#[test]
fn a_csv_is_not_a_document() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("notes.csv");
    std::fs::write(&path, "a,b\nx,y\n").unwrap();
    assert_refused(&convert(&path, &[]), "unsupported", 4);
}

/// A file name that is not UTF-8 is a typed refusal, not a panic.
#[cfg(unix)]
#[test]
fn a_non_utf8_file_name_is_refused_not_a_panic() {
    use std::os::unix::ffi::OsStrExt;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(std::ffi::OsStr::from_bytes(b"bad\xff.pdf"));
    let why = assert_refused(&convert(&path, &[]), "unreadable", 10);
    assert!(why.contains("not valid UTF-8"), "{why}");
}

/// The #442 budget catches an INFALLIBLE allocation too: a TIFF predictor with 2^30 colours makes
/// the converter request a huge row buffer that cannot fail gracefully, so the child aborts. That
/// is still the budget (8), not a converter failure (12).
#[test]
fn an_aborting_allocation_is_over_budget_not_failed() {
    // A zlib stream holding one stored byte (0x00): header, stored block, adler32.
    let stream: &[u8] = &[
        0x78, 0x01, 0x01, 0x01, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x01, 0x00, 0x01,
    ];
    let mut objects: Vec<Vec<u8>> = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
    ]
    .iter()
    .map(|o| o.as_bytes().to_vec())
    .collect();
    let mut stm = concat!(
        "<< /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /DecodeParms ",
        "<< /Predictor 2 /Columns 1 /Colors 1073741824 /BitsPerComponent 1 >> /Length 12 >>\n",
        "stream\n"
    )
    .as_bytes()
    .to_vec();
    stm.extend_from_slice(stream);
    stm.extend_from_slice(b"\nendstream");
    objects.push(stm);
    let mut pdf = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        writeln!(pdf, "{} 0 obj", i + 1).unwrap();
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
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
    let path = tmp.path().join("tiff.pdf");
    std::fs::write(&path, pdf).unwrap();
    assert_refused(&convert(&path, &[]), "over_budget", 8);
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

/// The deadline bounds conversion AND masking, and the refusal comes when the deadline does, not
/// when the work would have finished: a document whose conversion and masking take a measurable
/// time is refused under a 1 ms deadline in less time than the full run takes.
#[test]
fn a_conversion_past_its_deadline_is_killed() {
    let why = assert_refused(
        &convert(&fixture("long.docx"), &["--deadline-ms", "1"]),
        "deadline",
        7,
    );
    assert!(why.contains("1 ms"), "{why}");

    let tmp = tempfile::tempdir().unwrap();
    // Many short emphasis runs: cheap to convert, slow for the Markdown masking parser.
    let doc = rtf(
        tmp.path(),
        "emphasis.rtf",
        &format!("{}password=x", "*a ".repeat(300_000)),
    );
    let (full, full_took) = convert_with(&doc, &[], &[]);
    assert_converted(&full);
    let (cut, cut_took) = convert_with(&doc, &["--deadline-ms", "1"], &[]);
    assert_refused(&cut, "deadline", 7);
    assert!(
        cut_took < full_took,
        "the 1 ms deadline took {cut_took:?}, the full run {full_took:?}"
    );
}

#[test]
fn an_unknown_format_is_unsupported_and_a_broken_one_malformed() {
    let tmp = tempfile::tempdir().unwrap();
    // No signature and no document extension: nothing names a format.
    let path = tmp.path().join("notes.bin");
    std::fs::write(&path, b"plain words, not a document").unwrap();
    assert_refused(&convert(&path, &[]), "unsupported", 4);
    // The extension names Word, and the bytes are not a Word document.
    let path = tmp.path().join("notes.docx");
    std::fs::write(&path, b"plain words, not a document").unwrap();
    assert_refused(&convert(&path, &[]), "malformed", 11);
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
        assert_eq!(
            out.status.code(),
            Some(1),
            "{flag} {value}: {}",
            stderr(&out)
        );
        assert!(out.stdout.is_empty(), "{flag} {value}");
        assert!(
            stderr(&out).contains("usage: wicked-estate convert"),
            "{flag} {value}: {}",
            stderr(&out)
        );
    }
}

/// A PDF under a user password is refused as encrypted. The fixture is regenerated by
/// wicked-estate-extract/tests/fixture-tools/make_locked_pdf.py.
#[test]
fn a_password_protected_pdf_is_encrypted() {
    assert_refused(&convert(&fixture("locked.pdf"), &[]), "encrypted", 5);
}

/// R2 F1: the conversion child bounds itself. A caller that kills `convert` on its own timeout
/// leaves the child with no parent to kill it; the child still ends at the deadline it was given,
/// here with its input never even finished.
#[test]
fn an_orphaned_conversion_child_ends_at_its_own_deadline() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .args(["__doc-convert", "docx", "deadline=300"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the conversion child");
    // Held open and never written: the child stalls on its input.
    let stdin = child.stdin.take();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(stdin);
    assert!(
        status.is_some(),
        "the child outlived its 300 ms deadline by 30 s"
    );
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut reply).unwrap();
    assert!(
        reply.starts_with("SKIP\nunreadable\t") && reply.contains("took longer"),
        "{reply:?}"
    );
}

/// R2 F2: a stdout that cannot take the Markdown is its own exit code, never 1 (usage).
#[cfg(unix)]
#[test]
fn a_stdout_write_failure_has_its_own_code() {
    let home = tempfile::tempdir().unwrap();
    // A pipe with no reader before the process starts: its write fails (EPIPE) whatever the
    // scheduling.
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let child = Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .arg("convert")
        .arg(fixture("spec.docx"))
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("WICKED_HOME", home.path())
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .stdin(Stdio::null())
        .stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn wicked-estate");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = rx
        .recv_timeout(Duration::from_secs(120))
        .expect("wicked-estate did not return within the limit")
        .expect("wait for wicked-estate");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(13), "stderr={err}");
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 1, "{err}");
    assert!(lines[0].starts_with("refused output: "), "{err}");
}

/// R2 F3: `knowledge.ingest`'s secret-file rule applies: a file it would not read is refused,
/// unread, with its own code. Through a link with an innocent name too.
#[test]
fn a_secret_named_file_is_refused_unread() {
    let tmp = tempfile::tempdir().unwrap();
    let secret = tmp.path().join("credentials.docx");
    std::fs::copy(fixture("spec.docx"), &secret).unwrap();
    let why = assert_refused(&convert(&secret, &[]), "secret_file", 14);
    assert!(why.contains("secret-file rule"), "{why}");
    #[cfg(unix)]
    {
        let link = tmp.path().join("notes.docx");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert_refused(&convert(&link, &[]), "secret_file", 14);
    }
}
