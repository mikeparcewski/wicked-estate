# Vendored grammar provenance

Append-only record of every tree-sitter grammar vendored **as source** under this directory. Most
grammars in `wicked-estate-extract` are **pinned Cargo dependencies** (the `arborium-*` /
`tree-sitter-*` crates via `Cargo.lock`) — "pin, don't vendor source" — and are NOT listed here;
this file tracks only the source trees physically checked into `vendor/`.

Each row records where the source came from, the commit it was imported at (or `in-house` when the
tree originated in this repo), the date it was imported, and its license — so the origin and
license of every vendored tree is *stated, not assumed*. The root `NOTICE` carries the same
attribution for the assembled binary's license-disclosure obligations.

| Grammar | Subpath | Source | Commit / Origin | Imported | License | Notes |
|---|---|---|---|---|---|---|
| `tree-sitter-rpg` (Free-format RPG IV / ILE RPG) | `tree-sitter-rpg/` | In-house — authored in this repo (`https://github.com/mikeparcewski/wicked-estate`) | `in-house` — first committed in `797ce58` ("Initial public release"); not imported from an upstream grammar | 2026-06-14 | MIT (© 2026 Michael Parcewski) | Hand-authored via the "template-extrapolate" method (structure patterned after existing tree-sitter grammars; rules written for RPG free-form syntax). Validated by a corpus parse-gate + extraction-count assertions, **not** by upstream pedigree. Symbols+calls subset, not a full-language grammar. Crate: `wicked-estate-tree-sitter-rpg` v0.1.2. |

## Adding a vendored grammar

If a future grammar is vendored as source (e.g. an upstream grammar with no published crate),
append a row above with: the upstream repo URL, the exact commit SHA imported, the import date,
and the upstream license — and run a license check **before** the import. If the count of vendored
trees grows, graduate this table to a declarative `sources.manifest` + an idempotent import script
(the larger B16 pattern); for a single in-house tree the row + the root `NOTICE` line is the
right-sized record.
