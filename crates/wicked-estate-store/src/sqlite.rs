//! `SqliteStore` — the research-backed default backend (SQLite + WAL; FTS5 + sqlite-vec land at
//! W5). Stores nodes/edges with a few indexed query columns plus the full record as JSON, and
//! implements bounded reverse-reachability as a `WITH RECURSIVE` CTE (not N statements — fixing
//! the per-node-query BFS in prior art/prior art). Passes the same conformance suite as
//! `MemStore`, proving the `GraphStore` trait abstraction holds across backends (the W1.5 premise).

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashSet};
use wicked_estate_core::{
    Change, ChangeOp, Direction, Edge, Error, GraphRead, GraphStats, GraphWrite, HistoricalEdge,
    Node, NodeKind, NodeSemantics, RepoInfo, Result, StoreCapabilities, Subgraph, SymbolId,
    SymbolIndex, SymbolQuery, TraversalSpec, UnresolvedRef,
};

const SCHEMA: &str = include_str!("schema.sql");

// ── Vector math helpers (no external deps) ─────────────────────────────────

/// Compute the L2 (Euclidean) norm of `v`.
#[inline]
fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// Cosine similarity between `a` and `b`.  `a_norm` is the pre-computed L2 norm of `a`.
/// Returns 0.0 if either vector is zero-magnitude.
#[inline]
fn cosine_similarity(a: &[f32], b: &[f32], a_norm: f32) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let b_norm = l2_norm(b);
    if a_norm == 0.0 || b_norm == 0.0 {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    (dot / (a_norm * b_norm)).clamp(-1.0, 1.0)
}

/// Map any displayable error (e.g. `rusqlite::Error`) into our storage error.
fn st<E: std::fmt::Display>(e: E) -> Error {
    Error::Storage(e.to_string())
}

/// Escape a user-supplied string for use in an FTS5 `MATCH` expression.
///
/// FTS5 phrase syntax: `"<term>"` treats the content as a phrase / prefix search and bypasses
/// operator parsing.  Interior double-quotes are doubled (`""`) — the same escaping SQLite uses
/// for identifiers.  This prevents any user input from injecting FTS5 boolean operators
/// (`AND`, `OR`, `NOT`, `*`, `^`, `{ }`, etc.).
fn fts5_quote(term: &str) -> String {
    // Replace each `"` with `""` then wrap in `"…"`.
    let inner = term.replace('"', "\"\"");
    format!("\"{inner}\"")
}

