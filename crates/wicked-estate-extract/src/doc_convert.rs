//! Binary documents (PDF, Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) to GitHub-flavored
//! Markdown, in-process, with the [anydoc] crate. This is the ONE converter estate uses wherever it
//! meets a document it cannot read as text: the repo index stores and searches the Markdown under
//! the document's own path, and `knowledge.ingest {path}` chunks it.
//!
//! Local only. The anydoc Rust crate has no OCR and no network code (its hosted OCR lives only in
//! the npm wrapper, which estate does not use): a scanned or image-only PDF comes back as
//! [`Skipped::NeedsOcr`], and nothing leaves the host. The test
//! `anydoc_dependency_closure_has_no_network_client` is a tripwire on the dependency closure.
//!
//! Bounds, and what they do not cover: the input is capped at [`MAX_INPUT_BYTES`] before it is
//! read ([`read_capped`]); at most [`CONCURRENT_CONVERSIONS`] documents convert at once, so a repo
//! of documents holds a bounded number of parses in memory; anydoc bounds Office/ODF/EPUB
//! decompression (128 MiB per archive entry, 512 MiB per archive, 2M XML nodes) and pdf-inspector
//! bounds PDF streams per stream and per page. There is no document-wide PDF memory budget and no
//! timer (a thread cannot be preempted), and an abort (out of memory, stack overflow) inside the
//! parser ends the process: a hostile PDF can cost memory or time. Unwinding panics are caught and
//! reported as a skip. Skip reasons are fixed strings; document text never reaches a reason.
//!
//! [anydoc]: https://github.com/firecrawl/anydoc

use std::fmt;
use std::io::Read;
use std::sync::{Condvar, Mutex};

/// Largest document converted, in bytes (32 MiB). Bigger files are skipped, never parsed.
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// Documents converted at once, process-wide (each waits for a slot).
pub const CONCURRENT_CONVERSIONS: usize = 2;

/// Extensions (lowercase, no dot) this converter reads: every extension anydoc names except CSV,
/// which is already text.
pub const DOC_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "docm", "odt", "rtf", "epub", "ppt", "pps", "pot", "pptx", "pptm",
    "ppsx", "ppsm", "odp", "xls", "xlsx", "xlsm", "xlsb", "ods",
];

/// True when `ext` (any case, no dot) names a document this converter reads.
pub fn is_doc_ext(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    DOC_EXTENSIONS.contains(&ext.as_str())
}

/// True when `path`'s final extension names a document this converter reads.
pub fn is_doc_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, e)| is_doc_ext(e))
}

/// Why a document was not converted. Never fatal to the caller: it records the reason and moves on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// Scanned or image-only pages (the pages, e.g. "all 3 pages need OCR"): anydoc does no OCR,
    /// and estate never sends a document out.
    NeedsOcr(String),
    /// Over [`MAX_INPUT_BYTES`] (at least this many bytes).
    TooLarge(u64),
    /// Encrypted or password-protected.
    Encrypted,
    /// Unreadable, unsupported, over one of the converter's safety limits, or empty of text. A
    /// fixed reason, never document text.
    Unreadable(&'static str),
    /// The converter panicked on this input.
    Panicked,
}

impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Skipped::NeedsOcr(pages) => write!(f, "needs OCR ({pages}); not converted"),
            Skipped::TooLarge(n) => write!(
                f,
                "too large ({n} bytes, limit {MAX_INPUT_BYTES}); not converted"
            ),
            Skipped::Encrypted => write!(f, "encrypted; not converted"),
            Skipped::Unreadable(why) => write!(f, "not convertible ({why})"),
            Skipped::Panicked => write!(f, "converter failed on this file; not converted"),
        }
    }
}

/// Read `path`, refusing past [`MAX_INPUT_BYTES`] without reading the rest (the size is checked
/// before the read, and the read itself is capped, so a file that grows meanwhile is still bounded).
/// Only a regular file is opened: a FIFO or device named `x.pdf` is refused without blocking.
pub fn read_capped(path: &std::path::Path) -> Result<Vec<u8>, Skipped> {
    let meta = std::fs::metadata(path).map_err(|_| Skipped::Unreadable("cannot open"))?;
    if !meta.is_file() {
        return Err(Skipped::Unreadable("not a regular file"));
    }
    if meta.len() > MAX_INPUT_BYTES as u64 {
        return Err(Skipped::TooLarge(meta.len()));
    }
    let file = std::fs::File::open(path).map_err(|_| Skipped::Unreadable("cannot open"))?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Skipped::Unreadable("cannot read"))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Skipped::TooLarge(bytes.len() as u64));
    }
    Ok(bytes)
}

/// A counting gate over [`CONCURRENT_CONVERSIONS`] slots.
static SLOTS: Mutex<usize> = Mutex::new(0);
static SLOT_FREED: Condvar = Condvar::new();

