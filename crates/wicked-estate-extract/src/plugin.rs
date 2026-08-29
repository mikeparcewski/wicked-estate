//! Runtime language plugins — drop-in tree-sitter grammars loaded at runtime.
//!
//! Languages wired into [`LANG_TABLE`](crate::treesitter) are compiled into this (MIT) crate. A
//! *plugin* is the opposite: a grammar that lives entirely OUTSIDE the build. You drop a directory
//! into the plugins folder and wicked-estate loads it at startup:
//!
//! ```text
//! <plugins_dir>/nginx/
//!   plugin.toml      # manifest: name, extensions, library, symbol, query, abi, license
//!   libnginx.dylib   # the compiled tree-sitter grammar (its OWN license)
//!   nginx.scm        # the @code_* extraction query
//! ```
//!
//! Why this matters: the grammar is a **separate binary artifact**, never linked into this crate at
//! build time. So a grammar under a license incompatible with MIT (GPL, etc.) can be used as a
//! plugin without that license touching the core — the user obtains/builds the plugin themselves and
//! drops it in. Adding a language needs no recompile of wicked-estate.
//!
//! The plugins directory is `$WICKED_ESTATE_PLUGINS` if set, else `~/.wicked-estate/plugins`.
//!
//! ## Precedence (ADR-010)
//!
//! Three tiers: **built-in < query-only override < full grammar override.**
//!
//! - A plugin with no override fields is *additive*: lookups consult [`LANG_TABLE`] first, then
//!   loaded plugins, so an additive plugin never shadows a built-in (the pre-ADR-010 behaviour,
//!   unchanged).
//! - `override_query = "<lang>"` in `plugin.toml` makes the plugin a **query-only override**:
//!   its `.scm` replaces the built-in query for that `LANG_TABLE` entry, on the shipped grammar.
//!   No shared library is required (or loaded). Activation is manifest-only.
//! - `override = true` plus the language named in `WICKED_ESTATE_PLUGIN_OVERRIDE` (comma-separated
//!   exact names) arms a **full grammar override**: plugin grammar + plugin query replace the
//!   built-in pair for that language. Both signals are required — a manifest flag alone is INERT.
//!
//! Override queries are compiled **eagerly at registry load** (query-only: against the built-in
//! grammar; grammar tier: against the plugin's own grammar). A failed compile falls back LOUDLY to
//! the built-in extractor — an override can never make a built-in language unavailable. Duplicate
//! overrides of one language (any mode) disable each other, loudly. An armed override's claimed
//! extension owned by a *different* built-in language is refused unless that language is also
//! named in `WICKED_ESTATE_PLUGIN_OVERRIDE`.
//!
//! ## Safety
//! Loading a plugin `dlopen`s user-provided native code (inherently `unsafe`). After loading we
//! validate the grammar's tree-sitter ABI version (must be 13–15 for the 0.25 runtime) and skip —
//! with a warning to stderr — any plugin that fails to load or is ABI-incompatible, rather than
//! aborting. (That skip-with-warning covers load/ABI/manifest failures; a broken *override* query
//! additionally falls back to the built-in with a loud marker. A broken query on an *additive*
//! plugin still silently disables that plugin's language — pre-existing behaviour, unchanged
//! here.) The grammar function pointer is kept valid by holding the `Library` for the process
//! lifetime.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use tree_sitter_language::LanguageFn;

/// Lowest / highest tree-sitter ABI the 0.25 runtime can load.
const MIN_ABI: usize = 13;
const MAX_ABI: usize = 15;

/// A `plugin.toml` manifest.
#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    /// Language name (e.g. `nginx`). This is what `for_language("nginx")` matches.
    pub name: String,
    /// File extensions (no dot) this language claims for extension dispatch.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Base name of the shared library in the plugin dir (e.g. `libnginx` or `nginx`). The platform
    /// extension (`.so`/`.dylib`/`.dll`) and `lib` prefix are tried automatically.
    /// Required unless `override_query` is set (a query-only override loads no native code).
    #[serde(default)]
    pub library: Option<String>,
    /// The exported C symbol returning the grammar (defaults to `tree_sitter_<name>`).
    #[serde(default)]
    pub symbol: Option<String>,
    /// Query filename (relative to the plugin dir) in the `@code_*` convention.
    pub query: String,
    /// Informational: the grammar's tree-sitter ABI version.
    #[serde(default)]
    pub abi: Option<usize>,
    /// Informational: the grammar/plugin's SPDX license (may differ from this crate's MIT).
    #[serde(default)]
    pub license: Option<String>,
    /// Informational: extraction capabilities (`symbols`, `calls`, …).
    #[serde(default)]
    pub caps: Vec<String>,
    /// Query-only override (ADR-010 tier 2): names the built-in `LANG_TABLE` entry whose query
    /// this plugin's `.scm` replaces. Shipped grammar, user query; no library involved.
    #[serde(default)]
    pub override_query: Option<String>,
    /// Full grammar override flag (ADR-010 tier 3). Armed only when the language is ALSO named in
    /// `WICKED_ESTATE_PLUGIN_OVERRIDE` — the flag alone is inert. (`override` is a Rust keyword,
    /// hence the rename.)
    #[serde(default, rename = "override")]
    pub override_grammar: bool,
    /// Any manifest key this version does not know. Warned about by name at load so a typo like
    /// `override-query` is visible instead of silently dropped.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, toml::Value>,
}

