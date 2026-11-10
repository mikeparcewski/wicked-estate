//! The project linker's computation (DES-S1 §2, DES-S2 §3, slice X1): file-level cross-repo
//! `imports` links from package identity, never from names.
//!
//! [`link`] is a pure function of a [`LinkInput`] the caller reads from the store (the linker
//! never reads the disk): per member label, its stored manifests and lockfiles, the installed
//! package records captured at index time, its parked bare `imports` references, its indexed files,
//! and its recorded root and git remote. It returns the link edges, the boundaries (the "possible"
//! items: unverified, ambiguous, contradicted, undeclared, unmapped entries) and a fingerprint of
//! everything it read.
//!
//! The ecosystems are data (`ecosystems.toml`). Every link names the identity rule that decided its
//! provider ([`Identity`]); a dependency with no provenance is a boundary, never an edge.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// Bumped when the computation changes, so a stored link generation is recomputed.
pub const LINKER_VERSION: &str = "1";

const ECOSYSTEMS_SRC: &str = include_str!("../ecosystems.toml");

#[derive(Debug, Deserialize)]
struct EcoFile {
    ecosystem: Vec<Ecosystem>,
    #[serde(default)]
    identity: IdentityRules,
}

/// Repository-URL identity rules.
#[derive(Debug, Default, Deserialize)]
struct IdentityRules {
    /// Hosts whose repositories are exactly `owner/repo` (anything after is a view of the repo).
    #[serde(default)]
    two_level_hosts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Ecosystem {
    name: String,
    file_ext: Vec<String>,
    manifest: String,
    #[serde(default)]
    dep_tables: Vec<String>,
    #[serde(default)]
    lockfile: Option<String>,
    #[serde(default)]
    entries: Vec<String>,
    #[serde(default)]
    path_protocols: Vec<String>,
    #[serde(default)]
    alias_protocol: Option<String>,
    #[serde(default)]
    git_protocols: Vec<String>,
    #[serde(default)]
    builtin_prefixes: Vec<String>,
    #[serde(default)]
    local_roots: Vec<String>,
}

static ECOS: LazyLock<EcoFile> =
    LazyLock::new(|| toml::from_str(ECOSYSTEMS_SRC).expect("ecosystems.toml parses"));

fn eco(name: &str) -> &'static Ecosystem {
    ECOS.ecosystem
        .iter()
        .find(|e| e.name == name)
        .expect("ecosystem row")
}

/// A digest of the ecosystem rules (part of the link fingerprint).
pub fn rules_digest() -> String {
    format!("{:016x}", fnv(ECOSYSTEMS_SRC.as_bytes()))
}

fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in b {
        h ^= u64::from(x);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

// ── Input / output ────────────────────────────────────────────────────────────────────────────

/// One member label as the linker sees it. Paths are store paths (label-prefixed in a project db).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelInput {
    /// The `--repo` label ("" for an unlabelled db).
    pub label: String,
    /// The recorded checkout root (absolute), used to resolve `file:../x` across labels.
    pub root: Option<String>,
    /// The recorded git remote (exact identity class, DES-S7 §5).
    pub remote: Option<String>,
    /// Where the label root sits inside its git work tree (`""` at the top level; `None` unknown).
    /// A package's repository-relative directory is this joined with its label-local directory.
    #[serde(default)]
    pub subpath: Option<String>,
    /// Every indexed file path of the label.
    pub files: BTreeSet<String>,
    /// Stored text of the label's manifests and lockfiles (package.json, package-lock.json,
    /// tsconfig*.json, Cargo.toml, pyproject.toml), as X0's projection kept it.
    pub manifests: BTreeMap<String, String>,
    /// Installed npm package records captured at index time (one per declared dependency).
    pub artifacts: Vec<DepArtifact>,
    /// The label's parked bare `imports` references.
    pub imports: Vec<ImportRef>,
}

/// The installed copy of a declared npm dependency, as the consumer's index saw it
/// (`node_modules/<pkg>/package.json` and its `node_modules/.package-lock.json` record).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepArtifact {
    /// Store path of the consumer manifest directory the dependency was resolved from.
    pub manifest_dir: String,
    pub package: String,
    /// `name` of the installed manifest (the real package name, also under an `npm:` alias).
    #[serde(default)]
    pub name: Option<String>,
    pub version: Option<String>,
    pub repository: Option<String>,
    pub directory: Option<String>,
    /// The installed-tree record (`node_modules/.package-lock.json`) exists.
    #[serde(default)]
    pub tree_record: bool,
    /// The installed-tree record's entry for the package, when present.
    pub tree_version: Option<String>,
    pub tree_resolved: Option<String>,
    pub tree_integrity: Option<String>,
}

/// A parked import reference: the importing file and the written specifier (quotes kept or not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRef {
    pub file: String,
    pub raw: String,
}

/// The rule that decided a provider (`link.identity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Identity {
    Path,
    GitSource,
    RegistrySource,
    /// Registry source whose installed copy is bound to the lockfile by version only (no
    /// installed-tree record): lower confidence (DES-S1 §2.1 rule 3).
    RegistrySourceVersionOnly,
}

impl Identity {
    pub fn as_str(self) -> &'static str {
        match self {
            Identity::Path => "path",
            Identity::GitSource => "git-source",
            Identity::RegistrySource => "registry-source",
            Identity::RegistrySourceVersionOnly => "registry-source(version-only)",
        }
    }

    /// Placeholder confidences until the oracle measures them (DES-S1 §2.6 H6).
    pub fn confidence(self) -> f32 {
        match self {
            Identity::Path | Identity::GitSource | Identity::RegistrySource => 0.75,
            Identity::RegistrySourceVersionOnly => 0.6,
        }
    }
}

/// A file-level link: `from_file` (consumer) imports `to_file` (the provider's entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkEdge {
    pub from_file: String,
    pub to_file: String,
    pub ecosystem: String,
    pub package: String,
    pub identity: Identity,
    pub consumer_label: String,
    pub provider_label: String,
    pub consumer_version: Option<String>,
    pub provider_version: Option<String>,
    pub skew: bool,
}

/// Why a dependency is a "possible" item rather than a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryReason {
    /// Declared, a provider by that name exists, but nothing proves it is that provider.
    ProviderUnverified,
    /// More than one provider has the evidence.
    ProviderAmbiguous,
    /// The evidence names a different repository than every candidate.
    ProviderContradicted,
    /// Imported but not declared by the importing package (D5: never linked).
    Undeclared,
    /// The provider is decided but its entry is not an indexed file (or `tsc` build evidence is
    /// missing for a `dist` entry).
    EntryUnmapped,
    /// cargo `[patch]` / `[replace]` swaps the dependency's source; not followed in this slice.
    Patched,
    /// npm `overrides` / yarn `resolutions` replace the package with an alias, path or git source.
    Overridden,
}

impl BoundaryReason {
    pub fn as_str(self) -> &'static str {
        match self {
            BoundaryReason::ProviderUnverified => "dep:provider-unverified",
            BoundaryReason::ProviderAmbiguous => "dep:provider-ambiguous",
            BoundaryReason::ProviderContradicted => "dep:provider-contradicted",
            BoundaryReason::Undeclared => "dep:undeclared",
            BoundaryReason::EntryUnmapped => "link:entry-unmapped",
            BoundaryReason::Patched => "dep:patched",
            BoundaryReason::Overridden => "dep:override",
        }
    }
}

/// One boundary: a consumer label's package that could not be linked, with its importing files and
/// the candidate providers (`label:manifest`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Boundary {
    pub reason: BoundaryReason,
    pub ecosystem: String,
    pub consumer_label: String,
    pub package: String,
    pub files: Vec<String>,
    pub candidates: Vec<String>,
    /// Why the dependency stayed a possible item: one line per distinct cause, for a person to act
    /// on (`registry: no installed manifest recorded at index time`, ...).
    #[serde(default)]
    pub why: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinkOutput {
    pub edges: Vec<LinkEdge>,
    pub boundaries: Vec<Boundary>,
    /// Hash of every input row, the linker version and the rules digest.
    pub fingerprint: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────────────────────

fn prefix(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!("{label}/")
    }
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map(|(_, f)| f).unwrap_or(path)
}

/// `path` relative to its label's root: `""` for the label root itself (a store path equal to the
/// label has no trailing `/` to strip).
fn label_local<'s>(label: &str, path: &'s str) -> &'s str {
    if label.is_empty() {
        path
    } else if path == label {
        ""
    } else {
        path.strip_prefix(label)
            .and_then(|r| r.strip_prefix('/'))
            .unwrap_or(path)
    }
}

/// An absolute filesystem path in `/` form without a trailing `/` (Windows `\\` separators are
/// normalized, a drive prefix is kept).
fn norm_abs(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_string()
}

