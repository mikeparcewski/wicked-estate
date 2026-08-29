//! `wicked-estate` (lib) — the indexing pipeline + query helpers.
//!
//! `index_path` wires **EXTRACT** (`wicked-estate-extract`) → **RESOLVE** (`wicked-estate-resolve`) → **STORE**
//! (`wicked-estate-store`) entirely through the `wicked-estate-core` traits, so it runs against any `GraphStore`
//! (including the `Box<dyn GraphStore>` from `wicked_estate_store::open_store`, i.e. a future external DB).
//!
//! ## Incremental indexing (Wave 2.6)
//!
//! On re-index, only CHANGED or NEW files are re-extracted. UNCHANGED files (same xxh3 digest)
//! are skipped entirely — their nodes, edges, and unresolved-refs remain in the store from the
//! previous run. DELETED files (previously indexed but no longer present) have their contributions
//! removed via `store.remove_file(path)`.
//!
//! "Previously indexed" means [`GraphRead::indexed_files`] — this indexer's own digest rows — never
//! "every node in the store". A store may be shared with a writer that keeps non-source nodes in
//! it; sweeping those is data loss, not incremental indexing (FINDING-067).
//!
//! ### Importer re-extraction scope (lane relative-imports, Decision J)
//!
//! - **DELETED (or renamed) file**: its direct importers — files with a `File → File` `Imports`
//!   edge into it — are FORCED through re-extraction in the same run, so their relative-import
//!   refs re-park honestly instead of silently losing the edge to `prune_dangling_edges`.
//!   The importer set is collected BEFORE the removal batch (the batch destroys the very edges
//!   the discovery walks) and only from the original deleted list — no transitive cascade.
//! - **MODIFIED file**: nothing is forced. The importer's `File → File` edge survives
//!   `remove_file` by store semantics (the DELETE matches `file = ?1 OR source IN
//!   nodes-of-file`; the importer's edge has `file = importer`), and the target's File node is
//!   re-created under the same path-keyed SymbolId before `prune_dangling_edges` runs — the
//!   edge stays valid with zero importer re-extraction. Forcing importers on modification
//!   would re-parse every importer of a hub file on every save, for nothing.
//!
//! ### Known limitations (still true)
//!
//! An unchanged file F that CALLS a symbol S newly added by a changed file C will NOT have its
//! call to S re-resolved in this run — F's UnresolvedRef persists until F changes or a full
//! re-index. Likewise an importer whose relative-import ref was PARKED (target absent) is not
//! re-resolved when the target is later added: no edge exists to discover it by (D01-7 audit).
//!
//! ## Many repos, one graph
//!
//! [`index_path_as`] takes an optional repo label. With one, every path this run stores is
//! namespaced (`ledger/src/lib.rs`) — including the paths embedded in SymbolIds — so several repos
//! co-exist in one db without colliding, each with its own provenance and its own delete-sweep
//! scope. Without one, nothing is prefixed and the run is byte-identical to before. Either way the
//! [`repo_scope::guard`] refuses, before writing anything, an index that would overwrite another
//! repo's rows. Co-location only: **edges do not resolve across repos** — resolution is scoped to
//! the indexed repo's own nodes. See [`repo_scope`].

pub mod repo_scope;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rayon::prelude::*;
use wicked_estate_core::{
    ChangeOp, Edge, EdgeKind, Extraction, Extractor, GraphRead, GraphStats, Language, Location,
    Node, NodeKind, NodeSemantics, RepoInfo, ResolutionTier, Resolver, Result, SourceFile, Span,
    Symbol, SymbolId, SymbolIndex, SymbolQuery, TraversalSpec,
};
use wicked_estate_extract::{
    BlazeBrlExtractor, CicsSqlExtractor, DrlExtractor, ExtraEdgeExtractor, HlasmExtractor,
    IaCExtractor, ImsExtractor, JclExtractor, MqExtractor, RacfExtractor, RegoRulesExtractor,
    TfstateCollector,
    treesitter::{TreeSitterExtractor, extractor_for_extension, is_minified_or_huge},
};
use wicked_estate_resolve::{
    ImportMapResolver, InfraResolver, NameResolver, ScopedNameResolver, resolve_all,
};
use wicked_estate_retrieve::Embedder;
use wicked_estate_store::GraphStoreMutExt;
use wicked_estate_store::SqliteStore;
use xxhash_rust::xxh3::xxh3_64;

/// Extensions handled by the grammar-less line extractors (JCL job streams, HLASM assembler), whose
/// dispatch lives in `index_path`'s per-file closure rather than the tree-sitter `ext_map`.
fn is_grammarless_ext(ext: &str) -> bool {
    matches!(
        ext,
        "jcl"
            | "job"
            | "cntl"
            | "hlasm"
            | "asm"
            | "mlc"
            | "racf"
            | "dbd"
            | "psb"
            | "mqsc"
            | "drl"
            | "brl"
    ) || is_xml_rules_ext(ext)
}

/// Dispatch a grammar-less (non-tree-sitter) extractor by file extension — the mainframe estate
/// languages (JCL, HLASM, RACF security, IMS DBD/PSB, MQ MQSC) plus the heuristic rules-engine
/// formats (Drools DRL, FICO Blaze `.brl`) and, behind the `xml-rules` feature, the XML rules
/// formats (Camunda DMN, Progress Corticon). Returns `None` for everything else.
fn grammarless_extractor(ext: &str) -> Option<Box<dyn Extractor>> {
    match ext {
        "jcl" | "job" | "cntl" => Some(Box::new(JclExtractor::new())),
        "hlasm" | "asm" | "mlc" => Some(Box::new(HlasmExtractor::new())),
        "racf" => Some(Box::new(RacfExtractor::new())),
        "dbd" | "psb" => Some(Box::new(ImsExtractor::new())),
        "mqsc" => Some(Box::new(MqExtractor::new())),
        "drl" => Some(Box::new(DrlExtractor::new())),
        "brl" => Some(Box::new(BlazeBrlExtractor::new())),
        _ => xml_rules_extractor(ext),
    }
}

/// XML rules-engine extensions, dispatched only when the `xml-rules` feature is enabled.
#[cfg(feature = "xml-rules")]
fn is_xml_rules_ext(ext: &str) -> bool {
    matches!(ext, "ers" | "erf" | "ecore" | "dmn")
}
#[cfg(not(feature = "xml-rules"))]
fn is_xml_rules_ext(_ext: &str) -> bool {
    false
}

/// Dispatch the XML rules-engine extractors (Progress Corticon, Camunda DMN). Compiled in only with
/// the `xml-rules` feature; a no-op (always `None`) otherwise so the base binary stays dep-light.
#[cfg(feature = "xml-rules")]
fn xml_rules_extractor(ext: &str) -> Option<Box<dyn Extractor>> {
    use wicked_estate_extract::{CamundaDmnExtractor, CorticonExtractor};
    match ext {
        "ers" | "erf" | "ecore" => Some(Box::new(CorticonExtractor::new())),
        "dmn" => Some(Box::new(CamundaDmnExtractor::new())),
        _ => None,
    }
}
#[cfg(not(feature = "xml-rules"))]
fn xml_rules_extractor(_ext: &str) -> Option<Box<dyn Extractor>> {
    None
}

/// Language-based extraction dispatch for ONE file: grammar-less line extractors first, then the
/// IaC sniff for YAML/JSON, then tree-sitter by extension. Returns `None` when no extractor claims
/// the file or extraction fails — the caller decides whether the file still matters (e.g. because
/// a drop-in extra-edge rule targets it).
fn base_extraction(
    rel_path: &str,
    ext: &str,
    text: &str,
    ext_map: &HashMap<String, TreeSitterExtractor>,
) -> Option<Extraction> {
    // Grammar-less mainframe estate languages (JCL batch, HLASM assembler, RACF security,
    // IMS DBD/PSB data, MQ MQSC messaging): line/macro extractors dispatched by extension.
    // They map the estate into the same graph as the tree-sitter languages.
    if let Some(extractor) = grammarless_extractor(ext) {
        let language = extractor.languages().into_iter().next()?;
        let sf = SourceFile {
            path: rel_path.to_string(),
            language: language.clone(),
            text: text.to_string(),
        };
        let mut extraction = extractor.extract(&sf).ok()?;
        // Give every grammar-less file the same File node + Contains edges the tree-sitter
        // path emits, so the indexed-file count is honest and file-level blast-radius
        // ("what's defined in queues.mqsc?") works uniformly across all languages.
        let file_symbol = Symbol::file(rel_path).id();
        let floc = Location::new(rel_path, Span::ZERO);
        let child_syms: Vec<SymbolId> = extraction.nodes.iter().map(|n| n.symbol.clone()).collect();
        for sym in child_syms {
            extraction.local_edges.push(
                Edge::new(
                    file_symbol.clone(),
                    sym,
                    EdgeKind::Contains,
                    ResolutionTier::Parsed,
                    "grammarless",
                )
                .with_location(floc.clone()),
            );
        }
        extraction.nodes.push(Node::new(
            file_symbol,
            NodeKind::File,
            rel_path.to_string(),
            language,
            floc,
        ));
        return Some(extraction);
    }

    // IaC sniff dispatch for YAML / JSON files: cheap string check before extraction.
    // CloudFormation: top-level `Resources:` key.
    // Kubernetes: `kind:` + `apiVersion:` keys present.
    // All other YAML/JSON: fall through to the normal tree-sitter extractor.
    if matches!(ext, "yaml" | "yml" | "json") {
        let is_cfn = text.contains("Resources:")
            && (text.contains("AWSTemplateFormatVersion")
                || text.contains("CloudFormation")
                || text
                    .lines()
                    .any(|l| l.trim_start_matches(' ') == "Resources:"));
        let is_k8s = text.contains("kind:") && text.contains("apiVersion:");

        if is_cfn {
            let extractor = IaCExtractor::cloudformation();
            let language = extractor.languages().into_iter().next()?;
            let sf = SourceFile {
                path: rel_path.to_string(),
                language,
                text: text.to_string(),
            };
            return extractor.extract(&sf).ok();
        } else if is_k8s {
            let extractor = IaCExtractor::kubernetes();
            let language = extractor.languages().into_iter().next()?;
            let sf = SourceFile {
                path: rel_path.to_string(),
                language,
                text: text.to_string(),
            };
            return extractor.extract(&sf).ok();
        }
        // Non-IaC YAML/JSON — fall through to normal extractor below.
    }

    let extractor = ext_map.get(ext)?;
    let language = extractor
        .languages()
        .into_iter()
        .next()
        .expect("extractor has a language");
    let sf = SourceFile {
        path: rel_path.to_string(),
        language,
        text: text.to_string(),
    };
    let mut extraction = extractor.extract(&sf).ok()?;
    // COBOL: supplement the structural extraction with embedded EXEC CICS / EXEC SQL
    // commands (CICS LINK/XCTL programs + maps, Db2 tables). The COBOL grammar parses EXEC
    // blocks opaquely, so these come from a regex pass over the same source.
    if matches!(ext, "cbl" | "cob" | "cobol" | "cpy") {
        if let Ok(emb) = CicsSqlExtractor::new().extract(&sf) {
            extraction.nodes.extend(emb.nodes);
            extraction.local_edges.extend(emb.local_edges);
            extraction.refs.extend(emb.refs);
        }
    }
    // Rego: supplement the tree-sitter code parse (rules-as-functions) with the W15 rules
    // graph (RuleSet/Rule/Condition/Action/Fact), so policies surface in RulesInventory.
    if ext == "rego" {
        if let Ok(rules) = RegoRulesExtractor::new().extract(&sf) {
            extraction.nodes.extend(rules.nodes);
            extraction.local_edges.extend(rules.local_edges);
            extraction.refs.extend(rules.refs);
        }
    }
    Some(extraction)
}

/// Symbol index built ONCE from the store for the resolver pass. The resolver calls `by_name`
/// once per reference; at ~190k refs × N resolvers, a per-call SQL lookup was the dominant cost
/// (≈0.5M queries). Loading `all_nodes()` into in-memory maps makes resolution lookups O(1).
struct InMemoryIndex {
    by_name: HashMap<String, Vec<Node>>,
    by_id: HashMap<SymbolId, Node>,
}

