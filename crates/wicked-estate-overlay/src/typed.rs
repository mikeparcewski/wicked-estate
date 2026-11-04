//! Typed knowledge-plane edges (DES-KNOWLEDGE-DATA-001 §10.1): the links between source structure,
//! data shapes, code, rules and trackers that make up the combined knowledge view.
//!
//! They sit beside the `about` rows in `xedge.db` but in their own `typed_xedges` table, because
//! they differ in three ways the `about` arm does not need:
//!
//! - **a closed relation set** ([`TYPED_RELS`]): `describes` (table → code entity), `joins`
//!   (column ↔ column), `governed_by` (table → rule), `measures` (column → metric), `tracked_in`,
//!   `addresses`, `closes` (project-management links) and `derived_from` (a node to what it was
//!   built from);
//! - **an owner**: the knowledge node (an approved data shape) that asserted the edge. The owner is
//!   part of the key, so two shapes asserting the same edge keep two rows, and removing one shape's
//!   edges ([`XedgeStore::delete_typed_by_owner`]) never removes another's;
//! - **`meta`**: a JSON object the knowledge engine writes (the owner's `deps`, a code endpoint's
//!   `repo`, the proposer's `evidence`). This crate stores it as text and never reads it.
//!
//! Endpoints are store-qualified ([`Endpoint::engine`] is `"knowledge"` or `"estate"`) and
//! epoch-stamped like every cross-edge: a reader drops an edge whose stamped epoch differs from the
//! endpoint's live one (DEC-X6), so a deleted-then-re-added node never inherits old edges.
//!
//! [`OverlayReader`](crate::OverlayReader) never reads this table: memory's cross recall crosses only
//! the rels it names in `xedges`, so a typed edge cannot leak into it.

use crate::xedge::{Endpoint, XedgeReader, XedgeStore, map_err};
use rusqlite::params;
use wicked_estate_core::{Error, Result};

/// The typed relations the knowledge plane writes. Anything else is refused at write.
pub const TYPED_RELS: [&str; 8] = [
    "describes",
    "joins",
    "governed_by",
    "measures",
    "tracked_in",
    "addresses",
    "closes",
    "derived_from",
];

/// Is `rel` one of the knowledge plane's typed relations?
pub fn is_typed_rel(rel: &str) -> bool {
    TYPED_RELS.contains(&rel)
}

/// The `typed_xedges` DDL, applied by `XedgeStore::open` (idempotent).
pub(crate) const TYPED_SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS typed_xedges (
        src_store  TEXT    NOT NULL,
        src_id     TEXT    NOT NULL,
        src_epoch  INTEGER NOT NULL,
        tgt_store  TEXT    NOT NULL,
        tgt_id     TEXT    NOT NULL,
        tgt_epoch  INTEGER NOT NULL,
        rel        TEXT    NOT NULL,
        owner      TEXT    NOT NULL,
        confidence REAL    NOT NULL,
        meta       TEXT    NOT NULL DEFAULT '{}',
        PRIMARY KEY (src_store, src_id, tgt_store, tgt_id, rel, owner)
    );
    CREATE INDEX IF NOT EXISTS idx_typed_xedges_src ON typed_xedges (src_store, src_id, rel);
    CREATE INDEX IF NOT EXISTS idx_typed_xedges_tgt ON typed_xedges (tgt_store, tgt_id, rel);
    CREATE INDEX IF NOT EXISTS idx_typed_xedges_owner ON typed_xedges (owner);";

/// One typed edge: `(source) --rel--> (target)`, asserted by `owner`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedXEdge {
    pub source: Endpoint,
    pub target: Endpoint,
    /// One of [`TYPED_RELS`].
    pub rel: String,
    /// The node that asserted the edge (a data shape's knowledge node id).
    pub owner: String,
    pub confidence: f32,
    /// A JSON object, as text (written and read by the knowledge engine only).
    pub meta: String,
}