/// `dir` joined with a relative `rel` (`./`, `../` normalized). `None` when it climbs above "".
fn join(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn dequote(raw: &str) -> &str {
    raw.trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
}

/// Normalize a repository URL to the exact identity `host/owner/repo` (DES-S1 §2.1, DES-S7 §5):
/// scheme, `git+`, userinfo, `.git`, query, fragment and a trailing `/` removed; `git@host:o/r`
/// read as `host/o/r`; `github:o/r` as `github.com/o/r`; host lower-cased. `None` when it is not a
/// repository URL, or when X0 masked part of it (a masked identity is unverifiable, never compared).
pub fn repo_identity(url: &str) -> Option<String> {
    let mut u = url.trim();
    if u.contains("REDACTED") || u.is_empty() {
        return None;
    }
    for (short, host) in [
        ("github:", "github.com"),
        ("gitlab:", "gitlab.com"),
        ("bitbucket:", "bitbucket.org"),
    ] {
        if let Some(rest) = u.strip_prefix(short) {
            return finish_identity(host, rest);
        }
    }
    u = u.strip_prefix("git+").unwrap_or(u);
    u = u.split(['#', '?']).next().unwrap_or(u);
    if let Some(i) = u.find("://") {
        let rest = &u[i + 3..];
        let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        let host = host.split(':').next().unwrap_or(host);
        return finish_identity(host, path);
    }
    // scp-like `git@host:owner/repo`
    if let Some((left, path)) = u.split_once(':') {
        let host = left.rsplit_once('@').map(|(_, h)| h).unwrap_or(left);
        if host.contains('.') && !path.starts_with('/') {
            return finish_identity(host, path);
        }
    }
    None
}

fn finish_identity(host: &str, path: &str) -> Option<String> {
    let host = host.to_ascii_lowercase();
    // The repository path ends at a `-` segment (GitLab's `/-/tree/...` views).
    let segs: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .take_while(|s| *s != "-")
        .collect();
    if segs.len() < 2 {
        return None;
    }
    // Hosts with exactly `owner/repo` repositories keep two segments (`/tree/main/x` is a view);
    // any other host keeps the whole path: `group/subgroup/a` and `group/subgroup/b` differ.
    let n = if ECOS.identity.two_level_hosts.contains(&host) {
        2
    } else {
        segs.len()
    };
    let mut parts: Vec<&str> = segs[..n].to_vec();
    let last = parts.len() - 1;
    parts[last] = parts[last].strip_suffix(".git").unwrap_or(parts[last]);
    Some(format!("{host}/{}", parts.join("/")))
}

// ── Providers ─────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Provider {
    ecosystem: &'static str,
    /// Package name (npm name, cargo package name, normalized python dist name).
    name: String,
    /// Names code imports it by (npm: the name; cargo: the lib name; python: top-level packages).
    import_names: Vec<String>,
    label: String,
    /// Store path of the manifest directory.
    dir: String,
    version: Option<String>,
    /// Entry file for the package root, when it maps to an indexed file.
    entry: Option<String>,
    /// npm `exports` subpath entries (`./sub` -> store path).
    subpaths: BTreeMap<String, String>,
    /// python: package root directory (`src` or the manifest dir) holding the import packages.
    py_root: Option<String>,
}

impl Provider {
    fn key(&self) -> String {
        format!("{}:{}", self.label, self.dir)
    }
}

fn parse_json(text: &str) -> Option<serde_json::Value> {
    serde_json::from_str(text).ok()
}

fn parse_toml(text: &str) -> Option<toml::Value> {
    toml::from_str(text).ok()
}

fn json_str(v: &serde_json::Value, ptr: &str) -> Option<String> {
    v.pointer(ptr).and_then(|x| x.as_str()).map(str::to_string)
}

