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
use std::collections::{BTreeMap, BTreeSet, HashSet};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use wicked_estate_core::support::{
    EdgeKey, EdgeSupport, StoredFacts, SupportFact, SupportOwner, SupportOwnerState,
    SupportReplacement, base_upsert_wins, fact_key, normalize_facts, plan_replacement,
    project_edge,
};
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

                 -- TS-S2A support plane (wicked_estate_core::support): owner generations, the
                 -- owners' authoritative facts, and the base contribution of supported keys.
                 DEFINE TABLE support_owner SCHEMAFULL;
                 DEFINE FIELD producer   ON support_owner TYPE string;
                 DEFINE FIELD snapshot   ON support_owner TYPE string;
                 DEFINE FIELD generation ON support_owner TYPE int;
                 DEFINE INDEX support_owner_key ON support_owner COLUMNS producer, snapshot UNIQUE;
                 DEFINE TABLE edge_support SCHEMAFULL;
                 DEFINE FIELD producer ON edge_support TYPE string;
                 DEFINE FIELD snapshot ON edge_support TYPE string;
                 DEFINE FIELD src      ON edge_support TYPE string;
                 DEFINE FIELD tgt      ON edge_support TYPE string;
                 DEFINE FIELD kind     ON edge_support TYPE string;
                 DEFINE FIELD fact_id  ON edge_support TYPE string;
                 DEFINE FIELD data     ON edge_support TYPE string;
                 DEFINE INDEX edge_support_key   ON edge_support COLUMNS src, tgt, kind;
                 DEFINE INDEX edge_support_owner ON edge_support COLUMNS producer, snapshot;
                 DEFINE TABLE edge_base SCHEMAFULL;
                 DEFINE FIELD src  ON edge_base TYPE string;
                 DEFINE FIELD tgt  ON edge_base TYPE string;
                 DEFINE FIELD kind ON edge_base TYPE string;
                 DEFINE FIELD file ON edge_base TYPE string;
                 DEFINE FIELD data ON edge_base TYPE string;
                 DEFINE INDEX edge_base_key  ON edge_base COLUMNS src, tgt, kind UNIQUE;
                 DEFINE INDEX edge_base_file ON edge_base COLUMNS file;

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

// ── TS-S2A support plane (contract: `wicked_estate_core::support`) ─────────────
//
// SurrealStore has no batch transaction (`transactional_batch: false`), so every support-plane
// write is computed client-side first and then applied as ONE `BEGIN … COMMIT` query by
// [`sr_apply`]: either every statement lands or none does.

fn key_json(key: &EdgeKey) -> serde_json::Value {
    serde_json::json!({ "src": key.0, "tgt": key.1, "kind": key.2 })
}

fn edge_row(key: &EdgeKey, edge: &Edge) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "src": key.0,
        "tgt": key.1,
        "kind": key.2,
        "file": edge.location.as_ref().map(|l| l.file.clone()).unwrap_or_default(),
        "data": serde_json::to_string(edge).map_err(se)?,
    }))
}

/// One atomic support-plane write. Statements run in this order, so a key can be deleted and
/// re-created in one call.
#[derive(Default)]
struct SupportOps {
    del_support: Vec<serde_json::Value>,
    add_support: Vec<serde_json::Value>,
    del_base: Vec<serde_json::Value>,
    add_base: Vec<serde_json::Value>,
    del_public: Vec<serde_json::Value>,
    add_public: Vec<serde_json::Value>,
    owner: Option<serde_json::Value>,
}

/// A stored edge row: parsed, plus its exact JSON text (so a restore writes back the same bytes).
type RawEdge = (Edge, String);

fn location_file(e: &Edge) -> String {
    e.location
        .as_ref()
        .map(|l| l.file.clone())
        .unwrap_or_default()
}

impl SupportOps {
    /// Replace `key`'s public row. `file` is its OWNING file (see [`SupportOps::project`]).
    fn set_public(&mut self, key: &EdgeKey, row: Option<(&str, String)>) {
        self.del_public.push(key_json(key));
        if let Some((data, file)) = row {
            self.add_public.push(serde_json::json!({
                "src": key.0, "tgt": key.1, "kind": key.2, "file": file, "data": data,
            }));
        }
    }

