//! A hostile PDF must stay inside the converter child, and its skip must survive unchanged runs.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use wicked_estate_core::GraphRead;
use wicked_estate_store::SqliteStore;

fn crafted_pdf() -> Vec<u8> {
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
    pdf
}

fn run(root: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(root)
        .args(args)
        .args(["--db", "graph.db"])
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .output()
        .expect("spawn wicked-estate");
    assert!(
        out.status.success(),
        "{args:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    out
}

#[test]
fn crafted_pdf_is_memory_skipped_and_recorded_until_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let docs = tmp.path().join("repo/docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("crafted.pdf"), crafted_pdf()).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../wicked-estate-extract/tests/fixtures/docs/runbook.pdf"
        ),
        docs.join("runbook.pdf"),
    )
    .unwrap();

    let reason = "not convertible (over the 1 GiB conversion memory budget)";
    let line = format!("DOC-SKIPPED: docs/crafted.pdf: {reason}");
    for _ in 0..2 {
        let out = run(tmp.path(), &["index", "repo"]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.lines().any(|s| s == line), "{stderr}");

        let out = run(tmp.path(), &["stats", "--json"]);
        let stats: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            stats["skipped_documents"],
            serde_json::json!({
                "count": 1,
                "files": [{"path": "docs/crafted.pdf", "reason": reason}],
            }),
            "{stats}"
        );
        let store = SqliteStore::open_readonly(tmp.path().join("graph.db")).unwrap();
        let text = store
            .file_content("docs/runbook.pdf")
            .unwrap()
            .expect("the text PDF is indexed alongside the hostile PDF");
        assert!(text.contains("retry_budget"), "{text}");
        assert!(store.file_content("docs/crafted.pdf").unwrap().is_none());
    }

    std::fs::remove_file(docs.join("crafted.pdf")).unwrap();
    let out = run(tmp.path(), &["index", "repo"]);
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("DOC-SKIPPED:"),
        "a deleted document drops out of the record"
    );
    let out = run(tmp.path(), &["stats", "--json"]);
    let stats: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(stats["skipped_documents"]["count"], 0, "{stats}");
}