impl InMemoryIndex {
    /// `scope` restricts the index to one repo's nodes (path prefix `<label>/`). A labelled index
    /// resolves against its OWN repo only — same candidate set it would see in a private db, so
    /// co-locating repos neither creates cross-repo edges nor makes a name ambiguous that was
    /// unique before. `None` = every node, the single-repo behaviour.
    fn build(store: &dyn GraphRead, scope: Option<&str>) -> Result<Self> {
        let mut by_name: HashMap<String, Vec<Node>> = HashMap::new();
        let mut by_id = HashMap::new();
        for n in store.all_nodes()? {
            if scope.is_some_and(|p| !n.location.file.starts_with(p)) {
                continue;
            }
            by_name.entry(n.name.clone()).or_default().push(n.clone());
            by_id.insert(n.symbol.clone(), n);
        }
        Ok(Self { by_name, by_id })
    }

    /// Every node, for passes that derive edges from the whole population (the estate join) —
    /// reuses this already-loaded index instead of re-reading `all_nodes()` from the store.
    fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.by_id.values()
    }
}

impl SymbolIndex for InMemoryIndex {
    fn by_name(&self, name: &str) -> Vec<Node> {
        self.by_name.get(name).cloned().unwrap_or_default()
    }
    fn get(&self, id: &SymbolId) -> Option<Node> {
        self.by_id.get(id).cloned()
    }
    fn all_nodes(&self) -> wicked_estate_core::Result<Vec<Node>> {
        Ok(self.by_id.values().cloned().collect())
    }
}

/// Collect source files under `root` using the `ignore` crate — gitignore-aware, skips hidden +
/// VCS/build/vendor dirs (so indexing a real repo doesn't drown in `target/`, `node_modules/`, etc.).
fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .require_git(false)
        // Vendor/build dirs AND wicked's own generated measurement artifacts. The latter close a
        // self-reference trap: the domain-extraction coverage phase writes `coverage-report.json`
        // (and the modeler exports `requirements_graph.json`) through a RELATIVE `--out`, so on any
        // launch whose CWD is an indexed root — an onboard index, a bare `wicked-core coverage`
        // call — the report lands at the repo root, un-hidden. Indexing it back in turns each of its
        // JSON keys into a behavior-bearing `Struct` node that can never be domain-annotated, so the
        // NEXT coverage run is pinned below 1.0 forever: the measurement corrupts the measurand.
        // Governed worktree runs escape the trap only because their CWD is under `.wicked/` (hidden);
        // this skip closes the hole for every other launch path regardless of where the file lands.
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_string_lossy().as_ref(),
                "target"
                    | "node_modules"
                    | ".wicked-estate"
                    | ".reference"
                    | "dist"
                    | "build"
                    | "coverage-report.json"
                    | "requirements_graph.json"
            )
        })
        .build()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .collect()
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Load every drop-in extra-edge rule file (`.wicked-estate-extractors/*.toml`) under `root`.
///
/// Returns the compiled extractor (`None` when the dir is absent, has no rules, or the rules fail
/// to parse — parse failures print a LOUD `EXTRA-EDGE:` marker, never silently no-op) plus a
/// digest of the raw rule bytes. The digest is compared against the store's `extra_rules_digest`
/// meta key so *editing the rules* forces a full re-extract — otherwise edges produced by the old
/// rule set would linger on unchanged files.
fn load_extra_edge_rules(root: &Path) -> (Option<ExtraEdgeExtractor>, String) {
    let dir = root.join(".wicked-estate-extractors");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return (None, String::new());
    };
    let mut paths: Vec<PathBuf> = rd
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    paths.sort();

    let mut entries: Vec<(String, String)> = Vec::new();
    for p in &paths {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match std::fs::read_to_string(p) {
            Ok(contents) => entries.push((name, contents)),
            Err(e) => eprintln!("EXTRA-EDGE: cannot read {}: {e} — skipping", p.display()),
        }
    }
    if entries.is_empty() {
        return (None, String::new());
    }
    // Digest the raw bytes (name + contents), not the parsed rules, so ANY edit — including one
    // that breaks parsing — changes the digest and triggers the full re-extract.
    let mut digest_input = String::new();
    for (name, contents) in &entries {
        digest_input.push_str(name);
        digest_input.push('\0');
        digest_input.push_str(contents);
        digest_input.push('\0');
    }
    let digest = file_digest(digest_input.as_bytes());

    match ExtraEdgeExtractor::from_toml_named(&entries) {
        Ok(x) if x.rule_count() > 0 => (Some(x), digest),
        Ok(_) => (None, digest),
        Err(e) => {
            eprintln!("EXTRA-EDGE: {e} — extra-edge rules DISABLED for this run");
            (None, digest)
        }
    }
}

/// Second walk for extra-edge rule targets: INCLUDES hidden paths, because drop-in rules routinely
/// target dot-dir catalogs (e.g. `.claude-plugin/archetypes.json`) that [`collect_source_files`]'s
/// hidden-filter skips. Still gitignore-aware; skips `.git`, the rules dir itself, and exactly the
/// same build/vendor dir names as [`collect_source_files`] — the two walks must agree on what is
/// visible so a rule behaves the same whether or not its target is also an ordinary source file
/// (anything beyond that list, e.g. a committed Go-style `vendor/`, is gitignore's call in BOTH
/// walks). Only files matching at least one rule's `file_glob` are returned.
fn collect_extra_rule_files(root: &Path, rules: &ExtraEdgeExtractor) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .require_git(false)
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_string_lossy().as_ref(),
                ".git"
                    | ".wicked-estate-extractors"
                    | "target"
                    | "node_modules"
                    | ".wicked-estate"
                    | ".reference"
                    | "dist"
                    | "build"
                    | "coverage-report.json"
                    | "requirements_graph.json"
            )
        })
        .build()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .filter(|p| rules.matches_path(&rel(root, p)))
        .collect()
}

/// Compute an xxh3 hex digest for a byte slice.
fn file_digest(bytes: &[u8]) -> String {
    format!("{:016x}", xxh3_64(bytes))
}

