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
    // Unknown extension: a '.' after the first CHARACTER of the last segment (a leading dot is a
    // dotfile, not an extension). Char-wise skip, never a byte slice — `last_seg[1..]` panics
    // when the segment leads with a multi-byte char (`'./éclair'`), aborting the whole index run
    // (review round-1 R1-CORR-1 / RI-R1-1).
    if last_seg.chars().skip(1).any(|c| c == '.') {
        return (path.to_string(), SpecExt::Unknown);
    }
    (path.to_string(), SpecExt::None)
}

// ── RelativeImportResolver ────────────────────────────────────────────────────

use wicked_estate_core::{
    Confidence, Edge, EdgeKind, NodeKind, ResolutionTier, Resolver, Result as CoreResult, SymbolId,
    SymbolIndex, UnresolvedRef,
};

/// Stable id recorded on every edge this resolver emits.
pub const RELATIVE_IMPORT_RESOLVER_ID: &str = "relative-import";

/// Per-edge confidence override (Decision E, docs/recon/relative-imports.md): the joined-path
/// match is deterministic and adjudicated 100% on disk, but the resolver cannot see
/// `tsconfig.paths`, symlinks, or a case-insensitive FS — 0.9, not 1.0, and above the
/// `ImportMap` tier default (0.6). Documented in docs/ENGINE-CONTRACT.md: this deliberately wins
/// `resolve_all_with_coverage`'s max-confidence dedup against a Tsg-default (0.8) `Imports` edge.
const RELATIVE_IMPORT_CONFIDENCE: f32 = 0.9;

/// Counters returned by [`RelativeImportResolver::resolve_with_stats`] so the complexity guard
/// (S9) can pin O(files + refs): the candidate map is built at most ONCE per resolve call, and
/// every candidate check is one exact hash probe.
#[derive(Debug, Default, Clone, Copy)]
pub struct ResolveStats {
    /// Exact hash-map probes performed across all refs.
    pub probes: usize,
    /// How many times the File-node map was built (must be ≤ 1 per resolve call).
    pub map_builds: usize,
}

/// Binds relative JS/TS `Imports` refs (`'./foo'`, `'../bar'`) to their target File node.
///
/// - **Exact resolution only**: `parent_dir(importer) + spec`, normalised segment-wise with a
///   ROOT GUARD — a `..` that would pop below the repo root (or below the `--repo` label prefix
///   given at construction) parks the ref. No suffix matching exists on this path.
/// - **Conventions as data**: extension/index/remap behaviour comes from
///   `import-conventions.toml` per importer language ([`ImportConventions`]); an importer whose
///   language has no row is skipped.
/// - **O(files + refs)**: one `HashMap<full stored path, SymbolId>` over `NodeKind::File` nodes
///   per resolve call; every probe is an exact full path in the data-defined slot order, first
///   hit wins. Duplicate full paths are structurally impossible (`Symbol::file` derives the id
///   from the path) — `debug_assert`ed on insert.
/// - Emits `Imports` File→File at tier `ImportMap`, confidence 0.9,
///   `resolved_by = "relative-import"`, `metadata.via = "relative-path"`,
///   `metadata.rule ∈ {literal, remap, probe, index}`, `location = ref.location`.
#[derive(Debug)]
pub struct RelativeImportResolver {
    /// The `--repo` label prefix (`"<label>/"`) every stored path of this run carries, or `None`
    /// for an unlabelled repo. The root guard counts `..` pops against the importer's depth
    /// BELOW this prefix, so a labelled repo parks exactly where a plain one does.
    prefix: Option<String>,
    conventions: ImportConventions,
}

impl RelativeImportResolver {
    pub fn new(scope_prefix: Option<&str>) -> Self {
        Self {
            prefix: scope_prefix.map(str::to_string),
            conventions: ImportConventions::embedded(),
        }
    }

