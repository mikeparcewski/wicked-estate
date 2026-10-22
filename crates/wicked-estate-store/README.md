# wicked-estate-store

Graph storage backends for [wicked-estate](https://github.com/mikeparcewski/wicked-estate).

Implements the `GraphStore` trait: an in-memory reference store (`MemStore`) and the durable SQLite backend (FTS5 full-text + `sqlite-vec` vector search) that powers local-first indexing and retrieval. One file, no server.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.

### PostgreSQL deployment owner and runtime connection

With the `postgres` feature, `PostgresStore::open(url)` remains the explicit
initializer: it creates/migrates graph tables and initializes metadata. Run it
using the deployment owner, with a dedicated trusted schema in that login's
`search_path`. It records `meta.postgres_schema_version = 1`; an unknown version
is refused before schema changes. Existing unversioned databases require one
explicit owner initialization before the restricted connection is used.

`PostgresStore::connect_existing(url)` performs reads only. It requires the exact
supported schema version and valid graph-version metadata, reads the history
setting, and neither creates missing tables nor inserts metadata. A read-only
application login needs `USAGE` on the dedicated schema and `SELECT` on the graph
tables; it does not need ownership or schema `CREATE`. Mutating trait methods
remain subject to PostgreSQL permissions. This constructor does not grant access,
choose a tenant, validate external source provenance, or authorize cached content
for a platform caller. An unknown/missing schema version is an error; run the
appropriate explicit deployment migration rather than falling back to `open`.

`platform_retrieval_probe` in the umbrella crate demonstrates the public API
split (`index` initializes; `read` connects without migrations). The platform
consumer conformance records actual restricted-login PostgreSQL checks separately;
this example's process exit alone is not a production qualification claim.
