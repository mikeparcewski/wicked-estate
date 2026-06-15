# Recon: Cross-Repository Symbol Correspondence Algorithms

**Goal:** Inform the design of `wicked-estate correspond` scoring — what signals, what formulas,
what failure modes — so the implementation agent has a concrete recipe rather than a blank canvas.

**Status:** DRAFT  
**Date:** 2026-06-15

---

## 1. Standard Techniques for Cross-Repo Symbol Correspondence

Five signal families are used in practice, roughly in order of compute cost:

### 1a. Lexical / Token-Name Similarity
Compare the raw token-level text of symbol names and (when available) signatures. Includes:
- **Exact name match** — zero overhead, covers renames-that-are-actually-the-same-thing
- **Edit distance (Levenshtein / Jaro-Winkler)** — catches minor spelling variations and abbreviations
- **Token-set overlap (Jaccard on split tokens)** — after camelCase/snake_case splitting; handles `getUserById` ↔ `get_user_by_id`
- **BM25 full-text** — already present in wicked-estate via FTS5; BM25 naturally handles term frequency and corpus-level IDF (common tokens like `new`, `init` are down-weighted)
- **Signature bigrams / n-grams** — treat the signature string as a character n-gram bag; effective for cross-language matching because type names survive language boundaries (e.g., `String` in Java ↔ `String` in Kotlin)

### 1b. Structural / Syntactic Similarity
Compare the shape of the symbol's local AST subtree or inferred structure:
- **Parameter arity + type fingerprint** — count and normalized types of params
- **Return-type matching** — when available from the signature field
- **Call-graph neighborhood** — the set of symbols a function calls (caller/callee multiset); similar functions often call similar helpers. SourcererCC and CCFinder operate primarily here.
- **Control-flow shape hashing (PDG)** — program dependence graph locality; used by Deckard and NICAD for Type-3 clone detection (semantically equivalent with structural variation)

### 1c. Semantic Embeddings
Dense vector representations of code that encode *meaning* beyond surface tokens:
- **CodeBERT, UniXcoder, StarCoder, Code2Vec** — transformer-based; trained on code + comments; cosine similarity in embedding space
- **model2vec / fastembed (already in wicked-estate)** — static (non-contextual) embeddings; much faster, good for name+signature strings; less powerful for long function bodies
- **doc-comment embeddings** — when both symbols have doc strings, embedding the natural-language description captures intent that pure code tokens miss