/// A successfully loaded plugin grammar, ready to drive a `TreeSitterExtractor`.
pub struct LoadedPlugin {
    pub name: String,
    pub extensions: Vec<String>,
    pub language: tree_sitter::Language,
    pub query_src: String,
    pub license: Option<String>,
    // The dlopen'd library MUST outlive the `language` fn pointer — held for the process lifetime.
    _lib: libloading::Library,
}

/// A query-only override of a built-in language (ADR-010 tier 2): shipped grammar, user query.
pub struct QueryOverride {
    /// The overridden `LANG_TABLE` entry name.
    pub lang: String,
    /// The plugin directory the override was loaded from.
    pub dir: PathBuf,
    /// The override query source, cached for the process lifetime (the digest is over these
    /// cached bytes — never a fresh disk read).
    pub query_src: String,
    /// Eager compile result against the built-in grammar. `Err` means the built-in query stays in
    /// use (loud fallback, ADR-010).
    pub compiled: Result<(), String>,
    /// 16-hex digest of the cached query bytes (descriptor line component).
    pub digest: String,
}

/// An ARMED, eagerly-compiled full grammar override of a built-in language (ADR-010 tier 3).
/// Only overrides that passed both opt-in signals AND the eager query compile appear here.
pub struct GrammarOverride {
    /// The overridden `LANG_TABLE` entry name.
    pub lang: String,
    /// The plugin directory the override was loaded from.
    pub dir: PathBuf,
    /// The dlopen'd grammar + cached query (compile against it verified at load).
    pub plugin: LoadedPlugin,
    /// Extension claims that survived the cross-language ownership filter (ADR-010: a claim on an
    /// extension owned by a DIFFERENT built-in is dropped unless that owner is also named in
    /// `WICKED_ESTATE_PLUGIN_OVERRIDE`).
    pub extensions: Vec<String>,
    /// 16-hex digest of the cached query bytes + dylib bytes (descriptor line component).
    pub digest: String,
}

/// One row for `wicked-estate plugins list`.
pub struct PluginListing {
    pub name: String,
    /// Plugin directory basename.
    pub dir: String,
    pub extensions: Vec<String>,
    pub license: Option<String>,
    /// Override status column (`None` for a plain additive plugin). States: active query override,
    /// query FAILED, grammar armed, grammar FAILED, grammar INERT, DISABLED duplicate.
    pub status: Option<String>,
}

/// The plugins directory: `$WICKED_ESTATE_PLUGINS` or `~/.wicked-estate/plugins`.
pub fn plugins_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("WICKED_ESTATE_PLUGINS") {
        if !d.is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(Path::new(&home).join(".wicked-estate").join("plugins"))
}

/// `WICKED_ESTATE_PLUGIN_OVERRIDE` parsed as comma-separated exact language names. Read once at
/// first registry access (OnceLock semantics, ADR-010); no wildcard.
fn override_env_list() -> Vec<String> {
    std::env::var("WICKED_ESTATE_PLUGIN_OVERRIDE")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Is the full grammar override of `lang` armed? Pure double-opt-in rule (ADR-010): the manifest
/// flag AND the env list must both name it. Pure so the four-way matrix is unit-testable without
/// a dylib or env mutation.
pub fn grammar_override_armed(manifest_flag: bool, env_list: &[String], lang: &str) -> bool {
    manifest_flag && env_list.iter().any(|l| l == lang)
}

/// Everything the registry knows: additive plugins, overrides (effective and failed), listing
/// rows, and the canonical override descriptor.
struct Registry {
    /// Additive (non-override) plugins — the pre-ADR-010 registry, semantics unchanged.
    plugins: Vec<LoadedPlugin>,
    /// Query-only overrides for built-in languages (compiled-ok AND failed; failed ones stay
    /// listed but out of the effective set).
    query_overrides: Vec<QueryOverride>,
    /// Armed, compile-verified grammar overrides.
    grammar_overrides: Vec<GrammarOverride>,
    /// Rows for `plugins list` (includes inert/failed/disabled override plugins).
    listings: Vec<PluginListing>,
    /// Canonical descriptor of the EFFECTIVE override set (ADR-010): sorted
    /// `<lang>|<mode>|<dir basename>|<digest>` lines; empty when none. Both the index gate value
    /// and the audit record.
    descriptor: String,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| build_registry(plugins_dir().as_deref(), &override_env_list()))
}

