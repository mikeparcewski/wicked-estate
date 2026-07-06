# Knowledge Ingest (D-S.1)

Ingest a document into the knowledge base, hotspot-first, as a `doc` node plus retrievable `chunk`
nodes (each `derived_from` the doc).

> **DEC-R**: the agent IS the reasoner. The engine provides the deterministic write path
> (`knowledge.ingest` / `knowledge.write`); YOU decide how to chunk, what to title, and which scope.
> The engine never calls a model.

## Method

1. Read the source. Split it into self-contained chunks (one idea each — a paragraph, a section, a
   table row). Keep each chunk independently meaningful so recall can return it verbatim with a
   citation.
2. Call `knowledge.ingest` with the title, the chunk list, a `scope` (e.g. `project:<repo>`), and the
   `source` (file path or URL) so every chunk carries provenance.
3. For a single fact that isn't part of a document, use `knowledge.write` with `class: concept`.
4. Verify with a seeded `knowledge.recall` that the key chunks come back.

## Anti-patterns (falsifiers)

- Chunks that are not self-contained (recall returns a fragment that needs its neighbours).
- Dropping `source` — an uncited chunk cannot back a `cited-answer`.
