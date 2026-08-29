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
//! One id shape stays shared, unavoidably: an import target that was never resolved to a
//! definition is identified by the module SPECIFIER, not by a path — `node:fs`, an npm package,
//! and equally a relative `./index` — so repos importing the same specifier share one node row
//! whose `file` column belongs to whichever repo wrote it last. The same wart already exists
//! between two FILES in one repo; namespacing cannot reach it because the id carries no path to
//! namespace. Ownership of that column mutates through THREE paths, none repo-aware: (1) any
//! importer's re-index takes it (last-writer-wins upsert); (2) removing the owner re-homes the
//! node to the MIN(file) of its surviving importers' edges (`remove_file`'s shared-Import keep,
//! incr-integrity lane) — possibly ACROSS repo prefixes, so a path-prefix-scoped view can show an
//! edge whose target node is filtered out; (3) the last importer's removal deletes it. The edges
//! themselves are safe: removing the owner no longer dangles the other repos' edges (the node is
//! kept while any survivor edge targets it — pinned by
//! `cross_repo_shared_import_survives_owner_repo_deletion`).
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
    /// `root`'s position inside its git work tree at index time; see [`git_subpath`]. `None` for
    /// a non-git tree, and for records written before this field existed.
    pub subpath: Option<String>,
    pub info: RepoInfo,
}

/// Reject labels that would not survive being used as a path segment. The label is spliced into
/// `files.path` and into SymbolIds, so a `/` or a `..` in it would forge paths in another repo's
/// namespace — the exact collision this whole mechanism exists to prevent.
pub fn validate_label(label: &str) -> Result<()> {
    // A leading `-` is rejected even though the character is otherwise legal (Copilot on #117):
    // `--repo -foo` is refused by the CLI as a flag, so a label the LIBRARY accepted could never
    // be named again from the command line — indexed once, impossible to refresh, and the guard
    // would keep refusing every attempt to re-index it. The two entry points must agree on what
    // a label can be.
    let ok = !label.is_empty()
        && label.len() <= 64
        && label != "."
        && label != ".."
        && !label.starts_with('-')
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "invalid repo label {label:?}: use 1-64 chars of [A-Za-z0-9._-], not starting with \
             `-` (it becomes a path segment on every row this repo writes, and a leading `-` \
             could never be passed back on the command line)"
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
        // NOT `field` — the empty string is this value's most common REAL answer (a root at the
        // work-tree top level) and must stay distinguishable from "the key is not there", which
        // is what an older record looks like and which means "no evidence".
        subpath: store.meta_get_key(&key(label, "subpath")),
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
    // Written only when git answered. An absent key means "unknown", which `same_repo` reads as
    // "no evidence"; writing `""` for unknown would claim the work-tree top level instead.
    if let Some(sub) = git_subpath(root) {
        store.meta_set_key(&key(label, "subpath"), &sub);
    }
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
///
/// Only the AUTHORITY is case-folded. Lower-casing the path too made `/srv/git/Alpha` and
/// `/srv/git/alpha` — two different repositories on a case-sensitive filesystem, reachable as
/// path remotes — compare equal, and "equal" on this predicate is what lets an un-labelled index
/// overwrite the other one without a word.
fn canon_remote(remote: Option<&str>) -> Option<String> {
    let r = remote?.trim();
    if r.is_empty() {
        return None;
    }
    let r = r.trim_end_matches('/').trim_end_matches(".git");

    // A SCHEME remote's authority ends at the first `/` — any `:` inside it is a PORT, not a
    // path separator (Copilot on #117). Treating it as one turned
    // `ssh://git@github.com:2222/acme/x` into `github.com/2222/acme/x`, so the same repo compared
    // DIFFERENT depending on whether the clone url named a port: the guard then refuses a
    // legitimate re-index, and — worse — fails to notice the same repo being indexed under a
    // second label, which stores every symbol twice.
    if let Some((_, rest)) = r.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let authority = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        // Drop an explicit port: the transport does not change the repo's identity.
        let host = authority.rsplit_once(':').map_or(authority, |(h, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
                h
            } else {
                authority
            }
        });
        return Some(if path.is_empty() {
            host.to_ascii_lowercase()
        } else {
            format!("{}/{path}", host.to_ascii_lowercase())
        });
    }

    // Scheme-less. `user@host:path` is scp-style and rewrites to `host/path`; anything else is a
    // filesystem path and is left alone. A lone drive letter (`C:/repos/x`) is NOT an authority —
    // rewriting it produced `c//repos/x` — so require the pre-colon part to be longer than one
    // character before treating it as a host.
    let after_user = r.rsplit_once('@').map_or(r, |(_, host)| host);
    let scp = after_user
        .split_once(':')
        .filter(|(host, _)| host.len() > 1 && !host.contains('/'));
    match scp {
        Some((host, path)) => Some(format!(
            "{}/{}",
            host.to_ascii_lowercase(),
            path.trim_start_matches('/')
        )),
        // A path remote: only case-fold when there is no path to damage. `/srv/git/Alpha` and
        // `/srv/git/alpha` are different repositories on a case-sensitive filesystem.
        None => Some(r.to_string()),
    }
}

