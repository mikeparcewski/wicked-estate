//! Repo scoping — MANY repos in ONE graph, without a schema migration.
//!
//! ## Why identity, not just `files.path`
//!
//! `files.path` is a `TEXT PRIMARY KEY` holding a repo-relative path, and **SymbolIds embed that
//! same relative path** (`ts-typescript . . . src/index/alpha().`). Two repos that both have
//! `src/index.ts` therefore mint identical file rows AND identical symbol ids: the second index
//! overwrites the first, silently. A schema change on `files` alone would not have helped — the
//! collision is in the identity string.
//!
//! The fix is one label, applied at the single choke point every indexed path flows through
//! (`rel()` in `lib.rs`): with `--repo ledger`, `src/index.ts` becomes `ledger/src/index.ts`, and
//! every id derived from it becomes unique per repo. Absent a label, nothing is prefixed and the
//! behaviour is byte-identical to a single-repo index.
//!
//! ## What this does NOT do
//!
//! Co-location, not linkage. Edges do **not** resolve across repos: resolution is scoped to the
//! labelled repo's own nodes (see `InMemoryIndex::build`), exactly as if each repo were in its own
//! db. `studio → wicked-crew-api-types → crew` needs a package-resolver tier and is separate work.
//!
//! One id shape stays shared, unavoidably: the external-module targets of imports (`node:fs`) are
//! identified by module, not by path, so repos importing the same module share one node row whose
//! `file` column belongs to whichever repo wrote it last. Removing that file prunes the other
//! repos' edges to it until they re-index. The same wart already exists between two FILES in one
//! repo; namespacing cannot reach it because the id carries no path to namespace.
//!
//! ## Provenance
//!
//! Un-labelled dbs keep the singular `repo_commit`/`repo_branch`/`repo_remote`/`repo_dirty` meta
//! keys (`GraphWrite::set_repo_info`). Labelled dbs write `repo:<label>:*` instead and leave the
//! singular keys alone — so the second repo can no longer clobber the first's provenance, and
//! `repo_info()` returning `None` is the honest answer for a graph that holds several repos.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wicked_estate_core::{Error, RepoInfo, Result, Symbol, SymbolId};
use wicked_estate_extract::ExtraExtraction;
use wicked_estate_store::GraphStoreMutExt;

/// Meta key holding the JSON array of repo labels present in the graph.
const LABELS_KEY: &str = "repo_labels";

/// One repo recorded in a multi-repo graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRecord {
    /// The `--repo` label. Also the path prefix every one of this repo's rows carries.
    pub label: String,
    /// The root the repo was last indexed from (as the caller spelled it).
    pub root: String,
    pub info: RepoInfo,
}

/// Reject labels that would not survive being used as a path segment. The label is spliced into
/// `files.path` and into SymbolIds, so a `/` or a `..` in it would forge paths in another repo's
/// namespace — the exact collision this whole mechanism exists to prevent.
pub fn validate_label(label: &str) -> Result<()> {
    let ok = !label.is_empty()
        && label.len() <= 64
        && label != "."
        && label != ".."
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "invalid repo label {label:?}: use 1-64 chars of [A-Za-z0-9._-] (it becomes a path \
             segment on every row this repo writes)"
        )))
    }
}

/// The path prefix a labelled repo's rows carry, including the separator.
pub fn prefix(label: &str) -> String {
    format!("{label}/")
}

/// Namespace one repo-relative path. `None` ⇒ the path is returned unchanged.
pub fn namespaced(label: Option<&str>, rel_path: &str) -> String {
    match label {
        Some(l) => format!("{l}/{rel_path}"),
        None => rel_path.to_string(),
    }
}

fn key(label: &str, field: &str) -> String {
    format!("repo:{label}:{field}")
}

/// Per-run meta key: the bare name for an un-labelled graph, `repo:<label>:<name>` otherwise.
///
/// Used for the keys whose value is a property of ONE indexed tree — the binary version its rows
/// were extracted with, the digest of the extra-edge rules found under its root. Sharing those
/// across repos would make repo B's index silently mark repo A's stale rows as current.
pub fn meta_key(label: Option<&str>, name: &str) -> String {
    match label {
        Some(l) => key(l, name),
        None => name.to_string(),
    }
}

