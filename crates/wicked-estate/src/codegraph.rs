//! codegraph import: for the hybrid languages (TS/JS, Java, Kotlin, C#, Scala, Swift, VB.NET,
//! C/C++, PHP) a pinned codegraph bundle produces extraction and resolution, and estate maps its
//! output onto estate's own ids (DES-estate-lang-fixes, Plan v2, PR-A).
//!
//! The flow inside one `index` run:
//! 1. [`prepare`] rewrites a redacted mirror of the switched files (the same text estate extracts
//!    from, `redact_source`), runs codegraph on the mirror, reads its database and decides a
//!    per-file [`Verdict`]. Unchanged files whose codegraph rows changed are returned so the
//!    caller re-extracts them: estate is the one sync owner.
//! 2. At resolution, [`Prepared::partition`] routes the refs of files with an `Ok` verdict and a
//!    codegraph-owned kind to [`Prepared::resolve`] instead of estate's name resolvers.
//!    A ref binds the codegraph edge at the same site; codegraph-only edges are added; codegraph
//!    definitions estate lacks get estate-scheme ids.
//! 3. The caller stores [`Prepared::fingerprints_json`] under [`FINGERPRINTS_META`] after the
//!    run commits. A later run with codegraph off re-extracts every file listed there once, so
//!    estate's own resolvers rebuild their edges, and clears the key.
//!
//! Every file codegraph could not fully handle keeps estate's own pipeline and says so on stderr
//! (`CODEGRAPH-SKIPPED: <path> <reason>`); a failed run keeps estate's pipeline for all files
//! (`CODEGRAPH-FALLBACK: <reason>`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wicked_estate_core::refs::CODEGRAPH_REF_PREFIX;
use wicked_estate_core::symbol::{Descriptor, Suffix, Symbol};
use wicked_estate_core::{
    Confidence, Edge, EdgeKind, Language, Location, Node, NodeKind, ResolutionTier, Span, SymbolId,
    UnresolvedRef,
};

/// The only codegraph release estate runs.
pub const PINNED_VERSION: &str = "1.6.4";

/// sha256 of every file in the extracted 1.6.4 bundle, per platform ([`bundle_tree_digest`]).
/// A platform without an entry refuses to run codegraph (estate's own pipeline runs instead).
/// Computed from the release asset `codegraph-darwin-arm64.tar.gz` (sha256 `daa91427…18cc`, the
/// value in the release's SHA256SUMS, attestation verified). Other platforms are pinned by the
/// install change that fetches their bundles.
const PINNED_TREES: &[(&str, &str)] = &[(
    "darwin-arm64",
    "bd497096474efab32c6bb80093befe0beb1f3feafc0e0e61858fe6c72ad93e3b",
)];

/// Bumped when the fingerprint or verdict encoding changes, so a stored set never compares equal
/// across encodings.
const FINGERPRINT_VERSION: u32 = 1;

/// The default hybrid language list (codegraph language names).
pub const DEFAULT_LANGUAGES: &[&str] = &[
    "typescript",
    "javascript",
    "java",
    "kotlin",
    "csharp",
    "scala",
    "swift",
    "vbnet",
    "c",
    "cpp",
    "php",
];

/// Extension → codegraph language, for the languages estate can hand over. `tsx`/`jsx` belong to
/// `typescript`/`javascript` for the language switch.
fn language_of_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "cs" => "csharp",
        "scala" | "sc" => "scala",
        "swift" => "swift",
        "vb" => "vbnet",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "cpp",
        "php" => "php",
        _ => return None,
    })
}

/// Project files codegraph's resolvers read (path aliases, package roots, namespaces). They are
/// mirrored (redacted) next to the sources so resolution sees the same configuration.
pub fn is_resolver_config(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    (lower.starts_with("tsconfig") && lower.ends_with(".json"))
        || matches!(
            lower.as_str(),
            "jsconfig.json"
                | "package.json"
                | "pnpm-workspace.yaml"
                | "lerna.json"
                | "composer.json"
                | "go.mod"
                | "build.gradle"
                | "build.gradle.kts"
                | "settings.gradle"
                | "settings.gradle.kts"
                | "pom.xml"
                | "compile_commands.json"
                | "package.swift"
        )
        || lower.ends_with(".csproj")
        || lower.ends_with(".vbproj")
        || lower.ends_with(".sln")
}

/// How a run uses codegraph.
#[derive(Debug, Clone)]
pub struct CodegraphConfig {
    /// The extracted bundle directory (holds `node` and `lib/`).
    pub bundle: PathBuf,
    /// Estate-owned working directory for this repo: the mirror, codegraph's database, the
    /// scratch home and the fingerprints.
    pub state_dir: PathBuf,
    /// Codegraph language names switched to codegraph; empty means [`DEFAULT_LANGUAGES`].
    pub languages: Vec<String>,
    /// Wall-clock limit for one codegraph run.
    pub timeout: Duration,
}

impl CodegraphConfig {
    fn switched(&self, ext: &str) -> bool {
        let Some(lang) = language_of_ext(ext) else {
            return false;
        };
        if self.languages.is_empty() {
            DEFAULT_LANGUAGES.contains(&lang)
        } else {
            self.languages.iter().any(|l| l == lang)
        }
    }

    /// Whether `rel` (any path form) has a switched language.
    pub fn switched_path(&self, rel: &str) -> bool {
        let ext = Path::new(rel)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        self.switched(&ext)
    }

    fn mirror_root(&self) -> PathBuf {
        self.state_dir.join("tree")
    }

    fn db_path(&self) -> PathBuf {
        self.mirror_root().join(".codegraph").join("codegraph.db")
    }
}

/// Store meta key (per repo label) holding the per-file fingerprints of the last codegraph run:
/// a JSON object, estate path → fingerprint. Present only while codegraph covers the repo.
pub const FINGERPRINTS_META: &str = "codegraph_fingerprints";

/// Parse the stored fingerprints; an unreadable value counts as none.
pub fn parse_fingerprints(stored: Option<&str>) -> BTreeMap<String, String> {
    stored
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

// ── bundle ──────────────────────────────────────────────────────────────────────────────────

fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// sha256 over every file of `dir`, sorted by relative path: for each file,
/// `<rel path>\0<sha256 of its bytes>\n` (a symlink contributes its target text instead).
pub fn bundle_tree_digest(dir: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            if ft.is_dir() {
                stack.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    let mut rels: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(dir)
                .unwrap_or(&p)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            (rel, p)
        })
        .collect();
    rels.sort();
    let mut outer = Sha256::new();
    for (rel, p) in rels {
        let meta = std::fs::symlink_metadata(&p)?;
        let mut inner = Sha256::new();
        if meta.file_type().is_symlink() {
            inner.update(std::fs::read_link(&p)?.to_string_lossy().as_bytes());
        } else {
            let mut f = std::fs::File::open(&p)?;
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                inner.update(&buf[..n]);
            }
        }
        outer.update(rel.as_bytes());
        outer.update([0]);
        outer.update(hex(&inner.finalize()).as_bytes());
        outer.update(b"\n");
    }
    Ok(hex(&outer.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The cache key for the bundle's tree digest: a sha256 over every file's path, size, mtime,
/// ctime, inode and device. A content change moves the file's ctime, which user space cannot
/// set back (`touch -r` restores the mtime only), so an edit that keeps the size and the mtime
/// still misses the cache. `None` where the platform exposes no ctime: hash every run there.
#[cfg(unix)]
fn bundle_stamp(dir: &Path) -> std::io::Result<Option<String>> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    let mut lines = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let meta = std::fs::symlink_metadata(entry.path())?;
            if meta.is_dir() {
                stack.push(entry.path());
                continue;
            }
            lines.push(format!(
                "{}\0{}|{}.{}|{}.{}|{}|{}",
                entry.path().display(),
                meta.len(),
                meta.mtime(),
                meta.mtime_nsec(),
                meta.ctime(),
                meta.ctime_nsec(),
                meta.ino(),
                meta.dev()
            ));
        }
    }
    lines.sort();
    let mut h = Sha256::new();
    for l in &lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    Ok(Some(hex(&h.finalize())))
}

#[cfg(not(unix))]
fn bundle_stamp(_dir: &Path) -> std::io::Result<Option<String>> {
    Ok(None)
}