/// npm `exports` for one subpath (`"."` or `"./sub"`): a string, or a conditions object whose
/// first of `types`, `import`, `default`, `require`, `node` is taken (recursively).
fn exports_target(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    fn pick(v: &serde_json::Value) -> Option<String> {
        match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(m) => ["types", "import", "default", "require", "node"]
                .iter()
                .find_map(|k| m.get(*k).and_then(pick)),
            _ => None,
        }
    }
    match exports {
        serde_json::Value::String(s) if subpath == "." => Some(s.clone()),
        serde_json::Value::Object(m) => {
            if m.keys().any(|k| k.starts_with('.')) {
                m.get(subpath).and_then(pick)
            } else if subpath == "." {
                pick(exports)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Map an npm entry pointer (relative to the package dir) to an indexed file: the file itself, or
/// its source through `tsconfig-outdir` when the package's build is plain `tsc` (DES-S1 §2.3).
fn map_entry(li: &LabelInput, dir: &str, pointer: &str, pkg: &serde_json::Value) -> Option<String> {
    let path = join(dir, pointer)?;
    if li.files.contains(&path) {
        return Some(path);
    }
    tsconfig_outdir(li, dir, &path, pkg)
}

const BUNDLERS: &[&str] = &[
    "tsup",
    "rollup",
    "vite",
    "esbuild",
    "webpack",
    "bun build",
    "parcel",
    "swc",
];

/// Build steps a `tsc` build may chain with `&&` (output cleanup only).
const CLEANUP: &[&str] = &["rm", "rimraf", "del", "del-cli"];

/// The tsconfig a package's `build` script compiles with, when the build is exactly one plain `tsc`
/// (optionally after output cleanup): `-p`/`--project`/`-b`/`--build <config>`, else
/// `tsconfig.json`. `None` for anything else: a bundler anywhere, a second compile, a pipe or `;`,
/// another npm script, or a flag that moves or suppresses the output (the mapping reads the
/// config's `outDir`/`rootDir`, so a command-line override would make it wrong).
fn tsc_build_config(build: &str) -> Option<String> {
    if BUNDLERS.iter().any(|b| build.contains(b)) || build.contains([';', '|']) {
        return None;
    }
    let mut config: Option<String> = None;
    let mut compiles = 0;
    for seg in build.split("&&") {
        let words: Vec<&str> = seg.split_whitespace().collect();
        let first = *words.first()?;
        if first != "tsc" {
            // Cleanup only (`shx rm` is the portable `rm`).
            if CLEANUP.contains(&first) || (first == "shx" && words.get(1) == Some(&"rm")) {
                continue;
            }
            return None;
        }
        compiles += 1;
        let mut it = words[1..].iter().peekable();
        while let Some(w) = it.next() {
            match *w {
                "-p" | "--project" => config = Some(it.next()?.to_string()),
                "-b" | "--build" => {
                    if let Some(next) = it.peek().filter(|n| !n.starts_with('-')) {
                        config = Some(next.to_string());
                        it.next();
                    }
                    // `tsc -b a.json b.json` builds several projects: not one config.
                    if it.peek().is_some_and(|n| !n.starts_with('-')) {
                        return None;
                    }
                }
                "--clean" => return None,
                w if [
                    "--noEmit",
                    "--outDir",
                    "--outFile",
                    "--rootDir",
                    "--declarationDir",
                ]
                .iter()
                .any(|f| w == *f || w.starts_with(&format!("{f}="))) =>
                {
                    return None;
                }
                _ => {}
            }
        }
    }
    (compiles == 1).then(|| config.unwrap_or_else(|| "tsconfig.json".into()))
}

/// The common directory of the package's TypeScript sources (what `tsc` uses when `rootDir` is not
/// set): every non-declaration `.ts`/`.tsx`/`.mts`/`.cts` file under `dir`, outside `out` and
/// `node_modules`. Conservative: a stray source at the package top makes it the package dir, and
/// then a `dist` entry maps only when that file exists.
fn common_source_root(li: &LabelInput, dir: &str, out: &str) -> Option<String> {
    let pre = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let outp = format!("{out}/");
    let mut common: Option<Vec<&str>> = None;
    for f in li
        .files
        .iter()
        .filter(|f| f.starts_with(&pre) && !f.starts_with(&outp) && !f.contains("/node_modules/"))
    {
        let name = file_name(f);
        let ts = [".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|e| name.ends_with(e));
        if !ts || name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
            continue;
        }
        let parts: Vec<&str> = parent_dir(f).split('/').filter(|s| !s.is_empty()).collect();
        common = Some(match common {
            None => parts,
            Some(c) => c
                .iter()
                .zip(parts.iter())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    common.map(|c| c.join("/"))
}

fn tsconfig_outdir(
    li: &LabelInput,
    dir: &str,
    built: &str,
    pkg: &serde_json::Value,
) -> Option<String> {
    // Build evidence: the build script is one plain `tsc`, which names its config.
    let config = tsc_build_config(&json_str(pkg, "/scripts/build")?)?;
    let config = join(dir, &config)?;
    let config = if config.ends_with(".json") {
        config
    } else {
        join(&config, "tsconfig.json")?
    };
    let (out, root) = tsconfig_dirs(li, &config, 0)?;
    let out = out?;
    let root = match root {
        Some(r) => r,
        // No `rootDir`: tsc uses the common directory of its INPUTS. Inferred only when the
        // config selects no inputs itself (`files`/`include`/`exclude` anywhere in the chain
        // decide which files compile, and then the indexed files are not the input set).
        None if !tsconfig_selects_inputs(li, &config, 0) => common_source_root(li, dir, &out)?,
        None => return None,
    };
    let rest = built.strip_prefix(&format!("{out}/"))?;
    let stem = ["d.ts", "d.mts", "d.cts", "js", "mjs", "cjs"]
        .iter()
        .find_map(|ext| rest.strip_suffix(&format!(".{ext}")))?;
    ["ts", "tsx", "mts", "cts"]
        .iter()
        .map(|ext| {
            if root.is_empty() {
                format!("{stem}.{ext}")
            } else {
                format!("{root}/{stem}.{ext}")
            }
        })
        .find(|cand| li.files.contains(cand))
}

/// Whether a tsconfig (or a config it `extends`) chooses its input files itself.
fn tsconfig_selects_inputs(li: &LabelInput, path: &str, depth: u32) -> bool {
    let Some(v) = li.manifests.get(path).and_then(|t| parse_json(t)) else {
        return true; // unreadable: assume it does (no inference)
    };
    if ["files", "include", "exclude", "references"]
        .iter()
        .any(|k| v.get(*k).is_some())
        || v.pointer("/compilerOptions/rootDirs").is_some()
    {
        return true;
    }
    match json_str(&v, "/extends") {
        Some(ext) if depth < 3 && (ext.starts_with("./") || ext.starts_with("../")) => {
            let Some(t) = join(parent_dir(path), &ext) else {
                return true;
            };
            let t = if t.ends_with(".json") {
                t
            } else {
                format!("{t}.json")
            };
            tsconfig_selects_inputs(li, &t, depth + 1)
        }
        Some(_) => true, // a package-name base is not read: assume it selects
        None => false,
    }
}

/// A tsconfig's `(outDir, rootDir)`.
type TsDirs = (Option<String>, Option<String>);

/// `(outDir, rootDir)` of a tsconfig as store paths, each resolved against the directory of the
/// config that sets it; a child's value overrides its `extends` base (relative `extends` only,
/// within the label, to depth 3).
fn tsconfig_dirs(li: &LabelInput, path: &str, depth: u32) -> Option<TsDirs> {
    let v = parse_json(li.manifests.get(path)?)?;
    let here = parent_dir(path);
    let out = json_str(&v, "/compilerOptions/outDir").and_then(|o| join(here, &o));
    let root = json_str(&v, "/compilerOptions/rootDir").and_then(|r| join(here, &r));
    let (base_out, base_root) = match json_str(&v, "/extends") {
        Some(ext) if depth < 3 && (ext.starts_with("./") || ext.starts_with("../")) => {
            let target = join(here, &ext)?;
            let target = if target.ends_with(".json") {
                target
            } else {
                format!("{target}.json")
            };
            tsconfig_dirs(li, &target, depth + 1).unwrap_or((None, None))
        }
        _ => (None, None),
    };
    Some((out.or(base_out), root.or(base_root)))
}

fn npm_providers(li: &LabelInput, out: &mut Vec<Provider>) {
    for (path, text) in &li.manifests {
        if file_name(path) != eco("npm").manifest || path.contains("node_modules/") {
            continue;
        }
        let Some(pkg) = parse_json(text) else {
            continue;
        };
        let Some(name) = json_str(&pkg, "/name") else {
            continue;
        };
        let dir = parent_dir(path).to_string();
        let exports = pkg.get("exports");
        let mut entry = exports
            .and_then(|e| exports_target(e, "."))
            .and_then(|p| map_entry(li, &dir, &p, &pkg));
        // `exports` is authoritative: a package that has it is never entered through `main` & co.
        if entry.is_none() && exports.is_none() {
            for ptr in &eco("npm").entries {
                if let Some(p) = json_str(&pkg, &format!("/{ptr}")) {
                    entry = map_entry(li, &dir, &p, &pkg);
                    if entry.is_some() {
                        break;
                    }
                }
            }
        }
        if entry.is_none()
            && exports.is_none()
            && eco("npm").entries.iter().all(|p| pkg.get(p).is_none())
        {
            // npm's default `main` is index.js: that file, never a guess beyond it.
            entry = join(&dir, "index.js").filter(|p| li.files.contains(p));
        }
        let mut subpaths = BTreeMap::new();
        if let Some(serde_json::Value::Object(m)) = exports {
            for k in m.keys().filter(|k| k.starts_with("./") && !k.contains('*')) {
                if let Some(f) =
                    exports_target(exports.unwrap(), k).and_then(|p| map_entry(li, &dir, &p, &pkg))
                {
                    subpaths.insert(k.clone(), f);
                }
            }
        }
        out.push(Provider {
            ecosystem: "npm",
            import_names: vec![name.clone()],
            name,
            label: li.label.clone(),
            dir,
            version: json_str(&pkg, "/version"),
            entry,
            subpaths,
            py_root: None,
        });
    }
}

fn toml_str(v: &toml::Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_str().map(str::to_string)
}

fn cargo_providers(li: &LabelInput, out: &mut Vec<Provider>) {
    for (path, text) in &li.manifests {
        if file_name(path) != eco("cargo").manifest {
            continue;
        }
        let Some(v) = parse_toml(text) else { continue };
        let Some(name) = toml_str(&v, &["package", "name"]) else {
            continue;
        };
        let dir = parent_dir(path).to_string();
        let lib_name = toml_str(&v, &["lib", "name"]).unwrap_or_else(|| name.replace('-', "_"));
        let lib_path = toml_str(&v, &["lib", "path"]).unwrap_or_else(|| "src/lib.rs".into());
        let entry = join(&dir, &lib_path).filter(|p| li.files.contains(p));
        out.push(Provider {
            ecosystem: "cargo",
            import_names: vec![lib_name],
            name,
            label: li.label.clone(),
            dir,
            version: toml_str(&v, &["package", "version"]),
            entry,
            subpaths: BTreeMap::new(),
            py_root: None,
        });
    }
}

/// PEP 503 name normalization.
fn py_norm(name: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            sep = true;
        } else {
            if sep && !out.is_empty() {
                out.push('-');
            }
            sep = false;
            out.extend(c.to_lowercase());
        }
    }
    out
}

fn python_providers(li: &LabelInput, out: &mut Vec<Provider>) {
    for (path, text) in &li.manifests {
        if file_name(path) != eco("python").manifest {
            continue;
        }
        let Some(v) = parse_toml(text) else { continue };
        let name = toml_str(&v, &["project", "name"])
            .or_else(|| toml_str(&v, &["tool", "poetry", "name"]));
        let Some(name) = name else { continue };
        let dir = parent_dir(path).to_string();
        // Package root: `src/` when it holds packages, else the manifest dir.
        let src = join(&dir, "src").unwrap_or_default();
        let has = |root: &str| {
            let p = if root.is_empty() {
                String::new()
            } else {
                format!("{root}/")
            };
            li.files.iter().any(|f| {
                f.starts_with(&p)
                    && f.ends_with("/__init__.py")
                    && f[p.len()..].matches('/').count() == 1
            })
        };
        let root = if has(&src) { src } else { dir.clone() };
        let p = if root.is_empty() {
            String::new()
        } else {
            format!("{root}/")
        };
        let mut import_names: BTreeSet<String> = li
            .files
            .iter()
            .filter(|f| {
                f.starts_with(&p)
                    && f.ends_with("/__init__.py")
                    && f[p.len()..].matches('/').count() == 1
            })
            .map(|f| {
                f[p.len()..]
                    .split('/')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        // Declared packages (`[tool.poetry] packages = [{ include = "x" }]`, setuptools lists).
        for tool in ["poetry", "setuptools", "hatch", "pdm"] {
            if let Some(arr) = v
                .get("tool")
                .and_then(|t| t.get(tool))
                .and_then(|t| t.get("packages"))
                .and_then(|p| p.as_array())
            {
                for it in arr {
                    if let Some(s) = it
                        .as_str()
                        .or_else(|| it.get("include").and_then(|i| i.as_str()))
                    {
                        import_names.insert(s.split(['/', '.']).next().unwrap_or(s).to_string());
                    }
                }
            }
        }
        if import_names.is_empty() {
            import_names.insert(py_norm(&name).replace('-', "_"));
        }
        out.push(Provider {
            ecosystem: "python",
            name: py_norm(&name),
            import_names: import_names.into_iter().collect(),
            label: li.label.clone(),
            dir,
            version: toml_str(&v, &["project", "version"]),
            entry: None,
            subpaths: BTreeMap::new(),
            py_root: Some(root),
        });
    }
}

/// Python entry for `module` (dotted) under a provider's package root: the most specific indexed
/// module file, walking up to the top-level package's `__init__.py`.
fn py_entry(li: &LabelInput, root: &str, module: &str) -> Option<String> {
    let parts: Vec<&str> = module.split('.').collect();
    for n in (1..=parts.len()).rev() {
        let rel = parts[..n].join("/");
        for cand in [format!("{rel}.py"), format!("{rel}/__init__.py")] {
            let p = join(root, &cand)?;
            if li.files.contains(&p) {
                return Some(p);
            }
        }
    }
    None
}

// ── Consumers ─────────────────────────────────────────────────────────────────────────────────

/// A boundary's grouping key: reason, ecosystem, consumer label, package.
type BoundaryKey = (BoundaryReason, String, String, String);
/// A boundary's files, candidates and why-lines.
type BoundaryAcc = (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>);

struct Ctx<'a> {
    inputs: &'a [LabelInput],
    providers: Vec<Provider>,
}

impl<'a> Ctx<'a> {
    fn label(&self, l: &str) -> Option<&'a LabelInput> {
        self.inputs.iter().find(|i| i.label == l)
    }

    /// The store path for an absolute filesystem path: the member label whose recorded root
    /// contains it.
    fn store_path_for_abs(&self, abs: &str) -> Option<String> {
        let mut best: Option<(&LabelInput, usize)> = None;
        for li in self.inputs {
            let Some(root) = li.root.as_deref() else {
                continue;
            };
            let root = norm_abs(root);
            if (abs == root || abs.starts_with(&format!("{root}/")))
                && best.is_none_or(|(_, n)| root.len() > n)
            {
                best = Some((li, root.len()));
            }
        }
        let (li, n) = best?;
        let rest = abs[n..].trim_start_matches('/');
        let p = prefix(&li.label);
        Some(if rest.is_empty() {
            p.trim_end_matches('/').to_string()
        } else {
            format!("{p}{rest}")
        })
    }

    /// Resolve a relative path spec written in `manifest_dir` (a store path of `label`).
    fn resolve_path_spec(&self, label: &str, manifest_dir: &str, rel: &str) -> Option<String> {
        // Inside the label: plain join, unless it climbs above the label prefix.
        let p = prefix(label);
        if let Some(j) = join(manifest_dir, rel) {
            if p.is_empty() || j.starts_with(&p) || j == label {
                return Some(j);
            }
        }
        // Across labels: through the recorded roots.
        let li = self.label(label)?;
        let root = norm_abs(li.root.as_deref()?);
        let local = label_local(label, manifest_dir);
        let abs_dir = if local.is_empty() {
            root.clone()
        } else {
            format!("{root}/{local}")
        };
        // `join` drops the leading `/` of a POSIX root; a drive root (`C:/...`) has none.
        let lead = if root.starts_with('/') { "/" } else { "" };
        let abs = join(&abs_dir, rel).map(|s| format!("{lead}{s}"))?;
        self.store_path_for_abs(&abs)
    }

    fn providers_named(&self, eco: &str, name: &str) -> Vec<&Provider> {
        self.providers
            .iter()
            .filter(|p| p.ecosystem == eco && p.name == name)
            .collect()
    }

    fn provider_at(&self, eco: &str, dir: &str) -> Option<&Provider> {
        self.providers
            .iter()
            .find(|p| p.ecosystem == eco && p.dir == dir)
    }

    fn remote_of(&self, label: &str) -> Option<String> {
        self.label(label)?.remote.as_deref().and_then(repo_identity)
    }

    /// The provider's manifest dir relative to its git work tree (for `repository.directory`):
    /// the label's recorded subpath joined with the label-local dir.
    fn repo_dir(&self, p: &Provider) -> String {
        let local = label_local(&p.label, &p.dir).trim_matches('/');
        let sub = self
            .label(&p.label)
            .and_then(|l| l.subpath.as_deref())
            .unwrap_or("")
            .trim_matches('/');
        match (sub.is_empty(), local.is_empty()) {
            (true, _) => local.to_string(),
            (false, true) => sub.to_string(),
            (false, false) => format!("{sub}/{local}"),
        }
    }
}

/// The nearest manifest named `name` at or above `file`'s directory, inside `label`.
fn nearest_manifest<'a>(li: &'a LabelInput, file: &str, name: &str) -> Option<(&'a str, &'a str)> {
    let mut dir = parent_dir(file).to_string();
    let floor = prefix(&li.label);
    loop {
        let cand = if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        };
        if let Some((p, t)) = li.manifests.get_key_value(&cand) {
            return Some((p.as_str(), t.as_str()));
        }
        if dir.is_empty() || format!("{dir}/") == floor {
            return None;
        }
        dir = parent_dir(&dir).to_string();
    }
}

fn npm_declared(manifest: &serde_json::Value) -> HashMap<String, (String, String)> {
    // import name -> (real package, spec)
    let e = eco("npm");
    let mut out = HashMap::new();
    for t in &e.dep_tables {
        if let Some(m) = manifest.get(t).and_then(|x| x.as_object()) {
            for (k, v) in m {
                let spec = v.as_str().unwrap_or_default().to_string();
                let real = match e
                    .alias_protocol
                    .as_deref()
                    .and_then(|a| spec.strip_prefix(a))
                {
                    Some(rest) => {
                        // npm:real@range  or  npm:@scope/real@range. `@` is ASCII, so every
                        // offset below is a char boundary.
                        let from = usize::from(rest.starts_with('@'));
                        let real = match rest[from..].find('@') {
                            Some(i) => &rest[..from + i],
                            None => rest,
                        };
                        if real.len() <= from {
                            continue; // a malformed alias (`npm:`, `npm:@`) declares nothing
                        }
                        real.to_string()
                    }
                    None => k.clone(),
                };
                out.entry(k.clone()).or_insert((real, spec));
            }
        }
    }
    out
}

/// The specifier root of a bare npm specifier (`@scope/pkg/sub` -> `@scope/pkg`) and its subpath.
fn npm_root(spec: &str) -> Option<(String, String)> {
    let e = eco("npm");
    if spec.is_empty()
        || spec.starts_with('.')
        || spec.starts_with('/')
        || e.builtin_prefixes
            .iter()
            .any(|p| spec.starts_with(p.as_str()))
    {
        return None;
    }
    let mut segs = spec.splitn(3, '/');
    let first = segs.next()?;
    if first.starts_with('@') {
        let second = segs.next()?;
        let rest = segs
            .next()
            .map(|r| format!("./{r}"))
            .unwrap_or_else(|| ".".into());
        Some((format!("{first}/{second}"), rest))
    } else {
        let rest: Vec<&str> = spec.splitn(2, '/').collect();
        Some((
            first.to_string(),
            rest.get(1)
                .map(|r| format!("./{r}"))
                .unwrap_or_else(|| ".".into()),
        ))
    }
}

struct Lock {
    version: Option<String>,
    resolved: Option<String>,
    integrity: Option<String>,
    link: bool,
}

/// The lockfile entry for `pkg` as installed for `manifest_dir`: the nearest package-lock.json at
/// or above it, key `<rel>/node_modules/<pkg>` first, then the hoisted `node_modules/<pkg>`.
fn lock_entry(li: &LabelInput, manifest_dir: &str, pkg: &str) -> Option<(Lock, String)> {
    let probe = if manifest_dir.is_empty() {
        "x".to_string()
    } else {
        format!("{manifest_dir}/x")
    };
    let (lock_path, text) = nearest_manifest(li, &probe, eco("npm").lockfile.as_deref()?)?;
    let v = parse_json(text)?;
    let lock_dir = parent_dir(lock_path);
    let rel = manifest_dir
        .strip_prefix(lock_dir)
        .unwrap_or(manifest_dir)
        .trim_matches('/');
    let keys = [
        if rel.is_empty() {
            format!("node_modules/{pkg}")
        } else {
            format!("{rel}/node_modules/{pkg}")
        },
        format!("node_modules/{pkg}"),
    ];
    let packages = v.get("packages")?;
    for k in keys {
        if let Some(e) = packages.get(&k) {
            return Some((
                Lock {
                    version: e
                        .get("version")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                    resolved: e
                        .get("resolved")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                    integrity: e
                        .get("integrity")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                    link: e.get("link").and_then(|x| x.as_bool()).unwrap_or(false),
                },
                lock_dir.to_string(),
            ));
        }
    }
    None
}

enum Decision {
    Link(String, Identity), // provider key, identity
    /// reason, candidates, why (one line a person can act on)
    Boundary(BoundaryReason, Vec<String>, &'static str),
}

/// The workspace that contains `manifest_dir` inside its label: the nearest ancestor directory whose
/// `package.json` declares `workspaces` (an array, or `{ "packages": [...] }`) or that holds a
/// `pnpm-workspace.yaml`, with its member patterns.
fn workspace_of(li: &LabelInput, manifest_dir: &str) -> Option<(String, Vec<String>)> {
    let mut dir = manifest_dir.to_string();
    loop {
        let at = |name: &str| {
            if dir.is_empty() {
                name.to_string()
            } else {
                format!("{dir}/{name}")
            }
        };
        if let Some(v) = li
            .manifests
            .get(&at("package.json"))
            .and_then(|t| parse_json(t))
        {
            let ws = v.get("workspaces");
            let arr = ws.and_then(|w| w.as_array()).or_else(|| {
                ws.and_then(|w| w.get("packages"))
                    .and_then(|p| p.as_array())
            });
            if let Some(arr) = arr {
                let pats = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect();
                return Some((dir, pats));
            }
        }
        if let Some(t) = li.manifests.get(&at("pnpm-workspace.yaml")) {
            return Some((dir.clone(), pnpm_packages(t)));
        }
        if dir.is_empty() || dir == li.label {
            return None;
        }
        dir = parent_dir(&dir).to_string();
    }
}

/// The `packages:` list of a pnpm-workspace.yaml (block list of plain or quoted strings).
fn pnpm_packages(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim_end();
        if !t.starts_with([' ', '\t', '-']) {
            inside = t.trim() == "packages:";
            continue;
        }
        if let Some(item) = inside.then(|| t.trim().strip_prefix('-')).flatten() {
            let v = dequote(item.trim());
            if !v.is_empty() {
                out.push(v.to_string());
            }
        }
    }
    out
}

/// A workspace member pattern (`packages/*`, `apps/**`, `libs/x`; a leading `!` excludes) matched
/// against a directory relative to the workspace root.
fn workspace_member(pats: &[String], rel: &str) -> bool {
    fn glob(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => glob(&p[1..], s) || (!s.is_empty() && glob(p, &s[1..])),
            (Some(pp), Some(ss)) => {
                let ok = *pp == "*"
                    || pp == ss
                    || pp.split_once('*').is_some_and(|(pre, post)| {
                        ss.len() >= pre.len() + post.len()
                            && ss.starts_with(pre)
                            && ss.ends_with(post)
                    });
                ok && glob(&p[1..], &s[1..])
            }
            _ => false,
        }
    }
    let segs: Vec<&str> = rel.split('/').filter(|x| !x.is_empty()).collect();
    let hit = |p: &str| {
        let mut pp: Vec<&str> = p
            .trim_start_matches("./")
            .split('/')
            .filter(|x| !x.is_empty())
            .collect();
        // `**/**` is `**`; more than 3 `**` (or a deep pattern) is not a workspace layout and
        // would make the backtracking matcher exponential: such a pattern matches nothing.
        pp.dedup_by(|a, b| *a == "**" && *b == "**");
        if pp.iter().filter(|s| **s == "**").count() > 3 || pp.len() > 32 || segs.len() > 64 {
            return false;
        }
        glob(&pp, &segs)
    };
    pats.iter().any(|p| !p.starts_with('!') && hit(p.as_str()))
        && !pats.iter().any(|p| p.strip_prefix('!').is_some_and(hit))
}

/// A lockfile `resolved` that names a registry tarball (not git, not a local path).
fn registry_resolved(r: &str) -> bool {
    (r.starts_with("https://") || r.starts_with("http://")) && r.ends_with(".tgz")
}

/// `pkg` is the real package name (candidates); `key` the declared name the lockfile and the
/// installed `node_modules/<key>` folder use (they differ under an `npm:` alias).
fn decide_npm(
    ctx: &Ctx,
    li: &LabelInput,
    manifest_dir: &str,
    pkg: &str,
    key: &str,
    spec: &str,
) -> Decision {
    let e = eco("npm");
    let cands: Vec<&Provider> = ctx.providers_named("npm", pkg);
    let keys = |v: &[&Provider]| v.iter().map(|p| p.key()).collect::<Vec<_>>();
    let lock = lock_entry(li, manifest_dir, key);
    // The lockfile's local link for the dependency, as a store path (npm workspaces, `file:`).
    let lock_link: Option<Option<String>> = lock.as_ref().filter(|(l, _)| l.link).map(|(l, d)| {
        l.resolved
            .as_deref()
            .and_then(|r| ctx.resolve_path_spec(&li.label, d, r))
    });
    // 1. path / workspace
    if let Some(rel) = e
        .path_protocols
        .iter()
        .find_map(|p| spec.strip_prefix(p.as_str()))
    {
        if spec.starts_with("workspace:") && !rel.starts_with('.') {
            // A workspace dependency is a MEMBER of the consumer's workspace with that name.
            let Some((ws_dir, pats)) = workspace_of(li, manifest_dir) else {
                return Decision::Boundary(
                    BoundaryReason::ProviderUnverified,
                    keys(&cands),
                    "workspace: no workspace root declares the consumer's workspace",
                );
            };
            let members: Vec<&Provider> = cands
                .iter()
                .copied()
                .filter(|p| p.label == li.label)
                .filter(|p| {
                    let pre = if ws_dir.is_empty() {
                        String::new()
                    } else {
                        format!("{ws_dir}/")
                    };
                    p.dir
                        .strip_prefix(&pre)
                        .is_some_and(|r| workspace_member(&pats, r))
                })
                .collect();
            // `workspace:1.2.3` pins an exact version; a range (`*`, `^`, `~`) is satisfied by the
            // member pnpm/yarn would install, which is the member itself.
            let exact = rel
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.')
                .then_some(rel)
                .filter(|v| !v.is_empty());
            return match members.as_slice() {
                [p] if exact.is_some_and(|v| p.version.as_deref() != Some(v)) => {
                    Decision::Boundary(
                        BoundaryReason::ProviderContradicted,
                        keys(&members),
                        "workspace: the pinned version is not the member's version",
                    )
                }
                [p] => match &lock_link {
                    Some(Some(d)) if *d != p.dir => Decision::Boundary(
                        BoundaryReason::ProviderContradicted,
                        keys(&members),
                        "workspace: the lockfile links a different directory",
                    ),
                    _ => Decision::Link(p.key(), Identity::Path),
                },
                [] => Decision::Boundary(
                    BoundaryReason::ProviderUnverified,
                    keys(&cands),
                    "workspace: no workspace member of that name",
                ),
                _ => Decision::Boundary(
                    BoundaryReason::ProviderAmbiguous,
                    keys(&members),
                    "workspace: several workspace members of that name",
                ),
            };
        }
        let at = ctx.resolve_path_spec(&li.label, manifest_dir, rel);
        if lock.as_ref().is_some_and(|(l, _)| !l.link) {
            return Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                keys(&cands),
                "path: the lockfile installs this dependency from elsewhere (not a local link)",
            );
        }
        if matches!((&lock_link, &at), (Some(Some(l)), Some(a)) if l != a) {
            return Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                keys(&cands),
                "path: the lockfile links a different directory than the spec",
            );
        }
        return match at.and_then(|d| ctx.provider_at("npm", &d)) {
            Some(p) if p.name == pkg => Decision::Link(p.key(), Identity::Path),
            Some(p) => Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                vec![p.key()],
                "path: the path holds a different package",
            ),
            None => Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                keys(&cands),
                "path: no member package at the path",
            ),
        };
    }
    let spec_is_git = e.git_protocols.iter().any(|g| spec.starts_with(g.as_str()));
    // npm workspaces with a plain range: the lock records a linked local package. (A git spec
    // with a local lock link is contradictory evidence, decided below.)
    if let Some(l) = lock_link.as_ref().filter(|_| !spec_is_git) {
        return match l.as_deref().and_then(|d| ctx.provider_at("npm", d)) {
            Some(p) if p.name == pkg => Decision::Link(p.key(), Identity::Path),
            Some(p) => Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                vec![p.key()],
                "path: the lockfile links a different package",
            ),
            None => Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                keys(&cands),
                "path: the lockfile links a directory with no member package",
            ),
        };
    }
    // 2. git source (spec and/or lockfile resolved; both present must agree)
    if spec_is_git && lock_link.is_some() {
        return Decision::Boundary(
            BoundaryReason::ProviderContradicted,
            keys(&cands),
            "git: the lockfile links a local directory instead",
        );
    }
    let spec_git = spec_is_git.then(|| spec.to_string());
    let lock_git = lock
        .as_ref()
        .and_then(|(l, _)| l.resolved.clone())
        .filter(|r| r.starts_with("git") || r.contains(".git"));
    if spec_git.is_some() || lock_git.is_some() {
        let ids: BTreeSet<Option<String>> = [spec_git.as_deref(), lock_git.as_deref()]
            .into_iter()
            .flatten()
            .map(repo_identity)
            .collect();
        if ids.len() > 1 {
            return Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                keys(&cands),
                "git: the spec and the lockfile name different repositories",
            );
        }
        let Some(Some(id)) = ids.into_iter().next() else {
            return Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                keys(&cands),
                "git: the URL is not a repository URL",
            );
        };
        // npm installs a git dependency from the repository's top-level package.
        let hits: Vec<&Provider> = cands
            .iter()
            .copied()
            .filter(|p| ctx.remote_of(&p.label).as_deref() == Some(id.as_str()))
            .filter(|p| ctx.repo_dir(p).is_empty())
            .collect();
        return match hits.len() {
            1 => Decision::Link(hits[0].key(), Identity::GitSource),
            0 if !cands.is_empty() => Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                keys(&cands),
                "git: no candidate is the top-level package of that repository",
            ),
            0 => Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                vec![],
                "git: no member provides that name",
            ),
            _ => Decision::Boundary(
                BoundaryReason::ProviderAmbiguous,
                keys(&hits),
                "git: several members have that remote",
            ),
        };
    }
    // 3. registry source: the installed manifest names the provider's repository, bound to the lock.
    let Some((l, _)) = lock else {
        return Decision::Boundary(
            BoundaryReason::ProviderUnverified,
            keys(&cands),
            "registry: no lockfile entry for the dependency",
        );
    };
    if !l.resolved.as_deref().is_some_and(registry_resolved) {
        return Decision::Boundary(
            BoundaryReason::ProviderUnverified,
            keys(&cands),
            "registry: the lockfile entry does not resolve to a registry tarball",
        );
    }
    let Some(art) = li
        .artifacts
        .iter()
        .find(|a| a.manifest_dir == manifest_dir && a.package == key)
    else {
        return Decision::Boundary(
            BoundaryReason::ProviderUnverified,
            keys(&cands),
            "registry: no installed manifest recorded at index time",
        );
    };
    if art.name.as_deref() != Some(pkg) {
        return Decision::Boundary(
            BoundaryReason::ProviderUnverified,
            keys(&cands),
            "registry: the installed manifest has a different package name",
        );
    }
    if art.version.is_none() || art.version != l.version {
        return Decision::Boundary(
            BoundaryReason::ProviderUnverified,
            keys(&cands),
            "registry: installed version differs from the lockfile",
        );
    }
    let identity = if art.tree_record {
        let bound = art.tree_version == l.version
            && art.tree_resolved == l.resolved
            && art.tree_integrity == l.integrity
            && art.tree_version.is_some()
            && art.tree_resolved.is_some()
            && art.tree_integrity.is_some();
        if !bound {
            // A stale install: the installed copy is not the one the lockfile resolves.
            return Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                keys(&cands),
                "registry: the installed copy is not the one the lockfile resolves (stale install)",
            );
        }
        Identity::RegistrySource
    } else {
        Identity::RegistrySourceVersionOnly
    };
    let Some(art_id) = art.repository.as_deref().and_then(repo_identity) else {
        return Decision::Boundary(
            BoundaryReason::ProviderUnverified,
            keys(&cands),
            "registry: the installed manifest names no usable repository",
        );
    };
    let art_dir = art
        .directory
        .as_deref()
        .unwrap_or("")
        .trim_matches('/')
        .to_string();
    // No `repository.directory`: the name is unique within the repository (two same-named
    // packages in one repository are ambiguous below), so the repository alone selects it.
    let hits: Vec<&Provider> = cands
        .iter()
        .copied()
        .filter(|p| ctx.remote_of(&p.label).as_deref() == Some(art_id.as_str()))
        .filter(|p| art_dir.is_empty() || ctx.repo_dir(p) == art_dir)
        .collect();
    match hits.len() {
        1 => Decision::Link(hits[0].key(), identity),
        0 => Decision::Boundary(
            BoundaryReason::ProviderUnverified,
            keys(&cands),
            "registry: the installed manifest's repository matches no member",
        ),
        _ => Decision::Boundary(
            BoundaryReason::ProviderAmbiguous,
            keys(&hits),
            "registry: the installed manifest's repository matches several members",
        ),
    }
}