/// Compute the git blob SHA for `text`: `hex(SHA1("blob " + byte_len + "\0" + text))`.
///
/// This is exactly what `git hash-object` computes, so the value correlates to git history
/// and is stable across renames, identical for identical content.
pub fn git_blob_sha(text: &str) -> String {
    let bytes = text.as_bytes();
    let header = format!("blob {}\0", bytes.len());
    let mut h = Sha1::new();
    h.update(header.as_bytes());
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Statistics returned by [`SqliteStore::compact`] / [`MemStore::compact`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactStats {
    /// Edges removed because their source or target node no longer exists.
    pub dangling_edges: usize,
    /// Cache rows removed because their stored version is older than the current graph version.
    pub stale_cache_rows: usize,
    /// Embedding rows removed because their symbol is no longer in the nodes table.
    pub orphan_embeddings: usize,
    /// Content rows removed because no file or edge_history row references them.
    pub orphan_content: usize,
    /// Edge-history rows removed by the retention window (keep newest 20 per file).
    pub history_rows_pruned: usize,
}

/// SQLite-backed graph store.
pub struct SqliteStore {
    conn: Connection,
    in_batch: bool,
    /// Whether to archive edges to `edge_history` on `remove_file`. Default: `false` (opt-in).
    /// Enable with [`set_history_enabled`](SqliteStore::set_history_enabled) or pass `--history`
    /// on the CLI when edge provenance archival is desired.
    history_enabled: bool,
}

impl SqliteStore {
    // -----------------------------------------------------------------------
    // Symbol-string intern helpers (internal only; callers always pass/receive
    // SymbolId(String); interning is an on-disk representation detail).
    // -----------------------------------------------------------------------

    /// Intern `sym` into the `symbols` table and return its integer `sid`.
    ///
    /// Uses INSERT … ON CONFLICT DO NOTHING + a SELECT so one round-trip is
    /// sufficient for already-interned strings (the common case on upsert).
    fn intern(&self, sym: &str) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO symbols(sym) VALUES(?1) ON CONFLICT(sym) DO NOTHING",
                params![sym],
            )
            .map_err(st)?;
        let sid: i64 = self
            .conn
            .query_row("SELECT sid FROM symbols WHERE sym=?1", params![sym], |r| {
                r.get(0)
            })
            .map_err(st)?;
        Ok(sid)
    }

    /// Upsert nodes into the `nodes` table WITHOUT touching `nodes_fts`.
    ///
    /// Used by the hot write path: write all nodes first, then call
    /// [`rebuild_fts_for_files`] once for a bulk FTS rebuild.  This avoids the
    /// O(2 × nodes) per-node DELETE+INSERT into the FTS5 shadow tables.
    pub fn upsert_nodes_no_fts(&mut self, nodes: &[Node]) -> Result<()> {
        // Intern all symbols first, before prepare_cached borrows conn.
        let sids: Vec<i64> = nodes
            .iter()
            .map(|n| self.intern(&n.symbol.0))
            .collect::<Result<_>>()?;
        let mut up = self
            .conn
            .prepare_cached(
                "INSERT INTO nodes(symbol,name,kind,language,file,data) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(symbol) DO UPDATE SET
                   name=excluded.name, kind=excluded.kind,
                   language=excluded.language, file=excluded.file, data=excluded.data",
            )
            .map_err(st)?;
        for (n, sid) in nodes.iter().zip(sids.iter()) {
            let kind = serde_json::to_string(&n.kind)?;
            let data = serde_json::to_string(n)?;
            let file = &n.location.file;
            up.execute(params![sid, n.name, kind, n.language.0, file, data])
                .map_err(st)?;
        }
        Ok(())
    }

    /// Bulk-rebuild `nodes_fts` for every node that belongs to any of the given `files`.
    ///
    /// Two SQL statements regardless of the number of nodes:
    /// 1. DELETE stale FTS rows for the affected files.
    /// 2. INSERT all node rows for those files in a single SELECT.
    ///
    /// This replaces the old O(2 × nodes) per-node loop and avoids repeated incremental
    /// updates to the FTS5 shadow tables.
    ///
    /// `files` is a slice of repo-relative file paths (the same value stored in `nodes.file`).
    /// Passing an empty slice is a no-op.
    pub fn rebuild_fts_for_files(&mut self, files: &[&str]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        // CHUNK to stay under SQLite's bound-parameter limit (SQLITE_MAX_VARIABLE_NUMBER = 32766).
        // A monorepo with >32k changed files on first index (e.g. eliza: 33k+ files) would otherwise
        // blow the `IN (?1, …, ?N)` clause with "variable number must be between ?1 and ?32766".
        // Each chunk runs its own DELETE + INSERT, which is correct: every file's FTS rows are
        // handled in the chunk that contains it.
        const CHUNK: usize = 16_000;
        for chunk in files.chunks(CHUNK) {
            // Build an SQL `IN (?1, ?2, ...)` placeholder list for this chunk.
            // File paths are data — never interpolated as SQL identifiers, only as bound params.
            let placeholders: String = (1..=chunk.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");

            // Step 1: delete stale FTS rows for this chunk's files in one statement.
            // nodes.symbol is now INTEGER (sid); nodes_fts.symbol is TEXT (the string sym).
            // We join through the symbols table to resolve sid → sym for the FTS delete.
            // `prepare` (not `prepare_cached`) because the SQL length varies with the chunk size.
            let del_sql = format!(
                "DELETE FROM nodes_fts WHERE symbol IN \
                 (SELECT s.sym FROM nodes n \
                  JOIN symbols s ON s.sid = n.symbol \
                  WHERE n.file IN ({placeholders}))"
            );
            let mut del_stmt = self.conn.prepare(&del_sql).map_err(st)?;
            for (i, f) in chunk.iter().enumerate() {
                del_stmt.raw_bind_parameter(i + 1, *f).map_err(st)?;
            }
            del_stmt.raw_execute().map_err(st)?;

            // Step 2: bulk-insert FTS rows for all nodes that belong to this chunk's files.
            // signature and doc may be NULL in the JSON; COALESCE to empty string for FTS.
            // nodes.symbol is INTEGER; join symbols to get the string sym for FTS.symbol.
            let ins_sql = format!(
                "INSERT INTO nodes_fts(symbol, name, signature, doc) \
                 SELECT s.sym, n.name, \
                        COALESCE(json_extract(n.data, '$.signature'), ''), \
                        COALESCE(json_extract(n.data, '$.doc'), '') \
                 FROM nodes n \
                 JOIN symbols s ON s.sid = n.symbol \
                 WHERE n.file IN ({placeholders})"
            );
            let mut ins_stmt = self.conn.prepare(&ins_sql).map_err(st)?;
            for (i, f) in chunk.iter().enumerate() {
                ins_stmt.raw_bind_parameter(i + 1, *f).map_err(st)?;
            }
            ins_stmt.raw_execute().map_err(st)?;
        }

        Ok(())
    }
    /// Open an on-disk store (WAL mode), creating the schema if needed.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(st)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=NORMAL; \
             PRAGMA auto_vacuum=INCREMENTAL;",
        )
        .map_err(st)?;
        conn.execute_batch(SCHEMA).map_err(st)?;
        // Read history_enabled flag from meta (absent → OFF, the new default).
        // Old databases that never set this key default to OFF (no history) rather than ON.
        // Callers that want history must pass `--history` (or call set_history_enabled(true)).
        let history_enabled = {
            let v: Option<String> = conn
                .query_row("SELECT v FROM meta WHERE k='history_enabled'", [], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(st)?;
            // Absent → "0" (off). Explicit "1" → on. Anything else → off.
            v.is_some_and(|s| s == "1")
        };
        Ok(Self {
            conn,
            in_batch: false,
            history_enabled,
        })
    }

    /// Open an on-disk store from a [`std::path::Path`] — thin delegate to [`Self::open`].
    /// Used by the connection-pool manager, which holds a `PathBuf`.
    pub fn open_file(path: &std::path::Path) -> Result<Self> {
        Self::open(path)
    }

    /// Open an in-memory store (tests, ephemeral use).
    /// history_enabled defaults to false (opt-in, same as on-disk stores).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(st)?;
        conn.execute_batch(SCHEMA).map_err(st)?;
        Ok(Self {
            conn,
            in_batch: false,
            history_enabled: false,
        })
    }

    /// Enable or disable edge-history archival (default: `false`).
    ///
    /// Persists the flag to `meta` so it survives across open/close cycles on on-disk stores.
    /// Pass `true` (via `--history` CLI flag) when edge provenance archival is desired.
    pub fn set_history_enabled(&mut self, on: bool) -> Result<()> {
        self.history_enabled = on;
        self.meta_set("history_enabled", if on { "1" } else { "0" })?;
        Ok(())
    }

    /// All file paths that have a stored digest (i.e. were indexed). Used by the incremental
    /// CLI to detect deleted files (present in `files` but absent from the current directory scan).
    pub fn indexed_files(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM files").map_err(st)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(st)?);
        }
        Ok(out)
    }

    /// Remove the digest entry for `file` from the `files` table. Called when a file is deleted.
    pub fn remove_file_digest(&mut self, file: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM files WHERE path=?1", params![file])
            .map_err(st)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // W11.2 — Versioned query cache (prior art versioned cache-port pattern).
    // -----------------------------------------------------------------------

    /// Current graph version (integer stored in `meta`).
    fn graph_version(&self) -> Result<i64> {
        let v: String = self
            .conn
            .query_row("SELECT v FROM meta WHERE k='graph_version'", [], |r| {
                r.get(0)
            })
            .map_err(st)?;
        v.parse::<i64>().map_err(st)
    }

    /// Return the cached value for `key` only if it was stored at the current graph version.
    /// Returns `None` when the key is absent or was stored at a prior version.
    pub fn cache_get(&self, key: &str) -> Result<Option<String>> {
        let ver = self.graph_version()?;
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM cache WHERE key=?1 AND version=?2",
                params![key, ver],
                |r| r.get(0),
            )
            .optional()
            .map_err(st)?;
        Ok(value)
    }

    /// Store `value` for `key` at the current graph version.
    pub fn cache_put(&mut self, key: &str, value: &str) -> Result<()> {
        let ver = self.graph_version()?;
        self.conn
            .execute(
                "INSERT INTO cache(key, version, value) VALUES(?1, ?2, ?3)
                 ON CONFLICT(key, version) DO UPDATE SET value=excluded.value",
                params![key, ver, value],
            )
            .map_err(st)?;
        Ok(())
    }

    /// Increment the graph version. All cache entries stored at prior versions become stale
    /// (cache_get will return None for them) without requiring a DELETE sweep.
    pub fn bump_version(&mut self) -> Result<()> {
        self.conn
            .execute(
                "UPDATE meta SET v=CAST(CAST(v AS INTEGER)+1 AS TEXT) WHERE k='graph_version'",
                [],
            )
            .map_err(st)?;
        Ok(())
    }

    /// Read an arbitrary string value from the `meta` table.
    /// Returns `None` when the key is absent.
    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        let val: Option<String> = self
            .conn
            .query_row("SELECT v FROM meta WHERE k=?1", params![key], |r| r.get(0))
            .optional()
            .map_err(st)?;
        Ok(val)
    }

    /// Write an arbitrary string value to the `meta` table (insert or replace).
    pub fn meta_set(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO meta(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                params![key, value],
            )
            .map_err(st)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // W5.2 — Vector embeddings store (inherent, not on the trait).
    //
    // These are intentionally NOT on GraphRead/GraphWrite: the trait is
    // object-safe and designed for graph topology; vector search is an
    // optional sidecar used through concrete types or the VectorStore helper
    // in wicked-estate-retrieve.  The pattern mirrors set_file_content/cache_get above.
    //
    // Storage: vec is packed little-endian f32 bytes (dim * 4 bytes).
    // Brute-force cosine similarity over all stored vectors.  An ANN index
    // (HNSW or similar) is a future optimisation; brute-force is correct
    // and sufficient for local-first scale (≤100k symbols).
    // -----------------------------------------------------------------------

    /// Store (or replace) the embedding vector for `symbol`.
    ///
    /// `vec` must be non-empty; the dimensionality is inferred from `vec.len()`.
    pub fn set_embedding(&mut self, symbol: &SymbolId, vec: &[f32]) -> Result<()> {
        if vec.is_empty() {
            return Err(Error::Invalid("embedding vector must be non-empty".into()));
        }
        let dim = vec.len() as i64;
        // Pack f32 → little-endian bytes.
        let blob: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.conn
            .execute(
                "INSERT INTO embeddings(symbol, dim, vec) VALUES(?1, ?2, ?3)
                 ON CONFLICT(symbol) DO UPDATE SET dim=excluded.dim, vec=excluded.vec",
                params![symbol.0, dim, blob],
            )
            .map_err(st)?;
        Ok(())
    }

    /// Retrieve the stored embedding vector for `symbol`, or `None` if absent.
    pub fn embedding(&self, symbol: &SymbolId) -> Result<Option<Vec<f32>>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT dim, vec FROM embeddings WHERE symbol=?1",
                params![symbol.0],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(st)?;
        match row {
            None => Ok(None),
            Some((dim, blob)) => {
                let expected = dim as usize * 4;
                if blob.len() != expected {
                    return Err(Error::Storage(format!(
                        "embedding blob length mismatch: expected {expected}, got {}",
                        blob.len()
                    )));
                }
                let vec: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                Ok(Some(vec))
            }
        }
    }

    /// Find the `k` nearest symbols to `query` by cosine similarity (brute-force).
    ///
    /// Returns `(SymbolId, cosine_similarity)` pairs sorted descending (highest similarity
    /// first).  Symbols whose stored dimensionality does not match `query.len()` are silently
    /// skipped so a mixed-dimension store does not panic.  Ties are broken by `SymbolId`
    /// lexicographic order for deterministic output.
    ///
    /// Time complexity: O(n × d) where n = number of stored embeddings, d = dimension.
    /// An ANN index (HNSW) is a future optimisation task — track it in WAVE-PLAN W5.2.
    pub fn nearest(&self, query: &[f32], k: usize) -> Result<Vec<(SymbolId, f32)>> {
        if query.is_empty() || k == 0 {
            return Ok(vec![]);
        }
        // Pre-compute the L2 norm of the query vector once.
        let q_norm = l2_norm(query);
        if q_norm == 0.0 {
            return Ok(vec![]);
        }

        // Pull all embeddings from SQLite and score in Rust.  For large corpora a
        // materialised ANN index would replace this scan; see WAVE-PLAN W5.2.
        let mut stmt = self
            .conn
            .prepare("SELECT symbol, dim, vec FROM embeddings")
            .map_err(st)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(st)?;

        let dim = query.len();
        let mut scored: Vec<(SymbolId, f32)> = Vec::new();

        for row in rows {
            let (sym_str, stored_dim, blob) = row.map_err(st)?;
            if stored_dim as usize != dim {
                continue; // dimension mismatch — skip gracefully
            }
            if blob.len() != dim * 4 {
                continue; // corrupt blob — skip gracefully
            }
            let stored: Vec<f32> = blob
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            let sim = cosine_similarity(query, &stored, q_norm);
            scored.push((SymbolId(sym_str), sim));
        }

        // Sort: descending similarity, then ascending SymbolId for ties (deterministic).
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.0.cmp(&b.0.0))
        });
        scored.truncate(k);
        Ok(scored)
    }

    /// Compact the store: prune dangling edges, stale cache rows, orphan embeddings and content,
    /// edge-history beyond the 20-row-per-file retention window, then checkpoint the WAL and VACUUM.
    ///
    /// This is an **inherent** method (not on the trait) so the CLI can open a concrete
    /// `SqliteStore` and call it directly.  It is intentionally not on `GraphWrite` — compact
    /// is an operational / DBA-style operation, not part of the core graph mutation contract.
    pub fn compact(&mut self) -> Result<CompactStats> {
        // (1) prune dangling edges (reuses the GraphWrite impl).
        let dangling_edges = self.prune_dangling_edges()?;

        // (2) prune stale cache rows: version < current graph_version.
        let ver = self.graph_version()?;
        let stale_cache_rows = self
            .conn
            .execute("DELETE FROM cache WHERE version < ?1", params![ver])
            .map_err(st)?;

        // (3) orphan embeddings: symbol (TEXT) not referenced by any interned node.
        // embeddings.symbol is TEXT (the string sym); nodes.symbol is INTEGER (sid).
        // We join through the symbols table: an embedding is live when its sym exists in
        // symbols AND that sym's sid is present in nodes.
        let orphan_embeddings = self
            .conn
            .execute(
                "DELETE FROM embeddings \
                 WHERE symbol NOT IN ( \
                   SELECT s.sym FROM nodes n JOIN symbols s ON s.sid = n.symbol \
                 )",
                [],
            )
            .map_err(st)?;

        // (4) orphan content: git_sha not referenced by any files row AND not referenced by
        //     any edge_history row (history preserves old content until the history itself is pruned).
        let orphan_content = self
            .conn
            .execute(
                "DELETE FROM content \
                 WHERE git_sha NOT IN (SELECT git_sha FROM files WHERE git_sha IS NOT NULL) \
                   AND git_sha NOT IN (SELECT DISTINCT git_sha FROM edge_history WHERE git_sha != '')",
                [],
            )
            .map_err(st)?;

        // (5) edge_history retention: keep the newest 20 rows per file; delete older ones.
        //     We identify the cut-off archived_seq per file and delete everything below it.
        let history_rows_pruned = self
            .conn
            .execute(
                "DELETE FROM edge_history \
                 WHERE archived_seq NOT IN ( \
                   SELECT archived_seq FROM edge_history h2 \
                   WHERE h2.file = edge_history.file \
                   ORDER BY archived_seq DESC \
                   LIMIT 20 \
                 )",
                [],
            )
            .map_err(st)?;

        // (6) SQLite-specific: checkpoint WAL then VACUUM to reclaim disk space.
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
            .map_err(st)?;

        Ok(CompactStats {
            dangling_edges,
            stale_cache_rows,
            orphan_embeddings,
            orphan_content,
            history_rows_pruned,
        })
    }

    /// Reachable set from `start` in one direction, via a bounded recursive CTE → {sym: min depth}.
    ///
    /// The CTE traverses edges by INTEGER sid (fast integer comparisons); the start sid is looked
    /// up from the `symbols` table.  Results are resolved back to string SymbolIds by joining
    /// `symbols` at the final SELECT boundary.
    fn cte_reach(
        &self,
        start: &SymbolId,
        dir: Direction,
        spec: &TraversalSpec,
    ) -> Result<BTreeMap<String, u32>> {
        // Look up the start symbol's sid.  If not interned yet, nothing can be reachable.
        let start_sid: Option<i64> = self
            .conn
            .query_row(
                "SELECT sid FROM symbols WHERE sym=?1",
                params![start.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(st)?;
        let start_sid = match start_sid {
            Some(s) => s,
            None => return Ok(BTreeMap::new()),
        };

        // (column matched against the frontier, column we advance to)
        let (match_col, advance_col) = match dir {
            Direction::Dependents => ("target", "source"),
            Direction::Dependencies => ("source", "target"),
            Direction::Both => unreachable!("Both handled in traverse()"),
        };
        let kind_filter = if spec.edge_kinds.is_empty() {
            String::new()
        } else {
            let list = spec
                .edge_kinds
                .iter()
                .map(|k| {
                    let s = serde_json::to_string(k).unwrap_or_default();
                    format!("'{}'", s.replace('\'', "''"))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("AND e.kind IN ({list})")
        };
        // The CTE walks INTEGER sid columns in edges (fast).  The final SELECT resolves
        // sid → sym by joining the symbols table.
        // column names are fixed literals (never user input) → safe to interpolate.
        let sql = format!(
            "WITH RECURSIVE walk(id, depth) AS (
                 SELECT ?1, 0
                 UNION
                 SELECT e.{advance_col}, walk.depth + 1
                   FROM edges e JOIN walk ON e.{match_col} = walk.id
                  WHERE walk.depth < ?2 AND e.confidence >= ?3 {kind_filter}
             )
             SELECT s.sym, MIN(walk.depth)
               FROM walk
               JOIN symbols s ON s.sid = walk.id
              WHERE walk.id <> ?1
              GROUP BY walk.id
              ORDER BY 2
              LIMIT ?4"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(st)?;
        let rows = stmt
            .query_map(
                params![
                    start_sid,
                    spec.max_depth,
                    spec.min_confidence as f64,
                    spec.max_nodes as i64
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32)),
            )
            .map_err(st)?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (sym, depth) = row.map_err(st)?;
            out.insert(sym, depth);
        }
        Ok(out)
    }
}

impl GraphWrite for SqliteStore {
    fn begin_batch(&mut self) -> Result<()> {
        if !self.in_batch {
            self.conn.execute_batch("BEGIN").map_err(st)?;
            self.in_batch = true;
        }
        Ok(())
    }

    fn commit_batch(&mut self) -> Result<()> {
        if self.in_batch {
            self.conn.execute_batch("COMMIT").map_err(st)?;
            self.in_batch = false;
        }
        Ok(())
    }

    fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()> {
        // Intern all symbols first, before prepare_cached borrows conn.
        // `prepare_cached` reuses ONE prepared statement across the loop (no per-row re-prepare —
        // the difference between ~300k VM compiles and ~3 on a large repo). §9: slow is a defect.
        let sids: Vec<i64> = nodes
            .iter()
            .map(|n| self.intern(&n.symbol.0))
            .collect::<Result<_>>()?;
        let mut up = self
            .conn
            .prepare_cached(
                "INSERT INTO nodes(symbol,name,kind,language,file,data) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(symbol) DO UPDATE SET
                   name=excluded.name, kind=excluded.kind,
                   language=excluded.language, file=excluded.file, data=excluded.data",
            )
            .map_err(st)?;
        // W5.1: keep the FTS table in sync. DELETE + INSERT (no `ON CONFLICT` for virtual tables).
        // nodes_fts.symbol is TEXT (the string sym), so we use the original string directly.
        let mut fts_del = self
            .conn
            .prepare_cached("DELETE FROM nodes_fts WHERE symbol = ?1")
            .map_err(st)?;
        let mut fts_ins = self
            .conn
            .prepare_cached(
                "INSERT INTO nodes_fts(symbol, name, signature, doc) VALUES(?1, ?2, ?3, ?4)",
            )
            .map_err(st)?;
        for (n, sid) in nodes.iter().zip(sids.iter()) {
            let kind = serde_json::to_string(&n.kind)?;
            let data = serde_json::to_string(n)?;
            let file = &n.location.file;
            up.execute(params![sid, n.name, kind, n.language.0, file, data])
                .map_err(st)?;
            // FTS uses the string symbol directly (nodes_fts.symbol is TEXT).
            fts_del.execute(params![n.symbol.0]).map_err(st)?;
            fts_ins
                .execute(params![
                    n.symbol.0,
                    n.name,
                    n.signature.as_deref().unwrap_or(""),
                    n.doc.as_deref().unwrap_or(""),
                ])
                .map_err(st)?;
        }
        Ok(())
    }

    fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()> {
        // Intern all source/target symbol strings first, before prepare_cached borrows conn.
        let sids: Vec<(i64, i64)> = edges
            .iter()
            .map(|e| {
                let src = self.intern(&e.source.0)?;
                let tgt = self.intern(&e.target.0)?;
                Ok((src, tgt))
            })
            .collect::<Result<_>>()?;
        // On a (source,target,kind) collision, keep the higher-confidence edge (W3.4).
        let mut stmt = self
            .conn
            .prepare_cached(
                "INSERT INTO edges(source,target,kind,confidence,file,data) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(source,target,kind) DO UPDATE SET
                   confidence=excluded.confidence, file=excluded.file, data=excluded.data
                 WHERE excluded.confidence >= edges.confidence",
            )
            .map_err(st)?;
        for (e, (src_sid, tgt_sid)) in edges.iter().zip(sids.iter()) {
            let kind = serde_json::to_string(&e.kind)?;
            let data = serde_json::to_string(e)?;
            // Use the location file when present; empty string when None (e.g. synthetic edges).
            // After Fix A (wicked-estate-extract), Contains edges always carry a location, so this is always
            // populated for tree-sitter-produced edges.
            let file = e.location.as_ref().map(|l| l.file.as_str()).unwrap_or("");
            stmt.execute(params![
                src_sid,
                tgt_sid,
                kind,
                e.confidence.get() as f64,
                file,
                data
            ])
            .map_err(st)?;
        }
        Ok(())
    }

    fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()> {
        // Intern all from_sym strings first, before prepare_cached borrows conn.
        // Persist COVERAGE fidelity only: from_sym, raw_name, kind, file, line.
        // The full span and hints are intentionally NOT persisted (the in-memory resolve pass
        // owns those).  This is a deliberate ~8× disk reduction vs the old `data` JSON blob.
        let from_sids: Vec<i64> = refs
            .iter()
            .map(|r| self.intern(&r.from.0))
            .collect::<Result<_>>()?;
        let mut stmt = self
            .conn
            .prepare_cached(
                "INSERT INTO unresolved_refs(from_sym, raw_name, kind, file, line) \
                 VALUES(?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(st)?;
        for (r, from_sid) in refs.iter().zip(from_sids.iter()) {
            let kind = serde_json::to_string(&r.kind)?;
            let file = &r.location.file;
            let line = r.location.span.start_line as i64;
            stmt.execute(params![from_sid, r.raw_name, kind, file, line])
                .map_err(st)?;
        }
        Ok(())
    }

    fn remove_file(&mut self, file: &str) -> Result<()> {
        // Step 1: read the file's CURRENT git_sha (the version being superseded; NULL → "").
        let current_git_sha: String = {
            let v: Option<Option<String>> = self
                .conn
                .query_row(
                    "SELECT git_sha FROM files WHERE path=?1",
                    params![file],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()
                .map_err(st)?;
            v.flatten().unwrap_or_default()
        };

        // Step 2: if history is enabled, archive the current edges for this file into edge_history
        // BEFORE deleting them. Each edge is stored with the file's current git_sha.
        //
        // Match edges by EITHER:
        //   (a) edges.file = this file (edge carries an explicit location)
        //   (b) edge.source IN nodes of this file (edge created without explicit location;
        //       e.g. synthetic or resolver-produced edges that don't carry a span)
        // This ensures all edges logically owned by a file are archived, not just the subset
        // that happened to carry an explicit location when produced.
        if self.history_enabled {
            let edges_to_archive: Vec<String> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT data FROM edges \
                         WHERE file=?1 \
                            OR source IN (SELECT symbol FROM nodes WHERE file=?1)",
                    )
                    .map_err(st)?;
                let rows = stmt
                    .query_map(params![file], |r| r.get::<_, String>(0))
                    .map_err(st)?;
                let mut v = Vec::new();
                for row in rows {
                    v.push(row.map_err(st)?);
                }
                v
            };
            if !edges_to_archive.is_empty() {
                let mut ins = self
                    .conn
                    .prepare_cached(
                        "INSERT INTO edge_history(git_sha, file, edge_json) VALUES(?1, ?2, ?3)",
                    )
                    .map_err(st)?;
                for edge_json in &edges_to_archive {
                    ins.execute(params![current_git_sha, file, edge_json])
                        .map_err(st)?;
                }
            }
        }

        // Step 3: delete FTS rows and embeddings for nodes that belong to this file.
        // nodes.symbol is now INTEGER (sid); resolve to string via symbols table for FTS
        // (nodes_fts.symbol is TEXT) and embeddings (embeddings.symbol is TEXT).
        let syms: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT s.sym FROM nodes n \
                     JOIN symbols s ON s.sid = n.symbol \
                     WHERE n.file=?1",
                )
                .map_err(st)?;
            let rows = stmt
                .query_map(params![file], |r| r.get::<_, String>(0))
                .map_err(st)?;
            let mut v = Vec::new();
            for row in rows {
                v.push(row.map_err(st)?);
            }
            v
        };
        for sym in &syms {
            self.conn
                .execute("DELETE FROM nodes_fts WHERE symbol=?1", params![sym])
                .map_err(st)?;
            self.conn
                .execute("DELETE FROM embeddings WHERE symbol=?1", params![sym])
                .map_err(st)?;
        }

        // Step 4: delete nodes, edges, unresolved_refs, and the files row.
        // NOTE: do NOT delete from `content` here — content is content-addressed and may be
        // shared across versions; orphans are reclaimed by compact().
        // Delete edges using the same matching logic as Step 2 (location file OR source node
        // in this file) so that edges without an explicit location are also removed.
        // IMPORTANT: delete edges BEFORE nodes so the subquery on nodes is still valid.
        self.conn
            .execute(
                "DELETE FROM edges \
                 WHERE file=?1 \
                    OR source IN (SELECT symbol FROM nodes WHERE file=?1)",
                params![file],
            )
            .map_err(st)?;
        self.conn
            .execute("DELETE FROM nodes WHERE file=?1", params![file])
            .map_err(st)?;
        self.conn
            .execute("DELETE FROM unresolved_refs WHERE file=?1", params![file])
            .map_err(st)?;
        self.conn
            .execute("DELETE FROM files WHERE path=?1", params![file])
            .map_err(st)?;
        Ok(())
    }

    fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO files(path, digest) VALUES(?1, ?2)
                 ON CONFLICT(path) DO UPDATE SET digest=excluded.digest",
                params![file, digest],
            )
            .map_err(st)?;
        Ok(())
    }

    fn set_file_content(&mut self, file: &str, text: &str) -> Result<()> {
        // Compute the git blob SHA for this content version.
        let sha = git_blob_sha(text);
        // Compress at level 3 (fast, ~4× ratio for source code).
        let compressed = zstd::encode_all(text.as_bytes(), 3)
            .map_err(|e| Error::Storage(format!("zstd encode: {e}")))?;
        // INSERT OR IGNORE: identical content in different files shares one content row (dedup).
        self.conn
            .execute(
                "INSERT OR IGNORE INTO content(git_sha, blob) VALUES(?1, ?2)",
                params![sha, compressed],
            )
            .map_err(st)?;
        // Upsert the files row, recording both digest (kept for incremental) and git_sha.
        self.conn
            .execute(
                "INSERT INTO files(path, digest, git_sha) VALUES(?1, '', ?2)
                 ON CONFLICT(path) DO UPDATE SET git_sha=excluded.git_sha",
                params![file, sha],
            )
            .map_err(st)?;
        Ok(())
    }

    fn prune_dangling_edges(&mut self) -> Result<usize> {
        // Delete edges whose source or target is not present in the nodes table.
        // A single SQL DELETE with OR is correct and faster than two passes.
        let n = self
            .conn
            .execute(
                "DELETE FROM edges \
                 WHERE source NOT IN (SELECT symbol FROM nodes) \
                    OR target NOT IN (SELECT symbol FROM nodes)",
                [],
            )
            .map_err(st)?;
        Ok(n)
    }

    fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()> {
        // Persist as individual meta keys — stays generic, no schema change.
        self.meta_set("repo_commit", info.commit.as_deref().unwrap_or(""))?;
        self.meta_set("repo_branch", info.branch.as_deref().unwrap_or(""))?;
        self.meta_set("repo_remote", info.remote.as_deref().unwrap_or(""))?;
        self.meta_set("repo_dirty", if info.dirty { "1" } else { "0" })?;
        Ok(())
    }

    fn log_change(&mut self, op: ChangeOp, target: &str) -> Result<()> {
        let op_str = match op {
            ChangeOp::Upsert => "upsert",
            ChangeOp::Remove => "remove",
        };
        self.conn
            .execute(
                "INSERT INTO changes(op, target) VALUES(?1, ?2)",
                params![op_str, target],
            )
            .map_err(st)?;
        Ok(())
    }

    fn set_node_semantics(
        &mut self,
        symbol: &SymbolId,
        description: Option<&str>,
        requirement: Option<&str>,
        requirement_validated: Option<bool>,
    ) -> Result<()> {
        // No fields to set → no-op (nothing to write).
        if description.is_none() && requirement.is_none() && requirement_validated.is_none() {
            return Ok(());
        }
        // Look up the sid without interning: if not present, the symbol is not a node.
        let sid: Option<i64> = self
            .conn
            .query_row(
                "SELECT sid FROM symbols WHERE sym=?1",
                params![symbol.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(st)?;
        let sid = match sid {
            Some(s) => s,
            None => return Ok(()), // symbol never interned → cannot be a node
        };

        // Issue one UPDATE per provided field.  Column names are compile-time literals.
        // Each UPDATE affects 0 rows when the sid has no node row — that is the defined no-op.
        if let Some(d) = description {
            self.conn
                .execute(
                    "UPDATE nodes SET description=?2 WHERE symbol=?1",
                    params![sid, d],
                )
                .map_err(st)?;
        }
        if let Some(r) = requirement {
            self.conn
                .execute(
                    "UPDATE nodes SET requirement=?2 WHERE symbol=?1",
                    params![sid, r],
                )
                .map_err(st)?;
        }
        if let Some(v) = requirement_validated {
            let flag: i64 = v as i64;
            self.conn
                .execute(
                    "UPDATE nodes SET requirement_validated=?2 WHERE symbol=?1",
                    params![sid, flag],
                )
                .map_err(st)?;
        }
        Ok(())
    }
}

impl GraphRead for SqliteStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            full_text_search: true,      // W5.1: FTS5 BM25 via nodes_fts
            vector_search: true,         // W5.2: brute-force cosine via embeddings table
            server_side_traversal: true, // WITH RECURSIVE CTE
            transactional_batch: true,
            shared_writers: false, // single file/connection; an external DB sets this true
        }
    }

    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
        // Resolve string → sid; if not interned, the node cannot exist.
        let sid: Option<i64> = self
            .conn
            .query_row("SELECT sid FROM symbols WHERE sym=?1", params![id.0], |r| {
                r.get(0)
            })
            .optional()
            .map_err(st)?;
        let sid = match sid {
            Some(s) => s,
            None => return Ok(None),
        };
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM nodes WHERE symbol=?1",
                params![sid],
                |r| r.get(0),
            )
            .optional()
            .map_err(st)?;
        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
    }

    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
        // W5.1: when `text` is set, use FTS5 BM25 ranking to retrieve candidates in relevance
        // order, then apply the remaining filters (kinds / language / exact_name) + limit in Rust.
        // When `text` is None, keep the original indexed-name or full-scan path.
        let mut nodes: Vec<Node> = if let Some(text) = &query.text {
            // Escape the user term so arbitrary input cannot inject FTS5 syntax.
            // Strategy: quote the whole term as a phrase by replacing interior double-quotes with
            // two double-quotes, then wrapping in double-quotes.  This is the only syntax-safe
            // approach for user-supplied strings in FTS5.
            let escaped = fts5_quote(text);
            // Join back to `nodes` via the symbols table:
            //   nodes_fts.symbol (TEXT) → symbols.sym → symbols.sid → nodes.symbol (INTEGER).
            // ordered by BM25 (lower is better).
            let sql = "SELECT n.data \
                       FROM nodes_fts f \
                       JOIN symbols s ON s.sym = f.symbol \
                       JOIN nodes n ON n.symbol = s.sid \
                       WHERE nodes_fts MATCH ?1 \
                       ORDER BY bm25(nodes_fts)";
            let mut stmt = self.conn.prepare(sql).map_err(st)?;
            let rows = stmt
                .query_map(params![escaped], |r| r.get::<_, String>(0))
                .map_err(st)?;
            let mut v = Vec::new();
            for row in rows {
                v.push(serde_json::from_str::<Node>(&row.map_err(st)?)?);
            }
            v
        } else if let Some(name) = &query.exact_name {
            let mut stmt = self
                .conn
                .prepare("SELECT data FROM nodes WHERE name=?1 ORDER BY symbol")
                .map_err(st)?;
            let rows = stmt
                .query_map(params![name], |r| r.get::<_, String>(0))
                .map_err(st)?;
            let mut v = Vec::new();
            for row in rows {
                v.push(serde_json::from_str::<Node>(&row.map_err(st)?)?);
            }
            v
        } else {
            let mut stmt = self
                .conn
                .prepare("SELECT data FROM nodes ORDER BY symbol")
                .map_err(st)?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(st)?;
            let mut v = Vec::new();
            for row in rows {
                v.push(serde_json::from_str::<Node>(&row.map_err(st)?)?);
            }
            v
        };

        // Apply remaining filters in Rust (BM25 order preserved because retain is stable).
        nodes.retain(|n| {
            if !query.kinds.is_empty() && !query.kinds.contains(&n.kind) {
                return false;
            }
            if let Some(lang) = &query.language {
                if &n.language != lang {
                    return false;
                }
            }
            // When text was the driver, skip the old substring check — BM25 already ranked them.
            // When text is None but exact_name is also None (full scan), no text filter to apply.
            if query.text.is_none() {
                if let Some(name) = &query.exact_name {
                    if &n.name != name {
                        return false;
                    }
                }
            }
            true
        });
        if let Some(limit) = query.limit {
            nodes.truncate(limit);
        }
        Ok(nodes)
    }

    fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>> {
        // Resolve string → sid; if not interned, no edges can exist for this symbol.
        let sid: Option<i64> = self
            .conn
            .query_row("SELECT sid FROM symbols WHERE sym=?1", params![id.0], |r| {
                r.get(0)
            })
            .optional()
            .map_err(st)?;
        let sid = match sid {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };
        let sql = match dir {
            Direction::Dependents => "SELECT data FROM edges WHERE target=?1",
            Direction::Dependencies => "SELECT data FROM edges WHERE source=?1",
            Direction::Both => "SELECT data FROM edges WHERE source=?1 OR target=?1",
        };
        let mut stmt = self.conn.prepare(sql).map_err(st)?;
        let rows = stmt
            .query_map(params![sid], |r| r.get::<_, String>(0))
            .map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str::<Edge>(&row.map_err(st)?)?);
        }
        Ok(out)
    }

    fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph> {
        let depths = match spec.direction {
            Direction::Both => {
                let mut a = self.cte_reach(start, Direction::Dependents, spec)?;
                for (k, v) in self.cte_reach(start, Direction::Dependencies, spec)? {
                    a.entry(k).and_modify(|e| *e = (*e).min(v)).or_insert(v);
                }
                a
            }
            d => self.cte_reach(start, d, spec)?,
        };
        let truncated = depths.len() >= spec.max_nodes;

        let mut nodes = Vec::new();
        if let Some(n) = self.get_node(start)? {
            nodes.push(n);
        }
        for id in depths.keys() {
            if let Some(n) = self.get_node(&SymbolId(id.clone()))? {
                nodes.push(n);
            }
        }

        // Induced edges: neighbors of start + reached nodes in the traversal direction.
        let mut edges = Vec::new();
        let mut seen = HashSet::new();
        let mut anchors: Vec<SymbolId> = vec![start.clone()];
        anchors.extend(depths.keys().map(|k| SymbolId(k.clone())));
        for a in &anchors {
            for e in self.neighbors(a, spec.direction)? {
                if seen.insert(e.dedup_key()) {
                    edges.push(e);
                }
            }
        }

        Ok(Subgraph {
            nodes,
            edges,
            depths,
            truncated,
        })
    }

    fn all_nodes(&self) -> Result<Vec<Node>> {
        let mut stmt = self.conn.prepare("SELECT data FROM nodes").map_err(st)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str::<Node>(&row.map_err(st)?)?);
        }
        Ok(out)
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare("SELECT data FROM edges").map_err(st)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str::<Edge>(&row.map_err(st)?)?);
        }
        Ok(out)
    }

    fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>> {
        use wicked_estate_core::{Location, Span};
        // from_sym is now INTEGER (sid); join symbols to resolve it back to the string sym.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT s.sym, u.raw_name, u.kind, u.file, u.line \
                 FROM unresolved_refs u \
                 JOIN symbols s ON s.sid = u.from_sym \
                 WHERE u.raw_name=?1",
            )
            .map_err(st)?;
        let rows = stmt
            .query_map(params![name], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            let (from_sym, raw_name, kind_json, file, line) = row.map_err(st)?;
            let kind = serde_json::from_str(&kind_json)?;
            let location = Location::new(
                file,
                Span {
                    start_line: line as u32,
                    start_byte: 0,
                    end_byte: 0,
                    start_col: 0,
                    end_line: 0,
                    end_col: 0,
                },
            );
            out.push(UnresolvedRef {
                from: SymbolId(from_sym),
                raw_name,
                kind,
                location,
                hints: Default::default(),
            });
        }
        Ok(out)
    }

    fn file_digest(&self, file: &str) -> Result<Option<String>> {
        let digest: Option<String> = self
            .conn
            .query_row(
                "SELECT digest FROM files WHERE path=?1",
                params![file],
                |r| r.get(0),
            )
            .optional()
            .map_err(st)?;
        Ok(digest)
    }

    fn file_git_sha(&self, file: &str) -> Result<Option<String>> {
        let sha: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT git_sha FROM files WHERE path=?1",
                params![file],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(st)?;
        Ok(sha.flatten())
    }

    fn repo_info(&self) -> Result<Option<RepoInfo>> {
        // If any of the repo_* meta keys are missing we treat it as "never set".
        let commit = self.meta_get("repo_commit")?;
        match commit {
            None => Ok(None),
            Some(c) => {
                let branch = self.meta_get("repo_branch")?;
                let remote = self.meta_get("repo_remote")?;
                let dirty = self.meta_get("repo_dirty")?.is_some_and(|v| v == "1");
                Ok(Some(RepoInfo {
                    commit: if c.is_empty() { None } else { Some(c) },
                    branch: branch.filter(|s| !s.is_empty()),
                    remote: remote.filter(|s| !s.is_empty()),
                    dirty,
                }))
            }
        }
    }

    fn changes_since(&self, cursor: u64) -> Result<Vec<Change>> {
        // Cap per-call: 10_000 rows. Callers loop until they receive fewer than 10_000 rows.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT seq, op, target FROM changes \
                 WHERE seq > ?1 ORDER BY seq ASC LIMIT 10000",
            )
            .map_err(st)?;
        let rows = stmt
            .query_map(params![cursor as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, op_str, target) = row.map_err(st)?;
            let op = match op_str.as_str() {
                "remove" => ChangeOp::Remove,
                _ => ChangeOp::Upsert,
            };
            out.push(Change {
                seq: seq as u64,
                op,
                target,
            });
        }
        Ok(out)
    }

    fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT archived_seq, git_sha, edge_json FROM edge_history \
                 WHERE file=?1 ORDER BY archived_seq DESC",
            )
            .map_err(st)?;
        let rows = stmt
            .query_map(params![file], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            let (archived_seq, git_sha, edge_json) = row.map_err(st)?;
            let edge: Edge = serde_json::from_str(&edge_json)?;
            out.push(HistoricalEdge {
                git_sha,
                archived_seq: archived_seq as u64,
                edge,
            });
        }
        Ok(out)
    }

    fn file_content(&self, file: &str) -> Result<Option<String>> {
        // Resolve via the content-addressed join: files.git_sha → content.blob.
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT c.blob FROM files f \
                 JOIN content c ON c.git_sha = f.git_sha \
                 WHERE f.path=?1",
                params![file],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(st)?;
        match blob {
            None => Ok(None),
            Some(b) => {
                let bytes = zstd::decode_all(&b[..])
                    .map_err(|e| Error::Storage(format!("zstd decode: {e}")))?;
                let text = String::from_utf8(bytes)
                    .map_err(|e| Error::Storage(format!("content utf8: {e}")))?;
                Ok(Some(text))
            }
        }
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
        // tree-sitter byte offsets are always on valid UTF-8 character boundaries.
        // We still guard with is_char_boundary to surface any unexpected violation as
        // None rather than a panic.
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Ok(None);
        }
        Ok(Some(text[start..end].to_string()))
    }

    fn node_semantics(&self, symbol: &SymbolId) -> Result<Option<NodeSemantics>> {
        // Look up sid without creating an intern row (SELECT-only, no INSERT).
        let sid: Option<i64> = self
            .conn
            .query_row(
                "SELECT sid FROM symbols WHERE sym=?1",
                params![symbol.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(st)?;
        let sid = match sid {
            Some(s) => s,
            None => return Ok(None), // symbol never seen → no node, no semantics
        };
        // Only return Some when at least one semantic column has been written.
        // `description IS NOT NULL OR requirement IS NOT NULL OR requirement_validated != 0`
        // distinguishes "never annotated" (all defaults) from "annotated to empty string/false".
        let row: Option<(Option<String>, Option<String>, i64)> = self
            .conn
            .query_row(
                "SELECT description, requirement, requirement_validated \
                 FROM nodes \
                 WHERE symbol=?1 \
                   AND (description IS NOT NULL \
                        OR requirement IS NOT NULL \
                        OR requirement_validated != 0)",
                params![sid],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(st)?;
        match row {
            None => Ok(None), // node row exists but no semantic annotation has been set
            Some((description, requirement, validated_int)) => Ok(Some(NodeSemantics {
                description,
                requirement,
                requirement_validated: validated_int != 0,
            })),
        }
    }

    fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>> {
        // SELECT node data for every node whose `requirement` column matches.
        // Join symbols to ensure the node's string SymbolId is reconstructed correctly via `data`.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT n.data FROM nodes n \
                 WHERE n.requirement=?1 \
                 ORDER BY n.symbol",
            )
            .map_err(st)?;
        let rows = stmt
            .query_map(params![requirement], |r| r.get::<_, String>(0))
            .map_err(st)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str::<Node>(&row.map_err(st)?)?);
        }
        Ok(out)
    }

    fn stats(&self) -> Result<GraphStats> {
        let node_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
            .map_err(st)?;
        let edge_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .map_err(st)?;
        let file_kind = serde_json::to_string(&NodeKind::File)?;
        let file_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE kind=?1",
                params![file_kind],
                |r| r.get(0),
            )
            .map_err(st)?;
        let unresolved_ref_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM unresolved_refs", [], |r| r.get(0))
            .map_err(st)?;

        let mut nodes_by_kind = BTreeMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT kind, COUNT(*) FROM nodes GROUP BY kind")
                .map_err(st)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
                })
                .map_err(st)?;
            for row in rows {
                let (k, c) = row.map_err(st)?;
                nodes_by_kind.insert(k, c);
            }
        }
        let mut edges_by_kind = BTreeMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT kind, COUNT(*) FROM edges GROUP BY kind")
                .map_err(st)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
                })
                .map_err(st)?;
            for row in rows {
                let (k, c) = row.map_err(st)?;
                edges_by_kind.insert(k, c);
            }
        }

        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .map_err(st)?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .map_err(st)?;
        let db_size_bytes = (page_count * page_size) as u64;

        Ok(GraphStats {
            node_count: node_count as u64,
            edge_count: edge_count as u64,
            file_count: file_count as u64,
            unresolved_ref_count: unresolved_ref_count as u64,
            nodes_by_kind,
            edges_by_kind,
            db_size_bytes,
        })
    }
}

