# Document Extractor — spec (recon)

> **⚠ SUPERSEDED (framing)** by `wicked-memory/docs/recon/knowledge-capability-agent-spec.md`.
> The work is **agent-driven (tools + skills)**, not an estate `Extractor` or a Rust build plan.
> Retained for the still-valid spine-constraint analysis (§1–§2: why decode / vision / semantic
> chunking cannot live inside the pure `Extractor`) that drove the agent-first reframe, and the
> carried-forward decisions (single document language, ontology-as-graph, the accepted-doc contract).
>
> Status: **decisions locked** (see §8), pre-build. Grounded against the trait spine.
> Scope: ingest non-code documents into the estate graph as first-class nodes, organized by a
> **lightweight ontology carried in the same graph**, so knowledge, code, and memory share one
> traversable store. This is the one capability with zero coverage in estate + wicked-memory today.
>
<!-- historical -->
> Retirement gate (since resolved — wicked-brain retired 2026-08): replacing wicked-brain was
> **blocked** on a real recall head-to-head that did not exist yet — see
> `docs/recon/baseline-recall-eval.md` (the 100% at time of writing was the LongMemEval `_oracle`
> plumbing pass, not a retrieval signal; no brain arm existed). Build the extractor in parallel; do
> NOT retire brain until the baseline lands.
<!-- /historical -->
>
> Verdict: **no new crate.** A feature-gated module in `wicked-estate-extract`, a decode/contract
> branch in the `wicked-estate` binary, and two resolvers. Decode, semantic chunking, vision, and
> ontology tagging are **judgment → upstream in the ingestion skill**, never in the engine.

---

## 1. The constraint that shapes everything

`Extractor` (crates/wicked-estate-core/src/traits.rs):

```rust
pub trait Extractor: Send + Sync {
    fn languages(&self) -> Vec<Language>;
    fn extract(&self, file: &SourceFile) -> Result<Extraction>;   // SourceFile.text: String
}
```

`extract` is **synchronous, pure, per-file, `Send + Sync`, no cross-file knowledge**, handed
`text: String` — never bytes, never a model handle. Therefore anything requiring **judgment**
(vision/OCR, *semantic* chunk boundaries, concept tagging) cannot live inside an `Extractor`. It runs
**upstream** and reaches the engine as a normalized, deterministic contract (§2). Same grammar-less
shape as `jcl.rs` ("the pattern for ANY grammar-less legacy format").

---

## 2. Two-tier ingestion

```
 bytes / pixels on disk
   │
   ▼
 ┌─ Tier 2 — JUDGMENT, in the ingestion SKILL / host (NOT estate) ────────────────────────┐
 │   decode (incl. VISION for flagged doc classes)  →  text/markdown                       │
 │   SEMANTIC chunking                              →  chunk boundaries                     │
 │   ontology tagging                               →  concept ids per chunk               │
 │   stamps decode_confidence + decoded_by (R7)                                            │
 │   emits the ACCEPTED-DOC CONTRACT (§4)                                                   │
 └─────────────────────────────────────────┬───────────────────────────────────────────────┘
                                            ▼
 ┌─ Tier 1 — DETERMINISTIC, in the engine ───────────────────────────────────────────────┐
 │   binary decode stage maps the contract (or a text-native file) → SourceFile.text        │
 │   DocumentExtractor → doc / section / chunk / concept nodes + Contains/about/broader      │
 │                       edges + doc→code mention refs                                       │
 │   (built-in STRUCTURAL chunking = offline fallback when no contract boundaries given)     │
 └───────────────────────────────────────────────────────────────────────────────────────┘
```

The model does the relevance-bearing judgment (decode, semantic chunking, concept tags) **before**
estate sees anything; estate maps a clean intermediate to nodes, deterministically. This keeps
determinism, the `estate-core ⇏ memory` firewall, rules-as-data, and engine-vs-skills all intact.

**Relevance is not just chunking.** relevance = chunking × **embeddings** × graph edges × rerank. The
cheapest big lever is the embedder: estate's default `HashEmbedder` is FNV bag-of-words (effectively
lexical). Real semantic recall needs `model2vec` or `fastembed` (feature-gated in `estate-retrieve`)
with a cached model — and the engine **silently falls back to HashEmbedder** if the model can't load
(`wicked-memory/.../lib.rs:81`), so this must be made explicit, not assumed. Treat enabling a real
embedder as a first-class build item, not a footnote.

---

## 3. Node, edge & ontology model (ontology-as-graph — DECISION 3)