/// Cargo dependency table entries: import name (`-`->`_`) -> (real package, spec). A key declared
/// again with a DIFFERENT source (unix and windows target tables naming two paths) is returned in
/// the conflict set: which one a file compiles against is not decided statically.
type CargoDeclared = HashMap<String, (String, toml::Value)>;

fn cargo_declared(v: &toml::Value, ws: Option<&toml::Value>) -> (CargoDeclared, BTreeSet<String>) {
    let mut out: CargoDeclared = HashMap::new();
    let mut conflicts = BTreeSet::new();
    let mut tables: Vec<&toml::Value> = Vec::new();
    for t in &eco("cargo").dep_tables {
        if let Some(x) = v.get(t) {
            tables.push(x);
        }
    }
    if let Some(targets) = v.get("target").and_then(|t| t.as_table()) {
        for (_, tv) in targets {
            for t in &eco("cargo").dep_tables {
                if let Some(x) = tv.get(t) {
                    tables.push(x);
                }
            }
        }
    }
    for t in tables {
        let Some(m) = t.as_table() else { continue };
        for (k, spec) in m {
            let mut spec = spec.clone();
            // `x = { workspace = true }` -> the workspace's declaration.
            if spec.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
                if let Some(wsdep) = ws
                    .and_then(|w| w.get("workspace"))
                    .and_then(|w| w.get("dependencies"))
                    .and_then(|d| d.get(k))
                {
                    // Keep the marker: an inherited `path` is relative to the workspace root.
                    spec = match wsdep {
                        toml::Value::Table(t) => {
                            let mut t = t.clone();
                            t.insert("workspace".into(), toml::Value::Boolean(true));
                            toml::Value::Table(t)
                        }
                        other => other.clone(),
                    };
                }
            }
            let real = spec
                .get("package")
                .and_then(|p| p.as_str())
                .unwrap_or(k)
                .to_string();
            let key = k.replace('-', "_");
            match out.get(&key) {
                Some((r, sp)) if *r != real || source_of(sp) != source_of(&spec) => {
                    conflicts.insert(key);
                }
                Some(_) => {}
                None => {
                    out.insert(key, (real, spec));
                }
            }
        }
    }
    (out, conflicts)
}

