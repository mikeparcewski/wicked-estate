-- wicked_estate graph schema (ADR-001). Storage-agnostic model mapped to SQLite.
-- Nodes/edges keep a few indexed query columns plus the full record as JSON in `data`.

-- Wave 2.6: per-file content digests for incremental re-indexing. The path is the
-- repo-relative file path (same as node/edge/unresolved_refs `file` column). On a
-- re-index, any file whose stored digest matches the current xxh3 digest is skipped;
-- stale/deleted files are cleaned up via remove_file().
-- Wave 7: git_sha = hex(SHA1("blob " + byte_len + "\0" + content)) — the git blob id.
--         Computed and stored by set_file_content; NULL until content is stored.
CREATE TABLE IF NOT EXISTS files (
  path    TEXT PRIMARY KEY,
  digest  TEXT NOT NULL DEFAULT '',
  git_sha TEXT
);

-- Symbol-string intern table.  Every distinct SymbolId string is stored once here;
-- all other tables reference it by integer sid.  This cuts on-disk footprint for repos
-- where the same symbol string appears across nodes + edges + unresolved_refs rows.
-- The `sym` column carries a UNIQUE constraint so INSERT … ON CONFLICT DO NOTHING is safe.
-- `gen` (M8 / DoD-XA4) is the symbol's EPOCH: a monotonic live-node generation counter. The
-- intern row is append-only and survives remove_file (only the `nodes` row is deleted), so a
-- delete-then-re-add reuses the SAME sid. `gen` is bumped (in `upsert_nodes_inner`, NOT `intern`)
-- whenever a sid that has NO live `nodes` row gains one — i.e. a reuse-after-delete. A first-ever
-- node (including a symbol that previously existed only as an edge endpoint / unresolved-ref) stays
-- gen=0. `GraphRead::symbol_epoch` exposes the live symbol's current gen; cross-store about-arm
-- consumers stamp/validate xedge endpoints against it so a stale row never resolves to a live-wrong
-- node. DEFAULT 0 so existing DBs migrate (the idempotent ALTER TABLE in sqlite.rs adds the column).
CREATE TABLE IF NOT EXISTS symbols (
  sid      INTEGER PRIMARY KEY AUTOINCREMENT,
  sym      TEXT UNIQUE NOT NULL,  -- the UNIQUE constraint implies an index; no separate idx needed
  gen      INTEGER NOT NULL DEFAULT 0,  -- live-node epoch (M8/DoD-XA4): bumped on reuse-after-delete
  had_node INTEGER NOT NULL DEFAULT 0   -- sticky 1 once a node has EVER existed for this sid; the
                                        -- durable signal that distinguishes a reuse-after-delete
                                        -- (had_node=1, no live node) from a first-ever / edge-only
                                        -- symbol getting its first node (had_node=0, no bump).
);

CREATE TABLE IF NOT EXISTS nodes (
  symbol                  INTEGER PRIMARY KEY,  -- sid FK → symbols.sid (ADR-002 stable identity)
  name                    TEXT NOT NULL,
  kind                    TEXT NOT NULL,        -- serialized NodeKind (e.g. "function")
  language                TEXT NOT NULL,
  file                    TEXT NOT NULL DEFAULT '', -- repo-relative source file (Wave 2.6 incremental)
  data                    TEXT NOT NULL,        -- full Node as JSON (still carries string SymbolId for round-trip)
  description             TEXT,                -- semantic: what is this symbol? (Semantic linking)
  requirement             TEXT,                -- semantic: requirement this symbol matches/fulfils
  requirement_validated   INTEGER NOT NULL DEFAULT 0, -- semantic: match validated as true (0/1)
  scope                   TEXT NOT NULL DEFAULT ''  -- hierarchical ownership/partition path (root='')
);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(file);
CREATE INDEX IF NOT EXISTS idx_nodes_scope ON nodes(scope);

