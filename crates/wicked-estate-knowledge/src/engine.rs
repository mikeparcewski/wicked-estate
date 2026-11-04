//! `KnowledgeEngine` — the 3rd engine's storage + recall, mirroring the memory topology VERBATIM
//! (D2.1, the OQ-B1 resolution): **own file, own `nodes_fts`, single writer**. Because the store is a
//! separate `SqliteStore`, FTS dilution and multi-writer `SQLITE_BUSY` are *structurally impossible*
//! (B3 / B-BLOCK-1/2) — not policed, just unreachable.
//!
//! Knowledge nodes ride estate `Node{kind = Other("kdoc"|"ksection"|"kchunk"|"kconcept")}` with their
//! fields in `metadata` (D1.1 — the `Other("memory")` precedent, G5); stable identity is
//! `Symbol::synthetic(class, uuid-v7)`. Recall **REUSES** the `wicked-memory-core` pipeline
//! (`rrf_fuse` + `budget_pack` + `Candidate`) over this store's FTS + vector — there is **no second
//! `recall_impl`** (R3): the fusion/budget math lives once, in `-core`.

use uuid::Uuid;
use wicked_estate_core::{
    Confidence, Direction, Edge, EdgeKind, GraphRead, GraphWrite, Language, Location, Node,
    NodeKind, ResolutionTier, Result, Scope, Span, Symbol, SymbolId, SymbolQuery,
};
use wicked_estate_memory_core::{Candidate, Tier, budget_pack, rrf_fuse};
use wicked_estate_overlay::XedgeStore;
use wicked_estate_retrieve::{Embedder, HashEmbedder, VectorStore};
use wicked_estate_store::SqliteStore;

use crate::plane::{Visible, effective_deps, is_untrusted, scope_deps};

/// The four knowledge node classes (D1.1). Ride `NodeKind::Other("k*")` so estate stores them
/// generically and they NEVER collide with code (`function`/`class`/…) or memory (`Other("memory")`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KClass {
    /// A whole ingested document.
    Doc,
    /// A section within a document.
    Section,
    /// A retrievable chunk (the unit recall returns).
    Chunk,
    /// A distilled concept (the relation-typing pass's node).
    Concept,
}

impl KClass {
    pub fn as_kind(self) -> &'static str {
        match self {
            KClass::Doc => "kdoc",
            KClass::Section => "ksection",
            KClass::Chunk => "kchunk",
            KClass::Concept => "kconcept",
        }
    }
    /// The class name the tools take and recall reports (`doc|section|chunk|concept`).
    pub fn as_class(self) -> &'static str {
        match self {
            KClass::Doc => "doc",
            KClass::Section => "section",
            KClass::Chunk => "chunk",
            KClass::Concept => "concept",
        }
    }
    pub fn from_kind(k: &str) -> Option<Self> {
        match k {
            "kdoc" => Some(KClass::Doc),
            "ksection" => Some(KClass::Section),
            "kchunk" => Some(KClass::Chunk),
            "kconcept" => Some(KClass::Concept),
            _ => None,
        }
    }
    /// Is `kind` one of the knowledge classes? (the kind-guard predicate, shared with `erase`).
    pub fn is_knowledge_kind(k: &str) -> bool {
        Self::from_kind(k).is_some()
    }
}

/// Knowledge metadata keys (the opaque ExtensionData slot — estate-core never reads these).
pub mod meta_keys {
    pub const KCLASS: &str = "kclass";
    pub const CONTENT: &str = "content";
    pub const SCOPE: &str = "scope";
    pub const SOURCE: &str = "source";
    pub const CREATED_AT: &str = "created_at";
    /// Dedup: the loser's source/id appended here on collapse-but-surface (D4.1).
    pub const ALSO_FOUND_IN: &str = "also_found_in";
    /// Dedup: the canonical node carries `canonical = true`.
    pub const CANONICAL: &str = "canonical";
    /// DES-KNOWLEDGE-DATA-001 §4.3: the sources and repos (`source:<id>` / `repo:<id>`) a node was
    /// derived from. A JSON array; absent = none. See [`crate::plane`]. (A `project:<id>` dep is
    /// carried only by link edges, never stored on a node.)
    pub const DEPS: &str = "deps";
    /// `true` when the node's text came from a source the person does not control (catalog
    /// comments, vendor descriptions): consumers fence it as data, never instructions.
    pub const UNTRUSTED: &str = "untrusted";
}

/// A knowledge node before it is written as an estate `Node`.
#[derive(Debug, Clone)]
pub struct KNode {
    pub id: String,
    pub class: KClass,
    pub content: String,
    pub scope: String,
    pub source: String,
    pub created_at: i64,
}

impl KNode {
    /// New knowledge node with a fresh uuid-v7 identity.
    pub fn new(
        class: KClass,
        content: impl Into<String>,
        scope: impl Into<String>,
        source: impl Into<String>,
        now: i64,
    ) -> Self {
        Self {
            id: Uuid::now_v7().to_string(),
            class,
            content: content.into(),
            scope: scope.into(),
            source: source.into(),
            created_at: now,
        }
    }

    /// Stable estate symbol id (`Synthetic{scheme:<class>, id:<uuid-v7>}`).
    pub fn symbol(&self) -> SymbolId {
        Symbol::synthetic(self.class.as_kind(), self.id.clone()).id()
    }

    /// Materialize as an estate `Node` (kind `Other("k*")`, fields in `metadata`).
    ///
    /// The scope is stamped TWICE: canonically on `Node.scope` (so the store-side
    /// `SymbolQuery::scope_prefix` predicate — pushed into FTS candidate retrieval BEFORE any
    /// `limit`, multi-tenant isolation — can see it) and verbatim in `metadata` (the wire field
    /// [`KNode::from_node`] hydrates). Nodes written before scope stamping (≤ 0.15) carry scope
    /// only in metadata; see [`KnowledgeEngine::recall`] for how they behave under a scoped recall.
    pub fn to_node(&self) -> Node {
        let mut node = Node::new(
            self.symbol(),
            NodeKind::Other(self.class.as_kind().to_string()),
            self.content.clone(),
            Language::new("knowledge"),
            Location::new("knowledge", Span::ZERO),
        )
        .with_scope(Scope::parse(&self.scope));
        let m = &mut node.metadata;
        m.insert(meta_keys::KCLASS.into(), self.class.as_kind().into());
        m.insert(meta_keys::CONTENT.into(), self.content.clone().into());
        m.insert(meta_keys::SCOPE.into(), self.scope.clone().into());
        m.insert(meta_keys::SOURCE.into(), self.source.clone().into());
        m.insert(meta_keys::CREATED_AT.into(), self.created_at.into());
        node
    }