/// The source part of a cargo dependency spec (path / git / registry), for conflict detection.
fn source_of(spec: &toml::Value) -> (Option<&str>, Option<&str>) {
    (
        spec.get("path").and_then(|p| p.as_str()),
        spec.get("git").and_then(|g| g.as_str()),
    )
}

/// The `[patch.<source>]` / `[replace]` entry that applies to a dependency on `pkg` declared as
/// `spec`, from the manifest holding the patches (the workspace root, or the package itself). Only
/// the dependency's own source is patched: `[patch.crates-io]` for a registry version,
/// `[patch."<url>"]` for a git dependency; a `path` dependency is never patched.
fn cargo_patch_for<'v>(
    patches: Option<&'v toml::Value>,
    pkg: &str,
    spec: &toml::Value,
) -> Option<&'v toml::Value> {
    let ws = patches?;
    if spec.get("path").is_some() {
        return None;
    }
    let git = spec.get("git").and_then(|g| g.as_str());
    let names = |k: &str, v: &toml::Value| {
        k == pkg || v.get("package").and_then(|p| p.as_str()) == Some(pkg)
    };
    let patched = ws
        .get("patch")
        .and_then(|p| p.get(git.unwrap_or("crates-io")))
        .and_then(|t| t.as_table())
        .and_then(|t| t.iter().find(|(k, v)| names(k, v)).map(|(_, v)| v));
    let replaced = || {
        ws.get("replace").and_then(|r| r.as_table()).and_then(|t| {
            t.iter()
                .find(|(k, _)| *k == pkg || k.starts_with(&format!("{pkg}:")))
                .map(|(_, v)| v)
        })
    };
    patched.or_else(|| if git.is_none() { replaced() } else { None })
}