/// Collect git provenance for the repo rooted at `path`.
///
/// Shells `git -C <path>` once per field; every failure (git absent, not a repo, non-zero exit)
/// produces `None`/`false` for that field — never panics.  Follows the same pattern as
/// [`commits_behind`].
pub fn collect_repo_info(path: &Path) -> RepoInfo {
    let r = path.to_string_lossy();
    // Helper: run one git command, return trimmed stdout or None on any failure.
    let run = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git").args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        Some(s.trim().to_string())
    };

    let commit = run(&["-C", &r, "rev-parse", "HEAD"]);
    // Detached HEAD reports the literal string "HEAD" — treat that as None.
    let branch = run(&["-C", &r, "rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD");
    let remote = run(&["-C", &r, "remote", "get-url", "origin"]);
    let dirty = run(&["-C", &r, "status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    RepoInfo {
        commit,
        branch,
        remote,
        dirty,
    }
}

/// Index every supported source file under `root` into `store`, then resolve cross-file edges.
/// Returns the resulting graph stats. This is the two-phase EXTRACT→RESOLVE→STORE pipeline.
///
/// On the first run the store is empty, so every file is "new" (full index). On subsequent runs
/// only CHANGED or NEW files are re-extracted; UNCHANGED files are skipped (incremental).
///
/// W7.4: stores the canonical root path in store meta so staleness checks can locate the git repo.
/// W9.4: InfraResolver is included in the resolver slice so IaC/tfstate resource refs resolve.
/// W11.3: bumps the store version at the start (invalidating stale cache) and populates the
///   `pagerank.top` cache entry at the end with the top-N PageRank scores.
pub fn index_path(store: &mut dyn GraphStoreMutExt, root: &Path) -> Result<GraphStats> {
    index_path_as(store, root, None)
}

/// [`index_path`] with an optional repo label — the multi-repo entry point.
///
/// `repo = Some("ledger")` namespaces every path this run stores as `ledger/<rel>`, which is what
/// makes `files.path`, `nodes.file` and the path-embedded SymbolIds unique per repo; it scopes the
/// delete-sweep and the resolver's candidate set to that repo, and records provenance under
/// `repo:ledger:*` instead of the singular `repo_*` keys. `repo = None` is the single-repo path
/// and behaves exactly as it always has.
///
/// Refuses (before any write) when the run would overwrite another repo's rows — see
/// [`repo_scope::guard`].
pub fn index_path_as(
    store: &mut dyn GraphStoreMutExt,
    root: &Path,
    repo: Option<&str>,
) -> Result<GraphStats> {
    let t = std::time::Instant::now();

    if let Some(label) = repo {
        repo_scope::validate_label(label)?;
    }
    // Path prefix carried by every row this run writes. Also the delete-sweep's scope.
    let scope = repo.map(repo_scope::prefix);

    // W7: capture git provenance once per index run. Non-fatal: git absent / not a repo →
    // all-None RepoInfo, which is a valid default. Collected BEFORE the guard runs — the remote
    // it carries is the evidence the guard decides repo identity on.
    let repo_info = collect_repo_info(root);

    // ── Derive the set of previously-indexed file paths ─────────────────────────────────────
    // Every path a prior `index_path` recorded through a file-writing call — `set_file_digest` or
    // `set_file_content` — and nothing else. Both count: this indexer issues both for every file it
    // takes, and scoping to digests alone would miss a path whose content was stored but whose
    // digest write did not land. `remove_file` drops nodes and the file row atomically, so a path
    // removed last run does not reappear here.
    //
    // This USED to read `all_nodes()`, mapping each node's `location.file`. That answers a
    // different question: not "what did I index?" but "what is in this store?" — and the sweep
    // below deletes everything in that set which is not on disk right now. The two sets are equal
    // only in a store no other writer touches, and that assumption does not survive contact with a
    // shared store.
    //
    // It did not survive here. An orchestrator sharing this store kept its operational domain
    // objects as nodes with synthetic `location.file` values — `agent_session/<id>`,
    // `work_unit/<id>`, `validator_vault/<pin>`, `repo_entry/<id>`. Indexing one repo subdirectory
    // into that store swept all 833 of them: every session, work unit, workflow, work output,
    // validator vault entry, policy and repo registration, in one transaction, including the run
    // whose worker issued the index — which then died with `run not found` (FINDING-067).
    //
    // A digest row can only come from this indexer, so scoping to it makes reaching a node we did
    // not create impossible rather than merely unlikely. Nothing about the deleted-file behaviour
    // changes: a real source file that vanished still has its digest row from last run and is
    // still swept.
    let all_indexed: HashSet<String> = store
        .indexed_files()?
        .into_iter()
        .filter(|f| !f.is_empty())
        .collect();

    // THE GUARD. Read-only, and it runs before the first mutation below: a refusal must leave the
    // graph exactly as it was.
    repo_scope::guard(store, &all_indexed, repo, root, &repo_info)?;

    // A labelled run owns only its own prefix. Un-prefixed: the whole set, as before.
    let previously_indexed: HashSet<String> = match &scope {
        Some(p) => all_indexed
            .into_iter()
            .filter(|f| f.starts_with(p.as_str()))
            .collect(),
        None => all_indexed,
    };

    // W11.3: bump the graph version so any prior cache entries become stale.
    store.version_bump();

    // W7.4: persist the indexed root so staleness checks can find the git repo. In a multi-repo
    // graph this is the LAST root indexed; each repo's own root is in its `repo:<label>:root`.
    store.meta_set_key("indexed_root", &root.to_string_lossy());
    // Read the previously-stored binary version BEFORE overwriting it so we can detect a
    // version upgrade and force full re-extraction when the binary has changed. Per repo: an
    // upgrade must force a re-extract of EACH repo the next time it is indexed, not just the
    // first one to run under the new binary.
    let version_key = repo_scope::meta_key(repo, "indexed_version");
    let prev_version = store.meta_get_key(&version_key);
    let mut force_full = prev_version
        .as_deref()
        .is_some_and(|v| v != env!("CARGO_PKG_VERSION"));
    store.meta_set_key(&version_key, env!("CARGO_PKG_VERSION"));
    if force_full {
        eprintln!(
            "VERSION CHANGE detected (v{prev} → v{cur}): forcing full re-extraction",
            prev = prev_version.as_deref().unwrap_or("?"),
            cur = env!("CARGO_PKG_VERSION"),
        );
    }

    // Drop-in extra-edge rules (`.wicked-estate-extractors/*.toml`, extractor SDK Part 2).
    // A changed rule set forces a full re-extract: extra edges live in per-file extractions, so
    // only re-extracting every file purges edges the OLD rules injected into unchanged files.
    // The rules are read from THIS root, so the digest is per repo too.
    let (extra, extra_digest) = load_extra_edge_rules(root);
    let digest_key = repo_scope::meta_key(repo, "extra_rules_digest");
    let prev_extra = store.meta_get_key(&digest_key).unwrap_or_default();
    if prev_extra != extra_digest {
        if prev_version.is_some() {
            eprintln!("EXTRA-EDGE rules changed: forcing full re-extraction");
        }
        force_full = true;
    }
    store.meta_set_key(&digest_key, &extra_digest);

    // W7: persist the git provenance collected above. A labelled run writes `repo:<label>:*` and
    // leaves the singular `repo_*` keys untouched — that is what stops the second repo indexed
    // into a graph from clobbering the first's commit/branch/remote/dirty.
    match repo {
        Some(label) => repo_scope::write_record(store, label, root, &repo_info),
        None => {
            let _ = store.set_repo_info(&repo_info);
        }
    }

    let mut files = collect_source_files(root);

    // Admit extra-edge rule targets the main walk cannot see (hidden dot-dir files). Rule-matched
    // files that are ALSO ordinary source files stay deduped — they get both passes below.
    if let Some(x) = &extra {
        let mut known: HashSet<PathBuf> = files.iter().cloned().collect();
        for p in collect_extra_rule_files(root, x) {
            if known.insert(p.clone()) {
                files.push(p);
            }
        }
    }

    // Build per-extension extractor cache so each language's Query is compiled ONCE.
    let mut extractor_cache: HashMap<String, Option<TreeSitterExtractor>> = HashMap::new();
    for path in &files {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        extractor_cache
            .entry(ext.clone())
            .or_insert_with(|| extractor_for_extension(&ext));
    }
    let ext_map: HashMap<String, TreeSitterExtractor> = extractor_cache
        .into_iter()
        .filter_map(|(k, v)| v.map(|ex| (k, ex)))
        .collect();

    // ── Classify files: UNCHANGED / CHANGED+NEW ──────────────────────────────────────────────
    // Read all file bytes upfront (needed for digest comparison); filter to supported extensions.
    // Parallelise the read+digest step across all files — I/O-bound, safe to fan out.
    struct FileWork {
        abs: PathBuf,
        /// Storage identity: repo-relative, and `<label>/`-prefixed under a repo label. Every
        /// path-derived id (file row, `nodes.file`, the path inside a SymbolId) comes from this.
        rel: String,
        /// The same path WITHOUT the label — what drop-in extra-edge rules glob against, so a
        /// rule written `src/**/*.js` keeps matching once the repo is labelled.
        raw: String,
        bytes: Vec<u8>,
        digest: String,
    }

    let supported: Vec<(PathBuf, String)> = files
        .into_iter()
        .filter_map(|p| {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let rel_path = rel(root, &p);
            // Admit tree-sitter languages AND the grammar-less line extractors (JCL / HLASM), whose
            // dispatch lives in the per-file closure below — they are NOT in `ext_map`. Files
            // matched by a drop-in extra-edge rule are admitted too (they may have no language
            // extractor at all — e.g. a JSON catalog only the rules care about).
            let extra_match = extra.as_ref().is_some_and(|x| x.matches_path(&rel_path));
            if ext_map.contains_key(&ext) || is_grammarless_ext(&ext) || extra_match {
                Some((p, rel_path))
            } else {
                None
            }
        })
        .collect();

    let current_rel_paths: HashSet<String> = supported
        .iter()
        .map(|(_, r)| repo_scope::namespaced(repo, r))
        .collect();

    let work: Vec<FileWork> = supported
        .into_par_iter()
        .filter_map(|(abs, rel_path)| {
            let bytes = std::fs::read(&abs).ok()?;
            let digest = file_digest(&bytes);
            Some(FileWork {
                abs,
                rel: repo_scope::namespaced(repo, &rel_path),
                raw: rel_path,
                bytes,
                digest,
            })
        })
        .collect();

    // ── Remove DELETED files ─────────────────────────────────────────────────────────────────
    let deleted: Vec<String> = previously_indexed
        .iter()
        .filter(|p| !current_rel_paths.contains(*p))
        .cloned()
        .collect();
    // Lane relative-imports (Decision J): collect each DELETED file's direct importers — files
    // holding a File→File `Imports` edge into it — BEFORE the removal batch, which destroys the
    // very edges this discovery walks. The set is computed once, from the original deleted list
    // only (no transitive cascade), and forces those files through re-extraction below so their
    // relative-import refs re-park honestly. A merely-MODIFIED target needs nothing: its
    // importers' edges survive `remove_file` by store semantics (see the module doc).
    let mut forced_importers: HashSet<String> = HashSet::new();
    for path in &deleted {
        let target_id = Symbol::file(path.clone()).id();
        for e in store.neighbors(&target_id, wicked_estate_core::Direction::Dependents)? {
            if e.kind != EdgeKind::Imports {
                continue;
            }
            if let Some(n) = store.get_node(&e.source)? {
                if matches!(n.kind, NodeKind::File) {
                    forced_importers.insert(n.location.file.clone());
                }
            }
        }
    }
    for d in &deleted {
        forced_importers.remove(d); // a deleted importer is gone, not re-extracted
    }
    if !deleted.is_empty() {
        store.begin_batch()?;
        for path in &deleted {
            store.remove_file(path)?;
            // W7.1: log the removal so subscribers see the deletion delta.
            let _ = store.log_change(ChangeOp::Remove, path);
        }
        store.commit_batch()?;
    }

    // ── Split CHANGED/NEW from UNCHANGED ────────────────────────────────────────────────────
    let mut changed: Vec<FileWork> = Vec::new();
    let mut unchanged_count: usize = 0;
    for fw in work {
        let stored = store.file_digest(&fw.rel)?;
        // A direct importer of a DELETED file is forced into `changed` even with a matching
        // digest (Decision J): consulted HERE, while the FileWork is still alive — the
        // unchanged arm drops it.
        let forced = forced_importers.contains(&fw.rel);
        if !force_full && !forced && stored.as_deref() == Some(&fw.digest) {
            // UNCHANGED: skip extraction entirely; its nodes/edges already in the store.
            unchanged_count += 1;
        } else {
            changed.push(fw);
        }
    }

    // If nothing changed, skip all phases.
    if changed.is_empty() {
        return store.stats();
    }

    // ── Remove stale contributions from CHANGED files ───────────────────────────────────────
    // A changed file's old nodes/edges must be purged before we write the new extraction.
    store.begin_batch()?;
    for fw in &changed {
        // Only remove if the file was previously indexed (it has a stored digest or prior nodes).
        // remove_file is idempotent when called on a file with no rows.
        store.remove_file(&fw.rel)?;
    }
    store.commit_batch()?;

    // ── EXTRACT: parse + query only CHANGED/NEW files in parallel ───────────────────────────
    // Each element carries the rel path + extraction + source text (for content storage).
    // Minified/huge files are detected via is_minified_or_huge and skipped; we count them
    // so index_path can report how many files were silently passed over.
    use std::sync::atomic::{AtomicUsize, Ordering};
    let skipped_minified = AtomicUsize::new(0);

    let extractions: Vec<(String, wicked_estate_core::Extraction, String)> = changed
        .par_iter()
        .filter_map(|fw| {
            let ext = fw
                .abs
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let text = String::from_utf8(fw.bytes.clone()).ok()?;
            let extra_rules = extra.as_ref().filter(|x| x.matches_path(&fw.raw));

            // Skip minified or huge files before any parsing — they generate noise, not signal.
            // A file explicitly targeted by an extra-edge rule still gets the (regex-only) extra
            // pass below; only the language extraction is skipped for it.
            let minified = is_minified_or_huge(&text);
            if minified && extra_rules.is_none() {
                skipped_minified.fetch_add(1, Ordering::Relaxed);
                return None;
            }

            let mut extraction = if minified {
                Extraction::default()
            } else {
                // None when no language extractor claims the file (e.g. a JSON catalog only the
                // extra-edge rules target) — the extra pass below still runs on it.
                base_extraction(&fw.rel, &ext, &text, &ext_map).unwrap_or_default()
            };

            // ── Extra-edge pass: drop-in domain rules (.wicked-estate-extractors/) ────────────
            if let Some(x) = extra_rules {
                // Rules glob against the RAW repo-relative path; the ids they mint are then
                // rewritten into the repo's namespace (no-op when un-labelled).
                let sf = SourceFile {
                    path: fw.raw.clone(),
                    language: Language::new("text"),
                    text: text.clone(),
                };
                let mut ee = x.extract_extra(&sf);
                if let Some(label) = repo {
                    repo_scope::namespace_extra(&mut ee, label, &fw.rel);
                }
                // Injected edges hang off the file node — guarantee it exists even for files no
                // language extractor claims (e.g. a hidden `.claude-plugin/archetypes.json`).
                let file_symbol = Symbol::file(&fw.rel).id();
                if !extraction.nodes.iter().any(|n| n.symbol == file_symbol) {
                    extraction.nodes.push(Node::new(
                        file_symbol,
                        NodeKind::File,
                        fw.rel.clone(),
                        Language::new("text"),
                        Location::new(&fw.rel, Span::ZERO),
                    ));
                }
                extraction.nodes.extend(ee.nodes);
                extraction.local_edges.extend(ee.edges);
                extraction.refs.extend(ee.unresolved_refs);
            }

            if extraction.nodes.is_empty()
                && extraction.local_edges.is_empty()
                && extraction.refs.is_empty()
            {
                return None;
            }
            Some((fw.rel.clone(), extraction, text))
        })
        .collect();

    let skipped_min_count = skipped_minified.load(Ordering::Relaxed);
    if skipped_min_count > 0 {
        eprintln!("SKIPPED_MINIFIED: {skipped_min_count} file(s)");
    }

    // ── Emit extraction counters (best-effort) ───────────────────────────────────────────────
    {
        let total_symbols: usize = extractions.iter().map(|(_, e, _)| e.nodes.len()).sum();
        let files_extracted = extractions.len();
        let sink = wicked_estate_observe::init_sink_from_env();
        let resource = wicked_estate_core::observability::Resource::service(
            "wicked_estate",
            env!("CARGO_PKG_VERSION"),
        );
        let scope =
            wicked_estate_core::observability::InstrumentationScope::new("wicked_estate.index");
        use wicked_estate_core::observability::*;
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let files_metric = Metric {
            name: "wicked_estate.extract.files_total".to_string(),
            description: String::new(),
            unit: "1".to_string(),
            data: MetricData::Sum {
                data_points: vec![NumberDataPoint {
                    attributes: vec![],
                    start_time_unix_nano: t,
                    time_unix_nano: t,
                    value: MetricValue::I64(files_extracted as i64),
                }],
                temporality: AggregationTemporality::Delta,
                is_monotonic: true,
            },
        };
        if let Err(e) = sink.export_metrics(&resource, &scope, &[files_metric]) {
            eprintln!("telemetry: {e}");
        }

        if files_extracted > 0 {
            let avg_symbols = total_symbols as f64 / files_extracted as f64;
            let sym_metric = Metric {
                name: "wicked_estate.extract.symbols_per_file".to_string(),
                description: String::new(),
                unit: "1".to_string(),
                data: MetricData::Histogram {
                    data_points: vec![HistogramDataPoint {
                        attributes: vec![],
                        start_time_unix_nano: t,
                        time_unix_nano: t,
                        count: files_extracted as u64,
                        sum: avg_symbols * files_extracted as f64,
                        bucket_counts: vec![files_extracted as u64],
                        explicit_bounds: vec![],
                    }],
                    temporality: AggregationTemporality::Delta,
                },
            };
            if let Err(e) = sink.export_metrics(&resource, &scope, &[sym_metric]) {
                eprintln!("telemetry: {e}");
            }
        }
    }

    // ── WRITE ────────────────────────────────────────────────────────────────────────────────
    // write-nodes: upsert nodes rows (hot path — prepare_cached, single transaction).
    // write-content: zstd-compress + upsert content/files rows.
    // write-FTS: bulk populate nodes_fts after all nodes are written (ONE INSERT SELECT).
    //
    // All writes run inside ONE outer transaction so SQLite flushes once, not per row.
    // FTS5 is populated in a single bulk pass AFTER all node rows exist — this replaces the
    // previous per-node DELETE+INSERT pair which caused ~2× extra write traffic on the FTS
    // shadow tables and forced the FTS trie to be rebuilt incrementally rather than in one shot.
    let mut all_refs = Vec::new();

    store.begin_batch()?;
    for (rel_path, extraction, text) in &extractions {
        store.upsert_nodes_skip_fts(&extraction.nodes)?;
        store.upsert_edges(&extraction.local_edges)?;
        all_refs.extend(extraction.refs.iter().cloned());
        // Store source text for the file (best-effort; log on failure, don't abort).
        if let Err(e) = store.set_file_content(rel_path, text) {
            eprintln!("warning: set_file_content({rel_path}) failed: {e}");
        }
    }

    // Bulk-rebuild FTS for every node that belongs to a changed file in one SQL pass.
    // This is O(1) SQL statements instead of O(2 × nodes) DELETE+INSERT pairs.
    store.bulk_rebuild_fts_for_files(
        &extractions
            .iter()
            .map(|(p, _, _)| p.as_str())
            .collect::<Vec<_>>(),
    )?;

    // Record the new digest for each changed/new file so subsequent runs can skip it.
    // W7.1: emit an Upsert change-log entry here — after the new state is durably committed —
    // so the store is in a consistent state before the subscriber sees the delta.
    for fw in &changed {
        store.set_file_digest(&fw.rel, &fw.digest)?;
        let _ = store.log_change(ChangeOp::Upsert, &fw.rel);
    }
    store.commit_batch()?;

    // ── RESOLVE (changed files only) ─────────────────────────────────────────────────────────
    // Build the in-memory index from ALL nodes (unchanged + newly written). Resolve only the refs
    // that came from the changed files. The resolved edges are written to the store.
    //
    // Importer scope (see the module doc): direct importers of DELETED files were already
    // forced into `changed` above (Decision J). Still-true limitation: an unchanged file F
    // whose CALL to a symbol S newly added by a changed file C stays unresolved until F itself
    // changes or a full re-index; same for a parked relative import whose target appears later.
    let (resolved, estate) = {
        let reader: &dyn GraphRead = &*store;
        // Scoped to this repo: a labelled run resolves against its own nodes only, so a name that
        // is unique inside the repo stays unique no matter how many repos share the graph.
        let index = InMemoryIndex::build(reader, scope.as_deref())?;
        // InfraResolver handles IaC/tfstate resource refs; it does not interfere with code
        // resolvers (it only fires when raw_name maps exclusively to resource nodes).
        // RelativeImportResolver binds relative JS/TS Imports refs to their target File node
        // (exact-path, root-guarded against the repo/label root; lane relative-imports).
        let relative = wicked_estate_resolve::RelativeImportResolver::new(scope.as_deref());
        let resolvers: &[&dyn Resolver] = &[
            &NameResolver,
            &ScopedNameResolver,
            &ImportMapResolver,
            &relative,
            &InfraResolver,
        ];
        let resolved = resolve_all(resolvers, &all_refs, &index)?;
        // Estate cross-domain join: RACF profiles → the datasets/MQ assets they protect, by RACF
        // generic profile matching (most-specific wins). Derived from the full node population (a
        // profile pattern can match assets declared in any file), reusing the index just built.
        let estate = wicked_estate_resolve::estate_edges(index.nodes());
        (resolved, estate)
    };

    // Compute unresolved refs (same logic as full index).
    let resolved_locations: HashSet<Location> =
        resolved.iter().filter_map(|e| e.location.clone()).collect();
    let unresolved: Vec<_> = all_refs
        .iter()
        .filter(|r| !resolved_locations.contains(&r.location))
        .cloned()
        .collect();

    store.begin_batch()?;
    store.upsert_edges(&resolved)?;
    store.upsert_edges(&estate)?;
    store.upsert_unresolved_refs(&unresolved)?;
    store.commit_batch()?;

    // W11.3: populate the pagerank.top cache so subsequent `rank`/`important_symbols` calls
    // can serve from cache instead of recomputing. Best-effort: failure is non-fatal.
    const PAGERANK_CACHE_N: usize = 100;
    let pr_result = {
        let reader: &dyn GraphRead = &*store;
        wicked_estate_rank::ranked_symbols(reader, &[], PAGERANK_CACHE_N)
    };
    match pr_result {
        Ok(ranked) => {
            let json_val = serde_json::to_string(&ranked).unwrap_or_default();
            store.cache_put_key("pagerank.top", &json_val);
        }
        Err(e) => {
            eprintln!("pagerank cache population failed (non-fatal): {e}");
        }
    }

    // Task D: prune dangling edges left by incremental symbol removals. After changed files
    // are removed and re-extracted, edges whose target symbol was deleted (but whose edge row
    // was not covered by the file-scoped DELETE) remain in the store. This cheap O(n_edges)
    // pass cleans them up so blast-radius never returns nodes that no longer exist.
    match store.prune_dangling_edges() {
        Ok(n) if n > 0 => {
            eprintln!("GRAPH-CLEANUP: pruned {n} dangling edge(s) after incremental index");
        }
        Ok(_) => {}
        Err(e) => {
            // Non-fatal: a failure to prune is stale data, not corrupt data.
            eprintln!("warning: prune_dangling_edges failed (non-fatal): {e}");
        }
    }

    // Reclaim freelist pages accumulated by PRAGMA auto_vacuum=INCREMENTAL.
    // Non-fatal: a failure here (e.g. read-only DB) should not abort an otherwise successful index.
    if let Err(e) = store.incremental_vacuum() {
        eprintln!("warning: incremental_vacuum failed (non-fatal): {e}");
    }

    let stats = store.stats()?;
    // Warn when the DB crosses 500 MB — a signal to run `compact`.
    if stats.db_size_bytes > 500 * 1_048_576 {
        eprintln!(
            "wicked-estate: db is {:.0}MB — run `wicked-estate compact` to reclaim space",
            stats.db_size_bytes as f64 / 1_048_576.0
        );
    }

    // Emit a span recording the total index duration and file/node counts.
    let sink = wicked_estate_observe::init_sink_from_env();
    let resource = wicked_estate_core::observability::Resource::service(
        "wicked_estate",
        env!("CARGO_PKG_VERSION"),
    );
    let scope = wicked_estate_core::observability::InstrumentationScope::new("wicked_estate.index");
    let elapsed_ns = t.elapsed().as_nanos() as u64;
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let span = wicked_estate_core::observability::SpanData {
        context: wicked_estate_core::observability::SpanContext {
            trace_id: wicked_estate_core::observability::TraceId::INVALID,
            span_id: wicked_estate_core::observability::SpanId::INVALID,
            trace_flags: 0,
            is_remote: false,
        },
        parent_span_id: None,
        name: "wicked_estate.index_path".to_string(),
        kind: wicked_estate_core::observability::SpanKind::Internal,
        start_time_unix_nano: now_ns.saturating_sub(elapsed_ns),
        end_time_unix_nano: now_ns,
        attributes: vec![
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.files_changed",
                changed.len() as i64,
            ),
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.files_unchanged",
                unchanged_count as i64,
            ),
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.nodes",
                stats.node_count as i64,
            ),
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.edges",
                stats.edge_count as i64,
            ),
        ],
        events: vec![],
        links: vec![],
        status: wicked_estate_core::observability::SpanStatus::ok(),
    };
    // Best-effort: telemetry failure must never abort indexing.
    let _ = sink.export_spans(&resource, &scope, &[span]);

    Ok(stats)
}