/// Check the bundle is the pinned release for this platform. The tree digest is cached in the
/// state dir under [`bundle_stamp`]; any file added, removed or rewritten re-hashes the tree.
fn verify_bundle(cfg: &CodegraphConfig) -> std::result::Result<(), String> {
    let pkg = std::fs::read_to_string(cfg.bundle.join("lib").join("package.json"))
        .map_err(|e| format!("bundle has no readable lib/package.json ({e})"))?;
    let version = serde_json::from_str::<serde_json::Value>(&pkg)
        .ok()
        .and_then(|v| {
            v.get("version")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    if version != PINNED_VERSION {
        return Err(format!(
            "bundle version {version:?} is not the pinned {PINNED_VERSION}"
        ));
    }
    let platform = platform_key();
    let Some((_, pinned)) = PINNED_TREES.iter().find(|(p, _)| *p == platform) else {
        return Err(format!("no pinned codegraph bundle digest for {platform}"));
    };
    let stamp = bundle_stamp(&cfg.bundle).map_err(|e| format!("cannot read the bundle ({e})"))?;
    let cache = cfg.state_dir.join("bundle-verified");
    let cache_line = stamp.map(|stamp| format!("{stamp}|{pinned}"));
    if let Some(line) = &cache_line {
        if std::fs::read_to_string(&cache).ok().as_deref() == Some(line.as_str()) {
            return Ok(());
        }
    }
    let digest =
        bundle_tree_digest(&cfg.bundle).map_err(|e| format!("cannot hash the bundle ({e})"))?;
    if digest != *pinned {
        return Err(format!(
            "bundle tree digest {digest} does not match the pinned {PINNED_VERSION} digest for {platform}"
        ));
    }
    if let Some(line) = cache_line {
        let _ = std::fs::write(&cache, line);
    }
    Ok(())
}

// ── mirror ──────────────────────────────────────────────────────────────────────────────────

/// One file the caller can hand to the mirror: the label-free repo path, the estate digest, and
/// a way to read its redacted text.
pub struct MirrorFile {
    pub raw: String,
    pub digest: String,
}

/// Bring the mirror in line with `files` (switched sources) and `configs` (resolver config
/// files). A source is rewritten only when its digest changed; a config when its text did.
/// Files no longer listed are removed. `read` returns the redacted text, `None` when the file
/// cannot be read (it is left out of the mirror, so codegraph reports it absent).
fn sync_mirror(
    cfg: &CodegraphConfig,
    files: &[MirrorFile],
    configs: &[String],
    read: &(dyn Fn(&str) -> Option<String> + Sync),
) -> std::io::Result<()> {
    let root = cfg.mirror_root();
    std::fs::create_dir_all(&root)?;
    let index_path = cfg.state_dir.join("mirror.json");
    let previous: BTreeMap<String, String> = std::fs::read_to_string(&index_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut next: BTreeMap<String, String> = BTreeMap::new();
    for f in files {
        if !safe_rel(&f.raw) {
            continue;
        }
        let target = root.join(&f.raw);
        if previous.get(&f.raw) == Some(&f.digest) && target.is_file() {
            next.insert(f.raw.clone(), f.digest.clone());
            continue;
        }
        match read(&f.raw) {
            Some(text) => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, text)?;
                next.insert(f.raw.clone(), f.digest.clone());
            }
            None => {
                let _ = std::fs::remove_file(&target);
            }
        }
    }
    for raw in configs {
        if !safe_rel(raw) {
            continue;
        }
        let target = root.join(raw);
        match read(raw) {
            Some(text) => {
                if std::fs::read_to_string(&target).ok().as_deref() != Some(text.as_str()) {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&target, &text)?;
                }
                next.insert(raw.clone(), String::from("config"));
            }
            None => {
                let _ = std::fs::remove_file(&target);
            }
        }
    }
    for raw in previous.keys() {
        if !next.contains_key(raw) && safe_rel(raw) {
            let _ = std::fs::remove_file(root.join(raw));
        }
    }
    std::fs::write(
        &index_path,
        serde_json::to_string(&next).unwrap_or_else(|_| "{}".into()),
    )?;
    Ok(())
}

/// A repo-relative path that stays inside the mirror and never touches codegraph's own dir.
fn safe_rel(raw: &str) -> bool {
    let p = Path::new(raw);
    !raw.is_empty()
        && p.is_relative()
        && p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        && !raw.starts_with(".codegraph/")
}

// ── run ─────────────────────────────────────────────────────────────────────────────────────

/// Run codegraph on the mirror: `init` the first time, `sync` after. A clean environment (no
/// inherited variables, a scratch home, telemetry and update checks off) and a wall-clock limit.
fn run_codegraph(cfg: &CodegraphConfig) -> std::result::Result<(), String> {
    let node = if cfg!(windows) {
        cfg.bundle.join("node.exe")
    } else {
        cfg.bundle.join("node")
    };
    let js = cfg
        .bundle
        .join("lib")
        .join("dist")
        .join("bin")
        .join("codegraph.js");
    let home = cfg.state_dir.join("home");
    for sub in ["", ".config", ".cache", ".local/share", "tmp"] {
        std::fs::create_dir_all(home.join(sub)).map_err(|e| format!("scratch home: {e}"))?;
    }
    let mirror = cfg.mirror_root();
    let verb = if cfg.db_path().is_file() {
        "sync"
    } else {
        "init"
    };
    let mut cmd = std::process::Command::new(&node);
    cmd.arg("--liftoff-only")
        .arg("--disable-warning=ExperimentalWarning")
        .arg("--max-semi-space-size=16")
        .arg(&js)
        .arg(verb);
    if verb == "init" {
        cmd.arg("-y");
    } else {
        cmd.arg("-q");
    }
    cmd.arg(&mirror)
        .current_dir(&mirror)
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("TMPDIR", home.join("tmp"))
        .env("TEMP", home.join("tmp"))
        .env("TMP", home.join("tmp"))
        .env("CODEGRAPH_TELEMETRY", "0")
        .env("DO_NOT_TRACK", "1")
        .env("CODEGRAPH_NO_UPDATE_CHECK", "1")
        .env("CI", "1")
        // The mirror sits under the user's work tree by default (`.wicked-estate/`): codegraph's
        // `git` calls must not find the user's repo above it, and `init` must never offer to
        // install git hooks there.
        .env("GIT_CEILING_DIRECTORIES", &cfg.state_dir)
        .env("CODEGRAPH_FORCE_WATCH", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    if let Some(path) = std::env::var_os("SystemRoot") {
        // Windows needs SystemRoot for its own DLL loading; it is not a secret.
        cmd.env("SystemRoot", path);
    }
    // Its own process group, so a timeout takes codegraph's `git` children down with it.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start codegraph ({e})"))?;
    let stderr = child.stderr.take();
    let tail = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut e) = stderr {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= cfg.timeout => {
                #[cfg(unix)]
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "codegraph {verb} timed out after {}s",
                    cfg.timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("codegraph {verb}: {e}")),
        }
    };
    let err = tail.join().unwrap_or_default();
    if !status.success() {
        let last = err
            .lines()
            .last()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();
        return Err(format!("codegraph {verb} exited with {status}: {last}"));
    }
    Ok(())
}

// ── codegraph database ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct CgNode {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub signature: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CgEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub confidence: Option<f64>,
    pub resolved_by: Option<String>,
    pub ref_name: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CgUnresolved {
    pub from: String,
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub col: u32,
    pub file: String,
}

/// What codegraph's database says, read once per run.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    /// path → `errors` JSON (`None` when the column is NULL or `[]`).
    pub files: HashMap<String, Option<String>>,
    pub nodes: HashMap<String, CgNode>,
    pub edges: Vec<CgEdge>,
    pub unresolved: Vec<CgUnresolved>,
}

impl Snapshot {
    pub(crate) fn read(db: &Path) -> std::result::Result<Snapshot, String> {
        use rusqlite::{Connection, OpenFlags};
        let conn = Connection::open_with_flags(
            db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("cannot open the codegraph database ({e})"))?;
        Snapshot::from_conn(&conn).map_err(|e| format!("cannot read the codegraph database ({e})"))
    }

    pub(crate) fn from_conn(conn: &rusqlite::Connection) -> rusqlite::Result<Snapshot> {
        let mut snap = Snapshot::default();
        let mut st = conn.prepare("SELECT path, errors FROM files")?;
        for row in st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })? {
            let (path, errors) = row?;
            let errors = errors.filter(|e| {
                serde_json::from_str::<serde_json::Value>(e)
                    .map(|v| v.as_array().is_none_or(|a| !a.is_empty()))
                    .unwrap_or(true)
            });
            snap.files.insert(path, errors);
        }
        let mut st = conn.prepare(
            "SELECT id, kind, name, qualified_name, file_path, start_line, end_line, \
             start_column, end_column, signature FROM nodes",
        )?;
        for row in st.query_map([], |r| {
            Ok(CgNode {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                qualified_name: r.get(3)?,
                file: r.get(4)?,
                start_line: r.get::<_, i64>(5)?.max(0) as u32,
                end_line: r.get::<_, i64>(6)?.max(0) as u32,
                start_col: r.get::<_, i64>(7)?.max(0) as u32,
                end_col: r.get::<_, i64>(8)?.max(0) as u32,
                signature: r.get(9)?,
            })
        })? {
            let n = row?;
            snap.nodes.insert(n.id.clone(), n);
        }
        let mut st = conn.prepare("SELECT source, target, kind, metadata, line, col FROM edges")?;
        for row in st.query_map([], |r| {
            let meta: Option<String> = r.get(3)?;
            let meta = meta
                .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
                .unwrap_or(serde_json::Value::Null);
            Ok(CgEdge {
                source: r.get(0)?,
                target: r.get(1)?,
                kind: r.get(2)?,
                line: r.get::<_, Option<i64>>(4)?.map(|v| v.max(0) as u32),
                col: r.get::<_, Option<i64>>(5)?.map(|v| v.max(0) as u32),
                confidence: meta.get("confidence").and_then(|v| v.as_f64()),
                resolved_by: meta
                    .get("resolvedBy")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                ref_name: meta
                    .get("refName")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
        })? {
            snap.edges.push(row?);
        }
        let mut st = conn.prepare(
            "SELECT from_node_id, reference_name, reference_kind, line, col, file_path \
             FROM unresolved_refs",
        )?;
        for row in st.query_map([], |r| {
            Ok(CgUnresolved {
                from: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                line: r.get::<_, i64>(3)?.max(0) as u32,
                col: r.get::<_, i64>(4)?.max(0) as u32,
                file: r.get(5)?,
            })
        })? {
            snap.unresolved.push(row?);
        }
        Ok(snap)
    }

    /// Per-file fingerprint of everything codegraph says about the file's outgoing facts:
    /// its error state, the edges sourced in it (targets by file + qualified name, never by
    /// line, so an edit above a target does not move it) and its unresolved rows.
    fn fingerprints(&self) -> BTreeMap<String, String> {
        let mut rows: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (path, errors) in &self.files {
            rows.entry(path.clone())
                .or_default()
                .push(format!("E|{}", errors.as_deref().unwrap_or("")));
        }
        for e in &self.edges {
            let (Some(s), Some(t)) = (self.nodes.get(&e.source), self.nodes.get(&e.target)) else {
                continue;
            };
            rows.entry(s.file.clone()).or_default().push(format!(
                "R|{}|{}|{}|{:?}|{:?}|{}|{}|{}|{:?}",
                s.qualified_name,
                s.kind,
                e.kind,
                e.line,
                e.col,
                t.file,
                t.qualified_name,
                t.kind,
                e.confidence.map(|c| (c * 1000.0).round() as i64)
            ));
        }
        for u in &self.unresolved {
            rows.entry(u.file.clone())
                .or_default()
                .push(format!("U|{}|{}|{}|{}", u.name, u.kind, u.line, u.col));
        }
        rows.into_iter()
            .map(|(path, mut lines)| {
                lines.sort();
                let digest = xxhash_rust::xxh3::xxh3_128(lines.join("\n").as_bytes());
                (path, format!("{FINGERPRINT_VERSION}:{digest:032x}"))
            })
            .collect()
    }
}

