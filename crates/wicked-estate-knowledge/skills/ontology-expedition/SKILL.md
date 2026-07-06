# Ontology Expedition — the relation-typing pass (D-S.2)

This is **the bar over a flat brain**: a brain stores notes; this engine stores notes *plus the typed
relations between them*. After ingest, walk the concepts and connect them with **TYPED** edges.

> **DEC-R**: the agent (you) decides the relation TYPE by reading the content. The engine writes
> exactly the edge you name — it never infers a relation with a model.

## Method

1. Recall the concepts in a scope (`knowledge.recall`, or read the ingested chunks).
2. For each genuine relationship, call `knowledge.relate(src, tgt, rel, confidence, provenance)` where
   `rel` is a **specific relation type** — e.g. `governs`, `refines`, `contradicts`, `depends_on`,
   `supersedes`, `implements`. Pass your confidence and a provenance note.
3. The engine writes ONE `Other("<rel>")` edge per relation, with both endpoints verified to exist.

## Hard rules (falsifiers)

- **Type the relation. Never write an opaque `see-also` / `related` slug.** A slug edge is the exact
  thing this pass exists to beat. `governs` is `Other("governs")`, NOT a built-in edge kind, NOT a
  slug.
- Relate only nodes that exist. A `relate` to a non-existent target returns `isError` — fix the id or
  write the node first (node-before-edge).
- One typed edge per real relationship; don't fabricate relations to inflate the graph.
