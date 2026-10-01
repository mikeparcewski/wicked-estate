//! `SurrealStore` — W1.5 bake-off challenger.
//!
//! Embedded SurrealDB (kv-mem, in-memory) behind the same `GraphStore` trait as `SqliteStore`.
//! Compiled ONLY when `--features surrealdb` is passed; the default build path never touches this
//! module.
//!
//! Model: SurrealDB is used as a document store. Every row carries its record as a JSON `data`
//! blob plus the scalar columns queries filter on (`symbol`, `file`, `src`/`tgt`, …). Traversal is
//! a client-side bounded BFS over `edge_rel`, mirroring [`MemStore`](crate::MemStore) — the
//! reference implementation this module ports step-for-step, so the two can be read side by side.
//!
//! Reads use typed column projection — `take::<Vec<String>>((stmt, "data"))` — rather than walking
//! `surrealdb::types::Value`. The `Value` walk is what broke when surrealdb 3.2 moved `Value` and
//! renamed its variants; a named-column projection is stable across that churn.
//!
//! Status: W1.5 bake-off. This is a challenger implementation, not the default.

#![cfg(feature = "surrealdb")]

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, HashSet};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use wicked_estate_core::{
    Annotation, Change, ChangeOp, Direction, Edge, Error, GraphRead, GraphStats, GraphWrite,
    HistoricalEdge, Node, NodeKind, NodeSemantics, RepoInfo, Result, StoreCapabilities, Subgraph,
    SymbolId, SymbolQuery, TraversalSpec, UnresolvedRef, ValidationClaim,
};

/// Map any displayable error into our storage error.
fn se<E: std::fmt::Display>(e: E) -> Error {
    Error::Storage(e.to_string())
}

/// The endpoint a traversal advances to across `e`, relative to `dir` and the node it is standing
/// on. Shared by the expansion step and the depth-horizon probe so the two can never disagree
/// about what "the next node" is (wicked-estate#190).
fn advance_to(dir: Direction, e: &Edge, cur: &SymbolId) -> SymbolId {
    match dir {
        Direction::Dependents => e.source.clone(),
        Direction::Dependencies => e.target.clone(),
        Direction::Both => {
            if &e.source == cur {
                e.target.clone()
            } else {
                e.source.clone()
            }
        }
    }
}

/// Decode statement `idx`'s `data` column — JSON blobs this store wrote — into `T`.
///
/// A blob that fails to decode is an ERROR, not a skipped row: a store that silently drops rows it
/// cannot read under-reports, which is the failure mode the honesty contract exists to prevent.
fn data_col<T: DeserializeOwned>(
    res: &mut surrealdb::IndexedResults,
    idx: usize,
) -> Result<Vec<T>> {
    let blobs: Vec<String> = res.take((idx, "data")).map_err(se)?;
    blobs
        .iter()
        .map(|s| serde_json::from_str(s).map_err(se))
        .collect()
}

/// The value of a `SELECT count() … GROUP ALL` statement (`0` when no rows matched — `GROUP ALL`
/// over an empty set yields no row at all). Projected by field name: in surrealdb 3 a
/// `SELECT count()` under `GROUP ALL` still yields an object, not a bare int.
fn count_of(res: &mut surrealdb::IndexedResults, idx: usize) -> Result<u64> {
    let n: Vec<i64> = res.take((idx, "count")).map_err(se)?;
    Ok(n.first().copied().unwrap_or(0).max(0) as u64)
}

/// Unix-seconds wall clock — the stamp the SQLite store's `strftime('%s','now')` column default
/// applies to an unset annotation `ts` / validation time.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The PREFERRED contribution among a symbol's per-file records (M4 / Option A —
/// wicked-estate#152): definitions (`!is_declaration()`) beat declarations; ties break on the
/// lexicographically smallest file. Identical key to MemStore's `preferred_contribution` and the
/// SQLite `ORDER BY is_def DESC, file ASC LIMIT 1`.
fn preferred_contribution(contribs: &[Node]) -> Option<&Node> {
    contribs.iter().min_by(|a, b| {
        (a.is_declaration(), a.location.file.as_str())
            .cmp(&(b.is_declaration(), b.location.file.as_str()))
    })
}

/// An annotation row. The owning symbol is stored INSIDE the blob so one `data` projection yields
/// the `(symbol, annotation)` pair the by-type / staleness reads return.
#[derive(Serialize, Deserialize)]
struct AnnotRow {
    symbol: String,
    annotation: Annotation,
}

/// Embedded SurrealDB graph store (W1.5 bake-off challenger).
///
/// Wraps the async SurrealDB client with a synchronous facade using a Tokio runtime, matching the
/// sync `GraphStore` trait surface.
pub struct SurrealStore {
    db: Surreal<Db>,
    rt: tokio::runtime::Runtime,
    /// Edge-history archival on `remove_file`. Opt-in, default `false` — MemStore / SqliteStore
    /// parity (it costs a write per superseded edge).
    history_enabled: bool,
}

