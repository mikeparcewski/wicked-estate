//! Per-query freshness (wicked-estate#351, agent rule R5): the ONE staleness implementation every
//! frontend uses.
//!
//! A retrieval tool sees only `&dyn GraphRead`, so it emits the
//! [`STALENESS_PLACEHOLDER`](wicked_estate_retrieve::STALENESS_PLACEHOLDER) cue and the
//! TRANSPORT states freshness. Both transports call [`FreshnessProbe::annotate`] on every
//! retrieval response — the MCP server after its response-cache lookup (hit or miss: the cache
//! stores un-annotated content), the CLI bridge after `invoke` — so MCP and CLI carry the same
//! facts, measured at call time:
//!
//! * **commits behind, per indexed root**, from the commit that root was indexed at (#243).
//!   HEAD is read from the git files every call (no process; exact), and the `rev-list --count`
//!   behind it is cached on `(root, HEAD, graph_version)`, so a commit or a re-index is seen on
//!   the next call;
//! * **drift of the files THIS answer touches**: every string under a key named `file` in the
//!   content is compared with the bytes that were indexed (`files.digest`, xxh3 — the same
//!   `file_digest` the indexer writes). Missing on disk → `deleted`; different bytes →
//!   `modified`. Comparing against indexed bytes, not HEAD, makes both directions right: an
//!   uncommitted edit is stale until re-indexed, a graph ahead of HEAD that matches disk is
//!   fresh, and a dirty file the answer does not touch never marks it;
//! * **new source files the graph has never seen** (R3): untracked or newly added files under a
//!   root whose extension a `languages.toml` row claims and that have no digest row — an answer
//!   cannot cite a caller in a file that was never indexed. The list comes from one `git status`
//!   per root, re-run only when HEAD, the git index or a directory holding an indexed file
//!   changes, and at least every `STATUS_MAX_AGE` (10 s) — so a warm call spawns no process
//!   (`docs/benchmarks/freshness-overhead.md`).
//!
//! Additive output: a `freshness` block in the content, `"stale"` on each row whose `file`
//! drifted, and `STALENESS:` diagnostics. Any probe failure degrades to "unknown", never an error
//! (R1). The probe never writes (R5 is a reporting rule).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use wicked_estate_store::GraphStoreMutExt;

use crate::{commits_behind_since, indexed_root_path, recorded_root_path, repo_scope};

/// At most this many entries in `freshness.stale_files` (the total is always reported).
pub const STALE_FILES_CAP: usize = 20;

/// Room a caller that sizes a reply to the R4 budget must leave for the `freshness` block in
/// its compact form (counts kept, detail lists emptied) — what [`FreshnessProbe::annotate`]
/// falls back to when the full block would not fit.
pub const BLOCK_RESERVE: usize = 400;

/// What can be said about a db's freshness, per indexed root — the commit-level half of R5.
#[derive(Debug, Default, Clone)]
pub struct StalenessReport {
    /// One line per root with commits since its last index.
    pub stale: Vec<String>,
    /// Roots whose freshness could NOT be determined (no git history at the root, a db spec
    /// with no file mtime, …), named by repo label or root path. The cause is not guessed.
    pub unknown: Vec<String>,
    /// Roots whose commits-behind count was read, stale or not.
    pub checked: usize,
    /// The worst count over every checked root — the one number machine output reports (R5).
    pub worst: Option<u64>,
}

impl StalenessReport {
    /// Exactly the freshness statements a caller should see (agent rule R5): a positive
    /// "current" claim only when EVERY indexed root was checked and none is behind — partial
    /// coverage presented as complete is the R3 failure.
    pub fn statements(&self, db: &str) -> Vec<String> {
        let mut out = self.stale.clone();
        for what in &self.unknown {
            out.push(format!(
                "STALENESS: unknown for {what} — freshness could not be determined"
            ));
        }
        if self.checked == 0 && self.unknown.is_empty() {
            out.push(format!(
                "STALENESS: unknown — {db} records no indexed root to check"
            ));
        } else if out.is_empty() {
            out.push("STALENESS: 0 commits since last index".to_string());
        }
        out
    }
}