// ── verdicts ────────────────────────────────────────────────────────────────────────────────

/// Whether a switched file takes codegraph's facts or keeps estate's own pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Skipped(String),
}

/// The error code codegraph recorded for a file (`size_exceeded`, `parse_error`, ...), or
/// `errors` when the JSON carries none.
fn error_reason(errors: &str) -> String {
    serde_json::from_str::<serde_json::Value>(errors)
        .ok()
        .and_then(|v| {
            v.as_array()?
                .iter()
                .find_map(|e| e.get("code").and_then(|c| c.as_str()).map(str::to_string))
        })
        .unwrap_or_else(|| "errors".to_string())
}

fn file_verdict(snap: &Snapshot, raw: &str) -> Verdict {
    match snap.files.get(raw) {
        None => Verdict::Skipped("absent".into()),
        Some(Some(errors)) => Verdict::Skipped(error_reason(errors)),
        Some(None) => Verdict::Ok,
    }
}

#[cfg(test)]
thread_local! {
    /// SQL that builds the codegraph database a test run reads instead of running codegraph;
    /// `"FAIL"` makes the run fail.
    pub(crate) static TEST_DATABASE_SQL: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

// ── prepare ─────────────────────────────────────────────────────────────────────────────────

/// The run's codegraph state, from [`prepare`] to the stored fingerprints.
pub struct Prepared {
    /// `<label>/` under a repo label, else empty: estate paths are `prefix + raw`.
    prefix: String,
    snap: Snapshot,
    /// Estate path → verdict, for every switched file in the work set.
    verdicts: HashMap<String, Verdict>,
    fingerprints: BTreeMap<String, String>,
    /// Codegraph edges a ref bound this run (by index into `snap.edges`).
    consumed: Mutex<HashSet<usize>>,
    /// Per-file facts the merge step dropped, reported once per run.
    pub dropped_edges: Mutex<usize>,
}

/// Inputs from the caller for [`prepare`].
pub struct PrepareInput<'a> {
    /// The indexed root. The state dir may sit under it only below a directory both walks always
    /// skip (the default `.wicked-estate/graph.db.codegraph`); elsewhere the mirror's copies
    /// would be indexed as sources.
    pub root: &'a Path,
    pub prefix: &'a str,
    /// Every switched file in this run's work set.
    pub files: Vec<MirrorFile>,
    /// Label-free paths of resolver config files under the root.
    pub configs: Vec<String>,
    /// Label-free path → redacted text.
    pub read: &'a (dyn Fn(&str) -> Option<String> + Sync),
    /// Estate paths of files whose digest is unchanged this run.
    pub unchanged: Vec<String>,
    /// The fingerprints the last run recorded ([`FINGERPRINTS_META`]).
    pub stored: BTreeMap<String, String>,
}

/// Refuse a state dir the walker would index: under `root` with no [`crate::SKIPPED_ENTRY_NAMES`]
/// directory on the way. Those names are dropped by `filter_entry` in both walks, whatever the
/// ignore files say; a plain dot directory is not enough (an `.ignore` whitelist re-opens it).
fn state_dir_outside_sources(state_dir: &Path, root: &Path) -> std::result::Result<(), String> {
    let (Ok(state), Ok(root)) = (dunce::canonicalize(state_dir), dunce::canonicalize(root)) else {
        return Ok(());
    };
    let Ok(rel) = state.strip_prefix(&root) else {
        return Ok(());
    };
    let skipped = rel
        .components()
        .any(|c| crate::SKIPPED_ENTRY_NAMES.contains(&c.as_os_str().to_string_lossy().as_ref()));
    if skipped {
        Ok(())
    } else {
        Err(format!(
            "the codegraph state dir {} is inside the indexed root, where its mirror would be indexed; keep the graph file outside the repo or under .wicked-estate/",
            rel.display()
        ))
    }
}

/// Run codegraph and decide the per-file verdicts. Returns the prepared state and the unchanged
/// files to re-extract (their codegraph facts or verdict changed since the last run), or the
/// reason codegraph cannot be used this run.
pub fn prepare(
    cfg: &CodegraphConfig,
    input: PrepareInput<'_>,
) -> std::result::Result<(Prepared, Vec<String>), String> {
    // Absolute paths: codegraph runs with the mirror as its working directory, so a relative
    // bundle or mirror path would resolve against the wrong directory.
    let absolute = |p: &Path| std::path::absolute(p).map_err(|e| format!("{}: {e}", p.display()));
    let cfg = &CodegraphConfig {
        bundle: absolute(&cfg.bundle)?,
        state_dir: absolute(&cfg.state_dir)?,
        ..cfg.clone()
    };
    std::fs::create_dir_all(&cfg.state_dir).map_err(|e| format!("state dir: {e}"))?;
    state_dir_outside_sources(&cfg.state_dir, input.root)?;
    // The mirror and the scratch home are never one `git add -A` away from a commit.
    std::fs::write(cfg.state_dir.join(".gitignore"), "*\n")
        .map_err(|e| format!("state dir: {e}"))?;
    #[cfg(test)]
    if let Some(sql) = TEST_DATABASE_SQL.with(|t| t.borrow().clone()) {
        // Tests stand a recorded codegraph database (real bundle output, see
        // tests/fixtures/codegraph) in for the run; the mirror is still written.
        sync_mirror(cfg, &input.files, &input.configs, input.read)
            .map_err(|e| format!("mirror: {e}"))?;
        if sql == "FAIL" {
            return Err("codegraph sync exited with exit status: 1: (test)".into());
        }
        let conn = rusqlite::Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(&sql).map_err(|e| e.to_string())?;
        let snap = Snapshot::from_conn(&conn).map_err(|e| e.to_string())?;
        return Ok(Prepared::new(
            input.prefix,
            snap,
            &input.files,
            &input.unchanged,
            &input.stored,
        ));
    }
    verify_bundle(cfg)?;
    sync_mirror(cfg, &input.files, &input.configs, input.read)
        .map_err(|e| format!("mirror: {e}"))?;
    run_codegraph(cfg)?;
    let snap = Snapshot::read(&cfg.db_path())?;
    Ok(Prepared::new(
        input.prefix,
        snap,
        &input.files,
        &input.unchanged,
        &input.stored,
    ))
}

impl Prepared {
    pub(crate) fn new(
        prefix: &str,
        snap: Snapshot,
        files: &[MirrorFile],
        unchanged: &[String],
        stored: &BTreeMap<String, String>,
    ) -> (Prepared, Vec<String>) {
        let mut verdicts = HashMap::new();
        for f in files {
            verdicts.insert(format!("{prefix}{}", f.raw), file_verdict(&snap, &f.raw));
        }
        let current = snap.fingerprints();
        let mut fingerprints = BTreeMap::new();
        for (path, verdict) in &verdicts {
            let raw = &path[prefix.len()..];
            let fp = format!(
                "{}|{}",
                match verdict {
                    Verdict::Ok => "ok".to_string(),
                    Verdict::Skipped(r) => format!("skipped:{r}"),
                },
                current.get(raw).map(String::as_str).unwrap_or("-")
            );
            fingerprints.insert(path.clone(), fp);
        }
        // An unchanged file is re-extracted when its codegraph facts or verdict moved, and when
        // no earlier run recorded it (codegraph was just switched on for it).
        let forced = unchanged
            .iter()
            .filter(|p| {
                fingerprints
                    .get(*p)
                    .is_some_and(|fp| stored.get(*p) != Some(fp))
            })
            .cloned()
            .collect();
        (
            Prepared {
                prefix: prefix.to_string(),
                snap,
                verdicts,
                fingerprints,
                consumed: Mutex::new(HashSet::new()),
                dropped_edges: Mutex::new(0),
            },
            forced,
        )
    }

    /// The verdict of an estate path, `None` for a file codegraph does not cover.
    pub fn verdict(&self, path: &str) -> Option<&Verdict> {
        self.verdicts.get(path)
    }

    /// This run's fingerprints for [`FINGERPRINTS_META`]. Store them only after the run's
    /// writes committed: a failed run must compare against the last good one.
    pub fn fingerprints_json(&self) -> String {
        serde_json::to_string(&self.fingerprints).unwrap_or_else(|_| "{}".into())
    }

    /// One `CODEGRAPH-SKIPPED` line per switched file that keeps estate's pipeline.
    pub fn skipped_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .verdicts
            .iter()
            .filter_map(|(p, v)| match v {
                Verdict::Skipped(r) => Some(format!("CODEGRAPH-SKIPPED: {p} {r}")),
                Verdict::Ok => None,
            })
            .collect();
        lines.sort();
        lines
    }
}

// ── merge ───────────────────────────────────────────────────────────────────────────────────

/// Edge kinds codegraph is authoritative for in a switched file. Other kinds (rules, value flow,
/// framework edges) keep estate's own resolvers.
fn codegraph_owned(kind: &EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Calls
            | EdgeKind::Imports
            | EdgeKind::References
            | EdgeKind::Instantiates
            | EdgeKind::Implements
            | EdgeKind::Extends
            | EdgeKind::Overrides
            | EdgeKind::HasType
            | EdgeKind::Returns
    )
}

fn map_edge_kind(kind: &str) -> Option<EdgeKind> {
    Some(match kind {
        "calls" => EdgeKind::Calls,
        "imports" => EdgeKind::Imports,
        "references" => EdgeKind::References,
        "instantiates" => EdgeKind::Instantiates,
        "implements" => EdgeKind::Implements,
        "extends" => EdgeKind::Extends,
        "overrides" => EdgeKind::Overrides,
        "returns" => EdgeKind::Returns,
        "type_of" => EdgeKind::HasType,
        "decorates" | "navigates" | "exports" => EdgeKind::Other(kind.to_string()),
        // `contains` is structural: estate's own extraction owns it.
        _ => return None,
    })
}

/// Coarse kind class for matching a codegraph definition to an estate node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum KindClass {
    Callable,
    Type,
    Value,
    File,
}