/// Labels recorded in the graph, sorted. Empty for an un-labelled (single-repo) db.
pub fn labels(store: &dyn GraphStoreMutExt) -> Vec<String> {
    store
        .meta_get_key(LABELS_KEY)
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default()
}

/// The record for one label, or `None` when the label is unknown to this graph.
pub fn record(store: &dyn GraphStoreMutExt, label: &str) -> Option<RepoRecord> {
    let root = store.meta_get_key(&key(label, "root"))?;
    let field = |f: &str| store.meta_get_key(&key(label, f)).filter(|s| !s.is_empty());
    Some(RepoRecord {
        label: label.to_string(),
        root,
        info: RepoInfo {
            commit: field("commit"),
            branch: field("branch"),
            remote: field("remote"),
            dirty: store
                .meta_get_key(&key(label, "dirty"))
                .is_some_and(|v| v == "1"),
        },
    })
}

/// Every repo in the graph, in label order. Empty ⇒ this is a single-repo (un-labelled) db.
pub fn registry(store: &dyn GraphStoreMutExt) -> Vec<RepoRecord> {
    labels(store)
        .iter()
        .filter_map(|l| record(store, l))
        .collect()
}

/// Record (or refresh) one repo's provenance and add it to the label list.
pub fn write_record(store: &mut dyn GraphStoreMutExt, label: &str, root: &Path, info: &RepoInfo) {
    store.meta_set_key(&key(label, "root"), &root.to_string_lossy());
    store.meta_set_key(&key(label, "commit"), info.commit.as_deref().unwrap_or(""));
    store.meta_set_key(&key(label, "branch"), info.branch.as_deref().unwrap_or(""));
    store.meta_set_key(&key(label, "remote"), info.remote.as_deref().unwrap_or(""));
    store.meta_set_key(&key(label, "dirty"), if info.dirty { "1" } else { "0" });

    let mut all = labels(store);
    if !all.iter().any(|l| l == label) {
        all.push(label.to_string());
        all.sort();
        if let Ok(json) = serde_json::to_string(&all) {
            store.meta_set_key(LABELS_KEY, &json);
        }
    }
}

/// Absolute-and-symlink-resolved form of a root, falling back to the string as written when the
/// path is gone (a stored root whose directory has since been deleted still has to compare).
fn canon(root: &str) -> String {
    std::fs::canonicalize(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| root.to_string())
}

/// `origin`'s URL reduced to `host/path`, so the transport it was cloned over does not change the
/// repo's identity: `git@github.com:acme/x.git`, `https://github.com/acme/x` and
/// `ssh://git@github.com/acme/x` all reduce to `github.com/acme/x`.
fn canon_remote(remote: Option<&str>) -> Option<String> {
    let r = remote?.trim();
    if r.is_empty() {
        return None;
    }
    let r = r.trim_end_matches('/').trim_end_matches(".git");
    // Drop the scheme, then the `user@` in the authority.
    let r = r.split_once("://").map_or(r, |(_, rest)| rest);
    let authority_end = r.find(['/', ':']).unwrap_or(r.len());
    let r = match r[..authority_end].find('@') {
        Some(at) => &r[at + 1..],
        None => r,
    };
    // scp-style `host:path` → `host/path`. A path remote has no colon and is left alone.
    Some(r.replacen(':', "/", 1).to_ascii_lowercase())
}

/// Are these two index runs the SAME repo?
///
/// Git remote first: it survives clones, worktrees, and a moved checkout, and two different repos
/// never share one. When either side has no remote (not a git repo, or no `origin`) there is no
/// evidence but the root path, so paths are compared canonically. That fallback errs toward
/// "different" — a non-git tree that MOVED reads as a new repo and gets refused. Refusal is
/// recoverable; the silent overwrite it replaces is not.
fn same_repo(a_root: &str, a_remote: Option<&str>, b_root: &str, b_remote: Option<&str>) -> bool {
    match (canon_remote(a_remote), canon_remote(b_remote)) {
        (Some(x), Some(y)) => x == y,
        _ => canon(a_root) == canon(b_root),
    }
}

