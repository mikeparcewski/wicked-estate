# Recon: `correspond` command — existing infrastructure audit

**Goal**: implement `wicked-estate correspond --db-a A.db --db-b B.db [--json]` that produces
a ranked list of `{a: SymbolId, b: SymbolId, score, basis}` pairs.

---

## 1. `crates/wicked-estate/src/main.rs`

### `cross-graph` command (lines 483–553)

- Opens **N DBs** by collecting `--db` flags into `Vec<String> db_paths`.
- The last `--db` value is also stored in `db` (single-db default).
- Calls `wicked_estate::cross_graph_search(&db_paths, name)` → `(Vec<(String, Node)>, Vec<Error>)`.
- Calls `wicked_estate::cross_graph_blast_radius(&db_paths, name, depth)`.
- Pattern: iterate `db_paths`, open each store independently, query per-store, union results.
- **Directly reusable**: the `--db-a`/`--db-b` flag pattern can piggyback on the same `db_paths` accumulator or use two dedicated flags. The two-store open pattern is identical.

### `semantic` command (lines 429–475)

- Opens two handles to the **same** DB: one `&dyn GraphRead` for node resolution, one `SqliteStore` for vector ANN (`nearest`).
- Uses `SemanticSearch::new(embedder, vec_store)` → `RetrievalTool`.
- The `invoke(&*graph_store, &json!({"query": ..., "k": ...}))` returns `matches[{symbol, name, kind, file, line, similarity}]`.
- **Reusable**: for `correspond`, embed the name/signature of each symbol in A, call `nearest` against B's embedding table. Two `SqliteStore` instances (one per DB) opened separately.

### `clusters` command (lines 756–786)

- Calls `wicked_estate_rank::detect_communities(store.as_ref(), min_size, false)`.
- Uses union-find over `CALLS`/`IMPORTS` edges; groups `SymbolId`s into `Vec<Vec<SymbolId>>`.
- Not directly useful for cross-DB correspondence, but shows the pattern for bulk-node graph algorithms.

### `nodes` command (lines 1141–1183)

- Calls `store.nodes_by_kind(&kind)` — returns ALL non-file nodes when `kind` is `""` or `"all"`.
- **This is the bulk-fetch path** you need to iterate all nodes in one DB. Returns `Vec<Node>`.

### How `open_store` / `open_store_ext` are used

| Function | Returns | Used when |
|---|---|---|
| `open_store(&db)` | `Box<dyn GraphRead>` | Read-only queries (query, blast-radius, source) |
| `open_store_ext(&db)` | `Box<dyn GraphStoreMutExt>` | Commands that may write (stats, rank, context) |
| `SqliteStore::open(&db)` | `SqliteStore` (concrete) | When you need inherent methods: `nearest`, `nodes_by_kind`, `nodes_in_file`, `annotate_node`, etc. |

For `correspond`, open **both** DBs as `SqliteStore::open(...)` — you need `all_nodes`/`nodes_by_kind` (via `GraphRead`) AND `nearest`/`embedding` (inherent) from each.

---

## 2. `crates/wicked-estate-store/src/sqlite.rs`

### Bulk node iteration — **YES, it exists**

| Method | Signature | Returns | Notes |
|---|---|---|---|
| `all_nodes()` | `&self` | `Result<Vec<Node>>` | **On `GraphRead` trait**. Full table scan `SELECT data FROM nodes`. Includes File nodes. |
| `nodes_by_kind(kind)` | `&self, &str` | `Result<Vec<Node>>` | **Inherent on `SqliteStore`**. Pass `""` or `"all"` to get all non-file nodes. Pass `"function"` etc. to filter. |
| `nodes_in_file(file)` | `&self, &str` | `Result<Vec<Node>>` | **Inherent**. By file path. Already used in `changed-since`. |

**Recommended for `correspond`**: `nodes_by_kind("")` on each store — returns all non-file nodes, avoids file-node noise.

### Embedding-based search

| Method | Signature | Returns | Notes |
|---|---|---|---|
| `set_embedding(symbol, vec)` | `&mut self` | `Result<()>` | Inherent; stores L-E f32 blob |
| `embedding(symbol)` | `&self` | `Result<Option<Vec<f32>>>` | Inherent; retrieves one vector |
| `nearest(query, k)` | `&self, &[f32], usize` | `Result<Vec<(SymbolId, f32)>>` | **Brute-force cosine over ALL stored embeddings**. Inherent. Returns `(SymbolId, cosine_sim)` sorted descending. |