struct Slot;

impl Slot {
    fn take() -> Slot {
        let mut busy = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
        while *busy >= CONCURRENT_CONVERSIONS {
            busy = SLOT_FREED.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut busy = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
        *busy -= 1;
        SLOT_FREED.notify_one();
    }
}

/// Largest converted text kept, in bytes (1 MiB, the index's own text limit). Both callers get it.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// A converted line longer than this is wrapped at whitespace (a soft break inside a Markdown
/// paragraph), so long prose paragraphs and table rows pass the extractors' minified-file guard.
const WRAP_AT: usize = 400;

/// Convert one document to Markdown. `path` only names the format when the bytes carry no
/// signature (anydoc detects the format from the content first). The text is post-processed for
/// indexing: inline-delimiter escapes are removed (`settle\_invoice` reads `settle_invoice`, so
/// identifiers and token shapes match as in any text) and lines over [`WRAP_AT`] characters are
/// wrapped at whitespace. It is NOT redacted here: callers redact it with the markdown rules, like
/// any `.md` file.
pub fn to_markdown(path: &str, bytes: &[u8]) -> Result<String, Skipped> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Skipped::TooLarge(bytes.len() as u64));
    }
    let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let converted = {
        // Detection opens containers too, so it runs under the slot and the unwind boundary.
        let _slot = Slot::take();
        // A malformed document must not take the index run down with it.
        std::panic::catch_unwind(|| {
            let format =
                anydoc::Format::from_bytes(bytes).or_else(|| anydoc::Format::from_extension(ext));
            format.map(|f| anydoc::to_markdown_bytes(bytes, f))
        })
        .map_err(|_| Skipped::Panicked)?
    };
    let Some(converted) = converted else {
        return Err(Skipped::Unreadable("unrecognized format"));
    };
    let md = match converted {
        Ok(md) => md,
        Err(e) => {
            return Err(match e {
                anydoc::ConvertError::NeedsOcr { .. } => {
                    // "page 2 of 5 needs OCR" / "all 3 pages need OCR": page numbers only.
                    Skipped::NeedsOcr(e.to_string())
                }
                anydoc::ConvertError::Encrypted => Skipped::Encrypted,
                anydoc::ConvertError::ResourceLimit { .. } => {
                    Skipped::Unreadable("over a converter safety limit")
                }
                anydoc::ConvertError::Unsupported(_) => Skipped::Unreadable("unsupported content"),
                anydoc::ConvertError::MissingPart { .. } => Skipped::Unreadable("missing part"),
                anydoc::ConvertError::Io(_) => Skipped::Unreadable("cannot read"),
                _ => Skipped::Unreadable("malformed document"),
            });
        }
    };
    if md.trim().is_empty() {
        return Err(Skipped::Unreadable("no text"));
    }
    if md.len() > MAX_TEXT_BYTES {
        return Err(Skipped::Unreadable(
            "converted text is over the 1 MiB limit",
        ));
    }
    Ok(wrap_long_lines(&unescape_delimiters(&md)))
}

/// `\_` `\*` `\~` and `` \` `` -> the bare character. Only the inline delimiters an escape can split
/// an identifier or token at; block-structure escapes (`\#`, `\-`, `\|`, ...) are kept, so a
/// paragraph that starts with `#` does not become a heading.
fn unescape_delimiters(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut chars = md.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && chars
                .peek()
                .is_some_and(|n| matches!(n, '_' | '*' | '~' | '`'))
        {
            continue;
        }
        out.push(c);
    }
    out
}

/// Wrap every line over [`WRAP_AT`] characters at the last whitespace before the limit.
fn wrap_long_lines(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + md.len() / WRAP_AT);
    for line in md.split_inclusive('\n') {
        let mut rest = line;
        while rest.chars().count() > WRAP_AT {
            let limit = rest
                .char_indices()
                .nth(WRAP_AT)
                .map_or(rest.len(), |(i, _)| i);
            let Some(cut) = rest[..limit].rfind(char::is_whitespace) else {
                break; // no whitespace to break at: leave the line whole
            };
            if cut == 0 {
                break;
            }
            out.push_str(rest[..cut].trim_end());
            out.push('\n');
            rest = rest[cut..].trim_start_matches([' ', '\t']);
        }
        out.push_str(rest);
    }
    out
}

/// Words a chunk from [`chunk_markdown`] holds at most.
pub const CHUNK_WORDS: usize = 800;