/// Describe a repo for an error message: remote when there is one, else the root.
fn describe(root: &str, remote: Option<&str>) -> String {
    match canon_remote(remote) {
        Some(_) => format!("{} (at {root})", remote.unwrap_or_default()),
        None => root.to_string(),
    }
}

/// THE GUARD. Refuse — loudly, before a single row is written — any index that would overwrite
/// content another repo already put in this graph.
///
/// `indexed` is this indexer's own file rows (`GraphRead::indexed_files`), the only evidence of
/// what a previous run wrote. A path that carries no known label prefix is un-labelled content.
///
/// The four refusals:
///   1. un-labelled index into a graph that holds labelled repos — its delete-sweep is graph-wide
///      and would remove every labelled repo's files;
///   2. un-labelled index of a DIFFERENT repo over an un-labelled one — the original silent-loss
///      bug;
///   3. labelled index into a graph that already holds un-labelled content — the bare rows would
///      be stranded in a namespace nothing owns;
///   4. a label already bound to another repo, or a repo already indexed under another label
///      (which would duplicate it).
pub fn guard(
    store: &dyn GraphStoreMutExt,
    indexed: &HashSet<String>,
    label: Option<&str>,
    root: &Path,
    info: &RepoInfo,
) -> Result<()> {
    let known = labels(store);
    let root_str = root.to_string_lossy().into_owned();
    let has_unlabelled = indexed
        .iter()
        .any(|f| !known.iter().any(|l| f.starts_with(&prefix(l))));

    match label {
        None => {
            if !known.is_empty() {
                return Err(Error::Invalid(format!(
                    "REPO COLLISION: this graph holds {n} labelled repo(s) [{list}], and an \
                     un-labelled index writes bare paths plus a graph-wide delete sweep that \
                     would remove them.\n\
                     fix: re-run with a label — `wicked-estate index {root_str} --repo <name>`",
                    n = known.len(),
                    list = known.join(", "),
                )));
            }
            if !has_unlabelled {
                return Ok(()); // empty graph — nothing to collide with.
            }
            let prev_root = match store.meta_get_key("indexed_root") {
                Some(r) => r,
                // Content but no recorded root (pre-W7.4 db): no evidence to judge on, so behave
                // exactly as before rather than refuse on a guess.
                None => return Ok(()),
            };
            let prev_remote = store.meta_get_key("repo_remote");
            if same_repo(
                &prev_root,
                prev_remote.as_deref(),
                &root_str,
                info.remote.as_deref(),
            ) {
                return Ok(()); // same repo, re-indexed — today's behaviour, untouched.
            }
            Err(Error::Invalid(format!(
                "REPO COLLISION: this graph already holds {prev}, and {new} shares relative paths \
                 with it — indexing it un-labelled would overwrite those rows and delete the \
                 rest.\n\
                 fix: give each repo a label — index the second as \
                 `wicked-estate index {root_str} --repo <name>`, and re-index the first the same \
                 way into a fresh --db (a graph cannot mix labelled and un-labelled repos).",
                prev = describe(&prev_root, prev_remote.as_deref()),
                new = describe(&root_str, info.remote.as_deref()),
            )))
        }
        Some(l) => {
            validate_label(l)?;
            if has_unlabelled {
                let prev_root = store
                    .meta_get_key("indexed_root")
                    .unwrap_or_else(|| "<unknown root>".to_string());
                return Err(Error::Invalid(format!(
                    "REPO COLLISION: this graph already holds un-labelled content from \
                     {prev_root}. Labelled rows live under `<label>/…`, so the existing rows \
                     would be stranded in a namespace no repo owns.\n\
                     fix: index into a fresh --db, giving EVERY repo a --repo label (including \
                     {prev_root})."
                )));
            }
            if let Some(existing) = record(store, l) {
                if !same_repo(
                    &existing.root,
                    existing.info.remote.as_deref(),
                    &root_str,
                    info.remote.as_deref(),
                ) {
                    return Err(Error::Invalid(format!(
                        "REPO COLLISION: label '{l}' is already bound to {prev} in this graph; \
                         indexing {new} under it would overwrite that repo's rows.\n\
                         fix: pick a different `--repo` name for {root_str}.",
                        prev = describe(&existing.root, existing.info.remote.as_deref()),
                        new = describe(&root_str, info.remote.as_deref()),
                    )));
                }
                return Ok(());
            }
            if let Some(other) = registry(store).into_iter().find(|r| {
                same_repo(
                    &r.root,
                    r.info.remote.as_deref(),
                    &root_str,
                    info.remote.as_deref(),
                )
            }) {
                return Err(Error::Invalid(format!(
                    "REPO COLLISION: {new} is already indexed in this graph as '{other}'; \
                     adding it again as '{l}' would store every symbol twice.\n\
                     fix: re-run as `wicked-estate index {root_str} --repo {other}` to refresh it.",
                    new = describe(&root_str, info.remote.as_deref()),
                    other = other.label,
                )));
            }
            Ok(())
        }
    }
}