impl XedgeStore {
    /// Insert or update one typed edge (keyed on both endpoints, `rel` and `owner`; a re-put
    /// refreshes the epochs, confidence and meta). Refuses a relation outside [`TYPED_RELS`], an
    /// empty owner and a self-loop.
    pub fn put_typed(&self, e: &TypedXEdge) -> Result<()> {
        if !is_typed_rel(&e.rel) {
            return Err(Error::Invalid(format!(
                "typed xedge: unknown rel {:?} (expected one of {})",
                e.rel,
                TYPED_RELS.join("|")
            )));
        }
        if e.owner.is_empty() {
            return Err(Error::Invalid("typed xedge: owner required".into()));
        }
        if e.source.engine == e.target.engine && e.source.stable_id == e.target.stable_id {
            return Err(Error::Invalid(
                "typed xedge: an edge cannot loop to itself".into(),
            ));
        }
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        conn.execute(
            "INSERT INTO typed_xedges
                (src_store, src_id, src_epoch, tgt_store, tgt_id, tgt_epoch, rel, owner, confidence, meta)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (src_store, src_id, tgt_store, tgt_id, rel, owner)
             DO UPDATE SET src_epoch = excluded.src_epoch, tgt_epoch = excluded.tgt_epoch,
                           confidence = excluded.confidence, meta = excluded.meta",
            params![
                e.source.engine,
                e.source.stable_id,
                e.source.epoch as i64,
                e.target.engine,
                e.target.stable_id,
                e.target.epoch as i64,
                e.rel,
                e.owner,
                e.confidence as f64,
                e.meta,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    /// Delete every typed edge `owner` asserted. Returns the number of rows removed.
    pub fn delete_typed_by_owner(&self, owner: &str) -> Result<u64> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        let n = conn
            .execute("DELETE FROM typed_xedges WHERE owner = ?1", params![owner])
            .map_err(map_err)?;
        Ok(n as u64)
    }

    /// Total typed-edge row count (health / test assertion).
    pub fn typed_len(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM typed_xedges", [], |r| r.get(0))
            .map_err(map_err)?;
        Ok(n as u64)
    }
}

impl XedgeReader {
    /// Typed edges whose SOURCE is `(store, id)`, filtered to `rels` (empty = every typed rel).
    pub fn typed_out(&self, store: &str, id: &str, rels: &[&str]) -> Result<Vec<TypedXEdge>> {
        self.typed_query("src_store", "src_id", store, id, rels)
    }

    /// Typed edges whose TARGET is `(store, id)`, filtered to `rels` (empty = every typed rel).
    pub fn typed_in(&self, store: &str, id: &str, rels: &[&str]) -> Result<Vec<TypedXEdge>> {
        self.typed_query("tgt_store", "tgt_id", store, id, rels)
    }

    /// Every typed edge owned by `owner`.
    pub fn typed_by_owner(&self, owner: &str) -> Result<Vec<TypedXEdge>> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        let mut stmt = conn
            .prepare(&format!("{SELECT_TYPED} WHERE owner = ?1 {ORDER_TYPED}"))
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![owner], row_to_typed)
            .map_err(map_err)?;
        rows.map(|r| r.map_err(map_err)).collect()
    }

    fn typed_query(
        &self,
        store_col: &str,
        id_col: &str,
        store: &str,
        id: &str,
        rels: &[&str],
    ) -> Result<Vec<TypedXEdge>> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        let mut sql = format!("{SELECT_TYPED} WHERE {store_col} = ?1 AND {id_col} = ?2");
        let mut bound: Vec<String> = vec![store.to_string(), id.to_string()];
        if !rels.is_empty() {
            let placeholders: Vec<String> =
                (0..rels.len()).map(|i| format!("?{}", i + 3)).collect();
            sql.push_str(&format!(" AND rel IN ({})", placeholders.join(", ")));
            bound.extend(rels.iter().map(|r| r.to_string()));
        }
        sql.push(' ');
        sql.push_str(ORDER_TYPED);
        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bound.iter()), row_to_typed)
            .map_err(map_err)?;
        rows.map(|r| r.map_err(map_err)).collect()
    }
}

const SELECT_TYPED: &str = "SELECT src_store, src_id, src_epoch, tgt_store, tgt_id, tgt_epoch, \
                            rel, owner, confidence, meta FROM typed_xedges";
const ORDER_TYPED: &str = "ORDER BY rel, src_store, src_id, tgt_store, tgt_id, owner";

