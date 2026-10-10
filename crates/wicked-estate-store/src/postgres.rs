//! `PostgresStore` — Postgres-backed [`GraphStore`] implementation.
//!
//! Uses `sqlx` with the postgres feature for connection pooling.  The `GraphStore` trait is
//! synchronous, so every async sqlx call is wrapped via [`rt_block`] which delegates to the
//! current Tokio runtime (via `block_in_place`) or creates a one-shot runtime when called from
//! a non-async context.
//!
//! **Batch atomicity (locked decision #8):** `begin_batch`/`commit_batch` open and commit ONE
//! real transaction at `READ COMMITTED`; every statement issued while the batch is open —
//! writes AND reads — rides that transaction (see [`PostgresStore::conn`]). Concurrent readers
//! on other connections see the pre-batch state or the full committed batch, never a torn
//! partial batch. `transactional_batch: true` in [`StoreCapabilities`].
//!
//! Schema mirrors `SqliteStore` but uses Postgres-native types:
//! - `BIGSERIAL` primary keys where SQLite uses `INTEGER PRIMARY KEY AUTOINCREMENT`
//! - `TEXT` for all symbol strings (no integer interning — Postgres handles string dedup well)
//! - `REAL` for confidence. NOTE: this is **not** the same as SQLite. SQLite `REAL` is an
//!   8-byte IEEE-754 double (f64); Postgres `REAL` is a 4-byte single (f32). Edge `Confidence`
//!   is already f32 in core, so edges round-trip losslessly on both. The `Annotation.confidence`
//!   field is f64, so on this backend it narrows f64 → f32 on write and widens back on read —
//!   a fraction like `0.6` reads back as `0.6000000238…`. The conformance kit asserts this with
//!   an f32-epsilon tolerance (see `graph_store_suite`'s annotation block).
//! - `TEXT` for JSON columns (same round-trip fidelity as SQLite)
//! - No zstd compression for content — Postgres applies page-level compression internally
//! - Full-text search via `ILIKE` (upgrade to `pg_trgm` / `tsvector` in a future pass)

use sha1::{Digest as Sha1Digest, Sha1};
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet};
use wicked_estate_core::support::{
    EdgeKey, EdgeSupport, StoredFacts, SupportFact, SupportOwner, SupportOwnerState,
    SupportReplacement, base_upsert_wins, fact_key, normalize_facts, plan_replacement,
    project_edge,
};
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
/// `f64` — a **lossy** narrowing relative to SQLite (whose `REAL` is f64): a fractional confidence
/// such as `0.6` reads back as `0.6000000238…`. `ts` / `last_verified` are `BIGINT` (i64). Mirrors
/// the column order the read queries
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

/// Split schema DDL into executable statements. Strips `--` comment lines FIRST (so a `;` inside a
/// comment can't split a statement mid-comment → "unterminated quoted string"), then splits on `;`
/// and drops empties. Pure string logic — unit-tested without a database.
fn split_ddl(sql: &str) -> Vec<String> {
    let cleaned: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    cleaned
        .split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

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
  requirement_validated BIGINT NOT NULL DEFAULT 0,
  requirement_validated_by TEXT,
  requirement_validated_at BIGINT,
  scope                 TEXT NOT NULL DEFAULT ''
);
-- Idempotent migration for DBs created before `scope` existed. MUST run BEFORE any index on
-- `scope` (DDL is split on semicolons, comment lines stripped first, executed in order): on a legacy nodes table the CREATE TABLE
-- is a no-op, so the column must be ADDed before `idx_nodes_scope` references it.
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS scope TEXT NOT NULL DEFAULT '';
-- A validated requirement must name WHO validated it, so the flag never travels alone. Added
-- independently of each other and of `scope`: on a legacy nodes table the CREATE TABLE above is a
-- no-op, so every column added after the original schema needs its own ADD.
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS requirement_validated_by TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS requirement_validated_at BIGINT;
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(file);
CREATE INDEX IF NOT EXISTS idx_nodes_scope ON nodes(scope);

-- Multi-file symbol contributions (M4 / Option A — wicked-estate#152), mirroring the SQLite
-- node_files table (see schema.sql there for the full rationale). One (symbol, file) row per
-- contributing file; `data` is the full Node JSON as THAT file's extraction produced it; `is_def`
-- is 0 for a declaration contribution (metadata.is_declaration truthy), 1 otherwise. The nodes row
-- is the DERIVED preferred contribution (is_def DESC, file ASC — definition wins, deterministic
-- tiebreak), never last-write-wins; remove_file deletes contributions and re-homes survivors.
CREATE TABLE IF NOT EXISTS node_files (
  symbol TEXT   NOT NULL,
  file   TEXT   NOT NULL,
  is_def BIGINT NOT NULL DEFAULT 1,
  data   TEXT   NOT NULL,
  PRIMARY KEY (symbol, file)
);
CREATE INDEX IF NOT EXISTS idx_node_files_file ON node_files(file);
-- Idempotent backfill for DBs created before node_files existed: seed one definition-preference
-- contribution per current node from the nodes projection. The NOT EXISTS guard makes this a
-- single-shot migration — any row in node_files (a fresh DB has none but also has no nodes)
-- disarms it, so it never duplicates or resurrects contributions on later opens.
INSERT INTO node_files(symbol, file, is_def, data)
  SELECT symbol, file,
         CASE WHEN COALESCE((data::jsonb->'metadata'->>'is_declaration')::boolean, false)
              THEN 0 ELSE 1 END,
         data
  FROM nodes
  WHERE NOT EXISTS (SELECT 1 FROM node_files)
ON CONFLICT (symbol, file) DO NOTHING;

-- M8/DoD-XA4: per-symbol live-node epoch. Postgres keys nodes on the symbol string and DELETEs the
-- node row on remove_file, so the epoch needs a dedicated table that SURVIVES remove_file (the
-- analogue of SQLite's symbols.gen/had_node columns on the append-only intern table). `had_node` is
-- the sticky "ever had a node" marker that separates a reuse-after-delete (bump) from a first-ever /
-- edge-only symbol getting its first node (no bump).
CREATE TABLE IF NOT EXISTS symbol_gen (
  symbol   TEXT PRIMARY KEY,
  gen      BIGINT NOT NULL DEFAULT 0,
  had_node BIGINT NOT NULL DEFAULT 0
);

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

-- TS-S2A support plane (wicked_estate_core::support; docs/ENGINE-CONTRACT.md §3.4) — additive,
-- created on any older database by this same CREATE ... IF NOT EXISTS DDL on open. Mirrors the
-- SQLite tables, except that a fact is keyed by `fact_hash` (SHA-1 of the producer's opaque `fact_id`):
-- a btree index entry is capped at ~2.7 KB and a canonical flow fact can exceed it.
CREATE TABLE IF NOT EXISTS support_owners (
  producer   TEXT NOT NULL,
  snapshot   TEXT NOT NULL,
  generation BIGINT NOT NULL,
  PRIMARY KEY (producer, snapshot)
);
CREATE TABLE IF NOT EXISTS edge_supports (
  producer  TEXT NOT NULL,
  snapshot  TEXT NOT NULL,
  source    TEXT NOT NULL,
  target    TEXT NOT NULL,
  kind      TEXT NOT NULL,
  fact_hash TEXT NOT NULL,
  fact_id   TEXT NOT NULL,
  data      TEXT NOT NULL,
  PRIMARY KEY (producer, snapshot, source, target, kind, fact_hash)
);
CREATE INDEX IF NOT EXISTS idx_edge_supports_key ON edge_supports(source, target, kind);
CREATE TABLE IF NOT EXISTS edge_base (
  source TEXT NOT NULL,
  target TEXT NOT NULL,
  kind   TEXT NOT NULL,
  file   TEXT NOT NULL DEFAULT '',
  data   TEXT NOT NULL,
  PRIMARY KEY (source, target, kind)
);
CREATE INDEX IF NOT EXISTS idx_edge_base_file ON edge_base(file);

CREATE TABLE IF NOT EXISTS unresolved_refs (
  id       BIGSERIAL PRIMARY KEY,
  from_sym TEXT NOT NULL,
  raw_name TEXT NOT NULL,
  kind     TEXT NOT NULL,
  file     TEXT NOT NULL DEFAULT '',
  line     BIGINT NOT NULL DEFAULT 0,
  start_byte BIGINT NOT NULL DEFAULT 0,
  end_byte   BIGINT NOT NULL DEFAULT 0
);
-- Admissibility F-B: byte-exact site identity, additive on legacy tables (the CREATE above is
-- a no-op there, so each column needs its own ADD). DEFAULT 0 = unknown/synthetic (Span::ZERO);
-- pre-existing rows read back span-zero until their file is re-persisted — no data rewrite.
ALTER TABLE unresolved_refs ADD COLUMN IF NOT EXISTS start_byte BIGINT NOT NULL DEFAULT 0;
ALTER TABLE unresolved_refs ADD COLUMN IF NOT EXISTS end_byte BIGINT NOT NULL DEFAULT 0;
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
  "type"            TEXT    NOT NULL DEFAULT 'note',
  source_type       TEXT    NOT NULL DEFAULT 'unspecified',
  extraction_method TEXT    NOT NULL DEFAULT 'manual',
  last_verified     BIGINT  NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_annotations_node ON annotations(node_sym);
CREATE INDEX IF NOT EXISTS idx_annotations_key  ON annotations(key);
CREATE INDEX IF NOT EXISTS idx_annotations_type ON annotations("type");
CREATE INDEX IF NOT EXISTS idx_annotations_last_verified ON annotations(last_verified);
"#;

// ── PostgresStore ─────────────────────────────────────────────────────────────

/// Postgres-backed graph store. Satisfies [`GraphRead`] + [`GraphWrite`] (and therefore
/// [`GraphStore`]).  Connects via `sqlx::PgPool`; every method blocks on the async layer
/// using [`rt_block`].
///
/// **Batch atomicity (locked decision #8):** `begin_batch`/`commit_batch` map to ONE real
/// Postgres transaction at `READ COMMITTED`. While a batch is open, every statement issued
/// through this store rides that transaction (reads included — the resolver's `SymbolIndex`
/// lookups must see the nodes written earlier in the same batch, exactly as they do on the
/// single-connection SQLite store). Concurrent readers on other connections therefore see
/// either the pre-batch state or the full committed batch — never a partial batch (the
/// torn-read bug this replaced: `shared_writers: true` with per-statement auto-commit).
pub struct PostgresStore {
    pool: sqlx::PgPool,
    /// The open batch transaction (`begin_batch` → `commit_batch`), if any. Behind a `Mutex`
    /// only because `GraphRead` methods take `&self` and must also ride the transaction;
    /// the store follows the single-writer contract, so the lock is uncontended. Dropping
    /// the store mid-batch rolls the transaction back (sqlx `Transaction` drop semantics).
    batch: std::sync::Mutex<Option<sqlx::Transaction<'static, sqlx::Postgres>>>,
    history_enabled: bool,
}

/// Connection handle resolved per method call: the open batch transaction when one exists,
/// else a plain pooled connection (per-statement auto-commit — the pre-batch behavior).
enum ConnHandle<'s> {
    /// A batch is open — statements join the single batch transaction (locked decision #8).
    Tx(std::sync::MutexGuard<'s, Option<sqlx::Transaction<'static, sqlx::Postgres>>>),
    /// No batch — a pooled connection, each statement auto-commits. `Option` only so the
    /// manual `Drop` below can move it out; it is `Some` for the handle's entire life.
    Pooled(Option<sqlx::pool::PoolConnection<sqlx::Postgres>>),
}

impl ConnHandle<'_> {
    /// The `PgConnection` to execute on. For `Tx` this is the transaction's connection.
    fn as_conn(&mut self) -> &mut sqlx::PgConnection {
        match self {
            // `Tx` is only constructed when the Option is Some (see `PostgresStore::conn`),
            // and `commit_batch` can't run while a handle is live (single-writer contract).
            ConnHandle::Tx(guard) => guard
                .as_mut()
                .map(|tx| &mut **tx)
                .expect("ConnHandle::Tx constructed from Some"),
            ConnHandle::Pooled(conn) => conn.as_mut().expect("Pooled is Some until drop"),
        }
    }
}

impl Drop for ConnHandle<'_> {
    fn drop(&mut self) {
        // Returning a `PoolConnection` to the pool spawns a task and PANICS outside a Tokio
        // context ("this functionality requires a Tokio context"). `GraphStore` is a sync
        // trait, so handles routinely die on plain threads — enter the process-wide runtime
        // for the release. (No-op cost when already inside one: enter() is a thread-local set.)
        if let ConnHandle::Pooled(opt) = self {
            if let Some(conn) = opt.take() {
                let _rt = global_rt().enter();
                drop(conn);
            }
        }
    }
}