One estate graph. Concepts + typed relations are nodes/edges in the existing model — the **rules
engine** (`RuleSet`/`Rule`/`Condition`/`Action`/`Fact` + `Governs`/`Evaluates`/`Produces`) already
proves estate carries a typed concept/relation schema natively. Knowledge nodes sit under a
`Node.scope` partition (e.g. `knowledge:`) so they're filterable and don't pollute code ranking by
default.

### Nodes

| Node | `NodeKind` | `Language` | key `metadata` |
|---|---|---|---|
| Document | `Other("document")` | `document` | `source_type`, `byte_digest` (xxh3), `ingested_at`, `decoded_by`, `decode_confidence`, `page_count` |
| Section | `Other("section")` | `document` | `heading_level`, `ordinal` |
| Chunk | `Other("chunk")` | `document` | `content` (canonical); **`Node.doc` = chunk text → FTS5-indexed** |
| Concept | `Other("concept")` | `ontology` | `canonical_label`, `aliases[]`, `source` (imported/emergent) |

Chunking: **semantic boundaries from the contract** (Tier 2); deterministic structural chunking is the
offline fallback only.

### Edges

- **`Contains`** (`Parsed`): document → section → chunk (containment / blast-radius), like job→step.
- **`Other("about")`** (chunk → concept): the relevance/grouping link. source = chunk (dependent),
  target = concept.
- **`Other("broader")`** (concept → parent concept): subsumption. source = sub-concept, target =
  parent. **Subsumption query = bounded `traverse(parent, Dependents, edge="broader")`** — hierarchy
  without a reasoner. (`related`/`instance_of` follow the same `Other(..)` pattern.)
- **doc→code mention** (`UnresolvedRef`, `References`): high-signal symbol mentions only; resolved
  cross-graph in RESOLVE (JCL→COBOL analog). Prose name-drops stay unresolved (R3).
- **concept→code** (optional, `Governs`/`about`): a concept relates to the code that implements it —
  closing the doc↔concept↔code triangle that justifies the shared store.

"All chunks about Payment, including sub-concepts" = expand concepts under `Payment`
(`broader`-traversal) → gather chunks with `about` edges into that set. Two bounded graph hops, no
inference engine.

### Identity (ADR-002 — never content-hash / line number)

- Document `Symbol::synthetic("doc", <path>)`; Section `("doc-section","<path>::<heading-path>")`;
  Chunk `("doc-chunk","<path>::<section>::<ordinal>")`; Concept `("concept", <canonical-id>)`.