/// The opening fence of a code block: its character and length (CommonMark: 3+ backticks or tildes).
fn fence_open(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Split converted Markdown into self-contained chunks for knowledge ingest: one per H1/H2 section
/// (a heading inside a code fence is not a boundary), a section over [`CHUNK_WORDS`] words split at
/// paragraph breaks, and a single paragraph over it split at word boundaries. Empty chunks are
/// dropped.
pub fn chunk_markdown(md: &str) -> Vec<String> {
    let mut sections: Vec<String> = vec![String::new()];
    let mut fence: Option<(char, usize)> = None;
    for line in md.lines() {
        match (fence, fence_open(line)) {
            // A closing fence: the same character, at least as long, nothing after it.
            (Some((c, n)), Some((c2, n2)))
                if c == c2 && n2 >= n && line.trim().chars().all(|x| x == c) =>
            {
                fence = None;
            }
            (None, Some(open)) => fence = Some(open),
            _ => {}
        }
        if fence.is_none() && (line.starts_with("# ") || line.starts_with("## ")) {
            sections.push(String::new());
        }
        let cur = sections.last_mut().expect("never empty");
        cur.push_str(line);
        cur.push('\n');
    }
    let mut chunks = Vec::new();
    for section in sections {
        let mut cur = String::new();
        let mut words = 0;
        let mut flush = |cur: &mut String, words: &mut usize| {
            if !cur.trim().is_empty() {
                chunks.push(cur.trim().to_string());
            }
            cur.clear();
            *words = 0;
        };
        for para in section.split("\n\n") {
            let n = para.split_whitespace().count();
            if words > 0 && words + n > CHUNK_WORDS {
                flush(&mut cur, &mut words);
            }
            if n > CHUNK_WORDS {
                // One paragraph over the limit: split it by words.
                let all: Vec<&str> = para.split_whitespace().collect();
                for piece in all.chunks(CHUNK_WORDS) {
                    cur.push_str(&piece.join(" "));
                    words = piece.len();
                    flush(&mut cur, &mut words);
                }
                continue;
            }
            cur.push_str(para);
            cur.push_str("\n\n");
            words += n;
        }
        flush(&mut cur, &mut words);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/docs/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture")
    }

    #[test]
    fn docx_converts_to_markdown_with_its_heading_and_identifier() {
        let md = to_markdown("docs/spec.docx", &fixture("spec.docx")).expect("docx converts");
        assert!(md.contains("Ledger reconciliation spec"), "{md}");
        assert!(md.contains("reconcile_ledger"), "{md}");
        // The fixture really carries the canary that callers' markdown redaction must mask.
        assert!(md.contains("AKIAABCDEFGHIJKLMNOP"), "{md}");
    }

    #[test]
    fn pptx_converts_every_slide() {
        let md = to_markdown("deck.pptx", &fixture("deck.pptx")).expect("pptx converts");
        assert!(md.contains("settle_invoice"), "{md}");
        assert!(md.contains("retry_budget"), "{md}");
    }

    #[test]
    fn text_pdf_converts() {
        let md = to_markdown("runbook.pdf", &fixture("runbook.pdf")).expect("pdf converts");
        assert!(md.contains("settle_invoice"), "{md}");
    }

    #[test]
    fn scanned_pdf_is_skipped_as_needs_ocr() {
        let err = to_markdown("scan.pdf", &fixture("scanned.pdf")).expect_err("no text layer");
        assert!(matches!(err, Skipped::NeedsOcr(_)), "{err:?}");
        assert!(err.to_string().starts_with("needs OCR"), "{err}");
    }

    #[test]
    fn oversized_input_is_skipped_before_parsing() {
        let big = vec![0u8; MAX_INPUT_BYTES + 1];
        assert_eq!(
            to_markdown("big.pdf", &big),
            Err(Skipped::TooLarge(MAX_INPUT_BYTES as u64 + 1))
        );
        let tmp = std::env::temp_dir().join(format!("doc-convert-big-{}.pdf", std::process::id()));
        std::fs::write(&tmp, &big).unwrap();
        let read = read_capped(&tmp);
        std::fs::remove_file(&tmp).ok();
        assert_eq!(read, Err(Skipped::TooLarge(MAX_INPUT_BYTES as u64 + 1)));
    }

    #[test]
    fn garbage_with_a_doc_extension_is_a_skip_not_a_panic() {
        let err = to_markdown("x.docx", b"PK\x03\x04 not really a zip").expect_err("garbage");
        assert!(
            matches!(err, Skipped::Unreadable(_) | Skipped::Panicked),
            "{err:?}"
        );
    }

    #[test]
    fn chunks_split_at_h1_h2_outside_fences_and_cap_long_sections() {
        let md = "intro\n\n# One\nbody one\n\n```\n# not a heading\n```\n## Two\nbody two\n### Two.a\nstill two\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        assert_eq!(chunks[0], "intro");
        assert!(chunks[1].starts_with("# One") && chunks[1].contains("# not a heading"));
        assert!(chunks[2].starts_with("## Two") && chunks[2].contains("still two"));

        let para = "word ".repeat(500);
        let long = format!("# Big\n{para}\n\n{para}\n\n{para}\n");
        let chunks = chunk_markdown(&long);
        assert_eq!(
            chunks.len(),
            3,
            "each 500-word paragraph alone: {}",
            chunks.len()
        );
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS + 2)
        );

        // One paragraph over the limit splits by words, so every chunk fits a recall budget.
        let one = format!("# Huge\n\n{}\n", "word ".repeat(3001));
        let chunks = chunk_markdown(&one);
        assert!(chunks.len() >= 4, "{}", chunks.len());
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS)
        );

        // A 3-backtick line inside a 4-backtick fence does not close it.
        let nested = "# A\n````\n```\n# still code\n```\n````\n## B\nb\n";
        let chunks = chunk_markdown(nested);
        assert_eq!(chunks.len(), 2, "{chunks:?}");
        assert!(chunks[0].contains("# still code"));
    }

    #[test]
    fn delimiter_escapes_are_removed_and_block_escapes_kept() {
        assert_eq!(
            unescape_delimiters(r"call settle\_invoice \*now\* \`x\` \~y"),
            "call settle_invoice *now* `x` ~y"
        );
        // A protected `#` stays protected: the paragraph does not become a heading.
        assert_eq!(
            unescape_delimiters(r"\# not a heading"),
            r"\# not a heading"
        );
        assert_eq!(unescape_delimiters("no escapes here"), "no escapes here");
    }

    #[test]
    fn long_prose_lines_are_wrapped_so_the_minified_guard_keeps_them() {
        let para = "alpha beta gamma delta ".repeat(400); // ~9,200 chars, one line
        let wrapped = wrap_long_lines(&format!("# Notes\n{para}\n"));
        assert!(
            wrapped.lines().all(|l| l.chars().count() <= WRAP_AT),
            "{wrapped}"
        );
        assert_eq!(
            wrapped.split_whitespace().count(),
            format!("# Notes\n{para}").split_whitespace().count(),
            "no word lost"
        );
        assert!(!crate::treesitter::is_minified_or_huge(&wrapped));
        assert!(
            crate::treesitter::is_minified_or_huge(&para.repeat(6)),
            "the guard would drop it"
        );
    }

    #[test]
    fn a_fifo_named_like_a_document_is_refused_without_blocking() {
        #[cfg(unix)]
        {
            let dir = std::env::temp_dir().join(format!("doc-convert-fifo-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let fifo = dir.join("pipe.pdf");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            if made.is_ok_and(|s| s.success()) {
                assert_eq!(
                    read_capped(&fifo),
                    Err(Skipped::Unreadable("not a regular file"))
                );
            }
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn skip_reasons_never_carry_document_text() {
        let err = to_markdown("x.docx", b"PK\x03\x04 SECRETPAYLOAD").expect_err("garbage");
        assert!(!err.to_string().contains("SECRETPAYLOAD"), "{err}");
    }

    #[test]
    fn csv_and_markdown_are_not_documents() {
        assert!(is_doc_path("a/b/Spec.DOCX"));
        assert!(is_doc_path("r.pdf"));
        assert!(!is_doc_path("data.csv"));
        assert!(!is_doc_path("README.md"));
        assert!(!is_doc_path("pdf"));
    }

    /// Hosted OCR must be unreachable: the converter's whole dependency closure, read from the
    /// workspace lockfile, carries no HTTP, TLS or async-network client. A new anydoc (or a
    /// transitive bump) that pulls one in fails here before it can ship.
    #[test]
    fn anydoc_dependency_closure_has_no_network_client() {
        let lock =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
                .expect("workspace Cargo.lock");
        let lock: toml::Value = toml::from_str(&lock).expect("lockfile parses");
        let pkgs = lock["package"].as_array().expect("packages");
        let deps_of = |name: &str| -> Vec<String> {
            pkgs.iter()
                .filter(|p| p["name"].as_str() == Some(name))
                .flat_map(|p| p.get("dependencies").and_then(|d| d.as_array()).cloned())
                .flatten()
                .filter_map(|d| {
                    d.as_str()
                        .map(|s| s.split(' ').next().unwrap_or(s).to_string())
                })
                .collect()
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut todo = vec!["anydoc".to_string()];
        while let Some(n) = todo.pop() {
            if seen.insert(n.clone()) {
                todo.extend(deps_of(&n));
            }
        }
        assert!(seen.contains("pdf-inspector"), "closure walked: {seen:?}");
        let banned = [
            "reqwest",
            "hyper",
            "ureq",
            "curl",
            "isahc",
            "attohttpc",
            "rustls",
            "native-tls",
            "openssl",
            "tokio",
            "h2",
            "surf",
        ];
        let hit: Vec<&str> = banned
            .iter()
            .copied()
            .filter(|b| seen.contains(*b))
            .collect();
        assert!(hit.is_empty(), "anydoc pulls in a network client: {hit:?}");
    }
}