impl PostgresStore {
    /// Run `f` inside a transaction of its own when no batch is open (TS-S2A). The support lock is
    /// transaction-scoped, so an unbatched base-plane write would otherwise hold it only for the
    /// one statement that takes it — and a replacement could interleave between that writer's
    /// support check and its write. With a batch open, `f` simply joins it. On error the implicit
    /// transaction is dropped, which rolls it back.
    fn batch_open(&mut self) -> bool {
        self.batch
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    fn in_implicit_batch<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let open = self
            .batch
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some();
        if open {
            return f(self);
        }
        self.begin_batch()?;
        match f(self) {
            Ok(v) => {
                self.commit_batch()?;
                Ok(v)
            }
            Err(e) => {
                let tx = self
                    .batch
                    .get_mut()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(tx) = tx {
                    // Report the ORIGINAL error even if the rollback itself fails.
                    if let Err(rollback) = rt_block(tx.rollback()) {
                        return Err(Error::Storage(format!(
                            "{e}; rolling the implicit transaction back also failed ({rollback})"
                        )));
                    }
                }
                Err(e)
            }
        }
    }

    /// Open (or create) a Postgres graph store at `url`.
    ///
    /// Runs the schema DDL (`CREATE TABLE IF NOT EXISTS ...`) and inserts the initial
    /// `graph_version=0` meta row if absent.
    pub fn open(url: &str) -> Result<Self> {
        let pool = rt_block(sqlx::PgPool::connect(url)).map_err(st)?;

        // Run schema DDL statement by statement (see `split_ddl`).
        rt_block(async {
            for stmt in split_ddl(SCHEMA) {
                sqlx::query(&stmt).execute(&pool).await?;
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
            batch: std::sync::Mutex::new(None),
            history_enabled,
        })
    }

    /// Resolve the connection every statement in the calling method executes on: the open
    /// batch transaction when one exists, else a pooled connection.
    ///
    /// The handle holds the batch lock (or a pooled connection) until dropped, so a method
    /// MUST NOT call another `self` method while its handle is alive — acquire late, drop
    /// early. All in-tree callers follow this (each method acquires exactly one handle).
    fn conn(&self) -> Result<ConnHandle<'_>> {
        let guard = self
            .batch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.is_some() {
            return Ok(ConnHandle::Tx(guard));
        }
        drop(guard);
        let conn = rt_block(self.pool.acquire()).map_err(st)?;
        Ok(ConnHandle::Pooled(Some(conn)))
    }

    // ── meta helpers ────────────────────────────────────────────────────────

