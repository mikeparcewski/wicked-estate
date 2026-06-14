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
CREATE TABLE IF NOT EXISTS symbols (
  sid INTEGER PRIMARY KEY AUTOINCREMENT,
  sym TEXT UNIQUE NOT NULL   -- the UNIQUE constraint implies an index; no separate idx needed
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
  requirement_validated   INTEGER NOT NULL DEFAULT 0 -- semantic: match validated as true (0/1)
);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(file);

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

CREATE TABLE IF NOT EXISTS edges (
  source     INTEGER NOT NULL,   -- sid FK → symbols.sid (dependent, edge-direction invariant)
  target     INTEGER NOT NULL,   -- sid FK → symbols.sid (dependency)
  kind       TEXT NOT NULL,      -- serialized EdgeKind (e.g. "calls")
  confidence REAL NOT NULL,
  file       TEXT NOT NULL DEFAULT '', -- repo-relative source file of the edge site (Wave 2.6)
  data       TEXT NOT NULL,      -- full Edge as JSON (still carries string SymbolIds for round-trip)
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
-- Disk-footprint design (W11 slim): columns are typed minimal — from_sym, raw_name, kind,
-- file, line.  The full span and hints are intentionally NOT persisted; the in-memory
-- resolve pass owns those.  This is a deliberate ~8× disk reduction vs the old `data` JSON
-- blob (~1.2KB/row).  Coverage fidelity is preserved: from_sym + raw_name + kind + file +
-- line are sufficient for honest blast-radius and resolver queries.
CREATE TABLE IF NOT EXISTS unresolved_refs (
  id       INTEGER PRIMARY KEY,
  from_sym INTEGER NOT NULL,     -- sid FK → symbols.sid of the referencing symbol
  raw_name TEXT NOT NULL,        -- the written name at the call/import site
  kind     TEXT NOT NULL,        -- serialized EdgeKind (same encoding as edges.kind)
  file     TEXT NOT NULL DEFAULT '', -- repo-relative source file (Wave 2.6 incremental)
  line     INTEGER NOT NULL DEFAULT 0 -- start_line of the reference site
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
