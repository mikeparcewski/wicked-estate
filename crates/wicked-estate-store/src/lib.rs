//! `wicked-estate-store` — [`GraphStore`] implementations.
//!
//! [`MemStore`] is the in-memory reference impl that proves the [`GraphStore`] contract (it
//! passes `wicked_estate_core::conformance::graph_store_suite`). The SQLite+FTS5+sqlite-vec default store
//! and the SurrealDB challenger land at Wave 1.5 behind the same trait, chosen by bake-off
//!.

pub mod sqlite;
pub use sqlite::{Annotation, CompactStats, SqliteStore};

// W1.5 bake-off challenger — compiled ONLY with --features surrealdb.
#[cfg(feature = "surrealdb")]
pub mod surreal;
#[cfg(feature = "surrealdb")]
pub use surreal::SurrealStore;

#[cfg(feature = "pool")]
pub mod pool;
#[cfg(feature = "pool")]
pub use pool::{SqlitePool, open_sqlite_pool};

// Postgres backend — compiled ONLY with --features postgres.
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "postgres")]
pub use postgres::PostgresStore;

// ── Vector math helpers for MemStore (no external deps) ────────────────────

#[inline]
fn mem_l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

#[inline]
fn mem_cosine_similarity(a: &[f32], b: &[f32], a_norm: f32) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let b_norm = mem_l2_norm(b);
    if a_norm == 0.0 || b_norm == 0.0 {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    (dot / (a_norm * b_norm)).clamp(-1.0, 1.0)
}

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use wicked_estate_core::{
    Change, ChangeOp, Direction, Edge, EdgeKind, Error, GraphRead, GraphStats, GraphStore,
    GraphWrite, HistoricalEdge, Node, NodeKind, NodeSemantics, RepoInfo, Result, StoreCapabilities,
    Subgraph, SymbolId, SymbolIndex, SymbolQuery, TraversalSpec, UnresolvedRef,
};

/// In-memory graph store: reference implementation + test double for the trait contract.
#[derive(Debug, Default)]
pub struct MemStore {
    nodes: HashMap<SymbolId, Node>,
    edges: Vec<Edge>,
    unresolved: Vec<UnresolvedRef>,
    /// Wave 2.6: file → content digest map for incremental re-indexing.
    file_digests: HashMap<String, String>,
    in_batch: bool,
    // W11.1: content-addressed source-text store (git_sha → text).
    content: HashMap<String, String>,
    // W11.1: file → git_sha pointer into content.
    file_git_shas: HashMap<String, String>,
    // W11.2: versioned query cache.
    cache: HashMap<String, (i64, String)>, // key → (version, value)
    graph_version: i64,
    // W5.2: per-symbol embedding vectors.
    embeddings: HashMap<SymbolId, Vec<f32>>,
    // W7.4 / W11.3: arbitrary key-value meta store (mirrors SqliteStore meta table).
    pub meta: HashMap<String, String>,
    // W7: repo provenance.
    repo_info: Option<RepoInfo>,
    // Semantic linking: symbol → NodeSemantics (description / requirement / validated).
    semantics: HashMap<SymbolId, NodeSemantics>,
    // W7: change log.
    changes: Vec<Change>,
    change_seq: u64,
    // W7: read-only edge history.
    // edge_history_files[i] is the file that was removed when edge_history[i] was archived.
    // Stored separately because HistoricalEdge (wicked-estate-core) carries no file field — the archived
    // edge may have been created without a location (e.g. synthetic edges).
    edge_history: Vec<HistoricalEdge>,
    edge_history_files: Vec<String>,
    history_archive_seq: u64,
    history_enabled: bool,
}

impl MemStore {
    /// Create a new in-memory store. `history_enabled` defaults to `false` (opt-in).
    pub fn new() -> Self {
        Self {
            history_enabled: false,
            ..Default::default()
        }
    }

    /// Create a new in-memory store with edge-history archival enabled.
    /// Used by conformance tests that assert history behaviour.
    pub fn new_with_history() -> Self {
        Self {
            history_enabled: true,
            ..Default::default()
        }
    }

    /// Enable or disable edge-history archival (default: `true`).
    pub fn set_history_enabled(&mut self, on: bool) {
        self.history_enabled = on;
    }

    fn kind_allowed(spec_kinds: &[EdgeKind], kind: &EdgeKind) -> bool {
        spec_kinds.is_empty() || spec_kinds.contains(kind)
    }

    /// All file paths that have a stored digest. Used by the incremental CLI to detect deletions.
    pub fn indexed_files(&self) -> Vec<String> {
        self.file_digests.keys().cloned().collect()
    }

    /// Remove the digest entry for `file`. Called when a deleted file is cleaned up.
    pub fn remove_file_digest(&mut self, file: &str) {
        self.file_digests.remove(file);
    }

    // -----------------------------------------------------------------------
    // W11.2 — Versioned query cache (prior art versioned cache-port pattern).
    // -----------------------------------------------------------------------

    /// Return the cached value for `key` only if it was stored at the current graph version.
    /// Returns `None` when the key is absent or was stored at a prior version.
    pub fn cache_get(&self, key: &str) -> Result<Option<String>> {
        match self.cache.get(key) {
            Some((ver, val)) if *ver == self.graph_version => Ok(Some(val.clone())),
            _ => Ok(None),
        }
    }

    /// Store `value` for `key` at the current graph version.
    pub fn cache_put(&mut self, key: &str, value: &str) -> Result<()> {
        self.cache
            .insert(key.to_string(), (self.graph_version, value.to_string()));
        Ok(())
    }

