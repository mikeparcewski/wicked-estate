# Cited Answer (D-S.4)

Answer a question from the knowledge base with a **grounded, cited** slice — never from memory of the
model alone.

> **DEC-R**: `knowledge.recall` returns the relevant chunks (keyword ∪ vector, RRF-fused); YOU
> synthesise the answer and attach the citations. The engine ranks; it does not write the prose.

## Method

1. Call `knowledge.recall(query)`. Each returned line carries its `source` citation.
2. Compose the answer ONLY from the recalled chunks. Quote or paraphrase, and cite the `source` for
   each claim.
3. If recall returns nothing relevant (a miss is logged), say so and hand off to `gap-hunting` rather
   than inventing an answer.

## Anti-patterns (falsifiers)

- Answering beyond what the recalled chunks support (ungrounded claim).
- Dropping the citation — an answer the reader cannot verify is not a cited answer.
