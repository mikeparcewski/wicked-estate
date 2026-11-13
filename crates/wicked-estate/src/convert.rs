//! `wicked-estate convert --chunks <file>`: one file to chunks of text with their locations, as
//! one JSON object. Local only: no network, no store.
//!
//! The file is sniffed by CONTENT, not by name:
//!
//! * an image (PNG, JPEG, GIF, WebP) has no text: status `needs_vision`, `pages` 1;
//! * a binary document (PDF, ZIP-packaged Office/OpenDocument/EPUB, OLE2 Office, RTF; the
//!   `doc_convert::DOC_EXTENSIONS` set) goes through [`doc_convert::to_markdown`] and is chunked
//!   with [`doc_convert::chunk_markdown_located`]. The 32 MiB input and 1 MiB text caps of
//!   `doc_convert` hold. A PDF with no text layer is `needs_vision` with its page count;
//! * anything else is text (Markdown, plain text): at most 1 MiB and valid UTF-8, else refused,
//!   and chunked with the same chunker.
//!
//! Output: `{status: "ok"|"needs_vision"|"refused", reason?, pages?, chunks: [{text, location:
//! {heading_path, ordinal, page?}}]}`. `ordinal` is the chunk's zero-based position in the file.
//! `page` is set only where the converter's Markdown marks pages. None of anydoc's Markdown output
//! is known to carry a page marker (the chunker reads headings, nothing else), so no format sets
//! it today: the field is omitted, never guessed. A format that gains markers sets it in
//! [`page_of`], the one place that reads them.
//!
//! A detected secret refuses the file: the credential detectors of the redactor
//! ([`redact::has_secret`]: token shapes, private keys, URL credentials, a value assigned to a
//! secret-named key) over the text and every heading, or a secret file name
//! ([`redact::is_excluded`]). The refusal carries no chunks and no text. This is the DETECTION
//! half of the redactor; the default-deny value masking of config and code files is not applied,
//! so `interest.compound: false` and `WS-INTEREST-PERIOD is 'DAILY'` pass.
//!
//! Diagnostics are the fixed `reason` codes below, never converter text.
//!
//! Exit codes: 0 ok, 3 needs_vision, 4 refused, 2 usage or I/O error (no JSON on stdout).

use std::path::Path;

use serde_json::{Value, json};
use wicked_estate_extract::doc_convert::{self, LocatedChunk, Skipped};
use wicked_estate_extract::redact;

const USAGE: &str = "usage: wicked-estate convert --chunks <file>";

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_NEEDS_VISION: i32 = 3;
pub const EXIT_REFUSED: i32 = 4;

/// What `convert` answers: the process exit code and the JSON object for stdout (`None` for a
/// usage or I/O error, which prints nothing on stdout).
#[derive(Debug)]
pub struct Outcome {
    pub exit: i32,
    pub json: Option<Value>,
}

enum Kind {
    Image,
    Document,
    Text,
}

/// The kind of content `bytes` is, from its leading bytes.
fn sniff(bytes: &[u8]) -> Kind {
    let image = bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP");
    if image {
        return Kind::Image;
    }
    let document = bytes.starts_with(b"%PDF-")
        || bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
        || bytes.starts_with(b"{\\rtf");
    if document {
        return Kind::Document;
    }
    Kind::Text
}

/// The page a chunk starts on, where the converter's Markdown marks pages. No format does today
/// (see the module doc), so this answers `None` for every chunk.
fn page_of(_chunk: &LocatedChunk) -> Option<u32> {
    None
}

fn refused(reason: &str) -> Outcome {
    Outcome {
        exit: EXIT_REFUSED,
        json: Some(json!({"status": "refused", "reason": reason, "chunks": []})),
    }
}

fn needs_vision(pages: Option<u32>) -> Outcome {
    let mut v = json!({"status": "needs_vision", "chunks": []});
    if let Some(n) = pages {
        v["pages"] = json!(n);
    }
    Outcome {
        exit: EXIT_NEEDS_VISION,
        json: Some(v),
    }
}

fn usage(why: &str) -> Outcome {
    eprintln!("convert: {why}\n{USAGE}");
    Outcome {
        exit: EXIT_USAGE,
        json: None,
    }
}

/// Run `convert` over its arguments (after the verb): exactly `--chunks <file>`.
pub fn run(args: &[String]) -> Outcome {
    let mut chunks = false;
    let mut file: Option<&str> = None;
    for a in args {
        match a.as_str() {
            "--chunks" if !chunks => chunks = true,
            "--chunks" => return usage("--chunks given more than once"),
            f if f.starts_with("--") => return usage("unknown flag"),
            f if file.is_none() => file = Some(f),
            _ => return usage("one file only"),
        }
    }
    match (chunks, file) {
        (true, Some(f)) => convert_chunks(Path::new(f)),
        (false, _) => usage("--chunks is required"),
        (true, None) => usage("a file is required"),
    }
}