    /// The writes that make `key`'s public edge the projection of `base` + `facts`; with no facts,
    /// the base contribution is dropped and becomes the plain public edge again (its exact bytes).
    /// A projected row's `file` is the base contribution's file or `''`, never a fact's site — the
    /// same rule as `SqliteStore::reproject`.
    fn project(&mut self, key: &EdgeKey, base: Option<&RawEdge>, facts: &[Edge]) -> Result<()> {
        if facts.is_empty() {
            self.del_base.push(key_json(key));
            self.set_public(key, base.map(|(b, raw)| (raw.as_str(), location_file(b))));
            return Ok(());
        }
        let refs: Vec<&Edge> = facts.iter().collect();
        let file = base.map(|(b, _)| location_file(b)).unwrap_or_default();
        match project_edge(base.map(|(b, _)| b), &refs) {
            Some(p) => {
                let data = serde_json::to_string(&p).map_err(se)?;
                self.set_public(key, Some((&data, file)));
            }
            None => self.set_public(key, None),
        }
        Ok(())
    }
}

async fn sr_apply(db: &Surreal<Db>, ops: SupportOps) -> Result<()> {
    let owner = ops.owner.into_iter().collect::<Vec<_>>();
    let vars = serde_json::json!({
        "del_support": ops.del_support,
        "add_support": ops.add_support,
        "del_base": ops.del_base,
        "add_base": ops.add_base,
        "del_public": ops.del_public,
        "add_public": ops.add_public,
        "owner": owner,
    });
    db.query(
        "BEGIN TRANSACTION;
         FOR $k IN $del_support { DELETE edge_support WHERE producer=$k.producer AND snapshot=$k.snapshot
             AND src=$k.src AND tgt=$k.tgt AND kind=$k.kind AND fact_id=$k.fact_id; };
         FOR $r IN $add_support { CREATE edge_support CONTENT $r; };
         FOR $k IN $del_base { DELETE edge_base WHERE src=$k.src AND tgt=$k.tgt AND kind=$k.kind; };
         FOR $r IN $add_base { CREATE edge_base CONTENT $r; };
         FOR $k IN $del_public { DELETE edge_rel WHERE src=$k.src AND tgt=$k.tgt AND kind=$k.kind; };
         FOR $r IN $add_public { CREATE edge_rel CONTENT $r; };
         FOR $o IN $owner {
             DELETE support_owner WHERE producer=$o.producer AND snapshot=$o.snapshot;
             CREATE support_owner CONTENT $o;
         };
         COMMIT TRANSACTION;",
    )
    .bind(vars)
    .await
    .map_err(se)?
    .check()
    .map_err(se)?;
    Ok(())
}

async fn sr_key_data(db: &Surreal<Db>, table: &str, key: &EdgeKey) -> Result<Vec<Edge>> {
    let mut res = db
        .query(format!(
            "SELECT data FROM {table} WHERE src=$src AND tgt=$tgt AND kind=$kind"
        ))
        .bind(("src", key.0.clone()))
        .bind(("tgt", key.1.clone()))
        .bind(("kind", key.2.clone()))
        .await
        .map_err(se)?;
    data_col(&mut res, 0)
}

/// The exact stored row (parsed + raw JSON) of `table` for `key`.
async fn sr_raw(db: &Surreal<Db>, table: &str, key: &EdgeKey) -> Result<Option<RawEdge>> {
    let mut res = db
        .query(format!(
            "SELECT data FROM {table} WHERE src=$src AND tgt=$tgt AND kind=$kind LIMIT 1"
        ))
        .bind(("src", key.0.clone()))
        .bind(("tgt", key.1.clone()))
        .bind(("kind", key.2.clone()))
        .await
        .map_err(se)?;
    let blobs: Vec<String> = res.take((0, "data")).map_err(se)?;
    blobs
        .into_iter()
        .next()
        .map(|d| Ok((serde_json::from_str(&d).map_err(se)?, d)))
        .transpose()
}