- Re-ingest = `GraphWrite::remove_file(path)` + re-extract. Doc/section/**concept** ids are stable;
  **chunk ids may churn** on edit — so memory + cross-edges target **document/section/concept**, never
  a chunk ordinal.

---

## 4. The accepted-doc contract (DECISION 4 — model decodes before estate)

The "specs for docs we accept." Tier 2 emits this; Tier 1 maps it deterministically.

```jsonc
{
  "source_ref": "reports/q3.pdf", "source_type": "pdf",
  "decoded_by": "vision",            // native | pdf-text | docx | … | vision
  "decode_confidence": 0.82,         // R7 — visible; vision < text-layer < native
  "title": "Q3 Review", "doc_metadata": { "author": "...", "date": "..." },
  "digest": "xxh3:...",              // R5 — staleness; re-ingest on change
  "chunks": [{
    "anchor": "q3.pdf::risk::2",     // stable id basis
    "text": "...", "heading_path": ["Risk","Payments"], "ordinal": 2,
    "concepts": [{ "id": "payment-retry", "label": "Payment Retry", "confidence": 0.9 }],
    "mentions": [{ "symbol_hint": "PaymentService", "kind": "references", "confidence": 0.7 }]
  }],
  "concepts": [{ "id":"payment-retry", "label":"Payment Retry", "broader":["payment"] }]
}
```

**Use vision for:** scanned / image-only PDFs, diagrams & figures, visual-heavy decks, handwriting.
**Deterministic decode for:** native-text PDF, docx/pptx/xlsx, html, md, txt, csv.

Where it lives — still **no new crate**:

| Piece | Home | Precedent |
|---|---|---|
| Contract schema + decode/ingest branch | `wicked-estate` binary, at the `SourceFile` build site (`src/lib.rs:~386`) | replaces today's silent `from_utf8(bytes).ok()?` drop |
| `DocumentExtractor` (contract → nodes; text-native fallback) | new `wicked-estate-extract/src/documents.rs` | `jcl.rs` struct + `pub use` |
| `ConceptResolver` (label → canonical concept node) | `wicked-estate-resolve` | resolver-tier cascade |
| `DocMentionResolver` (doc → code) | `wicked-estate-resolve` | JCL→COBOL resolution |
| Optional in-engine binary parsers (offline fallback) | `documents.rs`, `#[cfg(feature="documents")]` | `excel_rules` feature-gating |
| Vision / semantic chunking / concept extraction | **ingestion skill — NOT estate** | wicked-memory `Reasoner` seam |

Concept vocabulary is owned upstream (the model emits canonical `concept.id`s); estate upserts concept
nodes + `about`/`broader` edges deterministically. `ConceptResolver` only does fuzzy label→canonical
as a fallback — vocabulary *judgment* stays out of the engine. Not a `languages.toml` row (the
manifest test rejects grammar-less entries; documents take the JCL/MQ struct path).

---

## 5. wicked-memory integration

Documents, sections, **and concepts** become `capture_about(mem, &[SymbolId])` targets. Recall from a
code seed now traverses code → doc and code → concept → doc — the "recall with zero lexical overlap"
differentiator extended to grounded documents and the concept hierarchy. No `MemoryApi` change;
they're just more nodes in the shared store.

---

## 6. Honesty / agent-behavior rules

- **R6:** decode failures emit `DOC-FALLBACK:` + skip-with-reason. The current silent UTF-8 drop is a
  **defect to fix in this work** (retire it in the same change, §8/§"retire as you go").
- **R7:** every doc node carries `decode_confidence` + `decoded_by`; vision-derived docs are never
  presented as fact. Concept `about` edges carry the model's tag confidence.
- **R3:** report sections/pages that failed to decode; leave low-signal mentions unresolved rather
  than fabricate edges.
- **R5:** `byte_digest` + `ingested_at`; re-ingest via `remove_file` on digest change.

---

## 7. Build plan (spine before fan-out)

| Wave | Deliverable | Consumer / gate |
|---|---|---|
| **D.0** | Accepted-doc **contract schema** + decode/ingest branch in the binary with loud skip (R6). | test: unknown binary skipped loudly, not silently; contract round-trips |
| **D.1** | `DocumentExtractor`: contract → doc/section/chunk + `Contains`; text-native deterministic fallback (md/txt/html). | conformance test + binary dispatch |
| **D.2** | Ontology-as-graph: concept nodes + `about`/`broader` edges from the contract; `ConceptResolver` (label→canonical). | test: `traverse(parent, Dependents, "broader")` returns sub-concept chunks |
| **D.3** | `DocMentionResolver` (doc→code). | cross-graph test: chunk mentions a real symbol → resolved edge |
| **D.4** | wicked-memory: documents + concepts as `capture_about` targets; recall from code seed surfaces grounding docs. | recall test |
| **D.5** | **(skill, out of engine)** decode + vision + semantic chunking + concept tagging → emits the contract + provenance/confidence. | skill eval |
| **E** | Enable a real embedder (`model2vec`/`fastembed`) with a cached model; kill the silent HashEmbedder fallback for the knowledge path. | semantic-recall smoke; required for relevance AND a fair baseline |

**Gates (§9):** per-crate `build`/`test`/`clippy -D warnings`; `≥73` language parity unaffected; new
doc + concept conformance; **no silent-skip** behavior test (R6). **Retire-as-you-go (§8):** D.0
deletes the silent `from_utf8(...).ok()?` drop.

---

## 8. Decisions (locked)

1. **Single `Language("document")`** + `source_type` in metadata. ✅
2. **Semantic chunking** — yes, but it's Tier-2 judgment: the model emits boundaries in the contract;
   the extractor ingests them; structural chunking is the offline fallback. Relevance also needs a
   real embedder (Wave E). ✅
3. **Ontology-as-graph in estate** (shared store, concepts as nodes/edges, subsumption via bounded
   traversal). Separation, if ever needed, comes from the store-spec abstraction (`open_store`), not
   from a premature split. ✅
4. **Model decodes/chunks/tags upstream**; estate ingests the accepted-doc contract. Vision is
   recommended per the doc-class table (§4). Heavy in-engine parsers are optional fallback. ✅

### Now-open (next)

- **Ontology source:** imported (existing enterprise ontology / SKOS) vs **emergent** (model extracts
  concepts from the corpus, with periodic consolidation/merge like wicked-memory `reflect()`)? Most
  likely emergent-first with an import path later. Needs a call before D.2.
- **The retirement baseline** (separate from this spec): build the real head-to-head — get LongMemEval
  `_s`/full, an engine-agnostic answer-session recall@k harness, a brain `POST /api` adapter, run
  memory with a cached semantic model, freeze dataset+threshold with an independent decider. ~3–4 eng
  days. See `docs/recon/baseline-recall-eval.md`.