impl SymbolIndex for SqliteStore {
    fn by_name(&self, name: &str) -> Vec<Node> {
        let query = SymbolQuery {
            exact_name: Some(name.to_string()),
            ..Default::default()
        };
        self.find_symbols(&query).unwrap_or_default()
    }
    fn get(&self, id: &SymbolId) -> Option<Node> {
        self.get_node(id).ok().flatten()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — W5.2 vector storage (SqliteStore)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> SymbolId {
        SymbolId(s.to_string())
    }

    fn unit_vec(dim: usize, i: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; dim];
        v[i] = 1.0;
        v
    }

    fn open() -> SqliteStore {
        SqliteStore::in_memory().expect("in-memory store")
    }

    #[test]
    fn sqlite_rebuild_fts_chunks_beyond_sqlite_param_limit() {
        // Regression: a monorepo with >32766 changed files (eliza has 33k+) must not blow SQLite's
        // bound-parameter limit (SQLITE_MAX_VARIABLE_NUMBER = 32766). `rebuild_fts_for_files` chunks
        // the file list. We pass 40_000 synthetic names (no matching nodes — the statements touch 0
        // rows but must still bind+execute across multiple chunks without error).
        let mut store = open();
        let names: Vec<String> = (0..40_000).map(|i| format!("src/f{i}.rs")).collect();
        let files: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        store
            .rebuild_fts_for_files(&files)
            .expect("rebuild_fts_for_files must chunk under the SQLite parameter limit");
    }