/// One indexed root as the probe sees it.
#[derive(Debug, Clone)]
struct Root {
    /// `None` for a single-repo (un-labelled) graph.
    label: Option<String>,
    /// The on-disk root (recorded root resolved against the db's directory, #248).
    path: PathBuf,
    /// The spelling recorded in the graph, for the fix hint.
    recorded: String,
    /// The commit the root was indexed at.
    indexed: Option<String>,
}

/// Commit-level freshness of every root, computed fresh (no cache). The bespoke CLI arms use this
/// for their header line; retrieval responses go through [`FreshnessProbe::annotate`].
pub fn staleness_report(store: &dyn GraphStoreMutExt, db: &str) -> StalenessReport {
    let mut report = StalenessReport::default();
    for root in roots(store, db) {
        let n = commits_behind_since(&root.path, root.indexed.as_deref(), db);
        record(&mut report, &root, n);
    }
    report
}

fn record(report: &mut StalenessReport, root: &Root, n: Option<u64>) {
    match n {
        Some(n) => {
            report.checked += 1;
            report.worst = Some(report.worst.map_or(n, |w| w.max(n)));
            if n > 0 {
                report.stale.push(match &root.label {
                    // A multi-repo graph names the label so the operator re-indexes THAT repo.
                    Some(label) => format!(
                        "STALENESS: {n} commit(s) in '{label}' since last index — run \
                         `wicked-estate index {} --repo {label}` to refresh",
                        root.recorded
                    ),
                    None => format!(
                        "STALENESS: {n} commit(s) since last index — run `wicked-estate index \
                         {}` to refresh",
                        root.recorded
                    ),
                });
            }
        }
        None => report.unknown.push(match &root.label {
            Some(label) => format!("repo '{label}'"),
            None => root.recorded.clone(),
        }),
    }
}

/// Every indexed root: the repo registry of a multi-repo graph, else the single `indexed_root`.
fn roots(store: &dyn GraphStoreMutExt, db: &str) -> Vec<Root> {
    let repos = repo_scope::registry(store);
    if !repos.is_empty() {
        return repos
            .into_iter()
            .map(|rec| Root {
                path: recorded_root_path(&rec.root, db),
                recorded: rec.root,
                label: Some(rec.label),
                indexed: rec.info.commit,
            })
            .collect();
    }
    let Some(path) = indexed_root_path(store, db) else {
        return vec![];
    };
    vec![Root {
        recorded: path.to_string_lossy().into_owned(),
        path,
        label: None,
        indexed: store.repo_info().ok().flatten().and_then(|i| i.commit),
    }]
}

/// `git -C <root> <args>` → stdout, `None` on any failure. `GIT_OPTIONAL_LOCKS=0`: a probe
/// must never take `index.lock` (an opportunistic `git status` index refresh would race the
/// user's own git commands).
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    String::from_utf8(out.stdout).ok()
}

/// `(mtime, len)` of a path, `None` when it cannot be read.
fn stamp(p: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

/// The git directory of the work tree holding `root`, found the way git does: the first
/// ancestor with a `.git` directory, or a `.git` file naming one (`gitdir: …`, a linked
/// worktree). Discovery never walks into a `GIT_CEILING_DIRECTORIES` entry.
fn discover_git_dir(root: &Path) -> Option<PathBuf> {
    let ceilings: Vec<PathBuf> = std::env::var_os("GIT_CEILING_DIRECTORIES")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let ceilings: Vec<PathBuf> = ceilings.iter().map(|c| canon(c)).collect();
    let start = canon(root);
    for dir in start.ancestors() {
        if dir != start && ceilings.iter().any(|c| c == dir) {
            return None;
        }
        let dot = dir.join(".git");
        if dot.is_dir() {
            return Some(dot);
        }
        if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim();
            let p = Path::new(target);
            return Some(if p.is_absolute() {
                p.to_path_buf()
            } else {
                dir.join(p)
            });
        }
    }
    None
}