    /// [`Resolver::resolve`] plus the [`ResolveStats`] counters (complexity guard, S9).
    pub fn resolve_with_stats(
        &self,
        refs: &[UnresolvedRef],
        index: &dyn SymbolIndex,
    ) -> CoreResult<(Vec<Edge>, ResolveStats)> {
        let mut stats = ResolveStats::default();
        let mut out = Vec::new();
        // Built lazily, at most once: full stored path → (File SymbolId, language name).
        let mut file_map: Option<HashMap<String, (SymbolId, String)>> = None;

        for r in refs {
            if r.kind != EdgeKind::Imports {
                continue;
            }
            let spec = dequote(&r.raw_name);
            if !(spec.starts_with("./") || spec.starts_with("../")) {
                continue; // bare / alias specifiers are out of scope (Decision D)
            }

            if file_map.is_none() {
                stats.map_builds += 1;
                let mut m: HashMap<String, (SymbolId, String)> = HashMap::new();
                for n in index.all_nodes()? {
                    if n.kind == NodeKind::File {
                        let key = normalise_seps(&n.location.file);
                        let prev = m.insert(key, (n.symbol.clone(), n.language.0.clone()));
                        // Symbol::file derives the id from the path and stores key nodes by
                        // id, so a colliding full path cannot come from a real index
                        // (Decision C / ATT-INV-4).
                        debug_assert!(
                            prev.is_none(),
                            "duplicate File path in index: {}",
                            n.location.file
                        );
                    }
                }
                if m.is_empty() {
                    // D01-8: an empty index silently no-ops every ref — say so once.
                    eprintln!("relative-import: index has zero File nodes — nothing to bind");
                }
                file_map = Some(m);
            }
            let map = file_map.as_ref().expect("built above");
            if map.is_empty() {
                continue;
            }

            let importer_path = normalise_seps(&r.location.file);
            // Importer language from the index's own File node — never guessed from the ext.
            let Some((_, importer_lang)) = map.get(&importer_path) else {
                continue;
            };
            let Some(conv) = self.conventions.for_language(importer_lang) else {
                continue; // no conventions row for this language — skip (Decision B)
            };

            // Strip the label prefix so the root guard counts depth below the label.
            let rel_importer = match &self.prefix {
                Some(p) => match importer_path.strip_prefix(p.as_str()) {
                    Some(rest) => rest,
                    None => continue, // ref from outside this run's scope — not ours
                },
                None => importer_path.as_str(),
            };

            let dir = parent_dir(rel_importer);
            let Some(joined) = join_with_root_guard(dir, spec) else {
                continue; // root escape → PARK, never bind (Decision A)
            };

            // Probe list in the data-defined slot order (Decision C).
            //
            // A trailing-slash specifier names a DIRECTORY: TS (bundler/node16), Node CJS
            // `require('./x/')`, and every bundler resolve it ONLY to the directory index
            // (Node ESM rejects it outright) — a same-stem FILE is never a legal candidate,
            // so the literal/remap/probe slots are skipped entirely (round-2 RI-R2-1;
            // `join_with_root_guard` erases the trailing slash, so it must be read off the
            // spec BEFORE joining).
            let mut probes: Vec<(String, &'static str)> = Vec::new();
            if spec.ends_with('/') {
                for ix in &conv.index_names {
                    for pe in &conv.probe_exts {
                        probes.push((format!("{joined}/{ix}.{pe}"), "index"));
                    }
                }
            } else {
                let (stem, ext) = parse_spec_ext(&joined, conv);
                match ext {
                    SpecExt::Known(e) => {
                        if let Some(remapped) = conv.remap.get(&e) {
                            for re in remapped {
                                probes.push((format!("{stem}.{re}"), "remap"));
                            }
                        } else {
                            probes.push((joined.clone(), "literal"));
                        }
                    }
                    SpecExt::Unknown => probes.push((joined.clone(), "literal")),
                    SpecExt::None => {
                        for pe in &conv.probe_exts {
                            probes.push((format!("{joined}.{pe}"), "probe"));
                        }
                        for ix in &conv.index_names {
                            for pe in &conv.probe_exts {
                                probes.push((format!("{joined}/{ix}.{pe}"), "index"));
                            }
                        }
                    }
                }
            }

            for (cand, rule) in probes {
                stats.probes += 1;
                let full = match &self.prefix {
                    Some(p) => format!("{p}{cand}"),
                    None => cand,
                };
                if let Some((target, _)) = map.get(&full) {
                    if *target == r.from {
                        break; // self-import: no self-edges
                    }
                    let mut edge = Edge::new(
                        r.from.clone(),
                        target.clone(),
                        EdgeKind::Imports,
                        ResolutionTier::ImportMap,
                        RELATIVE_IMPORT_RESOLVER_ID,
                    )
                    .with_location(r.location.clone());
                    edge.confidence = Confidence::new(RELATIVE_IMPORT_CONFIDENCE);
                    edge.metadata.insert(
                        "via".to_string(),
                        serde_json::Value::String("relative-path".to_string()),
                    );
                    edge.metadata.insert(
                        "rule".to_string(),
                        serde_json::Value::String(rule.to_string()),
                    );
                    out.push(edge);
                    break; // first slot with a hit wins — later slots never probed
                }
            }
        }
        Ok((out, stats))
    }
}

impl Resolver for RelativeImportResolver {
    fn id(&self) -> &str {
        RELATIVE_IMPORT_RESOLVER_ID
    }