    /// Reconstruct from an estate `Node` written by [`KNode::to_node`]. `None` if not a knowledge node.
    pub fn from_node(node: &Node) -> Option<KNode> {
        let NodeKind::Other(k) = &node.kind else {
            return None;
        };
        let class = KClass::from_kind(k)?;
        let m = &node.metadata;
        let s = |key: &str| m.get(key).and_then(|v| v.as_str()).map(|s| s.to_string());
        let id = node
            .symbol
            .as_str()
            .rsplit(' ')
            .next()
            .unwrap_or("")
            .trim_end_matches(':')
            .to_string();
        Some(KNode {
            id,
            class,
            content: s(meta_keys::CONTENT).unwrap_or_else(|| node.name.clone()),
            scope: s(meta_keys::SCOPE).unwrap_or_default(),
            source: s(meta_keys::SOURCE).unwrap_or_default(),
            created_at: m
                .get(meta_keys::CREATED_AT)
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        })
    }
}

/// A recalled knowledge slice.
#[derive(Debug, Clone)]
pub struct KRecalled {
    pub id: SymbolId,
    pub content: String,
    pub score: f64,
    pub source: String,
}

/// A recalled knowledge slice with its place in the knowledge plane (DES-KNOWLEDGE-DATA-001 §4.3):
/// what [`KnowledgeEngine::recall_visible`] returns.
#[derive(Debug, Clone)]
pub struct KHit {
    pub id: SymbolId,
    pub content: String,
    pub score: f64,
    pub source: String,
    pub class: KClass,
    /// The node's own scope (verbatim, as written).
    pub scope: String,
    /// The node's effective dependency set (§4.3), every one of which the caller's `visible` held.
    pub deps: Vec<String>,
    /// Text from a source the person does not control: fence it as data.
    pub untrusted: bool,
}

/// One recall-miss sidecar row (T-B-MISSLOG seeds this; the foundation scaffolds the shape).
#[derive(Debug, Clone)]
pub struct RecallMiss {
    pub query: String,
    pub scope: String,
    pub ts: i64,
    pub result_count: usize,
    pub top_score: f64,
}

/// Knowledge scope-subtree predicate (arch-R5): the estate-wide segment-aware
/// `Scope::path_in_prefix` (`""` = everything, `"wiki:architecture"` = that scope + descendants,
/// mid-segment prefixes match nothing), EXTENDED with one knowledge-side form — a prefix ending in
/// `:` is a **kind wildcard**: `"wiki:"` matches every scope whose segment at that position
/// carries the `wiki` kind (`wiki:architecture`, `wiki:event-grammar`, …). Memory's predicate has
/// no such form (a partial segment matches nothing there); the extension is what lets one recall
/// span every `wiki:<area>` subtree, per the wiki scoping convention.
fn scope_in_prefix(scope: &str, prefix: &str) -> bool {
    let canonical = Scope::parse(scope);
    if prefix.ends_with(':') {
        // Kind wildcard: compare against the canonical rendering — the colon bounds the kind, so
        // "wiki:" cannot match a "wikix:…" scope, and "org:acme/unit:" scopes to acme's units.
        return canonical.as_path().starts_with(prefix);
    }
    canonical.path_in_prefix(prefix)
}

/// The knowledge engine: its OWN single-writer SqliteStore (own file → own FTS, DEC-1).
pub struct KnowledgeEngine {
    pub(crate) store: SqliteStore,
    embedder: Box<dyn Embedder>,
    /// RRF over-fetch per retriever.
    k: usize,
    /// recall floor: a recall whose top score is below this logs a miss (default 0 = log empties only).
    recall_floor: f64,
    /// in-process recall-miss log (the `memext` sidecar pattern; persisted form lands at T-B-MISSLOG).
    misses: Vec<RecallMiss>,
    pub(crate) xedge: Option<std::sync::Arc<XedgeStore>>,
    /// This engine's latest approval attempt per proposal id (DES-KNOWLEDGE-DATA-001 §10.1): its
    /// own failure path rolls back exactly that attempt and no other process's.
    pub(crate) attempts: std::collections::HashMap<String, String>,
    /// Test failpoint: `replace_source` writes this many more nodes, then fails (proves a failed
    /// generation writes and tombstones nothing).
    #[cfg(test)]
    pub(crate) fail_replace_after: Option<usize>,
    /// Test failpoint: `replace_source` fails after its tombstones, before its manifest.
    #[cfg(test)]
    pub(crate) fail_replace_before_manifest: bool,
    /// Test failpoint: an approval stops after its journal and edges, before its node (a crash).
    #[cfg(test)]
    pub(crate) fail_apply_before_node: bool,
    /// Test failpoint: a linked `replace_source` stops after writing its new link rows, before
    /// the knowledge generation commits (a crash before the flip; DES-PM-001 §3.4).
    #[cfg(test)]
    pub(crate) fail_links_before_flip: bool,
    /// Test failpoint: a linked `replace_source` stops after the flip, before it deletes the old
    /// generation's link rows (a crash after the flip).
    #[cfg(test)]
    pub(crate) fail_links_after_flip: bool,
    /// Test failpoint: the cleanup after the flip fails (the generation stays committed).
    #[cfg(test)]
    pub(crate) fail_links_cleanup: bool,
}

fn default_embedder() -> Box<dyn Embedder> {
    #[cfg(feature = "semantic-bge")]
    {
        if let Ok(e) = wicked_estate_retrieve::FastEmbedder::new() {
            return Box::new(e);
        }
    }
    #[cfg(all(feature = "semantic", not(feature = "semantic-bge")))]
    {
        if let Ok(e) = wicked_estate_retrieve::Model2VecEmbedder::new() {
            return Box::new(e);
        }
    }
    Box::new(HashEmbedder::new(256))
}

impl KnowledgeEngine {
    /// Open an in-memory engine (tests / ephemeral).
    pub fn in_memory() -> Result<Self> {
        Ok(Self::with_store(SqliteStore::in_memory()?))
    }

    /// Open a durable engine at `path` — knowledge's OWN file (NOT the code/memory db).
    pub fn open(path: &str) -> Result<Self> {
        Ok(Self::with_store(SqliteStore::open(path)?))
    }

    fn with_store(store: SqliteStore) -> Self {
        Self {
            store,
            embedder: default_embedder(),
            k: 32,
            recall_floor: 0.0,
            misses: Vec::new(),
            xedge: None,
            attempts: std::collections::HashMap::new(),
            #[cfg(test)]
            fail_replace_after: None,
            #[cfg(test)]
            fail_replace_before_manifest: false,
            #[cfg(test)]
            fail_apply_before_node: false,
            #[cfg(test)]
            fail_links_before_flip: false,
            #[cfg(test)]
            fail_links_after_flip: false,
            #[cfg(test)]
            fail_links_cleanup: false,
        }
    }

    /// Set the recall-miss floor (a recall whose top score is below this logs a miss). Builder.
    pub fn with_recall_floor(mut self, floor: f64) -> Self {
        self.recall_floor = floor;
        self
    }

    /// Set a custom embedder (e.g. estate's `Model2VecEmbedder`/`FastEmbedder` for real semantic
    /// recall, vs the compile-time default). MUST be set before any `write`/`ingest`, since stored
    /// vectors are only comparable to query vectors from the SAME embedder. Builder (mirrors
    /// `MemoryEngine::with_embedder`).
    pub fn with_embedder(mut self, e: Box<dyn Embedder>) -> Self {
        self.embedder = e;
        self
    }