/// HEAD's commit read from the git files, no process: a detached sha, or the ref it names in
/// the git dir, the common dir (a linked worktree's `commondir`) or `packed-refs`. `None` for
/// anything else (an unborn branch, a ref format this does not read) — the caller then asks git.
fn read_head(gitdir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let head = head.trim();
    let is_sha =
        |s: &str| (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit());
    let Some(refname) = head.strip_prefix("ref:").map(str::trim) else {
        return is_sha(head).then(|| head.to_string());
    };
    let common = std::fs::read_to_string(gitdir.join("commondir"))
        .ok()
        .map(|c| gitdir.join(c.trim()))
        .unwrap_or_else(|| gitdir.to_path_buf());
    for base in [gitdir, common.as_path()] {
        if let Ok(v) = std::fs::read_to_string(base.join(refname)) {
            let v = v.trim();
            if is_sha(v) {
                return Some(v.to_string());
            }
        }
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|l| {
        let (sha, name) = l.split_once(' ')?;
        (name.trim() == refname && is_sha(sha)).then(|| sha.to_string())
    })
}

/// The drift status of one file an answer touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drift {
    Fresh,
    Modified,
    Deleted,
    /// No digest row: a synthetic / live-collector node, not a file the indexer read.
    NotFileBacked,
    /// The file could not be checked (no root to resolve it against, unreadable).
    Unknown,
}

impl Drift {
    fn mark(self) -> Option<&'static str> {
        match self {
            Drift::Modified => Some("modified"),
            Drift::Deleted => Some("deleted"),
            _ => None,
        }
    }
}

/// The transport-side freshness probe. Holds its OWN store handle (the MCP handler holds only
/// `&dyn GraphRead`, and the repo registry needs [`GraphStoreMutExt`]) and two caches.
pub struct FreshnessProbe {
    db: String,
    store: Option<Box<dyn GraphStoreMutExt>>,
    /// root → (HEAD, graph_version, commits behind) of the last measurement.
    head_cache: HashMap<PathBuf, (String, String, Option<u64>)>,
    /// absolute path → (mtime, size, xxh3) of the last read.
    digest_cache: HashMap<PathBuf, (SystemTime, u64, String)>,
    /// Lower-case extensions an extractor claims, built on first use.
    source_exts: Option<HashSet<String>>,
    /// root → its path inside the work tree (`git rev-parse --show-prefix`), asked once.
    prefix_cache: HashMap<PathBuf, Option<String>>,
    /// root → (what the last `git status` saw, when, its new-file list): reused while HEAD, the
    /// git index and every directory that holds an indexed file are unchanged.
    status_cache: HashMap<PathBuf, (StatusKey, Instant, Option<Vec<String>>)>,
    /// (graph_version, root → the directories that hold indexed files, root included).
    dir_set: Option<(String, HashMap<PathBuf, Vec<PathBuf>>)>,
    /// root → its git directory, discovered once.
    gitdir_cache: HashMap<PathBuf, Option<PathBuf>>,
}

/// Re-run `git status` at least this often even when nothing it is keyed on moved: the one blind
/// spot of the key is a new source file in a directory tree that holds no indexed file yet.
const STATUS_MAX_AGE: Duration = Duration::from_secs(10);

/// What a cached `git status` result is valid for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusKey {
    head: Option<String>,
    index: Option<(SystemTime, u64)>,
    dirs: u64,
}

/// One `git status` of one root: its HEAD and the never-committed files under it.
struct RootStatus {
    head: Option<String>,
    /// Untracked (not ignored) or staged-as-added paths, relative to the root. `None` when git
    /// could not answer.
    new_files: Option<Vec<String>>,
}

impl FreshnessProbe {
    /// A probe over `db`. A store that cannot be opened (or `:memory:`, which no other handle can
    /// see) yields a probe that answers "unknown" — never an error.
    pub fn open(db: &str) -> Self {
        let store = if db == ":memory:" {
            None
        } else {
            wicked_estate_store::open_store_ext(db).ok()
        };
        Self::with_store(db, store)
    }

    /// A probe over an already-open handle (the CLI bridge reuses its own).
    pub fn with_store(db: &str, store: Option<Box<dyn GraphStoreMutExt>>) -> Self {
        Self {
            db: db.to_string(),
            store,
            head_cache: HashMap::new(),
            digest_cache: HashMap::new(),
            source_exts: None,
            prefix_cache: HashMap::new(),
            status_cache: HashMap::new(),
            dir_set: None,
            gitdir_cache: HashMap::new(),
        }
    }