fn row_to_typed(row: &rusqlite::Row<'_>) -> rusqlite::Result<TypedXEdge> {
    Ok(TypedXEdge {
        source: Endpoint::new(
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? as u64,
        ),
        target: Endpoint::new(
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)? as u64,
        ),
        rel: row.get(6)?,
        owner: row.get(7)?,
        confidence: row.get::<_, f64>(8)? as f32,
        meta: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::XEdge;

    fn edge(src: &str, tgt: &str, rel: &str, owner: &str) -> TypedXEdge {
        TypedXEdge {
            source: Endpoint::new("knowledge", src, 0),
            target: Endpoint::new("estate", tgt, 2),
            rel: rel.into(),
            owner: owner.into(),
            confidence: 0.9,
            meta: r#"{"deps":["source:s1"]}"#.into(),
        }
    }

    #[test]
    fn typed_edges_round_trip_and_filter_by_rel_and_side() {
        let store = XedgeStore::in_memory().unwrap();
        store
            .put_typed(&edge("doc:tickets", "Ticket", "describes", "shape:a"))
            .unwrap();
        store
            .put_typed(&edge("doc:tickets", "sla_rule", "governed_by", "shape:a"))
            .unwrap();
        let r = store.reader();
        let out = r.typed_out("knowledge", "doc:tickets", &[]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            r.typed_out("knowledge", "doc:tickets", &["describes"])
                .unwrap()
                .len(),
            1
        );
        let inbound = r.typed_in("estate", "Ticket", &[]).unwrap();
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].target.epoch, 2, "epochs round-trip");
        assert_eq!(inbound[0].meta, r#"{"deps":["source:s1"]}"#);
        // The `about` arm is a different table: nothing typed shows there, and back.
        assert!(store.is_empty().unwrap());
        store.put_edge(&XEdge::about("m1", "Ticket", 0)).unwrap();
        assert_eq!(store.typed_len().unwrap(), 2);
    }

    #[test]
    fn owner_is_part_of_the_key_and_deletes_only_its_own_rows() {
        let store = XedgeStore::in_memory().unwrap();
        // Two shapes assert the SAME edge: two rows.
        store
            .put_typed(&edge("doc:tickets", "Ticket", "describes", "shape:a"))
            .unwrap();
        store
            .put_typed(&edge("doc:tickets", "Ticket", "describes", "shape:b"))
            .unwrap();
        // A re-put by the same owner updates in place.
        store
            .put_typed(&edge("doc:tickets", "Ticket", "describes", "shape:a"))
            .unwrap();
        assert_eq!(store.typed_len().unwrap(), 2);
        assert_eq!(store.delete_typed_by_owner("shape:a").unwrap(), 1);
        let left = store.reader().typed_by_owner("shape:b").unwrap();
        assert_eq!(left.len(), 1, "another shape's identical edge survives");
    }

    #[test]
    fn unknown_rel_empty_owner_and_self_loop_are_refused() {
        let store = XedgeStore::in_memory().unwrap();
        assert!(
            store
                .put_typed(&edge("a", "b", "about", "shape:a"))
                .is_err()
        );
        assert!(store.put_typed(&edge("a", "b", "describes", "")).is_err());
        let mut lp = edge("a", "a", "joins", "shape:a");
        lp.target = Endpoint::new("knowledge", "a", 0);
        assert!(store.put_typed(&lp).is_err());
        assert_eq!(store.typed_len().unwrap(), 0);
    }

    /// An `xedge.db` written before typed edges existed (schema v1, `xedges` + `meta` only) opens
    /// and gains the table; its `about` rows are untouched.
    #[test]
    fn an_older_xedge_db_gains_the_typed_table_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("xedge.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE xedges (
                   src_engine TEXT NOT NULL, src_id TEXT NOT NULL, src_epoch INTEGER NOT NULL,
                   tgt_engine TEXT NOT NULL, tgt_id TEXT NOT NULL, tgt_epoch INTEGER NOT NULL,
                   rel TEXT NOT NULL, confidence REAL NOT NULL, provenance TEXT NOT NULL,
                   PRIMARY KEY (src_engine, src_id, src_epoch, tgt_engine, tgt_id, tgt_epoch, rel));
                 INSERT INTO xedges VALUES ('memory','m1',0,'estate','c1',0,'about',1.0,'t');",
            )
            .unwrap();
        }
        let store = XedgeStore::open(path.to_str().unwrap()).unwrap();
        assert_eq!(store.len().unwrap(), 1, "the about row survives");
        store
            .put_typed(&edge("doc:tickets", "Ticket", "describes", "shape:a"))
            .unwrap();
        assert_eq!(store.typed_len().unwrap(), 1);
    }
}
