//! Relative JS/TS import binding: conventions-as-data (this module's loader) + the
//! [`RelativeImportResolver`] that binds `'./foo'` / `'../bar'` `Imports` refs to their target
//! File node with EXACT-path semantics.
//!
//! Design (docs/recon/relative-imports.md): extension + index conventions are DATA per importer
//! language (`import-conventions.toml`, embedded via `include_str!`), the algorithm is
//! language-blind, and resolution is an exact joined-path lookup with a ROOT GUARD — a `..`
//! that would pop below the repo root (or the `--repo` label prefix) parks the ref, never binds.
//! Nothing here calls `dir_of`, `file_matches_module`, or `normalise_relative_path` — the review
//! (estate-review doc 01, findings D01-1..D01-9) proved that reuse false-binds.

use std::collections::HashMap;

use serde::Deserialize;

/// The embedded conventions data. One row per importer language.
const CONVENTIONS_TOML: &str = include_str!("../import-conventions.toml");

/// Top-level shape of `import-conventions.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConventionsFile {
    language: Vec<LanguageConventions>,
}

/// Per-importer-language resolution conventions. See `import-conventions.toml` for field docs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageConventions {
    pub name: String,
    /// Spec-side extensions recognised (matched longest-first).
    pub known_exts: Vec<String>,
    /// Candidate extensions for an extensionless spec, priority order.
    pub probe_exts: Vec<String>,
    /// Directory-index basenames (`index` → `<dir>/index.<ext>`).
    pub index_names: Vec<String>,
    /// Explicit spec extension → candidate extension list, priority order.
    #[serde(default)]
    pub remap: std::collections::BTreeMap<String, Vec<String>>,
}

/// Parsed + validated conventions, keyed by importer language name.
#[derive(Debug, Clone)]
pub struct ImportConventions {
    by_language: HashMap<String, LanguageConventions>,
}

impl ImportConventions {
    /// Parse the embedded `import-conventions.toml`. Panics on an invalid table — the file is
    /// compiled into the binary, so a bad row is a build defect, not a runtime condition.
    pub fn embedded() -> Self {
        Self::parse(CONVENTIONS_TOML).expect("embedded import-conventions.toml must be valid")
    }

    /// Parse + validate a conventions document. Rejects (in order): unknown keys (serde),
    /// a duplicate extension within any single list (the only constructible same-priority tie),
    /// and a probe/remap extension that is not a known extension.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: ConventionsFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut by_language = HashMap::new();
        for mut lang in file.language {
            validate_no_duplicates(&lang.name, "known_exts", &lang.known_exts)?;
            validate_no_duplicates(&lang.name, "probe_exts", &lang.probe_exts)?;
            for (key, exts) in &lang.remap {
                validate_no_duplicates(&lang.name, &format!("remap.{key}"), exts)?;
                if !lang.known_exts.contains(key) {
                    return Err(format!(
                        "[{}] remap key '{key}' is not in known_exts",
                        lang.name
                    ));
                }
                for e in exts {
                    if !lang.known_exts.contains(e) {
                        return Err(format!(
                            "[{}] remap.{key} ext '{e}' is not in known_exts",
                            lang.name
                        ));
                    }
                }
            }
            for e in &lang.probe_exts {
                if !lang.known_exts.contains(e) {
                    return Err(format!(
                        "[{}] probe ext '{e}' is not in known_exts",
                        lang.name
                    ));
                }
            }
            // Longest-first matching must never depend on file order.
            lang.known_exts.sort_by_key(|e| std::cmp::Reverse(e.len()));
            if by_language
                .insert(lang.name.clone(), lang.clone())
                .is_some()
            {
                return Err(format!("duplicate language row '{}'", lang.name));
            }
        }
        Ok(Self { by_language })
    }

    /// The conventions row for an importer language, if one exists.
    pub fn for_language(&self, name: &str) -> Option<&LanguageConventions> {
        self.by_language.get(name)
    }

    /// Every language name that has a row (for the registry cross-check test in `wicked-estate`).
    pub fn language_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.by_language.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

fn validate_no_duplicates(lang: &str, list: &str, exts: &[String]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for e in exts {
        if !seen.insert(e.as_str()) {
            return Err(format!("[{lang}] duplicate ext '{e}' in {list}"));
        }
    }
    Ok(())
}

/// How the spec's trailing path segment parses against a language's `known_exts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecExt {
    /// No extension on the last segment — extension probing applies.
    None,
    /// A known extension (longest-first match); `stem` is the spec without it.
    Known(String),
    /// An extension outside the family (`.css`, `.json`, …) — literal probe only.
    Unknown,
}

