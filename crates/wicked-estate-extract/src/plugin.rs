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
//! Lookups in [`TreeSitterExtractor`](crate::TreeSitterExtractor) consult [`LANG_TABLE`] first, then
//! loaded plugins — so a plugin never shadows a built-in.
//!
//! ## Safety
//! Loading a plugin `dlopen`s user-provided native code (inherently `unsafe`). After loading we
//! validate the grammar's tree-sitter ABI version (must be 13–15 for the 0.25 runtime) and skip —
//! with a warning to stderr — any plugin that fails to load or is ABI-incompatible, rather than
//! aborting. The grammar function pointer is kept valid by holding the `Library` for the process
//! lifetime.

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
    pub library: String,
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

static REGISTRY: OnceLock<Vec<LoadedPlugin>> = OnceLock::new();

/// All loaded plugins (scanned + loaded once, lazily, on first access).
pub fn loaded() -> &'static [LoadedPlugin] {
    REGISTRY.get_or_init(load_all)
}

/// Find a loaded plugin by language name.
pub fn find_by_name(name: &str) -> Option<&'static LoadedPlugin> {
    loaded().iter().find(|p| p.name == name)
}

/// Find a loaded plugin claiming a file extension (case-insensitive, leading dot tolerated).
pub fn find_by_extension(ext: &str) -> Option<&'static LoadedPlugin> {
    let needle = ext.trim_start_matches('.').to_ascii_lowercase();
    loaded()
        .iter()
        .find(|p| p.extensions.iter().any(|e| e.eq_ignore_ascii_case(&needle)))
}

fn load_all() -> Vec<LoadedPlugin> {
    let Some(dir) = plugins_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new(); // no plugins dir → no plugins (not an error)
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        let manifest = p.join("plugin.toml");
        if !p.is_dir() || !manifest.is_file() {
            continue;
        }
        match load_one(&p, &manifest) {
            Ok(plugin) => out.push(plugin),
            Err(e) => eprintln!("wicked-estate: skipping plugin at {}: {e}", p.display()),
        }
    }
    out
}

fn load_one(dir: &Path, manifest_path: &Path) -> Result<LoadedPlugin, String> {
    let text = std::fs::read_to_string(manifest_path).map_err(|e| format!("read manifest: {e}"))?;
    let m: PluginManifest = toml::from_str(&text).map_err(|e| format!("parse manifest: {e}"))?;

    let lib_path = resolve_library(dir, &m.library)?;
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

    Ok(LoadedPlugin {
        name: m.name,
        extensions: m.extensions,
        language,
        query_src,
        license: m.license,
        _lib: lib,
    })
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