fn cg_class(kind: &str) -> Option<KindClass> {
    Some(match kind {
        "function" | "method" => KindClass::Callable,
        "class" | "struct" | "interface" | "trait" | "protocol" | "enum" | "type_alias"
        | "namespace" | "module" | "union" => KindClass::Type,
        "property" | "field" | "variable" | "constant" | "enum_member" => KindClass::Value,
        "file" => KindClass::File,
        // parameter / import / export / component / route: estate's own nodes cover them.
        _ => return None,
    })
}

fn estate_class(kind: &NodeKind) -> Option<KindClass> {
    Some(match kind {
        NodeKind::Function | NodeKind::Method | NodeKind::Constructor => KindClass::Callable,
        NodeKind::Class
        | NodeKind::Struct
        | NodeKind::Enum
        | NodeKind::Interface
        | NodeKind::Trait
        | NodeKind::TypeAlias
        | NodeKind::Namespace
        | NodeKind::Module => KindClass::Type,
        NodeKind::Field | NodeKind::Constant | NodeKind::Variable => KindClass::Value,
        NodeKind::File => KindClass::File,
        _ => return None,
    })
}

fn map_node_kind(kind: &str) -> Option<NodeKind> {
    Some(match kind {
        "function" => NodeKind::Function,
        "method" => NodeKind::Method,
        "class" => NodeKind::Class,
        "struct" | "union" => NodeKind::Struct,
        "interface" | "protocol" => NodeKind::Interface,
        "trait" => NodeKind::Trait,
        "enum" => NodeKind::Enum,
        "type_alias" => NodeKind::TypeAlias,
        "namespace" => NodeKind::Namespace,
        "module" => NodeKind::Module,
        "property" | "field" => NodeKind::Field,
        "variable" => NodeKind::Variable,
        "constant" | "enum_member" => NodeKind::Constant,
        _ => return None,
    })
}

/// The name a ref was written with, for site matching: estate's routing prefixes removed and
/// the last member or path segment kept (`this.repo.save` → `save`, `@lib/util` → `util`). Both
/// sides of a match pass through it, so it only has to be consistent.
fn name_tail(raw: &str) -> String {
    let raw = raw
        .strip_prefix(wicked_estate_core::refs::LOCAL_BINDING_PREFIX)
        .map(|r| r.split(':').next().unwrap_or(r))
        .or_else(|| {
            raw.strip_prefix(wicked_estate_core::refs::IMPORT_ALIAS_PREFIX)
                .map(|r| r.split(':').next().unwrap_or(r))
        })
        .unwrap_or(raw);
    // An import spec keeps its quotes in estate (`'./util'`, `"@lib/util"`, `<stdio.h>`) and
    // loses them in codegraph's refName.
    let raw = raw.trim_matches(|c: char| matches!(c, '\'' | '"' | '`' | '<' | '>'));
    let raw = raw.trim_end_matches("()");
    let tail = raw
        .rsplit(['.', ':', '\\', '>', '/'])
        .find(|s| !s.is_empty())
        .unwrap_or(raw);
    tail.trim_end_matches("()").to_string()
}

/// Codegraph id → estate id for the definitions of the switched `Ok` files, plus the estate
/// nodes minted for codegraph definitions estate's extraction lacks.
#[derive(Default)]
pub(crate) struct IdMap {
    pub ids: HashMap<String, SymbolId>,
    pub minted: Vec<Node>,
    pub minted_edges: Vec<Edge>,
    /// Estate paths that keep estate's pipeline after all, with the reason: `ambiguous-def`
    /// (a definition matched more than one node) or `no-defs` (codegraph found no definition
    /// where estate found some).
    pub ambiguous: BTreeMap<String, &'static str>,
}

