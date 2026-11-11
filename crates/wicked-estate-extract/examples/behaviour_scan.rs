//! Print the behaviour class estate's extractor records for every inspected definition under a
//! directory, one JSON object per line. A precision check for `wicked_estate_extract::behaviour`:
//!
//! ```text
//! cargo run -p wicked-estate-extract --example behaviour_scan -- <dir> > classes.jsonl
//! ```
//!
//! Directory symlinks are not followed. Files that cannot be read or extracted are counted and
//! listed on stderr, and the exit status is 2 when any were skipped, so an incomplete census is
//! never mistaken for a complete one.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use wicked_estate_core::{Extractor, SourceFile};
use wicked_estate_extract::behaviour::{BEHAVIOUR_KEY, BEHAVIOUR_REASON_KEY};
use wicked_estate_extract::treesitter::{TreeSitterExtractor, extractor_for_extension};

fn walk(dir: &Path, out: &mut Vec<PathBuf>, skipped: &mut Vec<String>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            skipped.push(format!("{}: {e}", dir.display()));
            return;
        }
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else {
            continue;
        };
        let name = e.file_name();
        let name = name.to_string_lossy();
        if ft.is_dir() {
            if !name.starts_with('.') && name != "node_modules" && name != "target" {
                walk(&e.path(), out, skipped);
            }
        } else if ft.is_file() {
            out.push(e.path());
        }
    }
}

fn main() {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("usage: behaviour_scan <dir>");
        std::process::exit(1);
    };
    let root = Path::new(&root);
    if !root.is_dir() {
        eprintln!("behaviour_scan: {} is not a directory", root.display());
        std::process::exit(1);
    }
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    walk(root, &mut files, &mut skipped);
    files.sort();
    // One extractor per extension: building one compiles its query.
    let mut extractors: HashMap<String, Option<TreeSitterExtractor>> = HashMap::new();
    for f in files {
        let Some(ext) = f.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(ex) = extractors
            .entry(ext.to_string())
            .or_insert_with(|| extractor_for_extension(ext))
            .as_ref()
        else {
            continue;
        };
        let Some(lang) = ex.languages().into_iter().next() else {
            continue;
        };
        let rel = f
            .strip_prefix(root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let text = match std::fs::read_to_string(&f) {
            Ok(t) => t,
            Err(e) => {
                skipped.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let x = match ex.extract(&SourceFile {
            path: rel.clone(),
            language: lang,
            text,
        }) {
            Ok(x) => x,
            Err(e) => {
                skipped.push(format!("{rel}: {e}"));
                continue;
            }
        };
        for n in x.nodes {
            let Some(b) = n.metadata.get(BEHAVIOUR_KEY) else {
                continue;
            };
            let line = serde_json::json!({
                "file": rel,
                "name": n.name,
                "kind": n.kind,
                "symbol": n.symbol.as_str(),
                "behaviour": b,
                "reason": n.metadata.get(BEHAVIOUR_REASON_KEY),
                "start_line": n.location.span.start_line,
                "end_line": n.location.span.end_line,
            });
            println!("{line}");
        }
    }
    if !skipped.is_empty() {
        eprintln!("behaviour_scan: skipped {}:", skipped.len());
        for s in &skipped {
            eprintln!("  {s}");
        }
        std::process::exit(2);
    }
}
