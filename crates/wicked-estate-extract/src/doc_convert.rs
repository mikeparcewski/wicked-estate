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
//! read ([`read_capped`]) and the converted text at [`MAX_TEXT_BYTES`]; anydoc bounds
//! Office/ODF/EPUB decompression (128 MiB per archive entry, 512 MiB per archive, 2M XML nodes) and
//! pdf-inspector bounds PDF streams per stream and per page. Conversions run on the caller's
//! threads (the indexer's rayon pool), so at most one document per worker is in memory at once.
//! There is no document-wide PDF memory budget and no timer (a thread cannot be preempted), and an
//! abort (out of memory, stack overflow) inside the parser ends the process: a hostile PDF can cost
//! memory or time. Unwinding panics are caught and reported as a skip. Skip reasons are fixed
//! strings; document text never reaches a reason.
//!
//! [anydoc]: https://github.com/firecrawl/anydoc

use std::fmt;
use std::io::Read;

/// Largest document converted, in bytes (32 MiB). Bigger files are skipped, never parsed.
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

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

impl Skipped {
    /// The page count a [`Skipped::NeedsOcr`] reason names ("all 3 pages need OCR" is 3; "page 2
    /// of 5 needs OCR" is 5, the document's total), or `None` for any other skip or a reason with
    /// no number. Parsed from anydoc's message, which carries page numbers only.
    pub fn ocr_pages(&self) -> Option<u32> {
        let Skipped::NeedsOcr(msg) = self else {
            return None;
        };
        let words: Vec<&str> = msg.split_whitespace().collect();
        let num = |w: &&str| {
            w.trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<u32>()
                .ok()
        };
        let after_of = words
            .iter()
            .position(|w| *w == "of")
            .and_then(|i| words.get(i + 1))
            .and_then(num);
        after_of.or_else(|| words.iter().find_map(num))
    }
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

/// Largest converted text kept, in bytes (1 MiB, the index's own text limit). Both callers get it.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// Convert one document to Markdown. `path` only names the format when the bytes carry no
/// signature (anydoc detects the format from the content first). Inline-delimiter escapes are
/// removed (`settle\_invoice` reads `settle_invoice`, so identifiers and token shapes match as in
/// any text); nothing else is changed. It is NOT redacted here: callers redact it with the markdown
/// rules, like any `.md` file.
pub fn to_markdown(path: &str, bytes: &[u8]) -> Result<String, Skipped> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Skipped::TooLarge(bytes.len() as u64));
    }
    let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    // A malformed document must not take the index run down with it. Detection opens containers
    // too, so it runs inside the unwind boundary as well.
    let converted = {
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
    Ok(unescape_delimiters(&md))
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

/// Words a chunk from [`chunk_markdown`] holds at most.
pub const CHUNK_WORDS: usize = 800;

/// A code-fence line: its character and length (CommonMark: 3+ backticks or tildes, indented at
/// most 3 spaces; a backtick fence's info string holds no backtick, so a one-line code span is not
/// a fence).
fn fence_line(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    let rest = &t[n * c.len_utf8()..];
    (n >= 3 && !(c == '`' && rest.contains('`'))).then_some((c, n))
}

/// `line` in pieces of at most `max` words, cut at word starts so the text between words, spacing
/// included, is kept as written.
fn split_words(line: &str, max: usize) -> Vec<&str> {
    let mut starts = Vec::new();
    let mut prev_space = true;
    for (i, ch) in line.char_indices() {
        if !ch.is_whitespace() && prev_space {
            starts.push(i);
        }
        prev_space = ch.is_whitespace();
    }
    let mut pieces = Vec::new();
    let mut k = 0;
    while k < starts.len() {
        let from = if k == 0 { 0 } else { starts[k] };
        let to = starts.get(k + max).copied().unwrap_or(line.len());
        pieces.push(line[from..to].trim_end());
        k += max;
    }
    pieces
}

/// Split converted Markdown into chunks for knowledge ingest: one per H1/H2 section (a `#` line
/// inside a code fence is not a heading), each packed line by line and cut at a line boundary
/// before it passes [`CHUNK_WORDS`] words, so every chunk is at most that long. Lines are kept as
/// written; only a single line over the limit (a long paragraph or table row) is cut, at word
/// starts. A cut inside a code block is not re-fenced: chunks are retrieval units, and anydoc's
/// Office and PDF output rarely holds code. Chunks with no text are dropped.
pub fn chunk_markdown(md: &str) -> Vec<String> {
    let located = chunk_markdown_located(md);
    located.into_iter().map(|c| c.text).collect()
}

/// A chunk from [`chunk_markdown_located`]: its text and the headings it sits under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedChunk {
    pub text: String,
    /// The Markdown headings (any level, `#` to `######`, outside code fences) in force where the
    /// chunk starts, outermost first; empty before the first heading.
    pub heading_path: Vec<String>,
}