    /// ONE git process per root per call: `git status --porcelain=v2 --branch -z
    /// --untracked-files=normal -- .` gives HEAD (`# branch.oid`) and every untracked or
    /// staged-as-added path under the root. Porcelain paths are relative to the work-tree top,
    /// so the root's own prefix (asked once, cached) is stripped.
    fn git_status(&mut self, root: &Path) -> RootStatus {
        let Some(out) = git(
            root,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=normal",
                "--",
                ".",
            ],
        ) else {
            return RootStatus {
                head: None,
                new_files: None,
            };
        };
        let prefix = self
            .prefix_cache
            .entry(root.to_path_buf())
            .or_insert_with(|| {
                git(root, &["rev-parse", "--show-prefix"]).map(|s| s.trim().to_string())
            })
            .clone()
            .unwrap_or_default();
        let mut head = None;
        let mut new_files = Vec::new();
        let mut records = out.split('\0');
        while let Some(rec) = records.next() {
            if let Some(oid) = rec.strip_prefix("# branch.oid ") {
                head = (oid != "(initial)").then(|| oid.to_string());
            } else if let Some(path) = rec.strip_prefix("? ") {
                new_files.push(path.to_string());
            } else if rec.starts_with("1 A") {
                // `1 XY sub mH mI mW hH hI path`: X = A is staged-as-added.
                if let Some(path) = rec.splitn(9, ' ').nth(8) {
                    new_files.push(path.to_string());
                }
            } else if rec.starts_with("2 ") {
                // A rename carries its original path as the next record.
                records.next();
            }
        }
        let new_files = new_files
            .into_iter()
            .filter_map(|p| p.strip_prefix(prefix.as_str()).map(str::to_string))
            .collect();
        RootStatus {
            head,
            new_files: Some(new_files),
        }
    }

    /// Commit-level freshness, measured now (HEAD per call from [`Self::root_status`]; the
    /// `rev-list` count cached on `(root, HEAD, graph_version)`).
    fn commit_report(&mut self, roots: &[(Root, RootStatus)]) -> (StalenessReport, Vec<Value>) {
        let Some(store) = self.store.as_deref() else {
            return (
                StalenessReport {
                    unknown: vec![format!("{} (no readable graph)", self.db)],
                    ..Default::default()
                },
                vec![],
            );
        };
        let graph_version = store.meta_get_key("graph_version").unwrap_or_default();
        let mut report = StalenessReport::default();
        let mut rows = Vec::new();
        for (root, status) in roots {
            let n = match &status.head {
                Some(h) => match self.head_cache.get(&root.path) {
                    Some((ch, cv, n)) if ch == h && *cv == graph_version => *n,
                    _ => {
                        let n = commits_behind_since(&root.path, root.indexed.as_deref(), &self.db);
                        self.head_cache
                            .insert(root.path.clone(), (h.clone(), graph_version.clone(), n));
                        n
                    }
                },
                // Not a git work tree (or git absent): the mtime fallback still applies.
                None => commits_behind_since(&root.path, root.indexed.as_deref(), &self.db),
            };
            record(&mut report, root, n);
            rows.push(json!({
                "label": root.label,
                "root": root.recorded,
                "commits_behind": n,
                "head": status.head,
                "indexed": root.indexed,
            }));
        }
        (report, rows)
    }

    /// HEAD and the new-file list of one root, spawning NO process in the steady state (#351
    /// bench: a process spawn alone is ~2 ms on an idle host and tens of ms on a loaded one).
    /// HEAD is read from the git files every call — exact. The new-file list is the last
    /// `git status`, re-run when HEAD, the git index or any directory that holds an indexed file
    /// changes (a new file changes its directory's mtime), and at least every
    /// [`STATUS_MAX_AGE`].
    fn root_status(&mut self, root: &Path) -> RootStatus {
        let gitdir = self
            .gitdir_cache
            .entry(root.to_path_buf())
            .or_insert_with(|| discover_git_dir(root))
            .clone();
        let Some(gitdir) = gitdir else {
            // Not a git work tree: no HEAD, no new-file list; commits fall back to the mtime.
            return RootStatus {
                head: None,
                new_files: None,
            };
        };
        let head = read_head(&gitdir);
        let key = StatusKey {
            head: head.clone(),
            index: stamp(&gitdir.join("index")),
            dirs: self.dirs_fingerprint(root),
        };
        if let Some((k, at, files)) = self.status_cache.get(root) {
            if *k == key && at.elapsed() < STATUS_MAX_AGE {
                return RootStatus {
                    head,
                    new_files: files.clone(),
                };
            }
        }
        let fresh = self.git_status(root);
        self.status_cache.insert(
            root.to_path_buf(),
            (key, Instant::now(), fresh.new_files.clone()),
        );
        RootStatus {
            // The file read is authoritative; git's answer only fills a ref format it cannot read.
            head: head.or(fresh.head),
            new_files: fresh.new_files,
        }
    }

    /// A fingerprint of the mtimes of every directory under `root` that holds an indexed file
    /// (and `root`). The set is rebuilt when the graph version changes.
    fn dirs_fingerprint(&mut self, root: &Path) -> u64 {
        use std::hash::{Hash, Hasher};
        let Some(store) = self.store.as_deref() else {
            return 0;
        };
        let version = store.meta_get_key("graph_version").unwrap_or_default();
        if self.dir_set.as_ref().is_none_or(|(v, _)| *v != version) {
            let mut by_root: HashMap<PathBuf, BTreeSet<PathBuf>> = HashMap::new();
            let all_roots = roots(store, &self.db);
            for f in store.indexed_files().unwrap_or_default() {
                let Some(abs) = repo_scope::resolve_indexed_path(store, &f) else {
                    continue;
                };
                let Some(r) = all_roots.iter().find(|r| abs.starts_with(&r.path)) else {
                    continue;
                };
                let set = by_root.entry(r.path.clone()).or_default();
                let mut dir = abs.parent();
                while let Some(d) = dir {
                    if !d.starts_with(&r.path) || !set.insert(d.to_path_buf()) {
                        break;
                    }
                    dir = d.parent();
                }
            }
            self.dir_set = Some((
                version,
                by_root
                    .into_iter()
                    .map(|(k, v)| (k, v.into_iter().collect()))
                    .collect(),
            ));
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        stamp(root).hash(&mut h);
        if let Some((_, dirs)) = &self.dir_set {
            for d in dirs.get(root).map(Vec::as_slice).unwrap_or(&[]) {
                stamp(d).hash(&mut h);
            }
        }
        h.finish()
    }

    /// Every root with its one `git status`.
    fn root_statuses(&mut self) -> Vec<(Root, RootStatus)> {
        let Some(store) = self.store.as_deref() else {
            return vec![];
        };
        let roots = roots(store, &self.db);
        roots
            .into_iter()
            .map(|r| {
                let st = self.root_status(&r.path);
                (r, st)
            })
            .collect()
    }

    /// Drift of one graph path against the bytes that were indexed. `roots` (computed once per
    /// call) map the path to disk: `<label>/rest` joins that repo's root, an un-prefixed path the
    /// single root — the rule [`repo_scope::resolve_indexed_path`] applies, without re-reading
    /// the registry per file.
    fn drift(&mut self, graph_path: &str, roots: &[Root]) -> Drift {
        let Some(store) = self.store.as_deref() else {
            return Drift::Unknown;
        };
        let indexed = match store.file_digest(graph_path) {
            Ok(Some(d)) => d,
            Ok(None) => return Drift::NotFileBacked,
            Err(_) => return Drift::Unknown,
        };
        let by_label = roots.iter().find_map(|r| {
            let label = r.label.as_deref()?;
            graph_path
                .strip_prefix(&repo_scope::prefix(label))
                .map(|rest| r.path.join(rest))
        });
        let single = || {
            roots
                .iter()
                .find(|r| r.label.is_none())
                .map(|r| r.path.join(graph_path))
        };
        let Some(abs) = by_label
            .or_else(single)
            .or_else(|| repo_scope::resolve_indexed_path(store, graph_path))
        else {
            return Drift::Unknown;
        };
        let abs = if abs.is_absolute() {
            abs
        } else {
            recorded_root_path(&abs.to_string_lossy(), &self.db)
        };
        let meta = match std::fs::metadata(&abs) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Drift::Deleted,
            Err(_) => return Drift::Unknown,
        };
        let (mtime, size) = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        let digest = match self.digest_cache.get(&abs) {
            Some((m, s, d)) if *m == mtime && *s == size => d.clone(),
            _ => match std::fs::read(&abs) {
                Ok(bytes) => {
                    let d = crate::file_digest(&bytes);
                    self.digest_cache
                        .insert(abs.clone(), (mtime, size, d.clone()));
                    d
                }
                Err(_) => return Drift::Unknown,
            },
        };
        if digest == indexed {
            Drift::Fresh
        } else {
            Drift::Modified
        }
    }

    /// New source files under every root that the graph has never indexed (R3): untracked
    /// (respecting `.gitignore`) or staged-as-added, with an extension an extractor claims, and
    /// no digest row. `None` when no root could be asked.
    pub fn unindexed_files(&mut self) -> Option<usize> {
        let statuses = self.root_statuses();
        self.count_unindexed(&statuses)
    }

    fn count_unindexed(&mut self, statuses: &[(Root, RootStatus)]) -> Option<usize> {
        // The extensions `index_path` admits: every `languages.toml` row (data, so a new
        // language is covered with no code change) plus the grammar-less line extractors.
        let exts = self
            .source_exts
            .get_or_insert_with(|| {
                wicked_estate_extract::registry()
                    .into_iter()
                    .flat_map(|l| l.ext.into_iter())
                    .map(|e| e.to_ascii_lowercase())
                    .collect()
            })
            .clone();
        let store = self.store.as_deref()?;
        let mut count = 0usize;
        let mut asked = false;
        for (root, status) in statuses {
            let Some(files) = &status.new_files else {
                continue;
            };
            asked = true;
            for rel in files {
                let ext = Path::new(rel)
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase());
                if !ext.is_some_and(|e| exts.contains(&e) || crate::is_grammarless_ext(&e)) {
                    continue;
                }
                let graph_path = repo_scope::namespaced(root.label.as_deref(), rel);
                if matches!(store.file_digest(&graph_path), Ok(None)) {
                    count += 1;
                }
            }
        }
        asked.then_some(count)
    }

    /// How many indexed files differ from the bytes on disk (`modified` or `deleted`) — the
    /// whole-graph count `stats --json` reports. `None` when the graph cannot be read.
    pub fn dirty_indexed_files(&mut self) -> Option<usize> {
        let store = self.store.as_deref()?;
        let files = store.indexed_files().ok()?;
        let roots = roots(store, &self.db);
        Some(
            files
                .iter()
                .filter(|f| matches!(self.drift(f, &roots), Drift::Modified | Drift::Deleted))
                .count(),
        )
    }

    /// Annotate one retrieval response in place: replace the retrieval layer's placeholder with
    /// the real statements, add the `freshness` block, and mark drifted rows `"stale"`.
    pub fn annotate(&mut self, content: &mut Value, diags: &mut Vec<String>) {
        diags.retain(|d| d != wicked_estate_retrieve::STALENESS_PLACEHOLDER);
        let statuses = self.root_statuses();
        let (report, root_rows) = self.commit_report(&statuses);
        diags.extend(report.statements(&self.db));

        let mut files: BTreeSet<String> = BTreeSet::new();
        collect_files(content, &mut files);
        let mut status: BTreeMap<String, Drift> = BTreeMap::new();
        let root_list: Vec<Root> = statuses.iter().map(|(r, _)| r.clone()).collect();
        for f in &files {
            let d = self.drift(f, &root_list);
            status.insert(f.clone(), d);
        }
        let stale: Vec<(&String, &'static str)> = status
            .iter()
            .filter_map(|(f, d)| d.mark().map(|m| (f, m)))
            .collect();
        let checked = status
            .values()
            .filter(|d| matches!(d, Drift::Fresh | Drift::Modified | Drift::Deleted))
            .count();
        let not_file_backed = status
            .values()
            .filter(|d| **d == Drift::NotFileBacked)
            .count();
        let unknown_files = status.values().filter(|d| **d == Drift::Unknown).count();
        let unindexed = self.count_unindexed(&statuses);

        if !stale.is_empty() {
            let named: Vec<String> = stale
                .iter()
                .take(5)
                .map(|(f, m)| format!("{f} {m}"))
                .collect();
            diags.push(format!(
                "STALENESS: {} of {checked} file(s) in this answer differ from the index ({}{}) \
                 — rows marked \"stale\"; re-run `wicked-estate index` to refresh",
                stale.len(),
                named.join(", "),
                if stale.len() > 5 { ", …" } else { "" }
            ));
        }
        if unknown_files > 0 {
            diags.push(format!(
                "STALENESS: drift unknown for {unknown_files} file(s) in this answer — they could \
                 not be read against the index"
            ));
        }
        if let Some(n) = unindexed.filter(|n| *n > 0) {
            diags.push(format!(
                "STALENESS: {n} new source file(s) not in the graph — dependents may be missing"
            ));
        }

        let marks: HashMap<&str, &str> = stale.iter().map(|(f, m)| (f.as_str(), *m)).collect();
        let worst = if report.unknown.is_empty() && report.checked > 0 {
            report.worst
        } else {
            None
        };
        let block = json!({
            "commits_behind": worst,
            "roots": root_rows,
            "stale_files": stale
                .iter()
                .take(STALE_FILES_CAP)
                .map(|(f, m)| json!({"file": f, "status": m}))
                .collect::<Vec<_>>(),
            "stale_files_total": stale.len(),
            "unindexed_files": unindexed,
            "checked_files": checked,
            "not_file_backed": not_file_backed,
        });
        let Some(obj) = content.as_object_mut() else {
            // A non-object answer (none today) carries the diagnostics only.
            return;
        };
        obj.insert("freshness".to_string(), block);
        // R4: annotation must not push a reply the tool sized to the budget past it. First the
        // row marks go (the block still names every drifted file), then the block's detail
        // lists (its counts stay).
        let size = |c: &Value, d: &[String]| {
            serde_json::to_string(c).map(|s| s.len()).unwrap_or(0)
                + d.iter().map(|x| x.len() + 1).sum::<usize>()
        };
        let budget = wicked_estate_retrieve::R4_CHAR_BUDGET;
        if !marks.is_empty() {
            let before = content.clone();
            mark_rows(content, &marks);
            if size(content, diags) > budget {
                *content = before;
                diags.push(
                    "STALENESS: row marks omitted to stay within the R4 budget; see \
                     freshness.stale_files"
                        .to_string(),
                );
            }
        }
        if size(content, diags) > budget {
            if let Some(b) = content.get_mut("freshness").and_then(Value::as_object_mut) {
                b.insert("stale_files".to_string(), json!([]));
                b.insert("roots".to_string(), json!([]));
            }
            diags.push(
                "STALENESS: freshness detail lists omitted to stay within the R4 budget; the \
                 counts above are complete"
                    .to_string(),
            );
        }
    }
}