/// Set semantic annotations on a symbol — the requirement↔functionality link API. Partial update:
/// each `Some(..)` writes that column, `None` leaves it unchanged.
///
/// `validated_by` names the actor asserting the requirement is satisfied; it is REQUIRED whenever
/// `validated` is given. A claim with no author is what wicked-core#131 was made of — 34,897 nodes
/// self-validated by the agent that wrote them, with nothing recording who decided.
pub fn set_semantics(
    store: &mut dyn GraphStoreMutExt,
    symbol: &str,
    description: Option<&str>,
    requirement: Option<&str>,
    validated: Option<bool>,
    validated_by: Option<&str>,
) -> Result<()> {
    let claim =
        match (validated, validated_by) {
            (Some(v), Some(by)) => Some(
                wicked_estate_core::ValidationClaim::new(v, by)
                    .map_err(|e| wicked_estate_core::Error::Invalid(e.to_string()))?,
            ),
            (Some(_), None) => return Err(wicked_estate_core::Error::Invalid(
                "validating a requirement requires naming the actor asserting it (--validated-by)"
                    .to_string(),
            )),
            (None, _) => None,
        };
    store.set_node_semantics(
        &SymbolId(symbol.to_string()),
        description,
        requirement,
        claim.as_ref(),
    )
}

/// Read a symbol's semantic annotations (description / requirement / validated).
pub fn get_semantics(store: &dyn GraphStoreMutExt, symbol: &str) -> Result<Option<NodeSemantics>> {
    store.node_semantics(&SymbolId(symbol.to_string()))
}

/// All symbols annotated with a requirement — answers "which functionality satisfies R?".
pub fn symbols_for_requirement(
    store: &dyn GraphStoreMutExt,
    requirement: &str,
) -> Result<Vec<Node>> {
    store.find_by_requirement(requirement)
}

/// Find symbols by exact name.
pub fn search(store: &dyn GraphRead, name: &str) -> Result<Vec<Node>> {
    let q = SymbolQuery {
        exact_name: Some(name.to_string()),
        ..Default::default()
    };
    store.find_symbols(&q)
}

/// Blast radius: transitive dependents (callers) of every symbol named `name`, up to `depth`.
///
/// The traversal walks ALL edge kinds (the locked decision); the RESULT is classified through
/// `Subgraph::code_dependents` so import-transit File nodes never surface as dependents of a
/// symbol, while a File start keeps its importers (lane relative-imports Decision G).
pub fn blast_radius_by_name(store: &dyn GraphRead, name: &str, depth: u32) -> Result<Vec<Node>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for sym in search(store, name)? {
        let sub = store.traverse(&sym.symbol, &TraversalSpec::blast_radius(depth))?;
        for n in sub.code_dependents(&sym.symbol, Some(&sym.kind)) {
            if seen.insert(n.symbol.clone()) {
                out.push(n.clone());
            }
        }
    }
    Ok(out)
}

/// Ingest a SCIP index file into the store: reads `scip_path`, correlates its occurrences
/// against nodes already in the store, and upserts the resulting confidence-1.0 edges.
///
/// `root` is unused at this layer (SCIP paths are repo-relative already); it is accepted for
/// caller symmetry with `index_path`.
///
/// Returns the number of edges upserted.
///
/// # Prerequisite
/// The project must already be indexed (`index_path`) so that nodes exist to correlate against.
/// Running `ingest_scip` on an empty store produces 0 edges (no matching nodes → nothing to wire).
pub fn ingest_scip(
    store: &mut dyn GraphStoreMutExt,
    root: &Path,
    scip_path: &Path,
) -> Result<usize> {
    ingest_scip_as(store, root, scip_path, None)
}

/// [`ingest_scip`] against one repo of a multi-repo graph.
///
/// SCIP documents carry REPO-relative paths, while a labelled repo's nodes carry `<label>/…`. The
/// correlation is done on the repo-relative form (the label is stripped from a scratch copy of the
/// nodes and re-applied to the resulting edge locations), so a `.scip` file produced by a plain
/// `scip-typescript` run correlates without the indexer having to know about labels. Without this
/// the correlation matches nothing and reports "0 precise edges" — a silent no-op.
pub fn ingest_scip_as(
    store: &mut dyn GraphStoreMutExt,
    _root: &Path,
    scip_path: &Path,
    repo: Option<&str>,
) -> Result<usize> {
    // Validate here too, not only in `index_path_as` (Copilot on #117). `repo` becomes the
    // `<label>/` prefix that is stripped from SCIP's relative paths and re-applied to the edge
    // locations written back, so an unvalidated label — one containing `/` or `..` — writes
    // nonsensical or forged locations. Label validation is the single thing that makes path
    // forging unreachable; a second entry point that skips it is a hole in that guarantee.
    if let Some(label) = repo {
        repo_scope::validate_label(label)?;
    }
    let bytes = std::fs::read(scip_path).map_err(|e| {
        wicked_estate_core::Error::Io(std::io::Error::other(format!(
            "scip: cannot read {:?}: {e}",
            scip_path
        )))
    })?;

    let mut nodes = store.all_nodes()?;
    let prefix = repo.map(repo_scope::prefix);
    if let Some(p) = &prefix {
        for n in &mut nodes {
            if let Some(rest) = n.location.file.strip_prefix(p.as_str()) {
                n.location.file = rest.to_string();
            }
        }
    }
    let mut edges = wicked_estate_resolve::scip_edges(&bytes, &nodes)?;
    if let Some(p) = &prefix {
        for e in &mut edges {
            if let Some(loc) = &mut e.location {
                loc.file = format!("{p}{}", loc.file);
            }
        }
    }
    let count = edges.len();

    store.begin_batch()?;
    store.upsert_edges(&edges)?;
    store.commit_batch()?;

    Ok(count)
}