/// Rewrite one extra-edge extraction into `label`'s namespace.
///
/// Drop-in rules (`.wicked-estate-extractors/*.toml`) glob against REPO-RELATIVE paths — a rule
/// written as `src/**/*.js` must keep matching in a labelled graph — so the extra pass runs on the
/// raw path and its output is namespaced here instead. Both id shapes the pass can mint are
/// rebuilt through the canonical constructors, never spliced as strings:
///   * `file . <path>:`        — the matched file, and `target_kind = "file"` targets;
///   * `<scheme> synthetic <id>:` — rule-emitted topics/capabilities.
///
/// Synthetic ids are namespaced too. Left shared, two repos' rules would converge on ONE node row
/// whose `file` column belongs to whichever repo wrote last — and re-indexing that repo would
/// delete the node out from under the other, pruning its edges. Same-node convergence across
/// repos is cross-repo linkage, which this change explicitly does not do.
pub(crate) fn namespace_extra(extra: &mut ExtraExtraction, label: &str, file_path: &str) {
    for n in &mut extra.nodes {
        n.symbol = namespace_symbol(&n.symbol, label);
        n.location.file = file_path.to_string();
    }
    for e in &mut extra.edges {
        e.source = namespace_symbol(&e.source, label);
        e.target = namespace_symbol(&e.target, label);
        if let Some(loc) = &mut e.location {
            loc.file = file_path.to_string();
        }
    }
    for r in &mut extra.unresolved_refs {
        r.from = namespace_symbol(&r.from, label);
        r.location.file = file_path.to_string();
    }
}

/// Namespace a `file`/`synthetic` SymbolId. Any other shape is already path-namespaced by the
/// extractor (it was handed the prefixed path) and is returned untouched.
fn namespace_symbol(id: &SymbolId, label: &str) -> SymbolId {
    let s = id.as_str();
    if let Some(rest) = s.strip_prefix("file . ") {
        if let Some(path) = rest.strip_suffix(':') {
            if path.starts_with(&prefix(label)) {
                return id.clone();
            }
            return Symbol::file(namespaced(Some(label), path)).id();
        }
    }
    if let Some((scheme, rest)) = s.split_once(" synthetic ") {
        if let Some(sid) = rest.strip_suffix(':') {
            if sid.starts_with(&prefix(label)) {
                return id.clone();
            }
            return Symbol::synthetic(scheme, namespaced(Some(label), sid)).id();
        }
    }
    id.clone()
}

