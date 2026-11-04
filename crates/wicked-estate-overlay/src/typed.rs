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
//! - **an owner**: the knowledge node (an approved data shape) that asserted the edge, or, for a
//!   link a source's document owns (DES-PM-001 §3.4), `<doc node id>@<generation>`: the document AT
//!   the generation that asserted it, so two generations' rows never share a key and a reader
//!   keeps a link only while its document is at that generation. The owner is part of the key, so
//!   two shapes asserting the same edge keep two rows, and removing one owner's edges
//!   ([`XedgeStore::delete_typed_by_owner`]) never removes another's;
//! - **`meta`**: a JSON object the knowledge engine writes (the owner's `deps`, a code endpoint's
//!   `repo`, the proposer's `evidence`). This crate stores it as text and never reads it.
//!
//! Endpoints are store-qualified ([`Endpoint::engine`] is `"knowledge"`, `"estate"` or `"control"`)
//! and epoch-stamped like every cross-edge: a reader drops an edge whose stamped epoch differs from
//! the endpoint's live one (DEC-X6), so a deleted-then-re-added node never inherits old edges. A
//! `"control"` endpoint (a crew `project:`, `repo:`, `pr:` or `run:`) has no epoch in estate: it is
//! stamped 0 and never epoch-checked, and its visibility comes from the deps the knowledge engine
//! derives from its id (a run's from its links' filings).
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

/// The `typed_xedges` DDL. Applied on the FIRST typed write, never on open, so a `--readonly`
/// server opening an older `xedge.db` writes nothing; readers treat a missing table as empty.
const TYPED_SCHEMA: &str = "
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

/// Does the `typed_xedges` table exist yet?
fn typed_table_exists(conn: &rusqlite::Connection) -> Result<bool> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'typed_xedges'",
            [],
            |r| r.get(0),
        )
        .map_err(map_err)?;
    Ok(n > 0)
}

/// The checks every typed write runs: a closed rel, an owner, no self-loop.
fn check_typed(e: &TypedXEdge) -> Result<()> {
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
    Ok(())
}

const UPSERT_TYPED: &str = "INSERT INTO typed_xedges
                (src_store, src_id, src_epoch, tgt_store, tgt_id, tgt_epoch, rel, owner, confidence, meta)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (src_store, src_id, tgt_store, tgt_id, rel, owner)
             DO UPDATE SET src_epoch = excluded.src_epoch, tgt_epoch = excluded.tgt_epoch,
                           confidence = excluded.confidence, meta = excluded.meta";

