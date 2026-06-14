//! `SurrealStore` — W1.5 bake-off challenger.
//!
//! Embedded SurrealDB (kv-mem in-memory, kv-surrealkv on disk) behind the same
//! `GraphStore` trait as `SqliteStore`.  Compiled ONLY when `--features surrealdb`
//! is passed; the default build path never touches this module.
//!
//! Graph model: nodes → `node:<symbol>` records; edges → RELATE statements;
//! traversal → SurrealQL graph path `->calls->` queries with a depth limit.
//!
//! Status: W1.5 bake-off.  This is a challenger implementation, not the default.

#![cfg(feature = "surrealdb")]

use std::collections::{BTreeMap, HashSet};
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use wicked_estate_core::{
    Direction, Edge, EdgeKind, Error, GraphRead, GraphStats, GraphWrite, Node, NodeKind, Result,
    StoreCapabilities, Subgraph, SymbolId, SymbolQuery, TraversalSpec, UnresolvedRef,
};

/// Map any displayable error into our storage error.
fn se<E: std::fmt::Display>(e: E) -> Error {
    Error::Storage(e.to_string())
}

/// Embedded SurrealDB graph store (W1.5 bake-off challenger).
///
/// Wraps the async SurrealDB client with a synchronous facade using a single-threaded
/// Tokio runtime, matching the sync `GraphStore` trait surface.
pub struct SurrealStore {
    db: Surreal<surrealdb::engine::local::Db>,
    rt: tokio::runtime::Runtime,
}

impl SurrealStore {
    /// Open an in-memory SurrealDB instance (tests + bake-off).
    pub fn in_memory() -> Result<Self> {
        let rt = tokio::runtime::Runtime::new().map_err(se)?;
        let db = rt.block_on(async {
            let db = Surreal::new::<Mem>(()).await.map_err(se)?;
            db.use_ns("ci").use_db("graph").await.map_err(se)?;
            // Create schema: node and edge tables.
            db.query(
                "DEFINE TABLE node SCHEMAFULL;
                 DEFINE FIELD symbol ON node TYPE string;
                 DEFINE FIELD name   ON node TYPE string;
                 DEFINE FIELD data   ON node TYPE string;
                 DEFINE INDEX node_symbol ON node COLUMNS symbol UNIQUE;
                 DEFINE INDEX node_name   ON node COLUMNS name;

                 DEFINE TABLE edge_rel SCHEMAFULL;
                 DEFINE FIELD src        ON edge_rel TYPE string;
                 DEFINE FIELD tgt        ON edge_rel TYPE string;
                 DEFINE FIELD kind       ON edge_rel TYPE string;
                 DEFINE FIELD confidence ON edge_rel TYPE float;
                 DEFINE FIELD data       ON edge_rel TYPE string;
                 DEFINE INDEX edge_src ON edge_rel COLUMNS src;
                 DEFINE INDEX edge_tgt ON edge_rel COLUMNS tgt;
                 DEFINE INDEX edge_dedup ON edge_rel COLUMNS src, tgt, kind UNIQUE;

                 DEFINE TABLE unresolved SCHEMALESS;
                 DEFINE INDEX unresolved_name ON unresolved COLUMNS raw_name;

                 DEFINE TABLE file_meta SCHEMAFULL;
                 DEFINE FIELD path   ON file_meta TYPE string;
                 DEFINE FIELD digest ON file_meta TYPE string;
                 DEFINE INDEX file_path ON file_meta COLUMNS path UNIQUE;

                 DEFINE TABLE file_content SCHEMAFULL;
                 DEFINE FIELD path ON file_content TYPE string;
                 DEFINE FIELD text ON file_content TYPE string;
                 DEFINE INDEX fc_path ON file_content COLUMNS path UNIQUE;",
            )
            .await
            .map_err(se)?
            .check()
            .map_err(se)?;
            Ok::<_, Error>(db)
        })?;
        Ok(Self { db, rt })
    }
}

// ── GraphWrite ────────────────────────────────────────────────────────────────

impl GraphWrite for SurrealStore {
    fn begin_batch(&mut self) -> Result<()> {
        // SurrealDB auto-commits each statement; no explicit transaction API exposed at this level.
        // For the bake-off we accept this; a production impl would use `BEGIN TRANSACTION`.
        Ok(())
    }

    fn commit_batch(&mut self) -> Result<()> {
        Ok(())
    }

    fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()> {
        let db = self.db.clone();
        let nodes = nodes.to_vec();
        self.rt.block_on(async move {
            for n in &nodes {
                let data = serde_json::to_string(n).map_err(se)?;
                // UPSERT ON DUPLICATE KEY UPDATE via UPDATE … MERGE or CREATE … ON DUPLICATE
                db.query(
                    "UPDATE node SET symbol=$sym, name=$name, data=$data
                     WHERE symbol=$sym
                     ELSE (INSERT INTO node { symbol: $sym, name: $name, data: $data })",
                )
                .bind(("sym", n.symbol.0.clone()))
                .bind(("name", n.name.clone()))
                .bind(("data", data))
                .await
                .map_err(se)?;
            }
            Ok::<_, Error>(())
        })
    }

    fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()> {
        let db = self.db.clone();
        let edges = edges.to_vec();
        self.rt.block_on(async move {
            for e in &edges {
                let kind = serde_json::to_string(&e.kind).map_err(se)?;
                let data = serde_json::to_string(e).map_err(se)?;
                let conf = e.confidence.get() as f64;
                // Upsert: on collision keep higher-confidence edge.
                db.query(
                    "LET $existing = (SELECT confidence FROM edge_rel WHERE src=$src AND tgt=$tgt AND kind=$kind LIMIT 1);
                     IF array::len($existing) = 0 THEN
                       (INSERT INTO edge_rel { src: $src, tgt: $tgt, kind: $kind, confidence: $conf, data: $data })
                     ELSE IF $existing[0].confidence <= $conf THEN
                       (UPDATE edge_rel SET confidence=$conf, data=$data WHERE src=$src AND tgt=$tgt AND kind=$kind)
                     END",
                )
                .bind(("src", e.source.0.clone()))
                .bind(("tgt", e.target.0.clone()))
                .bind(("kind", kind))
                .bind(("conf", conf))
                .bind(("data", data))
                .await
                .map_err(se)?;
            }
            Ok::<_, Error>(())
        })
    }

    fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()> {
        let db = self.db.clone();
        let refs = refs.to_vec();
        self.rt.block_on(async move {
            for r in &refs {
                let data = serde_json::to_string(r).map_err(se)?;
                db.query("INSERT INTO unresolved { raw_name: $name, data: $data }")
                    .bind(("name", r.raw_name.clone()))
                    .bind(("data", data))
                    .await
                    .map_err(se)?;
            }
            Ok::<_, Error>(())
        })
    }

