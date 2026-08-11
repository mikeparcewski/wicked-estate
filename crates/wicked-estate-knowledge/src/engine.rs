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
    NodeKind, ResolutionTier, Result, Span, Symbol, SymbolId, SymbolQuery,
};
use wicked_estate_memory_core::{Candidate, Tier, budget_pack, rrf_fuse};
use wicked_estate_overlay::XedgeStore;
use wicked_estate_retrieve::{Embedder, HashEmbedder, VectorStore};
use wicked_estate_store::SqliteStore;

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
    pub fn to_node(&self) -> Node {
        let mut node = Node::new(
            self.symbol(),
            NodeKind::Other(self.class.as_kind().to_string()),
            self.content.clone(),
            Language::new("knowledge"),
            Location::new("knowledge", Span::ZERO),
        );
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

/// One recall-miss sidecar row (T-B-MISSLOG seeds this; the foundation scaffolds the shape).
#[derive(Debug, Clone)]
pub struct RecallMiss {
    pub query: String,
    pub scope: String,
    pub ts: i64,
    pub result_count: usize,
    pub top_score: f64,
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
        }
    }

    /// Set the recall-miss floor (a recall whose top score is below this logs a miss). Builder.
    pub fn with_recall_floor(mut self, floor: f64) -> Self {
        self.recall_floor = floor;
        self
    }

    /// Wire up the shared XedgeStore for cross-engine about-edge writes.
    pub fn with_xedge_store(mut self, xedge: std::sync::Arc<XedgeStore>) -> Self {
        self.xedge = Some(xedge);
        self
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

    /// Count knowledge nodes (optionally of one class).
    pub fn count(&self, class: Option<KClass>) -> Result<usize> {
        Ok(self.all_nodes(class)?.len())
    }

    /// All knowledge nodes (optionally of one class) decoded.
    pub fn all_nodes(&self, class: Option<KClass>) -> Result<Vec<KNode>> {
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
            scope_prefix: None,
        })?;
        Ok(nodes.iter().filter_map(KNode::from_node).collect())
    }

    /// Per-term FTS keyword candidates with OR semantics (mirrors memory's `keyword_candidates`:
    /// estate FTS phrase-matches the whole `text`, so multi-word queries need per-term union). Scoped
    /// to knowledge chunk/section/concept nodes.
    fn keyword_candidates(&self, query: &str) -> Result<Vec<SymbolId>> {
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
        for term in terms {
            let l: Vec<SymbolId> = self
                .store
                .find_symbols(&SymbolQuery {
                    text: Some(term),
                    exact_name: None,
                    kinds: recall_kinds.clone(),
                    language: None,
                    limit: Some(self.k),
                    scope_prefix: None,
                })?
                .iter()
                .map(|n| n.symbol.clone())
                .collect();
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

    /// Standalone knowledge recall. **REUSES** the `-core` pipeline: keyword (FTS OR-terms) ∪ vector
    /// (ANN), fused by `rrf_fuse`, packed by `budget_pack` — the SAME fusion/budget math memory uses
    /// (R3: no second `recall_impl`). Logs a miss when the top score falls at/below the floor.
    pub fn recall(&mut self, query: &str, token_budget: usize, now: i64) -> Result<Vec<KRecalled>> {
        let kw = self.keyword_candidates(query)?;

        let qvec = self.embedder.embed(query);
        let mut sem = Vec::new();
        for (id, _) in <SqliteStore as VectorStore>::nearest(&self.store, &qvec, self.k)? {
            if let Some(node) = self.store.get_node(&id)? {
                if let NodeKind::Other(k) = &node.kind {
                    if KClass::is_knowledge_kind(k) {
                        sem.push(id);
                    }
                }
            }
        }

        // RRF fuse keyword ∪ vector (the -core primitive), then hydrate → budget pack (also -core).
        let fused = rrf_fuse(&[kw, sem], 60.0);
        let mut cands = Vec::new();
        for (id, rrf) in fused {
            let Some(node) = self.store.get_node(&id)? else {
                continue;
            };
            let Some(kn) = KNode::from_node(&node) else {
                continue;
            };
            // Knowledge is NOT tiered: neutral recency/salience so the RRF rank drives ordering.
            cands.push(Candidate {
                id,
                content: kn.content,
                tier: Tier::Semantic,
                rrf,
                recency: 1.0,
                salience: 1.0,
            });
        }
        let packed = budget_pack(cands, token_budget, 0.5);

        let out: Vec<KRecalled> = packed
            .into_iter()
            .map(|c| {
                let score = c.final_score(0.5);
                let source = self
                    .node(&c.id)
                    .ok()
                    .flatten()
                    .and_then(|n| KNode::from_node(&n))
                    .map(|kn| kn.source)
                    .unwrap_or_default();
                KRecalled {
                    id: c.id,
                    content: c.content,
                    score,
                    source,
                }
            })
            .collect();

        let top = out.first().map(|r| r.score).unwrap_or(0.0);
        if out.is_empty() || top <= self.recall_floor {
            self.misses.push(RecallMiss {
                query: query.to_string(),
                scope: String::new(),
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
        // confidence rides free on the edge; evidence_count (brain consolidation) rides the metadata
        // slot and is promoted to the edges.evidence_count column by SqliteStore.
        e.confidence = Confidence::new(conf as f32);
        e = e.with_evidence_count(evidence_count);
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
            .recall("how does billing charge customers", 2000, 2)
            .unwrap();
        assert!(
            hits.iter().any(|h| h.content.contains("Stripe")),
            "recall must surface the ingested Stripe chunk, got: {:?}",
            hits.iter().map(|h| &h.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn separate_store_means_own_fts() {
        // DEC-1 / B3: the knowledge store is a distinct SqliteStore — code/memory nodes never appear
        // here. all_nodes only ever returns k* nodes (no dilution is even possible).
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.ingest("D", &["one chunk".into()], "s", "src", 1).unwrap();
        let all = e.all_nodes(None).unwrap();
        assert!(
            all.iter()
                .all(|n| matches!(n.class, KClass::Doc | KClass::Chunk))
        );
        assert_eq!(e.count(Some(KClass::Doc)).unwrap(), 1);
        assert_eq!(e.count(Some(KClass::Chunk)).unwrap(), 1);
    }

    #[test]
    fn relate_persists_confidence_and_evidence_count() {
        // Brain consolidation: relate must land BOTH tuned signals on the knowledge relation —
        // confidence (already carried) AND evidence_count (the new metadata-carried audit counter).
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
            gov.evidence_count(),
            4,
            "relate must persist evidence_count on the relation"
        );
        assert!(
            (gov.confidence.get() - 0.9).abs() < 1e-6,
            "relate must persist confidence on the relation"
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
}
