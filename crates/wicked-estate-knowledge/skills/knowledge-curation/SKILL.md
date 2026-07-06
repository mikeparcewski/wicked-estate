# Knowledge Curation (D-S.3)

Keep the knowledge base clean as it grows: resolve duplicates **collapse-but-surface**, never
hard-delete.

> **DEC-R**: dedup candidates are a deterministic HINT; YOU decide what is truly a duplicate. The
> engine never adjudicates with a model.

## Method

1. When two nodes describe the same thing, pick a canonical one. Mark it `canonical`; append the
   loser's source/id to the canonical's `also_found_in`. **Both nodes stay visible and traversable** —
   the duplicate collapses for ranking only.
2. Re-`relate` the loser's typed edges onto the canonical so no relation is lost.
3. The only hard-delete is `knowledge.erase(scope_prefix)`, and it is **kind-guarded**: it touches
   only knowledge (`k*`) nodes, never code or memory nodes that happen to share a scope.

## Anti-patterns (falsifiers)

- Hard-deleting a duplicate instead of collapsing it (you lose provenance + traversability).
- Erasing a scope that holds code/memory nodes and expecting them to survive — the kind-guard protects
  them, but don't rely on `erase` for knowledge cleanup that should be a collapse.