    /// Wire up the shared XedgeStore for cross-engine about-edge writes.
    pub fn with_xedge_store(mut self, xedge: std::sync::Arc<XedgeStore>) -> Self {
        self.xedge = Some(xedge);
        self
    }

    /// TRUNCATE-checkpoint the knowledge store's WAL — a one-line forwarder to
    /// `SqliteStore::checkpoint_truncate` (busy-tolerant, never blocking; a `busy` result just
    /// defers to a later call).
    pub fn checkpoint_truncate(&mut self) -> Result<wicked_estate_store::WalCheckpointStats> {
        self.store.checkpoint_truncate()
    }

    /// The engine's embedding of `text` (the vector `write` stores).
    pub(crate) fn embed(&self, text: &str) -> Vec<f32> {
        self.embedder.embed(text)
    }

    /// Write ONE knowledge node carrying `deps` and the untrusted flag (DES-KNOWLEDGE-DATA-001
    /// §4.3): the plane's write path for nodes derived from sources and repos.
    pub fn write_with_deps(
        &mut self,
        k: &KNode,
        deps: &[String],
        untrusted: bool,
    ) -> Result<SymbolId> {
        let sym = k.symbol();
        let mut node = k.to_node();
        crate::plane::stamp(&mut node, deps, untrusted);
        self.store.upsert_nodes(&[node])?;
        let vec = self.embedder.embed(&k.content);
        self.store.set_embedding(&sym, &vec)?;
        Ok(sym)
    }

    /// Write ONE knowledge node (the single-writer path). Node-before-edge ordering (G3) is the
    /// caller's contract for `relate`. Returns the node's stable symbol.
    pub fn write(&mut self, k: &KNode) -> Result<SymbolId> {
        let sym = k.symbol();
        self.store.upsert_nodes(&[k.to_node()])?;
        let vec = self.embedder.embed(&k.content);
        self.store.set_embedding(&sym, &vec)?;
        Ok(sym)
    }

    /// Ingest a document: write a `kdoc` node + one `kchunk` per non-empty chunk, each `derived_from`
    /// the doc. (Real chunking/sectioning is the `knowledge-ingest` skill's job — the engine provides
    /// the deterministic split + write path.) Returns (doc symbol, chunk symbols).
    pub fn ingest(
        &mut self,
        title: &str,
        chunks: &[String],
        scope: &str,
        source: &str,
        now: i64,
    ) -> Result<(SymbolId, Vec<SymbolId>)> {
        let doc = KNode::new(KClass::Doc, title, scope, source, now);
        let doc_sym = self.write(&doc)?;
        let mut chunk_syms = Vec::new();
        for chunk in chunks.iter().filter(|c| !c.trim().is_empty()) {
            let kn = KNode::new(KClass::Chunk, chunk.clone(), scope, source, now);
            let sym = self.write(&kn)?;
            self.store
                .upsert_edges(&[edge(&sym, &doc_sym, "derived_from")])?;
            chunk_syms.push(sym);
        }
        Ok((doc_sym, chunk_syms))
    }

    /// Fetch a node by id.
    pub fn node(&self, id: &SymbolId) -> Result<Option<Node>> {
        self.store.get_node(id)
    }

    /// Count knowledge nodes (optionally of one class, optionally within a scope subtree).
    pub fn count(&self, class: Option<KClass>, scope_prefix: Option<&str>) -> Result<usize> {
        Ok(self.all_nodes(class, scope_prefix)?.len())
    }

    /// All knowledge nodes (optionally of one class, optionally within a scope subtree) decoded.
    ///
    /// The scope predicate (arch-R5) is applied on the HYDRATED metadata scope — the field every
    /// knowledge node has carried since ingest gained `scope` — not pushed into
    /// `SymbolQuery::scope_prefix`, because nodes written before 0.16 predate `Node.scope`
    /// stamping and a store-side predicate would silently exclude them from an unlimited full
    /// scan that has no top-k to isolate. Same subtree semantics either way
    /// (`Scope::path_in_prefix`).
    pub fn all_nodes(
        &self,
        class: Option<KClass>,
        scope_prefix: Option<&str>,
    ) -> Result<Vec<KNode>> {
        let kinds = match class {
            Some(c) => vec![NodeKind::Other(c.as_kind().into())],
            None => [KClass::Doc, KClass::Section, KClass::Chunk, KClass::Concept]
                .iter()
                .map(|c| NodeKind::Other(c.as_kind().into()))
                .collect(),
        };
        let nodes = self.store.find_symbols(&SymbolQuery {
            text: None,
            exact_name: None,
            kinds,
            language: None,
            limit: None,
            scope_prefix: None, // predicate applied below on the hydrated metadata scope
        })?;
        Ok(nodes
            .iter()
            .filter_map(KNode::from_node)
            .filter(|kn| match scope_prefix {
                Some(p) => scope_in_prefix(&kn.scope, p),
                None => true,
            })
            .collect())
    }

    /// Is `node` a recall candidate for this caller? A live knowledge node, inside the scope
    /// subtree (when one is asked for), whose every effective dep the caller can see
    /// (DES-KNOWLEDGE-DATA-001 §4.3). The ONE predicate every candidate list applies before its
    /// `k` cut, so a hidden or out-of-scope node never takes a candidate slot, an RRF rank or a
    /// share of the token budget.
    ///
    /// Reads the node's metadata in place (no content copy): it runs on every FTS match.
    fn recallable(&self, node: &Node, scope_prefix: Option<&str>, visible: &Visible) -> bool {
        let NodeKind::Other(k) = &node.kind else {
            return false;
        };
        if !KClass::is_knowledge_kind(k) {
            return false;
        }
        let m = &node.metadata;
        let scope = m
            .get(meta_keys::SCOPE)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some(prefix) = scope_prefix {
            if !scope_in_prefix(scope, prefix) {
                return false;
            }
        }
        // Stored deps: every one must be visible; a malformed entry is never visible.
        let stored_ok = match m.get(meta_keys::DEPS) {
            None => true,
            Some(v) => v.as_array().is_some_and(|xs| {
                xs.iter()
                    .all(|x| x.as_str().is_some_and(|d| visible.contains(d)))
            }),
        };
        stored_ok
            && scope_deps(scope).iter().all(|d| visible.contains(d))
            // An approval still in flight (its journal exists) is invisible until complete.
            && !self.applying(&node.symbol)
    }

