//! `memory_node_ext` — a memory-owned **sidecar** index (PR-1; design in `docs/phases/PR-1-DESIGN.md`).
//!
//! `wicked-estate`'s `SqliteStore` encapsulates its connection, so memory keeps its own indexed
//! projection of recall/consolidation-critical fields in a **separate** SQLite file it solely owns
//! (no two-writers-one-file contention, `estate-core ⇏ memory` firewall intact). It stores the raw,
//! indexable inputs (tier / created_at / access / reinforcement) so consolidation can pull a **bounded
//! candidate set** by cheap indexed predicates instead of scanning + deserializing every node;
//! time-dependent salience is then computed in Rust on that small set (no stale snapshot).
//!
//! Consistency: estate-node and ext-row writes aren't atomic across files; [`MemExt::reconcile`] runs
//! at open and rebuilds the index from the authoritative nodes if the row counts diverge (crash-safe
//! by rebuild, not 2PC).

use rusqlite::{Connection, params};
use wicked_estate_core::Result as EResult;
use wicked_estate_core::error::Error as EError;
use wicked_estate_memory_core::{Memory, Tier};

fn map_err<E: std::fmt::Display>(e: E) -> EError {
    EError::Storage(format!("memext: {e}"))
}

/// The sidecar index connection + schema.
pub(crate) struct MemExt {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS memory_node_ext(
  node_id         TEXT PRIMARY KEY,
  mem_kind        TEXT NOT NULL,
  tier            INTEGER NOT NULL,
  scope           TEXT NOT NULL DEFAULT '',
  created_at      INTEGER NOT NULL,
  last_access     INTEGER NOT NULL,
  access_count    INTEGER NOT NULL,
  reinforce_pos   INTEGER NOT NULL,
  reinforce_total INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mem_ext ON memory_node_ext(tier, created_at);
CREATE INDEX IF NOT EXISTS idx_mem_ext_kind ON memory_node_ext(mem_kind, reinforce_total);
";

/// Deterministic per-row hash over the drift-prone fields (order-independent when wrapping-summed).
fn row_hash(
    node_id: &str,
    tier: i64,
    reinforce_total: i64,
    last_access: i64,
    access_count: i64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    node_id.hash(&mut h);
    tier.hash(&mut h);
    reinforce_total.hash(&mut h);
    last_access.hash(&mut h);
    access_count.hash(&mut h);
    h.finish()
}

/// Fingerprint of the authoritative memory set, comparable to [`MemExt::fingerprint`].
pub fn memory_fingerprint(all: &[Memory]) -> u64 {
    let mut acc = 0u64;
    for m in all {
        acc = acc.wrapping_add(row_hash(
            &m.symbol().0,
            tier_code(m.tier),
            m.reinforce_total as i64,
            m.last_access,
            m.access_count as i64,
        ));
    }
    acc
}

/// Current sidecar schema version (NFR-8). Bump when adding a migration below.
pub const CURRENT_SCHEMA_VERSION: i64 = 1;

/// Ordered forward migrations. `MIGRATIONS[i]` upgrades schema version `i+1` → `i+2` and runs only
/// when the open DB's `user_version` is `< i+2`. Empty at v1 (the base `SCHEMA` IS v1). Append-only:
/// never edit a shipped entry — add a new one. Guarantees forward migration without data loss (AC-11).
const MIGRATIONS: &[&str] = &[
    // e.g. v1→v2: "ALTER TABLE memory_node_ext ADD COLUMN ...;"
];

// Compile-time invariant: the schema version equals the base (v1) plus the number of migrations.
// Adding a migration without bumping CURRENT_SCHEMA_VERSION (or vice-versa) fails the build.
const _: () = assert!(
    MIGRATIONS.len() + 1 == CURRENT_SCHEMA_VERSION as usize,
    "bump CURRENT_SCHEMA_VERSION to base(1) + MIGRATIONS.len() when adding a migration"
);

/// Apply pending forward migrations on `conn`. Each migration runs in its OWN transaction together
/// with its version bump, so a partial failure rolls back atomically and a re-open resumes at the
/// last successfully-applied version (no "duplicate column" dead-end). The base `SCHEMA` IS v1.
fn migrate(conn: &Connection) -> EResult<()> {
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(map_err)?;
    // Stamp the base version for fresh / pre-versioned DBs (SCHEMA already created the v1 tables).
    let mut cur = v;
    if cur < 1 {
        conn.execute_batch("PRAGMA user_version = 1;")
            .map_err(map_err)?;
        cur = 1;
    }
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = (i as i64) + 2; // MIGRATIONS[0]: v1 → v2
        if cur < target {
            conn.execute_batch("BEGIN").map_err(map_err)?;
            let step = conn
                .execute_batch(sql)
                .and_then(|_| conn.execute_batch(&format!("PRAGMA user_version = {target};")));
            match step {
                Ok(()) => {
                    conn.execute_batch("COMMIT").map_err(map_err)?;
                    cur = target;
                }
                Err(e) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(map_err(e)); // user_version unchanged → re-open resumes here
                }
            }
        }
    }
    Ok(())
}