/// The most important symbols by global PageRank (the "where do I even start" view for a complex
/// codebase). Returns up to `top_n` `(Node, score)` pairs, highest first.
///
/// W11.3: reads from the `pagerank.top` cache when available (populated at `index_path` end).
/// Falls back to live PageRank computation when the cache is absent or stale.
pub fn important_symbols(store: &dyn GraphStoreMutExt, top_n: usize) -> Result<Vec<(Node, f32)>> {
    // Try the cache first (W11.3).
    if let Some(cached) = store.cache_get_key("pagerank.top") {
        if let Ok(ranked) = serde_json::from_str::<Vec<(String, f32)>>(&cached) {
            let mut out = Vec::new();
            for (sym_str, score) in ranked {
                if out.len() >= top_n {
                    break;
                }
                let id = SymbolId(sym_str);
                if let Some(node) = store.get_node(&id)? {
                    // BR-1 (lane relative-imports): a pre-upgrade `pagerank.top` cache on an
                    // un-reindexed DB still carries File/Import rows (the write path filters
                    // them now, but old caches persist until a re-index). Clean them at READ
                    // time so no consumer of important_symbols ever serves one — the
                    // precondition for graph-view dropping its post-hoc exclusion.
                    if matches!(node.kind, NodeKind::File | NodeKind::Import) {
                        continue;
                    }
                    out.push((node, score));
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }
    }
    // Cache miss or empty — compute live.
    let reader: &dyn GraphRead = store;
    let mut out = Vec::new();
    for (symbol, score) in wicked_estate_rank::ranked_symbols(reader, &[], top_n)? {
        if let Some(node) = store.get_node(&symbol)? {
            out.push((node, score));
        }
    }
    Ok(out)
}

// ── New lib fns (W10 drift, W7.4 staleness, Task B tfstate ingest) ─────────────

/// Ingest a Terraform state file (`*.tfstate`) into `store`.
///
/// Parses `tfstate_json` via [`TfstateCollector`], upserts the resulting resource nodes and
/// edges so the LIVE side of the estate is indexed alongside IaC nodes.
///
/// Returns the number of resource nodes upserted.
pub fn ingest_tfstate(store: &mut dyn GraphStoreMutExt, tfstate_json: &str) -> Result<usize> {
    let collector = TfstateCollector::new();
    let extraction = collector.collect(tfstate_json)?;
    let node_count = extraction.nodes.len();
    store.begin_batch()?;
    store.upsert_nodes(&extraction.nodes)?;
    store.upsert_edges(&extraction.local_edges)?;
    store.upsert_unresolved_refs(&extraction.refs)?;
    store.commit_batch()?;
    Ok(node_count)
}

/// W10 drift report: classification of estate resources.
#[derive(Debug)]
pub struct DriftReport {
    /// Resources present in the live state but absent from IaC (unmanaged / shadow resources).
    pub unmanaged: Vec<Node>,
    /// Resources declared in IaC but absent from live state (not deployed / deleted).
    pub undeployed: Vec<Node>,
    /// Resources present in both IaC and live state (managed + deployed).
    pub managed: Vec<Node>,
}

/// Compute a drift report by partitioning all `Other("resource")` nodes by their `origin`
/// metadata field (`"iac"` vs `"live"`).
///
/// A resource is identified by the pair `(type, logical_name)` extracted from the node name
/// (e.g. `"aws_s3_bucket.app"` → type=`"aws_s3_bucket"`, name=`"app"`). Two nodes are
/// considered the same resource when their `(type, name)` identity matches regardless of
/// origin.
///
/// # Origin convention
///
/// - IaC extractors (CloudFormation, Kubernetes, HCL) set `metadata["origin"] = "iac"`.
/// - `TfstateCollector` sets `metadata["origin"] = "live"`.
/// - Nodes without an `origin` key are treated as `"iac"` (conservative assumption).
pub fn estate_drift(store: &dyn GraphRead) -> Result<DriftReport> {
    let all_nodes = store.all_nodes()?;

    // Collect only resource nodes.
    let resource_nodes: Vec<Node> = all_nodes
        .into_iter()
        .filter(|n| matches!(&n.kind, NodeKind::Other(k) if k == "resource"))
        .collect();

    // Partition by origin.
    let mut iac: Vec<Node> = Vec::new();
    let mut live: Vec<Node> = Vec::new();
    for n in &resource_nodes {
        let origin = n
            .metadata
            .get("origin")
            .and_then(|v| v.as_str())
            .unwrap_or("iac");
        if origin == "live" {
            live.push(n.clone());
        } else {
            iac.push(n.clone());
        }
    }

    // Build identity key: normalise the node name to (resource_type, logical_name).
    // For tfstate addresses like "aws_s3_bucket.app[0]" we strip the index suffix.
    fn identity(node: &Node) -> String {
        // Strip array index suffix e.g. "[0]", "[1]" for terraform count resources.
        let name = node.name.trim();
        let name = if let Some(pos) = name.rfind('[') {
            &name[..pos]
        } else {
            name
        };
        name.to_lowercase()
    }

    use std::collections::HashSet;
    let iac_ids: HashSet<String> = iac.iter().map(identity).collect();
    let live_ids: HashSet<String> = live.iter().map(identity).collect();

    let managed_ids: HashSet<String> = iac_ids.intersection(&live_ids).cloned().collect();

    let unmanaged: Vec<Node> = live
        .iter()
        .filter(|n| !managed_ids.contains(&identity(n)))
        .cloned()
        .collect();
    let undeployed: Vec<Node> = iac
        .iter()
        .filter(|n| !managed_ids.contains(&identity(n)))
        .cloned()
        .collect();
    let managed: Vec<Node> = resource_nodes
        .iter()
        .filter(|n| managed_ids.contains(&identity(n)))
        .cloned()
        .collect();

    Ok(DriftReport {
        unmanaged,
        undeployed,
        managed,
    })
}

/// W7.4: Compute how many commits have been made to the repo at `root` since the database
/// file at `db_path` was last written.
///
/// Returns `None` when:
/// - `root` is not a git repository.
/// - `git` is not on `$PATH`.
/// - The database file does not exist (no mtime to compare against).
///
/// Never panics or returns an `Err`; all failures produce `None` (degraded gracefully).
pub fn commits_behind(root: &Path, db_path: &str) -> Option<u64> {
    if db_path == ":memory:" {
        return None;
    }
    // Get the file modification time of the database.
    let db_file = std::path::Path::new(db_path);
    let mtime = db_file.metadata().ok()?.modified().ok()?;

    // Format the mtime as an ISO 8601 string for git --since.
    let since = {
        use std::time::UNIX_EPOCH;
        let secs = mtime.duration_since(UNIX_EPOCH).ok()?.as_secs();
        // git --since accepts "@<unix_timestamp>" format.
        format!("@{secs}")
    };

    // Run: git -C <root> rev-list --count --since=<mtime> HEAD
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-list",
            "--count",
            &format!("--since={since}"),
            "HEAD",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    s.trim().parse::<u64>().ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// W12 — Cross-graph / federated query API
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a federated query: `(tagged_nodes, per_db_errors)`.
///
/// Each entry in the first vec is `(repo_db_path, Node)`.  Per-db errors (unopenable files,
/// corrupt DBs) are collected into the second vec rather than propagating as `Err`.
type FedResult = wicked_estate_core::Result<(Vec<(String, wicked_estate_core::Node)>, Vec<String>)>;

/// Open `db_path` as a read-only [`GraphRead`] store via `wicked_estate_store::open_store`.
///
/// Returns an `Err` when the path cannot be opened (missing file, corrupt DB, etc.); the
/// caller is responsible for logging and skipping that DB in a federation fan-out.
fn open_read_store(db_path: &str) -> wicked_estate_core::Result<Box<dyn GraphRead>> {
    wicked_estate_store::open_store(db_path).map(|b| b as Box<dyn GraphRead>)
}

/// **Federated symbol search** across multiple per-repo databases.
///
/// Opens each database in `db_paths`, runs [`search`] on each, and tags every match with
/// the database path it came from (the "repo identifier").  The union is returned as an
/// ordered `Vec<(repo_db, Node)>` — all matches from the first DB first, then the second, etc.
///
/// # Cross-repo matching semantics
///
/// Matching is by **exact simple name** (`SymbolId`'s logical name component).  This means:
///
/// * A symbol named `process_payment` in `repo-a.db` AND in `repo-b.db` will both appear in
///   the result — the caller sees both occurrences tagged by repo.
/// * Where a [`Symbol::Global`] carries [`Package`] coordinates (`manager`, `name`, `version`),
///   callers can further filter to the same package identity.  This function returns the raw
///   nodes so the caller decides whether package-equality is required.
///
/// **Honest limitation**: cross-repo *edges* (calls from repo-A into a symbol in repo-B) are
/// NOT resolved here.  Each repo's graph only has edges within its own indexed codebase.
/// Precise cross-repo edge resolution requires a package-aware import resolver that can look up
/// symbols across repo boundaries — a future step (package-resolver tier).  The federation
/// results are therefore a best-effort **name-union**; treat them as candidate matches, not as
/// a connected cross-repo call graph.
///
/// # Errors
///
/// Per-database errors are collected but do NOT abort the whole fan-out.  A DB that fails to
/// open simply contributes no results; the caller gets a partial result.  The returned
/// `Vec<String>` contains error messages for DBs that could not be queried (empty when all
/// succeeded).
pub fn cross_graph_search(db_paths: &[String], name: &str) -> FedResult {
    let mut results: Vec<(String, wicked_estate_core::Node)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for db_path in db_paths {
        match open_read_store(db_path) {
            Err(e) => {
                errors.push(format!("{db_path}: failed to open — {e}"));
            }
            Ok(store) => match search(store.as_ref(), name) {
                Err(e) => {
                    errors.push(format!("{db_path}: search error — {e}"));
                }
                Ok(nodes) => {
                    for node in nodes {
                        results.push((db_path.clone(), node));
                    }
                }
            },
        }
    }

    Ok((results, errors))
}

/// **Federated blast-radius** across multiple per-repo databases.
///
/// Opens each database in `db_paths`, runs [`blast_radius_by_name`] on each, and tags every
/// dependent node with the database path it came from.  The union is the set of all known
/// dependents of `name` across the org's repos.
///
/// See [`cross_graph_search`] for the cross-repo matching semantics and the honest limitation
/// regarding cross-repo edges.
pub fn cross_graph_blast_radius(db_paths: &[String], name: &str, depth: u32) -> FedResult {
    let mut results: Vec<(String, wicked_estate_core::Node)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for db_path in db_paths {
        match open_read_store(db_path) {
            Err(e) => {
                errors.push(format!("{db_path}: failed to open — {e}"));
            }
            Ok(store) => match blast_radius_by_name(store.as_ref(), name, depth) {
                Err(e) => {
                    errors.push(format!("{db_path}: blast-radius error — {e}"));
                }
                Ok(nodes) => {
                    for node in nodes {
                        results.push((db_path.clone(), node));
                    }
                }
            },
        }
    }

    Ok((results, errors))
}

// ─────────────────────────────────────────────────────────────────────────────
// W5.2 — compute_embeddings (opt-in, separate from index_path)
// ─────────────────────────────────────────────────────────────────────────────

/// The embedder used by `index --embeddings` and `semantic`. With the `fastembed` feature it is
/// the ONNX/BGE [`FastEmbedder`](wicked_estate_retrieve::FastEmbedder) (real semantic vectors, 384-dim);
/// otherwise the lexical [`HashEmbedder`](wicked_estate_retrieve::HashEmbedder). If the feature is on but the
/// model can't load (no network on first run, etc.), it prints a LOUD marker and falls back to the
/// lexical embedder for the whole run — never silently presents lexical results as semantic.
///
/// The same factory is used at index-time and query-time, so the stored and query vectors always
/// share a dimension (384 vs 128 differ across embedders).
pub fn default_embedder() -> Box<dyn Embedder> {
    // Tier 1: FastEmbedder (contextual ONNX/BGE) — highest quality if its feature is on + loads.
    #[cfg(feature = "fastembed")]
    {
        match wicked_estate_retrieve::FastEmbedder::new() {
            Ok(e) => return Box::new(e),
            Err(err) => {
                eprintln!(
                    "EMBED-FALLBACK: fastembed model unavailable ({err}); trying lighter tiers"
                )
            }
        }
    }
    // Tier 2: Model2VecEmbedder (static distilled) — real semantic, light, no ONNX.
    #[cfg(feature = "model2vec")]
    {
        match wicked_estate_retrieve::Model2VecEmbedder::new() {
            Ok(e) => return Box::new(e),
            Err(err) => {
                eprintln!(
                    "EMBED-FALLBACK: model2vec model unavailable ({err}); using lexical HashEmbedder"
                )
            }
        }
    }
    // Tier 3: HashEmbedder — lexical, zero-dep. If a semantic feature was enabled but the model
    // couldn't load, the loud markers above already explained why this is lexical, not semantic.
    Box::new(wicked_estate_retrieve::HashEmbedder::default())
}

/// Compute and store embedding vectors for every node currently in `store`.
///
/// Iterates all nodes, builds a text representation (`name [kind] signature? doc-first-line?`),
/// embeds it with `embedder`, and persists the vector via `store.set_embedding`.  Nodes that
/// already have a stored embedding are silently overwritten (idempotent re-run).
///
/// Returns the count of nodes embedded.
///
/// # Design note
///
/// This is an **inherent-store** call (`SqliteStore`) rather than a trait call because
/// `set_embedding` lives on the concrete type (it is not part of `GraphStoreMutExt`).
/// The function is intentionally **separate** from `index_path` so that `index_path`'s
/// public signature remains unchanged (wicked-estate-bench calls it).  The CLI `index` command
/// invokes this as an optional second step when `--embeddings` is passed.
///
/// # Dim-guard meta (DoD-A6a / §3.1, §3.3)
///
/// After **all** vectors are persisted, this writes `meta["embedder_id"] = embedder.id()` and
/// `meta["embedder_dim"] = embedder.dim()` so the MCP server can refuse to advertise/dispatch
/// SemanticSearch when the store's embedder identity does not match the runtime's. The meta write
/// is performed **LAST, after the loop** for crash-safety: a crash mid-embed leaves a partially
/// re-embedded store whose `meta` still reflects the PRIOR complete state (or `None` on a
/// first-ever embed) — never a half-written identity that would let mixed-dim rows be served.
/// Re-running overwrites both the vectors and the meta atomically-enough for the guard's purpose.
pub fn compute_embeddings(store: &mut SqliteStore, embedder: &dyn Embedder) -> Result<usize> {
    let nodes = GraphRead::all_nodes(store)?;
    let mut count = 0usize;
    for node in &nodes {
        // Build a plain-text representation from the stable, always-present fields.
        // Signature and doc are appended when present so the hash vector captures them.
        let mut text = format!("{} {:?}", node.name, node.kind);
        if let Some(sig) = &node.signature {
            text.push(' ');
            text.push_str(sig);
        }
        if let Some(doc) = &node.doc {
            // First line only — mirrors render_stub in wicked-estate-retrieve.
            if let Some(first) = doc.lines().next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    text.push(' ');
                    text.push_str(trimmed);
                }
            }
        }
        let vec = embedder.embed(&text);
        store.set_embedding(&node.symbol, &vec)?;
        count += 1;
    }
    // Tag the store with the embedder identity + dim — LAST, after every vector is persisted, so a
    // crash before this point leaves meta at the prior complete state (or None) → fail-closed.
    store.meta_set_key("embedder_id", embedder.id());
    store.meta_set_key("embedder_dim", &embedder.dim().to_string());
    Ok(count)
}

/// Open an async-capable store from a spec string.
///
/// Supported specs:
/// - `sqlite:<path>` or bare `<path>` — `SqlitePool` via deadpool, 8 connections
///
/// Returns a `SqlitePool` which implements `AsyncGraphStore`. Deadpool manages
/// the internal `Arc`, so the pool is cheap to clone if multiple owners are needed.
#[cfg(feature = "serve")]
pub fn open_async_store(spec: &str) -> crate::Result<wicked_estate_store::SqlitePool> {
    let path = if let Some(p) = spec.strip_prefix("sqlite:") {
        p
    } else {
        spec
    };
    wicked_estate_store::open_sqlite_pool(path, 8)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{GraphWrite, Language, Location, Node, NodeKind, Span, SymbolId};
    use wicked_estate_store::{GraphStoreMutExt, MemStore};

    fn resource_node(sym: &str, name: &str, origin: &str) -> Node {
        let mut meta = serde_json::Map::new();
        meta.insert(
            "origin".to_string(),
            serde_json::Value::String(origin.to_string()),
        );
        let mut n = Node::new(
            SymbolId(sym.to_string()),
            NodeKind::Other("resource".to_string()),
            name,
            Language::new("tfstate"),
            Location::new(name, Span::ZERO),
        );
        n.metadata = meta;
        n
    }

    // ── Task C: estate_drift ─────────────────────────────────────────────────

    #[test]
    fn drift_empty_store_produces_empty_report() {
        let store = MemStore::new();
        let report = estate_drift(&store).unwrap();
        assert!(report.unmanaged.is_empty());
        assert!(report.undeployed.is_empty());
        assert!(report.managed.is_empty());
    }

    #[test]
    fn drift_live_only_is_unmanaged() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[resource_node("live1", "aws_s3_bucket.logs", "live")])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert_eq!(
            report.unmanaged.len(),
            1,
            "live-only node must be unmanaged"
        );
        assert!(report.undeployed.is_empty(), "no iac-only nodes");
        assert!(report.managed.is_empty(), "no managed nodes");
    }

    #[test]
    fn drift_iac_only_is_undeployed() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[resource_node("iac1", "aws_s3_bucket.logs", "iac")])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert_eq!(
            report.undeployed.len(),
            1,
            "iac-only node must be undeployed"
        );
        assert!(report.unmanaged.is_empty(), "no live-only nodes");
        assert!(report.managed.is_empty(), "no managed nodes");
    }

    #[test]
    fn drift_matching_iac_and_live_is_managed() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                resource_node("iac1", "aws_s3_bucket.app", "iac"),
                resource_node("live1", "aws_s3_bucket.app", "live"),
            ])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert!(report.unmanaged.is_empty(), "no unmanaged");
        assert!(report.undeployed.is_empty(), "no undeployed");
        assert!(!report.managed.is_empty(), "both iac and live → managed");
    }

    #[test]
    fn drift_mixed_report() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                // managed pair
                resource_node("iac_app", "aws_s3_bucket.app", "iac"),
                resource_node("live_app", "aws_s3_bucket.app", "live"),
                // iac-only (undeployed)
                resource_node("iac_new", "aws_sqs_queue.jobs", "iac"),
                // live-only (unmanaged)
                resource_node("live_shadow", "aws_instance.old", "live"),
            ])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert_eq!(report.undeployed.len(), 1, "one undeployed");
        assert_eq!(report.unmanaged.len(), 1, "one unmanaged");
        assert_eq!(
            report.managed.len(),
            2,
            "two managed (both iac+live for app)"
        );
    }

    // ── Task B: ingest_tfstate ───────────────────────────────────────────────

    #[test]
    fn ingest_tfstate_round_trips_resource_nodes() {
        // Minimal valid tfstate JSON with one resource.
        let tfstate_json = r#"{
            "version": 4,
            "terraform_version": "1.5.0",
            "resources": [
                {
                    "mode": "managed",
                    "type": "aws_s3_bucket",
                    "name": "test_bucket",
                    "provider": "provider[\"registry.terraform.io/hashicorp/aws\"]",
                    "instances": [
                        {
                            "attributes": {
                                "id": "my-bucket-id",
                                "bucket": "my-bucket-id"
                            }
                        }
                    ]
                }
            ]
        }"#;

        let mut store = MemStore::new();
        let n = ingest_tfstate(&mut store, tfstate_json).unwrap();
        assert!(
            n > 0,
            "ingest_tfstate must upsert at least one resource node"
        );

        // At least one resource node should exist.
        let nodes = wicked_estate_core::GraphRead::all_nodes(&store).unwrap();
        let resources: Vec<_> = nodes
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Other(k) if k == "resource"))
            .collect();
        assert!(
            !resources.is_empty(),
            "at least one resource node must be in the store"
        );
    }

    // ── Task E: analytics cache ──────────────────────────────────────────────

    #[test]
    fn index_path_populates_pagerank_cache() {
        // Use a temp dir with a tiny Rust file so index_path has something to extract.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn hello() {}\npub fn world() { hello(); }\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // After indexing, the pagerank.top cache entry should be populated.
        let cached = store.cache_get_key("pagerank.top");
        assert!(
            cached.is_some(),
            "pagerank.top cache must be set after index_path"
        );
        let json_str = cached.unwrap();
        // Should be a valid JSON array.
        let parsed: Vec<(String, f32)> = serde_json::from_str(&json_str)
            .expect("pagerank.top must be a JSON array of (symbol, score) pairs");
        // With two functions, we should have at least one ranked result.
        assert!(
            !parsed.is_empty(),
            "ranked list must not be empty for a non-empty codebase"
        );
    }

    #[test]
    fn important_symbols_reads_from_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(
            &src,
            "pub fn a() {}\npub fn b() { a(); }\npub fn c() { b(); }\n",
        )
        .unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // important_symbols should return results (from cache or live compute).
        let top = important_symbols(&store, 10).unwrap();
        assert!(
            !top.is_empty(),
            "important_symbols must return at least one result"
        );
    }

    /// Lane relative-imports S5 (Decision H): after a real index, important_symbols never
    /// returns a File or Import node — the ranked seam filters live results and the cache
    /// write is already filtered.
    #[test]
    fn important_symbols_has_no_file_or_import_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("a.ts"),
            "import { f } from './b';\nexport function g() { return f(); }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("b.ts"),
            "export function f() { return 1; }\n",
        )
        .unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let top = important_symbols(&store, 50).unwrap();
        assert!(!top.is_empty(), "ranked results expected");
        for (node, _) in &top {
            assert!(
                !matches!(node.kind, NodeKind::File | NodeKind::Import),
                "File/Import must never rank as a hotspot: {:?} {}",
                node.kind,
                node.symbol.as_str()
            );
        }
    }

    /// Lane relative-imports S5 (BR-1): a STALE pre-upgrade `pagerank.top` cache containing
    /// File/Import ids is cleaned at READ time — the ids are never returned and `top_n` is
    /// still filled from the remaining rows. This is the precondition for graph-view dropping
    /// its post-hoc exclusion.
    #[test]
    fn important_symbols_drops_file_import_from_stale_cache() {
        use wicked_estate_core::GraphWrite;
        let mut store = MemStore::new();
        let file_node = Node::new(
            wicked_estate_core::Symbol::file("a.ts").id(),
            NodeKind::File,
            "a.ts",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        let import_node = Node::new(
            SymbolId("import:./hub".into()),
            NodeKind::Import,
            "./hub",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        let fn_x = Node::new(
            SymbolId("fn_x".into()),
            NodeKind::Function,
            "fn_x",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        let fn_y = Node::new(
            SymbolId("fn_y".into()),
            NodeKind::Function,
            "fn_y",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[file_node.clone(), import_node.clone(), fn_x, fn_y])
            .unwrap();
        store.commit_batch().unwrap();

        // Hand-seed the cache the way a PRE-upgrade binary wrote it: File/Import rows on top.
        let stale = serde_json::to_string(&vec![
            (file_node.symbol.0.clone(), 0.9_f32),
            (import_node.symbol.0.clone(), 0.8_f32),
            ("fn_x".to_string(), 0.5_f32),
            ("fn_y".to_string(), 0.4_f32),
        ])
        .unwrap();
        store.cache_put_key("pagerank.top", &stale);

        let top = important_symbols(&store, 2).unwrap();
        let ids: Vec<&str> = top.iter().map(|(n, _)| n.symbol.as_str()).collect();
        assert_eq!(
            ids,
            vec!["fn_x", "fn_y"],
            "stale File/Import cache rows skipped, top_n filled from the remaining rows"
        );
    }

    // ── W7.4: commits_behind returns None gracefully for non-git dir ─────────

    #[test]
    fn commits_behind_non_git_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        // Not a git repo + db doesn't exist = None.
        let result = commits_behind(tmp.path(), "/nonexistent/graph.db");
        assert!(
            result.is_none(),
            "commits_behind must return None for non-git dir"
        );
    }

    // ── Task A: InfraResolver in resolver slice ──────────────────────────────

    #[test]
    fn index_path_uses_infra_resolver_for_resource_refs() {
        use wicked_estate_core::{EdgeKind, GraphWrite, NodeKind, UnresolvedRef};
        // Build a store with a resource node and an unresolved ref to it.
        // index_path is not called here (that needs a real dir); we verify resolve_all
        // with InfraResolver by calling it directly.
        let resource_name = "aws_instance.web";
        let resource_sym = wicked_estate_core::Symbol::synthetic("tfstate", resource_name).id();
        let resource_node = Node::new(
            resource_sym.clone(),
            NodeKind::Other("resource".to_string()),
            resource_name,
            Language::new("tfstate"),
            Location::new(resource_name, Span::ZERO),
        );

        let eip_sym = wicked_estate_core::Symbol::synthetic("tfstate", "aws_eip.ip").id();
        let eip_node = Node::new(
            eip_sym.clone(),
            NodeKind::Other("resource".to_string()),
            "aws_eip.ip",
            Language::new("tfstate"),
            Location::new("aws_eip.ip", Span::ZERO),
        );

        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store.upsert_nodes(&[resource_node, eip_node]).unwrap();
        store.commit_batch().unwrap();

        let index = InMemoryIndex::build(&store, None).unwrap();
        let refs = vec![UnresolvedRef::new(
            eip_sym,
            resource_name,
            EdgeKind::Other("depends_on".to_string()),
            Location::new("aws_eip.ip", Span::ZERO),
        )];
        let resolvers: &[&dyn Resolver] = &[
            &NameResolver,
            &ScopedNameResolver,
            &ImportMapResolver,
            &InfraResolver,
        ];
        let edges = resolve_all(resolvers, &refs, &index).unwrap();
        assert_eq!(
            edges.len(),
            1,
            "InfraResolver must resolve the depends_on ref"
        );
        assert_eq!(edges[0].target, resource_sym);
    }

    // ── Task A: SKIPPED_MINIFIED notice ──────────────────────────────────────

    #[test]
    fn is_minified_or_huge_detects_long_lines() {
        // Verify the public API matches expectations used in index_path.
        let normal = "fn foo() {}\nfn bar() {}\n";
        assert!(
            !is_minified_or_huge(normal),
            "normal code must not be flagged"
        );

        // A >50,000-char single line is considered minified/huge.
        let long_line: String = "a".repeat(50_001);
        assert!(
            is_minified_or_huge(&long_line),
            ">50000-char line must be flagged"
        );
    }

    // ── W12 federation helpers ────────────────────────────────────────────────

    /// Build a SqliteStore at `path` with one symbol named `sym_name` + an optional caller.
    fn build_repo_db(
        path: &str,
        sym_id: &str,
        sym_name: &str,
        caller_id: Option<&str>,
        caller_name: Option<&str>,
    ) {
        use wicked_estate_core::{
            Edge, EdgeKind, GraphWrite, Language, Location, ResolutionTier, Span,
        };
        use wicked_estate_store::SqliteStore;

        let mut store = SqliteStore::open(path).expect("open temp store");
        store.begin_batch().unwrap();

        let sym = Node::new(
            SymbolId(sym_id.to_string()),
            NodeKind::Function,
            sym_name,
            Language::new("rust"),
            Location::new("src/lib.rs", Span::ZERO),
        );
        store.upsert_nodes(&[sym]).unwrap();

        if let (Some(cid), Some(cname)) = (caller_id, caller_name) {
            let caller = Node::new(
                SymbolId(cid.to_string()),
                NodeKind::Function,
                cname,
                Language::new("rust"),
                Location::new("src/lib.rs", Span::ZERO),
            );
            let edge = Edge::new(
                SymbolId(cid.to_string()),
                SymbolId(sym_id.to_string()),
                EdgeKind::Calls,
                ResolutionTier::Parsed,
                "test",
            );
            store.upsert_nodes(&[caller]).unwrap();
            store.upsert_edges(&[edge]).unwrap();
        }

        store.commit_batch().unwrap();
    }

    #[test]
    fn cross_graph_search_finds_symbol_in_both_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let db_a = tmp.path().join("repo_a.db").to_string_lossy().to_string();
        let db_b = tmp.path().join("repo_b.db").to_string_lossy().to_string();

        // Both repos have a symbol named "shared_fn".
        build_repo_db(&db_a, "sym_a", "shared_fn", None, None);
        build_repo_db(&db_b, "sym_b", "shared_fn", None, None);

        let (results, errors) = cross_graph_search(&[db_a.clone(), db_b.clone()], "shared_fn")
            .expect("federation search must succeed");

        assert!(errors.is_empty(), "no DB errors expected: {errors:?}");
        assert_eq!(results.len(), 2, "one match per repo = 2 total");

        // Both repo paths are represented.
        let repos: Vec<&str> = results.iter().map(|(r, _)| r.as_str()).collect();
        assert!(repos.contains(&db_a.as_str()), "repo_a in results");
        assert!(repos.contains(&db_b.as_str()), "repo_b in results");

        // Both matches have the expected name.
        assert!(
            results.iter().all(|(_, n)| n.name == "shared_fn"),
            "all matches must be named 'shared_fn'"
        );
    }

    #[test]
    fn cross_graph_blast_radius_unions_dependents_across_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let db_a = tmp.path().join("br_a.db").to_string_lossy().to_string();
        let db_b = tmp.path().join("br_b.db").to_string_lossy().to_string();

        // repo_a: "target_fn" called by "caller_a_fn"
        build_repo_db(
            &db_a,
            "target_a",
            "target_fn",
            Some("caller_a"),
            Some("caller_a_fn"),
        );
        // repo_b: "target_fn" also exists, called by "caller_b_fn"
        build_repo_db(
            &db_b,
            "target_b",
            "target_fn",
            Some("caller_b"),
            Some("caller_b_fn"),
        );

        let (results, errors) =
            cross_graph_blast_radius(&[db_a.clone(), db_b.clone()], "target_fn", 8)
                .expect("federation blast-radius must succeed");

        assert!(errors.is_empty(), "no DB errors expected: {errors:?}");
        // Each repo contributes its own caller to the union.
        assert_eq!(results.len(), 2, "one dependent per repo = 2 total");

        let caller_names: Vec<&str> = results.iter().map(|(_, n)| n.name.as_str()).collect();
        assert!(
            caller_names.contains(&"caller_a_fn"),
            "caller from repo_a must appear"
        );
        assert!(
            caller_names.contains(&"caller_b_fn"),
            "caller from repo_b must appear"
        );
    }

    #[test]
    fn cross_graph_search_nonexistent_symbol_returns_empty_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        let db_a = tmp.path().join("empty_a.db").to_string_lossy().to_string();
        build_repo_db(&db_a, "sym_a", "known_fn", None, None);

        let (results, errors) =
            cross_graph_search(&[db_a], "does_not_exist_xyz").expect("search must not error");

        assert!(errors.is_empty(), "no DB errors");
        assert!(results.is_empty(), "no results for unknown symbol");
    }

    #[test]
    fn cross_graph_search_bad_db_path_collects_error_not_panic() {
        let (results, errors) =
            cross_graph_search(&["/nonexistent/path/ghost.db".to_string()], "foo")
                .expect("federation must not propagate per-db errors");

        // Results may be empty; error list must name the bad path.
        assert!(results.is_empty(), "no results from an unopenable db");
        assert!(
            !errors.is_empty(),
            "error list must be non-empty for bad path"
        );
        assert!(
            errors[0].contains("ghost.db"),
            "error must mention the problematic db path"
        );
    }

    // ── W7: collect_repo_info graceful fallback ───────────────────────────────

    /// A temp dir that is NOT a git repo must produce an all-None / dirty=false RepoInfo
    /// without panicking.  This validates the git-absent / non-repo path of the function.
    #[test]
    fn collect_repo_info_non_git_dir_returns_all_none() {
        let tmp = tempfile::tempdir().unwrap();
        let info = collect_repo_info(tmp.path());
        assert!(info.commit.is_none(), "commit must be None for non-git dir");
        assert!(info.branch.is_none(), "branch must be None for non-git dir");
        assert!(info.remote.is_none(), "remote must be None for non-git dir");
        assert!(!info.dirty, "dirty must be false for non-git dir");
    }

    // ── W7.1: subscribe-after-index emits ≥1 change ──────────────────────────

    /// After indexing a directory with at least one source file, `changes_since(0)` must
    /// return at least one Upsert change entry.
    #[test]
    fn index_path_emits_change_log_entries() {
        use wicked_estate_core::ChangeOp;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn alpha() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let changes = store.changes_since(0).unwrap();
        assert!(
            !changes.is_empty(),
            "at least one change must be emitted after indexing"
        );
        assert!(
            changes.iter().any(|c| c.op == ChangeOp::Upsert),
            "at least one Upsert change must be present"
        );
    }

    /// A second index run where nothing changed must produce no NEW change-log entries
    /// beyond those from the first run.
    #[test]
    fn index_path_no_extra_changes_when_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn beta() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let after_first = store.changes_since(0).unwrap();
        let last_seq = after_first.iter().map(|c| c.seq).max().unwrap_or(0);

        // Second run — nothing changed.
        index_path(&mut store, tmp.path()).unwrap();

        let after_second = store.changes_since(last_seq).unwrap();
        assert!(
            after_second.is_empty(),
            "no new change-log entries when nothing has changed"
        );
    }

    /// Deleting a file then re-indexing must emit a Remove change.
    #[test]
    fn index_path_emits_remove_change_for_deleted_file() {
        use wicked_estate_core::ChangeOp;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("to_delete.rs");
        std::fs::write(&src, "pub fn gamma() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let after_first = store.changes_since(0).unwrap();
        let last_seq = after_first.iter().map(|c| c.seq).max().unwrap_or(0);

        // Delete the file, re-index.
        std::fs::remove_file(&src).unwrap();
        index_path(&mut store, tmp.path()).unwrap();

        let deltas = store.changes_since(last_seq).unwrap();
        assert!(
            deltas
                .iter()
                .any(|c| c.op == ChangeOp::Remove && c.target.contains("to_delete.rs")),
            "a Remove change for to_delete.rs must be emitted"
        );
    }

    /// P8 self-pollution. wicked's own coverage/requirements artifacts, if they land at an indexed
    /// root (a coverage run whose CWD is the repo, not a worktree), must never be collected as
    /// source — indexing `coverage-report.json` back in turns its JSON keys into behavior-bearing
    /// nodes that pin the NEXT coverage run below 1.0 forever. Tests the walk seam directly, so it
    /// is independent of any extractor: drop the artifact names from the `filter_entry` skip list
    /// and `coverage-report.json` reappears in the collected set, failing the second assert.
    #[test]
    fn collect_source_files_skips_wicked_measurement_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "pub fn foo() {}\n").unwrap();
        std::fs::write(
            tmp.path().join("coverage-report.json"),
            "{\"total\":1,\"coverage\":1.0}\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("requirements_graph.json"),
            "{\"reqs\":[]}\n",
        )
        .unwrap();

        let names: Vec<String> = collect_source_files(tmp.path())
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();

        assert!(
            names.iter().any(|n| n == "main.rs"),
            "real source must still be collected; got {names:?}"
        );
        assert!(
            !names
                .iter()
                .any(|n| n == "coverage-report.json" || n == "requirements_graph.json"),
            "wicked measurement artifacts must be skipped (self-pollution); got {names:?}"
        );
    }

    /// FINDING-067. The sibling of the test above, and the one that was missing: the sweep must
    /// remove deleted SOURCE files and nothing else. A store is not always exclusively the
    /// indexer's — an orchestrator sharing one keeps its domain objects as nodes whose
    /// `location.file` is a synthetic key (`agent_session/<id>`, `validator_vault/<pin>`), never a
    /// path on disk. Deriving "previously indexed" from `all_nodes()` made every one of those look
    /// like a file that had been deleted since the last run.
    ///
    /// Measured, not hypothesised: indexing one repo subdirectory into a live orchestrator's store
    /// removed 833 operational nodes in a single transaction — 27 sessions, 77 work units, 57
    /// phases, 53 work outputs, 23 workflows, 4 validator-vault entries, 3 policies and the repo
    /// registration — while upserting the 41 source files it had been asked to index. The session
    /// that issued the index was among the 27, and the run died with `run not found`.
    ///
    /// Revert `previously_indexed` to `all_nodes()` and this fails on the first assert.
    #[test]
    fn the_delete_sweep_leaves_foreign_nodes_alone() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("real.rs"), "pub fn kept() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // A node no indexer wrote: synthetic `location.file`, shaped exactly like the operational
        // rows that were destroyed. It is in the store when the index runs, and it is not on disk.
        let foreign_id = Symbol::synthetic("wicked-core", "agent_session/run-1").id();
        let foreign = Node::new(
            foreign_id.clone(),
            NodeKind::Other("agent_session".to_string()),
            "run-1".to_string(),
            Language::new("none"),
            Location::new("agent_session/run-1".to_string(), Span::ZERO),
        );
        store.upsert_nodes(&[foreign]).unwrap();

        // Re-index with a genuinely deleted source file in the same pass, so this pins the fix
        // rather than a sweep that simply stopped running.
        std::fs::write(tmp.path().join("doomed.rs"), "pub fn doomed() {}\n").unwrap();
        index_path(&mut store, tmp.path()).unwrap();
        std::fs::remove_file(tmp.path().join("doomed.rs")).unwrap();
        index_path(&mut store, tmp.path()).unwrap();

        assert!(
            store.get_node(&foreign_id).unwrap().is_some(),
            "indexing a directory must not delete a node it never indexed"
        );
        let files: HashSet<String> = GraphRead::all_nodes(&store)
            .unwrap()
            .into_iter()
            .map(|n| n.location.file)
            .collect();
        assert!(files.contains("real.rs"), "the live source file survives");
        assert!(
            !files.contains("doomed.rs"),
            "a source file that really was deleted is still swept — the sweep still works"
        );
    }

    // ── Task G: incremental scenario — removed symbol leaves no dangling edge ──────────────

    /// After a file is modified such that a symbol disappears, the next `index_path` must leave
    /// no dangling edges pointing at the removed symbol.
    ///
    /// Scenario:
    ///   - Round 1: file_a.rs defines `foo`; file_b.rs calls `foo` (resolved edge b→a).
    ///   - Round 2: file_a.rs is re-written and `foo` is gone. `index_path` is run again.
    ///   - Assert: no edge with target pointing to the (now absent) `foo` symbol.
    #[test]
    fn incremental_index_no_dangling_edge_after_symbol_removed() {
        let tmp = tempfile::tempdir().unwrap();

        // Round 1: both files present; file_a defines `foo`, file_b calls it.
        let file_a = tmp.path().join("file_a.rs");
        let file_b = tmp.path().join("file_b.rs");
        std::fs::write(&file_a, "pub fn foo() {}\n").unwrap();
        std::fs::write(&file_b, "use file_a::foo;\npub fn bar() { foo(); }\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // Sanity: after round 1 we should have nodes and edges.
        let stats_r1 = store.stats().unwrap();
        assert!(stats_r1.node_count > 0, "round-1 must produce nodes");

        // Round 2: overwrite file_a to remove `foo`. The symbol is gone.
        std::fs::write(&file_a, "pub fn other() {}\n").unwrap();

        index_path(&mut store, tmp.path()).unwrap();

        // After round 2, no edge must have a target that is not in the node set.
        let all_nodes: std::collections::HashSet<SymbolId> =
            wicked_estate_core::GraphRead::all_nodes(&store)
                .unwrap()
                .into_iter()
                .map(|n| n.symbol)
                .collect();
        let dangling: Vec<_> = store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(|e| !all_nodes.contains(&e.source) || !all_nodes.contains(&e.target))
            .collect();
        assert!(
            dangling.is_empty(),
            "after incremental re-index, no dangling edges must remain; found: {dangling:?}"
        );
    }

    // ── W5.2: compute_embeddings ──────────────────────────────────────────────

    /// With `--embeddings` ON: after indexing a tiny fixture and running
    /// `compute_embeddings`, `nearest` must return at least one result.
    #[test]
    fn compute_embeddings_on_nearest_returns_results() {
        use wicked_estate_retrieve::HashEmbedder;
        use wicked_estate_store::SqliteStore;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn hello() {}\npub fn world() { hello(); }\n").unwrap();
        let db_path = tmp.path().join("graph.db").to_string_lossy().to_string();

        // Index into a concrete SqliteStore.
        let mut store = SqliteStore::open(&db_path).expect("open temp store");
        index_path(&mut store, tmp.path()).unwrap();

        // Embeddings table must be empty before compute_embeddings.
        let embedder = HashEmbedder::default();
        let count = compute_embeddings(&mut store, &embedder).unwrap();
        assert!(count > 0, "compute_embeddings must embed at least one node");

        // nearest must return ≥1 result for a query that matches any token.
        let qvec = embedder.embed("hello");
        let hits = store.nearest(&qvec, 10).unwrap();
        assert!(
            !hits.is_empty(),
            "nearest must return results after embeddings are populated"
        );
    }

    /// With `--embeddings` OFF: after plain `index_path`, the embeddings table
    /// must remain empty (no embeddings written as a side effect of indexing).
    #[test]
    fn index_path_without_compute_embeddings_leaves_table_empty() {
        use wicked_estate_retrieve::HashEmbedder;
        use wicked_estate_store::SqliteStore;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn alpha() {}\n").unwrap();
        let db_path = tmp.path().join("graph.db").to_string_lossy().to_string();

        let mut store = SqliteStore::open(&db_path).expect("open temp store");
        index_path(&mut store, tmp.path()).unwrap();

        // No compute_embeddings call — nearest must return nothing.
        let embedder = HashEmbedder::default();
        let qvec = embedder.embed("alpha");
        let hits = store.nearest(&qvec, 10).unwrap();
        assert!(
            hits.is_empty(),
            "embeddings table must be empty when --embeddings was not used"
        );
    }

    // ── clusters --summary JSON shape ────────────────────────────────────────

    /// Helper: build a MemStore with two Function nodes connected by a Calls edge.
    fn build_two_node_store() -> (MemStore, SymbolId, SymbolId) {
        use wicked_estate_core::{
            Edge, EdgeKind, GraphWrite, Language, Location, ResolutionTier, Span,
        };

        let id_a = SymbolId("fn::alpha".to_string());
        let id_b = SymbolId("fn::beta".to_string());

        let node_a = Node::new(
            id_a.clone(),
            NodeKind::Function,
            "alpha",
            Language::new("rust"),
            Location::new("src/alpha.rs", Span::ZERO),
        );
        let node_b = Node::new(
            id_b.clone(),
            NodeKind::Function,
            "beta",
            Language::new("rust"),
            Location::new("src/beta.rs", Span::ZERO),
        );
        let edge = Edge::new(
            id_b.clone(),
            id_a.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );

        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store.upsert_nodes(&[node_a, node_b]).unwrap();
        store.upsert_edges(&[edge]).unwrap();
        store.commit_batch().unwrap();

        (store, id_a, id_b)
    }

    /// `summarize_communities` returns one entry per non-empty community with all
    /// required fields non-empty (size ≥ 1, top_symbols non-empty, dominant_files non-empty).
    ///
    /// This covers the JSON-shaping logic used by `clusters --summary --json`.
    #[test]
    fn clusters_summary_json_shape_has_required_fields() {
        let (store, id_a, id_b) = build_two_node_store();

        // A hand-crafted community partition: one community with both symbols.
        let communities: Vec<Vec<SymbolId>> = vec![vec![id_a.clone(), id_b.clone()]];

        let summaries =
            wicked_estate_rank::summarize_communities(&store, &communities, 1.0).unwrap();

        assert_eq!(summaries.len(), 1, "one summary per community");
        let s = &summaries[0];
        assert_eq!(s.size, 2, "size must match member count");
        assert!(!s.top_symbols.is_empty(), "top_symbols must not be empty");
        assert!(
            !s.dominant_files.is_empty(),
            "dominant_files must not be empty"
        );

        // Verify the JSON value we'd emit in the clusters arm has all expected keys.
        let members_json: Vec<String> = communities[0].iter().map(|id| id.to_string()).collect();
        let json_obj = serde_json::json!({
            "id": 0usize,
            "size": s.size,
            "members": members_json,
            "label_candidates": s.top_symbols,
            "dominant_files": s.dominant_files,
            "modularity_contribution": s.modularity_contribution,
        });
        for key in &[
            "id",
            "size",
            "members",
            "label_candidates",
            "dominant_files",
            "modularity_contribution",
        ] {
            assert!(
                json_obj.get(key).is_some(),
                "clusters --summary JSON must include key '{key}'"
            );
        }
    }

    /// Without `--summary`, `clusters --json` emits bare arrays (back-compat).
    ///
    /// The bare-array path produces `Vec<Vec<String>>` — a JSON array of string arrays.
    /// Verify it serialises correctly and does NOT include summary keys.
    #[test]
    fn clusters_bare_json_is_array_of_arrays() {
        let (_, id_a, id_b) = build_two_node_store();

        let communities: Vec<Vec<SymbolId>> = vec![vec![id_a.clone(), id_b.clone()]];

        // This is exactly the bare-array path in the clusters arm.
        let j: Vec<Vec<String>> = communities
            .iter()
            .map(|c| c.iter().map(|s| s.to_string()).collect())
            .collect();

        let json_val = serde_json::to_value(&j).unwrap();
        assert!(json_val.is_array(), "bare output must be a JSON array");
        let outer = json_val.as_array().unwrap();
        assert_eq!(outer.len(), 1, "one inner array per community");
        let inner = outer[0].as_array().unwrap();
        assert_eq!(inner.len(), 2, "two members in the community");
        // Must NOT have a 'size' key — that would indicate the summary path.
        assert!(
            outer[0].get("size").is_none(),
            "bare array must not have 'size' key"
        );
    }

    // ── nodes --json symbol_id field ─────────────────────────────────────────

    /// `nodes --json` objects must each carry a non-empty `symbol_id`.
    ///
    /// Exercise the JSON-shaping used by the nodes arm by calling the same
    /// `serde_json::json!` construction directly against real nodes from the store.
    #[test]
    fn nodes_json_includes_symbol_id() {
        let (store, id_a, _id_b) = build_two_node_store();

        // Retrieve the nodes as the nodes arm does (all_nodes is on GraphRead / GraphStoreMutExt).
        let nodes = wicked_estate_core::GraphRead::all_nodes(&store).unwrap();
        assert!(!nodes.is_empty(), "store must have nodes");

        // Reproduce the JSON shaping from the nodes arm for both code paths.
        let j: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "symbol_id": n.symbol.to_string(),
                    "name": n.name,
                    "kind": format!("{:?}", n.kind),
                    "file": n.location.file,
                    "line": n.location.span.start_line + 1,
                    "signature": n.signature,
                })
            })
            .collect();

        for obj in &j {
            let sym_id = obj.get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
            assert!(
                !sym_id.is_empty(),
                "every node JSON object must have a non-empty symbol_id; got: {obj}"
            );
        }

        // The node we inserted with id "fn::alpha" must be findable by symbol_id.
        let has_alpha = j
            .iter()
            .any(|obj| obj.get("symbol_id").and_then(|v| v.as_str()) == Some(id_a.0.as_str()));
        assert!(
            has_alpha,
            "fn::alpha must appear in nodes JSON by symbol_id"
        );

        // There must be no object missing the symbol_id key.
        let missing: Vec<_> = j
            .iter()
            .filter(|obj| obj.get("symbol_id").is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "{} node(s) are missing symbol_id",
            missing.len()
        );
    }

    // ── DoD-A6a: compute_embeddings tags the store with embedder id + dim (§3.1, §3.3) ──

    #[test]
    fn compute_embeddings_writes_embedder_meta_last() {
        use wicked_estate_retrieve::HashEmbedder;
        use wicked_estate_store::SqliteStore;

        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("g.db");
        let db = db.to_str().unwrap();

        let mut store = SqliteStore::open(db).unwrap();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[Node::new(
                SymbolId("fn::alpha".to_string()),
                NodeKind::Function,
                "alpha",
                Language::new("rust"),
                Location::new("src/lib.rs", Span::ZERO),
            )])
            .unwrap();
        store.commit_batch().unwrap();

        // Before embedding: no embedder meta — a store that predates tagging reads None → the MCP
        // guard fails closed (EMBED-META-MISSING). This is the §3.3 crash-safety prior state.
        assert_eq!(store.meta_get_key("embedder_id"), None);
        assert_eq!(store.meta_get_key("embedder_dim"), None);

        let embedder = HashEmbedder::default(); // dim 128, id "hash:v1"
        let n = compute_embeddings(&mut store, &embedder).unwrap();
        assert_eq!(n, 1, "one node embedded");

        // After embedding: meta carries the exact embedder identity + dim used at index time.
        assert_eq!(
            store.meta_get_key("embedder_id").as_deref(),
            Some("hash:v1")
        );
        assert_eq!(store.meta_get_key("embedder_dim").as_deref(), Some("128"));
    }
}