    /// One BM25-ordered FTS candidate list, scoped to the given knowledge kinds. `text` is
    /// phrase-quoted by the store, so a multi-token string requires ADJACENT tokens.
    ///
    /// A canonical `scope_prefix` (arch-R5) is pushed into the store predicate, which filters
    /// BEFORE any truncation — so a scoped query's top-k is never crowded out by other scopes'
    /// BM25 hits (the `SymbolQuery::scope_prefix` isolation contract). A kind-wildcard prefix
    /// (trailing `:`, e.g. `"wiki:"`) is NOT pushed down (the store predicate is strict).
    ///
    /// The store returns every match in BM25 order (the SQLite FTS path hydrates all of them and
    /// truncates in Rust), so [`Self::recallable`] runs on that full ordered list and the `k` cut
    /// comes after it — the visibility filter costs no extra store work.
    fn fts_list(
        &self,
        text: &str,
        kinds: &[NodeKind],
        scope_prefix: Option<&str>,
        visible: &Visible,
    ) -> Result<Vec<SymbolId>> {
        Ok(self
            .store
            .find_symbols(&SymbolQuery {
                text: Some(text.to_string()),
                exact_name: None,
                kinds: kinds.to_vec(),
                language: None,
                limit: None,
                // Store-side pushdown only for canonical prefixes — the store predicate is the
                // strict segment-aware one and would match NOTHING for a kind-wildcard ("wiki:")
                // prefix; `recallable` applies the knowledge predicate to every row either way.
                scope_prefix: scope_prefix
                    .filter(|p| !p.ends_with(':'))
                    .map(str::to_string),
            })?
            .iter()
            .filter(|n| self.recallable(n, scope_prefix, visible))
            .take(self.k)
            .map(|n| n.symbol.clone())
            .collect())
    }

    /// FTS keyword candidates: whole-query PHRASE list ∪ per-term OR lists, RRF-fused. Scoped to
    /// knowledge chunk/section/concept nodes (mirrors memory's `keyword_candidates`).
    ///
    /// The phrase list is the identifier-shaped-query (symbolish) fix: FTS5's default unicode61
    /// tokenizer splits code identifiers on `_`/`.`/`::` at index time, so the raw query
    /// phrase-matches those sub-tokens ADJACENTLY (`prompt_submit.py` → `[prompt, submit, py]` in
    /// order) and the chunk containing the literal identifier outranks chunks that merely scatter
    /// its unigrams. The per-term lists alone lose that adjacency — measured on the S3 parity
    /// bench as estate's one outright class loss vs wicked-brain FTS (symbolish r@10 0.494 vs
    /// 0.786). Skipped when the query is already a single bare term (identical list, no signal).
    fn keyword_candidates(
        &self,
        query: &str,
        scope_prefix: Option<&str>,
        visible: &Visible,
    ) -> Result<Vec<SymbolId>> {
        let mut terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 2)
            .map(|w| w.to_lowercase())
            .collect();
        if terms.is_empty() {
            let q = query.trim().to_lowercase();
            if !q.is_empty() {
                terms.push(q);
            }
        }
        let recall_kinds: Vec<NodeKind> = [KClass::Chunk, KClass::Section, KClass::Concept]
            .iter()
            .map(|c| NodeKind::Other(c.as_kind().into()))
            .collect();
        let mut lists: Vec<Vec<SymbolId>> = Vec::new();
        let phrase = query.trim();
        let single_bare_term = terms.len() == 1 && terms[0] == phrase.to_lowercase();
        if !phrase.is_empty() && !single_bare_term {
            let l = self.fts_list(phrase, &recall_kinds, scope_prefix, visible)?;
            if !l.is_empty() {
                lists.push(l);
            }
        }
        for term in terms {
            let l = self.fts_list(&term, &recall_kinds, scope_prefix, visible)?;
            if !l.is_empty() {
                lists.push(l);
            }
        }
        Ok(rrf_fuse(&lists, 60.0)
            .into_iter()
            .take(self.k)
            .map(|(id, _)| id)
            .collect())
    }

    /// Standalone knowledge recall with no declared visibility: only dep-free knowledge (every
    /// node written before DES-KNOWLEDGE-DATA-001) is returned — see [`Self::recall_visible`].
    pub fn recall(
        &mut self,
        query: &str,
        token_budget: usize,
        scope_prefix: Option<&str>,
        now: i64,
    ) -> Result<Vec<KRecalled>> {
        Ok(self
            .recall_visible(query, token_budget, scope_prefix, &Visible::none(), now)?
            .into_iter()
            .map(|h| KRecalled {
                id: h.id,
                content: h.content,
                score: h.score,
                source: h.source,
            })
            .collect())
    }

    /// Knowledge recall. **REUSES** the `-core` pipeline: keyword (FTS OR-terms) ∪ vector (ANN),
    /// fused by `rrf_fuse`, packed by `budget_pack` — the SAME fusion/budget math memory uses
    /// (R3: no second `recall_impl`). Logs a miss when the top score falls at/below the floor.
    ///
    /// `visible` (DES-KNOWLEDGE-DATA-001 §4.3) is the set of `source:<id>` / `repo:<id>` the caller
    /// may see. A node is a candidate only when EVERY one of its effective deps is in it; a
    /// dep-free node always is. The check runs inside each candidate list, before its `k` cut,
    /// before RRF and before the budget is packed, so hidden knowledge can neither surface nor
    /// crowd visible knowledge out.
    ///
    /// `scope_prefix` (arch-R5, mirroring `memory.recall`'s wire contract): when present, only
    /// knowledge whose scope equals the prefix or descends from it is recalled (`""` = root
    /// subtree = everything; `None` = no scope filtering — the pre-0.16 behavior exactly). The
    /// predicate is pushed into the FTS candidate query (store-side, pre-truncation — top-k
    /// isolation for nodes written ≥ 0.16, which stamp `Node.scope`) and checked on every
    /// candidate's metadata scope — the authoritative filter, which also covers the vector (ANN)
    /// lane and legacy nodes whose scope predates `Node.scope` stamping (under a canonical-prefix
    /// scoped recall those can only surface via the vector lane — a kind-wildcard prefix skips the
    /// pushdown, so FTS still sees them; re-ingest to give them full scoped-FTS visibility).
    ///
    /// A prefix ending in `:` is a kind wildcard (`"wiki:"` = every `wiki:<area>` subtree) — see
    /// [`scope_in_prefix`]; such prefixes are filtered engine-side only (no store pushdown).
    ///
    /// Scoping convention (arch-R5): the architecture wiki ingests under `wiki:<area>` scopes
    /// (`wiki:architecture`, `wiki:event-grammar`, …) with `source` = a stable URI (repo-relative
    /// doc path or `wiki://<page>#<anchor>`), and chunks embed their enforceable twin's
    /// `PAT-`/`POL-` rule id in the chunk TEXT — so one recall surfaces the guidance, its citation,
    /// and the id `rules.recall` enforces deterministically.
    pub fn recall_visible(
        &mut self,
        query: &str,
        token_budget: usize,
        scope_prefix: Option<&str>,
        visible: &Visible,
        now: i64,
    ) -> Result<Vec<KHit>> {
        let kw = self.keyword_candidates(query, scope_prefix, visible)?;

        // Vector (ANN) candidates — only when the embedder carries real semantic signal. The
        // dependency-free HashEmbedder fallback is a lexical hash: fusing its neighbours as a
        // peer retriever injects rank noise that degrades every query class (S3 parity bench,
        // keyword-only vs hash-fused). Model-backed embedders (`semantic`/`semantic-bge`) keep
        // contributing exactly as before. `nearest` scores every stored vector; the sorted list is
        // walked until `k` recallable nodes are taken, so hidden neighbours cannot take the vector
        // lane's slots (the walk is bounded by the store, like the FTS lane's full match list).
        let mut sem = Vec::new();
        if self.embedder.is_semantic() {
            let qvec = self.embedder.embed(query);
            for (id, _) in <SqliteStore as VectorStore>::nearest(&self.store, &qvec, usize::MAX)? {
                if sem.len() >= self.k {
                    break;
                }
                if let Some(node) = self.store.get_node(&id)? {
                    if self.recallable(&node, scope_prefix, visible) {
                        sem.push(id);
                    }
                }
            }
        }

        // RRF fuse keyword ∪ vector (the -core primitive), then hydrate → budget pack (also -core).
        //
        // Document diversity: keep only the BEST-fused piece per parent document (`derived_from`
        // target; a node without one is its own group). `ingest` splits a document into pieces,
        // so without this a multi-piece document's runners-up crowd out other documents' best
        // slices inside the token budget — breadth loss, not relevance (measured: parity-bench
        // symbolish r@10 0.652 → 0.769 at a 6000-token budget, every other class unchanged).
        // Same move as brain search's `collapseDuplicates` on the surface this replaces; a caller
        // wanting depth on one document has its identity in `source`/`KRecalled.id` to fetch more.
        let fused = rrf_fuse(&[kw, sem], 60.0);
        let mut seen_docs = std::collections::BTreeSet::new();
        let mut cands = Vec::new();
        let mut hydrated: std::collections::HashMap<SymbolId, (KNode, Vec<String>, bool)> =
            std::collections::HashMap::new();
        for (id, rrf) in fused {
            let Some(node) = self.store.get_node(&id)? else {
                continue;
            };
            // The same predicate the lists applied (authoritative re-check on the hydrated node).
            if !self.recallable(&node, scope_prefix, visible) {
                continue;
            }
            let Some(kn) = KNode::from_node(&node) else {
                continue;
            };
            let doc_group = self
                .store
                .neighbors(&id, Direction::Dependencies)?
                .into_iter()
                .find(|e| matches!(&e.kind, EdgeKind::Other(r) if r == "derived_from"))
                .map(|e| e.target)
                .unwrap_or_else(|| id.clone());
            if !seen_docs.insert(doc_group) {
                continue;
            }
            // Knowledge is NOT tiered: neutral recency/salience so the RRF rank drives ordering.
            // Knowledge has no facets ⇒ specificity 0 (no boost; identical ranking to before).
            cands.push(Candidate {
                id: id.clone(),
                content: kn.content.clone(),
                tier: Tier::Semantic,
                rrf,
                recency: 1.0,
                salience: 1.0,
                facet_specificity: 0,
            });
            let deps = effective_deps(&node);
            let untrusted = is_untrusted(&node);
            hydrated.insert(id, (kn, deps, untrusted));
        }
        let packed = budget_pack(cands, token_budget, 0.5);

        let out: Vec<KHit> = packed
            .into_iter()
            .filter_map(|c| {
                let score = c.final_score(0.5);
                let (kn, deps, untrusted) = hydrated.remove(&c.id)?;
                Some(KHit {
                    deps,
                    untrusted,
                    id: c.id,
                    content: c.content,
                    score,
                    source: kn.source,
                    class: kn.class,
                    scope: kn.scope,
                })
            })
            .collect();

        let top = out.first().map(|r| r.score).unwrap_or(0.0);
        if out.is_empty() || top <= self.recall_floor {
            self.misses.push(RecallMiss {
                query: query.to_string(),
                scope: scope_prefix.unwrap_or_default().to_string(),
                ts: now,
                result_count: out.len(),
                top_score: top,
            });
        }
        Ok(out)
    }

    /// Logged recall misses (read by `knowledge.coverage`; persisted form lands at T-B-MISSLOG).
    pub fn misses(&self) -> &[RecallMiss] {
        &self.misses
    }
}