/// npm `overrides` / yarn `resolutions` in the consumer's manifest or its workspace root that name
/// `pkg` with an alias or a local path: the install is not the declared package (`dep:override`).
/// A plain version override keeps the package's identity and is ignored.
fn npm_overridden(li: &LabelInput, manifest_dir: &str, pkg: &str) -> bool {
    let mut roots = vec![manifest_dir.to_string()];
    if let Some((ws, _)) = workspace_of(li, manifest_dir) {
        roots.push(ws);
    }
    roots.iter().any(|dir| {
        let path = if dir.is_empty() {
            "package.json".to_string()
        } else {
            format!("{dir}/package.json")
        };
        let Some(v) = li.manifests.get(&path).and_then(|t| parse_json(t)) else {
            return false;
        };
        ["overrides", "resolutions"].iter().any(|t| {
            v.get(*t).and_then(|o| o.as_object()).is_some_and(|m| {
                m.iter().any(|(k, val)| {
                    // `pkg`, `pkg@range`, `**/pkg`, `parent/pkg` (`@` is ASCII: safe offsets).
                    let name = match k.get(1..).and_then(|r| r.find('@')) {
                        Some(i) => &k[..i + 1],
                        None => k.as_str(),
                    };
                    let names_pkg = name == pkg || name.ends_with(&format!("/{pkg}"));
                    // `"pkg": "npm:x"` or the object form `"pkg": { ".": "npm:x" }`.
                    let s = val
                        .as_str()
                        .or_else(|| val.get(".").and_then(|d| d.as_str()))
                        .unwrap_or_default();
                    names_pkg
                        && (s.starts_with("npm:")
                            || s.starts_with("file:")
                            || s.starts_with("link:")
                            || s.starts_with("git")
                            || s.starts_with("github:"))
                })
            })
        })
    })
}

fn decide_cargo(
    ctx: &Ctx,
    li: &LabelInput,
    manifest_dir: &str,
    ws_dir: Option<&str>,
    pkg: &str,
    spec: &toml::Value,
) -> Decision {
    let cands: Vec<&Provider> = ctx.providers_named("cargo", pkg);
    let keys = |v: &[&Provider]| v.iter().map(|p| p.key()).collect::<Vec<_>>();
    if let Some(rel) = spec.get("path").and_then(|p| p.as_str()) {
        // A workspace-inherited path is relative to the workspace root.
        let base = if spec.get("workspace").is_none() {
            manifest_dir
        } else {
            ws_dir.unwrap_or(manifest_dir)
        };
        return match ctx
            .resolve_path_spec(&li.label, base, rel)
            .and_then(|d| ctx.provider_at("cargo", &d))
        {
            Some(p) if p.name == pkg => Decision::Link(p.key(), Identity::Path),
            Some(p) => Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                vec![p.key()],
                "path: the path holds a different crate",
            ),
            None => Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                keys(&cands),
                "path: no member crate at the path",
            ),
        };
    }
    if let Some(url) = spec.get("git").and_then(|g| g.as_str()) {
        let Some(id) = repo_identity(url) else {
            return Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                keys(&cands),
                "git: the URL is not a repository URL",
            );
        };
        let hits: Vec<&Provider> = cands
            .iter()
            .copied()
            .filter(|p| ctx.remote_of(&p.label).as_deref() == Some(id.as_str()))
            .collect();
        return match hits.len() {
            1 => Decision::Link(hits[0].key(), Identity::GitSource),
            0 if !cands.is_empty() => Decision::Boundary(
                BoundaryReason::ProviderContradicted,
                keys(&cands),
                "git: the URL names a repository no candidate member has",
            ),
            0 => Decision::Boundary(
                BoundaryReason::ProviderUnverified,
                vec![],
                "git: no member provides that crate",
            ),
            _ => Decision::Boundary(
                BoundaryReason::ProviderAmbiguous,
                keys(&hits),
                "git: several members have that remote",
            ),
        };
    }
    // A registry version: no installed-manifest equivalent in X1 -> unverified.
    Decision::Boundary(
        BoundaryReason::ProviderUnverified,
        keys(&cands),
        "registry: a crates.io version has no installed-manifest proof in this slice",
    )
}

