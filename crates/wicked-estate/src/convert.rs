//! `wicked-estate convert <file> [--max-bytes N] [--deadline-ms N]` (wicked-estate#556).
//!
//! One binary document (PDF, Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) to Markdown on
//! stdout, through the same converter the index uses ([`doc_convert`]): read under the size cap,
//! converted in the isolated child (#442: live-heap budget, wall-clock deadline, a crash stays in
//! the child), then masked with the Markdown redaction rules, as `knowledge.ingest` masks it.
//! Local only, and it stores nothing: it is dispatched before the store resolves and opens no
//! graph.
//!
//! A refusal leaves stdout empty, writes one `refused <kind>: <reason>` line on stderr and exits
//! with that kind's code ([`exit_code`]), so a caller branches on the code, not on prose.

use std::path::Path;
use std::time::Duration;

use wicked_estate_extract::doc_convert::{self, Skipped};
use wicked_estate_extract::redact;

use crate::cli_flags;

/// The exit code for each refusal kind ([`Skipped::kind`]). 0 is converted Markdown; 1 is a usage
/// error (the shared parser's).
pub const EXIT_CODES: &[(&str, i32)] = &[
    ("too_large", 3),
    ("unsupported", 4),
    ("encrypted", 5),
    ("needs_ocr", 6),
    ("deadline", 7),
    ("over_budget", 8),
    ("empty", 9),
    ("unreadable", 10),
    ("malformed", 11),
    ("failed", 12),
];

/// The exit code a refusal of `kind` returns; an unknown kind is a plain failure.
pub fn exit_code(kind: &str) -> i32 {
    EXIT_CODES
        .iter()
        .find(|(k, _)| *k == kind)
        .or_else(|| EXIT_CODES.iter().find(|(k, _)| *k == "failed"))
        .map(|(_, code)| *code)
        .expect("a code for failed")
}

/// Run the command: Markdown on stdout and `Ok(0)`, or a refusal on stderr and its code.
pub fn run(args: &cli_flags::Args) -> std::io::Result<i32> {
    use std::io::Write;
    let file = args.required(0);
    let max_bytes = args
        .usize("max-bytes")
        .unwrap_or(doc_convert::MAX_INPUT_BYTES);
    let deadline = args
        .u64("deadline-ms")
        .map(Duration::from_millis)
        .unwrap_or(doc_convert::CONVERT_DEADLINE);
    match convert(Path::new(file), max_bytes, deadline) {
        Ok(md) => {
            let mut out = std::io::stdout().lock();
            out.write_all(md.as_bytes())?;
            if !md.ends_with('\n') {
                out.write_all(b"\n")?;
            }
            out.flush()?;
            Ok(0)
        }
        Err(skip) => {
            let kind = skip.kind();
            eprintln!("refused {kind}: {}", reason(&skip, max_bytes, deadline));
            Ok(exit_code(kind))
        }
    }
}

fn convert(path: &Path, max_bytes: usize, deadline: Duration) -> Result<String, Skipped> {
    let bytes = doc_convert::read_capped_at(path, max_bytes)?;
    // The file name only names the format when the bytes carry no signature.
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let md = doc_convert::to_markdown_within(name, &bytes, deadline)?;
    Ok(redact::redact_source("convert.md", &md))
}

/// The refusal's reason, with the limits this run used where they differ from the converter's
/// defaults that [`Skipped`]'s own text names.
fn reason(skip: &Skipped, max_bytes: usize, deadline: Duration) -> String {
    match skip {
        Skipped::TooLarge(n) => {
            format!("too large ({n} bytes, limit {max_bytes}); not converted")
        }
        _ if skip.kind() == "deadline" => format!(
            "took longer than the {} ms conversion limit; not converted",
            deadline.as_millis()
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_own_code_and_none_is_usage_or_success() {
        let mut seen = std::collections::BTreeSet::new();
        for (kind, code) in EXIT_CODES {
            assert!(*code > 1, "{kind}");
            assert!(seen.insert(*code), "{kind} reuses {code}");
        }
        assert_eq!(exit_code("no-such-kind"), exit_code("failed"));
    }
}
