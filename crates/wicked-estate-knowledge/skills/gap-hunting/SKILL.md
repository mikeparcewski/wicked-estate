# Gap Hunting (D-S.5)

Turn recall **misses** into ingest tasks — close the loop so the base gets better at the questions it
keeps failing.

> **DEC-R**: the engine logs misses deterministically (a recall whose top score is at/below the
> floor); YOU cluster them and decide what to ingest. No model in the logging path.

## Method

1. Read `knowledge.coverage` — it reports node counts and the number of logged recall misses.
2. Inspect the miss log: cluster the missed queries by topic. A repeated miss on a topic is a coverage
   gap.
3. For each gap cluster, formulate an **ingest task**: what document/source would answer it. Hand the
   task to `knowledge-ingest`.
4. After ingesting, re-run the previously-missed queries to confirm the gap is closed.

## Anti-patterns (falsifiers)

- A logged miss that never becomes an ingest task (the loop is open — the base never improves).
- Treating a one-off miss as a gap (cluster first; a single odd query is noise).