/// Python: declared dist names (normalized) -> path source (store path) when declared by path.
fn py_declared(
    ctx: &Ctx,
    li: &LabelInput,
    manifest_dir: &str,
    v: &toml::Value,
) -> HashMap<String, Option<String>> {
    let mut out: HashMap<String, Option<String>> = HashMap::new();
    let pep508 = |s: &str| {
        let n: String = s
            .trim()
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        py_norm(&n)
    };
    if let Some(arr) = v
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
    {
        for d in arr.iter().filter_map(|x| x.as_str()) {
            out.entry(pep508(d)).or_insert(None);
        }
    }
    if let Some(opt) = v
        .get("project")
        .and_then(|p| p.get("optional-dependencies"))
        .and_then(|o| o.as_table())
    {
        for (_, arr) in opt {
            for d in arr
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str())
            {
                out.entry(pep508(d)).or_insert(None);
            }
        }
    }
    // `[tool.poetry.dependencies]` DECLARES (and may give a path); `[tool.uv.sources]` only says
    // where an already-declared dependency comes from, so it never declares one by itself (D5).
    let path_of = |spec: &toml::Value| {
        spec.get("path")
            .and_then(|p| p.as_str())
            .and_then(|rel| ctx.resolve_path_spec(&li.label, manifest_dir, rel))
    };
    if let Some(m) = v
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        for (k, spec) in m {
            let path = path_of(spec);
            let e = out.entry(py_norm(k)).or_insert(None);
            if path.is_some() {
                *e = path;
            }
        }
    }
    if let Some(m) = v
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.get("sources"))
        .and_then(|s| s.as_table())
    {
        for (k, spec) in m {
            if let (Some(e), Some(path)) = (out.get_mut(&py_norm(k)), path_of(spec)) {
                *e = Some(path);
            }
        }
    }
    out
}

// ── The linker ────────────────────────────────────────────────────────────────────────────────