/// Where an indexed root sits INSIDE its git work tree — `""` at the top level,
/// `packages/api` for a subdirectory. `None` when git cannot answer (no git, not a work tree,
/// path gone); the caller then has no evidence and must not judge on it.
///
/// This is the second half of repo identity. The remote alone is not enough: every package of a
/// monorepo shares one `origin`, and two of them both have `src/index.ts`.
pub fn git_subpath(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "rev-parse", "--show-prefix"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8(out.stdout)
            .ok()?
            .trim()
            .trim_matches('/')
            .to_string(),
    )
}

/// One side of an identity comparison.
#[derive(Clone, Copy)]
struct Ident<'a> {
    root: &'a str,
    remote: Option<&'a str>,
    /// `root`'s path inside its git work tree; see [`git_subpath`]. `None` = unknown.
    subpath: Option<&'a str>,
}

/// Are these two index runs the SAME TREE?
///
/// Git remote first: it survives clones, worktrees, and a moved checkout, and two different
/// repositories never share one. But a shared remote does not mean a shared tree — `mono/pkgA`
/// and `mono/pkgB` have one `origin` between them and both mint `src/index.ts`, so treating the
/// remote as the whole identity let the second one overwrite the first without a word (the exact
/// failure this guard exists to stop, reached through the commonest layout there is). So when the
/// remotes match, the roots' positions INSIDE the work tree must match too.
///
/// A position is compared only when both sides know theirs. Unknown on either side ⇒ remote alone,
/// which is what keeps a moved or re-cloned checkout reading as one repo.
///
/// With no remote on one side there is no evidence but the root path, so paths are compared
/// canonically. That fallback errs toward "different" — a non-git tree that MOVED reads as a new
/// repo and gets refused. Refusal is recoverable; the silent overwrite it replaces is not.
fn same_repo(a: Ident<'_>, b: Ident<'_>) -> bool {
    match (canon_remote(a.remote), canon_remote(b.remote)) {
        (Some(x), Some(y)) => {
            x == y
                && match (a.subpath, b.subpath) {
                    (Some(p), Some(q)) => p.trim_matches('/') == q.trim_matches('/'),
                    _ => true,
                }
        }
        _ => canon(a.root) == canon(b.root),
    }
}