    /// Increment the graph version. All cache entries stored at prior versions become stale.
    pub fn bump_version(&mut self) -> Result<()> {
        self.graph_version += 1;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // W5.2 — Vector embeddings (inherent, mirrors SqliteStore API).
    // -----------------------------------------------------------------------

    /// Store (or replace) the embedding vector for `symbol`.
    pub fn set_embedding(&mut self, symbol: &SymbolId, vec: &[f32]) -> Result<()> {
        if vec.is_empty() {
            return Err(wicked_estate_core::Error::Invalid(
                "embedding vector must be non-empty".into(),
            ));
        }
        self.embeddings.insert(symbol.clone(), vec.to_vec());
        Ok(())
    }

    /// Retrieve the stored embedding vector for `symbol`, or `None` if absent.
    pub fn embedding(&self, symbol: &SymbolId) -> Result<Option<Vec<f32>>> {
        Ok(self.embeddings.get(symbol).cloned())
    }

    /// Compact the store: prune dangling edges, stale cache entries, orphan embeddings and
    /// content, and edge-history beyond the 20-row-per-file retention window.
    /// Mirrors [`SqliteStore::compact`] — no VACUUM step because there is no on-disk file.
    pub fn compact(&mut self) -> Result<crate::sqlite::CompactStats> {
        // (1) prune dangling edges.
        let dangling_edges = self.prune_dangling_edges()?;

        // (2) prune stale cache rows.
        let current_ver = self.graph_version;
        let before_cache = self.cache.len();
        self.cache.retain(|_, (ver, _)| *ver >= current_ver);
        let stale_cache_rows = before_cache - self.cache.len();

        // (3) orphan embeddings: symbol not in nodes.
        let before_emb = self.embeddings.len();
        let nodes = &self.nodes;
        self.embeddings.retain(|sym, _| nodes.contains_key(sym));
        let orphan_embeddings = before_emb - self.embeddings.len();

        // (4) orphan content: git_sha not referenced by file_git_shas AND not referenced by
        //     any edge_history row.
        let live_shas: HashSet<&str> = self.file_git_shas.values().map(|s| s.as_str()).collect();
        let history_shas: HashSet<&str> = self
            .edge_history
            .iter()
            .map(|h| h.git_sha.as_str())
            .collect();
        let before_content = self.content.len();
        self.content.retain(|sha, _| {
            live_shas.contains(sha.as_str()) || history_shas.contains(sha.as_str())
        });
        let orphan_content = before_content - self.content.len();

        // (5) edge_history retention: keep newest 20 per file; delete older.
        // edge_history_files[i] is the file that was archived for edge_history[i].
        // Group by file using the parallel files vector (HistoricalEdge has no file field).
        debug_assert_eq!(
            self.edge_history.len(),
            self.edge_history_files.len(),
            "edge_history and edge_history_files must stay in sync"
        );
        let mut file_to_seqs: HashMap<&str, Vec<u64>> = HashMap::new();
        for (h, f) in self.edge_history.iter().zip(self.edge_history_files.iter()) {
            file_to_seqs
                .entry(f.as_str())
                .or_default()
                .push(h.archived_seq);
        }
        let mut keep_seqs: HashSet<u64> = HashSet::new();
        for seqs in file_to_seqs.values_mut() {
            seqs.sort_unstable_by(|a, b| b.cmp(a)); // descending
            for &seq in seqs.iter().take(20) {
                keep_seqs.insert(seq);
            }
        }
        let before_hist = self.edge_history.len();
        // Retain both vecs in sync by using index-based filtering.
        let mut new_history: Vec<HistoricalEdge> = Vec::with_capacity(self.edge_history.len());
        let mut new_files: Vec<String> = Vec::with_capacity(self.edge_history_files.len());
        for (h, f) in self
            .edge_history
            .drain(..)
            .zip(self.edge_history_files.drain(..))
        {
            if keep_seqs.contains(&h.archived_seq) {
                new_history.push(h);
                new_files.push(f);
            }
        }
        self.edge_history = new_history;
        self.edge_history_files = new_files;
        let history_rows_pruned = before_hist - self.edge_history.len();

        Ok(crate::sqlite::CompactStats {
            dangling_edges,
            stale_cache_rows,
            orphan_embeddings,
            orphan_content,
            history_rows_pruned,
        })
    }

    /// Find the `k` nearest symbols to `query` by cosine similarity (brute-force).
    ///
    /// Returns `(SymbolId, cosine_similarity)` pairs sorted descending (highest similarity
    /// first).  Ties broken by `SymbolId` lexicographic order for deterministic output.
    pub fn nearest(&self, query: &[f32], k: usize) -> Result<Vec<(SymbolId, f32)>> {
        if query.is_empty() || k == 0 {
            return Ok(vec![]);
        }
        let q_norm = mem_l2_norm(query);
        if q_norm == 0.0 {
            return Ok(vec![]);
        }
        let dim = query.len();
        let mut scored: Vec<(SymbolId, f32)> = self
            .embeddings
            .iter()
            .filter(|(_, v)| v.len() == dim)
            .map(|(id, v)| {
                let sim = mem_cosine_similarity(query, v, q_norm);
                (id.clone(), sim)
            })
            .collect();
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.0.cmp(&b.0.0))
        });
        scored.truncate(k);
        Ok(scored)
    }

    /// Return every stored `(symbol, embedding)` pair. Order is unspecified.
    ///
    /// Unlike [`nearest`](Self::nearest) (a top-k point query), this hands back the full vector
    /// set so analyses that operate over *all* embeddings — semantic clustering — can run without
    /// issuing N queries. O(n·d) clone.
    pub fn all_embeddings(&self) -> Result<Vec<(SymbolId, Vec<f32>)>> {
        Ok(self
            .embeddings
            .iter()
            .map(|(id, v)| (id.clone(), v.clone()))
            .collect())
    }
}

impl GraphWrite for MemStore {
    fn begin_batch(&mut self) -> Result<()> {
        self.in_batch = true;
        Ok(())
    }

    fn commit_batch(&mut self) -> Result<()> {
        self.in_batch = false;
        Ok(())
    }

    fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()> {
        for n in nodes {
            self.nodes.insert(n.symbol.clone(), n.clone());
        }
        Ok(())
    }

    fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()> {
        for e in edges {
            let key = e.dedup_key();
            match self.edges.iter_mut().find(|x| x.dedup_key() == key) {
                // On a collision the higher-confidence edge wins (W3.4 max-confidence merge).
                Some(existing) if e.confidence.get() >= existing.confidence.get() => {
                    *existing = e.clone();
                }
                Some(_) => {}
                None => self.edges.push(e.clone()),
            }
        }
        Ok(())
    }

    fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()> {
        self.unresolved.extend_from_slice(refs);
        Ok(())
    }