    /// Read an arbitrary string value from the `meta` table. Returns `None` when absent.
    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query("SELECT v FROM meta WHERE k = $1")
                .bind(key)
                .fetch_optional(h.as_conn())
                .await?;
            Ok::<Option<String>, sqlx::Error>(row.and_then(|r| r.try_get("v").ok()))
        })
        .map_err(st)
    }

    /// Write an arbitrary string value to the `meta` table (insert or replace).
    pub fn meta_set(&mut self, key: &str, value: &str) -> Result<()> {
        let mut h = self.conn()?;
        rt_block(
            sqlx::query(
                "INSERT INTO meta(k, v) VALUES($1, $2) \
                 ON CONFLICT(k) DO UPDATE SET v = EXCLUDED.v",
            )
            .bind(key)
            .bind(value)
            .execute(h.as_conn()),
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
        let mut h = self.conn()?;
        rt_block(
            sqlx::query("UPDATE meta SET v = (v::BIGINT + 1)::TEXT WHERE k = 'graph_version'")
                .execute(h.as_conn()),
        )
        .map_err(st)?;
        Ok(())
    }

    // ── cache helpers ────────────────────────────────────────────────────────

    /// Return the cached value for `key` only if stored at the current graph version.
    pub fn cache_get(&self, key: &str) -> Result<Option<String>> {
        // graph_version acquires (and releases) its own handle — must complete before ours.
        let ver = self.graph_version()?;
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT value FROM cache WHERE key = $1 AND version = $2")
                    .bind(key)
                    .bind(ver)
                    .fetch_optional(h.as_conn())
                    .await?;
            Ok::<Option<String>, sqlx::Error>(row.and_then(|r| r.try_get("value").ok()))
        })
        .map_err(st)
    }

    /// Store `value` for `key` at the current graph version.
    pub fn cache_put(&mut self, key: &str, value: &str) -> Result<()> {
        // graph_version acquires (and releases) its own handle — must complete before ours.
        let ver = self.graph_version()?;
        let mut h = self.conn()?;
        rt_block(
            sqlx::query(
                "INSERT INTO cache(key, version, value) VALUES($1, $2, $3) \
                 ON CONFLICT(key, version) DO UPDATE SET value = EXCLUDED.value",
            )
            .bind(key)
            .bind(ver)
            .bind(value)
            .execute(h.as_conn()),
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

    /// Every parked `Imports` row whose written specifier is relative (`./` / `../`, possibly
    /// quoted) — one kind-scoped SQL pass, relative-spec check in Rust. Backs the trait method
    /// of the same name on `GraphStoreMutExt` (the back-fill candidate fetch, wicked-estate#141).
    pub fn parked_relative_import_refs(&self) -> Result<Vec<UnresolvedRef>> {
        use wicked_estate_core::{EdgeKind, Location, Span};
        let kind_json = serde_json::to_string(&EdgeKind::Imports)?;
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query(
                "SELECT from_sym, raw_name, kind, file, line, start_byte, end_byte \
                 FROM unresolved_refs WHERE kind = $1",
            )
            .bind(&kind_json)
            .fetch_all(h.as_conn())
            .await?;
            let mut out = Vec::new();
            for row in rows {
                let raw_name: String = row.try_get("raw_name")?;
                if !crate::is_relative_import_spec(&raw_name) {
                    continue;
                }
                let from_sym: String = row.try_get("from_sym")?;
                let kind_json: String = row.try_get("kind")?;
                let file: String = row.try_get("file")?;
                let line: i64 = row.try_get("line")?;
                let start_byte: i64 = row.try_get("start_byte")?;
                let end_byte: i64 = row.try_get("end_byte")?;
                let kind = serde_json::from_str(&kind_json)
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
                // Same reconstruction as unresolved_refs_for_name: persisted site columns only.
                let location = Location::new(
                    file,
                    Span {
                        start_line: line as u32,
                        start_byte: start_byte as u32,
                        end_byte: end_byte as u32,
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

    /// Delete unresolved rows by persisted site identity (all 7 columns
    /// `upsert_unresolved_refs` writes; `from_sym` is stored as TEXT on this backend). Returns
    /// rows deleted. Backs the trait method of the same name on `GraphStoreMutExt`.
    pub fn delete_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<usize> {
        let mut h = self.conn()?;
        let mut deleted = 0usize;
        for r in refs {
            let kind = serde_json::to_string(&r.kind)?;
            let res = rt_block(
                sqlx::query(
                    "DELETE FROM unresolved_refs \
                     WHERE from_sym = $1 AND raw_name = $2 AND kind = $3 AND file = $4 \
                       AND line = $5 AND start_byte = $6 AND end_byte = $7",
                )
                .bind(&r.from.0)
                .bind(&r.raw_name)
                .bind(&kind)
                .bind(&r.location.file)
                .bind(r.location.span.start_line as i64)
                .bind(r.location.span.start_byte as i64)
                .bind(r.location.span.end_byte as i64)
                .execute(h.as_conn()),
            )
            .map_err(st)?;
            deleted += res.rows_affected() as usize;
        }
        Ok(deleted)
    }

    // ── recursive CTE traversal ──────────────────────────────────────────────

    fn cte_reach(
        &self,
        start: &SymbolId,
        dir: Direction,
        spec: &TraversalSpec,
    ) -> Result<(BTreeMap<String, u32>, bool)> {
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
        //
        // Second leg of the compound SELECT = the DEPTH-HORIZON existence probe
        // (wicked-estate#190). `frontier` is the set of nodes whose MIN depth is exactly
        // `max_depth` — the nodes the bounded recursion declined to expand. If any qualifying edge
        // leaves one of them toward a node the walk never visited, the horizon cut the result.
        // `LIMIT 1` keeps it an existence test and `frontier` carries the same node budget, so the
        // probe is bounded by `max_nodes`. The recursion is NOT widened to max_depth+1 and no
        // beyond-horizon row is returned (bounded-traversal invariant); `walk` is MATERIALIZED so
        // the recursion runs ONCE for both legs. The probe row is tagged `horizon = 1` and carries
        // a sentinel id the row loop discards.
        let sql = format!(
            "WITH RECURSIVE walk(id, depth) AS (
                 SELECT $1::TEXT, 0
                 UNION
                 SELECT e.{advance_col}, walk.depth + 1
                   FROM edges e JOIN walk ON e.{match_col} = walk.id
                  WHERE walk.depth < $2 AND e.confidence >= $3 {kind_filter}
             ),
             mins AS MATERIALIZED (
                 SELECT id, MIN(depth) AS d FROM walk GROUP BY id
             ),
             frontier AS (SELECT id FROM mins WHERE d = $2 LIMIT $4)
             SELECT id, d AS min_depth, 0 AS horizon FROM (
                 SELECT id, d FROM mins WHERE id <> $1 ORDER BY d, id LIMIT $4) ranked
             UNION ALL
             SELECT ''::TEXT, 0, 1 FROM (
                 SELECT 1 FROM edges e JOIN frontier f ON e.{match_col} = f.id
                  WHERE e.confidence >= $3 {kind_filter}
                    AND e.{advance_col} NOT IN (SELECT id FROM mins)
                  LIMIT 1) probe"
        );

        // Fetch max_nodes + 1 rows so we can distinguish "exactly max_nodes reachable" from
        // "truncated" — if we get more than max_nodes back, the result was cut.
        let mut h = self.conn()?;
        let rows = rt_block(async {
            sqlx::query(&sql)
                .bind(&start.0)
                .bind(spec.max_depth as i64)
                .bind(spec.min_confidence as f64)
                .bind((spec.max_nodes as i64) + 1)
                .fetch_all(h.as_conn())
                .await
        })
        .map_err(st)?;

        let mut out = BTreeMap::new();
        let mut depth_horizon_reached = false;
        for row in rows {
            let horizon: i32 = row.try_get("horizon").map_err(st)?;
            if horizon != 0 {
                depth_horizon_reached = true;
                continue;
            }
            let id: String = row.try_get("id").map_err(st)?;
            let depth: i32 = row.try_get("min_depth").map_err(st)?;
            out.insert(id, depth as u32);
        }
        Ok((out, depth_horizon_reached))
    }

    /// Hard-delete nodes by symbol id, plus every edge incident on them. Atomic (transaction): all
    /// deletes commit together or none. (On PG `edges.source/target` are the symbol strings directly,
    /// so no sid indirection is needed.) Returns the number of node rows removed.
    pub fn remove_nodes(&mut self, ids: &[SymbolId]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let syms: Vec<String> = ids.iter().map(|i| i.0.clone()).collect();
        let mut h = self.conn()?;
        let n = rt_block(async {
            // `Connection::begin` on the handle's connection: a real transaction when no batch
            // is open, an automatic SAVEPOINT when called inside an open batch transaction —
            // so the two deletes stay atomic in both modes without double-BEGIN errors.
            use sqlx::Connection;
            let mut tx = h.as_conn().begin().await?;
            sqlx::query("DELETE FROM edges WHERE source = ANY($1) OR target = ANY($1)")
                .bind(&syms)
                .execute(&mut *tx)
                .await?;
            // TS-S2A: erasure is total — support facts and base contributions naming the symbols
            // go too (support is otherwise never deleted outside `replace_edge_supports`).
            sqlx::query("DELETE FROM edge_supports WHERE source = ANY($1) OR target = ANY($1)")
                .bind(&syms)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM edge_base WHERE source = ANY($1) OR target = ANY($1)")
                .bind(&syms)
                .execute(&mut *tx)
                .await?;
            // Erasure removes the symbols' contribution records too (wicked-estate#152).
            sqlx::query("DELETE FROM node_files WHERE symbol = ANY($1)")
                .bind(&syms)
                .execute(&mut *tx)
                .await?;
            let res = sqlx::query("DELETE FROM nodes WHERE symbol = ANY($1)")
                .bind(&syms)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok::<u64, sqlx::Error>(res.rows_affected())
        })
        .map_err(st)?;
        Ok(n as usize)
    }
}

impl Drop for PostgresStore {
    fn drop(&mut self) {
        // A batch left open at drop rolls back (sqlx `Transaction` drop semantics). The inner
        // `PoolConnection` release needs a Tokio context (see `ConnHandle::drop`), so take the
        // transaction down inside the process-wide runtime.
        let tx = self
            .batch
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(tx) = tx {
            let _rt = global_rt().enter();
            drop(tx);
        }
    }
}

// ── TS-S2A support plane (contract: `wicked_estate_core::support`) ─────────────
//
// Free async helpers over one `PgConnection`, so they run inside whatever transaction or savepoint
// the caller opened (`replace_edge_supports` opens its own; `remove_file`/`prune_dangling_edges`/
// `upsert_edges` run on the batch transaction when one is open).

fn fact_hash(fact_id: &str) -> String {
    let mut h = Sha1::new();
    h.update(fact_id.as_bytes());
    format!("{:x}", h.finalize())
}

async fn pg_any_support(c: &mut sqlx::PgConnection) -> Result<bool> {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM edge_supports)")
        .fetch_one(c)
        .await
        .map_err(st)
}

async fn pg_key_supported(c: &mut sqlx::PgConnection, key: &EdgeKey) -> Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM edge_supports WHERE source = $1 AND target = $2 AND kind = $3)",
    )
    .bind(&key.0)
    .bind(&key.1)
    .bind(&key.2)
    .fetch_one(c)
    .await
    .map_err(st)
}

/// The advisory-lock key serializing the support plane across concurrent Postgres writers
/// (`shared_writers: true`). `replace_edge_supports` takes it EXCLUSIVE for its transaction; every
/// base-plane write that can touch a supported row (`upsert_edges`, `remove_file`,
/// `prune_dangling_edges`) takes it SHARED first, so a replacement never interleaves with them and
/// two replacements never interleave with each other (generation check-then-write, cross-owner
/// re-projection, first-support base capture). Transaction-scoped: it holds until the batch (or
/// the replacement's own transaction) ends; outside a batch, base-plane statements autocommit and
/// the shared lock protects only the statement it precedes.
const SUPPORT_LOCK_KEY: i64 = 0x5453_3241; // "TS2A"

async fn pg_support_lock(c: &mut sqlx::PgConnection, exclusive: bool) -> Result<()> {
    let sql = if exclusive {
        "SELECT pg_advisory_xact_lock($1)"
    } else {
        "SELECT pg_advisory_xact_lock_shared($1)"
    };
    sqlx::query(sql)
        .bind(SUPPORT_LOCK_KEY)
        .execute(c)
        .await
        .map_err(st)?;
    Ok(())
}

/// The kept-aside base contribution: parsed, and its stored JSON text so a restore writes back
/// the exact bytes.
async fn pg_edge_base_of(
    c: &mut sqlx::PgConnection,
    key: &EdgeKey,
) -> Result<Option<(Edge, String)>> {
    let data: Option<String> = sqlx::query_scalar(
        "SELECT data FROM edge_base WHERE source = $1 AND target = $2 AND kind = $3",
    )
    .bind(&key.0)
    .bind(&key.1)
    .bind(&key.2)
    .fetch_optional(c)
    .await
    .map_err(st)?;
    data.map(|d| Ok((serde_json::from_str(&d)?, d))).transpose()
}

async fn pg_set_edge_base(c: &mut sqlx::PgConnection, key: &EdgeKey, edge: &Edge) -> Result<()> {
    let file = edge
        .location
        .as_ref()
        .map(|l| l.file.as_str())
        .unwrap_or("");
    sqlx::query(
        "INSERT INTO edge_base(source, target, kind, file, data) VALUES($1, $2, $3, $4, $5) \
         ON CONFLICT(source, target, kind) DO UPDATE SET file = EXCLUDED.file, data = EXCLUDED.data",
    )
    .bind(&key.0)
    .bind(&key.1)
    .bind(&key.2)
    .bind(file)
    .bind(serde_json::to_string(edge)?)
    .execute(c)
    .await
    .map_err(st)?;
    Ok(())
}