/// The text of an ATX heading line (`## Title ##` -> `(2, "Title")`); `None` for any other line.
fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let level = t.chars().take_while(|c| *c == '#').count();
    let rest = &t[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim_end()))
}

/// [`chunk_markdown`] with the heading path of each chunk. The chunks and their text are the same.
pub fn chunk_markdown_located(md: &str) -> Vec<LocatedChunk> {
    let mut chunks: Vec<LocatedChunk> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut words = 0usize;
    let mut fence: Option<(char, usize)> = None;
    let mut path: Vec<(usize, String)> = Vec::new();
    let mut start_path: Vec<String> = Vec::new();
    let mut emit = |cur: &mut Vec<&str>, words: &mut usize, start_path: &[String]| {
        let text = cur.join("\n");
        let text = text.trim_start_matches('\n').trim_end();
        if !text.trim().is_empty() {
            chunks.push(LocatedChunk {
                text: text.to_string(),
                heading_path: start_path.to_vec(),
            });
        }
        cur.clear();
        *words = 0;
    };
    for line in md.lines() {
        let is_fence = fence_line(line);
        if fence.is_none() && (line.starts_with("# ") || line.starts_with("## ")) {
            emit(&mut cur, &mut words, &start_path);
        }
        if fence.is_none() && is_fence.is_none() {
            if let Some((level, title)) = atx_heading(line) {
                while path.last().is_some_and(|(l, _)| *l >= level) {
                    path.pop();
                }
                path.push((level, title.to_string()));
            }
        }
        match (fence, is_fence) {
            // A closing fence: the same character, at least as long, nothing after it.
            (Some((c, n)), Some((c2, n2)))
                if c == c2 && n2 >= n && line.trim().chars().all(|x| x == c) =>
            {
                fence = None;
            }
            (None, Some(open)) => fence = Some(open),
            _ => {}
        }
        let n = line.split_whitespace().count();
        if words + n > CHUNK_WORDS {
            emit(&mut cur, &mut words, &start_path);
        }
        if cur.is_empty() {
            start_path = path.iter().map(|(_, t)| t.clone()).collect();
        }
        if n > CHUNK_WORDS {
            let pieces = split_words(line, CHUNK_WORDS);
            let last = pieces.len() - 1;
            for (i, piece) in pieces.into_iter().enumerate() {
                cur.push(piece);
                words = piece.split_whitespace().count();
                if i < last {
                    emit(&mut cur, &mut words, &start_path);
                }
            }
            continue;
        }
        cur.push(line);
        words += n;
    }
    emit(&mut cur, &mut words, &start_path);
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

        // Paragraphs: cut at the last blank line before the limit.
        let para = "word ".repeat(500);
        let long = format!("# Big\n{para}\n\n{para}\n\n{para}\n");
        let chunks = chunk_markdown(&long);
        assert_eq!(chunks.len(), 3, "{}", chunks.len());
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS + 2)
        );

        // One line over the limit splits by words, so every chunk fits a recall budget.
        let one = format!("# Huge\n\n{}\n", "word ".repeat(3001));
        let chunks = chunk_markdown(&one);
        assert!(chunks.len() >= 4, "{}", chunks.len());
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS)
        );

        // A paragraph over several lines: packed line by line, never past the limit.
        let line = |w: usize| -> String { format!("{}\n", "w ".repeat(w)) };
        let multi = format!("{}\n{}{}", line(100), line(600), line(600));
        let chunks = chunk_markdown(&multi);
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS),
            "{chunks:?}"
        );

        // A 3-backtick line inside a 4-backtick fence does not close it.
        let nested = "# A\n````\n```\n# still code\n```\n````\n## B\nb\n";
        let chunks = chunk_markdown(nested);
        assert_eq!(chunks.len(), 2, "{chunks:?}");
        assert!(chunks[0].contains("# still code"));
    }

    #[test]
    fn a_long_code_block_keeps_its_lines_and_every_chunk_fits() {
        let code: String = (0..810).map(|i| format!("    print({i})\n")).collect();
        let md = format!("# Example\n```python\n{code}```\nafter\n");
        let chunks = chunk_markdown(&md);
        assert!(chunks.len() >= 2, "{}", chunks.len());
        assert!(
            chunks
                .iter()
                .all(|c| c.split_whitespace().count() <= CHUNK_WORDS)
        );
        let joined = chunks.join("\n");
        for i in [0, 405, 809] {
            assert!(
                joined.contains(&format!("    print({i})")),
                "line {i} kept, indented"
            );
        }
        // A one-line code span and a 4-space-indented line are not fences.
        assert_eq!(fence_line("```foo```"), None);
        assert_eq!(fence_line("    ```"), None);
        assert_eq!(fence_line("   ~~~~ rust"), Some(('~', 4)));
        // A long line is cut at word starts: spacing inside each piece is kept as written.
        let line = format!("s = \"a  b\"; {}", "w ".repeat(900));
        let pieces = split_words(&line, CHUNK_WORDS);
        assert_eq!(pieces.len(), 2);
        assert!(
            pieces[0].starts_with("s = \"a  b\";"),
            "{}",
            &pieces[0][..20]
        );
        let total: usize = pieces.iter().map(|p| p.split_whitespace().count()).sum();
        assert_eq!(
            total,
            line.split_whitespace().count(),
            "no word lost or doubled"
        );
    }

    /// A private key inside a long Word paragraph keeps its one-line markers through conversion,
    /// so the markdown redaction masks its body (a layout pass that re-flowed lines once broke the
    /// marker and leaked the body).
    #[test]
    fn a_private_key_in_a_long_paragraph_is_masked_by_the_markdown_redaction() {
        let md = to_markdown("handover.docx", &fixture("pem.docx")).expect("converts");
        let begin = ["-----BEGIN", " PRIVATE KEY-----"].concat();
        assert!(md.contains(&begin), "marker on one line: {md}");
        let body = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo";
        assert!(md.contains(body), "{md}");
        let red = crate::redact::redact_source("handover.docx.md", &md);
        assert!(!red.contains(body), "key body masked: {red}");
        assert!(red.contains("pemcanary_body"), "prose kept: {red}");
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

    #[test]
    fn located_chunks_carry_their_heading_path_and_match_the_plain_chunker() {
        let md = "intro\n\n# A\n\ntext a\n\n## B\n\ntext b\n\n### C ###\n\ntext c\n\n```\n# not a heading\n```\n\n## D\n\ntext d\n";
        let located = chunk_markdown_located(md);
        let paths: Vec<Vec<&str>> = located
            .iter()
            .map(|c| c.heading_path.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            paths,
            vec![vec![], vec!["A"], vec!["A", "B"], vec!["A", "D"]],
            "{located:#?}"
        );
        let section = &located[2].text;
        assert!(section.contains("text c") && section.contains("# not a heading"));
        let plain: Vec<String> = located.iter().map(|c| c.text.clone()).collect();
        assert_eq!(plain, chunk_markdown(md));
    }

    #[test]
    fn an_over_long_section_keeps_its_heading_path_across_cuts() {
        let body = vec!["word"; CHUNK_WORDS * 2 + 10].join(" ");
        let md = format!("# Big\n\n{body}\n");
        let located = chunk_markdown_located(&md);
        assert!(located.len() >= 3, "{}", located.len());
        let big = vec!["Big".to_string()];
        assert!(located.iter().all(|c| c.heading_path == big));
    }

    #[test]
    fn ocr_pages_reads_the_count_from_the_reason() {
        let pages = |m: &str| Skipped::NeedsOcr(m.to_string()).ocr_pages();
        assert_eq!(pages("all 3 pages need OCR"), Some(3));
        assert_eq!(pages("page 2 of 5 needs OCR"), Some(5));
        assert_eq!(pages("needs OCR"), None);
        assert_eq!(Skipped::Encrypted.ocr_pages(), None);
    }

    #[test]
    fn the_scanned_fixture_names_its_page_count() {
        let err = to_markdown("scan.pdf", &fixture("scanned.pdf")).expect_err("no text layer");
        assert!(err.ocr_pages().is_some_and(|n| n >= 1), "{err}");
    }
}