-- Multi-file symbol contributions (M4 / Option A — wicked-estate#152). ONE logical symbol may be
-- contributed by MORE THAN ONE file (a C/C++ header member prototype and its out-of-line `.cpp`
-- definition mint one SymbolId across two files — ADR-002 scheme 3, pinned by
-- `cpp_member_proto_def_cross_file_single_id_hazard`). This table records every (symbol, file)
-- contribution: `data` is the full Node JSON exactly as THAT file's extraction produced it, and
-- `is_def` is 0 when the record is a declaration contribution (`metadata.is_declaration` truthy),
-- 1 otherwise. The `nodes` row is a DERIVED projection: always equal to the PREFERRED contribution
-- (`ORDER BY is_def DESC, file ASC LIMIT 1` — definition wins; lexicographic file tiebreak), never
-- last-write-wins. `remove_file` deletes the removed file's contribution rows and re-homes a node
-- with surviving contributions to the new preferred record; only a contribution-less node is
-- deleted. Rows retire ONLY through remove_file (the incremental indexer removes a changed file
-- before re-upserting it, so a symbol that stops being contributed by a file loses that row).
-- Existing DBs are backfilled by the idempotent migration in sqlite.rs (one definition-preference
-- row per current node, seeded from nodes.file/nodes.data).
CREATE TABLE IF NOT EXISTS node_files (
  symbol INTEGER NOT NULL,           -- sid FK → symbols.sid
  file   TEXT    NOT NULL,           -- repo-relative contributing file
  is_def INTEGER NOT NULL DEFAULT 1, -- 1 = definition contribution, 0 = declaration
  data   TEXT    NOT NULL,           -- full Node JSON as this file's extraction produced it
  PRIMARY KEY (symbol, file)
);
CREATE INDEX IF NOT EXISTS idx_node_files_file ON node_files(file);

-- W5.1: FTS5 virtual table for BM25 full-text search over name/signature/doc.
-- `symbol` stores the string SymbolId (TEXT) so the join-back to nodes goes through
-- symbols.sym → symbols.sid → nodes.symbol (integer).  Kept as TEXT here because FTS5
-- virtual tables cannot expose an integer rowid as a regular column for joins.
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
  symbol UNINDEXED,   -- string SymbolId used for join-back via symbols table; not tokenised
  name,               -- simple name
  signature,          -- optional type/parameter signature
  doc                 -- optional doc-comment
);

-- evidence_count (brain-consolidation): promoted audit counter for how many times a relationship
-- has been confirmed/contradicted. The authoritative value is the first-class `Edge.evidence_count`
-- field, which round-trips inside `data` like every other Edge field; this column mirrors it for
-- SQL-queryable auditing/lints (written in lockstep by upsert_edges, so it can never drift).
-- DEFAULT 0 backfills every pre-existing edge (code edges never confirm → stay 0).
-- New DBs get the column here; existing DBs via the idempotent ALTER TABLE migration in sqlite.rs.
CREATE TABLE IF NOT EXISTS edges (
  source         INTEGER NOT NULL,   -- sid FK → symbols.sid (dependent, edge-direction invariant)
  target         INTEGER NOT NULL,   -- sid FK → symbols.sid (dependency)
  kind           TEXT NOT NULL,      -- serialized EdgeKind (e.g. "calls")
  confidence     REAL NOT NULL,
  file           TEXT NOT NULL DEFAULT '', -- repo-relative source file of the edge site (Wave 2.6)
  data           TEXT NOT NULL,      -- full Edge as JSON (still carries string SymbolIds for round-trip)
  evidence_count INTEGER NOT NULL DEFAULT 0, -- queryable mirror of the first-class Edge.evidence_count field (brain consolidation)
  PRIMARY KEY (source, target, kind)
);
-- target index powers blast-radius (Dependents); source index powers Dependencies.
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
CREATE INDEX IF NOT EXISTS idx_edges_file ON edges(file);