/// The `derived_from` edge a piece writes to its document (the recall diversity group).
pub(crate) fn derived_from_edge(src: &SymbolId, tgt: &SymbolId) -> Edge {
    edge(src, tgt, "derived_from")
}

/// One typed `Other(<rel>)` edge with confidence + provenance riding free (DEC-2 — even `governs` is
/// `Other("governs")`, NEVER `EdgeKind::Governs`). Shared by `ingest` and `relate`.
fn edge(src: &SymbolId, tgt: &SymbolId, rel: &str) -> Edge {
    Edge::new(
        src.clone(),
        tgt.clone(),
        EdgeKind::Other(rel.into()),
        ResolutionTier::Heuristic,
        "wicked-knowledge",
    )
}

impl KnowledgeEngine {
    /// `relate` (T-B-RELATE same-store half, D1.2/D1.3/DEC-2): upsert exactly ONE typed
    /// `Other(<rel>)` edge, **verifying BOTH endpoints have a live node first** (same-store
    /// `get_node`). A dangling target returns `Err` (the caller maps it to `isError:true`) rather than
    /// silently persisting a dangling-but-traversable edge (the grounded `upsert_edges` failure mode).
    /// The cross-store endpoint check (overlay foreign-existence) folds in once Lane X `T-X-OVL` lands.
    pub fn relate(
        &mut self,
        src: &SymbolId,
        tgt: &SymbolId,
        rel: &str,
        conf: f64,
        evidence_count: u32,
        prov: &str,
    ) -> Result<()> {
        // node-before-edge (G3): both endpoints MUST resolve to a live node, else this is a dangling
        // edge that hydrates to nothing.
        if self.store.get_node(src)?.is_none() {
            return Err(dangling_err("source", src));
        }
        if self.store.get_node(tgt)?.is_none() {
            return Err(dangling_err("target", tgt));
        }
        let mut e = Edge::new(
            src.clone(),
            tgt.clone(),
            // DEC-2: ALWAYS Other(<rel>), never a built-in EdgeKind (even for "governs").
            EdgeKind::Other(rel.to_string()),
            ResolutionTier::Heuristic,
            prov,
        );
        // confidence + evidence_count (brain consolidation) both ride free on the edge as first-class
        // fields; every backend round-trips them, and SqliteStore also promotes evidence_count to a
        // queryable column.
        e.confidence = Confidence::new(conf as f32);
        e.evidence_count = evidence_count;
        self.store.upsert_edges(&[e])?;
        Ok(())
    }

    /// Typed out-edges of a node (used to verify `relate` wrote a typed edge).
    pub fn out_edges(&self, id: &SymbolId) -> Result<Vec<Edge>> {
        self.store.neighbors(id, Direction::Dependencies)
    }
}