/// All loaded additive plugins (scanned + loaded once, lazily, on first access).
pub fn loaded() -> &'static [LoadedPlugin] {
    &registry().plugins
}

/// Find a loaded additive plugin by language name.
pub fn find_by_name(name: &str) -> Option<&'static LoadedPlugin> {
    loaded().iter().find(|p| p.name == name)
}

/// Find a loaded additive plugin claiming a file extension (case-insensitive, leading dot
/// tolerated).
pub fn find_by_extension(ext: &str) -> Option<&'static LoadedPlugin> {
    let needle = ext.trim_start_matches('.').to_ascii_lowercase();
    loaded()
        .iter()
        .find(|p| p.extensions.iter().any(|e| e.eq_ignore_ascii_case(&needle)))
}

/// The active (compiled-ok) query-only override source for a built-in language, if any.
/// A failed override returns `None` here — the caller uses the built-in query (loud fallback
/// already fired at registry load).
pub fn override_query_for(lang: &str) -> Option<&'static str> {
    registry()
        .query_overrides
        .iter()
        .find(|o| o.lang == lang && o.compiled.is_ok())
        .map(|o| o.query_src.as_str())
}

/// The armed, compile-verified grammar override for a built-in language, if any.
pub fn grammar_override_for_name(lang: &str) -> Option<&'static GrammarOverride> {
    registry().grammar_overrides.iter().find(|o| o.lang == lang)
}

/// The armed grammar override claiming a file extension (surviving claims only).
pub fn grammar_override_for_ext(ext: &str) -> Option<&'static GrammarOverride> {
    let needle = ext.trim_start_matches('.').to_ascii_lowercase();
    registry()
        .grammar_overrides
        .iter()
        .find(|o| o.extensions.iter().any(|e| e == &needle))
}

/// The canonical descriptor of the effective override set — the `plugin_overrides` meta-key value
/// (gate + audit record, ADR-010). Empty string when no override is active.
pub fn override_state() -> &'static str {
    &registry().descriptor
}

/// Rows for `wicked-estate plugins list` — additive plugins plus every override plugin dir,
/// including inert / failed / disabled ones.
pub fn listings() -> &'static [PluginListing] {
    &registry().listings
}

/// 16-hex content digest (xxh3, the workspace's digest function family).
fn digest16(bytes: &[u8]) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(bytes))
}

fn dir_base(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// A classified plugin dir, pre-duplicate-check.
impl std::fmt::Debug for Classified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Classified::Additive(_) => "Additive",
            Classified::QueryOv(..) => "QueryOv",
            Classified::GrammarOv(..) => "GrammarOv",
            Classified::ListedOnly(_) => "ListedOnly",
        })
    }
}

enum Classified {
    Additive(LoadedPlugin),
    QueryOv(QueryOverride, PluginListing),
    GrammarOv(GrammarOverride, PluginListing),
    /// Listed but inactive (inert grammar override, failed armed grammar override).
    ListedOnly(PluginListing),
}

