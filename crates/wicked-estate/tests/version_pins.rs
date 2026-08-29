//! Release guard: every internal `wicked-estate-*` version pin in the workspace
//! must equal this crate's own version (`CARGO_PKG_VERSION`, inherited from
//! `[workspace.package]`).
//!
//! Why this exists: internal deps are `{ path = "...", version = "X.Y.Z" }` pins.
//! On `cargo publish` the path component is stripped and the version requirement
//! is resolved against crates.io with caret semantics — `^0.14.2` excludes
//! `0.15.0`. Before 0.15.0 the 40 per-crate pins had silently drifted to
//! `"0.14.2"` while the workspace was at 0.14.6: any minor-version release would
//! have failed publish verification MID-SEQUENCE, after leaf crates were already
//! irreversibly published. This test makes that drift red before a tag exists.
//! `.github/workflows/release.yml` runs it before "commit and tag".
//!
//! Scope: the root `Cargo.toml` `[workspace.dependencies]` pins (consumed via
//! `workspace = true`, so their version reqs flow into published manifests) plus
//! every `wicked-estate-*` pin in every dependency table of `crates/*/Cargo.toml`.
//! Vendored grammar crates (`wicked-estate-tree-sitter-*`) version independently
//! and are excluded.
//!
//! Std-only line parsing on purpose: adding a `toml` dev-dependency would itself
//! be a manifest change this guard is meant to police. The pin lines are a
//! uniform single-line `name = { path = "...", version = "..." }` shape.

use std::fs;
use std::path::{Path, PathBuf};

/// Pins found in `crates/*/Cargo.toml` at the 0.15.0 release (40) plus the root
/// `[workspace.dependencies]` pins (2). If a refactor legitimately removes pins
/// (e.g. migrating internal deps to `[workspace.dependencies]`), lower this
/// floor in the same change — a silent drop to zero parsed pins must never pass.
const MIN_EXPECTED_PINS: usize = 42;

/// Extract the value of `version = "..."` from a single manifest line, if present.
fn version_req(line: &str) -> Option<&str> {
    let idx = line.find("version")?;
    let rest = &line[idx + "version".len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// True for lines that declare an internal `wicked-estate-*` dependency
/// (any table), excluding the independently-versioned vendored grammars.
fn is_internal_pin_line(trimmed: &str) -> bool {
    if !trimmed.starts_with("wicked-estate") {
        return false;
    }
    if trimmed.starts_with("wicked-estate-tree-sitter") {
        return false;
    }
    // Name boundary: `wicked-estate = {`, `wicked-estate-store = {`,
    // `wicked-estate-core.workspace = true`, ... — never a false prefix match.
    matches!(
        trimmed["wicked-estate".len()..].chars().next(),
        Some('-') | Some('.') | Some(' ') | Some('=')
    )
}

/// Collect `(file, line_no, name, pinned_version)` for every internal pin
/// carrying an explicit `version = "..."`.
fn collect_pins(manifest: &Path) -> Vec<(PathBuf, usize, String, String)> {
    let text = fs::read_to_string(manifest)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest.display()));
    let mut pins = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || !is_internal_pin_line(trimmed) {
            continue;
        }
        if let Some(req) = version_req(trimmed) {
            let name = trimmed
                .split(['=', ' ', '.'])
                .next()
                .unwrap_or("")
                .to_string();
            pins.push((manifest.to_path_buf(), i + 1, name, req.to_string()));
        }
    }
    pins
}

#[test]
fn internal_version_pins_match_workspace_version() {
    let expected = env!("CARGO_PKG_VERSION");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../Cargo.toml");

    // This test file is packaged into the published .crate (cargo packages
    // tests/ by default). There the workspace root manifest does not exist —
    // skip loudly instead of failing downstream `cargo test`.
    let root_text = match fs::read_to_string(&root) {
        Ok(t) if t.contains("[workspace]") => t,
        _ => {
            eprintln!(
                "SKIPPED version_pins: no workspace root manifest at {} — \
                 running outside the wicked-estate workspace (e.g. a published \
                 .crate). This guard only verifies the source tree.",
                root.display()
            );
            return;
        }
    };
    drop(root_text);

    let mut pins = collect_pins(&root);
    let crates_dir = manifest_dir.join("../..").join("crates");
    let mut entries: Vec<_> = fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", crates_dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path().join("Cargo.toml"))
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    for manifest in entries {
        pins.extend(collect_pins(&manifest));
    }

    assert!(
        pins.len() >= MIN_EXPECTED_PINS,
        "parsed only {} internal wicked-estate-* version pins (expected >= {}) — \
         the parser no longer sees the pins; fix the parser (or, after a \
         deliberate pin-removing refactor, lower MIN_EXPECTED_PINS in the same \
         change)",
        pins.len(),
        MIN_EXPECTED_PINS
    );

    let bad: Vec<String> = pins
        .iter()
        .filter(|(_, _, _, req)| req != expected)
        .map(|(file, line, name, req)| {
            format!("{}:{}: {name} pinned at \"{req}\"", file.display(), line)
        })
        .collect();
    assert!(
        bad.is_empty(),
        "internal version pins out of sync with workspace version {expected} \
         (publish would fail mid-sequence: ^old excludes {expected}):\n{}",
        bad.join("\n")
    );
}