/// Write (or delete) the public `edges` row unconditionally — a projection is authoritative, so
/// the base plane's `>=` rule does not apply to it. `row` = (edge, exact JSON, owning file).
async fn pg_set_public_edge(
    c: &mut sqlx::PgConnection,
    key: &EdgeKey,
    row: Option<(&Edge, &str, &str)>,
) -> Result<()> {
    match row {
        Some((e, data, file)) => {
            sqlx::query(
                "INSERT INTO edges(source, target, kind, confidence, file, data) \
                 VALUES($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT(source, target, kind) DO UPDATE SET \
                   confidence = EXCLUDED.confidence, file = EXCLUDED.file, data = EXCLUDED.data",
            )
            .bind(&key.0)
            .bind(&key.1)
            .bind(&key.2)
            .bind(e.confidence.get() as f64)
            .bind(file)
            .bind(data)
            .execute(c)
            .await
            .map_err(st)?;
        }
        None => {
            sqlx::query("DELETE FROM edges WHERE source = $1 AND target = $2 AND kind = $3")
                .bind(&key.0)
                .bind(&key.1)
                .bind(&key.2)
                .execute(c)
                .await
                .map_err(st)?;
        }
    }
    Ok(())
}

/// Recompute `key`'s public edge from its base contribution and every owner's facts. With no
/// support left, the base contribution becomes the plain public edge again (its exact bytes). A
/// projected row's `edges.file` is the base contribution's file or `''` — never a fact's site
/// (see `SqliteStore::reproject`).
async fn pg_reproject(c: &mut sqlx::PgConnection, key: &EdgeKey) -> Result<()> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT data FROM edge_supports WHERE source = $1 AND target = $2 AND kind = $3",
    )
    .bind(&key.0)
    .bind(&key.1)
    .bind(&key.2)
    .fetch_all(&mut *c)
    .await
    .map_err(st)?;
    let facts: Vec<Edge> = rows
        .iter()
        .map(|d| serde_json::from_str(d))
        .collect::<std::result::Result<_, _>>()?;
    let base = pg_edge_base_of(&mut *c, key).await?;
    let base_file = |b: &Edge| {
        b.location
            .as_ref()
            .map(|l| l.file.clone())
            .unwrap_or_default()
    };
    if facts.is_empty() {
        sqlx::query("DELETE FROM edge_base WHERE source = $1 AND target = $2 AND kind = $3")
            .bind(&key.0)
            .bind(&key.1)
            .bind(&key.2)
            .execute(&mut *c)
            .await
            .map_err(st)?;
        return match &base {
            Some((b, raw)) => pg_set_public_edge(c, key, Some((b, raw, &base_file(b)))).await,
            None => pg_set_public_edge(c, key, None).await,
        };
    }
    let refs: Vec<&Edge> = facts.iter().collect();
    let file = base.as_ref().map(|(b, _)| base_file(b)).unwrap_or_default();
    match project_edge(base.as_ref().map(|(b, _)| b), &refs) {
        Some(p) => {
            let data = serde_json::to_string(&p)?;
            pg_set_public_edge(c, key, Some((&p, &data, &file))).await
        }
        None => pg_set_public_edge(c, key, None).await,
    }
}

fn pg_keys(rows: &[sqlx::postgres::PgRow]) -> Result<BTreeSet<EdgeKey>> {
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get("source").map_err(st)?,
                r.try_get("target").map_err(st)?,
                r.try_get("kind").map_err(st)?,
            ))
        })
        .collect()
}

/// The support-plane half of an edge-deleting step, run BEFORE its edge DELETE with the same
/// predicate (unaliased columns, `$1` bound to `arg`): take the shared support lock, capture the
/// supported rows about to be deleted, and retire matching base contributions. `None` when no
/// support exists — the step then runs exactly its pre-TS-S2A statements.
async fn pg_support_pre_delete(
    c: &mut sqlx::PgConnection,
    predicate: &str,
    arg: Option<&str>,
) -> Result<Option<(BTreeSet<EdgeKey>, BTreeSet<EdgeKey>)>> {
    pg_support_lock(&mut *c, false).await?;
    if !pg_any_support(&mut *c).await? {
        return Ok(None);
    }
    let select = format!(
        "SELECT source, target, kind FROM edges WHERE ({predicate}) AND EXISTS ( \
           SELECT 1 FROM edge_supports s WHERE s.source = edges.source \
             AND s.target = edges.target AND s.kind = edges.kind)"
    );
    let mut q = sqlx::query(&select);
    if let Some(a) = arg {
        q = q.bind(a);
    }
    let deleted = pg_keys(&q.fetch_all(&mut *c).await.map_err(st)?)?;
    let retire = format!("DELETE FROM edge_base WHERE {predicate} RETURNING source, target, kind");
    let mut q = sqlx::query(&retire);
    if let Some(a) = arg {
        q = q.bind(a);
    }
    let retired = pg_keys(&q.fetch_all(&mut *c).await.map_err(st)?)?;
    Ok(Some((deleted, retired)))
}

/// The other half, AFTER the DELETE: re-project the captured keys. Returns how many deleted
/// supported rows came back (all of them — support is producer-owned).
async fn pg_support_post_delete(
    c: &mut sqlx::PgConnection,
    captured: Option<(BTreeSet<EdgeKey>, BTreeSet<EdgeKey>)>,
) -> Result<usize> {
    let Some((deleted, retired)) = captured else {
        return Ok(0);
    };
    for key in deleted.union(&retired) {
        pg_reproject(&mut *c, key).await?;
    }
    Ok(deleted.len())
}