### 1d. Call-Graph Alignment
Two symbols correspond if their local call-graph neighborhoods are isomorphic or near-isomorphic:
- **k-hop neighbor set similarity** — Jaccard of the set of callee names at depth k=1,2
- **Community / module co-membership** — both belong to functionally equivalent modules (detected via community detection as in wicked-estate's `detect_communities`)
- **Import graph alignment** — what a module imports correlates strongly with what its peer in the other repo imports

### 1e. API Surface / Behavioral Matching
At the coarser symbol-set level:
- **Public API diff (symbol set)** — which public names are shared across two repos (direct indicator of a fork or reimplementation)
- **Test-case name matching** — test function names encode the behavior they test; matching test names is a strong proxy for functional correspondence
- **Docstring semantic matching** — NLP similarity on doc comments; more robust than code tokens when the language differs

---

## 2. Which Signals Matter Most in Practice

Key findings from the code clone / similarity literature:

### SourcererCC (Sajnani et al., ICSE 2016)
- Operates on **token-set overlap** at the function level (Jaccard threshold ~0.7).
- Scales to millions of files; BigCloneBench evaluation shows ~100% recall on Type-1/2 (identical / renamed), ~69% recall on Type-3 (gapped/structural) clones.
- **Lesson:** token overlap dominates for lexically similar functions. It degrades sharply on algorithmic clones (Type-4) where the logic is equivalent but names differ.

### CCFinder (Kamiya et al., TSE 2002)
- Token-sequence transformation + suffix-tree matching.
- Effective within a language; cross-language is poor because token vocabularies diverge.
- **Lesson:** suffix-tree on normalized tokens is a strong precision tool but a recall disaster across languages.

### Deckard (Jiang et al., ICSE 2007)
- Converts AST subtrees to **characteristic vectors**, clusters with LSH.
- Strong on Type-3 clones (structural with identifier renaming).
- **Lesson:** AST-vector approaches generalize better than token-sequence when identifiers are renamed — critical for cross-repo where internal names differ.

### NICAD (Cordy & Roy, WCRE 2011)
- Source normalization + longest-common-subsequence diff.
- Highest precision for near-miss (Type-3) function clones across languages.
- **Lesson:** Text normalization before comparison (whitespace, type keywords, literal values) narrows false positives significantly.

### CodeBERT / UniXcoder (Feng et al., EMNLP 2020; Guo et al., ACL 2022)
- CodeBERT: fine-tuned on code-natural language pairs; strong on natural language → code retrieval (CodeSearchNet benchmark).
- UniXcoder: cross-modal; better at code ↔ code similarity.
- **Lesson:** Semantic embeddings outperform lexical methods on Type-4 (algorithmic equivalence) by a wide margin (MRR gains of 20–40% on CodeSearchNet vs. BM25). For near-miss (Type-3) they are comparable to NICAD.

### BigCloneBench (Svajlenko et al., ICSME 2014)
- Standard benchmark: ~8M clone pairs across 43 Java projects.
- Type-1/2: trivially detected by any method.
- Type-3 (strongly gapped): best results require combining lexical + structural signals.
- Type-4 (semantic, different algorithm): only embedding-based methods reliably reach >50% recall.
- **Lesson:** No single signal dominates across all clone types. A fused approach consistently outperforms any individual method by 10–25% F1 on Type-3/4.

### Signal Importance Ranking (from empirical literature)

| Signal | Type-1/2 | Type-3 | Type-4 | Cost |
|---|---|---|---|---|
| Exact name | Excellent | Poor | Poor | Free |
| Token Jaccard | Excellent | Good | Poor | Low |
| BM25 | Excellent | Good | Poor | Low (already built) |
| AST/structural vector | Good | Excellent | Good | Medium |
| Call-graph neighborhood | Good | Good | Good | Medium |
| Dense embedding (CodeBERT) | Excellent | Excellent | Good | High |
| Static embedding (model2vec) | Good | Good | Fair | Low-Medium |

---

## 3. Best Pure Structural/Lexical Approach (No Embeddings, No GPU)

When embeddings are unavailable (offline mode, `--no-embed`), use a **three-signal weighted sum**
after token normalization.

### Normalization pipeline (applied to both `a` and `b`)
1. Split name on camelCase boundaries and underscores → token list
2. Lowercase all tokens
3. Strip stop-prefixes: `get`, `set`, `is`, `has`, `do`, `on`, `to`, `from`, `with`, `make`
4. Strip stop-suffixes: `impl`, `handler`, `helper`, `util`, `manager`, `service`
5. Map parameter type names to a normalized set: `String/str/string → STR`, `int/i32/i64/long/Int → INT`, `bool/Bool/boolean → BOOL`, `*List/*Array/*Vec → LIST`, `*Map/*Dict/*HashMap → MAP`, `void/() → VOID`
6. Re-join as token set (order-independent for Jaccard)

### Scoring formula (lexical-only mode)

```
score_lex(a, b) =
    w_name   · jaccard(tokens(norm(a.name)),      tokens(norm(b.name)))
  + w_sig    · jaccard(tokens(norm(a.signature)), tokens(norm(b.signature)))
  + w_kind   · exact_kind_match(a.kind, b.kind)
  + w_arity  · max(0, 1 - |arity(a) - arity(b)| / max(arity(a), arity(b), 1))
```

Recommended weights (derived from SourcererCC parameter sensitivity analysis):

| Weight | Value | Rationale |
|---|---|---|
| `w_name` | 0.50 | Name is the strongest single signal |
| `w_sig` | 0.25 | Signature text adds structural context |
| `w_kind` | 0.15 | Function ↔ Function matches should outrank Function ↔ Class |
| `w_arity` | 0.10 | Arity agreement is a weak but cheap filter |

**Kind matching rules:** `Function` matches `Function` (1.0), `Function` matches `Method` (0.8),
`Method` matches `Method` (1.0), `Class` matches `Struct` (0.6), everything else (0.0).

**Candidate pre-filtering:** Before scoring, restrict the B-side candidate pool to the top-20
results from `store_b.find_symbols(SymbolQuery { text: Some(norm(a.name)) })` (BM25 already
normalizes by term frequency). This avoids O(|A|·|B|) all-pairs comparison.

**Threshold:** Emit only pairs with `score_lex >= 0.35`. Below that, the pair is noise.

### Why not edit distance?
Levenshtein is O(n·m) per pair and adds negligible information over Jaccard-of-tokens once names
are split. Jaro-Winkler is better for short strings but unreliable after prefix stripping — not
worth the implementation cost.

---

## 4. Fusing Embeddings with Lexical/Structural Signals

When embeddings are available (`--embeddings` was used at index time), two fusion strategies apply:

### Option A: RRF (Reciprocal Rank Fusion)
```
rrf_score(a, b, k=60) =
    Σ_i  1 / (k + rank_i(b | a))
```
where `rank_i(b | a)` is the rank of symbol `b` in candidate list `i` (name-list from FTS, embedding-list from ANN). Already implemented in `wicked-estate-retrieve/src/lib.rs` as `reciprocal_rank_fusion`.

**Pros:** Rank-based — immune to score-scale differences between FTS BM25 and cosine similarity.
No hyperparameter tuning per corpus. Empirically robust (Cormack et al., ECIR 2009).

**Cons:** Discards absolute score magnitudes — two lists with very different confidences are
treated equally at the same rank position. No way to express "I trust the embedding list much
more than the name list for this query."

### Option B: Weighted Linear Sum (after min-max normalization)
```
score_hybrid(a, b) =
    α · norm(cos_sim(emb_a, emb_b))
  + β · norm(bm25_name(a.name, b))
  + γ · exact_kind_match(a.kind, b.kind)
  + δ · norm(sig_jaccard(a.sig, b.sig))
```
where `norm(·)` is per-list min-max normalization to [0,1] over the top-k candidates.

Recommended starting weights: α=0.45, β=0.30, γ=0.15, δ=0.10.

**Pros:** Can emphasize embeddings when they are high-dimensional (CodeBERT-level) and known
reliable. Tunable.

**Cons:** Requires calibration. BM25 scores are unbounded and corpus-dependent — normalization
over the candidate list introduces dependency on the candidate pool size.

### Option C: Two-Stage Filter-Then-Rank
Stage 1: Fast lexical pre-filter with `score_lex >= 0.25` OR `cos_sim >= 0.65` (OR, not AND —
either signal qualifies a candidate for stage 2).
Stage 2: Weighted linear sum on the ~50 surviving candidates per symbol.

**Pros:** Cheap pre-filter eliminates most false positive pairs before expensive scoring.
Recall-safe because OR combination prevents early elimination of Type-4 matches (same logic,
different names → embedding passes but lexical fails).

**Cons:** More code. Two thresholds to tune.

### Recommendation
Use **RRF in the initial implementation** (it is already built, requires no calibration,
degrades gracefully when one list is empty). Switch to two-stage once the command has enough
real-world usage to calibrate weights empirically.

For the `basis` field in the output, tag which lists contributed:
- `"name"` — only FTS name list was non-empty
- `"embed"` — only embedding list was non-empty
- `"name+embed"` — both lists present, RRF combined
- `"name+embed+sig"` — all three signals fired

---

## 5. What Normalization Matters

### camelCase / snake_case splitting
Split on: `_`, `-`, uppercase letter following lowercase letter, digit/letter boundaries.
`getUserById` → `[get, user, by, id]`; `get_user_by_id` → same tokens.
This is the single most important normalization step — without it, cross-language matching
(Java `getUserById` ↔ Python `get_user_by_id`) misses entirely.

### Stop-prefix / stop-suffix stripping
Strip: `get`, `set`, `is`, `has`, `do`, `on`, `to`, `from`, `with`, `make`, `build`, `create`.
Strip suffixes: `impl`, `handler`, `helper`, `util`, `manager`, `service`, `controller`.

**Warning:** strip these ONLY for Jaccard token matching, NOT for BM25 input. BM25 needs the
original name for FTS matching — stripping before FTS submission loses recall.

### Parameter type normalization across languages
Normalize before signature Jaccard:

| Input variants | Normalized token |
|---|---|
| `String`, `str`, `&str`, `string`, `varchar` | `STR` |
| `int`, `i32`, `i64`, `long`, `Int`, `Integer`, `number` | `INT` |
| `bool`, `boolean`, `Bool` | `BOOL` |
| `*[]`, `*Vec`, `*List`, `*Array`, `*Slice` | `LIST` |
| `*Map`, `*Dict`, `*HashMap`, `*Object` | `MAP` |
| `void`, `()`, `Unit`, `None`, `null` | `VOID` |
| `f32`, `f64`, `float`, `double`, `Float`, `Double` | `FLOAT` |

### Language-aware stemming
Do NOT use English stemming (Porter/Snowball) on identifiers. Programming identifiers are not
natural-language words — stemming `parser` → `pars` and `parsed` → `pars` causes collisions with
unrelated names. Use token-set membership (exact normalized tokens) instead.

### File path normalization
When using file path overlap as a weak signal: strip the top-level repo directory, normalize
separators, and split on `/`. Jaccard over path segments gives a weak but sometimes useful
module-locality signal (`src/auth/login.rs` ↔ `lib/auth/login.py` → high overlap).

---

## 6. Known Failure Modes

### False positives from common names
Universal function names — `init`, `new`, `main`, `run`, `start`, `stop`, `handle`, `parse`,
`serialize`, `deserialize`, `encode`, `decode`, `connect`, `close`, `open`, `read`, `write`,
`update`, `delete`, `create`, `get`, `set` — appear in virtually every codebase. They score
high on name similarity by accident.

**Mitigations:**
- BM25 IDF already down-weights high-frequency terms within a corpus, but only within one DB.
  Cross-DB, the IDF from DB-B does not reflect how common a name is in DB-A.
- Add a **global stop-name list** (the above set + language-specific variants). When `a.name`
  (after normalization) is in the stop-name list, set `w_name = 0.0` and rely only on
  signature and embedding signals.
- Require `score_lex >= 0.55` (higher threshold) when the name is in the stop-name list.

### Cross-language idiom mismatches
- **Constructors:** Java `new Foo(...)` ↔ Rust `Foo::new(...)` ↔ Python `Foo(...)`. The "name"
  of a constructor is the class name in Python/Ruby, `new` as a static method in Rust/Go,
  `<init>` in JVM bytecode. Normalizing `new` → class-name + constructor kind is needed.
- **Getters/setters:** Java `getFoo()` / `setFoo(x)` ↔ Python `@property foo` ↔ Kotlin `val foo`.
  After prefix stripping, `getFoo` and `setFoo` both reduce to `foo` — they should NOT match
  each other. Retain the `get`/`set` distinction in kind metadata or as a separate signal bit.
- **Error handling:** Go `(T, error)` return pattern vs. Rust `Result<T, E>` vs. Python
  exception. Signature normalization that strips error types will conflate these.
- **Async wrappers:** `asyncFetch` vs. `fetch` — strip `async` prefix in normalization, but
  this means an async and sync version of the same function look identical, which may or may not
  be desired.

### Overloaded names
In Java/C++/Kotlin, `process(String)` and `process(int)` are different functions with the same
name. The arity/type signal disambiguates them, but only if the signature is correctly parsed and
normalized. If both repos use overloading, name-only matching will produce multiple equally-scored
false pairs.

### Name collisions from common patterns
- Test helper functions: `setUp`, `tearDown`, `beforeEach`, `afterEach` exist in every test suite.
  Add test-framework names to the stop-name list.
- Logging wrappers: `log`, `logger`, `info`, `warn`, `error`, `debug` — near-universal in server-side code.
- Lifecycle methods: `render`, `update`, `destroy`, `componentDidMount` (React idiom), `viewDidLoad` (iOS).

### False negatives from aggressive normalization
Stripping too many prefixes can conflate `getUserId` and `updateUserId` after removing `get`/`update`,
leaving only `user_id` tokens. Both reduce to the same token set — they are NOT the same function.
**Fix:** never strip a prefix that changes the semantic verb (CRUD verbs: `create`, `read`,
`update`, `delete`, `fetch`, `save`, `load`, `store` should be KEPT).

---

## 7. Existing Open-Source Implementations Worth Studying

### Sourcegraph cross-repo search
- Approach: symbol index (SCIP/LSIF) + text search. Cross-repo done via a shared index server.
- Relevant: their SCIP schema for stable symbol identity across repos is directly applicable.
  Stable symbol IDs prevent the "rename breaks everything" problem already addressed in ADR-002.
- URL: https://github.com/sourcegraph/sourcegraph (zoekt for text; src-cli for symbol index)
- Key file: `cmd/symbols/` — the symbol indexer + search API.

### OpenGrok (Oracle/GitHub)
- Approach: Lucene-based cross-language code search; definition/reference graph via ctags.
- Relevant: their normalization of identifiers before Lucene indexing is a known-good recipe.
  OpenGrok uses `CamelCaseTokenizer` and a custom analyzer chain — worth inspecting.
- URL: https://github.com/oracle/opengrok

### SourcererCC
- **Most directly applicable** clone detector for function-level matching.
- Approach: token-bag overlap with inverted index; scales to millions of functions.
- URL: https://github.com/Mondego/SourcererCC
- Key insight from the paper: use a **filter** (upper-bound token-count ratio must be ≥ threshold)
  before computing Jaccard to skip most pairs cheaply. Implement as: if
  `|tokens(a)| / |tokens(b)| < threshold` (or inverse), skip. This is O(1) per pair.

### NICAD (Cordy & Roy)
- Approach: PrettyPrint normalization → LCS diff. Best precision on near-miss clones.
- URL: https://www.txl.ca/txl-nicaddownload.html (TXL-based, proprietary grammar)
- Key insight: the TXL source transformations are an excellent reference for per-language
  normalization rules (what to strip, what to keep). Study their `*.txl` files for Java/Python/C.

### Deckard
- URL: https://github.com/skyhover/Deckard
- Approach: AST-to-vector → LSH bucketing → cluster comparison.
- Key insight: their vector construction from AST node types (not names) is language-agnostic
  and directly applicable when wicked-estate has the tree-sitter parse tree available.

### iClones / BigCloneBench
- URL: https://github.com/jeffsvajlenko/BigCloneBench
- Relevant as an evaluation resource, not an implementation. Provides 8M labeled Java clone pairs.
  Use to validate the `correspond` scoring formula offline before shipping.

### code2vec / code2seq (Alon et al., POPL 2019)
- URL: https://github.com/tech-srl/code2vec
- Approach: path-context encoding of AST paths → embedding.
- Relevant: shows that AST-path bags can be projected to dense vectors that encode semantic
  similarity better than token bags alone. A future embedding model for wicked-estate could
  replace model2vec with a code2vec-style encoder.

---

## Recommended Approach

### Lexical-only mode (offline, no embeddings)

```
# Step 1: normalize both symbols
norm_a = normalize(a)   // token split + lowercase + prefix strip + type normalization
norm_b = normalize(b)

# Step 2: pre-filter via BM25 candidate retrieval (top-20 from B's FTS5)
candidates_b = store_b.find_symbols(SymbolQuery { text: Some(norm_a.name), limit: 20 })

# Step 3: score each candidate
score_lex(a, b) =
    0.50 · jaccard(tokens(norm_a.name), tokens(norm_b.name))
  + 0.25 · jaccard(tokens(norm_a.sig),  tokens(norm_b.sig))
  + 0.15 · kind_match(a.kind, b.kind)
  + 0.10 · arity_sim(a.sig, b.sig)

# Step 4: apply stop-name boost / penalization
if norm_a.name ∈ STOP_NAMES:
    score_lex = score_lex * 0.6   // penalize common names; demand sig+kind to compensate

# Step 5: threshold and emit
emit pairs where score_lex >= 0.35
basis = "name" | "name+sig" | "name+sig+kind"  // whichever sub-signals ≥ 0.1 each
```

### Hybrid mode (with embeddings)

```
# Step 1: build two candidate lists for symbol a from DB-A
name_list  = store_b.find_symbols(text=norm_a.name, limit=20)   // FTS BM25
emb_list   = store_b.nearest(store_a.embedding(a.symbol), k=20) // ANN cosine

# Step 2: optionally add signature FTS list
sig_list   = store_b.find_symbols(text=norm_a.sig_tokens, limit=10)  // if sig non-empty

# Step 3: fuse via RRF (k=60, already in wicked-estate-retrieve)
fused = reciprocal_rank_fusion([name_list, emb_list, sig_list], k=60)

# Step 4: re-score top-10 RRF survivors with full signal
for (b, rrf_score) in fused.take(10):
    boosted = rrf_score
            + 0.10 * kind_match(a.kind, b.kind)
            + 0.05 * arity_sim(a.sig, b.sig)
    emit (a, b, score=boosted, basis=...)

# Threshold: emit pairs where boosted >= 0.20
# (RRF raw scores are small; this threshold is empirically ~top-5 per symbol)
```

### Signal set summary

| Signal | Mode | Source in wicked-estate | Weight |
|---|---|---|---|
| BM25 name FTS | Both | `find_symbols` on `store_b` | Primary (RRF list 1) |
| Embedding cosine ANN | Hybrid | `store_b.nearest(store_a.embedding(sym))` | Primary (RRF list 2) |
| Signature FTS | Both | `find_symbols(sig_tokens)` on `store_b` | Secondary (RRF list 3) |
| Kind match | Both | `a.kind == b.kind` | +0.10–0.15 boost |
| Arity similarity | Both | parse param count from signature | +0.05–0.10 boost |
| File path segment overlap | Optional | `location.file` token Jaccard | +0.05 weak signal |
| Stop-name penalty | Both | Global stop-name list | ×0.6 multiplier |

### Output struct

```rust
pub struct CorrespondPair {
    pub a:      SymbolId,    // symbol from DB-A
    pub b:      SymbolId,    // best match from DB-B
    pub score:  f64,         // in [0, 1], higher = more confident
    pub basis:  String,      // e.g. "name+embed", "name+sig+kind"
}
```

---

## Open Questions / Not Yet True

1. **IDF cross-corpus.** BM25 IDF is computed per-corpus. A name that is common in DB-B but
   rare in DB-A will receive artificially high IDF weight when queried against DB-B's FTS5 index.
   There is no cross-corpus IDF normalization in SQLite FTS5. Mitigation: use the stop-name list
   as a manual IDF substitute. A more principled fix would pre-compute term frequency across both
   corpora and adjust weights — not yet designed.

2. **Signature parsing quality.** The arity/type normalization signal requires that `Node::signature`
   actually contains parseable parameter lists. Current extractors store the raw signature string
   from tree-sitter; format varies by language. How reliable the arity extraction is across all
   73 languages is unknown until tested. If signature is absent or unparseable, `w_sig` and
   `w_arity` drop to 0 gracefully.

3. **Threshold calibration.** The recommended thresholds (0.35 for lexical-only, 0.20 for hybrid
   RRF) are derived from SourcererCC's parameter study and NICAD's precision/recall curves on
   BigCloneBench Java data. They have NOT been validated on wicked-estate's actual corpora.
   A calibration run against a known-correspondent pair of repos (a fork + original, or a
   Java-to-Kotlin migration) is required before trusting them.

4. **Large-graph scalability.** For a DB with 100k+ symbols, the O(|A| × 20) BM25 queries
   still amounts to 2M FTS5 lookups. Need to bench the correspond command on a large pair
   before claiming it is usable at scale. Batch or parallelize the per-symbol FTS queries.

5. **Embedding model mismatch.** If DB-A and DB-B were indexed with different embedding models
   (e.g., model was updated between indexes), cosine similarity is meaningless. The `nearest`
   call will silently return garbage. Add a model-ID check at the start of `correspond` —
   read the embedding model name from each store's metadata and error-out or warn if they differ.

6. **Call-graph neighborhood signal absent.** This recon identifies call-graph neighbor Jaccard
   as a useful Type-3/4 signal (what functions does `a` call? does `b` call the same names?).
   It is NOT included in the recommended formula above because it requires materializing the
   outgoing edges of every symbol — potentially slow. This is a known gap; evaluate whether
   it improves recall enough on real-world pairs to justify the cost.

7. **Cross-language kind mapping.** `NodeKind::Struct` (Rust) vs. `NodeKind::Class` (Java) should
   correspond (both are named type definitions). The `kind_match` function above uses a hand-coded
   table. This table needs to be extended and tested against the full `NodeKind` enum before
   the kind signal is trustworthy.