fn build_registry(dir: Option<&Path>, env_list: &[String]) -> Registry {
    let empty = Registry {
        plugins: Vec::new(),
        query_overrides: Vec::new(),
        grammar_overrides: Vec::new(),
        listings: Vec::new(),
        descriptor: String::new(),
    };
    let Some(dir) = dir else {
        return empty;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return empty; // no plugins dir → no plugins (not an error)
    };
    // Deterministic order (ADR-010): sorted by path, so the registry, `plugins list`, and the
    // override descriptor never depend on read_dir order.
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();

    let mut classified: Vec<Classified> = Vec::new();
    for p in &dirs {
        let manifest = p.join("plugin.toml");
        if !p.is_dir() || !manifest.is_file() {
            continue;
        }
        match classify_one(p, &manifest, env_list) {
            Ok(Some(c)) => classified.push(c),
            Ok(None) => {} // inert-and-unlisted never happens today; reserved
            Err(e) => eprintln!("wicked-estate: skipping plugin at {}: {e}", p.display()),
        }
    }

    // Duplicate refusal (ADR-010), including cross-mode: two override records for one language
    // disable each other loudly. Deterministic refusal beats a silent arbitrary winner.
    let mut lang_dirs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &classified {
        match c {
            Classified::QueryOv(o, _) => lang_dirs
                .entry(o.lang.clone())
                .or_default()
                .push(dir_base(&o.dir)),
            Classified::GrammarOv(o, _) => lang_dirs
                .entry(o.lang.clone())
                .or_default()
                .push(dir_base(&o.dir)),
            _ => {}
        }
    }
    let duplicated: Vec<&String> = lang_dirs
        .iter()
        .filter(|(_, dirs)| dirs.len() > 1)
        .map(|(lang, _)| lang)
        .collect();
    for lang in &duplicated {
        eprintln!(
            "PLUGIN-OVERRIDE: {n} plugin dirs override '{lang}' ({dirs}) — ALL disabled; keep exactly one",
            n = lang_dirs[*lang].len(),
            dirs = lang_dirs[*lang].join(", "),
        );
    }
    let is_dup = |lang: &str| duplicated.iter().any(|l| l.as_str() == lang);
    let other_dirs = |lang: &str, own: &str| -> String {
        lang_dirs
            .get(lang)
            .map(|ds| {
                ds.iter()
                    .filter(|d| d.as_str() != own)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };

    let mut reg = empty;
    for c in classified {
        match c {
            Classified::Additive(p) => {
                reg.listings.push(PluginListing {
                    name: p.name.clone(),
                    dir: String::new(),
                    extensions: p.extensions.clone(),
                    license: p.license.clone(),
                    status: None,
                });
                reg.plugins.push(p);
            }
            Classified::QueryOv(o, mut l) => {
                if is_dup(&o.lang) {
                    let own = dir_base(&o.dir);
                    l.status = Some(format!(
                        "override=query({}) DISABLED: duplicate of {}",
                        o.lang,
                        other_dirs(&o.lang, &own)
                    ));
                    reg.listings.push(l);
                } else {
                    reg.listings.push(l);
                    reg.query_overrides.push(o);
                }
            }
            Classified::GrammarOv(o, mut l) => {
                if is_dup(&o.lang) {
                    let own = dir_base(&o.dir);
                    l.status = Some(format!(
                        "override=grammar({}) DISABLED: duplicate of {}",
                        o.lang,
                        other_dirs(&o.lang, &own)
                    ));
                    reg.listings.push(l);
                } else {
                    reg.listings.push(l);
                    reg.grammar_overrides.push(o);
                }
            }
            Classified::ListedOnly(l) => reg.listings.push(l),
        }
    }

    reg.descriptor = descriptor_of(&reg.query_overrides, &reg.grammar_overrides);
    reg
}

/// Canonical descriptor of the EFFECTIVE override set (ADR-010): sorted
/// `<lang>|<mode>|<dir basename>|<digest>` lines. Only compiled-ok query overrides and armed,
/// compile-verified grammar overrides are effective — a failed or inert override drops out, so a
/// graph previously extracted under it honestly re-extracts under the built-in.
fn descriptor_of(query: &[QueryOverride], grammar: &[GrammarOverride]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for o in query.iter().filter(|o| o.compiled.is_ok()) {
        lines.push(format!(
            "{}|query|{}|{}",
            o.lang,
            dir_base(&o.dir),
            o.digest
        ));
    }
    for o in grammar {
        lines.push(format!(
            "{}|grammar|{}|{}",
            o.lang,
            dir_base(&o.dir),
            o.digest
        ));
    }
    lines.sort();
    lines.join("\n")
}

/// Warn (never fail) about manifest keys this version does not know — a typo like
/// `override-query` must be visible, not silently dropped.
fn warn_unknown_keys(dir: &Path, m: &PluginManifest) {
    for k in m.unknown.keys() {
        eprintln!(
            "wicked-estate: plugin at {}: unknown manifest key `{k}` (ignored)",
            dir.display()
        );
    }
}

/// A compiled override query that captures none of the recognized roles extracts nothing —
/// loud warning for the compiles-but-useless case.
fn warn_zero_roles(dir: &Path, lang: &str, q: &tree_sitter::Query) {
    let useful = q
        .capture_names()
        .iter()
        .any(|n| n.starts_with("code_") || *n == "call" || n.starts_with("call."));
    if !useful {
        eprintln!(
            "wicked-estate: override for '{lang}' at {} compiles but captures no @code_*/@call \
             roles — it will extract nothing",
            dir.display()
        );
    }
}

fn classify_one(
    dir: &Path,
    manifest_path: &Path,
    env_list: &[String],
) -> Result<Option<Classified>, String> {
    let text = std::fs::read_to_string(manifest_path).map_err(|e| format!("read manifest: {e}"))?;
    let m: PluginManifest = toml::from_str(&text).map_err(|e| format!("parse manifest: {e}"))?;
    warn_unknown_keys(dir, &m);

    if m.override_query.is_some() && m.override_grammar {
        return Err(
            "manifest sets both `override_query` and `override = true` — pick one mode".into(),
        );
    }

    // ── Tier 2: query-only override (manifest-only activation, ADR-010) ──────────────────────
    if let Some(lang) = &m.override_query {
        let Some(language) = crate::treesitter::builtin_language(lang) else {
            return Err(format!(
                "override_query '{lang}' names no built-in grammar language"
            ));
        };
        if m.library.is_some() {
            eprintln!(
                "wicked-estate: plugin at {}: `library` is ignored for a query-only override \
                 (shipped grammar, user query)",
                dir.display()
            );
        }
        if !m.extensions.is_empty() {
            eprintln!(
                "wicked-estate: plugin at {}: extension claims are ignored for a query-only \
                 override — dispatch follows the built-in '{lang}' entry",
                dir.display()
            );
        }
        let query_src = std::fs::read_to_string(dir.join(&m.query))
            .map_err(|e| format!("read query `{}`: {e}", m.query))?;
        // Eager compile against the built-in grammar (ADR-010): a broken override must fall back
        // to the built-in query LOUDLY at load — never inherit the silent-deletion `.ok()?` path.
        let compiled = match tree_sitter::Query::new(&language, &query_src) {
            Ok(q) => {
                warn_zero_roles(dir, lang, &q);
                Ok(())
            }
            Err(e) => {
                eprintln!(
                    "QUERY-OVERRIDE: {lang} override at {} failed to compile: {e} — using \
                     built-in query",
                    dir.display()
                );
                Err(e.to_string())
            }
        };
        let digest = digest16(query_src.as_bytes());
        let status = match &compiled {
            Ok(()) => format!("override=query({lang})"),
            Err(e) => format!("override=query({lang}) FAILED: {e} — built-in in use"),
        };
        let listing = PluginListing {
            name: m.name.clone(),
            dir: dir_base(dir),
            extensions: Vec::new(),
            license: m.license.clone(),
            status: Some(status),
        };
        return Ok(Some(Classified::QueryOv(
            QueryOverride {
                lang: lang.clone(),
                dir: dir.to_path_buf(),
                query_src,
                compiled,
                digest,
            },
            listing,
        )));
    }

    // ── Tiers 1 + 3: a real dylib plugin (additive, or a grammar override of a built-in) ─────
    let Some(library) = &m.library else {
        return Err("plugin has no `library` (required unless `override_query` is set)".into());
    };

    let overrides_builtin =
        m.override_grammar && crate::treesitter::builtin_language(&m.name).is_some();
    if m.override_grammar && !overrides_builtin {
        eprintln!(
            "wicked-estate: plugin at {}: `override = true` but '{}' is not a built-in language \
             — flag ignored, loaded as an ordinary plugin",
            dir.display(),
            m.name
        );
    }

    let (plugin, lib_path) = load_dylib(dir, &m, library)?;

    if !overrides_builtin {
        return Ok(Some(Classified::Additive(plugin)));
    }

    // ── Tier 3: grammar override — double opt-in (ADR-010) ───────────────────────────────────
    let lang = m.name.clone();
    if !grammar_override_armed(true, env_list, &lang) {
        // Manifest flag without the env var: fully INERT — not registered anywhere, only listed.
        return Ok(Some(Classified::ListedOnly(PluginListing {
            name: lang.clone(),
            dir: dir_base(dir),
            extensions: plugin.extensions.clone(),
            license: plugin.license.clone(),
            status: Some(format!(
                "override=grammar({lang}) [INERT — not named in WICKED_ESTATE_PLUGIN_OVERRIDE]"
            )),
        })));
    }

    // Eager compile against the plugin's OWN grammar (ADR-010): an armed override with a broken
    // query disarms to built-in grammar + built-in query, loudly — the `from_grammar` silent
    // `.ok()?` path must never see a user query for a built-in language.
    match tree_sitter::Query::new(&plugin.language, &plugin.query_src) {
        Ok(q) => warn_zero_roles(dir, &lang, &q),
        Err(e) => {
            eprintln!(
                "GRAMMAR-OVERRIDE: {lang} override at {} failed to compile: {e} — built-in \
                 grammar and query in use",
                dir.display()
            );
            return Ok(Some(Classified::ListedOnly(PluginListing {
                name: lang.clone(),
                dir: dir_base(dir),
                extensions: plugin.extensions.clone(),
                license: plugin.license.clone(),
                status: Some(format!(
                    "override=grammar({lang}) FAILED: {e} — built-in in use"
                )),
            })));
        }
    }

    // Cross-language extension capture rule (ADR-010): a claimed extension owned by a DIFFERENT
    // built-in language is refused unless that owner is also named in the env list. The double
    // opt-in is per captured language, not per plugin.
    let extensions = filter_ext_claims(&lang, &plugin.extensions, env_list);

    // Digest over the CACHED bytes extraction will actually use: query source + dylib bytes.
    let dylib_bytes =
        std::fs::read(&lib_path).map_err(|e| format!("read dylib for digest: {e}"))?;
    let mut digest_input = plugin.query_src.as_bytes().to_vec();
    digest_input.push(0);
    digest_input.extend_from_slice(&dylib_bytes);
    let digest = digest16(&digest_input);

    let listing = PluginListing {
        name: lang.clone(),
        dir: dir_base(dir),
        extensions: extensions.clone(),
        license: plugin.license.clone(),
        status: Some(format!("override=grammar({lang}) [armed]")),
    };
    Ok(Some(Classified::GrammarOv(
        GrammarOverride {
            lang,
            dir: dir.to_path_buf(),
            plugin,
            extensions,
            digest,
        },
        listing,
    )))
}

/// Apply the cross-language extension capture rule (ADR-010) to an armed override's claims:
/// keep an extension owned by the overridden language itself or by no built-in; drop — loudly —
/// one owned by a different built-in language, unless that owner is also named in `env_list`.
fn filter_ext_claims(lang: &str, claims: &[String], env_list: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for c in claims {
        let ext = c.trim_start_matches('.').to_ascii_lowercase();
        match crate::treesitter::builtin_owner_of_ext(&ext) {
            Some(owner) if owner != lang && !env_list.iter().any(|l| l == owner) => {
                eprintln!(
                    "GRAMMAR-OVERRIDE: extension '{ext}' is owned by built-in '{owner}' — claim \
                     dropped (name '{owner}' in WICKED_ESTATE_PLUGIN_OVERRIDE to allow)"
                );
            }
            _ => out.push(ext),
        }
    }
    out
}

/// dlopen + ABI-validate a plugin's shared library and read its query. Returns the loaded plugin
/// plus the resolved dylib path (the grammar-override digest reads its bytes).
fn load_dylib(
    dir: &Path,
    m: &PluginManifest,
    library: &str,
) -> Result<(LoadedPlugin, PathBuf), String> {
    let lib_path = resolve_library(dir, library)?;
    let symbol = m
        .symbol
        .clone()
        .unwrap_or_else(|| format!("tree_sitter_{}", m.name));

    // SAFETY: loading user-provided native code. We validate the ABI immediately after and never
    // hand out a Language whose backing library could be unloaded (the Library is moved into the
    // returned LoadedPlugin and lives for the process).
    let lib = unsafe {
        libloading::Library::new(&lib_path)
            .map_err(|e| format!("dlopen {}: {e}", lib_path.display()))?
    };
    let language: tree_sitter::Language = unsafe {
        let func: libloading::Symbol<unsafe extern "C" fn() -> *const ()> = lib
            .get(symbol.as_bytes())
            .map_err(|e| format!("symbol `{symbol}`: {e}"))?;
        LanguageFn::from_raw(*func).into()
    };

    let abi = language.abi_version();
    if !(MIN_ABI..=MAX_ABI).contains(&abi) {
        return Err(format!(
            "grammar ABI {abi} unsupported (this runtime loads {MIN_ABI}–{MAX_ABI})"
        ));
    }

    let query_src = std::fs::read_to_string(dir.join(&m.query))
        .map_err(|e| format!("read query `{}`: {e}", m.query))?;

    Ok((
        LoadedPlugin {
            name: m.name.clone(),
            extensions: m.extensions.clone(),
            language,
            query_src,
            license: m.license.clone(),
            _lib: lib,
        },
        lib_path,
    ))
}

/// Resolve a shared-library base name to a real file in `dir`, trying the name as-given, then with
/// the platform extension, then with the platform `lib` prefix + extension.
fn resolve_library(dir: &Path, base: &str) -> Result<PathBuf, String> {
    let candidates = [
        base.to_string(),
        format!("{base}{}", std::env::consts::DLL_SUFFIX),
        format!(
            "{}{base}{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ),
    ];
    for c in &candidates {
        let p = dir.join(c);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(format!(
        "shared library `{base}` not found in {} (tried {candidates:?})",
        dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Manifest parsing (D12) ────────────────────────────────────────────────────────────────

    #[test]
    fn nginx_manifest_stays_byte_valid() {
        // The shipped example manifest must parse unchanged after `library` relaxed to Option.
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/nginx/plugin.toml"),
        )
        .expect("nginx example manifest exists");
        let m: PluginManifest = toml::from_str(&text).expect("nginx manifest parses");
        assert_eq!(m.name, "nginx");
        assert!(m.library.is_some(), "nginx example declares a library");
        assert!(m.override_query.is_none());
        assert!(!m.override_grammar);
        assert!(
            m.unknown.is_empty(),
            "nginx example has no unknown keys: {:?}",
            m.unknown.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn query_only_override_manifest_parses_without_library() {
        let m: PluginManifest = toml::from_str(
            r#"
name = "ts-patch"
query = "typescript.scm"
override_query = "typescript"
"#,
        )
        .expect("query-only manifest parses");
        assert_eq!(m.override_query.as_deref(), Some("typescript"));
        assert!(m.library.is_none());
        assert!(!m.override_grammar);
    }

    #[test]
    fn grammar_override_manifest_parses() {
        let m: PluginManifest = toml::from_str(
            r#"
name = "typescript"
library = "libts"
query = "ts.scm"
override = true
"#,
        )
        .expect("grammar-override manifest parses");
        assert!(m.override_grammar);
        assert!(m.override_query.is_none());
    }

    #[test]
    fn unknown_keys_are_captured_not_fatal() {
        let m: PluginManifest = toml::from_str(
            r#"
name = "x"
library = "libx"
query = "x.scm"
override-query = "typescript"
"#,
        )
        .expect("unknown keys must not be fatal");
        // The typo'd key lands in `unknown` (warned by name at load), not silently dropped.
        assert!(m.unknown.contains_key("override-query"));
        assert!(m.override_query.is_none());
    }

    // ── Override target validation (D12) — via classify_one on temp dirs ────────────────────

    fn write_plugin(dir: &Path, manifest: &str, query_name: &str, query: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("plugin.toml"), manifest).unwrap();
        std::fs::write(dir.join(query_name), query).unwrap();
        dir.join("plugin.toml")
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "we-plugin-unit-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn override_query_target_must_be_a_lang_table_entry() {
        for (tag, target) in [
            ("grammarless", "jcl"),
            ("unknown", "no-such-language"),
            ("pluginname", "nginx"),
        ] {
            let d = tmpdir(&format!("target-{tag}"));
            let dir = d.join("p");
            let manifest =
                format!("name = \"p\"\nquery = \"q.scm\"\noverride_query = \"{target}\"\n");
            write_plugin(&dir, &manifest, "q.scm", "(comment) @code_function.def");
            let err = classify_one(&dir, &dir.join("plugin.toml"), &[])
                .expect_err("non-LANG_TABLE override_query target must be refused");
            assert!(
                err.contains("names no built-in grammar language"),
                "unexpected error for {target}: {err}"
            );
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn both_modes_in_one_manifest_is_refused() {
        let d = tmpdir("bothmodes");
        let dir = d.join("p");
        write_plugin(
            &dir,
            "name = \"typescript\"\nlibrary = \"libx\"\nquery = \"q.scm\"\noverride = true\noverride_query = \"typescript\"\n",
            "q.scm",
            "(comment) @code_function.def",
        );
        let err =
            classify_one(&dir, &dir.join("plugin.toml"), &[]).expect_err("both modes refused");
        assert!(err.contains("pick one mode"), "got: {err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn additive_plugin_without_library_is_refused() {
        let d = tmpdir("nolib");
        let dir = d.join("p");
        write_plugin(
            &dir,
            "name = \"typescript\"\nquery = \"q.scm\"\n",
            "q.scm",
            "(comment) @code_function.def",
        );
        let err = classify_one(&dir, &dir.join("plugin.toml"), &[])
            .expect_err("library-less non-override plugin refused");
        assert!(err.contains("no `library`"), "got: {err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // ── Query-only override classification (D7): compile-ok and compile-fail ─────────────────

    #[test]
    fn query_override_compiles_eagerly_and_fails_loudly_into_fallback_state() {
        // Good query: compiled Ok, effective.
        let d = tmpdir("qov-good");
        let dir = d.join("ts-patch");
        write_plugin(
            &dir,
            "name = \"ts-patch\"\nquery = \"q.scm\"\noverride_query = \"typescript\"\n",
            "q.scm",
            "(function_declaration name: (identifier) @code_function.name) @code_function.def",
        );
        let c = classify_one(&dir, &dir.join("plugin.toml"), &[])
            .expect("classify ok")
            .expect("classified");
        match c {
            Classified::QueryOv(o, l) => {
                assert!(o.compiled.is_ok());
                assert_eq!(o.lang, "typescript");
                assert_eq!(o.digest.len(), 16);
                assert_eq!(l.status.as_deref(), Some("override=query(typescript)"));
            }
            _ => panic!("expected QueryOv"),
        }
        let _ = std::fs::remove_dir_all(&d);

        // Broken query: compiled Err — recorded, listed FAILED, never fatal.
        let d = tmpdir("qov-bad");
        let dir = d.join("ts-broken");
        write_plugin(
            &dir,
            "name = \"ts-broken\"\nquery = \"q.scm\"\noverride_query = \"typescript\"\n",
            "q.scm",
            "(no_such_node_kind) @code_function.def",
        );
        let c = classify_one(&dir, &dir.join("plugin.toml"), &[])
            .expect("a broken override query is NOT a load error — it falls back")
            .expect("classified");
        match c {
            Classified::QueryOv(o, l) => {
                assert!(o.compiled.is_err());
                let s = l.status.expect("status set");
                assert!(s.contains("FAILED"), "got: {s}");
                assert!(s.contains("built-in in use"), "got: {s}");
            }
            _ => panic!("expected QueryOv"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // ── Arming pure fn (D10): all four signal combinations ───────────────────────────────────

    #[test]
    fn arming_requires_both_signals() {
        let env = vec!["typescript".to_string()];
        let empty: Vec<String> = Vec::new();
        assert!(grammar_override_armed(true, &env, "typescript"));
        assert!(!grammar_override_armed(true, &empty, "typescript"));
        assert!(!grammar_override_armed(false, &env, "typescript"));
        assert!(!grammar_override_armed(false, &empty, "typescript"));
        // Exact-name match only — no wildcard, no prefix.
        assert!(!grammar_override_armed(true, &env, "tsx"));
        let wild = vec!["*".to_string()];
        assert!(!grammar_override_armed(true, &wild, "typescript"));
    }

    #[test]
    fn env_list_parsing_is_comma_separated_and_trimmed() {
        // Parse shape only (the fn under test is pure string logic, no env mutation).
        let parsed: Vec<String> = " typescript , tsx ,,python "
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(parsed, vec!["typescript", "tsx", "python"]);
    }

    // ── Ext-claim ownership refusal (I3/D10) ─────────────────────────────────────────────────

    #[test]
    fn foreign_owned_extension_claim_is_dropped() {
        let env = vec!["typescript".to_string()];
        let claims = vec!["ts".to_string(), "py".to_string(), "customext".to_string()];
        let kept = filter_ext_claims("typescript", &claims, &env);
        // Own extension survives; python-owned `py` dropped; non-built-in survives.
        assert_eq!(kept, vec!["ts", "customext"]);
    }

    #[test]
    fn foreign_claim_allowed_when_owner_also_named_in_env() {
        let env = vec!["typescript".to_string(), "python".to_string()];
        let claims = vec!["py".to_string()];
        let kept = filter_ext_claims("typescript", &claims, &env);
        assert_eq!(kept, vec!["py"]);
    }

    // ── Descriptor determinism + duplicate refusal (D5/D11) via build_registry ──────────────

    #[test]
    fn descriptor_is_sorted_and_stable_and_duplicates_are_refused() {
        // One plugins dir with: two overrides of `typescript` (duplicate → both disabled) and one
        // of `tsx` (survives). Deterministic regardless of read_dir order.
        let root = tmpdir("registry");
        let q = "(function_declaration name: (identifier) @code_function.name) @code_function.def";
        write_plugin(
            &root.join("a-ts"),
            "name = \"a\"\nquery = \"q.scm\"\noverride_query = \"typescript\"\n",
            "q.scm",
            q,
        );
        write_plugin(
            &root.join("b-ts"),
            "name = \"b\"\nquery = \"q.scm\"\noverride_query = \"typescript\"\n",
            "q.scm",
            q,
        );
        write_plugin(
            &root.join("c-tsx"),
            "name = \"c\"\nquery = \"q.scm\"\noverride_query = \"tsx\"\n",
            "q.scm",
            q,
        );
        let reg1 = build_registry(Some(&root), &[]);
        let reg2 = build_registry(Some(&root), &[]);
        assert_eq!(
            reg1.descriptor, reg2.descriptor,
            "descriptor must be stable"
        );
        // Only the tsx override is effective — the typescript pair disabled each other.
        assert_eq!(reg1.query_overrides.len(), 1);
        assert_eq!(reg1.query_overrides[0].lang, "tsx");
        let lines: Vec<&str> = reg1.descriptor.lines().collect();
        assert_eq!(lines.len(), 1, "descriptor: {}", reg1.descriptor);
        assert!(
            lines[0].starts_with("tsx|query|c-tsx|"),
            "got: {}",
            lines[0]
        );
        // The duplicates are visible in listings as DISABLED, naming the other dir.
        let disabled: Vec<&PluginListing> = reg1
            .listings
            .iter()
            .filter(|l| l.status.as_deref().is_some_and(|s| s.contains("DISABLED")))
            .collect();
        assert_eq!(disabled.len(), 2);
        assert!(
            disabled[0]
                .status
                .as_deref()
                .unwrap()
                .contains("duplicate of b-ts"),
            "got: {:?}",
            disabled[0].status
        );
        assert!(
            disabled[1]
                .status
                .as_deref()
                .unwrap()
                .contains("duplicate of a-ts"),
            "got: {:?}",
            disabled[1].status
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_registry_has_empty_descriptor() {
        let root = tmpdir("empty");
        let reg = build_registry(Some(&root), &[]);
        assert_eq!(reg.descriptor, "");
        assert!(reg.plugins.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn descriptor_changes_with_query_bytes() {
        let root = tmpdir("bytes");
        let dir = root.join("ts");
        write_plugin(
            &dir,
            "name = \"ts\"\nquery = \"q.scm\"\noverride_query = \"typescript\"\n",
            "q.scm",
            "(function_declaration name: (identifier) @code_function.name) @code_function.def",
        );
        let before = build_registry(Some(&root), &[]).descriptor;
        std::fs::write(
            dir.join("q.scm"),
            "(class_declaration name: (type_identifier) @code_class.name) @code_class.def",
        )
        .unwrap();
        let after = build_registry(Some(&root), &[]).descriptor;
        assert_ne!(
            before, after,
            "a semantic query edit must change the descriptor"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