fn upsert_typed(conn: &rusqlite::Connection, e: &TypedXEdge) -> Result<()> {
    conn.execute(
        UPSERT_TYPED,
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

/// The exclusive upper bound of the owners that start with `prefix` (its last byte plus one), so a
/// prefix read is an index range. `None` for a prefix that is empty, or does not end in ASCII
/// below DEL.
fn prefix_upper(prefix: &str) -> Option<String> {
    let last = *prefix.as_bytes().last()?;
    if !(0x20..0x7f).contains(&last) {
        return None;
    }
    let mut upper = prefix[..prefix.len() - 1].to_string();
    upper.push(char::from(last + 1));
    Some(upper)
}

impl XedgeStore {
    /// Insert or update one typed edge (keyed on both endpoints, `rel` and `owner`; a re-put
    /// refreshes the epochs, confidence and meta). Refuses a relation outside [`TYPED_RELS`], an
    /// empty owner and a self-loop.
    pub fn put_typed(&self, e: &TypedXEdge) -> Result<()> {
        check_typed(e)?;
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        conn.execute_batch(TYPED_SCHEMA).map_err(map_err)?;
        upsert_typed(&conn, e)
    }

    /// [`Self::put_typed`] for many edges in ONE transaction: every edge is checked first, and
    /// then all of them land or none does.
    pub fn put_typed_batch(&self, edges: &[TypedXEdge]) -> Result<()> {
        for e in edges {
            check_typed(e)?;
        }
        if edges.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().expect("xedge mutex poisoned");
        conn.execute_batch(TYPED_SCHEMA).map_err(map_err)?;
        let tx = conn.transaction().map_err(map_err)?;
        for e in edges {
            upsert_typed(&tx, e)?;
        }
        tx.commit().map_err(map_err)
    }

    /// Delete every typed edge any of `owners` asserted, in ONE transaction. Returns the number of
    /// rows removed.
    pub fn delete_typed_by_owners(&self, owners: &[String]) -> Result<u64> {
        let mut conn = self.conn.lock().expect("xedge mutex poisoned");
        if owners.is_empty() || !typed_table_exists(&conn)? {
            return Ok(0);
        }
        let tx = conn.transaction().map_err(map_err)?;
        let mut n = 0u64;
        for owner in owners {
            n += tx
                .execute("DELETE FROM typed_xedges WHERE owner = ?1", params![owner])
                .map_err(map_err)? as u64;
        }
        tx.commit().map_err(map_err)?;
        Ok(n)
    }

    /// Delete every typed edge `owner` asserted. Returns the number of rows removed.
    pub fn delete_typed_by_owner(&self, owner: &str) -> Result<u64> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        if !typed_table_exists(&conn)? {
            return Ok(0);
        }
        let n = conn
            .execute("DELETE FROM typed_xedges WHERE owner = ?1", params![owner])
            .map_err(map_err)?;
        Ok(n as u64)
    }

    /// Total typed-edge row count (health / test assertion).
    pub fn typed_len(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        if !typed_table_exists(&conn)? {
            return Ok(0);
        }
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM typed_xedges", [], |r| r.get(0))
            .map_err(map_err)?;
        Ok(n as u64)
    }
}

impl XedgeReader {
    /// Typed edges whose SOURCE is `(store, id)`, filtered to `rels` (empty = every typed rel), at
    /// most `limit` rows in a stable order.
    pub fn typed_out(
        &self,
        store: &str,
        id: &str,
        rels: &[&str],
        limit: usize,
    ) -> Result<Vec<TypedXEdge>> {
        self.typed_query("src_store", "src_id", store, id, rels, limit)
    }

    /// Typed edges whose TARGET is `(store, id)`, filtered to `rels` (empty = every typed rel), at
    /// most `limit` rows in a stable order.
    pub fn typed_in(
        &self,
        store: &str,
        id: &str,
        rels: &[&str],
        limit: usize,
    ) -> Result<Vec<TypedXEdge>> {
        self.typed_query("tgt_store", "tgt_id", store, id, rels, limit)
    }

    /// SQLite's `PRAGMA data_version` on this reader's connection: it changes whenever another
    /// connection commits to `xedge.db` (a reader compares it before and after a multi-step read).
    pub fn data_version(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        conn.query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0))
            .map(|v| v as u64)
            .map_err(map_err)
    }

    /// The distinct owners that start with `prefix`, in order, at most `limit` (an index range read;
    /// `prefix` must end in printable ASCII, else this is an error).
    pub fn typed_owners_with_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        let upper = prefix_upper(prefix).ok_or_else(|| {
            Error::Invalid(format!(
                "typed xedge: owner prefix {prefix:?} must end in printable ASCII"
            ))
        })?;
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        if !typed_table_exists(&conn)? {
            return Ok(Vec::new());
        }
        let mut stmt = conn
            .prepare(&format!(
                "SELECT DISTINCT owner FROM typed_xedges WHERE owner >= ?1 AND owner < ?2 \
                 ORDER BY owner LIMIT {}",
                limit.min(i64::MAX as usize)
            ))
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![prefix, upper], |r| r.get::<_, String>(0))
            .map_err(map_err)?;
        rows.map(|r| r.map_err(map_err)).collect()
    }

    /// Every typed edge owned by `owner`.
    pub fn typed_by_owner(&self, owner: &str) -> Result<Vec<TypedXEdge>> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        if !typed_table_exists(&conn)? {
            return Ok(Vec::new());
        }
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
        limit: usize,
    ) -> Result<Vec<TypedXEdge>> {
        let conn = self.conn.lock().expect("xedge mutex poisoned");
        if !typed_table_exists(&conn)? {
            return Ok(Vec::new());
        }
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
        sql.push_str(&format!(" LIMIT {}", limit.min(i64::MAX as usize)));
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
        let out = r.typed_out("knowledge", "doc:tickets", &[], 100).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            r.typed_out("knowledge", "doc:tickets", &["describes"], 100)
                .unwrap()
                .len(),
            1
        );
        let inbound = r.typed_in("estate", "Ticket", &[], 100).unwrap();
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
    fn an_older_xedge_db_opens_untouched_and_gains_the_table_on_first_write() {
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
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            assert!(
                !typed_table_exists(&conn).unwrap(),
                "opening writes nothing: a --readonly server must not migrate the file"
            );
        }
        let r = store.reader();
        assert!(r.typed_in("estate", "Ticket", &[], 10).unwrap().is_empty());
        assert_eq!(store.typed_len().unwrap(), 0);
        assert_eq!(store.delete_typed_by_owner("shape:a").unwrap(), 0);
        store
            .put_typed(&edge("doc:tickets", "Ticket", "describes", "shape:a"))
            .unwrap();
        assert_eq!(store.typed_len().unwrap(), 1);
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;

    fn edge(tgt: &str, owner: &str) -> TypedXEdge {
        TypedXEdge {
            source: Endpoint::new("knowledge", "doc", 0),
            target: Endpoint::new("control", tgt, 0),
            rel: "tracked_in".into(),
            owner: owner.into(),
            confidence: 1.0,
            meta: "{}".into(),
        }
    }

    #[test]
    fn a_batch_lands_whole_or_not_at_all() {
        let store = XedgeStore::in_memory().unwrap();
        store
            .put_typed_batch(&[edge("project:a", "o@1"), edge("repo:r", "o@1")])
            .unwrap();
        assert_eq!(store.typed_len().unwrap(), 2);
        // One bad edge refuses the whole batch before anything is written.
        let mut bad = edge("project:b", "o@2");
        bad.rel = "about".into();
        assert!(
            store
                .put_typed_batch(&[edge("project:c", "o@2"), bad])
                .is_err()
        );
        assert_eq!(store.typed_len().unwrap(), 2);
        assert!(store.put_typed_batch(&[]).is_ok());
    }

    /// A store error on the second of two valid rows rolls the first back (review r1: the check
    /// above refuses before any SQL; this one fails inside the transaction).
    #[test]
    fn a_store_error_mid_batch_rolls_the_batch_back() {
        let store = XedgeStore::in_memory().unwrap();
        store.put_typed(&edge("project:a", "o@1")).unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER refuse_doomed BEFORE INSERT ON typed_xedges
                 WHEN NEW.tgt_id = 'project:doomed' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .unwrap();
        assert!(
            store
                .put_typed_batch(&[edge("project:b", "o@2"), edge("project:doomed", "o@2")])
                .is_err()
        );
        assert_eq!(store.typed_len().unwrap(), 1, "project:b was rolled back");
        assert!(store.reader().typed_by_owner("o@2").unwrap().is_empty());
    }

    /// Another connection's commit changes `data_version`; this connection's own write does not.
    #[test]
    fn data_version_moves_on_another_connections_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("xedge.db");
        let a = XedgeStore::open(path.to_str().unwrap()).unwrap();
        let b = XedgeStore::open(path.to_str().unwrap()).unwrap();
        let v0 = a.reader().data_version().unwrap();
        a.put_typed(&edge("project:own", "o@1")).unwrap();
        assert_eq!(a.reader().data_version().unwrap(), v0, "its own write");
        b.put_typed(&edge("project:other", "o@1")).unwrap();
        assert_ne!(
            a.reader().data_version().unwrap(),
            v0,
            "another connection's commit"
        );
    }

    #[test]
    fn owners_are_listed_by_prefix_and_deleted_together() {
        let store = XedgeStore::in_memory().unwrap();
        assert!(
            store
                .reader()
                .typed_owners_with_prefix("kdoc synthetic source:t1/", 10)
                .unwrap()
                .is_empty(),
            "no table yet"
        );
        store
            .put_typed_batch(&[
                edge("project:a", "kdoc synthetic source:t1/x:@1"),
                edge("repo:r", "kdoc synthetic source:t1/x:@1"),
                edge("project:a", "kdoc synthetic source:t1/y:@2"),
                edge("project:a", "kdoc synthetic source:t10/x:@1"),
                edge("project:a", "kdoc synthetic source:t1:@1"),
            ])
            .unwrap();
        let r = store.reader();
        assert_eq!(
            r.typed_owners_with_prefix("kdoc synthetic source:t1/", 10)
                .unwrap(),
            vec![
                "kdoc synthetic source:t1/x:@1".to_string(),
                "kdoc synthetic source:t1/y:@2".to_string()
            ],
            "distinct, ordered, and t10 is another source"
        );
        assert_eq!(
            r.typed_owners_with_prefix("kdoc synthetic source:t1/", 1)
                .unwrap()
                .len(),
            1
        );
        assert!(r.typed_owners_with_prefix("", 10).is_err());
        assert_eq!(
            store
                .delete_typed_by_owners(&["kdoc synthetic source:t1/x:@1".to_string()])
                .unwrap(),
            2
        );
        assert_eq!(store.typed_len().unwrap(), 3);
        assert_eq!(store.delete_typed_by_owners(&[]).unwrap(), 0);
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    #[test]
    fn adjacency_reads_are_bounded() {
        let store = XedgeStore::in_memory().unwrap();
        for i in 0..30 {
            store
                .put_typed(&TypedXEdge {
                    source: Endpoint::new("knowledge", "hub", 0),
                    target: Endpoint::new("knowledge", format!("n{i:02}"), 0),
                    rel: "joins".into(),
                    owner: "shape:a".into(),
                    confidence: 1.0,
                    meta: "{}".into(),
                })
                .unwrap();
        }
        let r = store.reader();
        let some = r.typed_out("knowledge", "hub", &[], 10).unwrap();
        assert_eq!(some.len(), 10);
        assert_eq!(some[0].target.stable_id, "n00", "a stable order");
    }
}