    // -- round-trip ---------------------------------------------------------------

    #[test]
    fn sqlite_set_get_embedding_roundtrip() {
        let mut store = open();
        let id = sym("foo");
        let vec = vec![0.1_f32, 0.2, 0.3];
        store.set_embedding(&id, &vec).unwrap();
        let got = store.embedding(&id).unwrap().expect("should be present");
        assert_eq!(got.len(), 3);
        for (a, b) in got.iter().zip(vec.iter()) {
            assert!((a - b).abs() < 1e-5, "f32 round-trip through le bytes");
        }
    }

    #[test]
    fn sqlite_embedding_absent_returns_none() {
        let store = open();
        assert!(store.embedding(&sym("nope")).unwrap().is_none());
    }

    #[test]
    fn sqlite_set_embedding_empty_vec_returns_error() {
        let mut store = open();
        assert!(store.set_embedding(&sym("bad"), &[]).is_err());
    }

    #[test]
    fn sqlite_set_embedding_replaces_existing() {
        let mut store = open();
        let id = sym("x");
        store.set_embedding(&id, &[1.0, 0.0]).unwrap();
        store.set_embedding(&id, &[0.0, 1.0]).unwrap(); // replace
        let got = store.embedding(&id).unwrap().unwrap();
        assert!((got[0] - 0.0).abs() < 1e-5);
        assert!((got[1] - 1.0).abs() < 1e-5);
    }