fn tier_code(t: Tier) -> i64 {
    match t {
        Tier::Working => 0,
        Tier::Episodic => 1,
        Tier::Semantic => 2,
        Tier::Procedural => 3,
        Tier::Archival => 4,
    }
}

impl MemExt {
    /// Open the sidecar for an estate store path. `:memory:` → an in-memory sidecar.
    pub fn open(estate_spec_path: &str) -> EResult<Self> {
        let conn = if estate_spec_path == ":memory:" || estate_spec_path.is_empty() {
            Connection::open_in_memory().map_err(map_err)?
        } else {
            Connection::open(format!("{estate_spec_path}.memext")).map_err(map_err)?
        };
        conn.execute_batch(SCHEMA).map_err(map_err)?;
        migrate(&conn)?; // NFR-8/AC-11: forward-migrate + stamp the schema version
        Ok(Self { conn })
    }

    /// The sidecar's persisted schema version (NFR-8).
    pub fn schema_version(&self) -> EResult<i64> {
        self.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(map_err)
    }

    /// Upsert the index row for a memory (called on capture/reinforce/consolidation writes).
    pub fn upsert(&self, m: &Memory) -> EResult<()> {
        self.conn
            .execute(
                "INSERT INTO memory_node_ext
                   (node_id, mem_kind, tier, scope, created_at, last_access, access_count, reinforce_pos, reinforce_total)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(node_id) DO UPDATE SET
                   mem_kind=excluded.mem_kind, tier=excluded.tier, scope=excluded.scope,
                   created_at=excluded.created_at, last_access=excluded.last_access,
                   access_count=excluded.access_count, reinforce_pos=excluded.reinforce_pos,
                   reinforce_total=excluded.reinforce_total",
                params![
                    m.symbol().0,
                    m.kind.as_str(),
                    tier_code(m.tier),
                    m.scope.as_path(),
                    m.created_at,
                    m.last_access,
                    m.access_count as i64,
                    m.reinforce_pos as i64,
                    m.reinforce_total as i64,
                ],
            )
            .map_err(map_err)?;
        Ok(())
    }