/// Compute the project's package-import links over `inputs` (the member labels; nothing else in
/// the database is read or linked to).
pub fn link(inputs: &[LabelInput]) -> LinkOutput {
    let mut providers = Vec::new();
    for li in inputs {
        npm_providers(li, &mut providers);
        cargo_providers(li, &mut providers);
        python_providers(li, &mut providers);
    }
    let ctx = Ctx { inputs, providers };
    let mut edges: Vec<LinkEdge> = Vec::new();
    let mut bounds: BTreeMap<BoundaryKey, BoundaryAcc> = BTreeMap::new();
    let mut boundary = |r: BoundaryReason,
                        eco: &str,
                        label: &str,
                        pkg: &str,
                        file: &str,
                        cands: Vec<String>,
                        why: &str| {
        let e = bounds
            .entry((r, eco.to_string(), label.to_string(), pkg.to_string()))
            .or_default();
        e.0.insert(file.to_string());
        e.1.extend(cands);
        e.2.insert(why.to_string());
    };

    for li in inputs {
        for imp in &li.imports {
            let ext = imp.file.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            let raw = dequote(&imp.raw);
            if eco("npm").file_ext.iter().any(|e| e == ext) {
                let Some((root, sub)) = npm_root(raw) else {
                    continue;
                };
                let cands = ctx.providers_named("npm", &root);
                let Some(man) = nearest_manifest(li, &imp.file, &eco("npm").manifest)
                    .and_then(|(p, t)| parse_json(t).map(|v| (p, v)))
                else {
                    // No readable manifest declares anything for this file: a project package it
                    // imports is a candidate, never a link (D5).
                    if !cands.is_empty() {
                        boundary(
                            BoundaryReason::Undeclared,
                            "npm",
                            &li.label,
                            &root,
                            &imp.file,
                            cands.iter().map(|p| p.key()).collect(),
                            "no readable package.json at or above the importing file",
                        );
                    }
                    continue;
                };
                let (mpath, man) = man;
                let declared = npm_declared(&man);
                let mdir = parent_dir(mpath);
                let Some((real, spec)) = declared.get(&root) else {
                    // H2 / D5: an undeclared import of a project package is a candidate, never a link.
                    let other: Vec<String> = cands
                        .iter()
                        .filter(|p| p.label != li.label || p.dir != mdir)
                        .map(|p| p.key())
                        .collect();
                    if !other.is_empty() {
                        boundary(
                            BoundaryReason::Undeclared,
                            "npm",
                            &li.label,
                            &root,
                            &imp.file,
                            other,
                            "the importing package does not declare it",
                        );
                    }
                    continue;
                };
                let cands_real = ctx.providers_named("npm", real);
                if cands_real.is_empty() {
                    continue; // not a project package: an external dependency, not the linker's
                }
                let decision = if npm_overridden(li, mdir, real) {
                    Decision::Boundary(
                        BoundaryReason::Overridden,
                        cands_real.iter().map(|p| p.key()).collect(),
                        "overrides/resolutions replace the package with an alias, path or git source",
                    )
                } else {
                    decide_npm(&ctx, li, mdir, real, &root, spec)
                };
                match decision {
                    Decision::Link(key, identity) => {
                        let p = ctx
                            .providers
                            .iter()
                            .find(|p| p.key() == key)
                            .expect("decided provider");
                        let target = if sub == "." {
                            p.entry.clone()
                        } else {
                            p.subpaths.get(&sub).cloned()
                        };
                        if p.label == li.label && p.dir == mdir {
                            continue; // a package importing itself
                        }
                        let Some(to) = target else {
                            boundary(
                                BoundaryReason::EntryUnmapped,
                                "npm",
                                &li.label,
                                real,
                                &imp.file,
                                vec![key],
                                "the provider's entry is not an indexed file",
                            );
                            continue;
                        };
                        // Only an exact (lockfile) version is a consumer version; a spec or range
                        // is not, so a path or workspace link never claims skew.
                        let consumer_version =
                            lock_entry(li, mdir, &root).and_then(|(l, _)| l.version);
                        edges.push(edge(
                            &imp.file,
                            &to,
                            "npm",
                            real,
                            identity,
                            li,
                            p,
                            consumer_version,
                        ));
                    }
                    Decision::Boundary(r, c, why) => {
                        boundary(r, "npm", &li.label, real, &imp.file, c, why)
                    }
                }
            } else if eco("cargo").file_ext.iter().any(|e| e == ext) {
                let root = raw
                    .trim_start_matches("::")
                    .split("::")
                    .next()
                    .unwrap_or_default()
                    .trim();
                if root.is_empty() || eco("cargo").local_roots.iter().any(|l| l == root) {
                    continue;
                }
                let by_import: Vec<&Provider> = ctx
                    .providers
                    .iter()
                    .filter(|p| p.ecosystem == "cargo" && p.import_names.iter().any(|n| n == root))
                    .collect();
                let Some((mpath, mtext, man)) =
                    nearest_manifest(li, &imp.file, &eco("cargo").manifest)
                        .and_then(|(p, t)| parse_toml(t).map(|v| (p, t, v)))
                else {
                    if !by_import.is_empty() {
                        boundary(
                            BoundaryReason::Undeclared,
                            "cargo",
                            &li.label,
                            root,
                            &imp.file,
                            by_import.iter().map(|p| p.key()).collect(),
                            "no readable Cargo.toml at or above the importing file",
                        );
                    }
                    continue;
                };
                // The workspace root (nearest ancestor manifest with [workspace]).
                let mdir = parent_dir(mpath);
                let ws = nearest_manifest(li, mdir, &eco("cargo").manifest)
                    .filter(|(p, _)| *p != mpath)
                    .or_else(|| (man.get("workspace").is_some()).then_some((mpath, mtext)));
                let ws_v = ws.and_then(|(_, t)| parse_toml(t));
                let ws_dir = ws.map(|(p, _)| parent_dir(p));
                let (declared, conflicts) = cargo_declared(&man, ws_v.as_ref());
                // The declaration this import uses: a dependency key (or `package =` rename) equal
                // to the import root, or an unrenamed dependency whose crate has a custom
                // `[lib] name` equal to it.
                let found = declared
                    .get(root)
                    .map(|(r, sp)| (r, sp, root.to_string()))
                    .or_else(|| {
                        // Exactly one declared crate must have that lib name; two (a feature- or
                        // target-dependent choice) are not decided here.
                        let mut hits = by_import.iter().filter_map(|p| {
                            let k = p.name.replace('-', "_");
                            declared
                                .get(&k)
                                .filter(|(r, sp)| *r == p.name && sp.get("package").is_none())
                                .map(|(r, sp)| (r, sp, k))
                        });
                        let first = hits.next();
                        if hits.next().is_some() { None } else { first }
                    });
                let Some((real, spec, decl_key)) = found else {
                    let other: Vec<String> = by_import
                        .iter()
                        .filter(|p| p.dir != mdir)
                        .map(|p| p.key())
                        .collect();
                    if !other.is_empty() {
                        boundary(
                            BoundaryReason::Undeclared,
                            "cargo",
                            &li.label,
                            root,
                            &imp.file,
                            other,
                            "the importing crate does not declare it",
                        );
                    }
                    continue;
                };
                if ctx.providers_named("cargo", real).is_empty() {
                    continue;
                }
                let cand_keys = || {
                    ctx.providers_named("cargo", real)
                        .iter()
                        .map(|p| p.key())
                        .collect::<Vec<_>>()
                };
                let decision = if conflicts.contains(&decl_key) {
                    Decision::Boundary(
                        BoundaryReason::ProviderAmbiguous,
                        cand_keys(),
                        "target-specific declarations name different sources",
                    )
                } else if let Some(patch) =
                    cargo_patch_for(ws_v.as_ref().or(Some(&man)), real, spec)
                {
                    // The build uses the patch. A local `path` patch IS the dependency's source
                    // (tokio's `[patch.crates-io] tokio = { path = "tokio" }`): decided as a path,
                    // relative to the manifest that holds the patch. Anything else is not followed.
                    match patch.get("path").and_then(|p| p.as_str()) {
                        Some(path) => {
                            let mut t = toml::map::Map::new();
                            t.insert("path".into(), toml::Value::String(path.to_string()));
                            if ws_v.is_some() {
                                t.insert("workspace".into(), toml::Value::Boolean(true));
                            }
                            decide_cargo(&ctx, li, mdir, ws_dir, real, &toml::Value::Table(t))
                        }
                        None => Decision::Boundary(
                            BoundaryReason::Patched,
                            cand_keys(),
                            "[patch] or [replace] swaps this dependency's source",
                        ),
                    }
                } else {
                    decide_cargo(&ctx, li, mdir, ws_dir, real, spec)
                };
                match decision {
                    Decision::Link(key, identity) => {
                        let p = ctx
                            .providers
                            .iter()
                            .find(|p| p.key() == key)
                            .expect("decided provider");
                        if p.dir == mdir && p.label == li.label {
                            continue;
                        }
                        // An unrenamed dependency is imported by its crate's lib name only.
                        if spec.get("package").is_none()
                            && !p.import_names.iter().any(|n| n == root)
                        {
                            continue;
                        }
                        let Some(to) = p.entry.clone() else {
                            boundary(
                                BoundaryReason::EntryUnmapped,
                                "cargo",
                                &li.label,
                                real,
                                &imp.file,
                                vec![key],
                                "the crate's lib entry is not an indexed file",
                            );
                            continue;
                        };
                        // A requirement (`1.2`) is a range, not a version: no skew claim without
                        // the lockfile (not read in this slice).
                        edges.push(edge(&imp.file, &to, "cargo", real, identity, li, p, None));
                    }
                    Decision::Boundary(r, c, why) => {
                        boundary(r, "cargo", &li.label, real, &imp.file, c, why)
                    }
                }
            } else if eco("python").file_ext.iter().any(|e| e == ext) {
                if raw.starts_with('.') {
                    continue;
                }
                let root = raw.split('.').next().unwrap_or_default();
                let by_import: Vec<&Provider> = ctx
                    .providers
                    .iter()
                    .filter(|p| p.ecosystem == "python" && p.import_names.iter().any(|n| n == root))
                    .collect();
                if by_import.is_empty() {
                    continue;
                }
                let Some((mpath, man)) = nearest_manifest(li, &imp.file, &eco("python").manifest)
                    .and_then(|(p, t)| parse_toml(t).map(|v| (p, v)))
                else {
                    boundary(
                        BoundaryReason::Undeclared,
                        "python",
                        &li.label,
                        root,
                        &imp.file,
                        by_import.iter().map(|p| p.key()).collect(),
                        "no readable pyproject.toml at or above the importing file",
                    );
                    continue;
                };
                let mdir = parent_dir(mpath);
                let own: Vec<&Provider> = by_import
                    .iter()
                    .copied()
                    .filter(|p| p.dir == mdir && p.label == li.label)
                    .collect();
                if !own.is_empty() {
                    continue; // the project's own package
                }
                let declared = py_declared(&ctx, li, mdir, &man);
                let hits: Vec<&Provider> = by_import
                    .iter()
                    .copied()
                    .filter(|p| declared.contains_key(&p.name))
                    .collect();
                let keys: Vec<String> = by_import.iter().map(|p| p.key()).collect();
                match hits.as_slice() {
                    [] => boundary(
                        BoundaryReason::Undeclared,
                        "python",
                        &li.label,
                        root,
                        &imp.file,
                        keys,
                        "the importing project does not declare it",
                    ),
                    [p] => match declared.get(&p.name).cloned().flatten() {
                        Some(dir) if dir == p.dir => match py_entry(
                            ctx.label(&p.label).unwrap_or(li),
                            p.py_root.as_deref().unwrap_or(&p.dir),
                            raw,
                        ) {
                            Some(to) => edges.push(edge(
                                &imp.file,
                                &to,
                                "python",
                                &p.name,
                                Identity::Path,
                                li,
                                p,
                                None,
                            )),
                            None => boundary(
                                BoundaryReason::EntryUnmapped,
                                "python",
                                &li.label,
                                &p.name,
                                &imp.file,
                                vec![p.key()],
                                "the module is not an indexed file of the provider",
                            ),
                        },
                        Some(_) => boundary(
                            BoundaryReason::ProviderContradicted,
                            "python",
                            &li.label,
                            &p.name,
                            &imp.file,
                            vec![p.key()],
                            "path: the declared path holds a different project",
                        ),
                        None => boundary(
                            BoundaryReason::ProviderUnverified,
                            "python",
                            &li.label,
                            &p.name,
                            &imp.file,
                            vec![p.key()],
                            "registry: a registry requirement has no proof in this slice",
                        ),
                    },
                    many => boundary(
                        BoundaryReason::ProviderAmbiguous,
                        "python",
                        &li.label,
                        root,
                        &imp.file,
                        many.iter().map(|p| p.key()).collect(),
                        "several declared members provide that import name",
                    ),
                }
            }
        }
    }
    // One edge per (file, entry, package): two packages entering through one file stay two links.
    edges.sort_by(|a, b| {
        (&a.from_file, &a.to_file, &a.package).cmp(&(&b.from_file, &b.to_file, &b.package))
    });
    edges.dedup_by(|a, b| {
        a.from_file == b.from_file && a.to_file == b.to_file && a.package == b.package
    });
    let boundaries = bounds
        .into_iter()
        .map(
            |((reason, ecosystem, consumer_label, package), (files, cands, why))| Boundary {
                reason,
                ecosystem,
                consumer_label,
                package,
                files: files.into_iter().collect(),
                candidates: cands.into_iter().collect(),
                why: why.into_iter().collect(),
            },
        )
        .collect();
    let fingerprint = fingerprint(inputs);
    LinkOutput {
        edges,
        boundaries,
        fingerprint,
    }
}

#[allow(clippy::too_many_arguments)]
fn edge(
    from: &str,
    to: &str,
    eco: &str,
    pkg: &str,
    identity: Identity,
    li: &LabelInput,
    p: &Provider,
    consumer_version: Option<String>,
) -> LinkEdge {
    let skew = match (&consumer_version, &p.version) {
        (Some(c), Some(v)) => c.trim_start_matches(['^', '~', '=']) != v.as_str(),
        _ => false,
    };
    LinkEdge {
        from_file: from.to_string(),
        to_file: to.to_string(),
        ecosystem: eco.to_string(),
        package: pkg.to_string(),
        identity,
        consumer_label: li.label.clone(),
        provider_label: p.label.clone(),
        consumer_version,
        provider_version: p.version.clone(),
        skew,
    }
}

/// Hash of every input row (labels in order), the linker version and the rules digest.
pub fn fingerprint(inputs: &[LabelInput]) -> String {
    let mut sorted: Vec<&LabelInput> = inputs.iter().collect();
    sorted.sort_by(|a, b| a.label.cmp(&b.label));
    let mut h = fnv(LINKER_VERSION.as_bytes()) ^ fnv(ECOSYSTEMS_SRC.as_bytes()).rotate_left(1);
    for li in sorted {
        let s = serde_json::to_string(li).unwrap_or_default();
        h = h.rotate_left(5) ^ fnv(s.as_bytes());
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests;