async fn sr_count(db: &Surreal<Db>, query: &str, key: Option<&EdgeKey>) -> Result<u64> {
    let mut q = db.query(query);
    if let Some(k) = key {
        q = q
            .bind(("src", k.0.clone()))
            .bind(("tgt", k.1.clone()))
            .bind(("kind", k.2.clone()));
    }
    let mut res = q.await.map_err(se)?;
    count_of(&mut res, 0)
}

async fn sr_any_support(db: &Surreal<Db>) -> Result<bool> {
    Ok(sr_count(db, "SELECT count() FROM edge_support GROUP ALL", None).await? > 0)
}

async fn sr_key_supported(db: &Surreal<Db>, key: &EdgeKey) -> Result<bool> {
    Ok(sr_count(
        db,
        "SELECT count() FROM edge_support WHERE src=$src AND tgt=$tgt AND kind=$kind GROUP ALL",
        Some(key),
    )
    .await?
        > 0)
}

async fn sr_reproject(db: &Surreal<Db>, key: &EdgeKey) -> Result<()> {
    let facts = sr_key_data(db, "edge_support", key).await?;
    let base = sr_raw(db, "edge_base", key).await?;
    let mut ops = SupportOps::default();
    ops.project(key, base.as_ref(), &facts)?;
    sr_apply(db, ops).await
}

/// Three string columns of statement `idx`, zipped into keys.
fn key_cols(res: &mut surrealdb::IndexedResults, idx: usize) -> Result<Vec<EdgeKey>> {
    let src: Vec<String> = res.take((idx, "src")).map_err(se)?;
    let tgt: Vec<String> = res.take((idx, "tgt")).map_err(se)?;
    let kind: Vec<String> = res.take((idx, "kind")).map_err(se)?;
    Ok(src
        .into_iter()
        .zip(tgt)
        .zip(kind)
        .map(|((s, t), k)| (s, t, k))
        .collect())
}

/// Delete the base contributions of `keys` — by exact key, the delete shape `sr_apply` uses. (A
/// predicate DELETE `… WHERE file=$file OR src INSIDE $syms` on `edge_base` matched in SELECT
/// but removed nothing under surrealdb 3.2's planner; selecting the keys and deleting by key is
/// the same predicate, applied in two steps.)
async fn sr_retire_base(db: &Surreal<Db>, keys: &BTreeSet<EdgeKey>) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let ops = SupportOps {
        del_base: keys.iter().map(key_json).collect(),
        ..SupportOps::default()
    };
    sr_apply(db, ops).await
}

/// Of `candidates` (the keys an edge-deleting step is about to remove), those with support.
async fn sr_supported_of(db: &Surreal<Db>, candidates: Vec<EdgeKey>) -> Result<BTreeSet<EdgeKey>> {
    let mut out = BTreeSet::new();
    for key in candidates {
        if sr_key_supported(db, &key).await? {
            out.insert(key);
        }
    }
    Ok(out)
}