    /// Node-ids of memories in `tier` (indexed). For `reflect` over episodic memories.
    pub fn ids_in_tier(&self, tier: Tier) -> EResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT node_id FROM memory_node_ext WHERE tier = ?1")
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![tier_code(tier)], |r| r.get::<_, String>(0))
            .map_err(map_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_err)
    }

    /// Node-ids of facts with `reinforce_total >= min_total` (indexed by `idx_mem_ext_kind`).
    pub fn fact_ids_reinforced(&self, min_total: u64) -> EResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT node_id FROM memory_node_ext WHERE mem_kind = 'fact' AND reinforce_total >= ?1",
            )
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![min_total as i64], |r| r.get::<_, String>(0))
            .map_err(map_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_err)
    }

    /// Node-ids of aged candidates in `tiers` created before `created_before` (uses `idx_mem_ext`).
    /// Salience is computed in Rust on this bounded set by the caller (avoids stale snapshots).
    pub fn aged_ids(&self, tiers: &[Tier], created_at_or_before: i64) -> EResult<Vec<String>> {
        if tiers.is_empty() {
            return Ok(Vec::new()); // guard: empty IN () is invalid SQL
        }
        let codes: Vec<String> = tiers.iter().map(|t| tier_code(*t).to_string()).collect();
        // `<=` matches the caller's inclusive Rust predicate `(now - created_at) >= max_age_secs`
        // (i.e. created_at <= now - max_age_secs), so a memory exactly at the cutoff isn't dropped.
        let sql = format!(
            "SELECT node_id FROM memory_node_ext WHERE tier IN ({}) AND created_at <= ?1",
            codes.join(",")
        );
        let mut stmt = self.conn.prepare(&sql).map_err(map_err)?;
        let rows = stmt
            .query_map(params![created_at_or_before], |r| r.get::<_, String>(0))
            .map_err(map_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(map_err)
    }

    pub fn count(&self) -> EResult<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM memory_node_ext", [], |r| r.get(0))
            .map_err(map_err)?;
        Ok(n as usize)
    }

    /// Remove rows for the given node-ids (erasure/purge). Used by PR-12 erasure + tests.
    #[allow(dead_code)]
    pub fn remove(&self, ids: &[String]) -> EResult<()> {
        for id in ids {
            self.conn
                .execute(
                    "DELETE FROM memory_node_ext WHERE node_id = ?1",
                    params![id],
                )
                .map_err(map_err)?;
        }
        Ok(())
    }

    /// Rebuild the index from the authoritative memories (crash-recovery / first build).
    pub fn rebuild(&self, all: &[Memory]) -> EResult<()> {
        self.conn
            .execute("DELETE FROM memory_node_ext", [])
            .map_err(map_err)?;
        for m in all {
            self.upsert(m)?;
        }
        Ok(())
    }

    /// Order-independent fingerprint of the index over the drift-prone fields. Compared against
    /// [`memory_fingerprint`] in [`reconcile`] to catch IN-PLACE drift (e.g. a `reinforce` whose
    /// estate write committed but whose ext write failed) — count-equality alone cannot.
    pub fn fingerprint(&self) -> EResult<u64> {
        let mut stmt = self
            .conn
            .prepare("SELECT node_id, tier, reinforce_total, last_access, access_count FROM memory_node_ext")
            .map_err(map_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(map_err)?;
        let mut acc = 0u64;
        for row in rows {
            let (id, t, rt, la, ac) = row.map_err(map_err)?;
            acc = acc.wrapping_add(row_hash(&id, t, rt, la, ac));
        }
        Ok(acc)
    }

    /// Reconcile against the authoritative memories; rebuild on count OR content divergence (called
    /// at open — crash-recovery for cross-file non-atomic writes).
    pub fn reconcile(&self, authoritative: &[Memory]) -> EResult<()> {
        if self.count()? != authoritative.len()
            || self.fingerprint()? != memory_fingerprint(authoritative)
        {
            self.rebuild(authoritative)?;
        }
        Ok(())
    }

    /// `EXPLAIN QUERY PLAN` for the aged-candidates query — used by the L1 EXPLAIN gate test.
    #[allow(dead_code)]
    pub fn explain_aged(&self) -> EResult<String> {
        let mut stmt = self
            .conn
            .prepare("EXPLAIN QUERY PLAN SELECT node_id FROM memory_node_ext WHERE tier IN (1,2) AND created_at < 0")
            .map_err(map_err)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .map_err(map_err)?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(map_err)?
            .join(" | "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_memory_core::{MemKind, Memory, Scope, Tier};

    fn mem(kind: MemKind, tier: Tier, content: &str, created: i64) -> Memory {
        Memory::new(kind, tier, Scope::root(), content, created)
    }

    #[test]
    fn explain_gate_uses_the_index_not_a_scan() {
        // L1 EXPLAIN gate (PR-1): the aged-candidates query must use idx_mem_ext, not SCAN.
        let ext = MemExt::open(":memory:").unwrap();
        for i in 0..200 {
            ext.upsert(&mem(MemKind::Episode, Tier::Episodic, &format!("e{i}"), i))
                .unwrap();
        }
        let plan = ext.explain_aged().unwrap().to_lowercase();
        assert!(
            plan.contains("idx_mem_ext"),
            "aged query must use the index; plan: {plan}"
        );
        assert!(
            !plan.contains("scan memory_node_ext"),
            "must not full-scan; plan: {plan}"
        );
    }

    #[test]
    fn candidate_queries_are_bounded_and_correct() {
        let ext = MemExt::open(":memory:").unwrap();
        ext.upsert(&mem(MemKind::Episode, Tier::Episodic, "old", 0))
            .unwrap();
        ext.upsert(&mem(MemKind::Episode, Tier::Episodic, "new", 1000))
            .unwrap();
        let mut f = mem(MemKind::Fact, Tier::Semantic, "reinforced", 0);
        f.reinforce_total = 9;
        ext.upsert(&f).unwrap();

        assert_eq!(ext.ids_in_tier(Tier::Episodic).unwrap().len(), 2);
        assert_eq!(ext.fact_ids_reinforced(5).unwrap().len(), 1);
        assert_eq!(ext.fact_ids_reinforced(20).unwrap().len(), 0);
        // aged: created_at < 500 → only "old"
        assert_eq!(
            ext.aged_ids(&[Tier::Episodic, Tier::Semantic], 500)
                .unwrap()
                .len(),
            2
        ); // old episode + fact@0
    }

    #[test]
    fn reconcile_detects_inplace_field_drift() {
        // Same row COUNT but different content (e.g. a reinforce whose ext write was lost) → the
        // fingerprint must differ so reconcile rebuilds. Count-only reconcile would miss this.
        let ext = MemExt::open(":memory:").unwrap();
        let mut m = mem(MemKind::Fact, Tier::Semantic, "billing", 0);
        ext.upsert(&m).unwrap(); // ext has reinforce_total=0
        m.reinforce_total = 7; // authoritative advanced, ext stale (same id, same count)
        let authoritative = vec![m.clone()];
        assert_eq!(ext.count().unwrap(), authoritative.len()); // counts match...
        assert_ne!(
            ext.fingerprint().unwrap(),
            memory_fingerprint(&authoritative)
        ); // ...content differs
        ext.reconcile(&authoritative).unwrap();
        assert_eq!(
            ext.fingerprint().unwrap(),
            memory_fingerprint(&authoritative)
        ); // rebuilt
    }

    #[test]
    fn schema_version_and_migration_preserve_data() {
        // NFR-8/AC-11: version is stamped; a forward migration preserves existing rows.
        let base = std::env::temp_dir().join(format!("wmem-mig-{}", std::process::id()));
        let path = base.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(format!("{path}.memext"));
        {
            let ext = MemExt::open(&path).unwrap();
            assert_eq!(ext.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
            ext.upsert(&mem(MemKind::Fact, Tier::Semantic, "persist me", 0))
                .unwrap();
            ext.conn.execute_batch("PRAGMA user_version = 0;").unwrap(); // simulate a pre-versioned DB
        }
        let ext2 = MemExt::open(&path).unwrap(); // reopen → migrate() runs
        assert_eq!(
            ext2.schema_version().unwrap(),
            CURRENT_SCHEMA_VERSION,
            "forward-migrated"
        );
        assert_eq!(ext2.count().unwrap(), 1, "data preserved across migration");
        let _ = std::fs::remove_file(format!("{path}.memext"));
    }

    #[test]
    fn empty_tiers_is_safe() {
        let ext = MemExt::open(":memory:").unwrap();
        assert!(ext.aged_ids(&[], 1000).unwrap().is_empty()); // no invalid `IN ()` SQL
    }

    #[test]
    fn remove_and_reconcile() {
        let ext = MemExt::open(":memory:").unwrap();
        let m = mem(MemKind::Fact, Tier::Semantic, "x", 0);
        ext.upsert(&m).unwrap();
        assert_eq!(ext.count().unwrap(), 1);
        ext.remove(std::slice::from_ref(&m.symbol().0)).unwrap();
        assert_eq!(ext.count().unwrap(), 0);
        // reconcile rebuilds from authoritative set when diverged.
        let all = vec![
            mem(MemKind::Episode, Tier::Episodic, "a", 0),
            mem(MemKind::Fact, Tier::Semantic, "b", 0),
        ];
        ext.reconcile(&all).unwrap();
        assert_eq!(ext.count().unwrap(), 2);
    }
}