    fn remove_file(&mut self, file: &str) -> Result<()> {
        // Step 1: read current git_sha for this file (the version being superseded).
        let current_git_sha = self.file_git_shas.get(file).cloned().unwrap_or_default();

        // Step 2: collect the set of symbols defined in this file BEFORE we remove nodes.
        // We need it for archival (edges whose source is in this file) and for Step 3.
        let file_symbols: HashSet<SymbolId> = self
            .nodes
            .values()
            .filter(|n| n.location.file == file)
            .map(|n| n.symbol.clone())
            .collect();

        // Step 3: if history enabled, archive edges that belong to this file.
        // An edge "belongs to" this file when its location.file matches OR its source symbol
        // is defined in this file — covering edges created without an explicit location.
        if self.history_enabled {
            let edges_to_archive: Vec<Edge> = self
                .edges
                .iter()
                .filter(|e| {
                    let loc_file = e.location.as_ref().map(|l| l.file.as_str()).unwrap_or("");
                    loc_file == file || file_symbols.contains(&e.source)
                })
                .cloned()
                .collect();
            for edge in edges_to_archive {
                self.history_archive_seq += 1;
                self.edge_history.push(HistoricalEdge {
                    git_sha: current_git_sha.clone(),
                    archived_seq: self.history_archive_seq,
                    edge,
                });
                self.edge_history_files.push(file.to_string());
            }
        }

        // Step 4: remove nodes, edges, unresolved refs, digest, git_sha pointer.
        // file_symbols was already computed in Step 2 above.
        self.nodes.retain(|_, n| n.location.file != file);
        self.edges.retain(|e| {
            let loc_file = e.location.as_ref().map(|l| l.file.as_str()).unwrap_or("");
            loc_file != file && !file_symbols.contains(&e.source)
        });
        self.unresolved.retain(|r| r.location.file != file);
        self.file_digests.remove(file);
        self.file_git_shas.remove(file);
        // NOTE: do NOT remove from self.content — content is content-addressed and may be
        // retained for history; orphans are pruned in compact().
        // Remove embeddings for all removed symbols.
        for sym in &file_symbols {
            self.embeddings.remove(sym);
        }
        Ok(())
    }

    fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()> {
        self.file_digests
            .insert(file.to_string(), digest.to_string());
        Ok(())
    }

    fn set_file_content(&mut self, file: &str, text: &str) -> Result<()> {
        let sha = crate::sqlite::git_blob_sha(text);
        // Dedup: INSERT OR IGNORE semantics — only store if sha not already present.
        self.content
            .entry(sha.clone())
            .or_insert_with(|| text.to_string());
        // Update the file → sha pointer.
        self.file_git_shas.insert(file.to_string(), sha);
        Ok(())
    }

    fn prune_dangling_edges(&mut self) -> Result<usize> {
        let before = self.edges.len();
        self.edges
            .retain(|e| self.nodes.contains_key(&e.source) && self.nodes.contains_key(&e.target));
        Ok(before - self.edges.len())
    }

    fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()> {
        self.repo_info = Some(info.clone());
        Ok(())
    }

    fn log_change(&mut self, op: ChangeOp, target: &str) -> Result<()> {
        self.change_seq += 1;
        self.changes.push(Change {
            seq: self.change_seq,
            op,
            target: target.to_string(),
        });
        Ok(())
    }

    fn set_node_semantics(
        &mut self,
        symbol: &SymbolId,
        description: Option<&str>,
        requirement: Option<&str>,
        requirement_validated: Option<bool>,
    ) -> Result<()> {
        // No-op if the symbol is not a node.
        if !self.nodes.contains_key(symbol) {
            return Ok(());
        }
        // No-op if nothing is being changed.
        if description.is_none() && requirement.is_none() && requirement_validated.is_none() {
            return Ok(());
        }
        let entry = self.semantics.entry(symbol.clone()).or_default();
        if let Some(d) = description {
            entry.description = Some(d.to_string());
        }
        if let Some(r) = requirement {
            entry.requirement = Some(r.to_string());
        }
        if let Some(v) = requirement_validated {
            entry.requirement_validated = v;
        }
        Ok(())
    }
}