/// Split a (already root-joined) path into `(stem, SpecExt)` against `conv.known_exts`,
/// matching longest-first so `foo.d.ts` → (`foo`, `d.ts`) and `foo.test.ts` → (`foo.test`, `ts`).
/// Candidate-side stripping does not exist anywhere in this module — this runs on the SPEC side
/// only, which is what keeps the `rsplit_once('.')` `.d.ts` defect class unconstructible.
pub fn parse_spec_ext(path: &str, conv: &LanguageConventions) -> (String, SpecExt) {
    let last_seg = path.rsplit('/').next().unwrap_or(path);
    for ext in &conv.known_exts {
        if let Some(stem_seg) = last_seg.strip_suffix(ext.as_str()) {
            // Require `<non-empty>.<ext>` — a bare `.ts` file name is not an extension match.
            if let Some(stem_seg) = stem_seg.strip_suffix('.') {
                if !stem_seg.is_empty() {
                    let stem = format!("{}{}", &path[..path.len() - last_seg.len()], stem_seg);
                    return (stem, SpecExt::Known(ext.clone()));
                }
            }
        }
    }
    // Unknown extension: a '.' after the first character of the last segment (a leading dot is a
    // dotfile, not an extension).
    if last_seg[1.min(last_seg.len())..].contains('.') {
        return (path.to_string(), SpecExt::Unknown);
    }
    (path.to_string(), SpecExt::None)
}

#[cfg(test)]
mod loader_tests {
    use super::*;

    fn ts(conv: &ImportConventions) -> &LanguageConventions {
        conv.for_language("typescript").expect("typescript row")
    }

    #[test]
    fn conventions_load_and_deny_unknown_fields() {
        let conv = ImportConventions::embedded();
        for lang in ["typescript", "tsx", "javascript"] {
            assert!(conv.for_language(lang).is_some(), "row for {lang}");
        }
        // Unknown keys are rejected, not silently ignored (the LanguageSpec failure mode).
        let bad = r#"
            [[language]]
            name = "x"
            known_exts = ["ts"]
            probe_exts = ["ts"]
            index_names = ["index"]
            surprise = true
        "#;
        let err = ImportConventions::parse(bad).unwrap_err();
        assert!(err.contains("surprise"), "unknown key must be named: {err}");
    }

    #[test]
    fn known_exts_longest_first() {
        let conv = ImportConventions::embedded();
        let (stem, ext) = parse_spec_ext("foo.d.ts", ts(&conv));
        assert_eq!((stem.as_str(), ext), ("foo", SpecExt::Known("d.ts".into())));
        let (stem, ext) = parse_spec_ext("foo.test.ts", ts(&conv));
        assert_eq!(
            (stem.as_str(), ext),
            ("foo.test", SpecExt::Known("ts".into()))
        );
        // Directory part is preserved on the stem.
        let (stem, ext) = parse_spec_ext("src/deep/q.js", ts(&conv));
        assert_eq!(
            (stem.as_str(), ext),
            ("src/deep/q", SpecExt::Known("js".into()))
        );
    }

    #[test]
    fn unknown_ext_is_literal() {
        let conv = ImportConventions::embedded();
        let (stem, ext) = parse_spec_ext("styles.css", ts(&conv));
        assert_eq!((stem.as_str(), ext), ("styles.css", SpecExt::Unknown));
        // Extensionless and dotfile forms are NOT unknown-ext.
        assert_eq!(parse_spec_ext("utils/index", ts(&conv)).1, SpecExt::None);
        assert_eq!(parse_spec_ext(".hidden", ts(&conv)).1, SpecExt::None);
    }

    #[test]
    fn every_probe_ext_is_a_known_ext() {
        // The embedded file passes the loader's own validation; assert the property directly
        // so a future edit that weakens the loader still fails here.
        let conv = ImportConventions::embedded();
        for name in conv.language_names() {
            let row = conv.for_language(name).unwrap();
            for e in &row.probe_exts {
                assert!(row.known_exts.contains(e), "[{name}] probe ext {e}");
            }
            for exts in row.remap.values() {
                for e in exts {
                    assert!(row.known_exts.contains(e), "[{name}] remap ext {e}");
                }
            }
        }
    }

    #[test]
    fn duplicate_ext_in_list_rejected() {
        let bad = r#"
            [[language]]
            name = "x"
            known_exts = ["ts", "js"]
            probe_exts = ["ts", "ts"]
            index_names = ["index"]
        "#;
        let err = ImportConventions::parse(bad).unwrap_err();
        assert!(
            err.contains("duplicate ext 'ts'"),
            "duplicate must be rejected at load: {err}"
        );
    }
}