impl Prepared {
    /// Map codegraph definitions onto estate nodes. `nodes` is the post-write population (the
    /// resolution index); `mint_in` names the files extracted this run, the only ones whose
    /// unmatched definitions get new nodes (an unchanged file's were minted when it changed).
    pub(crate) fn id_map(&self, nodes: &[&Node], mint_in: &HashSet<String>) -> IdMap {
        let mut map = IdMap::default();
        // Estate candidates per (file, name, class).
        let mut estate: HashMap<(&str, &str, KindClass), Vec<&Node>> = HashMap::new();
        let mut files_lang: HashMap<&str, &Language> = HashMap::new();
        let mut taken: HashSet<&SymbolId> = HashSet::new();
        for n in nodes {
            if !matches!(self.verdicts.get(&n.location.file), Some(Verdict::Ok)) {
                continue;
            }
            taken.insert(&n.symbol);
            if n.kind == NodeKind::File {
                files_lang.insert(n.location.file.as_str(), &n.language);
            }
            if let Some(class) = estate_class(&n.kind) {
                estate
                    .entry((n.location.file.as_str(), n.name.as_str(), class))
                    .or_default()
                    .push(n);
            }
        }
        // Codegraph definitions per (file, name, class), with their containing definition.
        let mut parent: HashMap<&str, &str> = HashMap::new();
        for e in &self.snap.edges {
            if e.kind == "contains" {
                parent.insert(e.target.as_str(), e.source.as_str());
            }
        }
        let mut cg: HashMap<(String, &str, KindClass), Vec<&CgNode>> = HashMap::new();
        for n in self.snap.nodes.values() {
            let path = format!("{}{}", self.prefix, n.file);
            if !matches!(self.verdicts.get(&path), Some(Verdict::Ok)) {
                continue;
            }
            if let Some(class) = cg_class(&n.kind) {
                cg.entry((path, n.name.as_str(), class))
                    .or_default()
                    .push(n);
            }
        }
        // A file codegraph read without one definition, where estate's extraction has some, is
        // a silent miss (a grammar gap, a truncated parse): estate keeps it.
        let mut estate_defs: HashMap<&str, usize> = HashMap::new();
        for ((file, _, class), nodes) in &estate {
            if *class != KindClass::File {
                *estate_defs.entry(file).or_default() += nodes.len();
            }
        }
        let mut cg_defs: HashMap<&str, usize> = HashMap::new();
        for ((file, _, class), nodes) in &cg {
            if *class != KindClass::File {
                *cg_defs.entry(file.as_str()).or_default() += nodes.len();
            }
        }
        for (file, n) in &estate_defs {
            if *n > 0 && cg_defs.get(file).copied().unwrap_or(0) == 0 {
                map.ambiguous.insert((*file).to_string(), "no-defs");
            }
        }
        let owner_of = |n: &CgNode| -> Option<String> {
            let p = self.snap.nodes.get(*parent.get(n.id.as_str())?)?;
            (cg_class(&p.kind) == Some(KindClass::Type)).then(|| p.name.clone())
        };
        let mut unmatched: Vec<(&CgNode, String)> = Vec::new();
        let mut keys: Vec<_> = cg.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let defs = &cg[&key];
            let (path, name, class) = (&key.0, key.1, key.2);
            if class == KindClass::File {
                for d in defs {
                    map.ids
                        .insert(d.id.clone(), Symbol::file(path.clone()).id());
                }
                continue;
            }
            let cands: Vec<&Node> = estate
                .get(&(path.as_str(), name, class))
                .cloned()
                .unwrap_or_default();
            // Pair each codegraph def with the estate nodes on its start line, then break ties
            // by owner, then by column. Anything still not one-to-one is ambiguous.
            let mut used: HashSet<&SymbolId> = HashSet::new();
            for d in defs {
                let line0 = d.start_line.saturating_sub(1);
                let mut on_line: Vec<&Node> = cands
                    .iter()
                    .copied()
                    .filter(|n| n.location.span.start_line == line0 && !used.contains(&n.symbol))
                    .collect();
                if on_line.len() > 1 {
                    if let Some(owner) = owner_of(d) {
                        on_line
                            .retain(|n| estate_owner(&n.symbol).as_deref() == Some(owner.as_str()));
                    }
                }
                if on_line.len() > 1 {
                    on_line.retain(|n| n.location.span.start_col == d.start_col);
                }
                if on_line.is_empty() {
                    // Fall back to span overlap (an annotation line may start one span and not
                    // the other); must still be unique.
                    let overlap: Vec<&Node> = cands
                        .iter()
                        .copied()
                        .filter(|n| {
                            !used.contains(&n.symbol)
                                && n.location.span.start_line <= d.end_line.saturating_sub(1)
                                && line0 <= n.location.span.end_line
                        })
                        .collect();
                    match overlap.as_slice() {
                        [one] => on_line.push(one),
                        [] => {}
                        _ => {
                            map.ambiguous.insert(path.clone(), "ambiguous-def");
                            continue;
                        }
                    }
                }
                match on_line.as_slice() {
                    [one] => {
                        used.insert(&one.symbol);
                        map.ids.insert(d.id.clone(), one.symbol.clone());
                    }
                    [] => unmatched.push((d, path.clone())),
                    _ => {
                        map.ambiguous.insert(path.clone(), "ambiguous-def");
                    }
                }
            }
        }
        // Mint estate-scheme ids for codegraph-only definitions in files extracted this run.
        // Outer definitions first, so a nested one can hang under a minted owner.
        unmatched.sort_by_key(|(d, p)| (p.clone(), d.start_line, std::cmp::Reverse(d.end_line)));
        for (d, path) in unmatched {
            if is_package_declaration(d) {
                // `package p;` holds the file's imports in codegraph; estate's File node does.
                map.ids
                    .insert(d.id.clone(), Symbol::file(path.clone()).id());
                continue;
            }
            if !mint_in.contains(&path) || map.ambiguous.contains_key(&path) {
                continue;
            }
            if !mintable(d) {
                continue;
            }
            // Only module- and type-level definitions: estate keeps function bodies flat
            // (ADR-002, `enclosing_chain`), so a helper nested in a callback gets no id of its
            // own. Minting one would need a position ordinal, which moves under edits.
            let declared_at_top = match parent
                .get(d.id.as_str())
                .and_then(|p| self.snap.nodes.get(*p))
            {
                None => true,
                Some(p) => {
                    p.kind == "file"
                        || is_package_declaration(p)
                        || cg_class(&p.kind) == Some(KindClass::Type)
                }
            };
            if !declared_at_top {
                continue;
            }
            let Some(kind) = map_node_kind(&d.kind) else {
                continue;
            };
            let Some(lang) = files_lang.get(path.as_str()) else {
                continue;
            };
            // Chain: the codegraph `contains` ancestors that are types, innermost first, cut at
            // the first non-type (estate's enclosing_chain rule). File-level namespaces
            // (`package p;`) are not containers in codegraph's `contains` from the file.
            let mut chain = Vec::new();
            let mut cur = parent.get(d.id.as_str()).copied();
            while let Some(pid) = cur {
                let Some(p) = self.snap.nodes.get(pid) else {
                    break;
                };
                if cg_class(&p.kind) != Some(KindClass::Type) {
                    break;
                }
                if p.kind == "namespace" && p.start_line == p.end_line {
                    // a one-line `package`/`namespace x;` declaration, not a block.
                    break;
                }
                chain.push(Descriptor::new(p.name.clone(), Suffix::Type));
                cur = parent.get(pid).copied();
            }
            chain.reverse();
            let suffix = match cg_class(&d.kind) {
                Some(KindClass::Callable) => Suffix::Method,
                Some(KindClass::Type) => Suffix::Type,
                _ => Suffix::Term,
            };
            let module = module_path(&path);
            let scheme = format!("ts-{}", lang.as_str());
            // A taken id gets the overload ordinal (`f(+1).`), which only a method renders; a
            // taken type or term id is not minted at all (it would overwrite estate's node).
            let mut ordinal = 0u32;
            let id = loop {
                let mut descriptors = vec![Descriptor::new(module.clone(), Suffix::Namespace)];
                descriptors.extend(chain.iter().cloned());
                descriptors.push(Descriptor {
                    name: d.name.clone(),
                    suffix,
                    disambiguator: (ordinal > 0).then(|| format!("+{ordinal}")),
                });
                let id = Symbol::global(scheme.clone(), None, descriptors).id();
                if !taken.contains(&id) && !map.minted.iter().any(|m| m.symbol == id) {
                    break Some(id);
                }
                if suffix != Suffix::Method || ordinal >= 64 {
                    break None;
                }
                ordinal += 1;
            };
            let Some(id) = id else {
                *self.dropped_edges.lock().unwrap_or_else(|e| e.into_inner()) += 1;
                continue;
            };
            let span = Span {
                start_byte: 0,
                end_byte: 0,
                start_line: d.start_line.saturating_sub(1),
                start_col: d.start_col,
                end_line: d.end_line.saturating_sub(1),
                end_col: d.end_col,
            };
            let mut node = Node::new(
                id.clone(),
                kind,
                d.name.clone(),
                (*lang).clone(),
                Location::new(path.clone(), span),
            );
            node.signature = d.signature.clone();
            node.metadata.insert(
                "origin".into(),
                serde_json::Value::String("codegraph".into()),
            );
            let owner = parent
                .get(d.id.as_str())
                .and_then(|p| map.ids.get(*p).cloned())
                .unwrap_or_else(|| Symbol::file(path.clone()).id());
            map.minted_edges.push(
                Edge::new(
                    owner,
                    id.clone(),
                    EdgeKind::Contains,
                    ResolutionTier::Parsed,
                    "codegraph",
                )
                .with_location(Location::new(path.clone(), span)),
            );
            map.ids.insert(d.id.clone(), id);
            map.minted.push(node);
        }
        // An ambiguous file keeps estate's pipeline: drop its mappings and mints.
        if !map.ambiguous.is_empty() {
            let amb = &map.ambiguous;
            map.minted.retain(|n| !amb.contains_key(&n.location.file));
            map.minted_edges.retain(|e| {
                e.location
                    .as_ref()
                    .is_none_or(|l| !amb.contains_key(&l.file))
            });
        }
        map
    }

    /// Whether a ref is answered by codegraph: an `Ok` file, not made ambiguous by the id map,
    /// and a codegraph-owned kind.
    pub(crate) fn owns(&self, r: &UnresolvedRef, ids: &IdMap) -> bool {
        codegraph_owned(&r.kind)
            && matches!(self.verdicts.get(&r.location.file), Some(Verdict::Ok))
            && !ids.ambiguous.contains_key(&r.location.file)
    }

    /// Codegraph's site edges for the switched files, keyed for ref matching:
    /// (estate path, 0-based line, name tail) → indices into `snap.edges`.
    fn sites(&self) -> HashMap<(String, u32, String), Vec<usize>> {
        let mut sites: HashMap<(String, u32, String), Vec<usize>> = HashMap::new();
        for (i, e) in self.snap.edges.iter().enumerate() {
            let (Some(line), Some(_)) = (e.line, map_edge_kind(&e.kind)) else {
                continue;
            };
            let Some(src) = self.snap.nodes.get(&e.source) else {
                continue;
            };
            let name = e
                .ref_name
                .as_deref()
                .map(name_tail)
                .or_else(|| self.snap.nodes.get(&e.target).map(|t| t.name.clone()))
                .unwrap_or_default();
            sites
                .entry((
                    format!("{}{}", self.prefix, src.file),
                    line.saturating_sub(1),
                    name,
                ))
                .or_default()
                .push(i);
        }
        sites
    }

    /// Whether codegraph answers for the definitions of `path`: an `Ok` file the id map did not
    /// hand back to estate. Estate's resolvers bind into every other file.
    pub(crate) fn answers_file(&self, path: &str, ids: &IdMap) -> bool {
        matches!(self.verdicts.get(path), Some(Verdict::Ok)) && !ids.ambiguous.contains_key(path)
    }

    /// Answer `refs` (all owned by codegraph, from files extracted this run) and add the
    /// codegraph-only facts of those files. Returns the site edges (one per bound ref, plus
    /// codegraph-only edges), the estate refs codegraph bound nothing for (unchanged, for
    /// estate's resolvers to try against files codegraph does not answer for), and codegraph's
    /// own unresolved rows at sites no estate ref covered (with the `codegraph:` prefix).
    pub(crate) fn resolve(
        &self,
        refs: &[UnresolvedRef],
        ids: &IdMap,
        extracted: &HashSet<String>,
    ) -> (Vec<Edge>, Vec<UnresolvedRef>, Vec<UnresolvedRef>) {
        let sites = self.sites();
        let mut consumed = self.consumed.lock().unwrap_or_else(|e| e.into_inner());
        let mut edges = Vec::new();
        let mut unbound = Vec::new();
        let mut unresolved = Vec::new();
        let mut covered: HashSet<(String, u32, String)> = HashSet::new();
        let mut dropped = 0usize;
        let key_of = |r: &UnresolvedRef| {
            (
                r.location.file.clone(),
                r.location.span.start_line,
                name_tail(&r.raw_name),
            )
        };
        // Estate refs sharing a (file, line, name) key. A key with one estate ref binds to every
        // codegraph site on it; a shared key binds each ref only to the site at its own column,
        // so `new A().run(); unknown.run()` never hands A.run to the second call.
        let mut per_key: HashMap<(String, u32, String), usize> = HashMap::new();
        for r in refs {
            *per_key.entry(key_of(r)).or_default() += 1;
        }
        for r in refs {
            let key = key_of(r);
            covered.insert(key.clone());
            // A site binds only a ref of a fitting kind: a call never takes a `references` or
            // `imports` site that shares its line and name.
            let mut hits: Vec<usize> = sites
                .get(&key)
                .map(|v| {
                    v.iter()
                        .copied()
                        .filter(|i| site_kind_fits(&r.kind, &self.snap.edges[*i].kind))
                        .collect()
                })
                .unwrap_or_default();
            let exact: Vec<usize> = hits
                .iter()
                .copied()
                .filter(|i| self.snap.edges[*i].col == Some(r.location.span.start_col))
                .collect();
            if !exact.is_empty() || per_key.get(&key).copied().unwrap_or(0) > 1 {
                hits = exact;
            }
            let mut bound = false;
            for i in hits {
                let e = &self.snap.edges[i];
                let Some(target) = ids.ids.get(&e.target) else {
                    dropped += 1;
                    continue;
                };
                if *target == r.from {
                    continue;
                }
                consumed.insert(i);
                edges.push(codegraph_edge(
                    r.from.clone(),
                    target.clone(),
                    r.kind.clone(),
                    e,
                    r.location.clone(),
                ));
                bound = true;
            }
            if !bound {
                unbound.push(r.clone());
            }
        }
        // Codegraph-only edges sourced in files extracted this run.
        for (i, e) in self.snap.edges.iter().enumerate() {
            if consumed.contains(&i) {
                continue;
            }
            let Some(kind) = map_edge_kind(&e.kind) else {
                continue;
            };
            let Some(src) = self.snap.nodes.get(&e.source) else {
                continue;
            };
            let path = format!("{}{}", self.prefix, src.file);
            if !extracted.contains(&path) || ids.ambiguous.contains_key(&path) {
                continue;
            }
            if !matches!(self.verdicts.get(&path), Some(Verdict::Ok)) {
                continue;
            }
            let (Some(source), Some(target)) = (ids.ids.get(&e.source), ids.ids.get(&e.target))
            else {
                dropped += 1;
                continue;
            };
            if source == target {
                continue;
            }
            let span = Span {
                start_byte: 0,
                end_byte: 0,
                start_line: e.line.unwrap_or(0).saturating_sub(1),
                start_col: e.col.unwrap_or(0),
                end_line: e.line.unwrap_or(0).saturating_sub(1),
                end_col: e.col.unwrap_or(0),
            };
            edges.push(codegraph_edge(
                source.clone(),
                target.clone(),
                kind,
                e,
                Location::new(path, span),
            ));
        }
        // Codegraph's unresolved rows at sites no estate ref covered.
        for u in &self.snap.unresolved {
            let path = format!("{}{}", self.prefix, u.file);
            if !extracted.contains(&path) || ids.ambiguous.contains_key(&path) {
                continue;
            }
            let Some(kind) = map_edge_kind(&u.kind) else {
                continue;
            };
            let key = (path.clone(), u.line.saturating_sub(1), name_tail(&u.name));
            if covered.contains(&key) {
                continue;
            }
            let Some(from) = ids.ids.get(&u.from) else {
                continue;
            };
            covered.insert(key);
            let span = Span {
                start_byte: 0,
                end_byte: 0,
                start_line: u.line.saturating_sub(1),
                start_col: u.col,
                end_line: u.line.saturating_sub(1),
                end_col: u.col,
            };
            unresolved.push(UnresolvedRef {
                from: from.clone(),
                raw_name: format!("{CODEGRAPH_REF_PREFIX}{}", u.name),
                kind,
                location: Location::new(path, span),
                hints: Default::default(),
            });
        }
        *self.dropped_edges.lock().unwrap_or_else(|e| e.into_inner()) += dropped;
        (edges, unbound, unresolved)
    }
}