impl GraphRead for MemStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            full_text_search: false,
            vector_search: true, // W5.2: brute-force cosine via embeddings HashMap
            server_side_traversal: true, // in-process, no round-trips
            transactional_batch: false, // begin/commit are no-ops
            shared_writers: false,
        }
    }

    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
        Ok(self.nodes.get(id).cloned())
    }

    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
        let mut out: Vec<Node> = self
            .nodes
            .values()
            .filter(|n| {
                if let Some(name) = &query.exact_name {
                    if &n.name != name {
                        return false;
                    }
                }
                if let Some(text) = &query.text {
                    let hay = format!("{} {}", n.name, n.signature.clone().unwrap_or_default())
                        .to_lowercase();
                    if !hay.contains(&text.to_lowercase()) {
                        return false;
                    }
                }
                if !query.kinds.is_empty() && !query.kinds.contains(&n.kind) {
                    return false;
                }
                if let Some(lang) = &query.language {
                    if &n.language != lang {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.symbol.0.cmp(&b.symbol.0)); // deterministic
        if let Some(limit) = query.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>> {
        Ok(self
            .edges
            .iter()
            .filter(|e| match dir {
                Direction::Dependents => &e.target == id,
                Direction::Dependencies => &e.source == id,
                Direction::Both => &e.source == id || &e.target == id,
            })
            .cloned()
            .collect())
    }

    fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph> {
        let mut depths: BTreeMap<String, u32> = BTreeMap::new();
        let mut sub_nodes: Vec<Node> = Vec::new();
        let mut sub_edges: Vec<Edge> = Vec::new();
        let mut seen: HashSet<SymbolId> = HashSet::new();
        let mut queue: VecDeque<(SymbolId, u32)> = VecDeque::new();
        let mut truncated = false;

        seen.insert(start.clone());
        queue.push_back((start.clone(), 0));
        if let Some(n) = self.nodes.get(start) {
            sub_nodes.push(n.clone());
        }

        while let Some((cur, depth)) = queue.pop_front() {
            if depth >= spec.max_depth {
                continue;
            }
            for e in self.neighbors(&cur, spec.direction)? {
                if e.confidence.get() < spec.min_confidence {
                    continue;
                }
                if !Self::kind_allowed(&spec.edge_kinds, &e.kind) {
                    continue;
                }
                // The endpoint we advance to, relative to the traversal direction.
                let next = match spec.direction {
                    Direction::Dependents => e.source.clone(),
                    Direction::Dependencies => e.target.clone(),
                    Direction::Both => {
                        if e.source == cur {
                            e.target.clone()
                        } else {
                            e.source.clone()
                        }
                    }
                };
                sub_edges.push(e.clone());
                if seen.contains(&next) {
                    continue;
                }
                if sub_nodes.len() >= spec.max_nodes {
                    truncated = true;
                    continue;
                }
                seen.insert(next.clone());
                depths.insert(next.0.clone(), depth + 1);
                if let Some(n) = self.nodes.get(&next) {
                    sub_nodes.push(n.clone());
                }
                queue.push_back((next, depth + 1));
            }
        }

        Ok(Subgraph {
            nodes: sub_nodes,
            edges: sub_edges,
            depths,
            truncated,
        })
    }

    fn all_nodes(&self) -> Result<Vec<Node>> {
        Ok(self.nodes.values().cloned().collect())
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        Ok(self.edges.clone())
    }

    fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>> {
        Ok(self
            .unresolved
            .iter()
            .filter(|r| r.raw_name == name)
            .cloned()
            .collect())
    }

    fn file_digest(&self, file: &str) -> Result<Option<String>> {
        Ok(self.file_digests.get(file).cloned())
    }

    fn file_git_sha(&self, file: &str) -> Result<Option<String>> {
        Ok(self.file_git_shas.get(file).cloned())
    }

    fn repo_info(&self) -> Result<Option<RepoInfo>> {
        Ok(self.repo_info.clone())
    }

    fn changes_since(&self, cursor: u64) -> Result<Vec<Change>> {
        let out: Vec<Change> = self
            .changes
            .iter()
            .filter(|c| c.seq > cursor)
            .take(10_000)
            .cloned()
            .collect();
        Ok(out)
    }

    fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>> {
        // edge_history_files[i] is the file that was removed when edge_history[i] was archived.
        // Return entries for this file, newest first.
        let mut out: Vec<HistoricalEdge> = self
            .edge_history
            .iter()
            .zip(self.edge_history_files.iter())
            .filter(|(_, f)| f.as_str() == file)
            .map(|(h, _)| h.clone())
            .collect();
        out.sort_by_key(|h| std::cmp::Reverse(h.archived_seq));
        Ok(out)
    }

    fn file_content(&self, file: &str) -> Result<Option<String>> {
        // Resolve via content-addressed join: file_git_shas[file] → content[sha].
        let text = self
            .file_git_shas
            .get(file)
            .and_then(|sha| self.content.get(sha))
            .cloned();
        Ok(text)
    }

    fn symbol_source(&self, node: &Node) -> Result<Option<String>> {
        let span = node.location.span;
        if span.start_byte == 0 && span.end_byte == 0 {
            return Ok(None);
        }
        let text = match self.file_content(&node.location.file)? {
            Some(t) => t,
            None => return Ok(None),
        };
        let start = span.start_byte as usize;
        let end = span.end_byte as usize;
        if start > end || end > text.len() {
            return Ok(None);
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Ok(None);
        }
        Ok(Some(text[start..end].to_string()))
    }

    fn node_semantics(&self, symbol: &SymbolId) -> Result<Option<NodeSemantics>> {
        // Return None if the symbol has no recorded semantics (not the same as "absent node").
        Ok(self.semantics.get(symbol).cloned())
    }

    fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>> {
        let mut out: Vec<Node> = self
            .semantics
            .iter()
            .filter(|(_, s)| s.requirement.as_deref() == Some(requirement))
            .filter_map(|(sym, _)| self.nodes.get(sym).cloned())
            .collect();
        out.sort_by(|a, b| a.symbol.0.cmp(&b.symbol.0)); // deterministic
        Ok(out)
    }

    fn stats(&self) -> Result<GraphStats> {
        let mut nodes_by_kind: BTreeMap<String, u64> = BTreeMap::new();
        let mut file_count = 0u64;
        for n in self.nodes.values() {
            *nodes_by_kind
                .entry(serde_json::to_string(&n.kind).unwrap_or_default())
                .or_default() += 1;
            if matches!(n.kind, NodeKind::File) {
                file_count += 1;
            }
        }
        let mut edges_by_kind: BTreeMap<String, u64> = BTreeMap::new();
        for e in &self.edges {
            *edges_by_kind
                .entry(serde_json::to_string(&e.kind).unwrap_or_default())
                .or_default() += 1;
        }
        Ok(GraphStats {
            node_count: self.nodes.len() as u64,
            edge_count: self.edges.len() as u64,
            file_count,
            unresolved_ref_count: self.unresolved.len() as u64,
            nodes_by_kind,
            edges_by_kind,
            db_size_bytes: 0,
        })
    }
}

/// MemStore also serves as a [`SymbolIndex`] for the resolver pass.
impl SymbolIndex for MemStore {
    fn by_name(&self, name: &str) -> Vec<Node> {
        self.nodes
            .values()
            .filter(|n| n.name == name)
            .cloned()
            .collect()
    }
    fn get(&self, id: &SymbolId) -> Option<Node> {
        self.nodes.get(id).cloned()
    }
}

// ---------------------------------------------------------------------------
// Backend factory — the external-DB seam (docs/adr/ADR-003-storage-backends.md).
// Only SQLite is built; Postgres / SurrealDB are *designed* and return a clear
// "not yet built" error. Adding an external backend later is one match arm here,
// with zero changes to any caller (CLI / MCP / bench / indexer).
// ---------------------------------------------------------------------------

/// Where the graph lives, parsed from a connection spec by [`open_store`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreBackend {
    /// `sqlite://<path>`, a bare path, or `:memory:`.
    Sqlite { path: String },
    /// `postgres://…` — external relational backend (designed, ADR-003).
    Postgres { url: String },
    /// `surrealdb://…` — server graph backend (W1.5 bake-off challenger).
    SurrealDb { url: String },
}

impl StoreBackend {
    pub fn parse(spec: &str) -> StoreBackend {
        if spec == ":memory:" {
            StoreBackend::Sqlite {
                path: ":memory:".into(),
            }
        } else if let Some(rest) = spec.strip_prefix("sqlite://") {
            StoreBackend::Sqlite {
                path: rest.to_string(),
            }
        } else if spec.starts_with("postgres://") || spec.starts_with("postgresql://") {
            StoreBackend::Postgres {
                url: spec.to_string(),
            }
        } else if let Some(rest) = spec.strip_prefix("surrealdb://") {
            StoreBackend::SurrealDb {
                url: rest.to_string(),
            }
        } else {
            StoreBackend::Sqlite {
                path: spec.to_string(),
            } // bare path → sqlite file
        }
    }
}

/// Open a graph store from a connection spec. Every entrypoint goes through this one seam, so
/// an external backend drops in here with no caller changes.
pub fn open_store(spec: &str) -> Result<Box<dyn GraphStore>> {
    match StoreBackend::parse(spec) {
        StoreBackend::Sqlite { path } if path == ":memory:" => {
            Ok(Box::new(SqliteStore::in_memory()?))
        }
        StoreBackend::Sqlite { path } => Ok(Box::new(SqliteStore::open(path)?)),
        #[cfg(feature = "postgres")]
        StoreBackend::Postgres { url } => Ok(Box::new(PostgresStore::open(&url)?)),
        #[cfg(not(feature = "postgres"))]
        StoreBackend::Postgres { .. } => Err(Error::Invalid(
            "postgres backend requires the 'postgres' feature (ADR-003)".into(),
        )),
        StoreBackend::SurrealDb { .. } => Err(Error::Invalid(
            "surrealdb backend lands in the W1.5 bake-off".into(),
        )),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Extension traits for store operations beyond the frozen wicked-estate-core GraphStore trait.
//
// The `wicked-estate-core` GraphStore trait is object-safe and frozen.  `bump_version`, `cache_put`,
// `cache_get`, `meta_set`, `meta_get` are inherent methods on `SqliteStore` and `MemStore`.
//
// `GraphStoreMutExt` is an **extension supertrait** of `GraphStore` defined here in wicked-estate-store
// (not in wicked-estate-core).  `open_store_ext` returns `Box<dyn GraphStoreMutExt>`.  Callers that
// need versioning/meta/cache use `open_store_ext` instead of `open_store`.  Callers that
// only need topology (MCP reads) continue using `open_store` / `&dyn GraphRead`.
// ─────────────────────────────────────────────────────────────────────────────

/// Extension trait for mutable store operations not on the frozen wicked-estate-core GraphStore trait.
/// Object-safe (all methods take/return concrete types via `&str` / `Option<String>`).
/// Implemented for every concrete store shipped in this crate.
pub trait GraphStoreMutExt: GraphStore {
    /// Increment the graph version, invalidating all prior cache entries.
    fn version_bump(&mut self);
    /// Write an arbitrary key→value pair to the meta store (survives across sessions).
    fn meta_set_key(&mut self, key: &str, value: &str);
    /// Read a meta key. Returns `None` when absent.
    fn meta_get_key(&self, key: &str) -> Option<String>;
    /// Write a versioned cache entry (stale at next `version_bump`).
    fn cache_put_key(&mut self, key: &str, value: &str);
    /// Read a versioned cache entry. Returns `None` when absent or stale.
    fn cache_get_key(&self, key: &str) -> Option<String>;

    /// Upsert nodes into the nodes table WITHOUT touching the FTS index.
    ///
    /// Used by the hot write path in `index_path`: all nodes are written first with this
    /// cheaper call, then [`bulk_rebuild_fts_for_files`] populates FTS in one SQL pass after
    /// all rows exist.  This avoids the O(2 × nodes) per-node DELETE+INSERT into the FTS5
    /// shadow tables during the main write loop.
    ///
    /// For `MemStore` this is identical to `upsert_nodes` (no separate FTS structure).
    fn upsert_nodes_skip_fts(
        &mut self,
        nodes: &[wicked_estate_core::Node],
    ) -> wicked_estate_core::Result<()>;

    /// Bulk-rebuild the FTS index for every node that belongs to any of the given `files`.
    ///
    /// For `SqliteStore`: executes
    ///   `DELETE FROM nodes_fts WHERE symbol IN (SELECT symbol FROM nodes WHERE file IN (...))`
    ///   followed by
    ///   `INSERT INTO nodes_fts(symbol,name,signature,doc) SELECT symbol,name,...`
    /// — two statements regardless of the number of nodes, replacing the old O(2 × nodes)
    /// per-node loop.
    ///
    /// For `MemStore`: no-op (MemStore has no FTS shadow table).
    fn bulk_rebuild_fts_for_files(&mut self, files: &[&str]) -> wicked_estate_core::Result<()>;

    /// Reclaim freelist pages back to the OS via `PRAGMA incremental_vacuum`.
    ///
    /// Only meaningful for `SqliteStore` (which sets `auto_vacuum=INCREMENTAL`).  All other
    /// backends use the default no-op below.  Never returns an error for the NONE-mode case
    /// — that PRAGMA is a documented no-op when `auto_vacuum=NONE`.
    fn incremental_vacuum(&mut self) -> wicked_estate_core::Result<()> {
        Ok(())
    }
}

impl GraphStoreMutExt for SqliteStore {
    fn version_bump(&mut self) {
        let _ = self.bump_version();
    }
    fn meta_set_key(&mut self, key: &str, value: &str) {
        let _ = self.meta_set(key, value);
    }
    fn meta_get_key(&self, key: &str) -> Option<String> {
        self.meta_get(key).ok().flatten()
    }
    fn cache_put_key(&mut self, key: &str, value: &str) {
        let _ = self.cache_put(key, value);
    }
    fn cache_get_key(&self, key: &str) -> Option<String> {
        self.cache_get(key).ok().flatten()
    }

    fn upsert_nodes_skip_fts(
        &mut self,
        nodes: &[wicked_estate_core::Node],
    ) -> wicked_estate_core::Result<()> {
        self.upsert_nodes_no_fts(nodes)
    }

    fn bulk_rebuild_fts_for_files(&mut self, files: &[&str]) -> wicked_estate_core::Result<()> {
        self.rebuild_fts_for_files(files)
    }

    fn incremental_vacuum(&mut self) -> wicked_estate_core::Result<()> {
        self.incremental_vacuum()
    }
}

impl GraphStoreMutExt for MemStore {
    fn version_bump(&mut self) {
        let _ = self.bump_version();
    }
    fn meta_set_key(&mut self, key: &str, value: &str) {
        self.meta.insert(key.to_string(), value.to_string());
    }
    fn meta_get_key(&self, key: &str) -> Option<String> {
        self.meta.get(key).cloned()
    }
    fn cache_put_key(&mut self, key: &str, value: &str) {
        let _ = self.cache_put(key, value);
    }
    fn cache_get_key(&self, key: &str) -> Option<String> {
        self.cache_get(key).ok().flatten()
    }

    /// MemStore has no FTS shadow table — identical to `upsert_nodes`.
    fn upsert_nodes_skip_fts(
        &mut self,
        nodes: &[wicked_estate_core::Node],
    ) -> wicked_estate_core::Result<()> {
        use wicked_estate_core::GraphWrite;
        self.upsert_nodes(nodes)
    }

    /// MemStore has no FTS shadow table — no-op.
    fn bulk_rebuild_fts_for_files(&mut self, _files: &[&str]) -> wicked_estate_core::Result<()> {
        Ok(())
    }
}

/// Open a store from a spec and return a `Box<dyn GraphStoreMutExt>`.
///
/// Use instead of `open_store` when the caller needs versioning, meta, or cache access.
/// `Box<dyn GraphStoreMutExt>` coerces to `Box<dyn GraphStore>` via deref, and the inner
/// value implements all of `GraphRead`, `GraphWrite`, and `GraphStore`.
pub fn open_store_ext(spec: &str) -> wicked_estate_core::Result<Box<dyn GraphStoreMutExt>> {
    match StoreBackend::parse(spec) {
        StoreBackend::Sqlite { path } if path == ":memory:" => {
            Ok(Box::new(SqliteStore::in_memory()?))
        }
        StoreBackend::Sqlite { path } => Ok(Box::new(SqliteStore::open(path)?)),
        #[cfg(feature = "postgres")]
        StoreBackend::Postgres { url } => Ok(Box::new(PostgresStore::open(&url)?)),
        #[cfg(not(feature = "postgres"))]
        StoreBackend::Postgres { .. } => Err(Error::Invalid(
            "postgres backend requires the 'postgres' feature (ADR-003)".into(),
        )),
        StoreBackend::SurrealDb { .. } => Err(Error::Invalid(
            "surrealdb backend lands in the W1.5 bake-off".into(),
        )),
    }
}

#[cfg(feature = "postgres")]
impl GraphStoreMutExt for PostgresStore {
    fn version_bump(&mut self) {
        let _ = self.bump_version();
    }
    fn meta_set_key(&mut self, key: &str, value: &str) {
        let _ = self.meta_set(key, value);
    }
    fn meta_get_key(&self, key: &str) -> Option<String> {
        self.meta_get(key).ok().flatten()
    }
    fn cache_put_key(&mut self, key: &str, value: &str) {
        let _ = self.cache_put(key, value);
    }
    fn cache_get_key(&self, key: &str) -> Option<String> {
        self.cache_get(key).ok().flatten()
    }

    /// PostgresStore uses column-level trigram index — no separate FTS shadow table.
    /// Identical to `upsert_nodes`.
    fn upsert_nodes_skip_fts(
        &mut self,
        nodes: &[wicked_estate_core::Node],
    ) -> wicked_estate_core::Result<()> {
        use wicked_estate_core::GraphWrite;
        self.upsert_nodes(nodes)
    }

    /// PostgresStore uses column-level trigram index — no separate FTS table to rebuild.
    fn bulk_rebuild_fts_for_files(&mut self, _files: &[&str]) -> wicked_estate_core::Result<()> {
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — W5.2 vector storage (MemStore)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> SymbolId {
        SymbolId(s.to_string())
    }

    // -- helper: build a normalised unit vector pointing mostly along axis `i` ----
    fn unit_vec(dim: usize, i: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; dim];
        v[i] = 1.0;
        v
    }

    // -- round-trip ---------------------------------------------------------------

    #[test]
    fn mem_set_get_embedding_roundtrip() {
        let mut store = MemStore::new();
        let id = sym("foo");
        let vec = vec![0.1_f32, 0.2, 0.3];
        store.set_embedding(&id, &vec).unwrap();
        let got = store.embedding(&id).unwrap().expect("should be present");
        assert_eq!(got.len(), 3);
        for (a, b) in got.iter().zip(vec.iter()) {
            assert!((a - b).abs() < 1e-6, "roundtrip value mismatch");
        }
    }

    #[test]
    fn mem_embedding_absent_returns_none() {
        let store = MemStore::new();
        assert!(store.embedding(&sym("missing")).unwrap().is_none());
    }

    #[test]
    fn mem_set_embedding_empty_vec_returns_error() {
        let mut store = MemStore::new();
        assert!(store.set_embedding(&sym("bad"), &[]).is_err());
    }

    // -- nearest ------------------------------------------------------------------

    #[test]
    fn mem_nearest_returns_closest_first() {
        let mut store = MemStore::new();
        // dim=4; each symbol aligns with a different axis.
        store.set_embedding(&sym("a"), &unit_vec(4, 0)).unwrap(); // [1,0,0,0]
        store.set_embedding(&sym("b"), &unit_vec(4, 1)).unwrap(); // [0,1,0,0]
        store.set_embedding(&sym("c"), &unit_vec(4, 2)).unwrap(); // [0,0,1,0]

        // Query close to "a".
        let q = vec![0.9_f32, 0.1, 0.0, 0.0];
        let results = store.nearest(&q, 3).unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, sym("a"), "a should be nearest");
        // Similarities must be non-increasing.
        assert!(results[0].1 >= results[1].1);
        assert!(results[1].1 >= results[2].1);
    }

    #[test]
    fn mem_nearest_exact_match_scores_one() {
        let mut store = MemStore::new();
        let v = unit_vec(3, 0);
        store.set_embedding(&sym("x"), &v).unwrap();
        let results = store.nearest(&v, 1).unwrap();
        assert_eq!(results.len(), 1);
        assert!(
            (results[0].1 - 1.0).abs() < 1e-5,
            "cosine of identical vectors = 1"
        );
    }

    #[test]
    fn mem_nearest_k_larger_than_store_returns_all() {
        let mut store = MemStore::new();
        store.set_embedding(&sym("p"), &unit_vec(2, 0)).unwrap();
        store.set_embedding(&sym("q"), &unit_vec(2, 1)).unwrap();
        let results = store.nearest(&[1.0, 0.0], 100).unwrap();
        assert_eq!(results.len(), 2, "k > stored count → return all");
    }

    #[test]
    fn mem_nearest_dim_mismatch_skipped() {
        let mut store = MemStore::new();
        store.set_embedding(&sym("dim2"), &[1.0_f32, 0.0]).unwrap();
        // Query with dim=3 — should not panic, should skip dim-2 vector.
        let results = store.nearest(&[1.0_f32, 0.0, 0.0], 5).unwrap();
        assert!(results.is_empty(), "dim-mismatch entries silently skipped");
    }

    #[test]
    fn mem_nearest_deterministic_ordering() {
        let mut store = MemStore::new();
        // Two vectors with identical cosine similarity to query.
        let v = unit_vec(2, 0);
        store.set_embedding(&sym("z"), &v).unwrap();
        store.set_embedding(&sym("a"), &v).unwrap();
        let r1 = store.nearest(&v, 2).unwrap();
        let r2 = store.nearest(&v, 2).unwrap();
        let ids1: Vec<_> = r1.iter().map(|(id, _)| id.0.clone()).collect();
        let ids2: Vec<_> = r2.iter().map(|(id, _)| id.0.clone()).collect();
        assert_eq!(ids1, ids2, "identical calls must return identical order");
        // Tie broken by SymbolId lex order: "a" < "z".
        assert_eq!(ids1[0], "a");
        assert_eq!(ids1[1], "z");
    }

    #[test]
    fn mem_nearest_empty_store_returns_empty() {
        let store = MemStore::new();
        let results = store.nearest(&[1.0, 0.0], 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn mem_capabilities_vector_search_true() {
        let store = MemStore::new();
        assert!(
            store.capabilities().vector_search,
            "MemStore must report vector_search = true"
        );
    }

    // ── Fix A: remove_file clears content + embeddings ───────────────────────

    fn make_node(symbol: &str, file: &str) -> wicked_estate_core::Node {
        wicked_estate_core::Node::new(
            wicked_estate_core::SymbolId(symbol.to_string()),
            wicked_estate_core::NodeKind::Function,
            symbol,
            wicked_estate_core::Language::new("rust"),
            wicked_estate_core::Location::new(file, wicked_estate_core::Span::ZERO),
        )
    }

    #[test]
    fn mem_remove_file_clears_content_row() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node("fn_a", "src/a.rs")])
            .unwrap();
        store.set_file_content("src/a.rs", "fn fn_a() {}").unwrap();

        assert!(store.file_content("src/a.rs").unwrap().is_some());

        // remove_file removes the file→sha pointer, so file_content returns None even
        // though the content itself may still sit in the content store until compact().
        store.remove_file("src/a.rs").unwrap();
        assert!(
            store.file_content("src/a.rs").unwrap().is_none(),
            "file_content must return None when the file→sha pointer is removed"
        );
    }

    #[test]
    fn mem_remove_file_clears_embeddings() {
        let mut store = MemStore::new();
        let id = wicked_estate_core::SymbolId("fn_b".to_string());
        store
            .upsert_nodes(&[make_node("fn_b", "src/b.rs")])
            .unwrap();
        store.set_embedding(&id, &[1.0_f32, 0.0]).unwrap();

        assert!(store.embedding(&id).unwrap().is_some());

        store.remove_file("src/b.rs").unwrap();
        assert!(
            store.embedding(&id).unwrap().is_none(),
            "embedding must be cleared when owning file is removed"
        );
    }

    // ── prune_dangling_edges ─────────────────────────────────────────────────

    #[test]
    fn mem_prune_dangling_edges_removes_orphans_keeps_valid() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        let mut store = MemStore::new();

        let a = sym("a");
        let b = sym("b");
        let ghost = sym("ghost");

        store
            .upsert_nodes(&[make_node("a", "src/lib.rs"), make_node("b", "src/lib.rs")])
            .unwrap();
        let valid_edge = Edge::new(
            a.clone(),
            b.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        let dangling_edge = Edge::new(
            a.clone(),
            ghost.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        store.upsert_edges(&[valid_edge, dangling_edge]).unwrap();
        assert_eq!(store.all_edges().unwrap().len(), 2);

        let pruned = store.prune_dangling_edges().unwrap();
        assert_eq!(pruned, 1, "one dangling edge removed");

        let remaining = store.all_edges().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].source, a);
        assert_eq!(remaining[0].target, b);
    }

    // ── compact ──────────────────────────────────────────────────────────────

    #[test]
    fn mem_compact_prunes_stale_cache_and_reports_stats() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node("fn_c", "src/c.rs")])
            .unwrap();
        store.set_file_content("src/c.rs", "fn fn_c() {}").unwrap();

        // Insert a cache entry at version 0, then bump so it becomes stale.
        store.cache_put("old_key", "old_val").unwrap();
        store.bump_version().unwrap();
        store.cache_put("new_key", "new_val").unwrap();

        let stats = store.compact().unwrap();
        assert_eq!(
            stats.stale_cache_rows, 1,
            "stale entry at version 0 must be pruned"
        );
        assert_eq!(stats.dangling_edges, 0);
        assert_eq!(stats.orphan_embeddings, 0);
        assert_eq!(stats.orphan_content, 0);

        assert_eq!(
            store.cache_get("new_key").unwrap(),
            Some("new_val".to_string())
        );
    }

    #[test]
    fn mem_compact_prunes_orphan_embeddings_and_content() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        let mut store = MemStore::new();

        let a = sym("a");
        let ghost = sym("ghost");

        store.upsert_nodes(&[make_node("a", "src/a.rs")]).unwrap();
        store.set_file_content("src/a.rs", "fn a() {}").unwrap();
        // Orphan content: insert directly into content map with a sha that no file references.
        store
            .content
            .insert("deadbeef".to_string(), "// dead".to_string());
        store.set_embedding(&a, &[1.0_f32, 0.0]).unwrap();
        store.set_embedding(&ghost, &[0.0_f32, 1.0]).unwrap();
        let dangling = Edge::new(
            a.clone(),
            ghost.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        store.upsert_edges(&[dangling]).unwrap();

        let stats = store.compact().unwrap();
        assert_eq!(stats.dangling_edges, 1);
        assert_eq!(stats.orphan_embeddings, 1);
        assert_eq!(stats.orphan_content, 1);

        assert!(store.embedding(&a).unwrap().is_some());
        assert!(store.file_content("src/a.rs").unwrap().is_some());
    }

    // ── Wave 7: git blob SHA + content-addressing ────────────────────────────

    #[test]
    fn mem_file_git_sha_after_set_file_content() {
        use wicked_estate_core::GraphWrite;
        let mut store = MemStore::new();
        store.set_file_content("src/hello.rs", "hello").unwrap();
        let sha = store
            .file_git_sha("src/hello.rs")
            .unwrap()
            .expect("sha must be set");
        assert_eq!(sha, "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0");
    }

    #[test]
    fn mem_content_dedup_identical_text() {
        use wicked_estate_core::GraphWrite;
        let mut store = MemStore::new();
        store.set_file_content("a.rs", "fn x() {}").unwrap();
        store.set_file_content("b.rs", "fn x() {}").unwrap();
        let sha_a = store.file_git_sha("a.rs").unwrap().unwrap();
        let sha_b = store.file_git_sha("b.rs").unwrap().unwrap();
        assert_eq!(sha_a, sha_b, "identical content → same git_sha");
        assert_eq!(store.content.len(), 1, "one content row for identical text");
        assert_eq!(
            store.file_content("a.rs").unwrap(),
            Some("fn x() {}".to_string())
        );
        assert_eq!(
            store.file_content("b.rs").unwrap(),
            Some("fn x() {}".to_string())
        );
    }

    // ── Wave 7.1: changes_since ─────────────────────────────────────────────

    #[test]
    fn mem_changes_since_order_and_resume() {
        use wicked_estate_core::{ChangeOp, GraphWrite};
        let mut store = MemStore::new();
        store.log_change(ChangeOp::Upsert, "a.rs").unwrap();
        store.log_change(ChangeOp::Upsert, "b.rs").unwrap();
        store.log_change(ChangeOp::Remove, "c.rs").unwrap();

        let all = store.changes_since(0).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].target, "a.rs");
        assert_eq!(all[2].op, ChangeOp::Remove);

        let after = store.changes_since(all[1].seq).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].target, "c.rs");
    }

    // ── Wave 7: repo_info round-trip ─────────────────────────────────────────

    #[test]
    fn mem_repo_info_roundtrip() {
        use wicked_estate_core::{GraphWrite, RepoInfo};
        let mut store = MemStore::new();
        assert!(store.repo_info().unwrap().is_none());

        let info = RepoInfo {
            commit: Some("abc123".to_string()),
            branch: Some("main".to_string()),
            remote: None,
            dirty: false,
        };
        store.set_repo_info(&info).unwrap();
        let got = store.repo_info().unwrap().expect("must be Some after set");
        assert_eq!(got.commit, Some("abc123".to_string()));
        assert_eq!(got.branch, Some("main".to_string()));
        assert!(!got.dirty);
    }

    // ── Wave 7: edge_history archival ────────────────────────────────────────

    #[test]
    fn mem_edge_history_archived_on_remove_file() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        // history must be ON to assert archival behaviour.
        let mut store = MemStore::new_with_history();

        let v1_text = "fn foo() {}";
        store.set_file_content("src/foo.rs", v1_text).unwrap();
        let v1_sha = store.file_git_sha("src/foo.rs").unwrap().unwrap();

        store
            .upsert_nodes(&[make_node("foo", "src/foo.rs")])
            .unwrap();
        store
            .upsert_nodes(&[make_node("bar", "src/bar.rs")])
            .unwrap();
        let e = Edge::new(
            wicked_estate_core::SymbolId("foo".to_string()),
            wicked_estate_core::SymbolId("bar".to_string()),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        store.upsert_edges(&[e]).unwrap();

        store.remove_file("src/foo.rs").unwrap();

        let history = store.edge_history("src/foo.rs").unwrap();
        assert_eq!(history.len(), 1, "one superseded edge must be in history");
        assert_eq!(history[0].git_sha, v1_sha);
    }

    // ── Wave 7: edge_history retention prune ─────────────────────────────────

    #[test]
    fn mem_compact_prunes_edge_history_beyond_retention() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        // history must be ON to populate edge_history via remove_file.
        let mut store = MemStore::new_with_history();

        store
            .upsert_nodes(&[make_node("target", "src/other.rs")])
            .unwrap();
        for i in 0..25_u32 {
            let text = format!("fn ver_{i}() {{}}");
            store.set_file_content("src/ver.rs", &text).unwrap();
            store
                .upsert_nodes(&[make_node("ver_fn", "src/ver.rs")])
                .unwrap();
            let e = Edge::new(
                wicked_estate_core::SymbolId("ver_fn".to_string()),
                wicked_estate_core::SymbolId("target".to_string()),
                EdgeKind::Calls,
                ResolutionTier::Parsed,
                "test",
            );
            store.upsert_edges(&[e]).unwrap();
            store.remove_file("src/ver.rs").unwrap();
        }

        let before = store.edge_history.len();
        assert_eq!(before, 25);

        let stats = store.compact().unwrap();
        assert_eq!(stats.history_rows_pruned, 5);
        assert_eq!(store.edge_history.len(), 20);
    }

    // ── Semantic linking (MemStore) ──────────────────────────────────────────

    #[test]
    fn mem_node_semantics_absent_before_annotation() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node("fn_a", "src/a.rs")])
            .unwrap();
        let got = store.node_semantics(&sym("fn_a")).unwrap();
        assert!(
            got.is_none(),
            "node_semantics must be None before any annotation"
        );
    }

    #[test]
    fn mem_node_semantics_full_roundtrip() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node("fn_b", "src/b.rs")])
            .unwrap();
        store
            .set_node_semantics(
                &sym("fn_b"),
                Some("does the thing"),
                Some("REQ-42"),
                Some(true),
            )
            .unwrap();
        let got = store
            .node_semantics(&sym("fn_b"))
            .unwrap()
            .expect("must be Some after full write");
        assert_eq!(got.description, Some("does the thing".to_string()));
        assert_eq!(got.requirement, Some("REQ-42".to_string()));
        assert!(got.requirement_validated);
    }

    #[test]
    fn mem_node_semantics_partial_update_preserves_untouched_fields() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node("fn_c", "src/c.rs")])
            .unwrap();
        store
            .set_node_semantics(&sym("fn_c"), Some("original"), Some("REQ-7"), Some(true))
            .unwrap();
        // Partial: change only description.
        store
            .set_node_semantics(&sym("fn_c"), Some("updated"), None, None)
            .unwrap();
        let got = store
            .node_semantics(&sym("fn_c"))
            .unwrap()
            .expect("must still be Some");
        assert_eq!(
            got.description,
            Some("updated".to_string()),
            "description updated"
        );
        assert_eq!(
            got.requirement,
            Some("REQ-7".to_string()),
            "requirement unchanged"
        );
        assert!(got.requirement_validated, "validated flag unchanged");
    }

    #[test]
    fn mem_find_by_requirement_returns_annotated_nodes() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node("fn_x", "src/x.rs"), make_node("fn_y", "src/y.rs")])
            .unwrap();
        store
            .set_node_semantics(&sym("fn_x"), Some("desc x"), Some("REQ-99"), Some(false))
            .unwrap();
        store
            .set_node_semantics(&sym("fn_y"), Some("desc y"), Some("REQ-other"), Some(false))
            .unwrap();
        let found = store.find_by_requirement("REQ-99").unwrap();
        assert_eq!(found.len(), 1, "exactly one node matches REQ-99");
        assert_eq!(found[0].symbol, sym("fn_x"));
    }

    #[test]
    fn mem_set_node_semantics_absent_symbol_noop() {
        let mut store = MemStore::new();
        store
            .set_node_semantics(&sym("ghost"), Some("desc"), Some("REQ-1"), Some(false))
            .unwrap();
        assert!(
            store.node_semantics(&sym("ghost")).unwrap().is_none(),
            "absent symbol must remain without semantics"
        );
    }

    #[test]
    fn mem_set_node_semantics_all_none_noop() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node("fn_d", "src/d.rs")])
            .unwrap();
        store
            .set_node_semantics(&sym("fn_d"), None, None, None)
            .unwrap();
        assert!(
            store.node_semantics(&sym("fn_d")).unwrap().is_none(),
            "all-None call must leave semantics as None"
        );
    }
}