**Critical**: `nearest` scans the **same store**. There is **no cross-store nearest**. To query "what in B is most similar to symbol X from A":
1. Get the embedding vector for X from store A: `store_a.embedding(&x_sym)`.
2. Call `store_b.nearest(&vec, k)` to find B's nearest neighbours.

Both stores must have been indexed with `--embeddings`. Dimension mismatch is silently skipped in `nearest`.

### FTS5 search path

`find_symbols` with `SymbolQuery { text: Some(...) }` hits `nodes_fts MATCH ?` ordered by BM25. Works on a single-store basis only — no cross-store FTS.

---

## 3. `crates/wicked-estate-retrieve/src/lib.rs`

### Tools relevant to `correspond`

| Component | What it does | Relevance |
|---|---|---|
| `semantic_search<S: VectorStore>` (free fn, line 1442) | Embeds query text, calls `store.nearest`, resolves SymbolIds to Nodes via `graph.get_node`. Cross-store is possible: pass `store_a` for graph and `store_b` as `VectorStore`. | **Directly usable** for one direction of cross-DB embedding matching. |
| `reciprocal_rank_fusion(lists, k)` (line 689) | Fuses N ranked `Vec<SymbolId>` lists via RRF. Returns `Vec<(SymbolId, f64)>` sorted descending. | Use to combine name-match score + embedding-cosine score + kind-match signal into a single score per pair. |
| `hybrid_search(name, graph, semantic, k)` (line 1473) | Thin wrapper around RRF over 3 lists. | Use to blend scoring signals. |
| `VectorStore` trait | Bridge trait; `SqliteStore` and `MemStore` both impl it. | Pass a concrete `SqliteStore` as `&dyn VectorStore`. |
| `Embedder` trait + `HashEmbedder` / `FastEmbedder` | Embed text → `Vec<f32>`. `wicked_estate::default_embedder()` picks the right one. | Use at query time (embed symbol A's name+sig, search B). |
| `SemanticSearch` (RetrievalTool) | Holds embedder + `Mutex<Box<dyn VectorStore>>`. | Can be adapted, but for `correspond` you want the raw `semantic_search` free function. |

### What `reciprocal_rank_fusion` buys you

For each symbol `a` in DB A, you can build:
- **name list**: top-k from `store_b.find_symbols(SymbolQuery { text: Some(a.name) })`.
- **embedding list**: top-k from `store_b.nearest(store_a.embedding(&a.symbol), k)` (if embeddings exist).
- **kind-filtered subset**: can be applied as a post-filter.

RRF fuses these into one ranked list of B candidates per A symbol. The RRF score becomes `score` in the output struct.

---

## 4. `crates/wicked-estate-core/src/traits.rs`

### `GraphRead` trait methods useful for `correspond`

| Method | Signature | Returns | Notes |
|---|---|---|---|
| `all_nodes()` | `&self` | `Result<Vec<Node>>` | All nodes including File nodes. For `correspond` prefer `nodes_by_kind("")` to skip file nodes. |
| `all_edges()` | `&self` | `Result<Vec<Edge>>` | Not needed for correspond. |
| `find_symbols(query)` | `&self, &SymbolQuery` | `Result<Vec<Node>>` | FTS or exact search within one store. Use for name-based signal. |
| `get_node(id)` | `&self, &SymbolId` | `Result<Option<Node>>` | Resolve a SymbolId to a full Node. Needed to get kind/sig/file after ANN. |

The `GraphRead` trait is **object-safe** and satisfied by both `SqliteStore` and `MemStore`. Both can be passed as `&dyn GraphRead`.

---

## Summary answers

### Is there a way to iterate ALL nodes in a DB?

Yes — two paths:
1. `GraphRead::all_nodes()` — trait method, returns `Vec<Node>` including File nodes. Available on `&dyn GraphRead`.
2. `SqliteStore::nodes_by_kind("")` — inherent method, returns all **non-file** nodes. Requires concrete `SqliteStore`.

For `correspond`, use `nodes_by_kind("")` on each store to get the matchable symbol universe.

### Does the store support embedding lookup across two separate store instances?

Not natively — `nearest` scans only its own `embeddings` table. But you can **bridge it manually**:
```rust
let vec_a: Vec<f32> = store_a.embedding(&sym_a)?.unwrap_or_default();
let hits_in_b: Vec<(SymbolId, f32)> = store_b.nearest(&vec_a, k)?;
```
This is the correct pattern. The free function `semantic_search` already does this when you pass `store_a` as the graph source and `store_b` as the `VectorStore`.

### What scoring signals are already available?

| Signal | Source | Type | Notes |
|---|---|---|---|
| **Name match** | `Node::name` | `String` | Exact string equality or FTS BM25 via `find_symbols` |
| **Kind match** | `Node::kind` | `NodeKind` enum | `Function`, `Method`, `Class`, `Struct`, etc. |
| **File path similarity** | `Node::location.file` | `String` | Path segment overlap (manual) |
| **Signature text** | `Node::signature` | `Option<String>` | Indexed in FTS5 (name+signature searched together) |
| **Doc comment** | `Node::doc` | `Option<String>` | Indexed in FTS5 |
| **Embedding cosine** | `SqliteStore::nearest` or `SqliteStore::embedding` | `f32` in [-1,1] | Requires `--embeddings` at index time |
| **Language** | `Node::language` | `Language` | Can filter correspond to same-language pairs only |
| **RRF fused score** | `reciprocal_rank_fusion` | `f64` | Combines name + embedding lists |

### Basis string

The `basis` field in the output `{a, b, score, basis}` can be constructed as a concatenation of which signals fired: e.g., `"name+kind"`, `"embedding:0.91"`, `"name+embedding"`.

---

## Recommended implementation sketch

```rust
"correspond" => {
    // Parse --db-a and --db-b from positional or dedicated flags.
    let db_a = ...; // path to A.db
    let db_b = ...; // path to B.db
    let json_out = positional.iter().any(|a| a == "--json");

    let store_a = SqliteStore::open(&db_a)?;
    let store_b = SqliteStore::open(&db_b)?;

    // 1. Get all non-file nodes from A.
    let nodes_a = store_a.nodes_by_kind("").map_err(to_any)?;

    let embedder = wicked_estate::default_embedder();
    let has_embeddings = store_a.capabilities().vector_search
        && store_b.capabilities().vector_search;

    let mut pairs: Vec<CorrespondPair> = Vec::new();

    for node_a in &nodes_a {
        let mut name_list: Vec<SymbolId> = store_b
            .find_symbols(&SymbolQuery {
                text: Some(node_a.name.clone()),
                limit: Some(20),
                ..Default::default()
            })?
            .into_iter()
            .map(|n| n.symbol)
            .collect();

        let mut emb_list: Vec<SymbolId> = if has_embeddings {
            if let Some(vec) = store_a.embedding(&node_a.symbol)? {
                store_b.nearest(&vec, 20)?
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect()
            } else { vec![] }
        } else { vec![] };

        // Optionally filter both lists to same kind.

        let fused = reciprocal_rank_fusion(&[name_list, emb_list], 60.0);
        for (sym_b, score) in fused.into_iter().take(5) {
            let basis = "name+embedding"; // refine based on which lists fired
            pairs.push(CorrespondPair { a: node_a.symbol.clone(), b: sym_b, score, basis });
        }
    }

    // Sort by score descending, deduplicate, emit.
}
```

---

## Not-yet-done / gaps

- `nodes_by_kind` is an inherent method on `SqliteStore` only — cannot be called through `Box<dyn GraphRead>`. You must open as `SqliteStore::open(...)`. Alternative: use `all_nodes()` on the trait and filter `kind != NodeKind::File` in Rust.
- Cross-DB embedding lookup is manual — no single function does it. The `semantic_search` free function is close but expects one `VectorStore` and one `GraphRead`; pass `store_b` as both (different handles opened from same path).
- No existing `CorrespondPair` struct or `correspond` scoring logic exists — this is new.
- If embeddings are absent from one or both DBs, the command must degrade gracefully to name-only matching (RRF with a single list, or direct exact/FTS matching).