fn dangling_err(which: &str, id: &SymbolId) -> wicked_estate_core::Error {
    wicked_estate_core::Error::Invalid(format!(
        "relate: {which} {id:?} has no live node — refusing to write a dangling edge"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_then_recall_round_trips() {
        // T-B-KMCP behavioral gate (M2's new-crate rule): ingest a doc, recall a chunk back — NOT a
        // test that re-states the tool count.
        let mut e = KnowledgeEngine::in_memory().unwrap();
        let (_doc, chunks) = e
            .ingest(
                "Billing design",
                &[
                    "The billing service charges customers via Stripe webhooks.".into(),
                    "Refunds are processed asynchronously through a queue.".into(),
                ],
                "project:pay",
                "docs/billing.md",
                1,
            )
            .unwrap();
        assert_eq!(chunks.len(), 2, "two non-empty chunks written");
        let hits = e
            .recall("how does billing charge customers", 2000, None, 2)
            .unwrap();
        assert!(
            hits.iter().any(|h| h.content.contains("Stripe")),
            "recall must surface the ingested Stripe chunk, got: {:?}",
            hits.iter().map(|h| &h.content).collect::<Vec<_>>()
        );
    }

    /// WAL checkpointing (perf #5): the engine forwards `checkpoint_truncate` to its own
    /// single-writer store — the `-wal` file must end up empty and recall must survive it.
    #[test]
    fn checkpoint_truncate_forwards_to_store_and_recall_survives() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.db");
        let mut e = KnowledgeEngine::open(path.to_str().unwrap()).unwrap();
        e.ingest(
            "WAL design",
            &["The write-ahead log must not outgrow the database it protects.".into()],
            "wiki:architecture",
            "wiki://architecture#wal",
            1,
        )
        .unwrap();

        let stats = e.checkpoint_truncate().unwrap();
        assert!(!stats.busy, "no concurrent reader ⇒ the truncate completes");
        let wal = dir.path().join("knowledge.db-wal");
        assert_eq!(
            std::fs::metadata(&wal).expect("wal file kept").len(),
            0,
            "TRUNCATE must leave a zero-byte -wal"
        );
        let hits = e.recall("write-ahead log outgrow", 2000, None, 2).unwrap();
        assert!(
            hits.iter().any(|h| h.content.contains("outgrow")),
            "recall must survive a TRUNCATE checkpoint"
        );
    }

    /// arch-R5: `scope_prefix` restricts recall to a scope subtree — the `wiki:<area>` convention.
    /// The same lexical query matches chunks in BOTH scopes; only the prefix separates them.
    #[test]
    fn recall_scope_prefix_restricts_to_subtree() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.ingest(
            "Architecture wiki: planes",
            &["The estate graph is the center of gravity for architecture guidance.".into()],
            "wiki:architecture",
            "wiki://architecture#planes",
            1,
        )
        .unwrap();
        e.ingest(
            "Session note",
            &["Random note that also mentions architecture guidance loosely.".into()],
            "project:pay",
            "notes/session.md",
            1,
        )
        .unwrap();

        // Scoped to the wiki subtree: only the wiki chunk may surface.
        let wiki_hits = e
            .recall("architecture guidance", 2000, Some("wiki:architecture"), 2)
            .unwrap();
        assert!(
            !wiki_hits.is_empty(),
            "scoped recall must surface the wiki chunk"
        );
        assert!(
            wiki_hits
                .iter()
                .all(|h| h.source == "wiki://architecture#planes"),
            "scoped recall must return ONLY wiki-scoped knowledge; got sources {:?}",
            wiki_hits.iter().map(|h| &h.source).collect::<Vec<_>>()
        );

        // Kind wildcard (the AW-8 acceptance form): "wiki:" spans every wiki:<area> subtree.
        let kind_hits = e
            .recall("architecture guidance", 2000, Some("wiki:"), 2)
            .unwrap();
        assert!(
            !kind_hits.is_empty()
                && kind_hits
                    .iter()
                    .all(|h| h.source == "wiki://architecture#planes"),
            "scope_prefix \"wiki:\" must return wiki-only guidance with citations; got {:?}",
            kind_hits.iter().map(|h| &h.source).collect::<Vec<_>>()
        );

        // Sibling subtree: nothing (segment-aware — "wiki:arch" must not match "wiki:architecture").
        let sibling_hits = e
            .recall("architecture guidance", 2000, Some("wiki:arch"), 2)
            .unwrap();
        assert!(
            sibling_hits.is_empty(),
            "a mid-segment prefix must match nothing; got {:?}",
            sibling_hits.iter().map(|h| &h.source).collect::<Vec<_>>()
        );

        // "" = root subtree = everything; None = no filtering (both scopes visible either way).
        for scope in [Some(""), None] {
            let all_hits = e.recall("architecture guidance", 2000, scope, 2).unwrap();
            let sources: Vec<&String> = all_hits.iter().map(|h| &h.source).collect();
            assert!(
                sources.iter().any(|s| *s == "wiki://architecture#planes")
                    && sources.iter().any(|s| *s == "notes/session.md"),
                "scope_prefix={scope:?} must see both scopes; got {sources:?}"
            );
        }
    }

    /// arch-R5: scoped `count` (the knowledge.coverage path) uses the same subtree predicate.
    #[test]
    fn count_scope_prefix_restricts_to_subtree() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.ingest(
            "Wiki doc",
            &["wiki chunk".into()],
            "wiki:estate",
            "wiki://estate",
            1,
        )
        .unwrap();
        e.ingest(
            "Other doc",
            &["other chunk".into()],
            "project:pay",
            "docs/other.md",
            1,
        )
        .unwrap();
        // Each ingest wrote 1 kdoc + 1 kchunk.
        assert_eq!(e.count(None, None).unwrap(), 4, "unscoped = everything");
        assert_eq!(
            e.count(None, Some("wiki:estate")).unwrap(),
            2,
            "scoped count must see only the wiki subtree"
        );
        assert_eq!(
            e.count(Some(KClass::Chunk), Some("wiki:estate")).unwrap(),
            1,
            "class + scope compose"
        );
        assert_eq!(e.count(None, Some("")).unwrap(), 4, "\"\" = root subtree");
    }

    #[test]
    fn recall_surfaces_source_on_wire() {
        // S4 gate: KRecalled.source must be non-empty for a known-source document and must survive
        // the full recall pipeline. This is the falsifier for the drop-source regression.
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.ingest(
            "Payment design",
            &["Stripe webhooks trigger charge events.".into()],
            "project:pay",
            "docs/payment.md",
            1,
        )
        .unwrap();
        // Query tokens overlap the chunk lexically: with the hash-fallback embedder the vector
        // list is gated (is_semantic=false), so keyword candidates must carry the hit themselves
        // (the old "how does payment work" phrasing only ever matched via hash-vector noise).
        let hits = e.recall("stripe charge events", 2000, None, 2).unwrap();
        assert!(!hits.is_empty(), "recall must return at least one hit");
        let hit = &hits[0];
        assert!(
            !hit.source.is_empty(),
            "KRecalled.source must be non-empty for a known-source document; got empty"
        );
        assert_eq!(
            hit.source, "docs/payment.md",
            "KRecalled.source must match the ingested source"
        );
    }

    // Symbolish parity (the S3 bench's one outright estate loss): an identifier-shaped query must
    // rank the chunk containing the LITERAL identifier above chunks that merely scatter its
    // unigrams. The whole-query phrase list in `keyword_candidates` is what wins this — the
    // per-term OR lists alone rank the decoys (more unigram hits) first.
    #[test]
    fn identifier_query_ranks_adjacent_phrase_above_scattered_unigrams() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        // Decoys: every unigram of the query, repeatedly, never adjacent.
        for i in 0..4 {
            e.write(&KNode::new(
                KClass::Chunk,
                format!(
                    "chunk {i}: the prompt asked users to submit feedback; \
                     submit the py bindings, then prompt again for py review"
                ),
                "s",
                format!("decoy-{i}.md"),
                1,
            ))
            .unwrap();
        }
        // Target: the literal identifier (tokenized [prompt, submit, py] ADJACENTLY by FTS5).
        e.write(&KNode::new(
            KClass::Chunk,
            "the prompt_submit.py hook flips the mandatory-pull flag on archetype match",
            "s",
            "hooks.md",
            1,
        ))
        .unwrap();

        let hits = e.recall("prompt_submit.py", 4000, None, 2).unwrap();
        assert!(!hits.is_empty(), "recall must return hits");
        assert!(
            hits[0].content.contains("prompt_submit.py"),
            "the chunk with the literal identifier must rank FIRST; got: {:?}",
            hits.iter().map(|h| &h.content).collect::<Vec<_>>()
        );
    }

    // Document diversity in recall packing: a multi-piece document contributes its BEST piece
    // only, so runners-up from the same document cannot crowd other documents' top slices out of
    // the token budget (breadth over same-document depth; the parity-bench q28 failure mode).
    #[test]
    fn recall_returns_at_most_one_piece_per_document() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        // Doc A: three pieces, all matching the query strongly.
        e.ingest(
            "gate policy",
            &[
                "the gate policy matrix resolves reviewers per gate".into(),
                "gate policy fallback: the gate policy names a fallback reviewer".into(),
                "gate policy modes: sequential, parallel, council per gate policy".into(),
            ],
            "s",
            "gate-policy.md",
            1,
        )
        .unwrap();
        // Doc B: one piece, also matching — must NOT be crowded out by A's runners-up.
        e.ingest(
            "adjudicator",
            &["the adjudicator applies the gate policy verdict".into()],
            "s",
            "adjudicator.md",
            1,
        )
        .unwrap();

        let hits = e.recall("gate policy", 6000, None, 2).unwrap();
        let from_a = hits.iter().filter(|h| h.source == "gate-policy.md").count();
        assert_eq!(
            from_a,
            1,
            "exactly ONE piece of the multi-piece document, got {from_a}: {:?}",
            hits.iter()
                .map(|h| (&h.source, &h.content))
                .collect::<Vec<_>>()
        );
        assert!(
            hits.iter().any(|h| h.source == "adjudicator.md"),
            "the other document's slice must surface, got: {:?}",
            hits.iter().map(|h| &h.source).collect::<Vec<_>>()
        );
    }

    // The vector-list gate: a NON-semantic embedder (the hash fallback) must not surface
    // zero-lexical-overlap docs through vector fusion; a semantic embedder must keep doing so.
    #[test]
    fn non_semantic_embedder_contributes_no_vector_candidates() {
        // A constant-vector embedder makes EVERY doc a nearest neighbour of every query, so any
        // vector contribution is fully observable. `semantic` flips only the self-identification.
        struct ConstEmbedder {
            semantic: bool,
        }
        impl Embedder for ConstEmbedder {
            fn id(&self) -> &str {
                "const:test"
            }
            fn embed(&self, _text: &str) -> Vec<f32> {
                vec![1.0, 0.0]
            }
            fn dim(&self) -> usize {
                2
            }
            fn is_semantic(&self) -> bool {
                self.semantic
            }
        }

        let run = |semantic: bool| {
            let mut e = KnowledgeEngine::in_memory()
                .unwrap()
                .with_embedder(Box::new(ConstEmbedder { semantic }));
            e.write(&KNode::new(KClass::Chunk, "zebra quagga", "s", "z.md", 1))
                .unwrap();
            // Query shares NO tokens with the doc — only the vector path can surface it.
            e.recall("unrelated words", 2000, None, 2).unwrap()
        };

        assert!(
            !run(true).is_empty(),
            "semantic embedder: vector path must surface the zero-overlap doc"
        );
        assert!(
            run(false).is_empty(),
            "non-semantic embedder: vector noise must be gated out of recall"
        );
    }

    #[test]
    fn separate_store_means_own_fts() {
        // DEC-1 / B3: the knowledge store is a distinct SqliteStore — code/memory nodes never appear
        // here. all_nodes only ever returns k* nodes (no dilution is even possible).
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.ingest("D", &["one chunk".into()], "s", "src", 1).unwrap();
        let all = e.all_nodes(None, None).unwrap();
        assert!(
            all.iter()
                .all(|n| matches!(n.class, KClass::Doc | KClass::Chunk))
        );
        assert_eq!(e.count(Some(KClass::Doc), None).unwrap(), 1);
        assert_eq!(e.count(Some(KClass::Chunk), None).unwrap(), 1);
    }

    #[test]
    fn relate_persists_confidence_and_evidence_count() {
        // Brain consolidation: relate must land BOTH tuned signals on the knowledge relation —
        // confidence (already carried) AND evidence_count (the new first-class Edge field).
        let mut e = KnowledgeEngine::in_memory().unwrap();
        let a = e
            .write(&KNode::new(KClass::Concept, "concept a", "", "", 1))
            .unwrap();
        let b = e
            .write(&KNode::new(KClass::Concept, "concept b", "", "", 1))
            .unwrap();
        e.relate(&a, &b, "governs", 0.9, 4, "test").unwrap();

        let edges = e.out_edges(&a).unwrap();
        let gov = edges
            .iter()
            .find(|ed| matches!(&ed.kind, EdgeKind::Other(r) if r == "governs"))
            .expect("the typed governs edge must exist");
        assert_eq!(
            gov.evidence_count, 4,
            "relate must persist evidence_count on the relation"
        );
        assert!(
            (gov.confidence.get() - 0.9).abs() < 1e-6,
            "relate must persist confidence on the relation"
        );
    }

    #[test]
    fn relate_update_lands_contradicted_signal_but_not_stale_writes() {
        // Brain consolidation: a contradicted link legitimately DROPS confidence while GROWING
        // evidence_count (both confirm and contradict bump the audit counter). The store's
        // higher-confidence-wins collision rule (W3.4) must not eat that update — evidence
        // growth wins. A write with NO evidence growth and lower confidence is a stale/duplicate
        // signal and must still lose.
        let mut e = KnowledgeEngine::in_memory().unwrap();
        let a = e
            .write(&KNode::new(KClass::Concept, "concept a", "", "", 1))
            .unwrap();
        let b = e
            .write(&KNode::new(KClass::Concept, "concept b", "", "", 1))
            .unwrap();
        e.relate(&a, &b, "governs", 0.9, 4, "test").unwrap();

        // Contradiction: confidence 0.9 → 0.7, evidence 4 → 5. Must land.
        e.relate(&a, &b, "governs", 0.7, 5, "test").unwrap();
        let gov = |e: &KnowledgeEngine| {
            e.out_edges(&a)
                .unwrap()
                .into_iter()
                .find(|ed| matches!(&ed.kind, EdgeKind::Other(r) if r == "governs"))
                .expect("the typed governs edge must exist")
        };
        let g = gov(&e);
        assert_eq!(g.evidence_count, 5, "evidence growth must win the upsert");
        assert!(
            (g.confidence.get() - 0.7).abs() < 1e-6,
            "the contradicted (lower) confidence must land alongside the evidence growth"
        );

        // Stale write: lower confidence, NO evidence growth. Must be ignored (W3.4).
        e.relate(&a, &b, "governs", 0.5, 5, "test").unwrap();
        let g = gov(&e);
        assert_eq!(g.evidence_count, 5);
        assert!(
            (g.confidence.get() - 0.7).abs() < 1e-6,
            "a same-evidence lower-confidence write is stale and must lose"
        );
    }

    #[test]
    fn knode_roundtrips_to_node() {
        let kn = KNode::new(
            KClass::Chunk,
            "a chunk of knowledge",
            "project:p",
            "src.md",
            7,
        );
        let node = kn.to_node();
        assert!(matches!(node.kind, NodeKind::Other(ref k) if k == "kchunk"));
        let back = KNode::from_node(&node).expect("roundtrip");
        assert_eq!(back.id, kn.id);
        assert_eq!(back.class, KClass::Chunk);
        assert_eq!(back.content, "a chunk of knowledge");
        assert_eq!(back.source, "src.md");
        assert_eq!(back.created_at, 7);
    }

    fn write_dep(e: &mut KnowledgeEngine, content: &str, scope: &str, deps: &[&str]) {
        let kn = KNode::new(KClass::Chunk, content, scope, format!("{content}.md"), 1);
        let deps: Vec<String> = deps.iter().map(|d| d.to_string()).collect();
        e.write_with_deps(&kn, &deps, false).unwrap();
    }

    /// NEGATIVE CONTROL (DES-KNOWLEDGE-DATA-001 §19 B3a): a hidden dep removes an item BEFORE the
    /// budget is packed. The hidden item ranks first and the budget fits one item, so a filter that
    /// ran after packing would return nothing; filtering before ranking returns the visible one.
    #[test]
    fn hidden_dep_is_removed_before_the_budget_is_packed() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        // Same length (same token cost); the hidden one holds every query term twice.
        let strong = "open tickets per region open tickets per region";
        let weak = "open tickets per region and then some padding..";
        assert_eq!(strong.len(), weak.len());
        write_dep(&mut e, strong, "", &["source:hidden"]);
        write_dep(&mut e, weak, "", &["source:seen"]);
        // One item's worth of tokens (each costs len/4 = 11).
        let budget = 15;
        let all = Visible::from_deps(["source:hidden", "source:seen"]).unwrap();
        let both = e
            .recall_visible("open tickets per region", budget, None, &all, 2)
            .unwrap();
        assert_eq!(both.len(), 1, "the budget fits exactly one item: {both:?}");
        assert!(
            both[0].deps.contains(&"source:hidden".to_string()),
            "with both visible, the hidden-dep item ranks first: {both:?}"
        );
        let seen = Visible::from_deps(["source:seen"]).unwrap();
        let hits = e
            .recall_visible("open tickets per region", budget, None, &seen, 2)
            .unwrap();
        assert_eq!(
            hits.iter().map(|h| h.content.as_str()).collect::<Vec<_>>(),
            vec![weak],
            "the hidden item must leave before packing, so the visible one takes the budget"
        );
        assert_eq!(hits[0].deps, vec!["source:seen".to_string()]);
    }

    /// Hidden candidates never take the FTS list's `k` slots: 40 hidden chunks outrank one visible
    /// chunk on BM25, and the visible chunk is still recalled.
    #[test]
    fn hidden_items_cannot_crowd_the_candidate_list() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        for i in 0..40 {
            write_dep(
                &mut e,
                &format!("escalation escalation escalation note {i}"),
                "",
                &["source:hidden"],
            );
        }
        write_dep(&mut e, "one escalation policy", "", &[]);
        let hits = e
            .recall_visible("escalation", 4000, None, &Visible::none(), 2)
            .unwrap();
        assert_eq!(
            hits.iter().map(|h| h.content.as_str()).collect::<Vec<_>>(),
            vec!["one escalation policy"]
        );
    }

    /// Omitted visibility = the empty set: dep-free knowledge is unchanged, a source scope is
    /// hidden even without stored deps, and every item carries scope, deps, untrusted and class.
    #[test]
    fn items_carry_scope_deps_untrusted_and_source_scopes_gate() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.write(&KNode::new(
            KClass::Concept,
            "refund approvals need a manager",
            "wiki:apps",
            "wiki://apps#refunds",
            1,
        ))
        .unwrap();
        // Written straight into a source scope, with no stored deps: the scope still gates it.
        e.write(&KNode::new(
            KClass::Section,
            "support.refunds: id int8, approved_by text",
            "source:s1",
            "source:s1/support.refunds",
            1,
        ))
        .unwrap();
        let plain = e
            .recall_visible("refunds approvals", 4000, None, &Visible::none(), 2)
            .unwrap();
        assert_eq!(
            plain.len(),
            1,
            "only dep-free knowledge without visible: {plain:?}"
        );
        assert_eq!(plain[0].scope, "wiki:apps");
        assert!(plain[0].deps.is_empty() && !plain[0].untrusted);
        assert_eq!(plain[0].class, KClass::Concept);
        let v = Visible::from_deps(["source:s1"]).unwrap();
        let hits = e
            .recall_visible("refunds approvals", 4000, None, &v, 2)
            .unwrap();
        let src = hits
            .iter()
            .find(|h| h.scope == "source:s1")
            .expect("the source section is visible with its dep");
        assert_eq!(src.deps, vec!["source:s1".to_string()]);
        assert!(src.untrusted, "source structure is untrusted");
        assert_eq!(src.class, KClass::Section);
    }

    /// A malformed stored dep is never visible (fail-closed), on the recall path and on the
    /// hydrated `KNode` every other read path uses.
    #[test]
    fn a_malformed_stored_dep_hides_the_node() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        let kn = KNode::new(KClass::Chunk, "malformed dep chunk", "", "m.md", 1);
        let mut node = kn.to_node();
        node.metadata
            .insert(meta_keys::DEPS.into(), serde_json::json!([42]));
        e.store.upsert_nodes(&[node.clone()]).unwrap();
        assert!(
            e.recall("malformed dep chunk", 4000, None, 2)
                .unwrap()
                .is_empty()
        );
        assert!(effective_deps(&node).contains(&"invalid:42".to_string()));
        assert!(!Visible::none().allows(&effective_deps(&node)));
    }
}