-- Unresolved references: calls/imports the resolver could NOT bind to a target symbol.
-- Kept to power honest blast-radius coverage (never silently claim "no dependents" when
-- calls to that name went unresolved). raw_name is indexed for exact-name lookup.
--
-- Disk-footprint design (W11 slim, amended by admissibility F-B): columns are typed minimal —
-- from_sym, raw_name, kind, file, line, start_byte, end_byte.  Hints and the remaining span
-- fields (cols, end_line) are intentionally NOT persisted; the in-memory resolve pass owns
-- those.  Still a deliberate ~8× disk reduction vs the old `data` JSON blob (~1.2KB/row).
-- The byte columns make site identity EXACT: two same-line sites (`q(); q();`) carry distinct
-- start_byte, so duplicate-site proofs are pure SQL instead of on-disk adjudication (the
-- closure's 1215 line-level "duplicate" groups were all real distinct same-line sites).
-- start_byte = end_byte = 0 means unknown/synthetic (Span::ZERO refs — RulesBridge,
-- extra-edge — and rows written before these columns existed carry it legitimately).
CREATE TABLE IF NOT EXISTS unresolved_refs (
  id       INTEGER PRIMARY KEY,
  from_sym INTEGER NOT NULL,     -- sid FK → symbols.sid of the referencing symbol
  raw_name TEXT NOT NULL,        -- the written name at the call/import site
  kind     TEXT NOT NULL,        -- serialized EdgeKind (same encoding as edges.kind)
  file     TEXT NOT NULL DEFAULT '', -- repo-relative source file (Wave 2.6 incremental)
  line     INTEGER NOT NULL DEFAULT 0, -- start_line of the reference site
  start_byte INTEGER NOT NULL DEFAULT 0, -- byte offset of the site start (0 = unknown/synthetic)
  end_byte   INTEGER NOT NULL DEFAULT 0  -- byte offset of the site end   (0 = unknown/synthetic)
);
CREATE INDEX IF NOT EXISTS idx_unresolved_refs_name ON unresolved_refs(raw_name);
CREATE INDEX IF NOT EXISTS idx_unresolved_refs_file ON unresolved_refs(file);

-- W11.1: content-addressed source-text store.
-- Keyed by git_sha (the git blob id), NOT by file path, so identical content deduplicates.
-- INSERT OR IGNORE into content + set files.git_sha; resolve text via files JOIN content.
-- Orphan rows (git_sha not referenced by files or edge_history) are reclaimed by compact().
-- W11 slim: payload stored as zstd-compressed BLOB instead of plain TEXT (~4× compression).
-- Read path: decode_all(&blob[..]) → UTF-8 string; callers see plain text (transparent).
CREATE TABLE IF NOT EXISTS content (
  git_sha TEXT PRIMARY KEY,
  blob    BLOB NOT NULL
);

-- W11.2: versioned query cache (prior art versioned cache-port pattern).
-- cache rows are keyed by (key, version); cache_get returns a value only when the stored
-- version matches the current graph_version held in meta. bump_version increments graph_version
-- so all prior cache entries become stale without requiring a DELETE sweep.
CREATE TABLE IF NOT EXISTS cache (
  key     TEXT    NOT NULL,
  version INTEGER NOT NULL,
  value   TEXT    NOT NULL,
  PRIMARY KEY (key, version)
);

-- Single-row meta table: k='graph_version' v='0' (integer stored as TEXT for simplicity).
CREATE TABLE IF NOT EXISTS meta (
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL
);
INSERT OR IGNORE INTO meta(k, v) VALUES('graph_version', '0');

-- W5.2: per-symbol embedding vectors for semantic / ANN search.
-- vec is the embedding as little-endian IEEE-754 f32 bytes (dim * 4 bytes).
-- dim is stored so callers can detect dimension mismatches at runtime.
-- An ANN index (e.g. HNSW) is a future optimisation; brute-force cosine over this
-- table is sufficient for local-first scale (tens of thousands of symbols).
CREATE TABLE IF NOT EXISTS embeddings (
  symbol TEXT PRIMARY KEY,  -- the string SymbolId (resolves via symbols.sym → symbols.sid → nodes.symbol)
  dim    INTEGER NOT NULL,  -- embedding dimensionality
  vec    BLOB    NOT NULL   -- little-endian f32 bytes, length = dim * 4
);

-- W7.1: reactive change log (file-granularity deltas for subscription).
-- `op` is "upsert" or "remove". `ts` is Unix seconds at insert time.
-- changes_since(cursor) returns rows WHERE seq > cursor ORDER BY seq ASC LIMIT 10_000.
CREATE TABLE IF NOT EXISTS changes (
  seq    INTEGER PRIMARY KEY AUTOINCREMENT,
  op     TEXT    NOT NULL,
  target TEXT    NOT NULL,
  ts     INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- W7: read-only edge history log.
-- Populated by remove_file() BEFORE the live edges are deleted, so we preserve
-- what each prior file version contributed. Never traversed — pure provenance.
-- Retention: compact() keeps the newest 20 rows per file, deletes older ones.
CREATE TABLE IF NOT EXISTS edge_history (
  archived_seq INTEGER PRIMARY KEY AUTOINCREMENT,
  git_sha      TEXT    NOT NULL DEFAULT '',
  file         TEXT    NOT NULL,
  edge_json    TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_edge_history_file ON edge_history(file);

-- Annotation store: external agents/tools/humans tag any indexed symbol with typed metadata.
-- node_sym is an sid FK → symbols.sid (same integer PK used by nodes.symbol).
-- key/value are arbitrary strings. confidence defaults 1.0, provenance/author default ''.
-- `type` is a plain string discriminator (NO enum): a known convention (note/assumption/
-- observation/comment/question/community) OR an arbitrary custom type — stored/queried
-- identically. Defaults to 'note' so legacy/untyped rows read back as a note. New DBs get the
-- column from here; existing DBs get it via the idempotent ALTER TABLE migration in sqlite.rs.
-- Evidence envelope (additive, backward-compatible): `source_type` (what KIND of source backed the
-- fact — code/config/sme-answer/static-analysis/…), `extraction_method` (by what method —
-- tool+version or 'manual'), `last_verified` (freshness clock, Unix-seconds; distinct from `ts`
-- which is write-time; 0 = never verified). Defaults match the struct ('unspecified' / 'manual' / 0)
-- so legacy rows backfill on read. New DBs get them here; existing DBs via the ALTER TABLE migration.
CREATE TABLE IF NOT EXISTS annotations (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  node_sym          INTEGER NOT NULL,
  key               TEXT NOT NULL,
  value             TEXT NOT NULL,
  confidence        REAL    NOT NULL DEFAULT 1.0,
  provenance        TEXT    NOT NULL DEFAULT '',
  author            TEXT    NOT NULL DEFAULT '',
  ts                INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
  type              TEXT    NOT NULL DEFAULT 'note',
  source_type       TEXT    NOT NULL DEFAULT 'unspecified',
  extraction_method TEXT    NOT NULL DEFAULT 'manual',
  last_verified     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_annotations_node ON annotations(node_sym);
CREATE INDEX IF NOT EXISTS idx_annotations_key  ON annotations(key);
CREATE INDEX IF NOT EXISTS idx_annotations_type ON annotations(type);
CREATE INDEX IF NOT EXISTS idx_annotations_last_verified ON annotations(last_verified);

-- Access telemetry (brain-consolidation): per-item access log. One row per (item, session, time)
-- an item was surfaced by search/recall. `item_id` is a stable identity string — a knowledge node
-- SymbolId for the knowledge store, or any node/document id. Aggregated into an access-count /
-- session-diversity ranking boost by consumers. This is the estate destination for wicked-brain's
-- `access_log` signal. Pure telemetry: no FK into nodes (an item may be logged then removed).
CREATE TABLE IF NOT EXISTS access_log (
  item_id     TEXT    NOT NULL,
  session_id  TEXT    NOT NULL,
  accessed_at INTEGER NOT NULL   -- epoch millis (matches the brain source clock)
);
CREATE INDEX IF NOT EXISTS idx_access_item    ON access_log(item_id);
CREATE INDEX IF NOT EXISTS idx_access_session ON access_log(session_id);

-- Search-miss log (brain-consolidation): failed / empty-result queries, the input to synonym
-- suggestion + gap-hunting. session_id is nullable (a miss may be logged outside a session).
-- This is the estate destination for wicked-brain's `search_misses` signal (and the persisted form
-- the knowledge engine's in-memory `RecallMiss` sidecar anticipated).
CREATE TABLE IF NOT EXISTS search_misses (
  query       TEXT    NOT NULL,
  searched_at INTEGER NOT NULL, -- epoch millis
  session_id  TEXT
);
CREATE INDEX IF NOT EXISTS idx_search_misses_time ON search_misses(searched_at);