/// After an edge-deleting step: re-project the supported keys it removed and the keys whose base
/// contribution it retired. Returns how many removed supported rows came back (all of them).
async fn sr_support_post_delete(
    db: &Surreal<Db>,
    deleted: BTreeSet<EdgeKey>,
    retired: BTreeSet<EdgeKey>,
) -> Result<usize> {
    for key in deleted.union(&retired) {
        sr_reproject(db, key).await?;
    }
    Ok(deleted.len())
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
            // TS-S2A: while no support exists anywhere this is the original path.
            let supported = sr_any_support(&db).await?;
            for e in &edges {
                let kind = serde_json::to_string(&e.kind).map_err(se)?;
                if supported {
                    let key: EdgeKey = (e.source.0.clone(), e.target.0.clone(), kind.clone());
                    if sr_key_supported(&db, &key).await? {
                        // The base rule applies to the kept-aside base contribution; the public
                        // row is re-projected — a base write never erases support.
                        let base = sr_raw(&db, "edge_base", &key).await?;
                        if base.as_ref().is_none_or(|(b, _)| base_upsert_wins(b, e)) {
                            let facts = sr_key_data(&db, "edge_support", &key).await?;
                            let mut ops = SupportOps::default();
                            ops.del_base.push(key_json(&key));
                            ops.add_base.push(edge_row(&key, e)?);
                            let raw = (e.clone(), serde_json::to_string(e).map_err(se)?);
                            ops.project(&key, Some(&raw), &facts)?;
                            sr_apply(&db, ops).await?;
                        }
                        continue;
                    }
                }
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
            let support_on = sr_any_support(&db).await?;
            for n in file_nodes.iter().filter(|n| n.kind == NodeKind::Import) {
                let mut res = db
                    .query("SELECT data FROM edge_rel WHERE tgt=$sym")
                    .bind(("sym", n.symbol.0.clone()))
                    .await
                    .map_err(se)?;
                let mut incoming: Vec<Edge> = data_col(&mut res, 0)?;
                // TS-S2A: a supported edge's owning location is its base contribution's (or none)
                // — a support fact's site is not an importer.
                if support_on {
                    let mut owned = Vec::with_capacity(incoming.len());
                    for mut e in incoming {
                        let key: EdgeKey = e.dedup_key();
                        if sr_key_supported(&db, &key).await? {
                            e.location = sr_raw(&db, "edge_base", &key)
                                .await?
                                .and_then(|(b, _)| b.location);
                        }
                        owned.push(e);
                    }
                    incoming = owned;
                }
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

            // TS-S2A: capture the supported rows Step 4 removes, and retire supported keys' base
            // contributions by exactly the edge predicate. Skipped when no support exists.
            let (deleted, retired) = if support_on {
                let mut res = db
                    .query(
                        "SELECT src, tgt, kind FROM edge_rel WHERE file=$file OR src INSIDE $syms;
                         SELECT src, tgt, kind FROM edge_base WHERE file=$file OR src INSIDE $syms;",
                    )
                    .bind(("file", file.clone()))
                    .bind(("syms", file_symbols.clone()))
                    .await
                    .map_err(se)?;
                let candidates = key_cols(&mut res, 0)?;
                let retired: BTreeSet<EdgeKey> = key_cols(&mut res, 1)?.into_iter().collect();
                res.check().map_err(se)?;
                sr_retire_base(&db, &retired).await?;
                (sr_supported_of(&db, candidates).await?, retired)
            } else {
                (BTreeSet::new(), BTreeSet::new())
            };

            // Step 4: remove nodes (kept ones were re-homed, so no longer match), the owned edges
            // (a kept node's own OUTGOING edges still die — SqliteStore parity), unresolved refs,
            // digest, and content.
            // The owned edges are SELECTED by the two-part predicate and deleted by exact key: a
            // predicate DELETE `WHERE file=$file OR src INSIDE $syms` matched only the `file`
            // half under surrealdb 3.2, leaving every location-less edge sourced from this file
            // behind (pinned by `graph_store_suite`'s remove_file block).
            db.query(
                "LET $doomed = (SELECT src, tgt, kind FROM edge_rel WHERE file=$file OR src INSIDE $syms);
                 FOR $k IN $doomed { DELETE edge_rel WHERE src=$k.src AND tgt=$k.tgt AND kind=$k.kind; };
                 DELETE node WHERE file=$file;
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
            // Support is producer-owned: re-project every supported edge the deletes removed.
            sr_support_post_delete(&db, deleted, retired).await?;
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
            // TS-S2A: a dangling supported key loses its base contribution (same predicate) but
            // not its support; it is re-projected below and not counted as pruned.
            let (deleted, retired) = if sr_any_support(&db).await? {
                let mut res = db
                    .query(
                        "LET $live = (SELECT VALUE symbol FROM node);
                         SELECT src, tgt, kind FROM edge_rel WHERE src NOTINSIDE $live OR tgt NOTINSIDE $live;
                         SELECT src, tgt, kind FROM edge_base WHERE src NOTINSIDE $live OR tgt NOTINSIDE $live;",
                    )
                    .await
                    .map_err(se)?;
                let candidates = key_cols(&mut res, 1)?;
                let retired: BTreeSet<EdgeKey> = key_cols(&mut res, 2)?.into_iter().collect();
                res.check().map_err(se)?;
                sr_retire_base(&db, &retired).await?;
                (sr_supported_of(&db, candidates).await?, retired)
            } else {
                (BTreeSet::new(), BTreeSet::new())
            };
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
            // Taking the two counts does not surface an error from the DELETE (or the LET):
            // check the whole response so a failed prune cannot report success.
            res.check().map_err(se)?;
            let restored = sr_support_post_delete(&db, deleted, retired).await?;
            Ok::<_, Error>((before.saturating_sub(after) as usize).saturating_sub(restored))
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

    fn replace_edge_supports(
        &mut self,
        owner: &SupportOwner,
        generation: u64,
        facts: &[SupportFact],
    ) -> Result<SupportReplacement> {
        let incoming = normalize_facts(facts)?;
        let db = self.db.clone();
        let owner = owner.clone();
        self.rt.block_on(async move {
            // Read and decide everything first; the only write is one transactional `sr_apply`,
            // so every error path below leaves the store untouched.
            let mut res = db
                .query(
                    "SELECT VALUE generation FROM support_owner WHERE producer=$p AND snapshot=$s;
                     SELECT src, tgt, kind, fact_id, data FROM edge_support WHERE producer=$p AND snapshot=$s;",
                )
                .bind(("p", owner.producer.clone()))
                .bind(("s", owner.snapshot.clone()))
                .await
                .map_err(se)?;
            let stored_generation: Vec<i64> = res.take(0).map_err(se)?;
            let keys = key_cols(&mut res, 1)?;
            let fact_ids: Vec<String> = res.take((1, "fact_id")).map_err(se)?;
            let contents: Vec<Edge> = data_col(&mut res, 1)?;
            let stored: StoredFacts = keys
                .into_iter()
                .zip(fact_ids)
                .zip(contents)
                .map(|(id, fact)| (id, fact_key(&fact)))
                .collect();
            let plan = plan_replacement(
                &owner,
                generation,
                stored_generation.first().map(|g| *g as u64),
                &stored,
                incoming,
            )?;
            if plan.report.replayed {
                return Ok(plan.report);
            }
            let removed: BTreeSet<&(EdgeKey, String)> = plan.delete.iter().collect();
            let mut ops = SupportOps::default();
            for key in &plan.touched {
                // The key's final fact set, every owner: what is stored, minus this owner's
                // retractions, plus its insertions.
                let mut res = db
                    .query(
                        "SELECT producer, snapshot, fact_id, data FROM edge_support \
                         WHERE src=$src AND tgt=$tgt AND kind=$kind",
                    )
                    .bind(("src", key.0.clone()))
                    .bind(("tgt", key.1.clone()))
                    .bind(("kind", key.2.clone()))
                    .await
                    .map_err(se)?;
                let producers: Vec<String> = res.take((0, "producer")).map_err(se)?;
                let snapshots: Vec<String> = res.take((0, "snapshot")).map_err(se)?;
                let row_keys: Vec<String> = res.take((0, "fact_id")).map_err(se)?;
                let rows: Vec<Edge> = data_col(&mut res, 0)?;
                let had_support = !rows.is_empty();
                let mut final_facts: Vec<Edge> = Vec::new();
                for (((p, s), fk), fact) in producers.into_iter().zip(snapshots).zip(row_keys).zip(rows) {
                    let own = p == owner.producer && s == owner.snapshot;
                    if !(own && removed.contains(&(key.clone(), fk))) {
                        final_facts.push(fact);
                    }
                }
                final_facts.extend(plan.insert.iter().filter(|f| &f.key == key).map(|f| f.edge.clone()));
                // First support on this key: the base plane's edge becomes its base contribution.
                let base = if had_support {
                    sr_raw(&db, "edge_base", key).await?
                } else {
                    let public = sr_raw(&db, "edge_rel", key).await?;
                    ops.del_base.push(key_json(key));
                    if let Some((p, raw)) = &public {
                        ops.add_base.push(serde_json::json!({
                            "src": key.0, "tgt": key.1, "kind": key.2,
                            "file": location_file(p), "data": raw,
                        }));
                    }
                    public
                };
                ops.project(key, base.as_ref(), &final_facts)?;
            }
            for (key, fact_id) in &plan.delete {
                ops.del_support.push(serde_json::json!({
                    "producer": owner.producer, "snapshot": owner.snapshot,
                    "src": key.0, "tgt": key.1, "kind": key.2, "fact_id": fact_id,
                }));
            }
            for fact in &plan.insert {
                ops.add_support.push(serde_json::json!({
                    "producer": owner.producer, "snapshot": owner.snapshot,
                    "src": fact.key.0, "tgt": fact.key.1, "kind": fact.key.2,
                    "fact_id": fact.fact_id,
                    "data": serde_json::to_string(&fact.edge).map_err(se)?,
                }));
            }
            ops.owner = Some(serde_json::json!({
                "producer": owner.producer, "snapshot": owner.snapshot,
                "generation": generation as i64,
            }));
            sr_apply(&db, ops).await?;
            Ok(plan.report)
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

    fn edge_supports(
        &self,
        source: &SymbolId,
        target: &SymbolId,
        kind: &wicked_estate_core::EdgeKind,
    ) -> Result<Vec<EdgeSupport>> {
        let db = self.db.clone();
        let key: EdgeKey = (
            source.0.clone(),
            target.0.clone(),
            serde_json::to_string(kind).map_err(se)?,
        );
        self.rt.block_on(async move {
            let mut res = db
                .query(
                    "SELECT producer, snapshot, fact_id, data FROM edge_support \
                     WHERE src=$src AND tgt=$tgt AND kind=$kind",
                )
                .bind(("src", key.0.clone()))
                .bind(("tgt", key.1.clone()))
                .bind(("kind", key.2.clone()))
                .await
                .map_err(se)?;
            let producers: Vec<String> = res.take((0, "producer")).map_err(se)?;
            let snapshots: Vec<String> = res.take((0, "snapshot")).map_err(se)?;
            let fact_keys: Vec<String> = res.take((0, "fact_id")).map_err(se)?;
            let facts: Vec<Edge> = data_col(&mut res, 0)?;
            let mut out = Vec::with_capacity(facts.len());
            let mut generations: BTreeMap<SupportOwner, u64> = BTreeMap::new();
            for (((p, s), fk), fact) in producers.into_iter().zip(snapshots).zip(fact_keys).zip(facts) {
                let owner = SupportOwner::new(p, s)?;
                let generation = match generations.get(&owner) {
                    Some(g) => *g,
                    None => {
                        let mut res = db
                            .query("SELECT VALUE generation FROM support_owner WHERE producer=$p AND snapshot=$s")
                            .bind(("p", owner.producer.clone()))
                            .bind(("s", owner.snapshot.clone()))
                            .await
                            .map_err(se)?;
                        let g: Vec<i64> = res.take(0).map_err(se)?;
                        let g = g.first().copied().unwrap_or(0) as u64;
                        generations.insert(owner.clone(), g);
                        g
                    }
                };
                out.push(EdgeSupport::new(owner, generation, fk, fact));
            }
            out.sort_by(|a, b| (&a.owner, &a.fact_id).cmp(&(&b.owner, &b.fact_id)));
            Ok(out)
        })
    }

    fn support_generation(&self, owner: &SupportOwner) -> Result<Option<u64>> {
        let db = self.db.clone();
        let owner = owner.clone();
        self.rt.block_on(async move {
            let mut res = db
                .query(
                    "SELECT VALUE generation FROM support_owner WHERE producer=$p AND snapshot=$s",
                )
                .bind(("p", owner.producer))
                .bind(("s", owner.snapshot))
                .await
                .map_err(se)?;
            let g: Vec<i64> = res.take(0).map_err(se)?;
            Ok(g.first().map(|g| *g as u64))
        })
    }

    fn support_owners(&self) -> Result<Vec<SupportOwnerState>> {
        let db = self.db.clone();
        self.rt.block_on(async move {
            let mut res = db
                .query("SELECT producer, snapshot, generation FROM support_owner")
                .await
                .map_err(se)?;
            let producers: Vec<String> = res.take((0, "producer")).map_err(se)?;
            let snapshots: Vec<String> = res.take((0, "snapshot")).map_err(se)?;
            let generations: Vec<i64> = res.take((0, "generation")).map_err(se)?;
            let mut out = producers
                .into_iter()
                .zip(snapshots)
                .zip(generations)
                .map(|((p, s), g)| Ok(SupportOwnerState::new(SupportOwner::new(p, s)?, g as u64)))
                .collect::<Result<Vec<_>>>()?;
            out.sort_by(|a, b| a.owner.cmp(&b.owner));
            Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{EdgeKind, ResolutionTier, SupportFact};

    fn calls(target: &str, by: &str) -> Edge {
        Edge::new(
            SymbolId("s:src".into()),
            SymbolId(format!("s:{target}")),
            EdgeKind::Calls,
            ResolutionTier::Scip,
            by,
        )
    }

    fn define(store: &SurrealStore, ddl: &str) {
        let db = store.db.clone();
        let ddl = ddl.to_string();
        store
            .rt
            .block_on(async move { db.query(ddl).await?.check() })
            .unwrap();
    }

    fn state(
        store: &SurrealStore,
        owner: &SupportOwner,
    ) -> (Vec<Edge>, Vec<EdgeSupport>, Option<u64>) {
        let mut edges = store.all_edges().unwrap();
        edges.sort_by_key(|e| e.dedup_key());
        let mut rows = Vec::new();
        for t in ["a", "b", "c", "d"] {
            let e = calls(t, "x");
            rows.extend(store.edge_supports(&e.source, &e.target, &e.kind).unwrap());
        }
        (edges, rows, store.support_generation(owner).unwrap())
    }

    /// The conformance suite's failures are all rejected before any write; this one fails INSIDE
    /// the single `BEGIN … COMMIT` (a field ASSERT refuses the poison row after the retraction and
    /// an earlier insert ran) and requires the store to be unchanged — the transaction, not
    /// up-front validation, is what keeps a half-old/half-new generation out.
    #[test]
    fn a_failure_inside_the_replacement_transaction_rolls_everything_back() {
        let mut store = SurrealStore::in_memory().unwrap();
        let owner = SupportOwner::new("scip-typescript", "web").unwrap();
        let facts = |targets: &[(&str, &str)]| -> Vec<SupportFact> {
            targets
                .iter()
                .map(|(t, by)| SupportFact::new(format!("occ:{t}"), calls(t, by)).unwrap())
                .collect()
        };
        store
            .replace_edge_supports(&owner, 1, &facts(&[("a", "scip"), ("b", "scip")]))
            .unwrap();
        define(
            &store,
            "DEFINE FIELD OVERWRITE data ON edge_support TYPE string \
             ASSERT !string::contains($value, 'poison');",
        );
        let before = state(&store, &owner);
        // `{a,b}` → `{b,c,d}` where d is poisoned: the delete of a and the insert of c precede it.
        let err = store
            .replace_edge_supports(
                &owner,
                2,
                &facts(&[("b", "scip"), ("c", "scip"), ("d", "poison")]),
            )
            .expect_err("the ASSERT rejects the poison row");
        assert!(!err.to_string().is_empty());
        assert_eq!(
            state(&store, &owner),
            before,
            "no half-old/half-new generation"
        );
        define(
            &store,
            "DEFINE FIELD OVERWRITE data ON edge_support TYPE string;",
        );
        let ok = store
            .replace_edge_supports(&owner, 2, &facts(&[("b", "scip"), ("c", "scip")]))
            .expect("generation 2 is still free after the rollback");
        assert_eq!((ok.asserted, ok.retained, ok.retracted), (1, 1, 1));
    }
}