/// Every string value under a key named `file`, anywhere in `v` (rows, hops, summaries). No
/// per-tool code: R4 bounds the content, so the walk is bounded too.
fn collect_files(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == "freshness" {
                    continue;
                }
                match (k.as_str(), child) {
                    ("file", Value::String(f)) if !f.is_empty() => {
                        out.insert(f.clone());
                    }
                    _ => collect_files(child, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_files(i, out)),
        _ => {}
    }
}

/// Add `"stale": <status>` to every object whose `file` drifted.
fn mark_rows(v: &mut Value, marks: &HashMap<&str, &str>) {
    match v {
        Value::Object(map) => {
            let hit = map
                .get("file")
                .and_then(Value::as_str)
                .and_then(|f| marks.get(f).copied());
            if let Some(m) = hit {
                map.insert("stale".to_string(), json!(m));
            }
            for (k, child) in map.iter_mut() {
                if k != "freshness" {
                    mark_rows(child, marks);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| mark_rows(i, marks)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_files_walks_every_file_key_but_the_freshness_block() {
        let v = json!({
            "dependents": [{"file": "a.rs"}, {"file": "b.rs", "x": {"file": "c.rs"}}],
            "summary": {"top_files": [{"file": "a.rs"}]},
            "freshness": {"stale_files": [{"file": "z.rs"}]},
            "file": "",
        });
        let mut out = BTreeSet::new();
        collect_files(&v, &mut out);
        assert_eq!(
            out.into_iter().collect::<Vec<_>>(),
            vec!["a.rs", "b.rs", "c.rs"]
        );
    }

    #[test]
    fn mark_rows_marks_only_drifted_files() {
        let mut v = json!({"rows": [{"file": "a.rs"}, {"file": "b.rs"}]});
        let marks: HashMap<&str, &str> = [("b.rs", "modified")].into_iter().collect();
        mark_rows(&mut v, &marks);
        assert!(v["rows"][0].get("stale").is_none());
        assert_eq!(v["rows"][1]["stale"], json!("modified"));
    }

    #[test]
    fn statements_say_unknown_rather_than_fresh_without_a_root() {
        let r = StalenessReport::default();
        assert!(r.statements("g.db")[0].contains("unknown"));
        let r = StalenessReport {
            checked: 1,
            worst: Some(0),
            ..Default::default()
        };
        assert_eq!(
            r.statements("g.db"),
            vec!["STALENESS: 0 commits since last index"]
        );
    }

    /// R4: a reply the tool sized to the budget edge stays within it after annotation — a
    /// `BlastRadius` page of 600 callers, every row in a drifted file.
    #[test]
    fn an_answer_at_the_budget_edge_stays_within_r4_after_annotation() {
        use wicked_estate_core::{
            Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, ResolutionTier,
            RetrievalTool, Span, SymbolId,
        };
        let dir = std::env::temp_dir().join(format!("wicked-estate-r4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("callers.rs"), "// changed since it was indexed\n").unwrap();
        let mut store = wicked_estate_store::SqliteStore::in_memory().unwrap();
        store.meta_set_key("indexed_root", &dir.to_string_lossy());
        let node = |id: &str, file: &str| {
            Node::new(
                SymbolId(id.into()),
                NodeKind::Function,
                id,
                Language::new("rust"),
                Location::new(file, Span::ZERO),
            )
        };
        let mut nodes = vec![node("hub", "hub.rs")];
        let mut edges = vec![];
        for i in 0..600 {
            let id = format!("caller_with_a_long_enough_name_{i:04}");
            nodes.push(node(&id, "callers.rs"));
            edges.push(Edge::new(
                SymbolId(id),
                SymbolId("hub".into()),
                EdgeKind::Calls,
                ResolutionTier::Parsed,
                "t",
            ));
        }
        store.begin_batch().unwrap();
        store.upsert_nodes(&nodes).unwrap();
        store.upsert_edges(&edges).unwrap();
        store
            .set_file_digest("callers.rs", "0000000000000000")
            .unwrap();
        store.commit_batch().unwrap();
        let res = wicked_estate_retrieve::BlastRadius
            .invoke(&store, &json!({"symbol": "hub"}))
            .unwrap();
        let (mut content, mut diags) = (res.content, res.diagnostics);
        let mut probe = FreshnessProbe::with_store(":memory:", Some(Box::new(store)));
        probe.annotate(&mut content, &mut diags);
        let size = serde_json::to_string(&content).unwrap().len()
            + diags.iter().map(|d| d.len() + 1).sum::<usize>();
        assert!(
            size <= wicked_estate_retrieve::R4_CHAR_BUDGET,
            "{size} chars"
        );
        assert_eq!(
            content["freshness"]["stale_files_total"],
            json!(1),
            "{}",
            content["freshness"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_probe_without_a_store_degrades_to_unknown() {
        let mut p = FreshnessProbe::with_store(":memory:", None);
        let mut content = json!({"dependents": [{"file": "a.rs"}]});
        let mut diags = vec![wicked_estate_retrieve::STALENESS_PLACEHOLDER.to_string()];
        p.annotate(&mut content, &mut diags);
        assert!(
            !diags
                .iter()
                .any(|d| d == wicked_estate_retrieve::STALENESS_PLACEHOLDER)
        );
        assert!(diags.iter().any(|d| d.contains("unknown")), "{diags:?}");
        assert_eq!(content["freshness"]["commits_behind"], Value::Null);
    }
}