/// Park a ref no resolver bound: the `codegraph:` prefix keeps every estate resolver, back-fill
/// included, from name-binding it later (`routed_elsewhere`).
pub(crate) fn park(r: &UnresolvedRef) -> UnresolvedRef {
    let mut parked = r.clone();
    parked.raw_name = format!("{CODEGRAPH_REF_PREFIX}{}", r.raw_name);
    parked.hints = Default::default();
    parked
}

/// Whether a codegraph site edge of kind `cg` can answer an estate ref of kind `estate`.
fn site_kind_fits(estate: &EdgeKind, cg: &str) -> bool {
    let call = matches!(cg, "calls" | "instantiates");
    match estate {
        EdgeKind::Calls | EdgeKind::Instantiates => call,
        EdgeKind::Imports => cg == "imports",
        _ => !call && cg != "imports",
    }
}

/// Whether a codegraph definition estate lacks becomes an estate node. Not minted: a one-line
/// `namespace`/`module` (a file-level `package p;` declaration: estate's ids do not nest under
/// it), and a name that is not an identifier (an anonymous class `<T$anon@84>`, an annotation
/// value `local.server.port`): estate never names a definition that way.
fn mintable(d: &CgNode) -> bool {
    if is_package_declaration(d) {
        return false;
    }
    let mut chars = d.name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// A one-line `namespace`/`module`: a file-level declaration (`package p;`, `namespace X;`), not a
/// block that contains definitions.
fn is_package_declaration(d: &CgNode) -> bool {
    matches!(d.kind.as_str(), "namespace" | "module") && d.start_line == d.end_line
}

/// The last `#` (type) descriptor of an estate id: the owner type of a member.
fn estate_owner(id: &SymbolId) -> Option<String> {
    let s = id.0.as_str();
    let body = s.rsplit(' ').next()?;
    let mut parts: Vec<&str> = body.split('#').collect();
    parts.pop()?;
    let owner = parts.pop()?;
    Some(owner.rsplit('/').next().unwrap_or(owner).to_string())
}

fn module_path(path: &str) -> String {
    let (dir, file) = match path.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, path),
    };
    let stem = match file.rsplit_once('.') {
        Some((s, _)) => s,
        None => file,
    };
    match dir {
        Some(d) => format!("{d}/{stem}"),
        None => stem.to_string(),
    }
}