/// Map a path as stored in the graph back to an absolute on-disk path.
///
/// Single-repo graphs join `indexed_root`; labelled graphs strip the `<label>/` prefix and join
/// THAT repo's root. Callers that read bytes off disk from a node's `location.file` (e.g.
/// `fingerprint --content`) need this — the graph path is no longer relative to a single root.
pub fn resolve_indexed_path(store: &dyn GraphStoreMutExt, graph_path: &str) -> Option<PathBuf> {
    for rec in registry(store) {
        if let Some(rest) = graph_path.strip_prefix(&prefix(&rec.label)) {
            return Some(Path::new(&rec.root).join(rest));
        }
    }
    store
        .meta_get_key("indexed_root")
        .map(|r| Path::new(&r).join(graph_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_store::MemStore;

    fn info(remote: Option<&str>) -> RepoInfo {
        RepoInfo {
            commit: Some("deadbeef".into()),
            branch: Some("main".into()),
            remote: remote.map(str::to_string),
            dirty: false,
        }
    }

    fn files(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn label_validation_rejects_path_forging() {
        assert!(validate_label("wicked-ledger").is_ok());
        assert!(validate_label("a/b").is_err());
        assert!(validate_label("..").is_err());
        assert!(validate_label("").is_err());
    }

    #[test]
    fn empty_graph_accepts_anything() {
        let store = MemStore::new();
        let empty = files(&[]);
        assert!(guard(&store, &empty, None, Path::new("/a"), &info(None)).is_ok());
        assert!(guard(&store, &empty, Some("a"), Path::new("/a"), &info(None)).is_ok());
    }

    #[test]
    fn same_repo_reindex_is_not_a_collision() {
        let mut store = MemStore::new();
        store.meta_set_key("indexed_root", "/repos/alpha");
        store.meta_set_key("repo_remote", "git@github.com:acme/alpha.git");
        let idx = files(&["src/index.ts"]);
        // Same remote, different spelling of the root — still one repo.
        let r = guard(
            &store,
            &idx,
            None,
            Path::new("/elsewhere/alpha"),
            &info(Some("https://github.com/acme/alpha")),
        );
        assert!(r.is_ok(), "same remote must not refuse: {r:?}");
    }

    #[test]
    fn remote_identity_ignores_the_transport() {
        let ssh = canon_remote(Some("git@github.com:acme/alpha.git"));
        assert_eq!(ssh.as_deref(), Some("github.com/acme/alpha"));
        assert_eq!(canon_remote(Some("https://github.com/acme/alpha/")), ssh);
        assert_eq!(
            canon_remote(Some("ssh://git@github.com/acme/alpha.git")),
            ssh
        );
        assert_ne!(canon_remote(Some("git@github.com:acme/beta.git")), ssh);
        assert_eq!(canon_remote(Some("  ")), None);
    }

    #[test]
    fn different_repo_unlabelled_is_refused() {
        let mut store = MemStore::new();
        store.meta_set_key("indexed_root", "/repos/alpha");
        store.meta_set_key("repo_remote", "git@github.com:acme/alpha.git");
        let idx = files(&["src/index.ts"]);
        let err = guard(
            &store,
            &idx,
            None,
            Path::new("/repos/beta"),
            &info(Some("git@github.com:acme/beta.git")),
        )
        .expect_err("must refuse");
        let msg = err.to_string();
        assert!(msg.contains("REPO COLLISION"), "{msg}");
        assert!(msg.contains("--repo"), "error must name the fix: {msg}");
    }

    #[test]
    fn labelled_index_over_unlabelled_content_is_refused() {
        let mut store = MemStore::new();
        store.meta_set_key("indexed_root", "/repos/alpha");
        let idx = files(&["src/index.ts"]);
        let err = guard(
            &store,
            &idx,
            Some("beta"),
            Path::new("/repos/beta"),
            &info(None),
        )
        .expect_err("must refuse");
        assert!(err.to_string().contains("un-labelled content"));
    }

    #[test]
    fn unlabelled_index_into_labelled_graph_is_refused() {
        let mut store = MemStore::new();
        write_record(
            &mut store,
            "alpha",
            Path::new("/repos/alpha"),
            &info(Some("git@github.com:acme/alpha.git")),
        );
        let idx = files(&["alpha/src/index.ts"]);
        let err = guard(&store, &idx, None, Path::new("/repos/beta"), &info(None))
            .expect_err("must refuse");
        assert!(err.to_string().contains("labelled repo(s) [alpha]"));
    }

    #[test]
    fn label_bound_to_another_repo_is_refused() {
        let mut store = MemStore::new();
        write_record(
            &mut store,
            "alpha",
            Path::new("/repos/alpha"),
            &info(Some("git@github.com:acme/alpha.git")),
        );
        let idx = files(&["alpha/src/index.ts"]);
        let err = guard(
            &store,
            &idx,
            Some("alpha"),
            Path::new("/repos/beta"),
            &info(Some("git@github.com:acme/beta.git")),
        )
        .expect_err("must refuse");
        assert!(err.to_string().contains("already bound"));
    }

    #[test]
    fn same_repo_under_a_second_label_is_refused() {
        let mut store = MemStore::new();
        write_record(
            &mut store,
            "alpha",
            Path::new("/repos/alpha"),
            &info(Some("git@github.com:acme/alpha.git")),
        );
        let idx = files(&["alpha/src/index.ts"]);
        let err = guard(
            &store,
            &idx,
            Some("alpha2"),
            Path::new("/repos/alpha"),
            &info(Some("git@github.com:acme/alpha.git")),
        )
        .expect_err("must refuse");
        assert!(err.to_string().contains("already indexed"));
    }

    #[test]
    fn adding_a_second_repo_is_allowed_and_recorded() {
        let mut store = MemStore::new();
        write_record(
            &mut store,
            "alpha",
            Path::new("/repos/alpha"),
            &info(Some("git@github.com:acme/alpha.git")),
        );
        let idx = files(&["alpha/src/index.ts"]);
        assert!(
            guard(
                &store,
                &idx,
                Some("beta"),
                Path::new("/repos/beta"),
                &info(Some("git@github.com:acme/beta.git")),
            )
            .is_ok()
        );
        write_record(
            &mut store,
            "beta",
            Path::new("/repos/beta"),
            &info(Some("git@github.com:acme/beta.git")),
        );
        let reg = registry(&store);
        assert_eq!(reg.len(), 2);
        assert_eq!(reg[0].label, "alpha");
        assert_eq!(reg[1].info.commit.as_deref(), Some("deadbeef"));
        // Per-repo provenance does not clobber: both repos kept their own remote.
        assert_eq!(
            reg[0].info.remote.as_deref(),
            Some("git@github.com:acme/alpha.git")
        );
    }

    #[test]
    fn file_and_synthetic_symbols_are_namespaced() {
        let f = Symbol::file("src/a.ts").id();
        assert_eq!(
            namespace_symbol(&f, "alpha").as_str(),
            Symbol::file("alpha/src/a.ts").id().as_str()
        );
        let s = Symbol::synthetic("bus-topic", "orders.created").id();
        assert_eq!(
            namespace_symbol(&s, "alpha").as_str(),
            Symbol::synthetic("bus-topic", "alpha/orders.created")
                .id()
                .as_str()
        );
        // Idempotent — a second pass must not double-prefix.
        let once = namespace_symbol(&f, "alpha");
        assert_eq!(namespace_symbol(&once, "alpha"), once);
        // A global symbol already carries the prefixed path; leave it alone.
        let g = SymbolId::from("ts-typescript . . . alpha/src/a/x().");
        assert_eq!(namespace_symbol(&g, "alpha"), g);
    }

    #[test]
    fn resolve_indexed_path_uses_the_owning_repo_root() {
        let mut store = MemStore::new();
        store.meta_set_key("indexed_root", "/repos/solo");
        assert_eq!(
            resolve_indexed_path(&store, "src/a.ts"),
            Some(PathBuf::from("/repos/solo/src/a.ts"))
        );
        write_record(&mut store, "alpha", Path::new("/repos/alpha"), &info(None));
        assert_eq!(
            resolve_indexed_path(&store, "alpha/src/a.ts"),
            Some(PathBuf::from("/repos/alpha/src/a.ts"))
        );
    }
}
