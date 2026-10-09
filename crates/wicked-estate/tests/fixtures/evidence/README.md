# Semantic-evidence contract fixtures (TS-S2C)

These envelopes are **contract fixtures only**. They exercise the language-neutral
`SemanticEvidence` v1 envelope with the identity shapes of four enterprise legacy toolchains. They are
hand-written: there is **no** COBOL, PL/SQL, ABAP or RPG producer integration in wicked-estate, and
a fixture that ingests cleanly does not make a language supported, precise or taint-ready.

| Fixture | Modelled on | What it pins |
|---|---|---|
| `cobol-copybook.contract.json` | Eclipse Che4z COBOL LS (definitions/references incl. copybooks, no index export) | a data item defined in a copybook belongs to the copybook document; `CALL 'TAXCALC'` without a declared `calls` capability is never a call |
| `plsql-plscope.contract.json` | Oracle PL/Scope `ALL_IDENTIFIERS` (USAGE = CALL with LINE/COL, SIGNATURE identity) | the one legacy source with site-level call evidence; quoted `"Raise"` and unquoted `RAISE` are different signatures and stay different; dynamic SQL is a dynamic target |
| `abap-namespace.contract.json` | SAP repository cross-reference (where-used) | namespaced objects (`/ABC/`, `/XYZ/`) with the same method name stay distinct |
| `rpg-dsppgmref.contract.json` | IBM i `DSPPGMREF` (object-level references, no line numbers or call sites) | a producer with no definitions and no call capability projects nothing |