/// The identity a recorded repo compares under.
fn ident_of(rec: &RepoRecord) -> Ident<'_> {
    Ident {
        root: &rec.root,
        remote: rec.info.remote.as_deref(),
        subpath: rec.subpath.as_deref(),
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
    // This run's position inside its work tree — the half of identity that tells two packages of
    // one monorepo apart. Computed once; `git_subpath` shells out.
    let this_subpath = git_subpath(root);
    let this = Ident {
        root: &root_str,
        remote: info.remote.as_deref(),
        subpath: this_subpath.as_deref(),
    };

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
            let prev_remote = store.meta_get_key("repo_remote");
            let prev_root = match store.meta_get_key("indexed_root") {
                Some(r) => r,
                // Content but no recorded root (pre-W7.4 db). The root is not the only evidence
                // there is: when both sides name an `origin`, a DIFFERENT origin still proves
                // these are different repos and the overwrite is still real. Only when that too
                // is silent is there nothing to judge on, and the old behaviour stands.
                None => {
                    return match (
                        canon_remote(prev_remote.as_deref()),
                        canon_remote(this.remote),
                    ) {
                        (Some(x), Some(y)) if x != y => Err(Error::Invalid(format!(
                            "REPO COLLISION: this graph already holds {prev}, and {new} shares \
                             relative paths with it — indexing it un-labelled would overwrite \
                             those rows and delete the rest.\n\
                             fix: give each repo a label — index the second as \
                             `wicked-estate index {root_str} --repo <name>`, and re-index the \
                             first the same way into a fresh --db.",
                            prev = describe("<unknown root>", prev_remote.as_deref()),
                            new = describe(&root_str, this.remote),
                        ))),
                        _ => Ok(()),
                    };
                }
            };
            // An un-labelled graph records no subpath — adding a meta key would be a visible
            // change to single-repo dbs, which this whole flag exists to leave alone — so ask git
            // about the recorded root directly. That is exactly the evidence this case needs: the
            // collision it guards against is two roots that BOTH exist right now. A root that has
            // since gone answers `None`, and the comparison falls back to the remote alone, which
            // is what keeps a moved checkout reading as one repo.
            let prev_subpath = git_subpath(Path::new(&prev_root));
            let prev = Ident {
                root: &prev_root,
                remote: prev_remote.as_deref(),
                subpath: prev_subpath.as_deref(),
            };
            if same_repo(prev, this) {
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
                new = describe(&root_str, this.remote),
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
                if !same_repo(ident_of(&existing), this) {
                    return Err(Error::Invalid(format!(
                        "REPO COLLISION: label '{l}' is already bound to {prev} in this graph; \
                         indexing {new} under it would overwrite that repo's rows.\n\
                         fix: pick a different `--repo` name for {root_str}. If this IS that repo \
                         moved on disk (a non-git tree is identified by its path, so a move reads \
                         as a new repo), re-index every repo into a fresh --db — reusing the label \
                         here would overwrite the rows still recorded under the old root.",
                        prev = describe(&existing.root, existing.info.remote.as_deref()),
                        new = describe(&root_str, this.remote),
                    )));
                }
                return Ok(());
            }
            if let Some(other) = registry(store)
                .into_iter()
                .find(|r| same_repo(ident_of(r), this))
            {
                return Err(Error::Invalid(format!(
                    "REPO COLLISION: {new} is already indexed in this graph as '{other}'; \
                     adding it again as '{l}' would store every symbol twice.\n\
                     fix: re-run as `wicked-estate index {root_str} --repo {other}` to refresh it.",
                    new = describe(&root_str, this.remote),
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

/// Namespace a `file`/`synthetic` SymbolId. Any other shape carries the path the extractor was
/// handed (already prefixed via `FileWork::rel`) and is returned untouched.
///
/// NOT idempotent, and deliberately so. An earlier version short-circuited when the id already
/// started with `<label>/`, to make a second pass a no-op — but that made the function silently
/// WRONG for the repos most likely to hit it: `--repo docs` on a repo that has a top-level `docs/`
/// left `file . docs/x.md:` unprefixed (it "already" started with `docs/`), the file row was
/// `docs/docs/x.md`, and the edge pointing at the un-prefixed id was pruned as dangling — an
/// extra edge lost with no diagnostic. Correctness beats a property nothing needs: the sole
/// caller, [`namespace_extra`], runs exactly once per extraction, immediately after
/// `extract_extra` and before anything is stored.
fn namespace_symbol(id: &SymbolId, label: &str) -> SymbolId {
    let s = id.as_str();
    if let Some(rest) = s.strip_prefix("file . ") {
        if let Some(path) = rest.strip_suffix(':') {
            return Symbol::file(namespaced(Some(label), path)).id();
        }
    }
    if let Some((scheme, rest)) = s.split_once(" synthetic ") {
        if let Some(sid) = rest.strip_suffix(':') {
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

    /// Only the AUTHORITY folds case. Two path remotes differing only in the case of a path
    /// segment are two repositories on a case-sensitive filesystem, and "same repo" on this
    /// predicate is the answer that lets one overwrite the other un-labelled.
    #[test]
    fn remote_identity_folds_the_host_but_not_the_path() {
        assert_eq!(
            canon_remote(Some("git@GitHub.COM:acme/alpha.git")).as_deref(),
            Some("github.com/acme/alpha")
        );
        assert_ne!(
            canon_remote(Some("/srv/git/Alpha")),
            canon_remote(Some("/srv/git/alpha"))
        );
    }

    fn ident<'a>(root: &'a str, remote: Option<&'a str>, sub: Option<&'a str>) -> Ident<'a> {
        Ident {
            root,
            remote,
            subpath: sub,
        }
    }

    /// The monorepo hole. One `origin`, two packages, both with `src/index.ts`: identical on the
    /// remote alone, and the guard reading them as one repo is what let the second index sweep the
    /// first away without a word.
    #[test]
    fn one_remote_two_packages_are_not_the_same_tree() {
        let r = Some("git@github.com:acme/mono.git");
        assert!(!same_repo(
            ident("/m/pkgA", r, Some("pkgA")),
            ident("/m/pkgB", r, Some("pkgB")),
        ));
        // …and a package against the whole repo is likewise not the same tree.
        assert!(!same_repo(
            ident("/m", r, Some("")),
            ident("/m/pkgB", r, Some("pkgB")),
        ));
        // Same package, same remote, different path on disk — a moved or re-cloned checkout.
        assert!(same_repo(
            ident("/old/m/pkgA", r, Some("pkgA")),
            ident(
                "/new/m/pkgA",
                Some("https://github.com/acme/mono"),
                Some("pkgA")
            ),
        ));
        // Position unknown on either side ⇒ no evidence ⇒ the remote alone decides, which is the
        // behaviour a record written before subpaths existed still gets.
        assert!(same_repo(
            ident("/a", r, None),
            ident("/b", r, Some("pkgB"))
        ));
    }

    /// A pre-W7.4 db has content but no `indexed_root`. The root is not the only evidence there:
    /// a DIFFERENT `origin` still proves the overwrite is real.
    #[test]
    fn a_rootless_db_still_refuses_a_different_remote() {
        let mut store = MemStore::new();
        store.meta_set_key("repo_remote", "git@github.com:acme/alpha.git");
        let idx = files(&["src/index.ts"]);
        let err = guard(
            &store,
            &idx,
            None,
            Path::new("/repos/beta"),
            &info(Some("git@github.com:acme/beta.git")),
        )
        .expect_err("a different origin is evidence enough");
        assert!(err.to_string().contains("REPO COLLISION"));
        // No remote on either side is genuinely no evidence — behave as before rather than guess.
        let mut bare = MemStore::new();
        bare.meta_set_key("repo_remote", "");
        assert!(guard(&bare, &idx, None, Path::new("/repos/beta"), &info(None)).is_ok());
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
        // A global symbol already carries the prefixed path; leave it alone.
        let g = SymbolId::from("ts-typescript . . . alpha/src/a/x().");
        assert_eq!(namespace_symbol(&g, "alpha"), g);
    }

    /// Regression: the label may equal a leading path segment of the id being namespaced
    /// (`--repo docs` on a repo that has `docs/`). It must STILL be prefixed — the file row it has
    /// to match is `docs/docs/x.md`. The old `starts_with(prefix)` short-circuit returned the
    /// un-prefixed id here, and the edge pointing at it was silently pruned as dangling.
    #[test]
    fn a_label_that_matches_a_leading_path_segment_is_still_namespaced() {
        let f = Symbol::file("docs/triage.md").id();
        assert_eq!(
            namespace_symbol(&f, "docs").as_str(),
            Symbol::file("docs/docs/triage.md").id().as_str()
        );
        let s = Symbol::synthetic("archetype", "docs/triage").id();
        assert_eq!(
            namespace_symbol(&s, "docs").as_str(),
            Symbol::synthetic("archetype", "docs/docs/triage")
                .id()
                .as_str()
        );
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

    /// One repo, many clone URLs — all must reduce to ONE identity, or the guard both refuses
    /// legitimate re-indexes AND fails to notice the same repo arriving under a second label.
    #[test]
    fn canon_remote_is_transport_and_port_independent() {
        let same = [
            "git@github.com:acme/x.git",
            "https://github.com/acme/x",
            "ssh://git@github.com/acme/x.git",
            // The port is transport, not identity (Copilot on #117).
            "ssh://git@github.com:2222/acme/x.git",
            "https://github.com:443/acme/x",
            "GIT@GitHub.com:acme/x.git",
        ];
        let canon: Vec<Option<String>> = same.iter().map(|r| canon_remote(Some(r))).collect();
        for (r, c) in same.iter().zip(&canon) {
            assert_eq!(
                c.as_deref(),
                Some("github.com/acme/x"),
                "{r} did not reduce to the shared identity"
            );
        }
    }

    /// Path remotes are NOT scp-style and must survive untouched — including a Windows drive
    /// letter, which the scp rewrite turned into `c//repos/x`.
    #[test]
    fn canon_remote_leaves_path_remotes_alone() {
        assert_eq!(
            canon_remote(Some("/srv/git/alpha")).as_deref(),
            Some("/srv/git/alpha")
        );
        // Case-sensitive filesystems: these are two different repositories.
        assert_ne!(
            canon_remote(Some("/srv/git/Alpha")),
            canon_remote(Some("/srv/git/alpha"))
        );
        assert_eq!(
            canon_remote(Some("C:/repos/x")).as_deref(),
            Some("C:/repos/x")
        );
        assert_eq!(
            canon_remote(Some("C:\\repos\\x")).as_deref(),
            Some("C:\\repos\\x")
        );
    }

    /// Different repos must NOT collapse together — the failure that lets one overwrite another.
    #[test]
    fn canon_remote_keeps_different_repos_apart() {
        assert_ne!(
            canon_remote(Some("git@github.com:acme/x")),
            canon_remote(Some("git@github.com:acme/y"))
        );
        assert_ne!(
            canon_remote(Some("git@github.com:acme/x")),
            canon_remote(Some("git@gitlab.com:acme/x"))
        );
    }
}
