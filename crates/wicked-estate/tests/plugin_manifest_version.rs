//! Release guard: the Claude-plugin manifest version
//! (`plugins/wicked-estate/.claude-plugin/plugin.json`) must equal the
//! workspace version (`CARGO_PKG_VERSION`, inherited from `[workspace.package]`).
//!
//! Why this exists: the manifest is a version field OUTSIDE Cargo's reach — no
//! cargo command, and no regex over `Cargo.toml`, ever touches it. It shipped
//! "0.13.1" for two whole minor releases (0.14.x and the 0.15.0 tag) before
//! anyone noticed: exactly the sibling of the per-crate pin drift that
//! `version_pins.rs` guards. This test makes that drift red on every
//! `cargo test --workspace` (the CI gate) and in the release workflow's guard
//! step, BEFORE a tag exists. `.github/workflows/release.yml` both bumps the
//! manifest alongside `Cargo.toml` and runs this test — belt and braces.
//!
//! Std-only line parsing on purpose, mirroring `version_pins.rs`: adding a JSON
//! dev-dependency would itself be a manifest change these guards police. The
//! file is a flat `.claude-plugin/plugin.json` whose single top-level
//! `"version"` key is the only `"version"` string in it — asserted below.

use std::fs;
use std::path::PathBuf;

/// Extract the string value following the FIRST `"version"` key in `text`.
/// Panics (fails the guard) on any shape surprise instead of guessing.
fn manifest_version(text: &str) -> String {
    // The manifest must contain exactly one `"version"` key — if a second one
    // ever appears (e.g. inside a nested object), this parser is no longer
    // unambiguous and must be upgraded, not trusted.
    let occurrences = text.matches("\"version\"").count();
    assert!(
        occurrences == 1,
        "expected exactly one \"version\" key in the plugin manifest, found {occurrences} — \
         upgrade this parser before trusting it"
    );
    let idx = text.find("\"version\"").expect("checked above");
    let rest = &text[idx + "\"version\"".len()..];
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix(':')
        .expect("plugin manifest: no ':' after \"version\" key");
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix('"')
        .expect("plugin manifest: version value is not a JSON string");
    let end = rest
        .find('"')
        .expect("plugin manifest: unterminated version string");
    rest[..end].to_string()
}

#[test]
fn plugin_manifest_version_matches_workspace_version() {
    let expected = env!("CARGO_PKG_VERSION");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plugin_manifest = manifest_dir
        .join("../..")
        .join("plugins/wicked-estate/.claude-plugin/plugin.json");

    // This test file is packaged into the published .crate (cargo packages
    // tests/ by default). There the plugin manifest does not exist — skip
    // loudly instead of failing downstream `cargo test`, mirroring
    // version_pins.rs.
    let text = match fs::read_to_string(&plugin_manifest) {
        Ok(t) => t,
        Err(_) => {
            eprintln!(
                "SKIPPED plugin_manifest_version: no plugin manifest at {} — \
                 running outside the wicked-estate source tree (e.g. a published \
                 .crate). This guard only verifies the source tree.",
                plugin_manifest.display()
            );
            return;
        }
    };

    let found = manifest_version(&text);
    assert_eq!(
        found, expected,
        "plugins/wicked-estate/.claude-plugin/plugin.json version \"{found}\" is out of \
         sync with the workspace version \"{expected}\" — bump the manifest in the same \
         change as the workspace version (RELEASING.md step 1; release.yml bumps both)"
    );
}