impl SurrealStore {
    /// Open an in-memory SurrealDB instance (tests + bake-off).
    pub fn in_memory() -> Result<Self> {
        let rt = tokio::runtime::Runtime::new().map_err(se)?;
        let db = rt.block_on(async {
            let db = Surreal::new::<Mem>(()).await.map_err(se)?;
            db.use_ns("ci").use_db("graph").await.map_err(se)?;
            db.query(
                "DEFINE TABLE node SCHEMAFULL;
                 DEFINE FIELD symbol ON node TYPE string;
                 DEFINE FIELD name   ON node TYPE string;
                 DEFINE FIELD file   ON node TYPE string;
                 DEFINE FIELD data   ON node TYPE string;
                 DEFINE INDEX node_symbol ON node COLUMNS symbol UNIQUE;
                 DEFINE INDEX node_name   ON node COLUMNS name;
                 DEFINE INDEX node_file   ON node COLUMNS file;

                 -- M4 / Option A (wicked-estate#152): one row per (symbol, contributing file). The
                 -- `node` row is a derived projection of the PREFERRED contribution, never
                 -- last-write-wins; remove_file retires contributions and re-homes survivors.
                 DEFINE TABLE node_contrib SCHEMAFULL;
                 DEFINE FIELD symbol ON node_contrib TYPE string;
                 DEFINE FIELD file   ON node_contrib TYPE string;
                 DEFINE FIELD data   ON node_contrib TYPE string;
                 DEFINE INDEX contrib_key  ON node_contrib COLUMNS symbol, file UNIQUE;
                 DEFINE INDEX contrib_file ON node_contrib COLUMNS file;

                 -- `file` = the edge's location file ('' when it has none) — what remove_file
                 -- scopes the delete and the history archive to.
                 DEFINE TABLE edge_rel SCHEMAFULL;
                 DEFINE FIELD src  ON edge_rel TYPE string;
                 DEFINE FIELD tgt  ON edge_rel TYPE string;
                 DEFINE FIELD kind ON edge_rel TYPE string;
                 DEFINE FIELD file ON edge_rel TYPE string;
                 DEFINE FIELD data ON edge_rel TYPE string;
                 DEFINE INDEX edge_src   ON edge_rel COLUMNS src;
                 DEFINE INDEX edge_tgt   ON edge_rel COLUMNS tgt;
                 DEFINE INDEX edge_file  ON edge_rel COLUMNS file;
                 DEFINE INDEX edge_dedup ON edge_rel COLUMNS src, tgt, kind UNIQUE;

                 DEFINE TABLE unresolved SCHEMAFULL;
                 DEFINE FIELD raw_name ON unresolved TYPE string;
                 DEFINE FIELD file     ON unresolved TYPE string;
                 DEFINE FIELD data     ON unresolved TYPE string;
                 DEFINE INDEX unresolved_name ON unresolved COLUMNS raw_name;
                 DEFINE INDEX unresolved_file ON unresolved COLUMNS file;

                 -- M8/DoD-XA4: per-symbol live-node epoch, mirroring SQLite's symbols.gen/had_node.
                 -- Keyed by the symbol string; SURVIVES remove_file (only `node` rows are deleted),
                 -- so a delete-then-re-add reuse can be detected and the epoch bumped.
                 DEFINE TABLE symgen SCHEMAFULL;
                 DEFINE FIELD symbol   ON symgen TYPE string;
                 DEFINE FIELD gen      ON symgen TYPE int;
                 DEFINE FIELD had_node ON symgen TYPE int;
                 DEFINE INDEX symgen_symbol ON symgen COLUMNS symbol UNIQUE;

                 DEFINE TABLE file_meta SCHEMAFULL;
                 DEFINE FIELD path   ON file_meta TYPE string;
                 DEFINE FIELD digest ON file_meta TYPE string;
                 DEFINE INDEX file_path ON file_meta COLUMNS path UNIQUE;

                 DEFINE TABLE file_content SCHEMAFULL;
                 DEFINE FIELD path ON file_content TYPE string;
                 DEFINE FIELD text ON file_content TYPE string;
                 DEFINE INDEX fc_path ON file_content COLUMNS path UNIQUE;

                 -- W7: repo-wide git provenance (a single row, overwritten).
                 DEFINE TABLE repo_meta SCHEMAFULL;
                 DEFINE FIELD data ON repo_meta TYPE string;

                 -- W7.1: change log. `seq` is the monotonic subscription cursor.
                 DEFINE TABLE changelog SCHEMAFULL;
                 DEFINE FIELD seq  ON changelog TYPE int;
                 DEFINE FIELD data ON changelog TYPE string;
                 DEFINE INDEX changelog_seq ON changelog COLUMNS seq UNIQUE;

                 -- W7: read-only history of edges superseded by remove_file. NEVER traversed.
                 DEFINE TABLE edge_hist SCHEMAFULL;
                 DEFINE FIELD file         ON edge_hist TYPE string;
                 DEFINE FIELD archived_seq ON edge_hist TYPE int;
                 DEFINE FIELD data         ON edge_hist TYPE string;
                 DEFINE INDEX edge_hist_file ON edge_hist COLUMNS file;
                 DEFINE INDEX edge_hist_seq  ON edge_hist COLUMNS archived_seq UNIQUE;

                 -- Semantic linking: description / requirement / validation claim per symbol.
                 DEFINE TABLE semantics SCHEMAFULL;
                 DEFINE FIELD symbol      ON semantics TYPE string;
                 DEFINE FIELD requirement ON semantics TYPE option<string>;
                 DEFINE FIELD data        ON semantics TYPE string;
                 DEFINE INDEX semantics_symbol ON semantics COLUMNS symbol UNIQUE;
                 DEFINE INDEX semantics_req    ON semantics COLUMNS requirement;

                 -- Typed annotations. A bare INSERT per annotate() (many per symbol). `ord` is a
                 -- monotonic tiebreak so rows sharing a seconds-resolution `ts` keep write order.
                 DEFINE TABLE annot SCHEMAFULL;
                 DEFINE FIELD symbol ON annot TYPE string;
                 DEFINE FIELD atype  ON annot TYPE string;
                 DEFINE FIELD akey   ON annot TYPE string;
                 DEFINE FIELD ts     ON annot TYPE int;
                 DEFINE FIELD ord    ON annot TYPE int;
                 DEFINE FIELD data   ON annot TYPE string;
                 DEFINE INDEX annot_symbol ON annot COLUMNS symbol;
                 DEFINE INDEX annot_type   ON annot COLUMNS atype;
                 DEFINE INDEX annot_ord    ON annot COLUMNS ord UNIQUE;",
            )
            .await
            .map_err(se)?
            .check()
            .map_err(se)?;
            Ok::<_, Error>(db)
        })?;
        Ok(Self {
            db,
            rt,
            history_enabled: false,
        })
    }

    /// Enable or disable edge-history archival on `remove_file` (default: off).
    pub fn set_history_enabled(&mut self, on: bool) {
        self.history_enabled = on;
    }
}

/// The next value of a monotonic `int` column (`max + 1`, starting at 1). Used for the change-log
/// cursor, the history archive order, and the annotation write order. Single writer (`&mut self`),
/// so read-then-insert cannot race.
async fn next_seq(db: &Surreal<Db>, table: &str, col: &str) -> Result<i64> {
    let mut res = db
        .query(format!(
            "SELECT VALUE {col} FROM {table} ORDER BY {col} DESC LIMIT 1"
        ))
        .await
        .map_err(se)?;
    let top: Option<i64> = res.take(0).map_err(se)?;
    Ok(top.unwrap_or(0) + 1)
}

/// Upsert the `node` row for `primary.symbol` as the projection of `primary`.
async fn write_node_row(db: &Surreal<Db>, primary: &Node) -> Result<()> {
    let data = serde_json::to_string(primary).map_err(se)?;
    db.query(
        "LET $ex = (SELECT id FROM node WHERE symbol=$sym LIMIT 1);
                 IF array::len($ex) = 0 { INSERT INTO node { symbol: $sym, name: $name, file: $file, data: $data } } ELSE { UPDATE node SET name=$name, file=$file, data=$data WHERE symbol=$sym };",
    )
    .bind(("sym", primary.symbol.0.clone()))
    .bind(("name", primary.name.clone()))
    .bind(("file", primary.location.file.clone()))
    .bind(("data", data))
    .await
    .map_err(se)?
    .check()
    .map_err(se)?;
    Ok(())
}

async fn get_node_async(db: &Surreal<Db>, sym: &str) -> Result<Option<Node>> {
    let mut res = db
        .query("SELECT data FROM node WHERE symbol=$sym LIMIT 1")
        .bind(("sym", sym.to_string()))
        .await
        .map_err(se)?;
    Ok(data_col::<Node>(&mut res, 0)?.into_iter().next())
}

async fn file_text_async(db: &Surreal<Db>, file: &str) -> Result<Option<String>> {
    let mut res = db
        .query("SELECT text FROM file_content WHERE path=$path LIMIT 1")
        .bind(("path", file.to_string()))
        .await
        .map_err(se)?;
    let texts: Vec<String> = res.take((0, "text")).map_err(se)?;
    Ok(texts.into_iter().next())
}

// ── GraphWrite ────────────────────────────────────────────────────────────────

impl GraphWrite for SurrealStore {
    fn begin_batch(&mut self) -> Result<()> {
        // SurrealDB auto-commits each statement; no explicit transaction API exposed at this level.
        // For the bake-off we accept this (`transactional_batch: false` says so honestly).
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
                // Epoch pre-pass (M8/DoD-XA4), BEFORE the node insert — same rule as the SQLite seam:
                // bump iff this symbol HAD a node (symgen.had_node==1) and has none now (reuse). The
                // `node` existence check runs against the pre-insert state.
                let mut probe = db
                    .query(
                        "SELECT VALUE had_node FROM symgen WHERE symbol=$sym LIMIT 1;
                         SELECT count() FROM node WHERE symbol=$sym GROUP ALL;",
                    )
                    .bind(("sym", n.symbol.0.clone()))
                    .await
                    .map_err(se)?;
                let had_node: Option<i64> = probe.take(0).map_err(se)?;
                let has_live = count_of(&mut probe, 1)? > 0;
                let bump: i64 = if had_node.unwrap_or(0) == 1 && !has_live {
                    1
                } else {
                    0
                };
                db.query(
                    "LET $ex = (SELECT id FROM symgen WHERE symbol=$sym LIMIT 1);
                 IF array::len($ex) = 0 { INSERT INTO symgen { symbol: $sym, gen: 0, had_node: 1 } } ELSE { UPDATE symgen SET gen = gen + $bump, had_node = 1
                     WHERE symbol=$sym };",
                )
                .bind(("sym", n.symbol.0.clone()))
                .bind(("bump", bump))
                .await
                .map_err(se)?
                .check()
                .map_err(se)?;

                // Multi-file contributions (M4 / Option A — wicked-estate#152): record the write as
                // THIS file's contribution, then derive the live node from the PREFERRED
                // contribution — never last-write-wins. A single-contribution symbol (the common
                // case) projects the record just written.
                let data = serde_json::to_string(n).map_err(se)?;
                db.query(
                    "LET $ex = (SELECT id FROM node_contrib WHERE symbol=$sym AND file=$file LIMIT 1);
                 IF array::len($ex) = 0 { INSERT INTO node_contrib { symbol: $sym, file: $file, data: $data } } ELSE { UPDATE node_contrib SET data=$data WHERE symbol=$sym AND file=$file };",
                )
                .bind(("sym", n.symbol.0.clone()))
                .bind(("file", n.location.file.clone()))
                .bind(("data", data))
                .await
                .map_err(se)?
                .check()
                .map_err(se)?;
                let mut res = db
                    .query("SELECT data FROM node_contrib WHERE symbol=$sym")
                    .bind(("sym", n.symbol.0.clone()))
                    .await
                    .map_err(se)?;
                let contribs: Vec<Node> = data_col(&mut res, 0)?;
                let primary = preferred_contribution(&contribs).unwrap_or(n);
                write_node_row(&db, primary).await?;
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
                let mut res = db
                    .query(
                        "SELECT data FROM edge_rel WHERE src=$src AND tgt=$tgt AND kind=$kind LIMIT 1",
                    )
                    .bind(("src", e.source.0.clone()))
                    .bind(("tgt", e.target.0.clone()))
                    .bind(("kind", kind.clone()))
                    .await
                    .map_err(se)?;
                let existing: Option<Edge> = data_col::<Edge>(&mut res, 0)?.into_iter().next();
                // On a collision the higher-confidence edge wins — UNLESS the incoming edge carries
                // more evidence. `evidence_count` is a monotonic audit counter, so growth is
                // strictly newer information. Same rule as MemStore / SqliteStore / PostgresStore.
                let write = match &existing {
                    None => true,
                    Some(x) => {
                        e.confidence.get() >= x.confidence.get()
                            || e.evidence_count > x.evidence_count
                    }
                };
                if !write {
                    continue;
                }
                let file = e
                    .location
                    .as_ref()
                    .map(|l| l.file.clone())
                    .unwrap_or_default();
                db.query(
                    "LET $ex = (SELECT id FROM edge_rel WHERE src=$src AND tgt=$tgt AND kind=$kind LIMIT 1);
                 IF array::len($ex) = 0 { INSERT INTO edge_rel { src: $src, tgt: $tgt, kind: $kind, file: $file, data: $data } } ELSE { UPDATE edge_rel SET file=$file, data=$data WHERE src=$src AND tgt=$tgt AND kind=$kind };",
                )
                .bind(("src", e.source.0.clone()))
                .bind(("tgt", e.target.0.clone()))
                .bind(("kind", kind))
                .bind(("file", file))
                .bind(("data", data))
                .await
                .map_err(se)?
                .check()
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
                db.query("INSERT INTO unresolved { raw_name: $name, file: $file, data: $data }")
                    .bind(("name", r.raw_name.clone()))
                    .bind(("file", r.location.file.clone()))
                    .bind(("data", data))
                    .await
                    .map_err(se)?
                    .check()
                    .map_err(se)?;
            }
            Ok::<_, Error>(())
        })
    }

    /// Ports `MemStore::remove_file` step-for-step (the numbered steps match its comments).
    fn remove_file(&mut self, file: &str) -> Result<()> {
        let db = self.db.clone();
        let file = file.to_string();
        let history_enabled = self.history_enabled;
        self.rt.block_on(async move {
            // Step 1: the git blob SHA of the version being superseded (tags archived edges).
            let current_git_sha = file_text_async(&db, &file)
                .await?
                .map(|t| crate::sqlite::git_blob_sha(&t))
                .unwrap_or_default();

            // Step 1a: multi-file contribution retirement + survivor re-home (M4 / Option A —
            // wicked-estate#152). Delete this file's CONTRIBUTION from every symbol it contributed
            // to; a node currently homed here with contributions from OTHER files is re-homed
            // WHOLESALE to the preferred survivor instead of being deleted. Runs before
            // `file_symbols` is computed, so everything below sees kept nodes at their new home.
            let mut res = db
                .query("SELECT VALUE symbol FROM node_contrib WHERE file=$file")
                .bind(("file", file.clone()))
                .await
                .map_err(se)?;
            let contributed: Vec<String> = res.take(0).map_err(se)?;
            db.query("DELETE node_contrib WHERE file=$file")
                .bind(("file", file.clone()))
                .await
                .map_err(se)?
                .check()
                .map_err(se)?;
            for sym in &contributed {
                let mut res = db
                    .query("SELECT data FROM node_contrib WHERE symbol=$sym")
                    .bind(("sym", sym.clone()))
                    .await
                    .map_err(se)?;
                let survivors: Vec<Node> = data_col(&mut res, 0)?;
                let Some(primary) = preferred_contribution(&survivors) else {
                    continue;
                };
                if get_node_async(&db, sym)
                    .await?
                    .is_some_and(|n| n.location.file == file)
                {
                    write_node_row(&db, primary).await?;
                }
            }

            // Step 2: symbols still homed in this file (contribution-kept nodes already moved).
            let mut res = db
                .query("SELECT data FROM node WHERE file=$file")
                .bind(("file", file.clone()))
                .await
                .map_err(se)?;
            let file_nodes: Vec<Node> = data_col(&mut res, 0)?;
            let file_symbols: Vec<String> = file_nodes.iter().map(|n| n.symbol.0.clone()).collect();
            let file_sym_set: HashSet<&str> = file_symbols.iter().map(String::as_str).collect();

            // The edges this file owns: located in it, or sourced from a symbol defined in it —
            // covering edges created without an explicit location. Steps 3 and 4 share the set.
            let mut res = db
                .query("SELECT data FROM edge_rel WHERE file=$file OR src INSIDE $syms")
                .bind(("file", file.clone()))
                .bind(("syms", file_symbols.clone()))
                .await
                .map_err(se)?;
            let owned_edges: Vec<Edge> = data_col(&mut res, 0)?;

            // Step 3: if history is enabled, archive the superseded edges (read-only history,
            // never traversed).
            for edge in owned_edges.into_iter().filter(|_| history_enabled) {
                let archived_seq = next_seq(&db, "edge_hist", "archived_seq").await?;
                let hist = HistoricalEdge {
                    git_sha: current_git_sha.clone(),
                    archived_seq: archived_seq as u64,
                    edge,
                };
                db.query("INSERT INTO edge_hist { file: $file, archived_seq: $seq, data: $data }")
                    .bind(("file", file.clone()))
                    .bind(("seq", archived_seq))
                    .bind(("data", serde_json::to_string(&hist).map_err(se)?))
                    .await
                    .map_err(se)?
                    .check()
                    .map_err(se)?;
            }

            // Step 3b: shared-Import keep + re-home (incr-integrity lane, D1/D2/D4). An Import node
            // homed here is KEPT when a SURVIVOR edge still targets it — one whose location file is
            // neither '' nor this file and whose source does not live in this file (exactly what
            // Step 4 deletes, so pre-delete evaluation equals post-delete state). It is re-homed to
            // the survivor with the MIN location file, so the keep self-terminates.
            for n in file_nodes.iter().filter(|n| n.kind == NodeKind::Import) {
                let mut res = db
                    .query("SELECT data FROM edge_rel WHERE tgt=$sym")
                    .bind(("sym", n.symbol.0.clone()))
                    .await
                    .map_err(se)?;
                let incoming: Vec<Edge> = data_col(&mut res, 0)?;
                let survivor_loc = incoming
                    .iter()
                    .filter(|e| !file_sym_set.contains(e.source.0.as_str()))
                    .filter_map(|e| e.location.as_ref())
                    .filter(|l| !l.file.is_empty() && l.file != file)
                    .min_by(|a, b| a.file.cmp(&b.file))
                    .cloned();
                if let Some(loc) = survivor_loc {
                    let mut rehomed = n.clone();
                    rehomed.location = loc;
                    write_node_row(&db, &rehomed).await?;
                }
            }

            // Step 4: remove nodes (kept ones were re-homed, so no longer match), the owned edges
            // (a kept node's own OUTGOING edges still die — SqliteStore parity), unresolved refs,
            // digest, and content.
            db.query(
                "DELETE node WHERE file=$file;
                 DELETE edge_rel WHERE file=$file OR src INSIDE $syms;
                 DELETE unresolved WHERE file=$file;
                 DELETE file_meta WHERE path=$file;
                 DELETE file_content WHERE path=$file;",
            )
            .bind(("file", file.clone()))
            .bind(("syms", file_symbols))
            .await
            .map_err(se)?
            .check()
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
                 IF array::len($ex) = 0 { INSERT INTO file_meta { path: $path, digest: $digest } } ELSE { UPDATE file_meta SET digest=$digest WHERE path=$path };",
            )
            .bind(("path", file))
            .bind(("digest", digest))
            .await
            .map_err(se)?
            .check()
            .map_err(se)?;
            Ok::<_, Error>(())
        })
    }

    fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()> {
        let db = self.db.clone();
        let data = serde_json::to_string(info).map_err(se)?;
        self.rt.block_on(async move {
            db.query("DELETE repo_meta; INSERT INTO repo_meta { data: $data };")
                .bind(("data", data))
                .await
                .map_err(se)?
                .check()
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
                 IF array::len($ex) = 0 { INSERT INTO file_content { path: $path, text: $text } } ELSE { UPDATE file_content SET text=$text WHERE path=$path };",
            )
            .bind(("path", file))
            .bind(("text", text))
            .await
            .map_err(se)?
            .check()
            .map_err(se)?;
            Ok::<_, Error>(())
        })
    }

    fn prune_dangling_edges(&mut self) -> Result<usize> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            let mut res = db
                .query(
                    "SELECT count() FROM edge_rel GROUP ALL;
                     LET $live = (SELECT VALUE symbol FROM node);
                     DELETE edge_rel WHERE src NOTINSIDE $live OR tgt NOTINSIDE $live;
                     SELECT count() FROM edge_rel GROUP ALL;",
                )
                .await
                .map_err(se)?;
            let before = count_of(&mut res, 0)?;
            let after = count_of(&mut res, 3)?;
            Ok::<_, Error>(before.saturating_sub(after) as usize)
        })
    }

    fn log_change(&mut self, op: ChangeOp, target: &str) -> Result<()> {
        let db = self.db.clone();
        let target = target.to_string();
        self.rt.block_on(async move {
            let seq = next_seq(&db, "changelog", "seq").await?;
            let change = Change {
                seq: seq as u64,
                op,
                target,
            };
            db.query("INSERT INTO changelog { seq: $seq, data: $data }")
                .bind(("seq", seq))
                .bind(("data", serde_json::to_string(&change).map_err(se)?))
                .await
                .map_err(se)?
                .check()
                .map_err(se)?;
            Ok::<_, Error>(())
        })
    }

    fn set_node_semantics(
        &mut self,
        symbol: &SymbolId,
        description: Option<&str>,
        requirement: Option<&str>,
        validation: Option<&ValidationClaim>,
    ) -> Result<()> {
        // No-op if nothing is being changed, or the symbol is not a node.
        if description.is_none() && requirement.is_none() && validation.is_none() {
            return Ok(());
        }
        if self.get_node(symbol)?.is_none() {
            return Ok(());
        }
        // PARTIAL update over the existing record.
        let mut sem = self.node_semantics(symbol)?.unwrap_or_default();
        if let Some(d) = description {
            sem.description = Some(d.to_string());
        }
        if let Some(r) = requirement {
            sem.requirement = Some(r.to_string());
        }
        if let Some(claim) = validation {
            // Flag and author set together — the invariant `ValidationClaim` exists to enforce.
            sem.requirement_validated = claim.validated;
            sem.requirement_validated_by = Some(claim.by.clone());
            sem.requirement_validated_at = Some(now_secs());
        }
        let db = self.db.clone();
        let sym = symbol.0.clone();
        let data = serde_json::to_string(&sem).map_err(se)?;
        let req = sem.requirement.clone();
        self.rt.block_on(async move {
            db.query(
                "LET $ex = (SELECT id FROM semantics WHERE symbol=$sym LIMIT 1);
                 IF array::len($ex) = 0 { INSERT INTO semantics { symbol: $sym, requirement: $req, data: $data } } ELSE { UPDATE semantics SET requirement=$req, data=$data WHERE symbol=$sym };",
            )
            .bind(("sym", sym))
            .bind(("req", req))
            .bind(("data", data))
            .await
            .map_err(se)?
            .check()
            .map_err(se)?;
            Ok::<_, Error>(())
        })
    }

    fn annotate(&mut self, symbol: &SymbolId, mut annotation: Annotation) -> Result<()> {
        // No-op if the symbol is not a node (mirrors the SQLite sid-lookup no-op).
        if self.get_node(symbol)?.is_none() {
            return Ok(());
        }
        if annotation.ts == 0 {
            annotation.ts = now_secs();
        }
        let db = self.db.clone();
        let row = AnnotRow {
            symbol: symbol.0.clone(),
            annotation,
        };
        self.rt.block_on(async move {
            let ord = next_seq(&db, "annot", "ord").await?;
            // Bare INSERT (NOT upsert): many annotations per symbol, incl. duplicate (type, key).
            db.query(
                "INSERT INTO annot { symbol: $sym, atype: $ty, akey: $key, ts: $ts, ord: $ord, data: $data }",
            )
            .bind(("sym", row.symbol.clone()))
            .bind(("ty", row.annotation.r#type.clone()))
            .bind(("key", row.annotation.key.clone()))
            .bind(("ts", row.annotation.ts))
            .bind(("ord", ord))
            .bind(("data", serde_json::to_string(&row).map_err(se)?))
            .await
            .map_err(se)?
            .check()
            .map_err(se)?;
            Ok::<_, Error>(())
        })
    }

    fn delete_annotations(
        &mut self,
        symbol: &SymbolId,
        ty: Option<&str>,
        key: &str,
    ) -> Result<usize> {
        let db = self.db.clone();
        let (sym, key) = (symbol.0.clone(), key.to_string());
        // `type` matched as an opaque string — no per-type branching (rules-as-DATA).
        let filter = if ty.is_some() {
            "symbol=$sym AND akey=$key AND atype=$ty"
        } else {
            "symbol=$sym AND akey=$key"
        };
        let ty = ty.map(str::to_string);
        self.rt.block_on(async move {
            let mut res = db
                .query(format!(
                    "SELECT count() FROM annot WHERE {filter} GROUP ALL;
                     DELETE annot WHERE {filter};"
                ))
                .bind(("sym", sym))
                .bind(("key", key))
                .bind(("ty", ty))
                .await
                .map_err(se)?;
            let n = count_of(&mut res, 0)?;
            res.check().map_err(se)?;
            Ok::<_, Error>(n as usize)
        })
    }
}

// ── GraphRead ────────────────────────────────────────────────────────────────

impl SurrealStore {
    /// Annotation pairs matching `filter` (a SurrealQL predicate over `annot`), ordered by symbol
    /// then `ts` (then write order) — the shared ordering contract of the by-type / stale reads.
    fn annot_pairs(
        &self,
        filter: &str,
        bind: Option<(&'static str, String)>,
    ) -> Result<Vec<(SymbolId, Annotation)>> {
        let db = self.db.clone();
        let sql =
            format!("SELECT data, symbol, ts, ord FROM annot {filter} ORDER BY symbol, ts, ord");
        self.rt.block_on(async move {
            let q = db.query(sql);
            let q = match bind {
                Some(b) => q.bind(b),
                None => q,
            };
            let mut res = q.await.map_err(se)?;
            let rows: Vec<AnnotRow> = data_col(&mut res, 0)?;
            Ok(rows
                .into_iter()
                .map(|r| (SymbolId(r.symbol), r.annotation))
                .collect())
        })
    }
}

impl GraphRead for SurrealStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            full_text_search: false, // kv-mem does not have BM25 in this config
            vector_search: false,    // HNSW available with kv-surrealkv; not wired here
            // Traversal is a client-side BFS issuing one `neighbors` query per expanded node —
            // NOT server-side. Advertising `true` would mislead retrieval's capability
            // negotiation into assuming a round-trip-free walk.
            server_side_traversal: false,
            transactional_batch: false, // begin/commit are no-ops in this impl
            shared_writers: false,
        }
    }

    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
        let db = self.db.clone();
        let sym = id.0.clone();
        self.rt
            .block_on(async move { get_node_async(&db, &sym).await })
    }

    fn symbol_epoch(&self, id: &SymbolId) -> Result<Option<u64>> {
        let db = self.db.clone();
        let id = id.clone();
        self.rt.block_on(async move {
            // Live only: epoch is defined iff a `node` row exists. The gen lives in `symgen`, which
            // survives remove_file; we gate on live-node existence so a removed symbol reads None.
            let mut res = db
                .query(
                    "SELECT count() FROM node WHERE symbol=$sym GROUP ALL;
                     SELECT VALUE gen FROM symgen WHERE symbol=$sym LIMIT 1;",
                )
                .bind(("sym", id.0.clone()))
                .await
                .map_err(se)?;
            if count_of(&mut res, 0)? == 0 {
                return Ok(None);
            }
            let epoch: Option<i64> = res.take(1).map_err(se)?;
            Ok(Some(epoch.unwrap_or(0).max(0) as u64))
        })
    }

    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
        let mut out = self.all_nodes()?;
        out.retain(|n| {
            // Scope is filtered BEFORE the `limit` truncate, so a scoped query never leaks another
            // scope's rows into (or out of) the top-k (multi-tenant isolation; MemStore parity).
            if let Some(prefix) = &query.scope_prefix {
                if !n.scope.path_in_prefix(prefix) {
                    return false;
                }
            }
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
    }

    fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>> {
        let db = self.db.clone();
        let sym = id.0.clone();
        let sql = match dir {
            Direction::Dependents => "SELECT data FROM edge_rel WHERE tgt=$id",
            Direction::Dependencies => "SELECT data FROM edge_rel WHERE src=$id",
            Direction::Both => "SELECT data FROM edge_rel WHERE src=$id OR tgt=$id",
        };
        self.rt.block_on(async move {
            let mut res = db.query(sql).bind(("id", sym)).await.map_err(se)?;
            data_col(&mut res, 0)
        })
    }

    fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph> {
        // Client-side BFS (bounded) — mirrors MemStore's approach.
        let mut depths: BTreeMap<String, u32> = BTreeMap::new();
        let mut seen: HashSet<SymbolId> = HashSet::new();
        let mut queue: std::collections::VecDeque<(SymbolId, u32)> =
            std::collections::VecDeque::new();

        seen.insert(start.clone());
        queue.push_back((start.clone(), 0));

        let mut sub_nodes: Vec<Node> = Vec::new();
        let mut sub_edges: Vec<Edge> = Vec::new();
        let mut node_cap_reached = false;
        let mut depth_horizon_reached = false;
        // Nodes the node cap declined. They were REACHED within the horizon, just not admitted,
        // so the horizon probe must count them as seen — else a cap cut reads as a depth cut.
        let mut capped: HashSet<SymbolId> = HashSet::new();

        if let Some(n) = self.get_node(start)? {
            sub_nodes.push(n);
        }

        while let Some((cur, depth)) = queue.pop_front() {
            if depth >= spec.max_depth {
                // DEPTH HORIZON (wicked-estate#190) — same rule as MemStore's BFS: a node left
                // unexpanded that still has an unreached qualifying neighbour means the horizon
                // cut real results. BFS level order makes `seen` ∪ `capped` complete for
                // depths <= max_depth, so the test is exact. Short-circuited on the flag, so the extra
                // `neighbors` query runs at most once per horizon node and never after the answer
                // is known. No node or edge from beyond the horizon is admitted to the result.
                if !depth_horizon_reached {
                    for e in self.neighbors(&cur, spec.direction)? {
                        if e.confidence.get() < spec.min_confidence {
                            continue;
                        }
                        if !spec.edge_kinds.is_empty() && !spec.edge_kinds.contains(&e.kind) {
                            continue;
                        }
                        let next = advance_to(spec.direction, &e, &cur);
                        if !seen.contains(&next) && !capped.contains(&next) {
                            depth_horizon_reached = true;
                            break;
                        }
                    }
                }
                continue;
            }
            for e in self.neighbors(&cur, spec.direction)? {
                if e.confidence.get() < spec.min_confidence {
                    continue;
                }
                if !spec.edge_kinds.is_empty() && !spec.edge_kinds.contains(&e.kind) {
                    continue;
                }
                let next = advance_to(spec.direction, &e, &cur);
                sub_edges.push(e);
                if seen.contains(&next) {
                    continue;
                }
                if sub_nodes.len() >= spec.max_nodes {
                    node_cap_reached = true;
                    capped.insert(next);
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
            ..Default::default()
        }
        .with_caps(node_cap_reached, depth_horizon_reached))
    }

    fn all_nodes(&self) -> Result<Vec<Node>> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            let mut res = db.query("SELECT data FROM node").await.map_err(se)?;
            data_col(&mut res, 0)
        })
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            let mut res = db.query("SELECT data FROM edge_rel").await.map_err(se)?;
            data_col(&mut res, 0)
        })
    }

    fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>> {
        let db = self.db.clone();
        let name = name.to_string();
        self.rt.block_on(async move {
            let mut res = db
                .query("SELECT data FROM unresolved WHERE raw_name=$name")
                .bind(("name", name))
                .await
                .map_err(se)?;
            data_col(&mut res, 0)
        })
    }

    fn indexed_files(&self) -> Result<Vec<String>> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            // Both file-writing calls. This store splits them across two tables —
            // `set_file_digest` writes `file_meta`, `set_file_content` writes `file_content` and
            // never creates a `file_meta` row — so reading only `file_meta` would hide every
            // content-recorded path from the delete-sweep.
            let mut res = db
                .query("SELECT path FROM file_meta; SELECT path FROM file_content")
                .await
                .map_err(se)?;
            let mut out: HashSet<String> = res
                .take::<Vec<String>>((0, "path"))
                .map_err(se)?
                .into_iter()
                .collect();
            out.extend(res.take::<Vec<String>>((1, "path")).map_err(se)?);
            Ok(out.into_iter().collect())
        })
    }

    fn file_digest(&self, file: &str) -> Result<Option<String>> {
        let db = self.db.clone();
        let file = file.to_string();
        self.rt.block_on(async move {
            let mut res = db
                .query("SELECT digest FROM file_meta WHERE path=$path LIMIT 1")
                .bind(("path", file))
                .await
                .map_err(se)?;
            let digests: Vec<String> = res.take((0, "digest")).map_err(se)?;
            Ok(digests.into_iter().next())
        })
    }

    fn file_git_sha(&self, file: &str) -> Result<Option<String>> {
        // The git blob SHA of the stored content — the same `git hash-object` value every store
        // records at `set_file_content` time; derived on read here because content and its SHA
        // live and die together in `file_content`.
        Ok(self
            .file_content(file)?
            .map(|t| crate::sqlite::git_blob_sha(&t)))
    }

    fn repo_info(&self) -> Result<Option<RepoInfo>> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            let mut res = db
                .query("SELECT data FROM repo_meta LIMIT 1")
                .await
                .map_err(se)?;
            Ok(data_col::<RepoInfo>(&mut res, 0)?.into_iter().next())
        })
    }

    fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>> {
        let db = self.db.clone();
        let file = file.to_string();
        self.rt.block_on(async move {
            // Newest first.
            let mut res = db
                .query(
                    "SELECT data, archived_seq FROM edge_hist WHERE file=$file ORDER BY archived_seq DESC",
                )
                .bind(("file", file))
                .await
                .map_err(se)?;
            data_col(&mut res, 0)
        })
    }

    fn file_content(&self, file: &str) -> Result<Option<String>> {
        let db = self.db.clone();
        let file = file.to_string();
        self.rt
            .block_on(async move { file_text_async(&db, &file).await })
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

    fn changes_since(&self, cursor: u64) -> Result<Vec<Change>> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            // Oldest first; capped per call like the other stores.
            let mut res = db
                .query("SELECT data, seq FROM changelog WHERE seq > $c ORDER BY seq LIMIT 10000")
                .bind(("c", cursor as i64))
                .await
                .map_err(se)?;
            data_col(&mut res, 0)
        })
    }

    fn node_semantics(&self, symbol: &SymbolId) -> Result<Option<NodeSemantics>> {
        let db = self.db.clone();
        let sym = symbol.0.clone();
        self.rt.block_on(async move {
            let mut res = db
                .query("SELECT data FROM semantics WHERE symbol=$sym LIMIT 1")
                .bind(("sym", sym))
                .await
                .map_err(se)?;
            Ok(data_col::<NodeSemantics>(&mut res, 0)?.into_iter().next())
        })
    }

    fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>> {
        let db = self.db.clone();
        let req = requirement.to_string();
        let syms: Vec<String> = self.rt.block_on(async move {
            let mut res = db
                .query("SELECT VALUE symbol FROM semantics WHERE requirement=$req")
                .bind(("req", req))
                .await
                .map_err(se)?;
            res.take::<Vec<String>>(0).map_err(se)
        })?;
        let mut out = Vec::new();
        for s in syms {
            if let Some(n) = self.get_node(&SymbolId(s))? {
                out.push(n);
            }
        }
        out.sort_by(|a, b| a.symbol.0.cmp(&b.symbol.0)); // deterministic
        Ok(out)
    }

    fn annotations(&self, symbol: &SymbolId) -> Result<Vec<Annotation>> {
        Ok(self
            .annot_pairs("WHERE symbol=$sym", Some(("sym", symbol.0.clone())))?
            .into_iter()
            .map(|(_, a)| a)
            .collect())
    }

    fn annotations_by_type(&self, ty: &str) -> Result<Vec<(SymbolId, Annotation)>> {
        // `type` matched as an opaque string (known convention OR custom — identical treatment).
        self.annot_pairs("WHERE atype=$ty", Some(("ty", ty.to_string())))
    }

    fn annotations_stale_since(&self, cutoff: i64) -> Result<Vec<(SymbolId, Annotation)>> {
        // Freshness read: the struct's own `is_stale_since` rule, same ordering as by-type.
        Ok(self
            .annot_pairs("", None)?
            .into_iter()
            .filter(|(_, a)| a.is_stale_since(cutoff))
            .collect())
    }

    fn stats(&self) -> Result<GraphStats> {
        let nodes = self.all_nodes()?;
        let edges = self.all_edges()?;

        let node_count = nodes.len() as u64;
        let edge_count = edges.len() as u64;
        let file_count = nodes.iter().filter(|n| n.kind == NodeKind::File).count() as u64;

        let db = self.db.clone();
        let unresolved_ref_count: u64 = self.rt.block_on(async move {
            let mut res = db
                .query("SELECT count() FROM unresolved GROUP ALL")
                .await
                .map_err(se)?;
            count_of(&mut res, 0)
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
            db_size_bytes: 0,
        })
    }
}
