//! `PostgresStore` — Postgres-backed [`GraphStore`] implementation.
//!
//! Uses `sqlx` with the postgres feature for connection pooling.  The `GraphStore` trait is
//! synchronous, so every async sqlx call is wrapped via [`rt_block`] which delegates to the
//! current Tokio runtime (via `block_in_place`) or creates a one-shot runtime when called from
//! a non-async context.
//!
//! Schema mirrors `SqliteStore` but uses Postgres-native types:
//! - `BIGSERIAL` primary keys where SQLite uses `INTEGER PRIMARY KEY AUTOINCREMENT`
//! - `TEXT` for all symbol strings (no integer interning — Postgres handles string dedup well)
//! - `REAL` for confidence (same as SQLite)
//! - `TEXT` for JSON columns (same round-trip fidelity as SQLite)
//! - No zstd compression for content — Postgres applies page-level compression internally
//! - Full-text search via `ILIKE` (upgrade to `pg_trgm` / `tsvector` in a future pass)

use sha1::{Digest as Sha1Digest, Sha1};
use sqlx::Row;
use std::collections::BTreeMap;
use wicked_estate_core::{
    Annotation, Change, ChangeOp, Direction, Edge, Error, GraphRead, GraphStats, GraphWrite,
    HistoricalEdge, Node, NodeKind, NodeSemantics, RepoInfo, Result, StoreCapabilities, Subgraph,
    SymbolId, SymbolIndex, SymbolQuery, TraversalSpec, UnresolvedRef,
};

// ── Error helper ─────────────────────────────────────────────────────────────

fn st<E: std::fmt::Display>(e: E) -> Error {
    Error::Storage(e.to_string())
}

// ── Sync runtime bridge ───────────────────────────────────────────────────────

/// Lazily-initialised, process-wide Tokio runtime used when `PostgresStore` is called from
/// outside any existing async context (e.g. unit tests, CLI entry points).
///
/// Storing the runtime *in* `PostgresStore` is tricky because dropping `Runtime` before the
/// pool would cancel all pending async tasks.  Using a process-wide runtime means the pool's
/// background keepalive tasks survive across multiple `PostgresStore` instances and `rt_block`
/// calls without each call spinning up/tearing down a fresh runtime.
fn global_rt() -> &'static tokio::runtime::Runtime {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime for PostgresStore")
    })
}

/// Run an async future synchronously.
///
/// - Inside a multi-thread Tokio runtime: uses `block_in_place` to avoid blocking the executor
///   thread.
/// - Inside a `current_thread` runtime (e.g. `#[tokio::test]`): `block_in_place` panics on
///   single-threaded runtimes, so we fall back to the process-wide global runtime instead.
/// - Outside any Tokio runtime: uses the process-wide global runtime directly.
fn rt_block<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    use tokio::runtime::RuntimeFlavor;
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(f))
        }
        _ => global_rt().block_on(f),
    }
}

// ── Git blob SHA (same as SqliteStore) ───────────────────────────────────────

/// Compute the git blob SHA for `text`: `hex(SHA1("blob " + byte_len + "\0" + text))`.
fn git_blob_sha(text: &str) -> String {
    let bytes = text.as_bytes();
    let header = format!("blob {}\0", bytes.len());
    let mut h = Sha1::new();
    h.update(header.as_bytes());
    h.update(bytes);
    format!("{:x}", h.finalize())
}

// ── Annotation row decode ─────────────────────────────────────────────────────

/// Decode a `PgRow` carrying the standard annotation columns (no `node_sym`) into an
/// [`Annotation`]. `confidence` is stored as Postgres `REAL` (f32) and widened to the struct's
/// `f64`; `ts` / `last_verified` are `BIGINT` (i64). Mirrors the column order the read queries
/// select. Used by all three annotation read methods so the mapping lives in one place.
fn row_to_annotation(r: &sqlx::postgres::PgRow) -> std::result::Result<Annotation, sqlx::Error> {
    let confidence: f32 = r.try_get("confidence")?;
    Ok(Annotation {
        key: r.try_get("key")?,
        value: r.try_get("value")?,
        confidence: confidence as f64,
        provenance: r.try_get("provenance")?,
        author: r.try_get("author")?,
        ts: r.try_get("ts")?,
        r#type: r.try_get("type")?,
        source_type: r.try_get("source_type")?,
        extraction_method: r.try_get("extraction_method")?,
        last_verified: r.try_get("last_verified")?,
    })
}

// ── Schema DDL ────────────────────────────────────────────────────────────────

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS files (
  path    TEXT PRIMARY KEY,
  digest  TEXT NOT NULL DEFAULT '',
  git_sha TEXT
);