async fn pg_replace_edge_supports(
    c: &mut sqlx::PgConnection,
    owner: &SupportOwner,
    generation: u64,
    incoming: Vec<SupportFact>,
) -> Result<SupportReplacement> {
    pg_support_lock(&mut *c, true).await?;
    let stored_generation: Option<i64> = sqlx::query_scalar(
        "SELECT generation FROM support_owners WHERE producer = $1 AND snapshot = $2",
    )
    .bind(&owner.producer)
    .bind(&owner.snapshot)
    .fetch_optional(&mut *c)
    .await
    .map_err(st)?;
    let rows = sqlx::query(
        "SELECT source, target, kind, fact_id, data FROM edge_supports \
         WHERE producer = $1 AND snapshot = $2",
    )
    .bind(&owner.producer)
    .bind(&owner.snapshot)
    .fetch_all(&mut *c)
    .await
    .map_err(st)?;
    let mut stored = StoredFacts::new();
    for r in &rows {
        let data: String = r.try_get("data").map_err(st)?;
        let fact: Edge = serde_json::from_str(&data)?;
        stored.insert(
            (
                (
                    r.try_get("source").map_err(st)?,
                    r.try_get("target").map_err(st)?,
                    r.try_get("kind").map_err(st)?,
                ),
                r.try_get("fact_id").map_err(st)?,
            ),
            fact_key(&fact),
        );
    }
    let plan = plan_replacement(
        owner,
        generation,
        stored_generation.map(|g| g as u64),
        &stored,
        incoming,
    )?;
    if plan.report.replayed {
        return Ok(plan.report);
    }
    for key in &plan.touched {
        // First support on this key: the base plane's edge becomes its base contribution.
        if !pg_key_supported(&mut *c, key).await? {
            sqlx::query("DELETE FROM edge_base WHERE source = $1 AND target = $2 AND kind = $3")
                .bind(&key.0)
                .bind(&key.1)
                .bind(&key.2)
                .execute(&mut *c)
                .await
                .map_err(st)?;
            sqlx::query(
                "INSERT INTO edge_base(source, target, kind, file, data) \
                 SELECT source, target, kind, file, data FROM edges \
                 WHERE source = $1 AND target = $2 AND kind = $3",
            )
            .bind(&key.0)
            .bind(&key.1)
            .bind(&key.2)
            .execute(&mut *c)
            .await
            .map_err(st)?;
        }
    }
    for (key, fact_id) in &plan.delete {
        sqlx::query(
            "DELETE FROM edge_supports WHERE producer = $1 AND snapshot = $2 \
             AND source = $3 AND target = $4 AND kind = $5 AND fact_hash = $6",
        )
        .bind(&owner.producer)
        .bind(&owner.snapshot)
        .bind(&key.0)
        .bind(&key.1)
        .bind(&key.2)
        .bind(fact_hash(fact_id))
        .execute(&mut *c)
        .await
        .map_err(st)?;
    }
    for fact in &plan.insert {
        sqlx::query(
            "INSERT INTO edge_supports(producer, snapshot, source, target, kind, fact_hash, fact_id, data) \
             VALUES($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&owner.producer)
        .bind(&owner.snapshot)
        .bind(&fact.key.0)
        .bind(&fact.key.1)
        .bind(&fact.key.2)
        .bind(fact_hash(&fact.fact_id))
        .bind(&fact.fact_id)
        .bind(serde_json::to_string(&fact.edge)?)
        .execute(&mut *c)
        .await
        .map_err(st)?;
    }
    sqlx::query(
        "INSERT INTO support_owners(producer, snapshot, generation) VALUES($1, $2, $3) \
         ON CONFLICT(producer, snapshot) DO UPDATE SET generation = EXCLUDED.generation",
    )
    .bind(&owner.producer)
    .bind(&owner.snapshot)
    .bind(generation as i64)
    .execute(&mut *c)
    .await
    .map_err(st)?;
    for key in &plan.touched {
        pg_reproject(&mut *c, key).await?;
    }
    Ok(plan.report)
}

// ── GraphWrite ────────────────────────────────────────────────────────────────

impl GraphWrite for PostgresStore {
    fn begin_batch(&mut self) -> Result<()> {
        // Locked decision #8: the graph batch is ONE real transaction at READ COMMITTED, so
        // concurrent readers (shared_writers: true) see old-or-new, never a partial batch.
        // Idempotent like SqliteStore: a second begin_batch inside an open batch is a no-op.
        // SERIALIZABLE is reserved for merge-critical paths if they ever need it — the graph
        // batch is a single-writer bulk load, so READ COMMITTED is sufficient and cheaper.
        if self
            .batch
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        {
            return Ok(());
        }
        let tx = rt_block(async {
            let mut tx = self.pool.begin().await?;
            // Explicit rather than relying on the server default: a cluster configured with
            // default_transaction_isolation=serializable must not change batch semantics.
            sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
                .execute(&mut *tx)
                .await?;
            Ok::<_, sqlx::Error>(tx)
        })
        .map_err(st)?;
        *self
            .batch
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tx);
        Ok(())
    }

    fn commit_batch(&mut self) -> Result<()> {
        // No open batch → no-op (mirrors SqliteStore).
        let tx = self
            .batch
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(tx) = tx {
            rt_block(tx.commit()).map_err(st)?;
        }
        Ok(())
    }

    fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()> {
        let mut h = self.conn()?;
        for n in nodes {
            let data = serde_json::to_string(n)?;

            // Epoch pre-pass (M8/DoD-XA4), BEFORE the node insert — same rule as the SQLite seam:
            // bump iff this symbol HAD a node (symbol_gen.had_node==1) and has none now (a reuse).
            // The live-node check reads the pre-insert state (through the batch transaction when
            // one is open, so it sees nodes written earlier in the same batch).
            rt_block(async {
                let row: Option<sqlx::postgres::PgRow> = sqlx::query(
                    "SELECT COALESCE(sg.had_node, 0) AS had_node, \
                            EXISTS(SELECT 1 FROM nodes nn WHERE nn.symbol = $1) AS has_live \
                     FROM (SELECT $1::text AS s) q \
                     LEFT JOIN symbol_gen sg ON sg.symbol = q.s",
                )
                .bind(&n.symbol.0)
                .fetch_optional(h.as_conn())
                .await?;
                let (had_node, has_live): (i64, bool) = match row {
                    Some(r) => (r.try_get("had_node")?, r.try_get("has_live")?),
                    None => (0, false),
                };
                let bump: i64 = if had_node == 1 && !has_live { 1 } else { 0 };
                // Upsert the marker: first sight INSERT (gen 0, had_node 1); thereafter add `bump`
                // and keep had_node sticky at 1.
                sqlx::query(
                    "INSERT INTO symbol_gen(symbol, gen, had_node) VALUES($1, 0, 1) \
                     ON CONFLICT(symbol) DO UPDATE SET \
                       gen = symbol_gen.gen + $2, had_node = 1",
                )
                .bind(&n.symbol.0)
                .bind(bump)
                .execute(h.as_conn())
                .await?;
                Ok::<(), sqlx::Error>(())
            })
            .map_err(st)?;

            // Multi-file contributions (M4 / Option A — wicked-estate#152), mirroring the SQLite
            // seam: record this write as the file's CONTRIBUTION, then derive the nodes row from
            // the PREFERRED contribution (is_def DESC, file ASC — definition-first, deterministic
            // tiebreak), never last-write-wins.
            let is_def: i64 = if n.is_declaration() { 0 } else { 1 };
            let (pref_file, pref_data): (String, String) = rt_block(async {
                sqlx::query(
                    "INSERT INTO node_files(symbol, file, is_def, data) VALUES($1, $2, $3, $4)
                     ON CONFLICT(symbol, file) DO UPDATE SET
                       is_def = EXCLUDED.is_def, data = EXCLUDED.data",
                )
                .bind(&n.symbol.0)
                .bind(&n.location.file)
                .bind(is_def)
                .bind(&data)
                .execute(h.as_conn())
                .await?;
                let row = sqlx::query(
                    "SELECT file, data FROM node_files WHERE symbol = $1 \
                     ORDER BY is_def DESC, file ASC LIMIT 1",
                )
                .bind(&n.symbol.0)
                .fetch_one(h.as_conn())
                .await?;
                Ok::<(String, String), sqlx::Error>((row.try_get("file")?, row.try_get("data")?))
            })
            .map_err(st)?;
            // Project the preferred record wholesale; skip the JSON re-parse when the record just
            // written IS the preferred one (the single-contribution common case).
            let parsed: Option<Node> = if pref_file == n.location.file {
                None
            } else {
                Some(serde_json::from_str(&pref_data)?)
            };
            let p: &Node = parsed.as_ref().unwrap_or(n);
            let p_data: &str = if parsed.is_some() { &pref_data } else { &data };
            let kind = serde_json::to_string(&p.kind)?;

            rt_block(
                sqlx::query(
                    "INSERT INTO nodes(symbol, name, kind, language, file, data, scope)
                     VALUES($1, $2, $3, $4, $5, $6, $7)
                     ON CONFLICT(symbol) DO UPDATE SET
                       name     = EXCLUDED.name,
                       kind     = EXCLUDED.kind,
                       language = EXCLUDED.language,
                       file     = EXCLUDED.file,
                       data     = EXCLUDED.data,
                       scope    = EXCLUDED.scope",
                )
                .bind(&n.symbol.0)
                .bind(&p.name)
                .bind(&kind)
                .bind(&p.language.0)
                .bind(&p.location.file)
                .bind(p_data)
                .bind(p.scope.as_path())
                .execute(h.as_conn()),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()> {
        if !self.batch_open() {
            return self.in_implicit_batch(|s| s.upsert_edges(edges));
        }
        let mut h = self.conn()?;
        // TS-S2A: while no support exists anywhere this is the original single-statement path.
        // The shared support lock first, so no replacement can interleave with this write.
        let supported = rt_block(async {
            pg_support_lock(h.as_conn(), false).await?;
            pg_any_support(h.as_conn()).await
        })?;
        for e in edges {
            let kind = serde_json::to_string(&e.kind)?;
            if supported {
                let key: EdgeKey = (e.source.0.clone(), e.target.0.clone(), kind.clone());
                let handled = rt_block(async {
                    let c = h.as_conn();
                    if !pg_key_supported(&mut *c, &key).await? {
                        return Ok::<bool, Error>(false);
                    }
                    // The base rule applies to the kept-aside base contribution; the public row
                    // is re-projected — a base write never erases support.
                    let wins = pg_edge_base_of(&mut *c, &key)
                        .await?
                        .is_none_or(|(existing, _)| base_upsert_wins(&existing, e));
                    if wins {
                        pg_set_edge_base(&mut *c, &key, e).await?;
                        pg_reproject(&mut *c, &key).await?;
                    }
                    Ok(true)
                })?;
                if handled {
                    continue;
                }
            }
            let data = serde_json::to_string(e)?;
            let file = e.location.as_ref().map(|l| l.file.as_str()).unwrap_or("");
            let confidence = e.confidence.get() as f64;
            rt_block(
                sqlx::query(
                    // Higher-confidence-wins (W3.4), UNLESS the incoming edge carries more
                    // evidence — evidence_count is a monotonic audit counter, so growth means
                    // strictly newer information (see SqliteStore::upsert_edges). Here it rides
                    // in the `data` JSON (no promoted column); absent on pre-field rows → 0.
                    "INSERT INTO edges(source, target, kind, confidence, file, data)
                     VALUES($1, $2, $3, $4, $5, $6)
                     ON CONFLICT(source, target, kind) DO UPDATE SET
                       confidence = EXCLUDED.confidence,
                       file       = EXCLUDED.file,
                       data       = EXCLUDED.data
                     WHERE EXCLUDED.confidence >= edges.confidence
                        OR COALESCE((EXCLUDED.data::jsonb->>'evidence_count')::bigint, 0)
                           > COALESCE((edges.data::jsonb->>'evidence_count')::bigint, 0)",
                )
                .bind(&e.source.0)
                .bind(&e.target.0)
                .bind(&kind)
                .bind(confidence)
                .bind(file)
                .bind(&data)
                .execute(h.as_conn()),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()> {
        let mut h = self.conn()?;
        for r in refs {
            let kind = serde_json::to_string(&r.kind)?;
            let file = &r.location.file;
            let line = r.location.span.start_line as i64;
            let start_byte = r.location.span.start_byte as i64;
            let end_byte = r.location.span.end_byte as i64;
            rt_block(
                sqlx::query(
                    "INSERT INTO unresolved_refs(from_sym, raw_name, kind, file, line, start_byte, end_byte)
                     VALUES($1, $2, $3, $4, $5, $6, $7)",
                )
                .bind(&r.from.0)
                .bind(&r.raw_name)
                .bind(&kind)
                .bind(file)
                .bind(line)
                .bind(start_byte)
                .bind(end_byte)
                .execute(h.as_conn()),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn remove_file(&mut self, file: &str) -> Result<()> {
        if !self.batch_open() {
            return self.in_implicit_batch(|s| s.remove_file(file));
        }
        let mut h = self.conn()?;
        // Step 1: read the file's current git_sha.
        let current_git_sha: String = rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT git_sha FROM files WHERE path = $1")
                    .bind(file)
                    .fetch_optional(h.as_conn())
                    .await?;
            Ok::<String, sqlx::Error>(
                row.and_then(|r| r.try_get::<Option<String>, _>("git_sha").ok().flatten())
                    .unwrap_or_default(),
            )
        })
        .map_err(st)?;

        // Step 1a: multi-file contribution retirement + survivor re-home (M4 / Option A —
        // wicked-estate#152, mirroring SqliteStore — see the comment there). Delete this file's
        // CONTRIBUTIONS; a node homed here with contributions surviving in other files is re-homed
        // wholesale (every projected column + the data JSON) to the preferred survivor
        // (is_def DESC, file ASC) instead of being deleted. Runs BEFORE the archive/edge/node
        // steps, so their `nodes.file = $1` sub-selects exclude kept nodes.
        {
            struct KeptContribution {
                symbol: String,
                node: Node,
                data: String,
            }
            let candidates: Vec<(String, Option<String>)> = rt_block(async {
                let rows = sqlx::query(
                    "SELECT n.symbol, \
                            (SELECT nf.data FROM node_files nf \
                              WHERE nf.symbol = n.symbol AND nf.file <> $1 \
                              ORDER BY nf.is_def DESC, nf.file ASC LIMIT 1) AS surv \
                     FROM nodes n \
                     WHERE n.file = $1",
                )
                .bind(file)
                .fetch_all(h.as_conn())
                .await?;
                let mut v = Vec::new();
                for row in rows {
                    v.push((
                        row.try_get::<String, _>("symbol")?,
                        row.try_get::<Option<String>, _>("surv")?,
                    ));
                }
                Ok::<_, sqlx::Error>(v)
            })
            .map_err(st)?;
            let mut kept: Vec<KeptContribution> = Vec::new();
            for (symbol, surv) in candidates {
                let Some(data) = surv else { continue };
                let node: Node = serde_json::from_str(&data)?;
                kept.push(KeptContribution { symbol, node, data });
            }
            rt_block(
                sqlx::query("DELETE FROM node_files WHERE file = $1")
                    .bind(file)
                    .execute(h.as_conn()),
            )
            .map_err(st)?;
            for k in &kept {
                let kind = serde_json::to_string(&k.node.kind)?;
                rt_block(
                    sqlx::query(
                        "UPDATE nodes SET name = $2, kind = $3, language = $4, file = $5, \
                                          data = $6, scope = $7 \
                         WHERE symbol = $1",
                    )
                    .bind(&k.symbol)
                    .bind(&k.node.name)
                    .bind(&kind)
                    .bind(&k.node.language.0)
                    .bind(&k.node.location.file)
                    .bind(&k.data)
                    .bind(k.node.scope.as_path())
                    .execute(h.as_conn()),
                )
                .map_err(st)?;
            }
        }

        // Step 2: archive edges to edge_history if history is enabled.
        if self.history_enabled {
            let edge_jsons: Vec<String> = rt_block(async {
                let rows = sqlx::query(
                    "SELECT data FROM edges \
                     WHERE file = $1 \
                        OR source IN (SELECT symbol FROM nodes WHERE file = $1)",
                )
                .bind(file)
                .fetch_all(h.as_conn())
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
                    .execute(h.as_conn()),
                )
                .map_err(st)?;
            }
        }

        // Step 2b: shared-Import keep + re-home (incr-integrity lane, D1/D2/D4 — mirrors
        // SqliteStore; see the comment there). ONE survivor predicate, computed once per call:
        // an edge targeting the candidate whose file is neither '' nor $1 and whose source does
        // not live in $1 (both exclusions are exactly what the Step-3 DELETE removes, so the
        // pre-delete evaluation equals post-delete state). Kept nodes are re-homed to MIN(file)
        // over the survivors — BOTH the `nodes.file` column and the `location` in the data JSON.
        let import_kind = serde_json::to_string(&NodeKind::Import)?;
        struct KeptImport {
            symbol: String,
            new_file: String,
            new_data: String,
        }
        let kept: Vec<KeptImport> = {
            let candidates: Vec<(String, String, Option<String>)> = rt_block(async {
                let rows = sqlx::query(
                    "SELECT n.symbol, n.data, \
                            (SELECT MIN(e.file) FROM edges e \
                              WHERE e.target = n.symbol \
                                AND e.file NOT IN ('', $1) \
                                AND e.source NOT IN \
                                    (SELECT symbol FROM nodes WHERE file = $1)) AS new_home \
                     FROM nodes n \
                     WHERE n.file = $1 AND n.kind = $2",
                )
                .bind(file)
                .bind(&import_kind)
                .fetch_all(h.as_conn())
                .await?;
                let mut v = Vec::new();
                for row in rows {
                    v.push((
                        row.try_get::<String, _>("symbol")?,
                        row.try_get::<String, _>("data")?,
                        row.try_get::<Option<String>, _>("new_home")?,
                    ));
                }
                Ok::<_, sqlx::Error>(v)
            })
            .map_err(st)?;
            let mut kept = Vec::new();
            for (symbol, data, new_home) in candidates {
                let Some(new_file) = new_home else { continue };
                // The survivor edge at MIN(file) supplies the new location (file + span).
                let edge_json: Option<String> = rt_block(async {
                    let row = sqlx::query(
                        "SELECT e.data FROM edges e \
                         WHERE e.target = $1 AND e.file = $2 \
                           AND e.source NOT IN (SELECT symbol FROM nodes WHERE file = $3) \
                         ORDER BY e.kind, e.source LIMIT 1",
                    )
                    .bind(&symbol)
                    .bind(&new_file)
                    .bind(file)
                    .fetch_optional(h.as_conn())
                    .await?;
                    Ok::<_, sqlx::Error>(match row {
                        Some(r) => Some(r.try_get::<String, _>("data")?),
                        None => None,
                    })
                })
                .map_err(st)?;
                let new_loc = edge_json
                    .and_then(|j| serde_json::from_str::<Edge>(&j).ok())
                    .and_then(|e| e.location)
                    .unwrap_or_else(|| {
                        wicked_estate_core::Location::new(
                            new_file.clone(),
                            wicked_estate_core::Span::ZERO,
                        )
                    });
                let mut node: Node = serde_json::from_str(&data)?;
                node.location = new_loc;
                kept.push(KeptImport {
                    symbol,
                    new_file,
                    new_data: serde_json::to_string(&node)?,
                });
            }
            kept
        };

        // TS-S2A: retire supported keys' base contributions by exactly the edge predicate, and
        // capture the supported rows this DELETE removes (healed below).
        let support = rt_block(pg_support_pre_delete(
            h.as_conn(),
            "file = $1 OR source IN (SELECT symbol FROM nodes WHERE file = $1)",
            Some(file),
        ))?;

        // Step 3: delete edges BEFORE nodes (subquery on nodes must still be valid).
        rt_block(
            sqlx::query(
                "DELETE FROM edges \
                 WHERE file = $1 \
                    OR source IN (SELECT symbol FROM nodes WHERE file = $1)",
            )
            .bind(file)
            .execute(h.as_conn()),
        )
        .map_err(st)?;

        // Step 4: re-home kept Import nodes (so the by-file DELETE below no longer matches
        // them), then delete nodes, unresolved_refs, files row.
        for k in &kept {
            rt_block(
                sqlx::query("UPDATE nodes SET file = $2, data = $3 WHERE symbol = $1")
                    .bind(&k.symbol)
                    .bind(&k.new_file)
                    .bind(&k.new_data)
                    .execute(h.as_conn()),
            )
            .map_err(st)?;
        }
        rt_block(
            sqlx::query("DELETE FROM nodes WHERE file = $1")
                .bind(file)
                .execute(h.as_conn()),
        )
        .map_err(st)?;

        rt_block(
            sqlx::query("DELETE FROM unresolved_refs WHERE file = $1")
                .bind(file)
                .execute(h.as_conn()),
        )
        .map_err(st)?;

        rt_block(
            sqlx::query("DELETE FROM files WHERE path = $1")
                .bind(file)
                .execute(h.as_conn()),
        )
        .map_err(st)?;
        // Support is producer-owned: re-project every supported edge the deletes removed.
        rt_block(pg_support_post_delete(h.as_conn(), support))?;

        Ok(())
    }

    fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()> {
        let mut h = self.conn()?;
        rt_block(
            sqlx::query(
                "INSERT INTO files(path, digest) VALUES($1, $2) \
                 ON CONFLICT(path) DO UPDATE SET digest = EXCLUDED.digest",
            )
            .bind(file)
            .bind(digest)
            .execute(h.as_conn()),
        )
        .map_err(st)?;
        Ok(())
    }

    fn set_file_content(&mut self, file: &str, text: &str) -> Result<()> {
        let sha = git_blob_sha(text);
        let mut h = self.conn()?;
        // Dedup: INSERT … ON CONFLICT DO NOTHING — identical content shares one content row.
        rt_block(
            sqlx::query("INSERT INTO content(git_sha, body) VALUES($1, $2) ON CONFLICT DO NOTHING")
                .bind(&sha)
                .bind(text)
                .execute(h.as_conn()),
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
            .execute(h.as_conn()),
        )
        .map_err(st)?;
        Ok(())
    }

    fn prune_dangling_edges(&mut self) -> Result<usize> {
        if !self.batch_open() {
            return self.in_implicit_batch(|s| s.prune_dangling_edges());
        }
        let mut h = self.conn()?;
        // TS-S2A: a dangling supported key loses its base contribution (same predicate) but not
        // its support; it is re-projected below and not counted as pruned.
        let support = rt_block(pg_support_pre_delete(
            h.as_conn(),
            "source NOT IN (SELECT symbol FROM nodes) OR target NOT IN (SELECT symbol FROM nodes)",
            None,
        ))?;
        let result = rt_block(
            sqlx::query(
                "DELETE FROM edges \
                 WHERE source NOT IN (SELECT symbol FROM nodes) \
                    OR target NOT IN (SELECT symbol FROM nodes)",
            )
            .execute(h.as_conn()),
        )
        .map_err(st)?;
        let restored = rt_block(pg_support_post_delete(h.as_conn(), support))?;
        Ok((result.rows_affected() as usize).saturating_sub(restored))
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
        let mut h = self.conn()?;
        rt_block(
            sqlx::query("INSERT INTO changes(op, target) VALUES($1, $2)")
                .bind(op_str)
                .bind(target)
                .execute(h.as_conn()),
        )
        .map_err(st)?;
        Ok(())
    }

    fn set_node_semantics(
        &mut self,
        symbol: &SymbolId,
        description: Option<&str>,
        requirement: Option<&str>,
        validation: Option<&wicked_estate_core::ValidationClaim>,
    ) -> Result<()> {
        if description.is_none() && requirement.is_none() && validation.is_none() {
            return Ok(());
        }
        let mut h = self.conn()?;
        // Check the symbol exists (no intern for Postgres — directly query nodes).
        let exists: bool = rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT 1 AS exists_flag FROM nodes WHERE symbol = $1")
                    .bind(&symbol.0)
                    .fetch_optional(h.as_conn())
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
                    .execute(h.as_conn()),
            )
            .map_err(st)?;
        }
        if let Some(r) = requirement {
            rt_block(
                sqlx::query("UPDATE nodes SET requirement = $2 WHERE symbol = $1")
                    .bind(&symbol.0)
                    .bind(r)
                    .execute(h.as_conn()),
            )
            .map_err(st)?;
        }
        if let Some(claim) = validation {
            // Flag, author and timestamp move in ONE statement. Splitting them would allow a
            // validated row with no author — the unattributable state `ValidationClaim` exists to
            // make unrepresentable (mirrors SqliteStore).
            let flag: i64 = claim.validated as i64;
            let now: i64 = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            rt_block(
                sqlx::query(
                    "UPDATE nodes SET requirement_validated = $2, requirement_validated_by = $3, \
                     requirement_validated_at = $4 WHERE symbol = $1",
                )
                .bind(&symbol.0)
                .bind(flag)
                .bind(&claim.by)
                .bind(now)
                .execute(h.as_conn()),
            )
            .map_err(st)?;
        }
        Ok(())
    }

    fn annotate(&mut self, symbol: &SymbolId, annotation: Annotation) -> Result<()> {
        let mut h = self.conn()?;
        rt_block(async {
            // An un-interned symbol is not a node → no-op (mirrors SqliteStore).
            let exists: bool = sqlx::query("SELECT 1 AS e FROM nodes WHERE symbol = $1")
                .bind(&symbol.0)
                .fetch_optional(h.as_conn())
                .await?
                .is_some();
            if !exists {
                return Ok::<(), sqlx::Error>(());
            }
            // Bare INSERT (NOT upsert): a symbol may carry MANY annotations, including a duplicate
            // (type, key). When ts is unset (0) let the column DEFAULT (NOW epoch) stamp it.
            if annotation.ts == 0 {
                sqlx::query(
                    "INSERT INTO annotations(node_sym, key, value, confidence, provenance, author, \"type\", source_type, extraction_method, last_verified) \
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
                .execute(h.as_conn())
                .await?;
            } else {
                sqlx::query(
                    "INSERT INTO annotations(node_sym, key, value, confidence, provenance, author, ts, \"type\", source_type, extraction_method, last_verified) \
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
                .execute(h.as_conn())
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
        let mut h = self.conn()?;
        let n = rt_block(async {
            // $2 IS NULL → key-only (all types); otherwise scope to (type = $2, key = $3).
            // `type` is matched as an opaque string — no per-type branching (rules-as-DATA).
            let result = sqlx::query(
                "DELETE FROM annotations \
                 WHERE node_sym = $1 AND key = $3 AND ($2::TEXT IS NULL OR \"type\" = $2)",
            )
            .bind(&symbol.0)
            .bind(ty)
            .bind(key)
            .execute(h.as_conn())
            .await?;
            Ok::<u64, sqlx::Error>(result.rows_affected())
        })
        .map_err(st)?;
        Ok(n as usize)
    }

    fn replace_edge_supports(
        &mut self,
        owner: &SupportOwner,
        generation: u64,
        facts: &[SupportFact],
    ) -> Result<SupportReplacement> {
        let incoming = normalize_facts(facts)?;
        let mut h = self.conn()?;
        rt_block(async {
            // `Connection::begin` on the handle's connection: a real transaction when no batch is
            // open, a SAVEPOINT inside an open batch — all-or-nothing in both modes. Inside a
            // batch the exclusive support lock is held until the batch ends.
            use sqlx::Connection;
            let mut tx = h.as_conn().begin().await.map_err(st)?;
            match pg_replace_edge_supports(&mut tx, owner, generation, incoming).await {
                Ok(report) => {
                    tx.commit().await.map_err(st)?;
                    Ok(report)
                }
                Err(e) => {
                    // Report the ORIGINAL error even if the rollback itself fails.
                    match tx.rollback().await {
                        Ok(()) => Err(e),
                        Err(rollback) => Err(Error::Storage(format!(
                            "{e}; rolling the replacement back also failed ({rollback})"
                        ))),
                    }
                }
            }
        })
    }
}

// ── GraphRead ─────────────────────────────────────────────────────────────────

impl GraphRead for PostgresStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            full_text_search: true,
            vector_search: false,
            server_side_traversal: true,
            // begin/commit map to ONE real READ COMMITTED transaction (locked decision #8) —
            // concurrent readers never observe a partial batch.
            transactional_batch: true,
            shared_writers: true, // Postgres supports concurrent writers
        }
    }

    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT data FROM nodes WHERE symbol = $1")
                    .bind(&id.0)
                    .fetch_optional(h.as_conn())
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

    fn symbol_epoch(&self, id: &SymbolId) -> Result<Option<u64>> {
        // Live only: the JOIN against nodes returns a row ONLY when a live node exists for the
        // symbol; the gen comes from symbol_gen (which survives remove_file). No live node → None.
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query(
                "SELECT COALESCE(sg.gen, 0) AS gen \
                 FROM nodes n \
                 LEFT JOIN symbol_gen sg ON sg.symbol = n.symbol \
                 WHERE n.symbol = $1",
            )
            .bind(&id.0)
            .fetch_optional(h.as_conn())
            .await?;
            match row {
                None => Ok::<Option<u64>, sqlx::Error>(None),
                Some(r) => {
                    let epoch: i64 = r.try_get("gen")?;
                    Ok(Some(epoch as u64))
                }
            }
        })
        .map_err(st)
    }

    fn edge_supports(
        &self,
        source: &SymbolId,
        target: &SymbolId,
        kind: &wicked_estate_core::EdgeKind,
    ) -> Result<Vec<EdgeSupport>> {
        let kind = serde_json::to_string(kind)?;
        let mut h = self.conn()?;
        let rows = rt_block(
            sqlx::query(
                "SELECT s.producer, s.snapshot, o.generation, s.fact_id, s.data \
                 FROM edge_supports s JOIN support_owners o \
                   ON o.producer = s.producer AND o.snapshot = s.snapshot \
                 WHERE s.source = $1 AND s.target = $2 AND s.kind = $3",
            )
            .bind(&source.0)
            .bind(&target.0)
            .bind(&kind)
            .fetch_all(h.as_conn()),
        )
        .map_err(st)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let generation: i64 = r.try_get("generation").map_err(st)?;
            let data: String = r.try_get("data").map_err(st)?;
            out.push(EdgeSupport::new(
                SupportOwner::new(
                    r.try_get::<String, _>("producer").map_err(st)?,
                    r.try_get::<String, _>("snapshot").map_err(st)?,
                )?,
                generation as u64,
                r.try_get::<String, _>("fact_id").map_err(st)?,
                serde_json::from_str(&data)?,
            ));
        }
        // Sorted here, not by ORDER BY: Postgres text order follows the database collation, and
        // the contract is byte order — the order MemStore and SQLite (BINARY) return.
        out.sort_by(|a, b| (&a.owner, &a.fact_id).cmp(&(&b.owner, &b.fact_id)));
        Ok(out)
    }

    fn support_generation(&self, owner: &SupportOwner) -> Result<Option<u64>> {
        let mut h = self.conn()?;
        let generation: Option<i64> = rt_block(
            sqlx::query_scalar(
                "SELECT generation FROM support_owners WHERE producer = $1 AND snapshot = $2",
            )
            .bind(&owner.producer)
            .bind(&owner.snapshot)
            .fetch_optional(h.as_conn()),
        )
        .map_err(st)?;
        Ok(generation.map(|g| g as u64))
    }

    fn support_owners(&self) -> Result<Vec<SupportOwnerState>> {
        let mut h = self.conn()?;
        let rows = rt_block(
            sqlx::query("SELECT producer, snapshot, generation FROM support_owners")
                .fetch_all(h.as_conn()),
        )
        .map_err(st)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let generation: i64 = r.try_get("generation").map_err(st)?;
            out.push(SupportOwnerState::new(
                SupportOwner::new(
                    r.try_get::<String, _>("producer").map_err(st)?,
                    r.try_get::<String, _>("snapshot").map_err(st)?,
                )?,
                generation as u64,
            ));
        }
        // Byte order, not the database collation (see `edge_supports`).
        out.sort_by(|a, b| a.owner.cmp(&b.owner));
        Ok(out)
    }

    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
        let mut h = self.conn()?;
        let mut nodes: Vec<Node> = if let Some(text) = &query.text {
            let pattern = format!("%{text}%");
            rt_block(async {
                let rows = sqlx::query(
                    "SELECT data FROM nodes WHERE name ILIKE $1 OR data ILIKE $1 ORDER BY symbol",
                )
                .bind(&pattern)
                .fetch_all(h.as_conn())
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
                    .fetch_all(h.as_conn())
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
        } else if !query.kinds.is_empty() {
            // INDEXED by-kind retrieval: push `kinds` into SQL so `idx_nodes_kind` is used
            // (O(matches), not a full scan). `nodes.kind` stores serde_json::to_string(&NodeKind),
            // so bind each kind in that same form. The Rust `retain` below stays as a backstop.
            let mut kind_strs: Vec<String> = Vec::with_capacity(query.kinds.len());
            for k in &query.kinds {
                kind_strs.push(serde_json::to_string(k)?);
            }
            rt_block(async {
                let rows =
                    sqlx::query("SELECT data FROM nodes WHERE kind = ANY($1) ORDER BY symbol")
                        .bind(&kind_strs[..])
                        .fetch_all(h.as_conn())
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
                    .fetch_all(h.as_conn())
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

        // Apply remaining filters in Rust. Scope is filtered before the limit truncate below, so a
        // scoped query never leaks another scope's rows into/out of the top-k (multi-tenant isolation).
        nodes.retain(|n| {
            if let Some(prefix) = &query.scope_prefix {
                if !n.scope.path_in_prefix(prefix) {
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
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query(sql).bind(&id.0).fetch_all(h.as_conn()).await?;
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
        // `Both` ORs the two directions' horizon flags: a cut in EITHER direction makes the
        // merged subgraph incomplete.
        let (depths, node_cap, depth_horizon): (BTreeMap<String, u32>, bool, bool) =
            match spec.direction {
                Direction::Both => {
                    let (mut merged, h1) = self.cte_reach(start, Direction::Dependents, spec)?;
                    let (other, h2) = self.cte_reach(start, Direction::Dependencies, spec)?;
                    for (k, v) in other {
                        // Keep the minimum depth when a node is reachable from both directions.
                        merged
                            .entry(k)
                            .and_modify(|e| *e = (*e).min(v))
                            .or_insert(v);
                    }
                    let was_truncated = merged.len() > spec.max_nodes;
                    // Sort by depth and keep only the closest max_nodes nodes.
                    let mut pairs: Vec<(String, u32)> = merged.into_iter().collect();
                    pairs.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
                    pairs.truncate(spec.max_nodes);
                    (pairs.into_iter().collect(), was_truncated, h1 || h2)
                }
                d => {
                    let (raw, horizon) = self.cte_reach(start, d, spec)?;
                    // cte_reach fetches max_nodes+1; more than max_nodes means something was cut.
                    let was_truncated = raw.len() > spec.max_nodes;
                    // #226: truncate by depth (then id), never by id alone — the map is ordered by
                    // symbol id, so a bare truncate could drop a near node and keep a far one.
                    let mut pairs: Vec<(String, u32)> = raw.into_iter().collect();
                    pairs.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
                    pairs.truncate(spec.max_nodes);
                    (pairs.into_iter().collect(), was_truncated, horizon)
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
            ..Default::default()
        }
        .with_caps(node_cap, depth_horizon))
    }

    fn all_nodes(&self) -> Result<Vec<Node>> {
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query("SELECT data FROM nodes")
                .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query("SELECT data FROM edges")
                .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query(
                "SELECT from_sym, raw_name, kind, file, line, start_byte, end_byte \
                 FROM unresolved_refs WHERE raw_name = $1",
            )
            .bind(name)
            .fetch_all(h.as_conn())
            .await?;
            let mut out = Vec::new();
            for row in rows {
                let from_sym: String = row.try_get("from_sym")?;
                let raw_name: String = row.try_get("raw_name")?;
                let kind_json: String = row.try_get("kind")?;
                let file: String = row.try_get("file")?;
                let line: i64 = row.try_get("line")?;
                let start_byte: i64 = row.try_get("start_byte")?;
                let end_byte: i64 = row.try_get("end_byte")?;
                let kind = serde_json::from_str(&kind_json)
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
                // start_byte/end_byte read from their columns (admissibility F-B); the
                // remaining span fields are not persisted and reconstruct as 0.
                let location = Location::new(
                    file,
                    Span {
                        start_line: line as u32,
                        start_byte: start_byte as u32,
                        end_byte: end_byte as u32,
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

    fn indexed_files(&self) -> Result<Vec<String>> {
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query("SELECT path FROM files")
                .fetch_all(h.as_conn())
                .await?;
            // `?`, not `.ok()`. A dropped row here does not fail loudly — it silently shrinks the
            // "previously indexed" set, and the caller reads that as "this path was never indexed",
            // so a file deleted from disk is never swept. A decode failure must surface as an error,
            // not as a quietly incomplete answer.
            rows.iter()
                .map(|r| r.try_get::<String, _>("path"))
                .collect::<std::result::Result<Vec<String>, sqlx::Error>>()
        })
        .map_err(st)
    }

    fn file_digest(&self, file: &str) -> Result<Option<String>> {
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT digest FROM files WHERE path = $1")
                    .bind(file)
                    .fetch_optional(h.as_conn())
                    .await?;
            Ok::<Option<String>, sqlx::Error>(row.and_then(|r| r.try_get("digest").ok()))
        })
        .map_err(st)
    }

    fn file_git_sha(&self, file: &str) -> Result<Option<String>> {
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> =
                sqlx::query("SELECT git_sha FROM files WHERE path = $1")
                    .bind(file)
                    .fetch_optional(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query(
                "SELECT seq, op, target FROM changes \
                 WHERE seq > $1 ORDER BY seq ASC LIMIT 10000",
            )
            .bind(cursor as i64)
            .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query(
                "SELECT archived_seq, git_sha, edge_json FROM edge_history \
                 WHERE file = $1 ORDER BY archived_seq DESC",
            )
            .bind(file)
            .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query(
                "SELECT c.body FROM files f \
                 JOIN content c ON c.git_sha = f.git_sha \
                 WHERE f.path = $1",
            )
            .bind(file)
            .fetch_optional(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query(
                "SELECT description, requirement, requirement_validated, \
                        requirement_validated_by, requirement_validated_at \
                 FROM nodes \
                 WHERE symbol = $1 \
                   AND (description IS NOT NULL \
                        OR requirement IS NOT NULL \
                        OR requirement_validated != 0)",
            )
            .bind(&symbol.0)
            .fetch_optional(h.as_conn())
            .await?;
            match row {
                None => Ok::<Option<NodeSemantics>, sqlx::Error>(None),
                Some(r) => {
                    let description: Option<String> = r.try_get("description")?;
                    let requirement: Option<String> = r.try_get("requirement")?;
                    let validated_int: i64 = r.try_get("requirement_validated")?;
                    let by: Option<String> = r.try_get("requirement_validated_by")?;
                    let at: Option<i64> = r.try_get("requirement_validated_at")?;
                    Ok(Some(NodeSemantics {
                        description,
                        requirement,
                        requirement_validated: validated_int != 0,
                        // NULL on a row written before authorship existed: a claim nobody signed.
                        // Surfaced rather than defaulted to a plausible-looking actor (mirrors
                        // SqliteStore).
                        requirement_validated_by: by,
                        requirement_validated_at: at,
                    }))
                }
            }
        })
        .map_err(st)
    }

    fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>> {
        let mut h = self.conn()?;
        rt_block(async {
            let rows = sqlx::query("SELECT data FROM nodes WHERE requirement = $1 ORDER BY symbol")
                .bind(requirement)
                .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            // Order by ts then id so identical-ts rows have a stable, insertion order.
            let rows = sqlx::query(
                "SELECT key, value, confidence, provenance, author, ts, \"type\", source_type, extraction_method, last_verified \
                 FROM annotations WHERE node_sym = $1 ORDER BY ts ASC, id ASC",
            )
            .bind(&symbol.0)
            .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            // idx_annotations_type backs the WHERE; ordered by symbol then ts for determinism.
            let rows = sqlx::query(
                "SELECT node_sym, key, value, confidence, provenance, author, ts, \"type\", source_type, extraction_method, last_verified \
                 FROM annotations \
                 WHERE \"type\" = $1 \
                 ORDER BY node_sym ASC, ts ASC, id ASC",
            )
            .bind(ty)
            .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            // Freshness read: every annotation last verified STRICTLY BEFORE `cutoff`. Never-verified
            // rows (last_verified = 0) fall out for any positive cutoff. idx_annotations_last_verified
            // backs the range scan; ordered by symbol then ts, parallel to annotations_by_type.
            let rows = sqlx::query(
                "SELECT node_sym, key, value, confidence, provenance, author, ts, \"type\", source_type, extraction_method, last_verified \
                 FROM annotations \
                 WHERE last_verified < $1 \
                 ORDER BY node_sym ASC, ts ASC, id ASC",
            )
            .bind(cutoff)
            .fetch_all(h.as_conn())
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
        let mut h = self.conn()?;
        rt_block(async {
            let node_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM nodes")
                .fetch_one(h.as_conn())
                .await?
                .try_get("c")?;
            let edge_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM edges")
                .fetch_one(h.as_conn())
                .await?
                .try_get("c")?;

            let file_kind = serde_json::to_string(&NodeKind::File)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            let file_count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM nodes WHERE kind = $1")
                .bind(&file_kind)
                .fetch_one(h.as_conn())
                .await?
                .try_get("c")?;

            let unresolved_ref_count: i64 =
                sqlx::query("SELECT COUNT(*) AS c FROM unresolved_refs")
                    .fetch_one(h.as_conn())
                    .await?
                    .try_get("c")?;

            let mut nodes_by_kind = BTreeMap::new();
            {
                let rows = sqlx::query("SELECT kind, COUNT(*) AS c FROM nodes GROUP BY kind")
                    .fetch_all(h.as_conn())
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
                    .fetch_all(h.as_conn())
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

    fn all_nodes(&self) -> wicked_estate_core::Result<Vec<Node>> {
        GraphRead::all_nodes(self)
    }
}

#[cfg(test)]
mod ddl_tests {
    use super::{SCHEMA, split_ddl};

    #[test]
    fn split_ddl_ignores_semicolons_in_comments() {
        let sql = "-- a comment with ; and 'quotes' inside\nCREATE TABLE t (x int);\nALTER TABLE t ADD y text DEFAULT '';";
        let s = split_ddl(sql);
        assert_eq!(s.len(), 2, "exactly two real statements");
        assert!(s[0].starts_with("CREATE TABLE"));
        assert!(
            s.iter().all(|x| !x.contains("--")),
            "no comment fragments leak into statements"
        );
        assert!(
            s.iter().all(|x| x.matches('\'').count() % 2 == 0),
            "each statement has balanced quotes"
        );
    }

    #[test]
    fn real_schema_splits_into_balanced_statements() {
        // Regression: the scope-ordering comment contains ';' — must not split a statement mid-comment.
        let stmts = split_ddl(SCHEMA);
        assert!(!stmts.is_empty());
        for stmt in &stmts {
            assert!(
                !stmt.trim_start().starts_with("--"),
                "comment leaked: {stmt}"
            );
            assert_eq!(
                stmt.matches('\'').count() % 2,
                0,
                "unbalanced quotes (would error in PG): {stmt}"
            );
        }
    }
}