    fn tier(&self) -> ResolutionTier {
        ResolutionTier::ImportMap
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> CoreResult<Vec<Edge>> {
        self.resolve_with_stats(refs, index).map(|(edges, _)| edges)
    }
}

// ── path helpers (own — `dir_of` and `normalise_relative_path` are NOT used: the
//    resolver-precision lane owns them, and both false-bind; review doc 01) ─────

/// Strip one pair of matching quotes from an extracted import specifier (`"'./foo'"` → `./foo`).
fn dequote(raw: &str) -> &str {
    let s = raw.trim();
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\''))
            || (s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('`') && s.ends_with('`')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Stored paths are `/`-normalised at index time; a `\` can still arrive from a hand-built
/// index or a Windows-era row — normalise defensively.
fn normalise_seps(path: &str) -> String {
    if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

/// Parent directory of a repo-relative file path: `""` for a root-level file, `"a"` for
/// `"a/b.ts"`. (The existing `dir_of` returns the FILENAME for a root-level file — the inverted
/// root behaviour in review doc 01.)
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(pos) => &path[..pos],
        None => "",
    }
}

/// Join `dir` + `spec` segment-wise. Returns `None` (PARK) when a `..` would pop below the
/// repo root — the silent-pop underflow in `normalise_relative_path` is the false-bind class
/// this replaces (D01-1) — or when the spec resolves to the root itself.
fn join_with_root_guard(dir: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
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
    fn multibyte_leading_segment_never_panics() {
        // R1-CORR-1 / RI-R1-1: `last_seg[1..]` panicked inside 'é' (byte 1 is not a char
        // boundary) and aborted the whole index run. The check is char-wise now.
        let conv = ImportConventions::embedded();
        let (stem, ext) = parse_spec_ext("src/éclair", ts(&conv));
        assert_eq!((stem.as_str(), ext), ("src/éclair", SpecExt::None));
        // Single multi-byte char segment (the minimal crasher).
        assert_eq!(parse_spec_ext("ü", ts(&conv)).1, SpecExt::None);
        // Multi-byte lead WITH a dot after the first char: unknown ext, literal probe only.
        let (stem, ext) = parse_spec_ext("Übersicht.css", ts(&conv));
        assert_eq!((stem.as_str(), ext), ("Übersicht.css", SpecExt::Unknown));
        // Multi-byte lead with a KNOWN ext still strips it.
        let (stem, ext) = parse_spec_ext("src/éclair.ts", ts(&conv));
        assert_eq!(
            (stem.as_str(), ext),
            ("src/éclair", SpecExt::Known("ts".into()))
        );
        // A multi-byte DOTFILE is a dotfile, not an extension.
        assert_eq!(parse_spec_ext(".émacs", ts(&conv)).1, SpecExt::None);
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

#[cfg(test)]
mod resolver_tests {
    use super::*;
    use crate::resolve_all_with_coverage;
    use wicked_estate_core::{Language, Location, Node, Provenance, Span, Symbol};

    /// Minimal in-memory index (mirrors the lib.rs test harness — kept local so this module
    /// stays self-contained).
    struct VecIndex(Vec<Node>);

    impl SymbolIndex for VecIndex {
        fn by_name(&self, name: &str) -> Vec<Node> {
            self.0.iter().filter(|n| n.name == name).cloned().collect()
        }
        fn get(&self, id: &SymbolId) -> Option<Node> {
            self.0.iter().find(|n| &n.symbol == id).cloned()
        }
        fn all_nodes(&self) -> CoreResult<Vec<Node>> {
            Ok(self.0.clone())
        }
    }

    fn file_node(path: &str, lang: &str) -> Node {
        Node::new(
            Symbol::file(path).id(),
            NodeKind::File,
            path,
            Language::new(lang),
            Location::new(path, Span::ZERO),
        )
    }

    /// An Imports ref exactly as the extractor emits it: `from` = the importer's File symbol,
    /// `raw_name` = the QUOTED specifier, location = the importer file.
    fn rel_ref(from_path: &str, spec: &str) -> UnresolvedRef {
        UnresolvedRef::new(
            Symbol::file(from_path).id(),
            format!("'{spec}'"),
            EdgeKind::Imports,
            Location::new(from_path, Span::ZERO),
        )
    }

    fn resolver() -> RelativeImportResolver {
        RelativeImportResolver::new(None)
    }

    /// Resolve one ref against an index; return the target paths of the emitted edges.
    fn targets_of(index: &VecIndex, r: UnresolvedRef) -> Vec<String> {
        let edges = resolver().resolve(&[r], index).unwrap();
        edges
            .iter()
            .map(|e| {
                index
                    .get(&e.target)
                    .expect("edge target must be an indexed node")
                    .location
                    .file
            })
            .collect()
    }

    #[test]
    fn plain_unique_spec_binds() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/w.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./w")),
            vec!["src/w.ts"]
        );
    }

    #[test]
    fn js_spec_remaps_per_importer_language() {
        // Both q.ts and q.js on disk: a TS importer resolves './q.js' to q.ts (tsc nodenext);
        // a JS importer resolves the literal q.js (Node). Both decided by DATA, not code.
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/main.js", "javascript"),
            file_node("src/q.ts", "typescript"),
            file_node("src/q.js", "javascript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./q.js")),
            vec!["src/q.ts"],
            "TS importer: remap.js probes q.ts first"
        );
        assert_eq!(
            targets_of(&index, rel_ref("src/main.js", "./q.js")),
            vec!["src/q.js"],
            "JS importer: literal q.js wins"
        );
        // TS importer with ONLY q.ts present still binds (the emitted-output spec form).
        let index2 = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/q.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index2, rel_ref("src/main.ts", "./q.js")),
            vec!["src/q.ts"]
        );
    }

    #[test]
    fn directory_index_binds() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/c/index.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./c")),
            vec!["src/c/index.ts"]
        );
    }

    #[test]
    fn explicit_index_specs_bind() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/utils/index.ts", "typescript"),
            file_node("src/index.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./utils/index")),
            vec!["src/utils/index.ts"]
        );
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./index")),
            vec!["src/index.ts"]
        );
    }

    #[test]
    fn dts_spec_binds_literal() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/foo.d.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./foo.d.ts")),
            vec!["src/foo.d.ts"]
        );
    }

    #[test]
    fn file_beats_directory_index() {
        // a.ts AND a/index.ts: TS order is deterministic — a.ts wins, no ambiguity park.
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/a.ts", "typescript"),
            file_node("src/a/index.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./a")),
            vec!["src/a.ts"]
        );
    }

    #[test]
    fn trailing_slash_spec_binds_directory_index_only() {
        // './x/' names a DIRECTORY: x/index.ts wins even with x.ts present — the file is
        // never a legal candidate for a trailing-slash specifier (RI-R2-1).
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/x.ts", "typescript"),
            file_node("src/x/index.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./x/")),
            vec!["src/x/index.ts"]
        );
    }

    #[test]
    fn dot_slash_spec_binds_own_directory_index_never_parent_file() {
        // './' from src/a.ts is src/index.ts — never the root-level FILE src.ts, which sits
        // OUTSIDE the directory the spec names (RI-R2-1).
        let index = VecIndex(vec![
            file_node("src/a.ts", "typescript"),
            file_node("src.ts", "typescript"),
            file_node("src/index.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/a.ts", "./")),
            vec!["src/index.ts"]
        );
    }

    #[test]
    fn trailing_slash_spec_without_index_parks() {
        // './x/' with x.ts present but NO x/index.* must PARK — a file is not a directory
        // (RI-R2-1: park, never bind).
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/x.ts", "typescript"),
        ]);
        assert!(targets_of(&index, rel_ref("src/main.ts", "./x/")).is_empty());
    }

    #[test]
    fn parent_dir_trailing_slash_spec_binds_parent_index() {
        // '../' from src/sub/a.ts is src/index.ts; from a depth-1 importer it would
        // resolve to the repo root, which the root guard parks (recorded in the recon doc).
        let index = VecIndex(vec![
            file_node("src/sub/a.ts", "typescript"),
            file_node("src/index.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/sub/a.ts", "../")),
            vec!["src/index.ts"]
        );
        let root_index = VecIndex(vec![
            file_node("src/a.ts", "typescript"),
            file_node("index.ts", "typescript"),
        ]);
        assert!(
            targets_of(&root_index, rel_ref("src/a.ts", "../")).is_empty(),
            "'../' resolving to the repo root parks (root guard), never binds"
        );
    }

    #[test]
    fn family_ext_beats_foreign_ext() {
        // b.ts AND b.css: './b' probes the TS family only — b.ts, never b.css.
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/b.ts", "typescript"),
            file_node("src/b.css", "css"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./b")),
            vec!["src/b.ts"]
        );
    }

    #[test]
    fn foreign_ext_is_literal_only() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/styles.css", "css"),
        ]);
        // Explicit './styles.css' binds the literal file (a stylesheet IS a dependency).
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./styles.css")),
            vec!["src/styles.css"]
        );
        // Extensionless './styles' must NOT probe .css — parks.
        assert!(targets_of(&index, rel_ref("src/main.ts", "./styles")).is_empty());
    }

    #[test]
    fn suffix_match_class_parks() {
        // './foo2' with no src/foo2.* — site/src/foo2.ts must NOT bind (the review's
        // false-bind: cand_stem.ends_with("/{logical}")).
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("site/src/foo2.ts", "typescript"),
        ]);
        assert!(targets_of(&index, rel_ref("src/main.ts", "./foo2")).is_empty());
    }

    #[test]
    fn root_escape_parks_even_when_a_suffix_path_exists() {
        // '../../../../escape/x' from src/deep/nested/esc.ts pops below the root by one —
        // PARK, even though escape/x.ts exists (the review's normalise_relative_path
        // silent-pop false-bind).
        let index = VecIndex(vec![
            file_node("src/deep/nested/esc.ts", "typescript"),
            file_node("escape/x.ts", "typescript"),
        ]);
        assert!(
            targets_of(
                &index,
                rel_ref("src/deep/nested/esc.ts", "../../../../escape/x")
            )
            .is_empty()
        );
        // The in-bounds '..' chain still binds.
        assert_eq!(
            targets_of(
                &index,
                rel_ref("src/deep/nested/esc.ts", "../../../escape/x")
            ),
            vec!["escape/x.ts"]
        );
    }

    #[test]
    fn root_level_importer_binds_and_parks_correctly() {
        // dir_of("index.ts") returns "index.ts" (the filename) — the inverted-root defect.
        // parent_dir returns "" so './config' binds and '../foo' parks.
        let index = VecIndex(vec![
            file_node("index.ts", "typescript"),
            file_node("config.ts", "typescript"),
            file_node("foo.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("index.ts", "./config")),
            vec!["config.ts"]
        );
        assert!(targets_of(&index, rel_ref("index.ts", "../foo")).is_empty());
    }

    #[test]
    fn multibyte_specifier_binds_and_parks_without_panicking() {
        // R1-CORR-1 / RI-R1-1: an extensionless spec whose last segment leads with a multi-byte
        // char ('./éclair') crashed the resolver (byte slice at index 1). Bind + park variants.
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/éclair.ts", "typescript"),
        ]);
        assert_eq!(
            targets_of(&index, rel_ref("src/main.ts", "./éclair")),
            vec!["src/éclair.ts"],
            "non-ASCII extensionless spec binds via probe_exts"
        );
        assert!(
            targets_of(&index, rel_ref("src/main.ts", "./Übersicht")).is_empty(),
            "non-ASCII spec with no candidate parks, never panics"
        );
    }

    #[test]
    fn bare_specifier_is_skipped() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("react.ts", "typescript"),
        ]);
        assert!(targets_of(&index, rel_ref("src/main.ts", "react")).is_empty());
    }

    #[test]
    fn backslash_importer_path_is_normalised() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/w.ts", "typescript"),
        ]);
        // The ref's location arrives with Windows separators; stored File nodes are '/'.
        let r = UnresolvedRef::new(
            Symbol::file("src/main.ts").id(),
            "'./w'",
            EdgeKind::Imports,
            Location::new("src\\main.ts", Span::ZERO),
        );
        let edges = resolver().resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            index.get(&edges[0].target).unwrap().location.file,
            "src/w.ts"
        );
    }

    #[test]
    fn labelled_prefix_guards_the_label_root() {
        // Under --repo repoa, stored paths carry 'repoa/'. '../../repoa/src/b' from
        // repoa/src/a.ts escapes the LABEL root (depth below the prefix) — PARK, even though
        // the full-store path 'repoa/src/b.ts' exists. './b' binds.
        let index = VecIndex(vec![
            file_node("repoa/src/a.ts", "typescript"),
            file_node("repoa/src/b.ts", "typescript"),
        ]);
        let resolver = RelativeImportResolver::new(Some("repoa/"));
        let park = resolver
            .resolve(&[rel_ref("repoa/src/a.ts", "../../repoa/src/b")], &index)
            .unwrap();
        assert!(park.is_empty(), "label-root escape must park: {park:?}");
        let bind = resolver
            .resolve(&[rel_ref("repoa/src/a.ts", "./b")], &index)
            .unwrap();
        assert_eq!(bind.len(), 1);
        assert_eq!(
            index.get(&bind[0].target).unwrap().location.file,
            "repoa/src/b.ts"
        );
    }

    #[test]
    fn unknown_importer_language_is_skipped() {
        let index = VecIndex(vec![
            file_node("src/main.rs", "rust"),
            file_node("src/w.rs", "rust"),
        ]);
        assert!(targets_of(&index, rel_ref("src/main.rs", "./w")).is_empty());
    }

    #[test]
    fn non_import_refs_are_skipped() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/w.ts", "typescript"),
        ]);
        let r = UnresolvedRef::new(
            Symbol::file("src/main.ts").id(),
            "'./w'",
            EdgeKind::Calls,
            Location::new("src/main.ts", Span::ZERO),
        );
        assert!(resolver().resolve(&[r], &index).unwrap().is_empty());
    }

    #[test]
    fn every_edge_field_is_pinned() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/w.ts", "typescript"),
        ]);
        let r = rel_ref("src/main.ts", "./w");
        let edges = resolver()
            .resolve(std::slice::from_ref(&r), &index)
            .unwrap();
        assert_eq!(edges.len(), 1);
        let e = &edges[0];
        assert_eq!(e.kind, EdgeKind::Imports);
        assert_eq!(e.source, Symbol::file("src/main.ts").id());
        assert_eq!(e.target, Symbol::file("src/w.ts").id());
        assert_eq!(
            e.location.as_ref(),
            Some(&r.location),
            "the edge must carry the REF's location — that is what marks the ref resolved (D01-11)"
        );
        assert_eq!(e.resolved_by, RELATIVE_IMPORT_RESOLVER_ID);
        assert_eq!(e.provenance, Provenance::ImportMap);
        assert!((e.confidence.get() - 0.9).abs() < 1e-6, "0.9 override");
        assert_eq!(
            e.metadata.get("via").and_then(|v| v.as_str()),
            Some("relative-path")
        );
        assert_eq!(
            e.metadata.get("rule").and_then(|v| v.as_str()),
            Some("probe")
        );
    }

    #[test]
    fn rule_metadata_names_the_probe_slot() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/q.ts", "typescript"),
            file_node("src/styles.css", "css"),
            file_node("src/c/index.ts", "typescript"),
        ]);
        let rule_of = |spec: &str| -> String {
            let edges = resolver()
                .resolve(&[rel_ref("src/main.ts", spec)], &index)
                .unwrap();
            edges[0]
                .metadata
                .get("rule")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string()
        };
        assert_eq!(rule_of("./q.js"), "remap");
        assert_eq!(rule_of("./styles.css"), "literal");
        assert_eq!(rule_of("./q"), "probe");
        assert_eq!(rule_of("./c"), "index");
    }

    #[test]
    fn resolve_all_keeps_the_higher_confidence_relative_edge() {
        // The 0.9 override must win resolve_all_with_coverage's max-confidence dedup over a lower-confidence
        // duplicate of the SAME (source, target, kind) — Decision E / ATT-INV-6.
        struct LowConfImportsResolver;
        impl Resolver for LowConfImportsResolver {
            fn id(&self) -> &str {
                "low-conf-imports"
            }
            fn tier(&self) -> ResolutionTier {
                ResolutionTier::Tsg // 0.8 default — the strongest tier that emits today
            }
            fn resolve(
                &self,
                refs: &[UnresolvedRef],
                _index: &dyn SymbolIndex,
            ) -> CoreResult<Vec<Edge>> {
                Ok(refs
                    .iter()
                    .map(|r| {
                        Edge::new(
                            r.from.clone(),
                            Symbol::file("src/w.ts").id(),
                            EdgeKind::Imports,
                            ResolutionTier::Tsg,
                            "low-conf-imports",
                        )
                        .with_location(r.location.clone())
                    })
                    .collect())
            }
        }

        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/w.ts", "typescript"),
        ]);
        let relative = resolver();
        let low = LowConfImportsResolver;
        let resolvers: &[&dyn Resolver] = &[&low, &relative];
        let edges = resolve_all_with_coverage(resolvers, &[rel_ref("src/main.ts", "./w")], &index).unwrap().edges;
        assert_eq!(edges.len(), 1, "one deduped edge: {edges:?}");
        assert_eq!(edges[0].resolved_by, RELATIVE_IMPORT_RESOLVER_ID);
        assert!((edges[0].confidence.get() - 0.9).abs() < 1e-6);
    }

    #[test]
    fn resolution_is_deterministic() {
        let index = VecIndex(vec![
            file_node("src/main.ts", "typescript"),
            file_node("src/a.ts", "typescript"),
            file_node("src/a/index.ts", "typescript"),
            file_node("src/q.ts", "typescript"),
            file_node("src/q.js", "javascript"),
            file_node("src/c/index.ts", "typescript"),
        ]);
        let refs = vec![
            rel_ref("src/main.ts", "./a"),
            rel_ref("src/main.ts", "./q.js"),
            rel_ref("src/main.ts", "./c"),
        ];
        let run = || {
            resolver()
                .resolve(&refs, &index)
                .unwrap()
                .iter()
                .map(|e| (e.source.0.clone(), e.target.0.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run(), "same input twice → identical output");
    }

    /// S9 complexity guard (ATT-INV-5): a deterministic OPERATION-COUNT invariant, not a
    /// wall-clock assertion. 20k File nodes / 100k refs: total exact-map probes stay ≤
    /// refs × 14 and the File map is built exactly ONCE — the O(refs × files) class
    /// (review doc 01: ~234 ns × refs × files ≈ 39 min on a 50k-file monorepo) cannot pass.
    /// Wall-clock lives in the lane's §5 release measurement protocol, not in the suite.
    #[test]
    fn twenty_k_files_hundred_k_refs_bounded_probes() {
        let n_files = 20_000usize;
        let mut nodes = Vec::with_capacity(n_files);
        for i in 0..n_files {
            nodes.push(file_node(&format!("src/m{i}.ts"), "typescript"));
        }
        let index = VecIndex(nodes);

        // 100k refs: 90k extensionless binds (1 probe each), 5k .js remap binds (1 probe),
        // 5k parked specs (worst case: full probe + index slots).
        let mut refs = Vec::with_capacity(100_000);
        for i in 0..90_000usize {
            let target = i % n_files;
            refs.push(rel_ref(
                &format!("src/m{}.ts", (i + 1) % n_files),
                &format!("./m{target}"),
            ));
        }
        for i in 0..5_000usize {
            let target = i % n_files;
            refs.push(rel_ref(
                &format!("src/m{}.ts", (i + 3) % n_files),
                &format!("./m{target}.js"),
            ));
        }
        for i in 0..5_000usize {
            refs.push(rel_ref(
                &format!("src/m{}.ts", i % n_files),
                &format!("./missing{i}"),
            ));
        }

        let (edges, stats) = resolver().resolve_with_stats(&refs, &index).unwrap();
        assert_eq!(edges.len(), 95_000, "all non-parked refs bind");
        assert_eq!(stats.map_builds, 1, "the File map is built exactly once");
        assert!(
            stats.probes <= refs.len() * 14,
            "probes must stay O(refs): {} > {} × 14",
            stats.probes,
            refs.len()
        );
    }
}