CREATE TABLE IF NOT EXISTS nodes (
  symbol                TEXT PRIMARY KEY,
  name                  TEXT NOT NULL,
  kind                  TEXT NOT NULL,
  language              TEXT NOT NULL,
  file                  TEXT NOT NULL DEFAULT '',
  data                  TEXT NOT NULL,
  description           TEXT,
  requirement           TEXT,
  requirement_validated BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(file);

CREATE TABLE IF NOT EXISTS edges (
  source     TEXT NOT NULL,
  target     TEXT NOT NULL,
  kind       TEXT NOT NULL,
  confidence REAL NOT NULL,
  file       TEXT NOT NULL DEFAULT '',
  data       TEXT NOT NULL,
  PRIMARY KEY (source, target, kind)
);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
CREATE INDEX IF NOT EXISTS idx_edges_file   ON edges(file);

CREATE TABLE IF NOT EXISTS unresolved_refs (
  id       BIGSERIAL PRIMARY KEY,
  from_sym TEXT NOT NULL,
  raw_name TEXT NOT NULL,
  kind     TEXT NOT NULL,
  file     TEXT NOT NULL DEFAULT '',
  line     BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_unresolved_refs_name ON unresolved_refs(raw_name);
CREATE INDEX IF NOT EXISTS idx_unresolved_refs_file ON unresolved_refs(file);

CREATE TABLE IF NOT EXISTS content (
  git_sha TEXT PRIMARY KEY,
  body    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cache (
  key     TEXT   NOT NULL,
  version BIGINT NOT NULL,
  value   TEXT   NOT NULL,
  PRIMARY KEY (key, version)
);

CREATE TABLE IF NOT EXISTS meta (
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS changes (
  seq    BIGSERIAL PRIMARY KEY,
  op     TEXT   NOT NULL,
  target TEXT   NOT NULL,
  ts     BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT
);

CREATE TABLE IF NOT EXISTS edge_history (
  archived_seq BIGSERIAL PRIMARY KEY,
  git_sha      TEXT NOT NULL DEFAULT '',
  file         TEXT NOT NULL,
  edge_json    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_edge_history_file ON edge_history(file);

-- Annotation store: external agents/tools/humans tag any indexed symbol with typed metadata.
-- Mirrors SqliteStore's `annotations` table, but `node_sym` is the TEXT symbol id (FK → nodes.symbol)
-- since Postgres does not intern symbols to integer sids. `type` is a plain string discriminator
-- (NO enum): a known convention OR an arbitrary custom type — stored/queried identically
-- (rules-as-DATA). Evidence envelope (additive): `source_type` (what KIND of source), `extraction_method`
-- (by what method), `last_verified` (freshness clock, Unix-seconds; distinct from `ts` write-time;
-- 0 = never verified). Defaults match the struct ('unspecified' / 'manual' / 0).
CREATE TABLE IF NOT EXISTS annotations (
  id                BIGSERIAL PRIMARY KEY,
  node_sym          TEXT    NOT NULL,
  key               TEXT    NOT NULL,
  value             TEXT    NOT NULL,
  confidence        REAL    NOT NULL DEFAULT 1.0,
  provenance        TEXT    NOT NULL DEFAULT '',
  author            TEXT    NOT NULL DEFAULT '',
  ts                BIGINT  NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT,
  type              TEXT    NOT NULL DEFAULT 'note',
  source_type       TEXT    NOT NULL DEFAULT 'unspecified',
  extraction_method TEXT    NOT NULL DEFAULT 'manual',
  last_verified     BIGINT  NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_annotations_node ON annotations(node_sym);
CREATE INDEX IF NOT EXISTS idx_annotations_key  ON annotations(key);
CREATE INDEX IF NOT EXISTS idx_annotations_type ON annotations(type);
CREATE INDEX IF NOT EXISTS idx_annotations_last_verified ON annotations(last_verified);
"#;

// ── PostgresStore ─────────────────────────────────────────────────────────────

/// Postgres-backed graph store. Satisfies [`GraphRead`] + [`GraphWrite`] (and therefore
/// [`GraphStore`]).  Connects via `sqlx::PgPool`; every method blocks on the async layer
/// using [`rt_block`].
pub struct PostgresStore {
    pool: sqlx::PgPool,
    in_batch: bool,
    history_enabled: bool,
}

impl PostgresStore {
    /// Open (or create) a Postgres graph store at `url`.
    ///
    /// Runs the schema DDL (`CREATE TABLE IF NOT EXISTS ...`) and inserts the initial
    /// `graph_version=0` meta row if absent.
    pub fn open(url: &str) -> Result<Self> {
        let pool = rt_block(sqlx::PgPool::connect(url)).map_err(st)?;

        // Run schema DDL statement by statement (split on ";") to avoid parsing issues.
        rt_block(async {
            for stmt in SCHEMA.split(';') {
                let trimmed = stmt.trim();
                if trimmed.is_empty() {
                    continue;
                }
                sqlx::query(trimmed).execute(&pool).await?;
            }
            Ok::<_, sqlx::Error>(())
        })
        .map_err(st)?;

        // Insert initial graph_version if absent.
        rt_block(
            sqlx::query(
                "INSERT INTO meta(k, v) VALUES('graph_version', '0') ON CONFLICT DO NOTHING",
            )
            .execute(&pool),
        )
        .map_err(st)?;

        // Read history_enabled from meta (absent → OFF).
        let history_enabled = rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT v FROM meta WHERE k = 'history_enabled'")
                    .fetch_optional(&pool)
                    .await?;
            Ok::<bool, sqlx::Error>(
                row.is_some_and(|r| r.try_get::<String, _>("v").ok().is_some_and(|v| v == "1")),
            )
        })
        .map_err(st)?;

        Ok(Self {
            pool,
            in_batch: false,
            history_enabled,
        })
    }

    // ── meta helpers ────────────────────────────────────────────────────────

    /// Read an arbitrary string value from the `meta` table. Returns `None` when absent.
    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query("SELECT v FROM meta WHERE k = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
            Ok::<Option<String>, sqlx::Error>(row.and_then(|r| r.try_get("v").ok()))
        })
        .map_err(st)
    }

    /// Write an arbitrary string value to the `meta` table (insert or replace).
    pub fn meta_set(&mut self, key: &str, value: &str) -> Result<()> {
        rt_block(
            sqlx::query(
                "INSERT INTO meta(k, v) VALUES($1, $2) \
                 ON CONFLICT(k) DO UPDATE SET v = EXCLUDED.v",
            )
            .bind(key)
            .bind(value)
            .execute(&self.pool),
        )
        .map_err(st)?;
        Ok(())
    }

    /// Current graph version (integer stored in `meta`).
    fn graph_version(&self) -> Result<i64> {
        let v = self
            .meta_get("graph_version")?
            .unwrap_or_else(|| "0".to_string());
        v.parse::<i64>().map_err(st)
    }

    /// Increment the graph version. All cache entries at prior versions become stale.
    pub fn bump_version(&mut self) -> Result<()> {
        rt_block(
            sqlx::query("UPDATE meta SET v = (v::BIGINT + 1)::TEXT WHERE k = 'graph_version'")
                .execute(&self.pool),
        )
        .map_err(st)?;
        Ok(())
    }

    // ── cache helpers ────────────────────────────────────────────────────────

    /// Return the cached value for `key` only if stored at the current graph version.
    pub fn cache_get(&self, key: &str) -> Result<Option<String>> {
        let ver = self.graph_version()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT value FROM cache WHERE key = $1 AND version = $2")
                    .bind(key)
                    .bind(ver)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok::<Option<String>, sqlx::Error>(row.and_then(|r| r.try_get("value").ok()))
        })
        .map_err(st)
    }

    /// Store `value` for `key` at the current graph version.
    pub fn cache_put(&mut self, key: &str, value: &str) -> Result<()> {
        let ver = self.graph_version()?;
        rt_block(
            sqlx::query(
                "INSERT INTO cache(key, version, value) VALUES($1, $2, $3) \
                 ON CONFLICT(key, version) DO UPDATE SET value = EXCLUDED.value",
            )
            .bind(key)
            .bind(ver)
            .bind(value)
            .execute(&self.pool),
        )
        .map_err(st)?;
        Ok(())
    }

    /// Enable or disable edge-history archival (default: `false`).
    pub fn set_history_enabled(&mut self, on: bool) -> Result<()> {
        self.history_enabled = on;
        self.meta_set("history_enabled", if on { "1" } else { "0" })?;
        Ok(())
    }

    // ── recursive CTE traversal ──────────────────────────────────────────────

    fn cte_reach(
        &self,
        start: &SymbolId,
        dir: Direction,
        spec: &TraversalSpec,
    ) -> Result<BTreeMap<String, u32>> {
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

        // Postgres WITH RECURSIVE on TEXT columns directly (no integer interning needed).
        let sql = format!(
            "WITH RECURSIVE walk(id, depth) AS (
                 SELECT $1::TEXT, 0
                 UNION
                 SELECT e.{advance_col}, walk.depth + 1
                   FROM edges e JOIN walk ON e.{match_col} = walk.id
                  WHERE walk.depth < $2 AND e.confidence >= $3 {kind_filter}
             )
             SELECT id, MIN(depth) AS min_depth
               FROM walk
              WHERE id <> $1
              GROUP BY id
              ORDER BY 2
              LIMIT $4"
        );

        // Fetch max_nodes + 1 rows so we can distinguish "exactly max_nodes reachable" from
        // "truncated" — if we get more than max_nodes back, the result was cut.
        let rows = rt_block(async {
            sqlx::query(&sql)
                .bind(&start.0)
                .bind(spec.max_depth as i64)
                .bind(spec.min_confidence as f64)
                .bind((spec.max_nodes as i64) + 1)
                .fetch_all(&self.pool)
                .await
        })
        .map_err(st)?;

        let mut out = BTreeMap::new();
        for row in rows {
            let id: String = row.try_get("id").map_err(st)?;
            let depth: i32 = row.try_get("min_depth").map_err(st)?;
            out.insert(id, depth as u32);
        }
        Ok(out)
    }
}