/// Chunk one file; see the module doc.
pub fn convert_chunks(path: &Path) -> Outcome {
    let shown = path.to_string_lossy();
    if redact::is_excluded(&shown) {
        return refused("secret_filename");
    }
    let bytes = match doc_convert::read_capped(path) {
        Ok(b) => b,
        Err(Skipped::TooLarge(_)) => return refused("too_large"),
        Err(_) => return usage("cannot read the file"),
    };
    let md: String = match sniff(&bytes) {
        Kind::Image => return needs_vision(Some(1)),
        Kind::Document => match doc_convert::to_markdown(&shown, &bytes) {
            Ok(md) => md,
            Err(skip) => {
                return match skip {
                    Skipped::NeedsOcr(_) => needs_vision(skip.ocr_pages()),
                    Skipped::TooLarge(_) => refused("too_large"),
                    Skipped::Encrypted => refused("encrypted"),
                    Skipped::Unreadable(_) => refused("unreadable"),
                    Skipped::Panicked => refused("converter_failed"),
                };
            }
        },
        Kind::Text => {
            if bytes.len() > doc_convert::MAX_TEXT_BYTES {
                return refused("too_large");
            }
            match String::from_utf8(bytes) {
                Ok(t) => t,
                Err(_) => return refused("invalid_utf8"),
            }
        }
    };
    let located = doc_convert::chunk_markdown_located(&md);
    // The whole text and each heading, so a secret in a heading is caught even where chunking
    // would cut it.
    if redact::has_secret(&md)
        || located
            .iter()
            .any(|c| c.heading_path.iter().any(|h| redact::has_secret(h)))
    {
        return refused("secret_detected");
    }
    let chunks: Vec<Value> = located
        .iter()
        .enumerate()
        .map(|(ordinal, c)| {
            let mut location = json!({"heading_path": c.heading_path, "ordinal": ordinal});
            if let Some(p) = page_of(c) {
                location["page"] = json!(p);
            }
            json!({"text": c.text, "location": location})
        })
        .collect();
    Outcome {
        exit: EXIT_OK,
        json: Some(json!({"status": "ok", "chunks": chunks})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wicked-estate-extract/tests/fixtures/docs")
            .join(name)
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn status(o: &Outcome) -> &str {
        o.json.as_ref().unwrap()["status"].as_str().unwrap()
    }

    #[test]
    fn markdown_is_chunked_with_heading_path_and_ordinal() {
        let tmp = tempfile::tempdir().unwrap();
        let md = "# Ledger\n\nintro words\n\n## Rounding\n\nround half even\n\n### Edge\n\nzero amounts\n";
        let o = convert_chunks(&write(tmp.path(), "spec.md", md.as_bytes()));
        assert_eq!(o.exit, EXIT_OK);
        let v = o.json.unwrap();
        assert_eq!(v["status"], "ok");
        let chunks = v["chunks"].as_array().unwrap();
        assert_eq!(chunks.len(), 2, "{v}");
        assert_eq!(chunks[0]["location"]["heading_path"], json!(["Ledger"]));
        assert_eq!(chunks[0]["location"]["ordinal"], 0);
        assert_eq!(
            chunks[1]["location"]["heading_path"],
            json!(["Ledger", "Rounding"])
        );
        assert_eq!(chunks[1]["location"]["ordinal"], 1);
        let text = chunks[1]["text"].as_str().unwrap();
        assert!(text.contains("zero amounts"));
        assert!(chunks[0]["location"].get("page").is_none());
    }

    #[test]
    fn a_docx_converts_with_locations() {
        let o = convert_chunks(&fixture("long.docx"));
        assert_eq!(o.exit, EXIT_OK, "{:?}", o.json);
        let v = o.json.unwrap();
        assert_eq!(v["status"], "ok");
        let chunks = v["chunks"].as_array().unwrap();
        assert!(!chunks.is_empty());
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c["location"]["ordinal"], i);
            assert!(c["location"]["heading_path"].is_array());
        }
    }

    #[test]
    fn a_png_needs_vision_with_one_page() {
        let tmp = tempfile::tempdir().unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0u8; 32]);
        let o = convert_chunks(&write(tmp.path(), "shot.png", &png));
        assert_eq!(o.exit, EXIT_NEEDS_VISION);
        let v = o.json.unwrap();
        assert_eq!(v["status"], "needs_vision");
        assert_eq!(v["pages"], 1);
        assert_eq!(v["chunks"], json!([]));
    }

    #[test]
    fn every_image_signature_needs_vision() {
        let tmp = tempfile::tempdir().unwrap();
        let mut webp = b"RIFF\x00\x00\x00\x00WEBPVP8 ".to_vec();
        webp.extend_from_slice(&[0u8; 8]);
        for (name, bytes) in [
            ("a.jpg", vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0]),
            ("b.gif", b"GIF89a\x01\x00\x01\x00".to_vec()),
            ("c.webp", webp),
        ] {
            let o = convert_chunks(&write(tmp.path(), name, &bytes));
            assert_eq!(o.exit, EXIT_NEEDS_VISION, "{name}");
        }
    }

    #[test]
    fn a_scanned_pdf_needs_vision_with_its_page_count() {
        let o = convert_chunks(&fixture("scanned.pdf"));
        assert_eq!(o.exit, EXIT_NEEDS_VISION, "{:?}", o.json);
        let v = o.json.unwrap();
        assert_eq!(v["status"], "needs_vision");
        assert!(v["pages"].as_u64().is_some_and(|n| n >= 1), "{v}");
        assert_eq!(v["chunks"], json!([]));
    }

    #[test]
    fn a_text_pdf_converts() {
        let o = convert_chunks(&fixture("runbook.pdf"));
        assert_eq!(o.exit, EXIT_OK, "{:?}", o.json);
    }

    #[test]
    fn a_ghp_token_refuses_the_file_and_the_output_holds_no_text() {
        let tmp = tempfile::tempdir().unwrap();
        let token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let o = convert_chunks(&write(
            tmp.path(),
            "notes.md",
            format!("# Deploy\n\nthe token is {token} keep it\n").as_bytes(),
        ));
        assert_eq!(o.exit, EXIT_REFUSED);
        let v = o.json.unwrap();
        assert_eq!(v["status"], "refused");
        assert_eq!(v["reason"], "secret_detected");
        assert_eq!(v["chunks"], json!([]));
        let out = v.to_string();
        assert!(!out.contains("ghp_") && !out.contains("keep it"), "{out}");
    }

    #[test]
    fn a_private_key_or_a_secret_named_assignment_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJBAKj34GkxFhD90vcNLYLInFEX6Ppy1tPf9Cnzj4p4WGeKLs1Pt8Qu\n-----END RSA PRIVATE KEY-----\n";
        let o = convert_chunks(&write(tmp.path(), "k.md", pem.as_bytes()));
        assert_eq!(o.json.unwrap()["reason"], "secret_detected");
        let o = convert_chunks(&write(tmp.path(), "c.txt", b"password: hunter2\n"));
        assert_eq!(o.exit, EXIT_REFUSED);
    }

    #[test]
    fn a_document_holding_a_credential_is_refused() {
        // spec.docx carries an AWS access key id canary.
        let o = convert_chunks(&fixture("spec.docx"));
        assert_eq!(o.exit, EXIT_REFUSED);
        let v = o.json.unwrap();
        assert_eq!(v["reason"], "secret_detected");
        assert!(!v.to_string().contains("AKIA"));
    }

    #[test]
    fn a_secret_file_name_is_refused_whatever_it_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let o = convert_chunks(&write(tmp.path(), ".env", b"HELLO=world\n"));
        assert_eq!(o.exit, EXIT_REFUSED);
        let v = o.json.unwrap();
        assert_eq!(v["reason"], "secret_filename");
        assert_eq!(v["chunks"], json!([]));
    }

    #[test]
    fn domain_prose_is_not_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let text = "# Interest\n\nWS-INTEREST-PERIOD is 'DAILY'.\n\ninterest.compound: false\n";
        let o = convert_chunks(&write(tmp.path(), "rules.md", text.as_bytes()));
        assert_eq!(o.exit, EXIT_OK, "{:?}", o.json);
        let v = o.json.unwrap();
        let all: String = v["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["text"].as_str().unwrap())
            .collect();
        assert!(all.contains("WS-INTEREST-PERIOD is 'DAILY'."));
        assert!(all.contains("compound: false"));
    }

    #[test]
    fn invalid_utf8_and_oversized_text_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let o = convert_chunks(&write(tmp.path(), "bad.txt", b"caf\xE9 \xFF\xFE text"));
        assert_eq!(o.exit, EXIT_REFUSED);
        assert_eq!(o.json.unwrap()["reason"], "invalid_utf8");
        let big = vec![b'a'; doc_convert::MAX_TEXT_BYTES + 1];
        let o = convert_chunks(&write(tmp.path(), "big.txt", &big));
        assert_eq!(o.exit, EXIT_REFUSED);
        assert_eq!(o.json.unwrap()["reason"], "too_large");
        let ok = vec![b'a'; doc_convert::MAX_TEXT_BYTES];
        let o = convert_chunks(&write(tmp.path(), "edge.txt", &ok));
        assert_eq!(status(&o), "ok");
    }

    #[test]
    fn usage_and_io_errors_exit_2_with_nothing_on_stdout() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for a in [
            args(&[]),
            args(&["--chunks"]),
            args(&["f.md"]),
            args(&["--chunks", "a", "b"]),
            args(&["--chunks", "--bogus", "a"]),
            args(&["--chunks", "/no/such/file.md"]),
        ] {
            let o = run(&a);
            assert_eq!(o.exit, EXIT_USAGE, "{a:?}");
            assert!(o.json.is_none());
        }
    }
}