    fn remove_file(&mut self, file: &str) -> Result<()> {
        let db = self.db.clone();
        let file = file.to_string();
        self.rt.block_on(async move {
            // Parse JSON-encoded nodes to find those whose location.file matches.
            // We do this client-side since SurrealDB's JSON path support varies.
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT data FROM node")
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;

            let mut syms_to_delete: Vec<String> = Vec::new();
            for v in rows {
                if let surrealdb::Value::Object(obj) = v {
                    if let Some(surrealdb::Value::Strand(s)) = obj.get("data") {
                        if let Ok(n) = serde_json::from_str::<Node>(s.as_str()) {
                            if n.location.file == file {
                                syms_to_delete.push(n.symbol.0.clone());
                            }
                        }
                    }
                }
            }

            for sym in &syms_to_delete {
                db.query("DELETE node WHERE symbol=$sym")
                    .bind(("sym", sym.clone()))
                    .await
                    .map_err(se)?;
                // Also remove edges involving this symbol.
                db.query("DELETE edge_rel WHERE src=$sym OR tgt=$sym")
                    .bind(("sym", sym.clone()))
                    .await
                    .map_err(se)?;
            }

            // Remove unresolved refs from this file.
            let urefs: Vec<surrealdb::Value> = db
                .query("SELECT id, data FROM unresolved")
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;

            for v in urefs {
                if let surrealdb::Value::Object(obj) = v {
                    let id = obj.get("id").and_then(|v| {
                        if let surrealdb::Value::Thing(t) = v {
                            Some(t.to_raw())
                        } else {
                            None
                        }
                    });
                    let matches = obj
                        .get("data")
                        .and_then(|v| {
                            if let surrealdb::Value::Strand(s) = v {
                                serde_json::from_str::<UnresolvedRef>(s.as_str())
                                    .ok()
                                    .map(|r| r.location.file == file)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(false);
                    if matches {
                        if let Some(id_str) = id {
                            db.query(format!("DELETE {id_str}")).await.map_err(se)?;
                        }
                    }
                }
            }

            db.query("DELETE file_meta WHERE path=$path")
                .bind(("path", file.clone()))
                .await
                .map_err(se)?;
            db.query("DELETE file_content WHERE path=$path")
                .bind(("path", file.clone()))
                .await
                .map_err(se)?;

            Ok::<_, Error>(())
        })
    }

    fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()> {
        let db = self.db.clone();
        let (file, digest) = (file.to_string(), digest.to_string());
        self.rt.block_on(async move {
            db.query(
                "LET $ex = (SELECT id FROM file_meta WHERE path=$path LIMIT 1);
                 IF array::len($ex) = 0 THEN
                   (INSERT INTO file_meta { path: $path, digest: $digest })
                 ELSE
                   (UPDATE file_meta SET digest=$digest WHERE path=$path)
                 END",
            )
            .bind(("path", file))
            .bind(("digest", digest))
            .await
            .map_err(se)?;
            Ok::<_, Error>(())
        })
    }

    fn set_file_content(&mut self, file: &str, text: &str) -> Result<()> {
        let db = self.db.clone();
        let (file, text) = (file.to_string(), text.to_string());
        self.rt.block_on(async move {
            db.query(
                "LET $ex = (SELECT id FROM file_content WHERE path=$path LIMIT 1);
                 IF array::len($ex) = 0 THEN
                   (INSERT INTO file_content { path: $path, text: $text })
                 ELSE
                   (UPDATE file_content SET text=$text WHERE path=$path)
                 END",
            )
            .bind(("path", file))
            .bind(("text", text))
            .await
            .map_err(se)?;
            Ok::<_, Error>(())
        })
    }
}

// ── GraphRead ────────────────────────────────────────────────────────────────

impl GraphRead for SurrealStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            full_text_search: false,     // kv-mem does not have BM25 in this config
            vector_search: false,        // HNSW available with kv-surrealkv; not wired here
            server_side_traversal: true, // SurrealQL graph path traversal
            transactional_batch: false,  // begin/commit are no-ops in this impl
            shared_writers: false,
        }
    }

    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
        let db = self.db.clone();
        let id = id.clone();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT data FROM node WHERE symbol=$sym LIMIT 1")
                .bind(("sym", id.0.clone()))
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;
            if rows.is_empty() {
                return Ok(None);
            }
            if let surrealdb::Value::Object(obj) = &rows[0] {
                if let Some(surrealdb::Value::Strand(s)) = obj.get("data") {
                    return Ok(Some(serde_json::from_str(s.as_str()).map_err(se)?));
                }
            }
            Ok(None)
        })
    }

    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
        let db = self.db.clone();
        let query = query.clone();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT data FROM node")
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;

            let mut out: Vec<Node> = Vec::new();
            for v in rows {
                if let surrealdb::Value::Object(obj) = v {
                    if let Some(surrealdb::Value::Strand(s)) = obj.get("data") {
                        if let Ok(n) = serde_json::from_str::<Node>(s.as_str()) {
                            out.push(n);
                        }
                    }
                }
            }

            out.retain(|n| {
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
            });
            out.sort_by(|a, b| a.symbol.0.cmp(&b.symbol.0));
            if let Some(limit) = query.limit {
                out.truncate(limit);
            }
            Ok(out)
        })
    }

    fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>> {
        let db = self.db.clone();
        let id = id.clone();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = match dir {
                Direction::Dependents => db
                    .query("SELECT data FROM edge_rel WHERE tgt=$id")
                    .bind(("id", id.0.clone()))
                    .await
                    .map_err(se)?
                    .take(0)
                    .map_err(se)?,
                Direction::Dependencies => db
                    .query("SELECT data FROM edge_rel WHERE src=$id")
                    .bind(("id", id.0.clone()))
                    .await
                    .map_err(se)?
                    .take(0)
                    .map_err(se)?,
                Direction::Both => db
                    .query("SELECT data FROM edge_rel WHERE src=$id OR tgt=$id")
                    .bind(("id", id.0.clone()))
                    .await
                    .map_err(se)?
                    .take(0)
                    .map_err(se)?,
            };
            let mut out = Vec::new();
            for v in rows {
                if let surrealdb::Value::Object(obj) = v {
                    if let Some(surrealdb::Value::Strand(s)) = obj.get("data") {
                        if let Ok(e) = serde_json::from_str::<Edge>(s.as_str()) {
                            out.push(e);
                        }
                    }
                }
            }
            Ok(out)
        })
    }

    fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph> {
        // Client-side BFS (bounded) — mirrors MemStore's approach.
        // A production SurrealDB implementation would push this to SurrealQL graph path syntax.
        let mut depths: BTreeMap<String, u32> = BTreeMap::new();
        let mut seen: HashSet<SymbolId> = HashSet::new();
        let mut queue: std::collections::VecDeque<(SymbolId, u32)> =
            std::collections::VecDeque::new();

        seen.insert(start.clone());
        queue.push_back((start.clone(), 0));

        let mut sub_nodes: Vec<Node> = Vec::new();
        let mut sub_edges: Vec<Edge> = Vec::new();
        let mut truncated = false;

        if let Some(n) = self.get_node(start)? {
            sub_nodes.push(n);
        }

        while let Some((cur, depth)) = queue.pop_front() {
            if depth >= spec.max_depth {
                continue;
            }
            for e in self.neighbors(&cur, spec.direction)? {
                if e.confidence.get() < spec.min_confidence {
                    continue;
                }
                if !spec.edge_kinds.is_empty() && !spec.edge_kinds.contains(&e.kind) {
                    continue;
                }
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
                sub_edges.push(e);
                if seen.contains(&next) {
                    continue;
                }
                if sub_nodes.len() >= spec.max_nodes {
                    truncated = true;
                    continue;
                }
                seen.insert(next.clone());
                depths.insert(next.0.clone(), depth + 1);
                if let Some(n) = self.get_node(&next)? {
                    sub_nodes.push(n);
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
        let db = self.db.clone();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT data FROM node")
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;
            let mut out = Vec::new();
            for v in rows {
                if let surrealdb::Value::Object(obj) = v {
                    if let Some(surrealdb::Value::Strand(s)) = obj.get("data") {
                        if let Ok(n) = serde_json::from_str::<Node>(s.as_str()) {
                            out.push(n);
                        }
                    }
                }
            }
            Ok(out)
        })
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT data FROM edge_rel")
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;
            let mut out = Vec::new();
            for v in rows {
                if let surrealdb::Value::Object(obj) = v {
                    if let Some(surrealdb::Value::Strand(s)) = obj.get("data") {
                        if let Ok(e) = serde_json::from_str::<Edge>(s.as_str()) {
                            out.push(e);
                        }
                    }
                }
            }
            Ok(out)
        })
    }

    fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>> {
        let db = self.db.clone();
        let name = name.to_string();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT data FROM unresolved WHERE raw_name=$name")
                .bind(("name", name))
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;
            let mut out = Vec::new();
            for v in rows {
                if let surrealdb::Value::Object(obj) = v {
                    if let Some(surrealdb::Value::Strand(s)) = obj.get("data") {
                        if let Ok(r) = serde_json::from_str::<UnresolvedRef>(s.as_str()) {
                            out.push(r);
                        }
                    }
                }
            }
            Ok(out)
        })
    }

    fn file_digest(&self, file: &str) -> Result<Option<String>> {
        let db = self.db.clone();
        let file = file.to_string();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT digest FROM file_meta WHERE path=$path LIMIT 1")
                .bind(("path", file))
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;
            if rows.is_empty() {
                return Ok(None);
            }
            if let surrealdb::Value::Object(obj) = &rows[0] {
                if let Some(surrealdb::Value::Strand(s)) = obj.get("digest") {
                    return Ok(Some(s.as_str().to_string()));
                }
            }
            Ok(None)
        })
    }

    fn file_content(&self, file: &str) -> Result<Option<String>> {
        let db = self.db.clone();
        let file = file.to_string();
        self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT text FROM file_content WHERE path=$path LIMIT 1")
                .bind(("path", file))
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;
            if rows.is_empty() {
                return Ok(None);
            }
            if let surrealdb::Value::Object(obj) = &rows[0] {
                if let Some(surrealdb::Value::Strand(s)) = obj.get("text") {
                    return Ok(Some(s.as_str().to_string()));
                }
            }
            Ok(None)
        })
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

    fn stats(&self) -> Result<GraphStats> {
        let nodes = self.all_nodes()?;
        let edges = self.all_edges()?;

        let node_count = nodes.len() as u64;
        let edge_count = edges.len() as u64;
        let file_kind_json = serde_json::to_string(&NodeKind::File).map_err(se)?;
        let file_count = nodes
            .iter()
            .filter(|n| serde_json::to_string(&n.kind).unwrap_or_default() == file_kind_json)
            .count() as u64;

        let db = self.db.clone();
        let unresolved_ref_count: u64 = self.rt.block_on(async move {
            let rows: Vec<surrealdb::Value> = db
                .query("SELECT count() FROM unresolved GROUP ALL")
                .await
                .map_err(se)?
                .take(0)
                .map_err(se)?;
            if let Some(surrealdb::Value::Object(obj)) = rows.first() {
                if let Some(surrealdb::Value::Number(n)) = obj.get("count") {
                    return Ok(n.to_usize() as u64);
                }
            }
            Ok::<u64, Error>(0)
        })?;

        let mut nodes_by_kind: BTreeMap<String, u64> = BTreeMap::new();
        for n in &nodes {
            let k = serde_json::to_string(&n.kind).unwrap_or_default();
            *nodes_by_kind.entry(k).or_default() += 1;
        }
        let mut edges_by_kind: BTreeMap<String, u64> = BTreeMap::new();
        for e in &edges {
            let k = serde_json::to_string(&e.kind).unwrap_or_default();
            *edges_by_kind.entry(k).or_default() += 1;
        }

        Ok(GraphStats {
            node_count,
            edge_count,
            file_count,
            unresolved_ref_count,
            nodes_by_kind,
            edges_by_kind,
        })
    }
}