// ── GraphWrite ────────────────────────────────────────────────────────────────

impl GraphWrite for PostgresStore {
    fn begin_batch(&mut self) -> Result<()> {
        // Postgres auto-commits per statement via connection pool; begin_batch is a no-op
        // for the conformance contract.  The in_batch flag is tracked for correctness.
        self.in_batch = true;
        Ok(())
    }

    fn commit_batch(&mut self) -> Result<()> {
        self.in_batch = false;
        Ok(())
    }

    fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()> {
        for n in nodes {
            let kind = serde_json::to_string(&n.kind)?;
            let data = serde_json::to_string(n)?;
            let file = &n.location.file;
            rt_block(
                sqlx::query(
                    "INSERT INTO nodes(symbol, name, kind, language, file, data)
                     VALUES($1, $2, $3, $4, $5, $6)
                     ON CONFLICT(symbol) DO UPDATE SET
                       name     = EXCLUDED.name,
                       kind     = EXCLUDED.kind,
                       language = EXCLUDED.language,
                       file     = EXCLUDED.file,
                       data     = EXCLUDED.data",
                )
                .bind(&n.symbol.0)
                .bind(&n.name)
                .bind(&kind)
                .bind(&n.language.0)
                .bind(file)
                .bind(&data)
                .execute(&self.pool),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()> {
        for e in edges {
            let kind = serde_json::to_string(&e.kind)?;
            let data = serde_json::to_string(e)?;
            let file = e.location.as_ref().map(|l| l.file.as_str()).unwrap_or("");
            let confidence = e.confidence.get() as f64;
            rt_block(
                sqlx::query(
                    "INSERT INTO edges(source, target, kind, confidence, file, data)
                     VALUES($1, $2, $3, $4, $5, $6)
                     ON CONFLICT(source, target, kind) DO UPDATE SET
                       confidence = EXCLUDED.confidence,
                       file       = EXCLUDED.file,
                       data       = EXCLUDED.data
                     WHERE EXCLUDED.confidence >= edges.confidence",
                )
                .bind(&e.source.0)
                .bind(&e.target.0)
                .bind(&kind)
                .bind(confidence)
                .bind(file)
                .bind(&data)
                .execute(&self.pool),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()> {
        for r in refs {
            let kind = serde_json::to_string(&r.kind)?;
            let file = &r.location.file;
            let line = r.location.span.start_line as i64;
            rt_block(
                sqlx::query(
                    "INSERT INTO unresolved_refs(from_sym, raw_name, kind, file, line)
                     VALUES($1, $2, $3, $4, $5)",
                )
                .bind(&r.from.0)
                .bind(&r.raw_name)
                .bind(&kind)
                .bind(file)
                .bind(line)
                .execute(&self.pool),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn remove_file(&mut self, file: &str) -> Result<()> {
        // Step 1: read the file's current git_sha.
        let current_git_sha: String = rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT git_sha FROM files WHERE path = $1")
                    .bind(file)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok::<String, sqlx::Error>(
                row.and_then(|r| r.try_get::<Option<String>, _>("git_sha").ok().flatten())
                    .unwrap_or_default(),
            )
        })
        .map_err(st)?;

        // Step 2: archive edges to edge_history if history is enabled.
        if self.history_enabled {
            let edge_jsons: Vec<String> = rt_block(async {
                let rows = sqlx::query(
                    "SELECT data FROM edges \
                     WHERE file = $1 \
                        OR source IN (SELECT symbol FROM nodes WHERE file = $1)",
                )
                .bind(file)
                .fetch_all(&self.pool)
                .await?;
                let mut v = Vec::new();
                for row in rows {
                    v.push(row.try_get::<String, _>("data")?);
                }
                Ok::<Vec<String>, sqlx::Error>(v)
            })
            .map_err(st)?;

            for edge_json in &edge_jsons {
                rt_block(
                    sqlx::query(
                        "INSERT INTO edge_history(git_sha, file, edge_json) VALUES($1, $2, $3)",
                    )
                    .bind(&current_git_sha)
                    .bind(file)
                    .bind(edge_json)
                    .execute(&self.pool),
                )
                .map_err(st)?;
            }
        }

        // Step 3: delete edges BEFORE nodes (subquery on nodes must still be valid).
        rt_block(
            sqlx::query(
                "DELETE FROM edges \
                 WHERE file = $1 \
                    OR source IN (SELECT symbol FROM nodes WHERE file = $1)",
            )
            .bind(file)
            .execute(&self.pool),
        )
        .map_err(st)?;

        // Step 4: delete nodes, unresolved_refs, files row.
        rt_block(
            sqlx::query("DELETE FROM nodes WHERE file = $1")
                .bind(file)
                .execute(&self.pool),
        )
        .map_err(st)?;

        rt_block(
            sqlx::query("DELETE FROM unresolved_refs WHERE file = $1")
                .bind(file)
                .execute(&self.pool),
        )
        .map_err(st)?;

        rt_block(
            sqlx::query("DELETE FROM files WHERE path = $1")
                .bind(file)
                .execute(&self.pool),
        )
        .map_err(st)?;

        Ok(())
    }

    fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()> {
        rt_block(
            sqlx::query(
                "INSERT INTO files(path, digest) VALUES($1, $2) \
                 ON CONFLICT(path) DO UPDATE SET digest = EXCLUDED.digest",
            )
            .bind(file)
            .bind(digest)
            .execute(&self.pool),
        )
        .map_err(st)?;
        Ok(())
    }

    fn set_file_content(&mut self, file: &str, text: &str) -> Result<()> {
        let sha = git_blob_sha(text);
        // Dedup: INSERT … ON CONFLICT DO NOTHING — identical content shares one content row.
        rt_block(
            sqlx::query("INSERT INTO content(git_sha, body) VALUES($1, $2) ON CONFLICT DO NOTHING")
                .bind(&sha)
                .bind(text)
                .execute(&self.pool),
        )
        .map_err(st)?;
        // Upsert the files row with the git_sha pointer.
        rt_block(
            sqlx::query(
                "INSERT INTO files(path, digest, git_sha) VALUES($1, '', $2) \
                 ON CONFLICT(path) DO UPDATE SET git_sha = EXCLUDED.git_sha",
            )
            .bind(file)
            .bind(&sha)
            .execute(&self.pool),
        )
        .map_err(st)?;
        Ok(())
    }

    fn prune_dangling_edges(&mut self) -> Result<usize> {
        let result = rt_block(
            sqlx::query(
                "DELETE FROM edges \
                 WHERE source NOT IN (SELECT symbol FROM nodes) \
                    OR target NOT IN (SELECT symbol FROM nodes)",
            )
            .execute(&self.pool),
        )
        .map_err(st)?;
        Ok(result.rows_affected() as usize)
    }

    fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()> {
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
        rt_block(
            sqlx::query("INSERT INTO changes(op, target) VALUES($1, $2)")
                .bind(op_str)
                .bind(target)
                .execute(&self.pool),
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
        if description.is_none() && requirement.is_none() && requirement_validated.is_none() {
            return Ok(());
        }
        // Check the symbol exists (no intern for Postgres — directly query nodes).
        let exists: bool = rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT 1 AS exists_flag FROM nodes WHERE symbol = $1")
                    .bind(&symbol.0)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok::<bool, sqlx::Error>(row.is_some())
        })
        .map_err(st)?;
        if !exists {
            return Ok(());
        }

        if let Some(d) = description {
            rt_block(
                sqlx::query("UPDATE nodes SET description = $2 WHERE symbol = $1")
                    .bind(&symbol.0)
                    .bind(d)
                    .execute(&self.pool),
            )
            .map_err(st)?;
        }
        if let Some(r) = requirement {
            rt_block(
                sqlx::query("UPDATE nodes SET requirement = $2 WHERE symbol = $1")
                    .bind(&symbol.0)
                    .bind(r)
                    .execute(&self.pool),
            )
            .map_err(st)?;
        }
        if let Some(v) = requirement_validated {
            let flag: i64 = v as i64;
            rt_block(
                sqlx::query("UPDATE nodes SET requirement_validated = $2 WHERE symbol = $1")
                    .bind(&symbol.0)
                    .bind(flag)
                    .execute(&self.pool),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn annotate(&mut self, symbol: &SymbolId, annotation: Annotation) -> Result<()> {
        rt_block(async {
            // An un-interned symbol is not a node → no-op (mirrors SqliteStore).
            let exists: bool = sqlx::query("SELECT 1 AS e FROM nodes WHERE symbol = $1")
                .bind(&symbol.0)
                .fetch_optional(&self.pool)
                .await?
                .is_some();
            if !exists {
                return Ok::<(), sqlx::Error>(());
            }
            // Bare INSERT (NOT upsert): a symbol may carry MANY annotations, including a duplicate
            // (type, key). When ts is unset (0) let the column DEFAULT (NOW epoch) stamp it.
            if annotation.ts == 0 {
                sqlx::query(
                    "INSERT INTO annotations(node_sym, key, value, confidence, provenance, author, type, source_type, extraction_method, last_verified) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                )
                .bind(&symbol.0)
                .bind(&annotation.key)
                .bind(&annotation.value)
                .bind(annotation.confidence as f32)
                .bind(&annotation.provenance)
                .bind(&annotation.author)
                .bind(&annotation.r#type)
                .bind(&annotation.source_type)
                .bind(&annotation.extraction_method)
                .bind(annotation.last_verified)
                .execute(&self.pool)
                .await?;
            } else {
                sqlx::query(
                    "INSERT INTO annotations(node_sym, key, value, confidence, provenance, author, ts, type, source_type, extraction_method, last_verified) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                )
                .bind(&symbol.0)
                .bind(&annotation.key)
                .bind(&annotation.value)
                .bind(annotation.confidence as f32)
                .bind(&annotation.provenance)
                .bind(&annotation.author)
                .bind(annotation.ts)
                .bind(&annotation.r#type)
                .bind(&annotation.source_type)
                .bind(&annotation.extraction_method)
                .bind(annotation.last_verified)
                .execute(&self.pool)
                .await?;
            }
            Ok(())
        })
        .map_err(st)?;
        Ok(())
    }

    fn delete_annotations(
        &mut self,
        symbol: &SymbolId,
        ty: Option<&str>,
        key: &str,
    ) -> Result<usize> {
        let n = rt_block(async {
            // $2 IS NULL → key-only (all types); otherwise scope to (type = $2, key = $3).
            // `type` is matched as an opaque string — no per-type branching (rules-as-DATA).
            let result = sqlx::query(
                "DELETE FROM annotations \
                 WHERE node_sym = $1 AND key = $3 AND ($2::TEXT IS NULL OR type = $2)",
            )
            .bind(&symbol.0)
            .bind(ty)
            .bind(key)
            .execute(&self.pool)
            .await?;
            Ok::<u64, sqlx::Error>(result.rows_affected())
        })
        .map_err(st)?;
        Ok(n as usize)
    }
}

// ── GraphRead ─────────────────────────────────────────────────────────────────

impl GraphRead for PostgresStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            full_text_search: true,
            vector_search: false,
            server_side_traversal: true,
            transactional_batch: false, // begin/commit are no-ops
            shared_writers: true,       // Postgres supports concurrent writers
        }
    }

    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT data FROM nodes WHERE symbol = $1")
                    .bind(&id.0)
                    .fetch_optional(&self.pool)
                    .await?;
            match row {
                None => Ok::<Option<Node>, sqlx::Error>(None),
                Some(r) => {
                    let json: String = r.try_get("data")?;
                    Ok(Some(
                        serde_json::from_str::<Node>(&json)
                            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
                    ))
                }
            }
        })
        .map_err(st)
    }

    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
        let mut nodes: Vec<Node> = if let Some(text) = &query.text {
            let pattern = format!("%{text}%");
            rt_block(async {
                let rows = sqlx::query(
                    "SELECT data FROM nodes WHERE name ILIKE $1 OR data ILIKE $1 ORDER BY symbol",
                )
                .bind(&pattern)
                .fetch_all(&self.pool)
                .await?;
                let mut v = Vec::new();
                for row in rows {
                    let json: String = row.try_get("data")?;
                    if let Ok(n) = serde_json::from_str::<Node>(&json) {
                        v.push(n);
                    }
                }
                Ok::<Vec<Node>, sqlx::Error>(v)
            })
            .map_err(st)?
        } else if let Some(name) = &query.exact_name {
            rt_block(async {
                let rows = sqlx::query("SELECT data FROM nodes WHERE name = $1 ORDER BY symbol")
                    .bind(name)
                    .fetch_all(&self.pool)
                    .await?;
                let mut v = Vec::new();
                for row in rows {
                    let json: String = row.try_get("data")?;
                    if let Ok(n) = serde_json::from_str::<Node>(&json) {
                        v.push(n);
                    }
                }
                Ok::<Vec<Node>, sqlx::Error>(v)
            })
            .map_err(st)?
        } else {
            rt_block(async {
                let rows = sqlx::query("SELECT data FROM nodes ORDER BY symbol")
                    .fetch_all(&self.pool)
                    .await?;
                let mut v = Vec::new();
                for row in rows {
                    let json: String = row.try_get("data")?;
                    if let Ok(n) = serde_json::from_str::<Node>(&json) {
                        v.push(n);
                    }
                }
                Ok::<Vec<Node>, sqlx::Error>(v)
            })
            .map_err(st)?
        };

        // Apply remaining filters in Rust.
        nodes.retain(|n| {
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

        if let Some(limit) = query.limit {
            nodes.truncate(limit);
        }
        Ok(nodes)
    }

    fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>> {
        let sql = match dir {
            Direction::Dependents => "SELECT data FROM edges WHERE target = $1",
            Direction::Dependencies => "SELECT data FROM edges WHERE source = $1",
            Direction::Both => "SELECT data FROM edges WHERE source = $1 OR target = $1",
        };
        rt_block(async {
            let rows = sqlx::query(sql).bind(&id.0).fetch_all(&self.pool).await?;
            let mut out = Vec::new();
            for row in rows {
                let json: String = row.try_get("data")?;
                if let Ok(e) = serde_json::from_str::<Edge>(&json) {
                    out.push(e);
                }
            }
            Ok::<Vec<Edge>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph> {
        // cte_reach fetches up to max_nodes+1 rows (fencepost probe).  More than max_nodes
        // means the result was cut by the database LIMIT.  For Both direction we merge two
        // such results (each capped at max_nodes+1); the combined unique set can exceed
        // max_nodes, so we sort by depth, keep min-depth per node via the merge, then
        // truncate to max_nodes and flag truncated if anything was dropped.
        let (depths, truncated): (BTreeMap<String, u32>, bool) = match spec.direction {
            Direction::Both => {
                let mut merged = self.cte_reach(start, Direction::Dependents, spec)?;
                for (k, v) in self.cte_reach(start, Direction::Dependencies, spec)? {
                    // Keep the minimum depth when a node is reachable from both directions.
                    merged
                        .entry(k)
                        .and_modify(|e| *e = (*e).min(v))
                        .or_insert(v);
                }
                let was_truncated = merged.len() > spec.max_nodes;
                // Sort by depth and keep only the closest max_nodes nodes.
                let mut pairs: Vec<(String, u32)> = merged.into_iter().collect();
                pairs.sort_unstable_by_key(|&(_, d)| d);
                pairs.truncate(spec.max_nodes);
                (pairs.into_iter().collect(), was_truncated)
            }
            d => {
                let raw = self.cte_reach(start, d, spec)?;
                // cte_reach fetches max_nodes+1; more than max_nodes means something was cut.
                let was_truncated = raw.len() > spec.max_nodes;
                let mut pairs: Vec<(String, u32)> = raw.into_iter().collect();
                pairs.truncate(spec.max_nodes);
                (pairs.into_iter().collect(), was_truncated)
            }
        };

        let mut nodes = Vec::new();
        if let Some(n) = self.get_node(start)? {
            nodes.push(n);
        }
        for id in depths.keys() {
            if let Some(n) = self.get_node(&SymbolId(id.clone()))? {
                nodes.push(n);
            }
        }

        // Induced edges: neighbors of start + all reached nodes in the traversal direction.
        let mut edges = Vec::new();
        let mut seen = std::collections::HashSet::new();
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
        rt_block(async {
            let rows = sqlx::query("SELECT data FROM nodes")
                .fetch_all(&self.pool)
                .await?;
            let mut out = Vec::new();
            for row in rows {
                let json: String = row.try_get("data")?;
                if let Ok(n) = serde_json::from_str::<Node>(&json) {
                    out.push(n);
                }
            }
            Ok::<Vec<Node>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        rt_block(async {
            let rows = sqlx::query("SELECT data FROM edges")
                .fetch_all(&self.pool)
                .await?;
            let mut out = Vec::new();
            for row in rows {
                let json: String = row.try_get("data")?;
                if let Ok(e) = serde_json::from_str::<Edge>(&json) {
                    out.push(e);
                }
            }
            Ok::<Vec<Edge>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>> {
        use wicked_estate_core::{Location, Span};
        rt_block(async {
            let rows = sqlx::query(
                "SELECT from_sym, raw_name, kind, file, line \
                 FROM unresolved_refs WHERE raw_name = $1",
            )
            .bind(name)
            .fetch_all(&self.pool)
            .await?;
            let mut out = Vec::new();
            for row in rows {
                let from_sym: String = row.try_get("from_sym")?;
                let raw_name: String = row.try_get("raw_name")?;
                let kind_json: String = row.try_get("kind")?;
                let file: String = row.try_get("file")?;
                let line: i64 = row.try_get("line")?;
                let kind = serde_json::from_str(&kind_json)
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
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
            Ok::<Vec<UnresolvedRef>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn file_digest(&self, file: &str) -> Result<Option<String>> {
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT digest FROM files WHERE path = $1")
                    .bind(file)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok::<Option<String>, sqlx::Error>(row.and_then(|r| r.try_get("digest").ok()))
        })
        .map_err(st)
    }

    fn file_git_sha(&self, file: &str) -> Result<Option<String>> {
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT git_sha FROM files WHERE path = $1")
                    .bind(file)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok::<Option<String>, sqlx::Error>(
                row.and_then(|r| r.try_get::<Option<String>, _>("git_sha").ok().flatten()),
            )
        })
        .map_err(st)
    }

    fn repo_info(&self) -> Result<Option<RepoInfo>> {
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
        rt_block(async {
            let rows = sqlx::query(
                "SELECT seq, op, target FROM changes \
                 WHERE seq > $1 ORDER BY seq ASC LIMIT 10000",
            )
            .bind(cursor as i64)
            .fetch_all(&self.pool)
            .await?;
            let mut out = Vec::new();
            for row in rows {
                let seq: i64 = row.try_get("seq")?;
                let op_str: String = row.try_get("op")?;
                let target: String = row.try_get("target")?;
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
            Ok::<Vec<Change>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>> {
        rt_block(async {
            let rows = sqlx::query(
                "SELECT archived_seq, git_sha, edge_json FROM edge_history \
                 WHERE file = $1 ORDER BY archived_seq DESC",
            )
            .bind(file)
            .fetch_all(&self.pool)
            .await?;
            let mut out = Vec::new();
            for row in rows {
                let archived_seq: i64 = row.try_get("archived_seq")?;
                let git_sha: String = row.try_get("git_sha")?;
                let edge_json: String = row.try_get("edge_json")?;
                let edge: Edge = serde_json::from_str(&edge_json)
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
                out.push(HistoricalEdge {
                    git_sha,
                    archived_seq: archived_seq as u64,
                    edge,
                });
            }
            Ok::<Vec<HistoricalEdge>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn file_content(&self, file: &str) -> Result<Option<String>> {
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query(
                "SELECT c.body FROM files f \
                 JOIN content c ON c.git_sha = f.git_sha \
                 WHERE f.path = $1",
            )
            .bind(file)
            .fetch_optional(&self.pool)
            .await?;
            Ok::<Option<String>, sqlx::Error>(row.and_then(|r| r.try_get("body").ok()))
        })
        .map_err(st)
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
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query(
                "SELECT description, requirement, requirement_validated \
                 FROM nodes \
                 WHERE symbol = $1 \
                   AND (description IS NOT NULL \
                        OR requirement IS NOT NULL \
                        OR requirement_validated != 0)",
            )
            .bind(&symbol.0)
            .fetch_optional(&self.pool)
            .await?;
            match row {
                None => Ok::<Option<NodeSemantics>, sqlx::Error>(None),
                Some(r) => {
                    let description: Option<String> = r.try_get("description")?;
                    let requirement: Option<String> = r.try_get("requirement")?;
                    let validated_int: i64 = r.try_get("requirement_validated")?;
                    Ok(Some(NodeSemantics {
                        description,
                        requirement,
                        requirement_validated: validated_int != 0,
                    }))
                }
            }
        })
        .map_err(st)
    }

    fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>> {
        rt_block(async {
            let rows = sqlx::query("SELECT data FROM nodes WHERE requirement = $1 ORDER BY symbol")
                .bind(requirement)
                .fetch_all(&self.pool)
                .await?;
            let mut out = Vec::new();
            for row in rows {
                let json: String = row.try_get("data")?;
                if let Ok(n) = serde_json::from_str::<Node>(&json) {
                    out.push(n);
                }
            }
            Ok::<Vec<Node>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn annotations(&self, symbol: &SymbolId) -> Result<Vec<Annotation>> {
        rt_block(async {
            // Order by ts then id so identical-ts rows have a stable, insertion order.
            let rows = sqlx::query(
                "SELECT key, value, confidence, provenance, author, ts, type, source_type, extraction_method, last_verified \
                 FROM annotations WHERE node_sym = $1 ORDER BY ts ASC, id ASC",
            )
            .bind(&symbol.0)
            .fetch_all(&self.pool)
            .await?;
            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                out.push(row_to_annotation(&r)?);
            }
            Ok::<Vec<Annotation>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn annotations_by_type(&self, ty: &str) -> Result<Vec<(SymbolId, Annotation)>> {
        rt_block(async {
            // idx_annotations_type backs the WHERE; ordered by symbol then ts for determinism.
            let rows = sqlx::query(
                "SELECT node_sym, key, value, confidence, provenance, author, ts, type, source_type, extraction_method, last_verified \
                 FROM annotations \
                 WHERE type = $1 \
                 ORDER BY node_sym ASC, ts ASC, id ASC",
            )
            .bind(ty)
            .fetch_all(&self.pool)
            .await?;
            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                let sid: String = r.try_get("node_sym")?;
                out.push((SymbolId(sid), row_to_annotation(&r)?));
            }
            Ok::<Vec<(SymbolId, Annotation)>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn annotations_stale_since(&self, cutoff: i64) -> Result<Vec<(SymbolId, Annotation)>> {
        rt_block(async {
            // Freshness read: every annotation last verified STRICTLY BEFORE `cutoff`. Never-verified
            // rows (last_verified = 0) fall out for any positive cutoff. idx_annotations_last_verified
            // backs the range scan; ordered by symbol then ts, parallel to annotations_by_type.
            let rows = sqlx::query(
                "SELECT node_sym, key, value, confidence, provenance, author, ts, type, source_type, extraction_method, last_verified \
                 FROM annotations \
                 WHERE last_verified < $1 \
                 ORDER BY node_sym ASC, ts ASC, id ASC",
            )
            .bind(cutoff)
            .fetch_all(&self.pool)
            .await?;
            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                let sid: String = r.try_get("node_sym")?;
                out.push((SymbolId(sid), row_to_annotation(&r)?));
            }
            Ok::<Vec<(SymbolId, Annotation)>, sqlx::Error>(out)
        })
        .map_err(st)
    }

    fn stats(&self) -> Result<GraphStats> {
        rt_block(async {
            let node_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM nodes")
                .fetch_one(&self.pool)
                .await?
                .try_get("c")?;
            let edge_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM edges")
                .fetch_one(&self.pool)
                .await?
                .try_get("c")?;

            let file_kind = serde_json::to_string(&NodeKind::File)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            let file_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM nodes WHERE kind = $1")
                .bind(&file_kind)
                .fetch_one(&self.pool)
                .await?
                .try_get("c")?;

            let unresolved_ref_count: i64 =
                sqlx::query("SELECT COUNT(*) AS c FROM unresolved_refs")
                    .fetch_one(&self.pool)
                    .await?
                    .try_get("c")?;

            let mut nodes_by_kind = BTreeMap::new();
            {
                let rows = sqlx::query("SELECT kind, COUNT(*) AS c FROM nodes GROUP BY kind")
                    .fetch_all(&self.pool)
                    .await?;
                for row in rows {
                    let k: String = row.try_get("kind")?;
                    let c: i64 = row.try_get("c")?;
                    nodes_by_kind.insert(k, c as u64);
                }
            }
            let mut edges_by_kind = BTreeMap::new();
            {
                let rows = sqlx::query("SELECT kind, COUNT(*) AS c FROM edges GROUP BY kind")
                    .fetch_all(&self.pool)
                    .await?;
                for row in rows {
                    let k: String = row.try_get("kind")?;
                    let c: i64 = row.try_get("c")?;
                    edges_by_kind.insert(k, c as u64);
                }
            }

            Ok::<GraphStats, sqlx::Error>(GraphStats {
                node_count: node_count as u64,
                edge_count: edge_count as u64,
                file_count: file_count as u64,
                unresolved_ref_count: unresolved_ref_count as u64,
                nodes_by_kind,
                edges_by_kind,
                db_size_bytes: 0,
            })
        })
        .map_err(st)
    }
}

// ── SymbolIndex ───────────────────────────────────────────────────────────────

impl SymbolIndex for PostgresStore {
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