fn codegraph_edge(
    source: SymbolId,
    target: SymbolId,
    kind: EdgeKind,
    e: &CgEdge,
    location: Location,
) -> Edge {
    let confidence = e.confidence.unwrap_or(0.5);
    // The tier names codegraph's method, never its confidence: only an import or a file path
    // is an import-map resolution; name, receiver, qualified-name and framework matches are
    // heuristics (codegraph's confidence still rides on the edge).
    let tier = match e.resolved_by.as_deref() {
        Some("import" | "file-path") => ResolutionTier::ImportMap,
        _ => ResolutionTier::Heuristic,
    };
    let mut edge = Edge::new(
        source,
        target,
        kind,
        tier,
        format!(
            "codegraph:{}",
            e.resolved_by.as_deref().unwrap_or("unknown")
        ),
    )
    .with_location(location);
    edge.confidence = Confidence::new(confidence as f32);
    edge
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::GraphRead;
    use wicked_estate_store::{GraphStoreMutExt, SqliteStore};

    const FIXTURE_SQL: &str = include_str!("../tests/fixtures/codegraph/codegraph-1.6.4.sql");

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codegraph/repo")
    }

    fn config(state: &Path) -> CodegraphConfig {
        CodegraphConfig {
            bundle: state.join("no-bundle"),
            state_dir: state.join("cg"),
            languages: Vec::new(),
            timeout: Duration::from_secs(5),
        }
    }

    /// Index `root` into `store` with the recorded codegraph database standing in for the run
    /// (`sql`), or with codegraph off (`cfg = None`).
    fn index(
        store: &mut SqliteStore,
        root: &Path,
        cfg: Option<&CodegraphConfig>,
        sql: Option<&str>,
    ) {
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = sql.map(str::to_string));
        crate::index_path_with(store, root, None, cfg).unwrap();
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = None);
    }

    fn edges(store: &SqliteStore, kind: EdgeKind) -> Vec<(String, String, String)> {
        let mut out: Vec<_> = store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == kind)
            .map(|e| (e.source.0, e.target.0, e.resolved_by))
            .collect();
        out.sort();
        out
    }

    fn calls_from<'a>(
        edges: &'a [(String, String, String)],
        source: &str,
    ) -> Vec<&'a (String, String, String)> {
        edges.iter().filter(|(s, _, _)| s == source).collect()
    }

    const GET: &str = "ts-typescript . . . src/svc/UserService#get().";
    const LIB_HELPER: &str = "ts-typescript . . . src/lib/util/helper().";
    const DECOY_HELPER: &str = "ts-typescript . . . src/util/helper().";

    #[test]
    fn codegraph_binds_a_tsconfig_alias_import_to_the_configured_file_only() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        let calls = edges(&store, EdgeKind::Calls);
        let from_get = calls_from(&calls, GET);
        assert!(
            from_get
                .iter()
                .any(|(_, t, by)| t == LIB_HELPER && by.starts_with("codegraph:")),
            "get() must call the helper the tsconfig alias names: {from_get:?}"
        );
        assert!(
            !from_get.iter().any(|(_, t, _)| t == DECOY_HELPER),
            "no name-only edge to the same-named decoy: {from_get:?}"
        );
    }

    #[test]
    fn same_line_members_keep_their_two_estate_ids_and_both_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        let ids: HashSet<String> = store
            .all_nodes()
            .unwrap()
            .into_iter()
            .map(|n| n.symbol.0)
            .collect();
        let a_run = "ts-typescript . . . src/same/A#run().";
        let b_run = "ts-typescript . . . src/same/B#run().";
        assert!(ids.contains(a_run) && ids.contains(b_run), "{ids:?}");
        let calls = edges(&store, EdgeKind::Calls);
        let both = calls_from(&calls, "ts-typescript . . . src/same/both().");
        for target in [a_run, b_run] {
            assert!(
                both.iter()
                    .any(|(_, t, by)| t == target && by.starts_with("codegraph:")),
                "both() must call {target}: {both:?}"
            );
        }
    }

    #[test]
    fn a_same_line_call_codegraph_left_unresolved_never_takes_its_neighbours_target() {
        // Codex r1 (W20): codegraph answered `new A().run()` but not `new B().run()` on the same
        // line. The second call must park, not inherit A.run through the shared (line, name) key.
        let sql = format!("{FIXTURE_SQL}\nDELETE FROM edges WHERE id = 29;");
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(&sql));
        let calls = edges(&store, EdgeKind::Calls);
        let both = calls_from(&calls, "ts-typescript . . . src/same/both().");
        let a_run = "ts-typescript . . . src/same/A#run().";
        assert_eq!(
            both.iter().filter(|(_, t, _)| t == a_run).count(),
            1,
            "only the A call binds A.run: {both:?}"
        );
        assert!(
            !both
                .iter()
                .any(|(_, t, _)| t == "ts-typescript . . . src/same/B#run()."),
            "{both:?}"
        );
        let parked = store
            .unresolved_refs_for_name(&format!("{CODEGRAPH_REF_PREFIX}run"))
            .unwrap();
        assert_eq!(parked.len(), 1, "the B call parks: {parked:?}");
        assert_eq!(parked[0].location.file, "src/same.ts");
    }

    #[test]
    fn a_definition_estate_lacks_gets_an_estate_scheme_id_and_its_edges() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        let nodes = store.all_nodes().unwrap();
        let point = nodes
            .iter()
            .find(|n| n.name == "Point" && n.kind == NodeKind::Class)
            .expect("the record Point is a node");
        assert_eq!(point.symbol.0, "ts-java . . . src/p/Shapes/Shapes#Point#");
        assert_eq!(point.location.file, "src/p/Shapes.java");
        assert_eq!(point.location.span.start_line, 4);
        let all = store.all_edges().unwrap();
        assert!(
            all.iter().any(|e| e.kind == EdgeKind::Contains
                && e.source.0 == "ts-java . . . src/p/Shapes/Shapes#"
                && e.target == point.symbol),
            "Shapes contains Point"
        );
        assert!(
            all.iter()
                .any(|e| e.source.0 == "ts-java . . . src/p/Shapes/Shapes#use()."
                    && e.target == point.symbol),
            "use() reaches the minted Point"
        );
        // A definition estate already has keeps estate's id: no second `g` node.
        assert_eq!(nodes.iter().filter(|n| n.name == "g").count(), 1);
    }

    #[test]
    fn a_site_codegraph_left_unresolved_parks_with_the_prefix_and_never_binds_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        let parked = store
            .unresolved_refs_for_name(&format!("{CODEGRAPH_REF_PREFIX}trim"))
            .unwrap();
        assert_eq!(parked.len(), 1, "{parked:?}");
        assert_eq!(parked[0].location.file, "src/svc.ts");
        // A second, unchanged run re-extracts nothing and back-fill binds nothing new.
        let before = store.all_edges().unwrap().len();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        assert_eq!(store.all_edges().unwrap().len(), before);
        assert_eq!(
            store
                .unresolved_refs_for_name(&format!("{CODEGRAPH_REF_PREFIX}trim"))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn no_production_resolver_binds_a_codegraph_parked_ref() {
        use wicked_estate_core::Resolver;
        let file = "src/a.ts";
        let target = Node::new(
            SymbolId("ts-typescript . . . src/b/helper().".into()),
            NodeKind::Function,
            "helper",
            Language::new("typescript"),
            Location::new("src/b.ts", Span::ZERO),
        );
        let mut mem = wicked_estate_store::MemStore::new();
        wicked_estate_core::GraphWrite::upsert_nodes(&mut mem, &[target]).unwrap();
        let index = crate::InMemoryIndex::build(&mem, None).unwrap();
        let r = UnresolvedRef {
            from: Symbol::file(file).id(),
            raw_name: format!("{CODEGRAPH_REF_PREFIX}helper"),
            kind: EdgeKind::Calls,
            location: Location::new(file, Span::ZERO),
            hints: Default::default(),
        };
        let relative = wicked_estate_resolve::RelativeImportResolver::new(None);
        let resolvers: &[&dyn Resolver] = &[
            &wicked_estate_resolve::NameResolver,
            &wicked_estate_resolve::ScopedNameResolver,
            &wicked_estate_resolve::ImportMapResolver,
            &relative,
            &wicked_estate_resolve::InfraResolver,
            &wicked_estate_resolve::RulesBridgeResolver,
            &wicked_estate_resolve::DynamicCallResolver,
            &wicked_estate_resolve::ApexlangRefResolver,
        ];
        for resolver in resolvers {
            assert!(
                resolver
                    .resolve(std::slice::from_ref(&r), &index)
                    .unwrap()
                    .is_empty(),
                "{} bound a codegraph-parked ref",
                resolver.id()
            );
        }
    }

    #[test]
    fn a_file_codegraph_reports_errors_for_keeps_estates_pipeline() {
        // The plan-v2-r2 probe: a file grown past 1 MiB keeps its `files` row, zero nodes and a
        // `size_exceeded` error after a successful sync.
        let sql = format!(
            "{FIXTURE_SQL}\nUPDATE files SET errors = '[{{\"message\":\"too big\",\"severity\":\"error\",\"code\":\"size_exceeded\"}}]', node_count = 0 WHERE path = 'src/svc.ts';\n\
             DELETE FROM edges WHERE source IN (SELECT id FROM nodes WHERE file_path = 'src/svc.ts');\n\
             DELETE FROM unresolved_refs WHERE file_path = 'src/svc.ts';\n\
             DELETE FROM nodes WHERE file_path = 'src/svc.ts';"
        );
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(&sql));
        let calls = edges(&store, EdgeKind::Calls);
        let get2 = calls_from(&calls, "ts-typescript . . . src/svc/UserService#get2().");
        assert!(
            get2.iter()
                .any(|(_, t, by)| t == GET && !by.starts_with("codegraph:")),
            "svc.ts keeps estate's own resolution: {get2:?}"
        );
        assert!(
            calls
                .iter()
                .all(|(s, _, by)| !s.contains("src/svc/") || !by.starts_with("codegraph:")),
            "no codegraph edge sourced in the skipped file"
        );
        // The other files still take codegraph's facts.
        assert!(calls.iter().any(|(_, _, by)| by.starts_with("codegraph:")));
    }

    fn try_index(
        store: &mut SqliteStore,
        root: &Path,
        cfg: Option<&CodegraphConfig>,
        sql: Option<&str>,
    ) -> wicked_estate_core::Result<()> {
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = sql.map(str::to_string));
        let out = crate::index_path_with(store, root, None, cfg).map(|_| ());
        TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = None);
        out
    }

    #[test]
    fn a_failed_run_keeps_the_last_good_graph_and_only_switching_off_hands_files_back() {
        // Coord r2 F3 (W12): one failed run must not move the whole repo to the other producer
        // (that deletes every minted node and leaves knowledge links dangling).
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        let fingerprints = store.meta_get_key(FINGERPRINTS_META);
        assert!(fingerprints.as_deref().is_some_and(|v| !v.is_empty()));
        let point = "ts-java . . . src/p/Shapes/Shapes#Point#";
        let graph = |store: &SqliteStore| {
            let mut nodes: Vec<String> = store
                .all_nodes()
                .unwrap()
                .into_iter()
                .map(|n| n.symbol.0)
                .collect();
            nodes.sort();
            (nodes, edges(store, EdgeKind::Calls))
        };
        let before = graph(&store);
        assert!(before.0.iter().any(|n| n == point));
        // Codegraph fails with codegraph state in the store: the index stops, loudly, and the
        // graph is the last good one.
        let err = try_index(&mut store, &fixture_root(), Some(&cfg), Some("FAIL")).unwrap_err();
        assert!(err.to_string().contains("CODEGRAPH-FAILED"), "{err}");
        assert_eq!(graph(&store), before);
        assert_eq!(store.meta_get_key(FINGERPRINTS_META), fingerprints);
        // An explicit switch-off hands every file back to estate's resolvers, once.
        index(&mut store, &fixture_root(), None, None);
        let calls = edges(&store, EdgeKind::Calls);
        assert!(!calls.is_empty());
        assert!(
            calls.iter().all(|(_, _, by)| !by.starts_with("codegraph:")),
            "{calls:?}"
        );
        assert!(!graph(&store).0.iter().any(|n| n == point));
        assert_eq!(store.meta_get_key(FINGERPRINTS_META).as_deref(), Some(""));
        // With no codegraph state stored, a failed run falls back to estate's pipeline.
        index(&mut store, &fixture_root(), Some(&cfg), Some("FAIL"));
        assert!(
            edges(&store, EdgeKind::Calls)
                .iter()
                .all(|(_, _, by)| !by.starts_with("codegraph:"))
        );
    }

    #[test]
    fn callers_of_a_file_that_fell_back_keep_their_edges_into_it() {
        // Coord r2 F1 (W16): src/lib/util.ts grew past the cap (size_exceeded, no nodes). Its
        // caller src/svc.ts is still codegraph's; the call to helper must bind into the fallen
        // back file through estate's resolvers, and never to the decoy codegraph answers for.
        let sql = format!(
            "{FIXTURE_SQL}\nUPDATE files SET errors = '[{{\"message\":\"too big\",\"severity\":\"error\",\"code\":\"size_exceeded\"}}]', node_count = 0 WHERE path = 'src/lib/util.ts';\n\
             DELETE FROM edges WHERE source IN (SELECT id FROM nodes WHERE file_path = 'src/lib/util.ts') OR target IN (SELECT id FROM nodes WHERE file_path = 'src/lib/util.ts');\n\
             DELETE FROM unresolved_refs WHERE file_path = 'src/lib/util.ts';\n\
             DELETE FROM nodes WHERE file_path = 'src/lib/util.ts';"
        );
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(&sql));
        let calls = edges(&store, EdgeKind::Calls);
        let from_get = calls_from(&calls, GET);
        assert!(
            from_get
                .iter()
                .any(|(_, t, by)| t == LIB_HELPER && !by.starts_with("codegraph:")),
            "get() keeps its call into the fallen-back file: {from_get:?}"
        );
        assert!(
            !from_get.iter().any(|(_, t, _)| t == DECOY_HELPER),
            "{from_get:?}"
        );
        assert!(
            store
                .unresolved_refs_for_name(&format!("{CODEGRAPH_REF_PREFIX}helper"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_import_of_a_file_codegraph_never_sees_keeps_its_edge() {
        // Coord r2 F2 (W12): `import './styles.css'` has no codegraph site (the mirror holds only
        // switched sources), so it goes to estate's resolvers instead of parking.
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap() {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    copy(&e.path(), &to.join(e.file_name()));
                } else {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        copy(&fixture_root(), &root);
        let svc = std::fs::read_to_string(root.join("src/svc.ts")).unwrap();
        std::fs::write(
            root.join("src/svc.ts"),
            format!("{svc}import './styles.css';\n"),
        )
        .unwrap();
        std::fs::write(root.join("src/styles.css"), ".user { color: red; }\n").unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &root, Some(&cfg), Some(FIXTURE_SQL));
        let imports = edges(&store, EdgeKind::Imports);
        assert!(
            imports
                .iter()
                .any(|(_, t, by)| t.contains("src/styles.css") && !by.starts_with("codegraph:")),
            "{imports:?}"
        );
        assert!(
            store
                .unresolved_refs_for_name(&format!("{CODEGRAPH_REF_PREFIX}'./styles.css'"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_site_binds_only_a_ref_of_a_fitting_kind_and_the_tier_names_the_method() {
        // Coord r2 L9, L10.
        assert!(site_kind_fits(&EdgeKind::Calls, "calls"));
        assert!(site_kind_fits(&EdgeKind::Calls, "instantiates"));
        assert!(!site_kind_fits(&EdgeKind::Calls, "references"));
        assert!(!site_kind_fits(&EdgeKind::Calls, "imports"));
        assert!(site_kind_fits(&EdgeKind::Imports, "imports"));
        assert!(!site_kind_fits(&EdgeKind::References, "calls"));
        assert!(site_kind_fits(&EdgeKind::References, "type_of"));
        let cg = |by: &str| CgEdge {
            source: "a".into(),
            target: "b".into(),
            kind: "calls".into(),
            confidence: Some(0.95),
            resolved_by: Some(by.into()),
            ref_name: None,
            line: Some(1),
            col: Some(0),
        };
        let at = Location::new("a.ts", Span::ZERO);
        let tier = |by: &str| {
            codegraph_edge(
                SymbolId("a".into()),
                SymbolId("b".into()),
                EdgeKind::Calls,
                &cg(by),
                at.clone(),
            )
            .provenance
        };
        use wicked_estate_core::Provenance;
        assert_eq!(tier("import"), Provenance::ImportMap);
        assert_eq!(tier("instance-method"), Provenance::Heuristic);
        assert_eq!(tier("framework"), Provenance::Heuristic);
    }

    #[test]
    fn narrowing_the_languages_returns_the_dropped_files_to_estates_resolvers() {
        // Codex r1 (W20): a file codegraph answered last run whose language is no longer
        // selected must not keep codegraph's edges after an unchanged re-index.
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        let ts_from_codegraph = |store: &SqliteStore| {
            edges(store, EdgeKind::Calls)
                .into_iter()
                .filter(|(s, _, by)| s.starts_with("ts-typescript") && by.starts_with("codegraph:"))
                .count()
        };
        assert!(ts_from_codegraph(&store) > 0);
        let java_only = CodegraphConfig {
            languages: vec!["java".into()],
            ..config(tmp.path())
        };
        index(
            &mut store,
            &fixture_root(),
            Some(&java_only),
            Some(FIXTURE_SQL),
        );
        assert_eq!(ts_from_codegraph(&store), 0);
        let fps = parse_fingerprints(store.meta_get_key(FINGERPRINTS_META).as_deref());
        assert!(fps.keys().all(|p| p.ends_with(".java")), "{fps:?}");
        // Java stays answered by codegraph.
        assert!(
            edges(&store, EdgeKind::Calls)
                .iter()
                .any(|(s, _, by)| s.starts_with("ts-java") && by.starts_with("codegraph:"))
                || store
                    .all_edges()
                    .unwrap()
                    .iter()
                    .any(|e| e.source.0.starts_with("ts-java")
                        && e.resolved_by.starts_with("codegraph:"))
        );
    }

    #[test]
    fn an_unchanged_file_whose_codegraph_facts_moved_is_re_resolved() {
        // A sync can change an unchanged file's edges (a target file changed under it). Estate
        // is the one sync owner: the file's fingerprint moved, so it re-extracts and re-binds.
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &fixture_root(), Some(&cfg), Some(FIXTURE_SQL));
        let has_edge = |store: &SqliteStore| {
            edges(store, EdgeKind::Calls)
                .iter()
                .any(|(s, t, _)| s == GET && t == LIB_HELPER)
        };
        assert!(has_edge(&store));
        let moved = format!(
            "{FIXTURE_SQL}\nDELETE FROM edges WHERE kind = 'calls' AND line = 3 AND source IN \
             (SELECT id FROM nodes WHERE file_path = 'src/svc.ts');"
        );
        index(&mut store, &fixture_root(), Some(&cfg), Some(&moved));
        assert!(
            !has_edge(&store),
            "svc.ts was re-resolved against the new facts"
        );
        assert_eq!(
            store
                .unresolved_refs_for_name(&format!("{CODEGRAPH_REF_PREFIX}helper"))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn the_mirror_holds_redacted_text_and_follows_deletions() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = config(tmp.path());
        std::fs::create_dir_all(&cfg.state_dir).unwrap();
        let secret = "hunter2-correct-horse-battery";
        let source = format!("export const password = \"{secret}\";\nexport function f() {{}}\n");
        let read = |raw: &str| -> Option<String> {
            (raw == "src/config.ts")
                .then(|| wicked_estate_extract::redact::redact_source(raw, &source))
        };
        let files = vec![MirrorFile {
            raw: "src/config.ts".into(),
            digest: "d1".into(),
        }];
        sync_mirror(&cfg, &files, &[], &read).unwrap();
        let mirrored = std::fs::read_to_string(cfg.mirror_root().join("src/config.ts")).unwrap();
        assert!(!mirrored.contains(secret), "{mirrored}");
        assert_eq!(mirrored.len(), source.len());
        assert_eq!(mirrored.lines().count(), source.lines().count());
        sync_mirror(&cfg, &[], &[], &read).unwrap();
        assert!(!cfg.mirror_root().join("src/config.ts").exists());
    }

    #[test]
    fn a_minified_file_estate_does_not_extract_is_mirrored_empty() {
        // Codegraph's facts about a vendored bundle have no estate node to land on.
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap() {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    copy(&e.path(), &to.join(e.file_name()));
                } else {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        copy(&fixture_root(), &root);
        let minified = format!("var a=1;{}\n", "a=a+1;".repeat(10_000));
        std::fs::create_dir_all(root.join("src/vendor")).unwrap();
        std::fs::write(root.join("src/vendor/lib.min.js"), &minified).unwrap();
        // Codex delta (W20): a resolver config on one long line is not minified source; blanking
        // it drops the aliases codegraph resolves imports with.
        let padding: Vec<String> = (0..120)
            .map(|i| format!("\"@pad{i}/*\": [\"src/pad{i}/*\"]"))
            .collect();
        let tsconfig = format!(
            "{{\"compilerOptions\":{{\"baseUrl\":\".\",\"paths\":{{\"@lib/*\":[\"src/lib/*\"],{}}}}}}}",
            padding.join(",")
        );
        assert!(crate::is_minified_or_huge(&tsconfig));
        std::fs::write(root.join("tsconfig.json"), &tsconfig).unwrap();
        let cfg = config(tmp.path());
        let mut store = SqliteStore::open(tmp.path().join("g.db")).unwrap();
        index(&mut store, &root, Some(&cfg), Some(FIXTURE_SQL));
        let tree = cfg.state_dir.join("tree");
        assert_eq!(
            std::fs::read_to_string(tree.join("src/vendor/lib.min.js")).unwrap(),
            ""
        );
        assert!(
            !std::fs::read_to_string(tree.join("src/svc.ts"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read_to_string(tree.join("tsconfig.json")).unwrap(),
            tsconfig
        );
    }

    #[test]
    fn a_bundle_that_is_not_the_pinned_release_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = config(tmp.path());
        cfg.bundle = tmp.path().join("bundle");
        std::fs::create_dir_all(cfg.bundle.join("lib")).unwrap();
        std::fs::create_dir_all(&cfg.state_dir).unwrap();
        std::fs::write(
            cfg.bundle.join("lib/package.json"),
            r#"{"version":"1.6.3"}"#,
        )
        .unwrap();
        assert!(
            verify_bundle(&cfg)
                .unwrap_err()
                .contains("not the pinned 1.6.4")
        );
        std::fs::write(
            cfg.bundle.join("lib/package.json"),
            r#"{"version":"1.6.4"}"#,
        )
        .unwrap();
        // Right version, wrong files: the tree digest refuses it (or the platform is unpinned).
        assert!(verify_bundle(&cfg).is_err());
    }

    #[test]
    fn a_state_dir_the_walker_would_index_is_refused_and_wicked_estate_is_not() {
        // Codex r1 (W12) and its delta (W20): the mirror under a state dir the walker reaches
        // (a plain dir, or a dot dir an `.ignore` whitelist re-opens) would be indexed as a
        // second copy of every switched source.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let read = |_: &str| -> Option<String> { None };
        let run = |state_dir: PathBuf| {
            let cfg = CodegraphConfig {
                state_dir,
                ..config(tmp.path())
            };
            TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = Some(FIXTURE_SQL.to_string()));
            let out = prepare(
                &cfg,
                PrepareInput {
                    root: &root,
                    prefix: "",
                    files: Vec::new(),
                    configs: Vec::new(),
                    read: &read,
                    unchanged: Vec::new(),
                    stored: BTreeMap::new(),
                },
            )
            .map(|_| ());
            TEST_DATABASE_SQL.with(|t| *t.borrow_mut() = None);
            out
        };
        for inside in ["out/graph.db.codegraph/_", ".cache/graph.db.codegraph/_"] {
            let err = run(root.join(inside)).unwrap_err();
            assert!(err.contains("inside the indexed root"), "{err}");
            assert!(!root.join(inside).join("tree").exists());
        }
        run(root.join(".wicked-estate/graph.db.codegraph/_")).unwrap();
        // Coord r2 L4: the state dir ignores itself.
        assert_eq!(
            std::fs::read_to_string(root.join(".wicked-estate/graph.db.codegraph/_/.gitignore"))
                .unwrap(),
            "*\n"
        );
        run(tmp.path().join("elsewhere/_")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_same_size_edit_with_its_mtime_restored_still_misses_the_bundle_cache() {
        // Codex r1 (W20): the cache key was size + newest mtime, so `touch -r` hid an edit.
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("lib/dist/x.js");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "aaaa").unwrap();
        let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
        let before = bundle_stamp(tmp.path()).unwrap().unwrap();
        assert_eq!(bundle_stamp(tmp.path()).unwrap().unwrap(), before);
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&file, "bbbb").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), mtime);
        assert_ne!(bundle_stamp(tmp.path()).unwrap().unwrap(), before);
    }

    #[test]
    fn name_tail_strips_members_and_routing_prefixes() {
        assert_eq!(name_tail("this.repo.save"), "save");
        assert_eq!(name_tail("helper().trim"), "trim");
        assert_eq!(name_tail("local-binding:Card:ts-x . . . a/Card()."), "Card");
        assert_eq!(name_tail("import-alias:h:helper"), "h");
        assert_eq!(name_tail("Foo::bar"), "bar");
        assert_eq!(name_tail("./util"), "util");
        assert_eq!(name_tail("'./util'"), "util");
        assert_eq!(name_tail("\"@lib/util\""), "util");
        assert_eq!(name_tail("<stdio.h>"), name_tail("stdio.h"));
    }

    #[test]
    fn estate_owner_is_the_last_type_descriptor() {
        let id = SymbolId("ts-java . . . src/A/A#R#g().".into());
        assert_eq!(estate_owner(&id).as_deref(), Some("R"));
        let top = SymbolId("ts-typescript . . . src/svc/top().".into());
        assert_eq!(estate_owner(&top), None);
    }

    #[test]
    fn package_declarations_and_non_identifier_names_are_never_minted() {
        let def = |kind: &str, name: &str, start: u32, end: u32| CgNode {
            id: "x".into(),
            kind: kind.into(),
            name: name.into(),
            qualified_name: name.into(),
            file: "a.java".into(),
            start_line: start,
            end_line: end,
            start_col: 0,
            end_col: 0,
            signature: None,
        };
        // Petclinic (real bundle run): 50 `package` namespaces and 3 anonymous classes.
        assert!(!mintable(&def("namespace", "org.example", 1, 1)));
        assert!(!mintable(&def("class", "<PetType$anon@84>", 84, 90)));
        assert!(!mintable(&def("constant", "local.server.port", 3, 3)));
        assert!(mintable(&def("namespace", "Outer", 1, 9)));
        assert!(mintable(&def("class", "Point", 5, 5)));
        assert!(mintable(&def("method", "Über_$1", 5, 5)));
    }

    #[test]
    fn module_path_drops_only_the_extension() {
        assert_eq!(module_path("src/a.b/c.ts"), "src/a.b/c");
        assert_eq!(module_path("x.java"), "x");
    }
}