    // -- nearest ------------------------------------------------------------------

    #[test]
    fn sqlite_nearest_returns_closest_first() {
        let mut store = open();
        store.set_embedding(&sym("a"), &unit_vec(4, 0)).unwrap();
        store.set_embedding(&sym("b"), &unit_vec(4, 1)).unwrap();
        store.set_embedding(&sym("c"), &unit_vec(4, 2)).unwrap();

        // Query close to "a".
        let q = vec![0.9_f32, 0.1, 0.0, 0.0];
        let results = store.nearest(&q, 3).unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, sym("a"), "a should be nearest");
        assert!(results[0].1 >= results[1].1);
        assert!(results[1].1 >= results[2].1);
    }

    #[test]
    fn sqlite_nearest_exact_match_scores_one() {
        let mut store = open();
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
    fn sqlite_nearest_k_larger_than_store_returns_all() {
        let mut store = open();
        store.set_embedding(&sym("p"), &unit_vec(2, 0)).unwrap();
        store.set_embedding(&sym("q"), &unit_vec(2, 1)).unwrap();
        let results = store.nearest(&[1.0, 0.0], 100).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn sqlite_nearest_dim_mismatch_skipped() {
        let mut store = open();
        store.set_embedding(&sym("dim2"), &[1.0_f32, 0.0]).unwrap();
        let results = store.nearest(&[1.0_f32, 0.0, 0.0], 5).unwrap();
        assert!(results.is_empty(), "dim-mismatch entries silently skipped");
    }

    #[test]
    fn sqlite_nearest_deterministic_ordering() {
        let mut store = open();
        let v = unit_vec(2, 0);
        store.set_embedding(&sym("z"), &v).unwrap();
        store.set_embedding(&sym("a"), &v).unwrap();
        let r1 = store.nearest(&v, 2).unwrap();
        let r2 = store.nearest(&v, 2).unwrap();
        let ids1: Vec<_> = r1.iter().map(|(id, _)| id.0.clone()).collect();
        let ids2: Vec<_> = r2.iter().map(|(id, _)| id.0.clone()).collect();
        assert_eq!(ids1, ids2, "identical calls must return identical order");
        assert_eq!(ids1[0], "a"); // tie broken by lex order
        assert_eq!(ids1[1], "z");
    }

    #[test]
    fn sqlite_capabilities_vector_search_true() {
        let store = open();
        assert!(store.capabilities().vector_search);
    }

    // ── Fix A: remove_file clears content + embeddings ───────────────────────

    fn make_node(symbol: &str, file: &str) -> wicked_estate_core::Node {
        wicked_estate_core::Node::new(
            SymbolId(symbol.to_string()),
            wicked_estate_core::NodeKind::Function,
            symbol,
            wicked_estate_core::Language::new("rust"),
            wicked_estate_core::Location::new(file, wicked_estate_core::Span::ZERO),
        )
    }

    #[test]
    fn sqlite_remove_file_clears_content_row() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        store
            .upsert_nodes(&[make_node("fn_a", "src/a.rs")])
            .unwrap();
        store.set_file_content("src/a.rs", "fn fn_a() {}").unwrap();

        // Verify content was stored.
        assert!(store.file_content("src/a.rs").unwrap().is_some());

        // Remove the file — the files row (and thus the join-path to content) is gone,
        // so file_content returns None even though the content row itself is still in the
        // content table (it becomes an orphan, reclaimed by compact).
        store.remove_file("src/a.rs").unwrap();
        assert!(
            store.file_content("src/a.rs").unwrap().is_none(),
            "file_content must return None when the files row is removed"
        );
    }

    #[test]
    fn sqlite_remove_file_clears_embeddings() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        let id = sym("fn_b");
        store
            .upsert_nodes(&[make_node("fn_b", "src/b.rs")])
            .unwrap();
        store.set_embedding(&id, &[1.0_f32, 0.0]).unwrap();

        assert!(store.embedding(&id).unwrap().is_some());

        store.remove_file("src/b.rs").unwrap();
        assert!(
            store.embedding(&id).unwrap().is_none(),
            "embedding row must be deleted when the owning file is removed"
        );
    }

    // ── prune_dangling_edges ─────────────────────────────────────────────────

    #[test]
    fn sqlite_prune_dangling_edges_removes_orphans_keeps_valid() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        let mut store = open();

        let a = sym("a");
        let b = sym("b");
        let ghost = sym("ghost");

        // Insert two nodes and a valid edge a→b.
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
        // A dangling edge: a → ghost (ghost never inserted as a node).
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
        assert_eq!(pruned, 1, "one dangling edge must be removed");

        let remaining = store.all_edges().unwrap();
        assert_eq!(remaining.len(), 1, "only the valid edge remains");
        assert_eq!(remaining[0].source, a);
        assert_eq!(remaining[0].target, b);
    }

    // ── compact ──────────────────────────────────────────────────────────────

    #[test]
    fn sqlite_compact_prunes_stale_cache_and_reports_stats() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();

        // Insert a node so the store is non-empty.
        store
            .upsert_nodes(&[make_node("fn_c", "src/c.rs")])
            .unwrap();
        store.set_file_content("src/c.rs", "fn fn_c() {}").unwrap();

        // Put a cache entry at version 0, then bump so it becomes stale.
        store.cache_put("old_key", "old_val").unwrap();
        store.bump_version().unwrap();
        // Put a fresh cache entry at the new version.
        store.cache_put("new_key", "new_val").unwrap();

        let stats = store.compact().unwrap();
        // The stale entry at version 0 must be pruned.
        assert_eq!(
            stats.stale_cache_rows, 1,
            "one stale cache row must be pruned"
        );
        // No dangling edges, orphan embeddings, orphan content, or history rows in this fixture.
        assert_eq!(stats.dangling_edges, 0);
        assert_eq!(stats.orphan_embeddings, 0);
        assert_eq!(stats.orphan_content, 0);
        assert_eq!(stats.history_rows_pruned, 0);

        // Fresh cache entry must still be readable.
        assert_eq!(
            store.cache_get("new_key").unwrap(),
            Some("new_val".to_string())
        );
    }

    #[test]
    fn sqlite_compact_prunes_orphan_embeddings_and_content() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        let mut store = open();

        let a = sym("a");
        let ghost = sym("ghost");

        store.upsert_nodes(&[make_node("a", "src/a.rs")]).unwrap();
        store.set_file_content("src/a.rs", "fn a() {}").unwrap();
        // Content for a file with no node (orphan): we manually insert a content row.
        // We cannot use set_file_content("src/orphan.rs", ...) because that now also creates a
        // files row, which prevents it from being treated as orphan content.
        // Instead insert directly into content table with a sha that no files row references.
        // The payload must be a valid zstd blob so the schema constraint (BLOB NOT NULL) holds.
        let orphan_blob = zstd::encode_all("// dead file".as_bytes(), 3).unwrap();
        store
            .conn
            .execute(
                "INSERT OR IGNORE INTO content(git_sha, blob) VALUES('deadbeef', ?1)",
                rusqlite::params![orphan_blob],
            )
            .unwrap();
        // Embedding for a → valid. Embedding for ghost → orphan.
        store.set_embedding(&a, &[1.0_f32, 0.0]).unwrap();
        store.set_embedding(&ghost, &[0.0_f32, 1.0]).unwrap();
        // Dangling edge.
        let dangling = Edge::new(
            a.clone(),
            ghost.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        store.upsert_edges(&[dangling]).unwrap();

        let stats = store.compact().unwrap();
        assert_eq!(stats.dangling_edges, 1, "dangling edge must be pruned");
        assert_eq!(stats.orphan_embeddings, 1, "ghost embedding must be pruned");
        assert_eq!(stats.orphan_content, 1, "orphan content row must be pruned");

        // Valid embedding and content must survive.
        assert!(store.embedding(&a).unwrap().is_some());
        assert!(store.file_content("src/a.rs").unwrap().is_some());
    }

    // ── Wave 7: git_blob_sha ─────────────────────────────────────────────────

    #[test]
    fn sqlite_git_blob_sha_matches_known_value() {
        // `echo -n hello | git hash-object --stdin` = b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0
        assert_eq!(
            git_blob_sha("hello"),
            "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0"
        );
    }

    // ── Wave 7: content-addressed set_file_content / file_git_sha ──────────

    #[test]
    fn sqlite_file_git_sha_after_set_file_content() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        store.set_file_content("src/hello.rs", "hello").unwrap();
        let sha = store
            .file_git_sha("src/hello.rs")
            .unwrap()
            .expect("sha must be set");
        assert_eq!(sha, "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0");
    }

    #[test]
    fn sqlite_content_dedup_identical_text() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        // Two files with identical content must share one content row.
        store.set_file_content("a.rs", "fn x() {}").unwrap();
        store.set_file_content("b.rs", "fn x() {}").unwrap();
        let sha_a = store.file_git_sha("a.rs").unwrap().unwrap();
        let sha_b = store.file_git_sha("b.rs").unwrap().unwrap();
        assert_eq!(sha_a, sha_b, "identical content → same git_sha");
        // Only one row in the content table.
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "identical content deduplicates to one content row"
        );
        // Both files readable.
        assert_eq!(
            store.file_content("a.rs").unwrap(),
            Some("fn x() {}".to_string())
        );
        assert_eq!(
            store.file_content("b.rs").unwrap(),
            Some("fn x() {}".to_string())
        );
    }

    #[test]
    fn sqlite_file_content_via_git_sha_join() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        store
            .set_file_content("src/lib.rs", "fn hello() {}")
            .unwrap();
        let got = store.file_content("src/lib.rs").unwrap();
        assert_eq!(got, Some("fn hello() {}".to_string()));
    }

    // ── Wave 7.1: changes_since ─────────────────────────────────────────────

    #[test]
    fn sqlite_changes_since_order_and_resume() {
        use wicked_estate_core::{ChangeOp, GraphWrite};
        let mut store = open();
        store.log_change(ChangeOp::Upsert, "a.rs").unwrap();
        store.log_change(ChangeOp::Upsert, "b.rs").unwrap();
        store.log_change(ChangeOp::Remove, "c.rs").unwrap();

        // From cursor=0: all three in seq order.
        let all = store.changes_since(0).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].target, "a.rs");
        assert_eq!(all[1].target, "b.rs");
        assert_eq!(all[2].target, "c.rs");
        assert_eq!(all[2].op, ChangeOp::Remove);

        // Resume: from cursor of the second entry, get only the third.
        let after = store.changes_since(all[1].seq).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].target, "c.rs");
    }

    // ── Wave 7: repo_info round-trip ─────────────────────────────────────────

    #[test]
    fn sqlite_repo_info_roundtrip() {
        use wicked_estate_core::{GraphWrite, RepoInfo};
        let mut store = open();

        // Before any set_repo_info, returns None.
        assert!(store.repo_info().unwrap().is_none());

        let info = RepoInfo {
            commit: Some("abc123".to_string()),
            branch: Some("main".to_string()),
            remote: Some("https://github.com/example/repo".to_string()),
            dirty: true,
        };
        store.set_repo_info(&info).unwrap();
        let got = store.repo_info().unwrap().expect("must be Some after set");
        assert_eq!(got.commit, Some("abc123".to_string()));
        assert_eq!(got.branch, Some("main".to_string()));
        assert_eq!(
            got.remote,
            Some("https://github.com/example/repo".to_string())
        );
        assert!(got.dirty);
    }

    // ── Wave 7: edge_history archival ────────────────────────────────────────

    #[test]
    fn sqlite_edge_history_archived_on_remove_file() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        let mut store = open();
        // history must be ON to assert archival behaviour (default is now OFF).
        store.set_history_enabled(true).unwrap();

        // Set file content (establishes git_sha v1).
        let v1_text = "fn foo() {}";
        store.set_file_content("src/foo.rs", v1_text).unwrap();
        let v1_sha = store.file_git_sha("src/foo.rs").unwrap().unwrap();

        // Upsert a node and an edge for that file.
        store
            .upsert_nodes(&[make_node("foo", "src/foo.rs")])
            .unwrap();
        store
            .upsert_nodes(&[make_node("bar", "src/bar.rs")])
            .unwrap();
        let e = Edge::new(
            sym("foo"),
            sym("bar"),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        store.upsert_edges(&[e]).unwrap();

        // Remove the file — the edge must be archived into edge_history.
        store.remove_file("src/foo.rs").unwrap();

        let history = store.edge_history("src/foo.rs").unwrap();
        assert_eq!(history.len(), 1, "one superseded edge must be in history");
        assert_eq!(
            history[0].git_sha, v1_sha,
            "archived edge must carry the prior git_sha"
        );
        assert_eq!(history[0].edge.source, sym("foo"));
    }

    // ── Wave 7: edge_history retention prune ─────────────────────────────────

    #[test]
    fn sqlite_compact_prunes_edge_history_beyond_retention() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        let mut store = open();
        // history must be ON to populate edge_history via remove_file.
        store.set_history_enabled(true).unwrap();

        // Simulate 25 versions of src/ver.rs by cycling: set content, upsert node+edge,
        // remove_file (archives the edge), repeat.
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
                sym("ver_fn"),
                sym("target"),
                EdgeKind::Calls,
                ResolutionTier::Parsed,
                "test",
            );
            store.upsert_edges(&[e]).unwrap();
            store.remove_file("src/ver.rs").unwrap();
        }

        let before_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM edge_history WHERE file='src/ver.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before_count, 25, "all 25 versions archived");

        let stats = store.compact().unwrap();
        assert_eq!(
            stats.history_rows_pruned, 5,
            "25 - 20 = 5 older rows must be pruned"
        );

        let after_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM edge_history WHERE file='src/ver.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(after_count, 20, "exactly 20 newest rows must remain");
    }

    // ── Wave 7: content retained while referenced by edge_history ───────────

    #[test]
    fn sqlite_orphan_content_retained_while_in_edge_history() {
        use wicked_estate_core::{Edge, EdgeKind, GraphWrite, ResolutionTier};
        let mut store = open();
        // history must be ON so remove_file archives the edge and records the sha.
        store.set_history_enabled(true).unwrap();

        let v1_text = "fn v1() {}";
        store.set_file_content("src/h.rs", v1_text).unwrap();
        let sha = store.file_git_sha("src/h.rs").unwrap().unwrap();

        store
            .upsert_nodes(&[make_node("h_fn", "src/h.rs")])
            .unwrap();
        store
            .upsert_nodes(&[make_node("other", "src/o.rs")])
            .unwrap();
        let e = Edge::new(
            sym("h_fn"),
            sym("other"),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        store.upsert_edges(&[e]).unwrap();

        // Remove the file — files row gone, edge archived with sha, content row becomes orphan
        // from the files perspective, but edge_history references it.
        store.remove_file("src/h.rs").unwrap();

        // Compact must NOT prune the content row since edge_history still references its sha.
        let stats = store.compact().unwrap();
        assert_eq!(
            stats.orphan_content, 0,
            "content still referenced by edge_history must survive compact"
        );

        let still_there: Option<Vec<u8>> = store
            .conn
            .query_row(
                "SELECT blob FROM content WHERE git_sha=?1",
                params![sha],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()
            .unwrap();
        assert!(still_there.is_some(), "content row must survive");
    }

    // ── W11 slim: unresolved_refs typed columns round-trip ───────────────────

    #[test]
    fn sqlite_unresolved_refs_roundtrip_typed_columns() {
        use wicked_estate_core::{EdgeKind, GraphWrite, Location, Span, UnresolvedRef};
        let mut store = open();

        let from = sym("a");
        let r = UnresolvedRef::new(
            from.clone(),
            "foo_func",
            EdgeKind::Calls,
            Location::new(
                "src/lib.rs",
                Span {
                    start_line: 42,
                    ..Span::ZERO
                },
            ),
        );
        store.upsert_unresolved_refs(&[r]).unwrap();

        let found = store.unresolved_refs_for_name("foo_func").unwrap();
        assert_eq!(found.len(), 1, "one ref must be returned");
        assert_eq!(found[0].from, from, "from_sym round-trips correctly");
        assert_eq!(found[0].raw_name, "foo_func");
        assert_eq!(found[0].kind, EdgeKind::Calls);
        assert_eq!(found[0].location.file, "src/lib.rs");
        assert_eq!(found[0].location.span.start_line, 42);

        let stats = store.stats().unwrap();
        assert_eq!(
            stats.unresolved_ref_count, 1,
            "stats count matches inserted row"
        );
    }

    #[test]
    fn sqlite_unresolved_refs_count_only_no_data_column() {
        // Verify the table schema has no `data` column (the old fat-blob column).
        // If the column existed, a SELECT of it would succeed; expecting an error proves it's gone.
        use wicked_estate_core::{EdgeKind, GraphWrite, Location, Span, UnresolvedRef};
        let mut store = open();
        let r = UnresolvedRef::new(
            sym("b"),
            "bar",
            EdgeKind::Imports,
            Location::new("src/b.rs", Span::ZERO),
        );
        store.upsert_unresolved_refs(&[r]).unwrap();

        let result = store
            .conn
            .query_row("SELECT data FROM unresolved_refs LIMIT 1", [], |r| {
                r.get::<_, String>(0)
            });
        assert!(
            result.is_err(),
            "data column must not exist in the new schema"
        );
    }

    // ── W11 slim: content zstd compress/decompress round-trip ────────────────

    #[test]
    fn sqlite_content_zstd_roundtrip() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        let src = "fn x() { let y = 1; y }";
        store.set_file_content("src/x.rs", src).unwrap();
        let got = store.file_content("src/x.rs").unwrap();
        assert_eq!(
            got.as_deref(),
            Some(src),
            "decompressed content must equal original"
        );
    }

    #[test]
    fn sqlite_content_zstd_dedup_same_sha() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        let src = "fn x() {}";
        store.set_file_content("a.rs", src).unwrap();
        store.set_file_content("b.rs", src).unwrap();
        let sha_a = store.file_git_sha("a.rs").unwrap().unwrap();
        let sha_b = store.file_git_sha("b.rs").unwrap().unwrap();
        assert_eq!(sha_a, sha_b, "identical content must share one git_sha");
        // Only one blob row.
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "identical content deduplicates to one content row"
        );
        // Both readable.
        assert_eq!(store.file_content("a.rs").unwrap().as_deref(), Some(src));
        assert_eq!(store.file_content("b.rs").unwrap().as_deref(), Some(src));
    }

    // ── Symbol-string interning: round-trip correctness ──────────────────────
    //
    // Interning is a pure storage-representation change: integer sids are stored
    // in nodes/edges/unresolved_refs, but all public API entry-points (get_node,
    // neighbors, traverse, unresolved_refs_for_name) must return the ORIGINAL
    // string SymbolId unchanged.  This test is the falsifier for that invariant.
    //
    // Footprint note: a real 154 MB store with N distinct symbols stores each
    // symbol string ONCE in the `symbols` table (≈N * avg_sym_len bytes) instead
    // of K occurrences across nodes + edges + unresolved_refs rows.  For a typical
    // repo (avg sym len ≈ 40 chars, K ≈ 5 occurrences per symbol) interning saves
    // ≈ (K-1)/K * 80% of sym-column bytes (80% because only index columns shrink;
    // the JSON `data` blob still carries the string for deserialization).
    //
    // The symbols table itself: N rows * (8-byte sid + avg_sym_len TEXT) — usually
    // well under 10 MB for a 100k-symbol repo, while the per-row INTEGER columns
    // save 4-8× on edge/unresolved_refs tables that dominate row count.

    #[test]
    fn sqlite_interning_roundtrip_preserves_symbol_strings() {
        use wicked_estate_core::{EdgeKind, Location, ResolutionTier, Span};

        let mut store = open();

        let sym_a = SymbolId("my::module::FunctionA".to_string());
        let sym_b = SymbolId("my::module::FunctionB".to_string());

        // Upsert node A and node B.
        store
            .upsert_nodes(&[
                make_node(&sym_a.0, "src/a.rs"),
                make_node(&sym_b.0, "src/b.rs"),
            ])
            .unwrap();

        // Upsert edge A → B.
        let edge = Edge::new(
            sym_a.clone(),
            sym_b.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );
        store.upsert_edges(&[edge]).unwrap();

        // Upsert an unresolved ref from A.
        let uref = UnresolvedRef::new(
            sym_a.clone(),
            "SomeExternalFunc",
            EdgeKind::Calls,
            Location::new(
                "src/a.rs",
                Span {
                    start_line: 7,
                    ..Span::ZERO
                },
            ),
        );
        store.upsert_unresolved_refs(&[uref]).unwrap();

        // --- get_node must return the exact string SymbolId ---
        let node_a = store
            .get_node(&sym_a)
            .unwrap()
            .expect("node A must be found");
        assert_eq!(node_a.symbol, sym_a, "get_node: SymbolId round-trips");

        // --- neighbors (Dependents) must return edge with correct string SymbolIds ---
        let deps = store.neighbors(&sym_b, Direction::Dependents).unwrap();
        assert_eq!(deps.len(), 1, "one dependent edge expected");
        assert_eq!(deps[0].source, sym_a, "neighbors: edge.source round-trips");
        assert_eq!(deps[0].target, sym_b, "neighbors: edge.target round-trips");

        // --- traverse blast-radius from sym_b: sym_a should be in the Dependents subgraph ---
        let spec = TraversalSpec {
            direction: Direction::Dependents,
            max_depth: 3,
            max_nodes: 100,
            min_confidence: 0.0,
            edge_kinds: vec![],
        };
        let sg = store.traverse(&sym_b, &spec).unwrap();
        let found_sym: Vec<_> = sg.nodes.iter().map(|n| n.symbol.0.as_str()).collect();
        assert!(
            found_sym.contains(&sym_a.0.as_str()),
            "traverse: sym_a must appear in blast-radius of sym_b; got {:?}",
            found_sym
        );
        // depths map keys must be string SymbolIds, not numeric strings.
        for key in sg.depths.keys() {
            assert!(
                !key.chars().all(|c| c.is_ascii_digit()),
                "traverse: depths key must be a string SymbolId, got numeric {:?}",
                key
            );
        }

        // --- unresolved_refs_for_name must return correct from SymbolId ---
        let refs = store.unresolved_refs_for_name("SomeExternalFunc").unwrap();
        assert_eq!(refs.len(), 1, "one unresolved ref expected");
        assert_eq!(
            refs[0].from, sym_a,
            "unresolved_refs_for_name: from SymbolId round-trips"
        );

        // --- verify the symbols table actually has exactly 2 rows (dedup) ---
        let sym_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            sym_count, 2,
            "symbols table must have exactly 2 rows (A and B)"
        );

        // --- verify nodes.symbol stores INTEGER, not TEXT ---
        // If interning works, nodes.symbol holds a small integer; fetching it as i64 must succeed
        // and produce a positive value.
        let node_sym_int: i64 = store
            .conn
            .query_row(
                "SELECT n.symbol FROM nodes n \
                 JOIN symbols s ON s.sid = n.symbol \
                 WHERE s.sym = ?1",
                rusqlite::params![sym_a.0],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            node_sym_int > 0,
            "nodes.symbol must be a positive integer sid"
        );
    }

    // ── Semantic linking (SqliteStore) ───────────────────────────────────────

    #[test]
    fn sqlite_node_semantics_absent_before_annotation() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        store
            .upsert_nodes(&[make_node("fn_a", "src/a.rs")])
            .unwrap();
        let got = store.node_semantics(&sym("fn_a")).unwrap();
        assert!(
            got.is_none(),
            "node_semantics must be None before any annotation is written"
        );
    }

    #[test]
    fn sqlite_node_semantics_full_roundtrip() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
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
    fn sqlite_node_semantics_partial_update_preserves_untouched_fields() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        store
            .upsert_nodes(&[make_node("fn_c", "src/c.rs")])
            .unwrap();
        // Full write first.
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
    fn sqlite_find_by_requirement_returns_annotated_nodes() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
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
    fn sqlite_set_node_semantics_absent_symbol_noop() {
        let mut store = open();
        // Symbol was never upserted — should not error.
        store
            .set_node_semantics(&sym("ghost"), Some("desc"), Some("REQ-1"), Some(false))
            .unwrap();
        assert!(
            store.node_semantics(&sym("ghost")).unwrap().is_none(),
            "absent symbol must remain without semantics"
        );
    }

    #[test]
    fn sqlite_set_node_semantics_all_none_noop() {
        use wicked_estate_core::GraphWrite;
        let mut store = open();
        store
            .upsert_nodes(&[make_node("fn_d", "src/d.rs")])
            .unwrap();
        // All-None call must be a no-op and must not error.
        store
            .set_node_semantics(&sym("fn_d"), None, None, None)
            .unwrap();
        assert!(
            store.node_semantics(&sym("fn_d")).unwrap().is_none(),
            "all-None call must leave semantics as None"
        );
    }
}
