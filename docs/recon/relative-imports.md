# Recon + plan: relative JS/TS import binding (lane `relative-imports`)

**Base:** `d7d3b58` on `lane/relative-imports`. **Trigger:** adversarial review of `estate-review/`
doc 01 (2026-08-28) — direction accepted, design rejected. This document is the change plan the
lane executes; every step names the test that proves it and what it deletes (§7, §8).

Sources read: `estate-review/REVIEW-adversarial-2026-08-28.md` (Doc 01 + engine defects),
`review-artifacts/findings.json` (all `D01-*` from four sources, `PER-1..12`), the reference
patch `review-artifacts/relative-import-resolver.patch` (shape only — its matcher is rejected),
and the four recon lenses (history / consumers / tests / risks) whose file:line anchors were
re-opened at `d7d3b58` before being cited below.

**Revision (attack round 1).** All six major issues accepted; none rejected. Resolution map:
ATT-INV-1 → S8 (independent `ts.resolveModuleName` oracle for the ambiguous subset; adjudicator
no longer self-referential); ATT-INV-2 (+FEAS-3, BR-5) → Decision J + S7 (importer-forcing scoped
to DELETED targets only — `remove_file` semantics re-verified at `sqlite.rs:1740-1756` show a
modified target keeps the importer's edge valid; collection ordering and FileWork retention spelled
out); ATT-INV-3 (+FEAS-4) → Decision H + S5 (cluster_summary added to the consumer table with a
behavior-preserving unfiltered score path; the concrete `pagerank_inner` signature change named);
BR-1 → S5 (kind filter added at the `important_symbols` cache-read seam, `lib.rs:1206-1213`, so
stale `pagerank.top` caches are cleaned at read time before graph-view's post-hoc exclusion is
deleted); BR-2 → S4 (MCP BlastRadius `dependents` through `cap_rows_to_budget` + R4 ceiling test;
CLI `--json` dependents bounded with an additive `truncated_dependents` count); FEAS-1 →
Decision G + S4 (depth≥2 rule replaced by the contains-aware transit rule — the depth rule was
factually wrong: caller-container Files are reachable at depth 2 at HEAD via Calls→Contains,
`store/src/lib.rs:634-637`, `sqlite.rs:1373-1376`, re-verified). Minors folded in: ATT-INV-4
(Decision C map/duplicate claim), ATT-INV-5 (S9 probe-count invariant instead of a wall-clock
unit test), ATT-INV-6 (Decision E contract sentence), BR-3 (ContextBundle unranked-tail filter),
BR-4 (confidence-stats-by-design note), FEAS-2 (corpus expectation tables corrected; dynamic
`import()`/`import=require` proven via fixtures, not the read-only corpora).

---

## 0. What exists today (verified at d7d3b58)

| Fact | Where |
|---|---|
| The extractor pushes one `Imports` `UnresolvedRef` per import statement with the **quoted** raw specifier (`'./foo'`), `from` = the File symbol, no hints. | `crates/wicked-estate-extract/src/treesitter.rs:1933-1937`, `:2128-2131`, `:2135-2143` |
| The extractor ALSO emits, per distinct specifier per file, a synthetic `NodeKind::Import` node whose id is `Symbol::global("ts-<lang>", [import, <spec>])` — **no file component, shared by every file importing the same text** — plus a `File → Import` `Imports` edge at `Parsed/1.0`, `resolved_by = "tree-sitter"`. | `treesitter.rs:1733`, `:2065-2096` |
| Local edges (incl. those synthetic edges) are written **before** the resolve pass; a `Resolver` returns `Vec<Edge>` only — it cannot delete or retarget anything. | `crates/wicked-estate/src/lib.rs:879-883`; `crates/wicked-estate-core/src/traits.rs:45-51` |
| The index slice is `[NameResolver, ScopedNameResolver, ImportMapResolver, InfraResolver]`; only refs from CHANGED files are resolved; "re-resolve direct importers" is a documented KNOWN LIMITATION, contradicting `docs/plan/WAVE-PLAN.md:106` ("touch 1 file → only it + importers re-resolve ✅"). | `lib.rs:18-25`, `:906-928` |
| `ImportMapResolver` handles `Calls` refs only; `file_matches_module` is its candidate-narrowing filter (suffix match `cand_stem.ends_with("/{logical}")`, `rsplit_once('.')` ext strip, `/index` trimmed on the candidate only) and `normalise_relative_path` pops `..` past root silently. No resolver binds `Imports` refs. | `crates/wicked-estate-resolve/src/lib.rs:253-283`, `:287-300`, `:336-339`, `:362-366` |
| Unresolved = refs whose `Location` is not carried by any deduped edge (location-keyed). | `lib.rs:937-946` (owned by the unresolved-accounting lane) |
| `TraversalSpec::blast_radius` walks ALL edge kinds (locked: "must follow every dependency edge kind"), `max_nodes 5000`; the SQLite CTE and `MemStore` BFS have no node-kind stop. `Contains` edges are `File → def`, so a `Dependents` walk from a symbol reaches its containing File at depth 1 AND every caller's containing File at caller-depth+1 (Calls backwards, then Contains backwards from the caller) — the walk stops only AT File nodes, because no edge targets a File at HEAD. **Correction from attack round 1 (FEAS-1):** the first draft misread this as "Files appear only at depth 1", which broke Decision G; re-verified at `store/src/lib.rs:634-637`, `sqlite.rs:1373-1376`. | `crates/wicked-estate-core/src/query.rs:40-55`; `docs/DESIGN-NOTES.md:16-20`; `crates/wicked-estate-store/src/sqlite.rs:1373-1409`; `store/src/lib.rs:609-660`; `treesitter.rs:2050-2060` |
| PageRank builds from ALL nodes, keeps `Calls|Imports` at uniform weight 1.0, no kind filter; `ranked_symbols` is the single result seam used by the `pagerank.top` cache, `important_symbols`, MCP `RankHotspots`, `blast_summary`, `ContextBundle`, bench `query_symbol`. graph-view excludes File/Import **after** fetching 4× the limit. | `crates/wicked-estate-rank/src/lib.rs:139-173`, `:289-305`; `wicked-estate/src/lib.rs:952-967`, `:1200-1225`; `retrieve/src/lib.rs:957`, `:1256`, `:1631`; `bench/src/capability.rs:287-301`; `main.rs:1514-1535` |
| Import nodes ALREADY sit in the top-20 at HEAD: studio 4/20, crew 5/20 with `import/json/` at #1 (consumers lens, `pagerank.top` on `repro/*-before.db`). This is a pre-existing defect, not one this lane introduces. | `scratchpad/repro/{studio,crew}-before.db` |
| `remove_file(target)` deletes edges `WHERE file=?1 OR source IN nodes-of-file`; an importer's `File→File` edge (file = importer) survives and is then dropped by `prune_dangling_edges` with no re-park. | `sqlite.rs:1739-1751`, `:1794-1807`; `lib.rs:970-981` |
| `ResolutionTier` has 7 variants enumerated by name in the property tests; `ImportMap = 0.6` by contract; per-edge confidence override inside a tier is the established pattern (`ScopedNameResolver` 0.65, `ImportMapResolver` 0.63 + `metadata.via`). | `core/src/edge.rs:28-68`; `core/tests/property_tests.rs:37-47`; `docs/ENGINE-CONTRACT.md:51`; `resolve/src/lib.rs:213-222`, `:384-390` |
| `wicked-estate-resolve` does not depend on `wicked-estate-extract`; `languages.toml` is `GENERATED by scripts/gen-language-manifest.py` from an external prior-art dir, `LanguageSpec` has no `deny_unknown_fields`, and its TS `ext = ["ts"]` diverges from the wired `LANG_TABLE` (`ts,mts,cts`). | `resolve/Cargo.toml`; `extract/languages.toml:1`, `:496-500`; `extract/src/lib.rs:145-176`; `scripts/gen-language-manifest.py:1-16`; `treesitter.rs:538-553` |
| JS/TS/TSX queries capture only `import_statement`; grammars expose `export_statement.source: string`, `call_expression.function: import`, and (TS/TSX only) `import_require_clause.source: string`. Text predicates (`#eq?`) are honoured. | `queries/typescript.scm:172-174`, `javascript.scm:100-102`, `tsx.scm:129-131`; tree-sitter-typescript 0.23.2 / -javascript 0.23.1 `node-types.json`; `queries/arm.scm:29` |
| Bench: `wicked-estate-bench <paths>` indexes local paths (no clone code; `baseline_corpus()` is a spec list), rewrites the tracked `docs/benchmarks/capability-report.md`, `query_symbol = important_symbols(...)[0]`, blast radius at depth 3, coverage% = resolved/(resolved+unresolved). No release bench binary exists in the main checkout's `target/release`. | `bench/src/main.rs:17-92`; `bench/src/lib.rs:98-118`; `capability.rs:287-338`, `:375-398`, `:1048-1080` |
| Baseline tests (lane target dir): resolve 61 unit + 1 + 4 (+1 ignored doctest, pre-existing); core 40 + 7 prop; rank 45 + 5; retrieve 100; extract 347 unit / 24 / 11; wicked-estate 47 lib + 20 main + 59 integration; bench 10 + 5. | tests lens, `cargo test -p <crate>` |

---

## 1. Findings acted on

| Id (source) | Finding | Acted on by |
|---|---|---|
| D01-1/D01-3 (attack), D01-2/D01-3 (audit), D01-3 (repro:impl), D01-4 (repro:baseline) | `file_matches_module` false-binds: `..` past root, suffix match; `dir_of` wrong for root files; `rsplit_once('.')` kills `.d.ts` | Step 2 (own exact-path binder, root guard, full-path map, no `dir_of`) |
| D01-4/D01-5/D01-6/D01-9 (repro:impl, attack) | `./index`, `./utils/index`, `./foo.d.ts` parked; `a.ts`+`a/index.ts`, `b.ts`+`b.css` parked as ambiguous | Step 1 data table + Step 2 priority order |
| D01-3 (attack), D01-11 (repro:impl) | O(refs × files), ~39 min on a 50k-file monorepo | Step 2 (O(files + refs) full-path map), Step 9 timing |
| D01-6 (attack), D01-6 (audit) | hard-coded ext table = rules-in-code; `ImportMap` provenance dishonest for a path join | Step 1 (data file), Decision E |
| D01-2 (attack), D01-5 (audit), PER-8 | no precise tier ever supersedes a File→File Imports edge; confidence is terminal | Decision E (documented as terminal) |
| D01-4/D01-5/D01-10 (audit/attack/repro), PER-6 | second `Imports` edge per statement; consumers count both | Decision F + Step 5/6 |
| PER-1 | blast-radius walks File→File; transitive importer Files become dependents | Decision G + Step 4 |
| PER-2 | File/Import nodes surface as hotspots; graph-view hides them after the fact | Decision H + Step 5 |
| PER-3 | bench gate skipped | Step 10 |
| PER-4 | confidence invisible on the blast-radius path | Step 4 adds `file_dependents_excluded` diagnostic; per-dependent confidence recorded as not-in-scope (§6) |
| PER-5, D01-4 (attack), D01-7 (audit) | incremental: target rename/delete prunes the importer's edge with no re-park | Step 7 (re-extract direct importers via the new edges) + rename test |
| D01-7 (attack + repro:impl) | `export … from`, `require()`, `import()` produce no ref; Python `relative_import` unmatched | Step 3 (JS/TS/TSX captures); Python deferred (§6) |
| D01-11 (audit) | edge must carry the ref's `Location` to count as resolved | Step 2 test asserts `edge.location == ref.location` |
| D01-13 (audit) | labelled repos + Windows separators untested | Step 2 tests (label prefix root guard; `\` importer path) |
| D01-9 (repro:impl) | duplicate-site refs stay "unresolved" by construction | Measured by `(file, spec)` pairs (§5); accounting fix belongs to the unresolved-accounting lane |
| D01-8 (audit) | `all_nodes()` empty → silent no-op | Step 2: resolver logs once when the index yields zero File nodes |

---

## 2. Decisions (every brief decision point, with evidence)

### A. Exact resolution only, with a root guard
Join `parent_dir(importer) + spec`, normalise segment-by-segment; a `..` that would pop below the
repo root **parks** the ref. Suffix matching is deleted from this path entirely.
Evidence: `../../../../escape/x` from `src/deep/nested/esc.ts` binds `escape/x.ts` under the
review patch (`edge-corpus/src/deep/nested/esc.ts:1`, `resolve/src/lib.rs:293`), and `./foo2`
binds `site/src/foo2.ts` (`lib.rs:274`).
**Root under a repo label:** the resolver takes the scope prefix (`Some("<label>/")` or `None`) at
construction — `wicked-estate/src/lib.rs:916` already has `scope` in hand where the slice is built.
The guard counts `..` pops against the importer's depth **below the prefix**, so
`../../repoa/src/b` from `repoa/src/a.ts` parks exactly as `../../src/b` from `src/a.ts` parks
(keeps `multi_repo.rs:186-200` plain == labelled). Deriving the root structurally from the File-node
population was rejected: an unlabelled repo whose files all live under `src/` would be
indistinguishable from a label and would false-bind (risks lens, high).
**Own parent-dir helper:** `parent_dir("index.ts") == ""`, `parent_dir("a/b.ts") == "a"`, splits on
both `/` and `\` (stored paths are `/`-normalised at `lib.rs:326-331`; the `\` arm is defence).
`dir_of` (`lib.rs:72-79`) is not called — the resolver-precision lane owns it.

### B. Conventions are DATA in a resolver-owned file, not in `languages.toml`
New file `crates/wicked-estate-resolve/import-conventions.toml`, `include_str!`-embedded, parsed
once at resolver construction with `#[serde(deny_unknown_fields)]`:

```toml
# Relative-specifier resolution conventions, keyed by the IMPORTER's language (Node.language).
[[language]]
name        = "typescript"                    # also rows for "tsx" and "javascript"
known_exts  = ["d.ts", "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"]  # stripped from the spec, longest first
probe_exts  = ["ts", "tsx", "d.ts", "js", "jsx", "mts", "cts", "mjs", "cjs"]  # extensionless spec: priority order
index_names = ["index"]
[language.remap]                              # explicit spec ext -> candidate exts, priority order (tsc nodenext)
js  = ["ts", "tsx", "d.ts", "js"]
jsx = ["tsx", "jsx"]
mjs = ["mts", "mjs"]
cjs = ["cts", "cjs"]
```
`javascript` row: `probe_exts = ["js","jsx","mjs","cjs","ts","tsx"]`, `remap.js = ["js","ts"]`
(Node semantics for a JS importer; tsc semantics for a TS importer — the `./q.js` with both
`q.js` and `q.ts` fork is decided per importer language, by data, and tested both ways).
Why not `languages.toml`: the resolve crate cannot read it without a new `resolve → extract`
crate edge (`resolve/Cargo.toml`), the file is regenerated by a script that would drop new keys
(`languages.toml:1`, `gen-language-manifest.py:16`), and `LanguageSpec` silently ignores unknown
keys (`extract/src/lib.rs:145-156`). Adding `toml = "0.8"` to the resolve crate (same version as
extract) is the only dependency change. A test in `wicked-estate` (which depends on both crates)
asserts every `language.name` in the conventions file exists in `registry()`. **No per-language
Rust arms**: the resolver's algorithm is language-blind; every difference is a table row.
**Table provenance (ATT-INV-1):** each row's `probe_exts`/`remap` order is transcribed from the
documented TypeScript resolution order (TS handbook "Module Resolution", `--moduleResolution
node16/nodenext`: a `./x.js` specifier probes `x.ts`, `x.tsx`, `x.d.ts` before the literal;
extensionless relative specifiers under `bundler` probe `.ts/.tsx/.d.ts` then directory
`index.*`), and the tables are NOT self-certifying — S8's independent oracle
(`ts.resolveModuleName`) adjudicates every ambiguous join point against the real compiler, so a
wrong row shows up as an oracle disagreement, not a vacuous pass.

### C. Candidate side: a full-path set, family via data, deterministic priority, ambiguity parks
The brief asks for a map keyed by extension-free path; the plan uses the simpler, strictly
stronger `HashMap<String /*full stored path*/, SymbolId>` over `NodeKind::File` nodes, built once
per `resolve` call from `index.all_nodes()`. Every probe is an exact full path
(`stem.ext`, `stem/index.ext`), so **no candidate-side extension stripping exists** — the
`.d.ts` / multi-dot / `rsplit_once` defect class cannot recur, and `foo.test.ts` is never confused
with `foo.ts`. Probe order for an importer of language L (row from B):
1. spec has ext `e` ∈ `known_exts` (longest-first match): try `stem.<remap[e]…>` if `e` has a
   remap, else the literal `stem.e`;
2. spec has an ext **not** in `known_exts` (`.css`, `.json`, `.svg`): try the literal path only
   (File nodes exist for css/json — studio has 6/5 — and a stylesheet import IS a dependency for
   blast-radius). Never extension-probe these.
3. extensionless spec: `stem.<probe_exts…>`;
4. then `stem/<index_names>.<probe_exts…>` (covers `./c` → `c/index.ts`; explicit `./index` and
   `./utils/index` go through 3 because the spec already names `index`).
The first priority slot with ≥1 hit wins. Ambiguity within a slot cannot arise from this map:
duplicate full stored paths are structurally impossible (`Symbol::file(path)` derives the id from
the path, `core/src/symbol.rs:110`, and the store keys nodes by id), so `HashMap<String,
SymbolId>::insert` never sees a colliding key from a real index — asserted with a
`debug_assert!` on insert and stated here instead of a park-on-duplicate test that could only be
constructed against a hand-built index (ATT-INV-4). What CAN tie is two different probe paths in
the same slot both existing (e.g. `remap.js = [ts, tsx, …]` with both `q.ts` and `q.tsx` on
disk): within a slot the ext list is itself ordered, so every probe is a distinct priority — a
genuine same-priority tie is therefore only possible if a future table row lists the same ext
twice, guarded by a conventions-load validation (`no duplicate exts within a list`). Candidates
are ordered by (slot, path) — never by map iteration order (`resolve_all` already returns
`HashMap` order, `lib.rs:845`, so a first-wins bug would be invisible).
Cost: ≤ ~14 hash probes per ref, O(files + refs). No family filter on the target is needed
beyond the extension lists themselves.

### D. Bare specifiers, aliases, Python: out of this resolver
Specs not starting with `./` or `../` are skipped (npm packages, `tsconfig.paths` aliases like
`@/x`, `#internal`, `package.json#exports/main`). Recorded in §6 so recall numbers are not read as
bugs. Python `from . import x` is a `relative_import` grammar node with package-dir semantics —
`python.scm` belongs to the extraction-gaps lane and the `.`-normaliser would be per-language
logic: **deferred**, D01-7.

### E. Confidence / provenance: `ImportMap` tier, dedicated id, overridden confidence 0.9
`Edge::new(importer_file, target_file, Imports, ResolutionTier::ImportMap, "relative-import")`,
`.with_location(ref.location)`, then `edge.confidence = 0.9`, `metadata.via = "relative-path"`,
`metadata.rule ∈ {"literal","remap","probe","index"}`.
- Not `Parsed`: Parsed = single-file AST fact (`edge.rs:31`); this is a cross-file join.
- Not a new tier/provenance variant: serde on-disk strings, `arb_resolution_tier`
  (`property_tests.rs:37-47`), every `match` on the enum, and older binaries deserialising
  `edges.data` (`neighbors()` errors → R1 abandonment). The per-edge override under `ImportMap` is
  the pattern `ScopedNameResolver` (`lib.rs:213-214`) and `ImportMapResolver` (`:384-386`) already use.
- Why 0.9 not 0.6: the join is deterministic and will be adjudicated 100% on disk (§5); the
  contract's 0.6 is the tier **default** for "import-map heuristics" (`ENGINE-CONTRACT.md:51`),
  and this edge is terminal — no precise tier ever emits `Imports`
  (`scip_edges` emits only Calls/References). Why not 1.0: the resolver cannot see
  `tsconfig.paths`/`moduleSuffixes`, symlinks, or a case-insensitive FS; 0.9 lands in the bench
  "high" band (`capability.rs:386`), deliberately distinct from `exact`.
- Doc: one line added to `docs/ENGINE-CONTRACT.md` §3 under the tier table stating the override
  AND its dedup consequence explicitly (ATT-INV-6): relative-import edges carry 0.9 and therefore
  win `resolve_all`'s max-confidence dedup against a Tsg-default (0.8) `Imports` edge by design —
  a future TSG/SCIP `Imports` emitter must exceed 0.9 or revisit this decision (docs-only; no
  other lane owns the file).

### F. Representation: coexist; the synthetic `File → Import` edge stays; consumers filter by kind
Retargeting/replacing the synthetic edge is not expressible in a `Resolver` (returns `Vec<Edge>`,
`traits.rs:45-51`; local edges are already written, `lib.rs:881`), and the Import node is shared
by every file importing the same text (219 such targets in studio) — deleting or retargeting it
for one importer breaks the others. Making Import nodes per-file changes SymbolIds for all 77
languages, orphans `annotations.node_sym` / memory `about` edges keyed to them, and violates the
fixture contract that requires Import nodes for the `imports` cap
(`extract/tests/language_integration.rs:560-586`). Neither is smaller than the coexist option.
What "two Imports edges per statement" does to each consumer, and what this lane does about it:
- **rank / hotspots:** File/Import nodes are dropped from `ranked_symbols` results (Decision H);
  mass that flows File→Import / File→File never reaches code symbols (File→def is `Contains`,
  which rank ignores, `rank/src/lib.rs:156`), so symbol ordering moves only through global
  normalisation — measured before/after (§5), not asserted.
- **blast-radius:** Decision G.
- **Lineage (Dependencies over Calls+Imports):** a File start now lists the Import node AND the
  resolved File for each relative import — that is the feature. Symbol starts are unaffected
  (symbols have no `Imports` out-edges).
- **graph-view degrees:** File/Import nodes are never selected (`main.rs:1524-1535`), so the
  extra edge is invisible there; unchanged.
- **entrypoints / leaves / isolated, memory, xedges:** node side excludes File; unaffected
  (PER-12, measured 1669/1669 crew, 2480/2480 studio).
- **communities / cluster_summary:** File→File edges add File-only structure; before/after
  community counts recorded (§5). One code change (ATT-INV-3, previously claimed "none"):
  `cluster_summary.rs:105` uses `ranked_symbols(store, &[], usize::MAX)` as a score-LOOKUP table
  feeding each community's `top_symbols` (`:163`, `unwrap_or(0.0)`), so the Decision H filter
  would silently zero File/Import members' scores and degrade Import-heavy communities to
  id-ordered arbitrary exemplars. cluster_summary therefore switches to the crate-internal
  UNFILTERED score fn (Decision H), preserving today's exemplar behavior byte-for-byte; a test
  pins it.
No consumer-side "dedup" of the two edges is required once File/Import are filtered from rank
results and File transit is handled in blast-radius; nothing else counts edges per statement.
§8 note: this is an addition, not a migration — nothing is replaced, so nothing is deleted at the
representation level. The deletions in this change are the graph-view post-hoc File/Import
exclusion (Step 5) and the WAVE-PLAN W2.6 overstatement (Step 7).

### G. Blast-radius: contains-aware File transit rule for non-File starts (revised, FEAS-1)
The locked decision (`query.rs:44-50`, `DESIGN-NOTES.md:16-20`) is about **edge kinds**; it is
kept verbatim — no edge kind is excluded. What changes is result classification, in one shared
place: a new `Subgraph::code_dependents(&self, start: &SymbolId, start_kind: Option<&NodeKind>)
-> Vec<&Node>` in `crates/wicked-estate-core/src/query.rs` (additive, no trait/serde change).
**The first draft's depth≥2 rule was factually wrong** (FEAS-1): the Dependents walk advances to
`e.source` for EVERY edge kind (`store/src/lib.rs:634-637`; CTE `sqlite.rs:1373-1376` with
`edge_kinds=[]` from `query.rs:48-52`), and Contains is File→def, so with `f` in FileB and a
caller `g` in FileA, HEAD dependents(f) = {g@1, FileB@1, FileA@2} — FileA reached via
Calls(g→f) then Contains(FileA→g) with ZERO import edges. A depth≥2 File drop would have changed
existing symbol-start results and failed §5's cross-binary gate by design. Rule adopted instead:
- **For non-File starts:** keep a File node iff the subgraph contains a `Contains` edge from it
  to a reached non-File node (the start's own file and every caller's containing file pass;
  Files reached only as File→File import transit fail). Implementable from `Subgraph.nodes` +
  `Subgraph.edges` + `Subgraph.depths` alone (`core/src/query.rs:60-66`) — no store change.
- **For File starts:** keep everything, including every transitive importer File: garden's
  documented file-path `blast-radius` (`wicked-garden/skills/search/SKILL.md:110-125`) and
  archetype→playbook File edges become meaningful — review engine defect #8 turns from "inert"
  to "works".
Why this gives EXACT HEAD parity for symbol starts: from a File node the only in-edges are the
new `Imports` File→File rows, whose sources are Files — so the import edges never make a new
symbol or a new contains-holding File reachable; every File kept under this rule was already a
HEAD dependent via a Contains edge, and every newly-reachable File is import-only transit and is
dropped. Note: a kept File's **min-depth may shift** when an import edge offers a shorter path
(FileA@2 via Contains may become FileA@1 via Imports); the S4 tests therefore assert
dependent-SET equality, not per-row depths for File rows. **Confidence stats (BR-4, by design):**
MCP BlastRadius's `confidence.min/avg/edge_count` are computed over ALL traversal edges
(`retrieve/src/lib.rs:855-860`) and continue to be — they describe the traversal, not the
dependent list, so a symbol start's `confidence.min` may drop to 0.9 once import edges exist;
stated here and in §4 rather than silently shifting.
Callers switched to the helper: `wicked_estate::blast_radius_by_name` (`lib.rs:1108-1120`; CLI +
bench inherit) and MCP `BlastRadius` (`retrieve/src/lib.rs:827-832`), which also gains a
diagnostic `blast-radius: N importing file(s) excluded from dependents (File transit)` so R7/R3
consumers see the cut. **Output bound (BR-2):** MCP BlastRadius `dependents` — the lane's own
"File starts keep every importer" feature at depth 8 — goes through the existing
`cap_rows_to_budget` (`retrieve/src/lib.rs:1170`, already used at `:1131/:1307/:1428`) with the
DoD-A8-style loud truncation diagnostic; the CLI `--json` path (`main.rs:1320-1347`, one
uncapped row per dependent, parsed by crew `projects/graph.ts:921-931` via `execCapped` where an
oversized payload truncates and `JSON.parse` throws TODAY) gets the same 25K-char bound with an
additive `"truncated_dependents": N` key and a text-path `…and N more` line — additive JSON, an
improvement over the pre-existing parse-break. **Residual (stated, not hidden):** the store walk
still spends `max_nodes` budget on transit Files; on a 50k-file monorepo the 5000 cap could
truncate real symbol dependents. The fix is a node-kind stop in the store CTE + `MemStore` +
Postgres + conformance — not this lane's seam; recorded in §6 with PER-1 for a follow-up.

### H. Rank: filter `File` and `Import` at the `ranked_symbols` seam; no weighting (revised)
`ranked_symbols` (`rank/src/lib.rs:289-305`) drops `NodeKind::File | NodeKind::Import` ids before
truncating to `top_n` (nodes stay in the graph for mass flow). **Concrete mechanics (ATT-INV-3 /
FEAS-4 — the first draft's "build_graph returns the excluded id set" was unimplementable as
stated):** `build_graph` is a free fn invoked INSIDE `pagerank_inner` (`rank/src/lib.rs:139-145`,
`:191`), and `ranked_symbols` only ever sees `(SymbolId, f32)` pairs from `PageRank::rank`
(`:294-296`). The actual change: `pagerank_inner` — which already iterates `store.all_nodes()` —
additionally collects the `HashSet<SymbolId>` of File/Import ids (zero extra store passes) and
returns `(HashMap<SymbolId, f32>, HashSet<SymbolId>)`; the public `PageRank::rank` keeps its
`HashMap` signature as a thin wrapper; a crate-internal `PageRank::rank_with_excluded` feeds
`ranked_symbols`, which filters the pairs against the set before sorting/truncating. All changes
are crate-internal; no external caller's signature moves.
**Consumer table (complete — cluster_summary was missing, ATT-INV-3):**
| Consumer | Path | Effect |
|---|---|---|
| `pagerank.top` cache write (`wicked-estate/lib.rs:952-967`) | compute | filtered at write |
| `important_symbols` live fallback (`lib.rs:1215-1225`) | compute | filtered |
| `important_symbols` CACHE READ (`lib.rs:1201-1213`) | **cache** | **NOT fixed by the seam (BR-1)** — the loop `take(top_n)` + `get_node` has no kind check, so a stale pre-upgrade `pagerank.top` on an un-reindexed DB (crew onboarding graphs, garden graph.db) would keep serving File/Import rows. S5 adds the kind filter AT THE CACHE-READ SEAM (skip File/Import before counting toward `top_n`) so stale caches are cleaned at read time — the precondition for deleting graph-view's post-hoc exclusion. |
| CLI `rank`/`hotspots` (`main.rs:1445`), graph-view (`main.rs:1520-1524`), bench `query_symbol` (`capability.rs:287`) | via `important_symbols` | fixed once the cache-read seam is fixed |
| MCP `RankHotspots` (`retrieve:1256`), `blast_summary` (`:957`), `ContextBundle` ranking (`:1631`) | compute (live) | filtered immediately |
| **`cluster_summary`** (`rank/src/cluster_summary.rs:105`, `:163`) | score LOOKUP, not top-N | must NOT be filtered: `ranked_symbols(store, &[], usize::MAX)` feeds `pr_scores.get(s).unwrap_or(0.0)` for every community member's `top_symbols`; the filter would zero File/Import members and degrade Import-heavy communities to id-ordered exemplars. It switches to a crate-internal `ranked_symbols_unfiltered` (the pre-filter pairs), preserving today's exemplar behavior byte-for-byte; pinned by a test. |
| ContextBundle unranked-candidate tail (`retrieve:1649-1653`, BR-3) | append path | File/Import filtered when appending candidates absent from the ranked list, so importer/imported Files pulled in by the Both/depth-2/200-node gather don't pad the pack tail |
Weighting by confidence was rejected: the File→Import edges are `Parsed/1.0`, so no weight
removes the pre-existing Import pollution (crew #1 = `import/json/` at HEAD). The bench
`query_symbol` on crew **will change identity** for this reason alone — the plan owns that
receipt shift (§5).

**Round-1 correction (R1-CORR-2):** S5 originally deleted graph-view's `NodeKind::File` /
`NodeKind::Import` entries from `excluded` as "post-hoc" (§8), reasoning that `important_symbols`
never returns them after the cache-read filter. That reasoning covered only the seed/backfill
path: the shared `passes` closure is ALSO the BFS **expansion** gate (`main.rs`, the
Calls|Imports neighbor walk), and File nodes enter the frontier via file-scope Calls edges
(a bare top-level `f();` attributes the ref to the File symbol), then pull Import nodes and more
Files through Imports edges — including this lane's File→File edges. The entries are restored;
they are redundant for seeds but load-bearing for expansion. Regression pinned by
`crates/wicked-estate/tests/graph_view_cli.rs` (fixture verified to FAIL on the unfixed binary:
4 functions + 3 file + 2 import nodes; passes with the gate restored).

### I. Captures: three JS/TS/TSX forms as `.scm` data, same capture names
Added to `typescript.scm`, `tsx.scm`, `javascript.scm` in one commit (§11 lockstep), reusing
`@import` / `@import.source` so `classify_capture` (`treesitter.rs:1617-1623`) needs no change:
```scheme
(export_statement source: (string) @import.source) @import
(call_expression function: (import) arguments: (arguments . (string) @import.source)) @import
(call_expression function: (identifier) @_req arguments: (arguments . (string) @import.source)
  (#eq? @_req "require")) @import
```
plus, TS/TSX only: `(import_statement (import_require_clause source: (string) @import.source)) @import`.
`@_req` classifies as `CaptureRole::Other` and is dropped — harmless. The existing `require`
`Calls` ref (from the generic identifier-call pattern) is left as-is: it is a bare unresolved call
to a global, not a false edge. `extract_name_module_pairs` has no arm for these statement shapes,
so they fall into the `file_import_sources` fallback (`treesitter.rs:1945-1953`) — hints stay
correct, nothing new is parsed in Rust.

### J. Incremental: re-extract direct importers of DELETED targets only (revised, ATT-INV-2)
**Scope narrowed to DELETED files** (option (a) of the attack's fix). Re-verified store
semantics: `remove_file(target)` deletes edges `WHERE file=?1 OR source IN nodes-of-file`
(`sqlite.rs:1740-1756`) — the importer A's A→B edge has `file = A` and `source = A`'s File
symbol, so for a merely-MODIFIED target B the edge survives `remove_file(B)`, B's File node is
re-created under the same path-keyed SymbolId (`symbol.rs:110`) before `prune_dangling_edges`
runs at pipeline end (`lib.rs:970-981`), and the edge remains correct with **no importer
re-extraction needed**. Forcing importers on modified targets would have re-parsed every
importer of a hub file on every save (O(fan-in) per incremental index, unmeasured) to buy
nothing for this lane's edges. Deleted-only is sufficient for the mandated rename test, PER-5,
and D01-4 (a rename is delete-B + new-C).
**Mechanics (FEAS-3 / BR-5 — the loop must be restructured, not just annotated):**
1. **BEFORE** the deleted-removal batch (`lib.rs:683-696`): collect importer rel-paths of each
   deleted file via `store.neighbors(Symbol::file(path).id(), Dependents)` filtered to `Imports`
   edges whose source is a File node — the batch's `remove_file` + later `prune_dangling_edges`
   destroy exactly the edges this discovery walks, so ordering is load-bearing.
2. The forced set is computed **once, from the original deleted list only** — never from forced
   files — so there is no transitive cascade (importers-of-importers are untouched).
3. The changed/unchanged split (`lib.rs:698-710`) consumes `work` and today DROPS unchanged
   `FileWork` (only `unchanged_count += 1`), so "force into changed" must happen inside that
   split: a `fw` whose `rel` is in the forced set goes to `changed` even when its digest matches
   (digest ignored for this run). The `FileWork` is still alive at that point — no re-read
   needed. The stale-contribution `remove_file` for changed files (`lib.rs:718-724`) then purges
   and re-extracts importers via the existing pipeline.
Result: a renamed target B→C leaves an `unresolved_refs` row for A (`'./b'`); a modified target
keeps the edge with zero extra work. `WAVE-PLAN.md:106` (W2.6) is reworded to the true scope:
"delete/rename a file → its direct importers re-extract and re-park; modify a file → only it
re-resolves (importer edges survive by store semantics)". Module doc `lib.rs:18-25` rewritten
likewise. **Release-note line (BR-5):** the rename/delete re-park behavior depends on the
File→File edges existing, i.e. requires one full re-index after upgrade before it works on an
existing DB. Residual hole (stated): an importer whose ref was **parked** (target absent) is not
re-resolved when the target is later added — no edge exists to find it. Recorded in §6 (D01-7
audit).

---

## 3. Steps (files · change · test · deletes)

Each step is one commit on `lane/relative-imports`, `--no-verify`, `type(scope): …` + trailers.
Cargo is always `CARGO_TARGET_DIR=<lane>/target cargo … -p <crate>`.

**S0 — baseline arms (no code change).**
Files: none in-tree. Build the BEFORE bench binary at `d7d3b58`:
`cargo build -p wicked-estate-bench --release`, copy `target/release/wicked-estate-bench` to
`<lane>/measure/bench-before`. Clone `axios@v1.7.9` (`git clone --depth 1 -b v1.7.9
https://github.com/axios/axios <lane>/measure/axios`). Copy `edge-corpus`, `edge-corpus2` into
`<lane>/measure/`. Run BEFORE indexes (main release `wicked-estate`) of studio, crew, both corpora
into `<lane>/measure/*-before.db`; run `bench-before` on axios+studio+crew from the lane worktree
and **`git checkout -- docs/benchmarks`** afterwards. Record every command in §5.
Test: n/a. Deletes: nothing.

**S1 — conventions data + loader.** `feat(resolve): import-conventions.toml + typed loader`
Files: `crates/wicked-estate-resolve/import-conventions.toml` (new),
`crates/wicked-estate-resolve/Cargo.toml` (+`toml = "0.8"`), `crates/wicked-estate-resolve/src/relative_import.rs` (new module: `ImportConventions`, `LanguageConventions`, `parse_spec`).
Tests (resolve unit): `conventions_load_and_deny_unknown_fields`, `known_exts_longest_first`
(`foo.d.ts` → stem `foo`, ext `d.ts`; `foo.test.ts` → stem `foo.test`), `unknown_ext_is_literal`
(`x.css` → literal), `every_probe_ext_is_a_known_ext`; wicked-estate integration test
`import_conventions_languages_exist_in_registry`. Deletes: nothing.

**S2 — the resolver.** `feat(resolve): RelativeImportResolver — exact-path File→File Imports binding`
Files: `crates/wicked-estate-resolve/src/relative_import.rs`, `src/lib.rs` (`pub mod` + re-export only — no edits to existing resolvers/helpers).
Shape: `RelativeImportResolver::new(scope_prefix: Option<&str>)`; `id() = "relative-import"`;
`tier() = ImportMap`; `resolve`: build the full-path `HashMap` from `index.all_nodes()` File nodes
(log once via `eprintln!` if zero File nodes — D01-8); for each `Imports` ref whose de-quoted
spec starts with `./`/`../`: importer language from the index's File node (`Node.language`), row
lookup (skip if no row), `parent_dir` + root guard + normalise, probe per Decision C, emit per
Decision E. Own `parent_dir` helper; no `dir_of`, no `file_matches_module`.
Tests (resolve unit, `VecIndex` + `file_node(path, lang)` on `Symbol::file` + `rel_ref(from, spec)` with the QUOTED spec):
`./w` binds · `./q.js` → `q.ts` (TS importer) · `./q.js` → `q.js` when both exist (JS importer) and → `q.ts` (TS importer) ·
`./c` → `c/index.ts` · `./utils/index` and `./index` bind · `./foo.d.ts` binds · `./a` with `a.ts`+`a/index.ts` → `a.ts` ·
`./b` with `b.ts`+`b.css` → `b.ts` · `./styles.css` literal binds, `./styles` with only `styles.css` parks ·
`./foo2` with only `site/src/foo2.ts` parks · `../../../../escape/x` from `src/deep/nested/esc.ts` parks though `escape/x.ts` exists ·
root importer `index.ts` → `./config` binds · `../foo` from a root file parks · conventions-load rejects a duplicate ext within a list (ATT-INV-4; duplicate full paths are impossible per Decision C, `debug_assert`ed) ·
bare `react` skipped · `\`-separated importer path · labelled prefix `repoa/`: `../../repoa/src/b` parks and `./b` binds ·
unknown importer language row → skip · every edge: kind Imports, source = importer File symbol, target = File symbol, `location == ref.location`, `resolved_by == "relative-import"`, provenance `ImportMap`, confidence 0.9, `metadata.via/rule` ·
`resolve_all` keeps the 0.9 edge over a lower duplicate key · determinism: same input twice, identical output.
Deletes: nothing (the reference patch was never applied).

**S3 — captures.** `feat(extract): capture export-from, require(), import() and import=require as Imports refs (JS/TS/TSX)`
Files: `crates/wicked-estate-extract/src/queries/{typescript,tsx,javascript}.scm`; new fixture `crates/wicked-estate-extract/tests/fixtures/typescript/reexports.ts` (+ tsx/js twins) — existing `sample.*` fixtures and their quoted-literal assertions stay untouched.
Tests (extract): per language, refs with `raw_name` `"'./y'"` (export-from), `"'./z'"` (require), `"'./dyn'"` (dynamic import), TS/TSX `"'./req'"` (import=require); an Import node exists for each (keeps the `language_integration.rs:560-586` gate honest); `import_dedup_same_module_twice_ts` unchanged; count-pinning tests (`treesitter.rs:3317-3370`) unchanged because their fixtures have none of the new forms. Deletes: nothing.

**S4 — blast-radius contains-aware transit rule + output bounds.** `fix(core,retrieve): contains-aware File transit rule for symbol starts; bound BlastRadius output (R4)`
Files: `crates/wicked-estate-core/src/query.rs` (`Subgraph::code_dependents`, additive), `crates/wicked-estate/src/lib.rs` (`blast_radius_by_name` uses it), `crates/wicked-estate-retrieve/src/lib.rs` (BlastRadius uses it + new diagnostic + `cap_rows_to_budget` on `dependents` + ContextBundle unranked-tail filter at `:1649-1653`), `crates/wicked-estate/src/main.rs` (CLI `--json` dependents 25K-char bound + `truncated_dependents`).
Tests: core unit for the helper (File start keeps everything; symbol start keeps a File iff a Contains edge from it reaches a kept non-File node — the start's file AND a caller's containing file both kept, an import-only transit File dropped);
retrieve `blast_radius_unchanged_when_only_import_edges_added` (File A, File B, `f` in B, `g` in A calls `f`; dependent SET of f == {g, File B, File A} before AND after adding A→B Imports + A→Import-node edges — exact HEAD parity including the caller-container File per FEAS-1; File rows' depths not asserted, min-depth may legally shift; diagnostic present when a transit File is cut; confidence block NOT asserted equal — transit edges enter the stats by design, Decision G/BR-4);
retrieve `blast_radius_of_a_file_lists_importing_files`; retrieve `blast_radius_file_start_output_capped_under_r4` (a File start with wide importer fan-in stays < 25K chars with the truncation diagnostic present — mirrors the DoD-A8 pattern at `retrieve:4588-4592`, BR-2);
wicked-estate integration `blast_radius_size_unchanged_for_function_in_imported_file` (indexes a two-file TS repo, compares `blast_radius_by_name` against the same repo with the import line removed, asserting equal size — passes under the contains rule because both runs include the caller-container File); CLI: `truncated_dependents` asserted additive (crew reads only `dependents`/`unresolved`). Deletes: nothing.

**S5 — rank seam filter + cache-read filter.** `fix(rank): File/Import nodes never rank as hotspots; clean stale pagerank.top caches at read; drop graph-view's post-hoc exclusion`
Files: `crates/wicked-estate-rank/src/lib.rs` (`pagerank_inner` returns `(scores, excluded_ids)`; public `PageRank::rank` unchanged as a wrapper; crate-internal `rank_with_excluded`; `ranked_symbols` filters; crate-internal `ranked_symbols_unfiltered`), `crates/wicked-estate-rank/src/cluster_summary.rs` (`:105` switches to the unfiltered fn — behavior-preserving), `crates/wicked-estate/src/lib.rs` (`important_symbols` cache-read loop `:1201-1213`: skip File/Import kinds after `get_node`, BEFORE counting toward `top_n` — cleans stale caches at read time, BR-1), `crates/wicked-estate/src/main.rs` (remove `NodeKind::File`, `NodeKind::Import` from graph-view `excluded` — safe only after the cache-read filter).
Tests (rank): `file_and_import_nodes_never_in_ranked_results` (3 Files + 1 Import node with heavy fan-in + 2 Functions → top-N contains only the Functions); `cluster_summary_exemplars_unchanged_by_rank_filter` (an Import-heavy community's `top_symbols` are identical before/after the seam change — pins the unfiltered lookup, ATT-INV-3); existing `imports_edges_contribute_to_rank`/`non_call_import_edges_ignored` unchanged; wicked-estate `important_symbols_has_no_file_or_import_nodes` after `index_path` on a TS fixture; `important_symbols_drops_file_import_from_stale_cache` (seed a `pagerank.top` cache containing an Import id by hand, assert `important_symbols` never returns it and still fills `top_n` from the remaining rows, BR-1). Deletes: the two `excluded` entries in `main.rs:1524-1527`.

**S6 — wire the resolver.** `feat(index): bind relative JS/TS imports in the resolve slice`
Files: `crates/wicked-estate/src/lib.rs` — one `let relative = RelativeImportResolver::new(scope.as_deref());` above the slice literal and exactly ONE new line inside it (`&relative,` after `&ImportMapResolver,`); `docs/ENGINE-CONTRACT.md` one line under the tier table.
Tests: wicked-estate integration `relative_imports_bind_file_to_file` on a temp fixture = the edge-corpus layout UNION edge-corpus2's `./c` + `./foo2` cases PLUS an `import('./dyn')` and a TS `import r = require('./req')` line (FEAS-2: the read-only corpora contain NO dynamic-import or import=require site, so those two ref forms are proven by the S3 extract fixtures and this extended temp fixture, not by the corpora): expected 14 binds / 3 parks (`../../../../escape/x`, `../../../../../vv`, `./foo2`) — the per-line table lives in the test; `multi_repo.rs`: new `labelled_relative_imports_match_plain` (make_repo gains `src/util.ts` + `import './util'` and `import '../../repoa/src/util'`; plain == labelled edge_count and both park the escaping spec). Deletes: nothing in code.

**S7 — incremental importer re-extraction (deleted targets only) + rename test.** `fix(index): re-extract direct importers of DELETED files; target rename re-parks the importer`
Files: `crates/wicked-estate/src/lib.rs` (Decision J mechanics: importer collection BEFORE the deleted-removal batch at `:683-696`; forced-set membership honoured inside the changed/unchanged split at `:698-710` where the unchanged `FileWork` is still alive — it is dropped today, so the split is restructured to consult the forced set before discarding), module doc `:18-25` rewritten to state the true scope + remaining hole (parked ref + later-added target), `docs/plan/WAVE-PLAN.md:106` reworded per Decision J ("delete/rename → importers re-extract; modify → only the file itself; importer edges survive by store semantics").
Tests (`crates/wicked-estate/tests/e2e.rs`): `incremental_target_rename_reparks_importer` (index `a.ts` importing `./b`; assert File a → File b; rename `b.ts`→`c.ts`; re-index; assert no edge a→b, no dangling edge, and an `unresolved_refs` row for `a.ts` with raw_name `"'./b'"`); `incremental_target_modified_keeps_importer_edge` (modify `b.ts` in place; re-index; edge a→b intact AND a was NOT re-extracted — asserted via the change log carrying no `Remove`/re-add for `a.ts`, proving deleted-only forcing, ATT-INV-2); `incremental_delete_does_not_cascade` (a imports b, z imports a; delete `b.ts`; only a is forced — z's nodes untouched, single-pass collection per Decision J step 2, BR-5); `incremental_importer_of_new_target_stays_parked_until_touched` (documents the residual honestly as an assertion, not a skip). Deletes: the KNOWN LIMITATION paragraph that this step makes false (replaced by the narrower true statement).

**S8 — adjudicator + independent oracle + corpora measurements.** `chore(measure): on-disk adjudication of every relative-import edge` (scripts live in `<lane>/measure/`, not in-tree; results go into this doc's §5).
Two-part adjudication (ATT-INV-1 — a Decision-C-verbatim adjudicator alone would be circular:
the tie-break tables under test would grade themselves):
(a) **Exact-join existence check, ALL edges** (`adjudicate.py`): read the importer at the edge's
span, join `parent_dir + spec` with the root guard, verify the edge's target path exists on disk
at exactly that join point. Table-independent — catches the suffix/root-escape false-bind class
regardless of any priority row.
(b) **Independent compiler oracle, the AMBIGUOUS subset** (`oracle.mjs`): for every edge whose
join point has >1 on-disk candidate across the family's extensions (a.ts vs a/index.ts vs a.js;
`./q.js` remap; d.ts ordering), call `ts.resolveModuleName` from the corpus's own
`node_modules/typescript` (verified present in BOTH wicked-studio and wicked-crew
`node_modules/typescript/package.json`) with the repo's tsconfig, and compare its winner to the
edge's target. Every oracle disagreement is a MISS to classify — never explained away by the
plan's own tables. Spot-check any oracle-vs-table discrepancy with `tsc --traceResolution` on
the importer before classifying.
Miss taxonomy (both parts): no-File-node (gitignored/minified/symlink), case mismatch,
root-escape, alias, table-order wrong (oracle disagreement → fix the conventions row, re-run).

**S9 — complexity guard + timing.** The committed unit test is a deterministic OPERATION-COUNT invariant, not a wall-clock assertion (ATT-INV-5 — a debug-mode timing ceiling in the suite is a permanent flake liability under the no-`#[ignore]` rule): `twenty_k_files_hundred_k_refs_bounded_probes` instruments the resolver's probe counter (a `#[cfg(test)]` counter or a returned stat) on a synthetic 20k-File / 100k-ref `VecIndex` and asserts total probes ≤ refs × 14 and exactly ONE map build — the O(refs × files) regression class cannot pass it. Wall-clock lives ONLY in the §5 measurement protocol (release build, human-read): resolver phase on studio logged via a one-off `Instant` in the resolve block during measurement (not committed), plus the same synthetic corpus timed in release.

**S10 — bench receipts.** Run `bench-before` and the lane `--release` bench on `<axios> <studio> <crew>` from the lane worktree; diff `edges`, `resolver_breakdown`, `blast_radius_node_count`, `blast_radius_coverage_pct`, `index_ms`, `query_symbol`; **commit the regenerated `docs/benchmarks/capability-report.md` deliberately** as the receipt (single `chore(bench): receipts before/after relative imports` commit) or revert it — never leave it dirty.

Every step ends with: `cargo build -p <crate>` (0 warnings), `cargo test -p <crate>`,
`cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo fmt -p <crate>`; wicked-estate
integration suite (~5 min) after S4–S7; the exact counts go into the final report next to the
baseline counts in §0.

---

## 4. Compatibility + migration

- **Schema:** none. `edges` PK is `(source, target, kind)`; the new rows are ordinary `Imports`
  edges with `resolved_by = "relative-import"`, provenance `import_map`, confidence 0.9. Older
  binaries read them (no new enum strings).
- **Existing DBs** (crew onboarding graphs, garden `.wicked-estate/graph.db`) gain File→File edges
  only on a **full re-index** (`wicked-estate index` on an unchanged tree is a digest no-op).
  Incremental runs after the upgrade bind only changed files' imports, and the S7 rename/delete
  re-park depends on the File→File edges existing — both need one full re-index after upgrade
  (two release-note lines, BR-5).
- **Consumers, what moves:** CLI `blast-radius`/`--json` and MCP `BlastRadius`: dependent SETS for
  symbol starts unchanged (contains-aware rule, Decision G — exact HEAD parity including
  caller-container Files); File rows' min-depths may shift; the `confidence` block's
  min/edge_count move when import edges enter the traversal (by design, BR-4). File starts now
  return importers (new; crew `graph.ts:917-940` and studio `RepoGraphModal.tsx:147-177` pass
  kinds through and render them), bounded: MCP dependents capped under R4 with a truncation
  diagnostic; CLI `--json` gains an additive `truncated_dependents` key at the same 25K bound —
  crew's `JSON.parse` of `execCapped` stdout stops being breakable by oversized payloads (BR-2).
  `stats`/`health` `edges_by_kind[imports]` rises (studio ≈ +1.3k, crew ≈ +0.4k).
- **Rank consumers — compute-path vs cache-backed (BR-1):** compute-path consumers (MCP
  `RankHotspots`, `blast_summary`, `ContextBundle`, cache write) lose File/Import rows
  immediately; cache-backed consumers (`important_symbols` → CLI `rank`/`hotspots`, graph-view,
  bench `query_symbol`) are fixed **at the cache-read seam**, so a stale pre-upgrade
  `pagerank.top` on an un-reindexed DB is also cleaned — graph-view never regresses to rendering
  File/Import nodes, and CLI `rank` on an old crew DB stops showing `import/json/` at #1 without
  waiting for a re-index. crew's top symbol changes identity (owned in the receipt). `Lineage`
  from a File start lists resolved Files. `Communities`: counts move; recorded — but
  `cluster_summary` exemplars are UNCHANGED (unfiltered score lookup preserved, ATT-INV-3).
  Entrypoints/leaves/isolated, memory, knowledge, xedges: unchanged (measured).
- **Garden SKILL text** (`skills/search/SKILL.md:117-118`) promises per-edge confidence the CLI
  path does not deliver — unchanged by this lane; noted for garden.
- **Bench receipts** change on purpose (Decision H, new resolver row in `resolver_breakdown`
  "high" band); the committed report is the before/after evidence.

---

## 5. Measurements (protocol; numbers filled in by the executing agent)

BEFORE = `/Users/michael.parcewski/Projects/wicked/wicked-estate/target/release/wicked-estate`
(read-only). AFTER = `<lane>/target/debug/wicked-estate` (and `--release` for timing). DBs under
`<lane>/measure/`. Corpora: `wicked-studio`, `wicked-crew` (full repos), `edge-corpus`,
`edge-corpus2` (copied), `axios@v1.7.9` (bench). Every command recorded verbatim in the final
report; kinds are JSON strings (`'"imports"'`).

| Measurement | SQL / method | Pass condition |
|---|---|---|
| File→File Imports edges added | `select count(*) from edges e join nodes t on t.symbol=e.target where e.kind='"imports"' and t.kind='"file"'` before/after | > 0; studio/crew ≈ review's +1286/+424 or explained |
| Relative Imports refs unresolved, by **(file, spec) pair** and by raw row | `select count(distinct file||raw_name) … where kind='"imports"' and (raw_name like '''./%' or raw_name like '''../%')`; raw `count(*)` too, labelled "location-keyed (D01-9, lane E)" | pair count near 0; raw count may rise because S3 adds sites — state why |
| Adjudication (a): exact-join existence, EVERY new edge | `adjudicate.py`: read importer at the edge's span, join+root-guard, verify target exists on disk at the join point | 100%, or every miss classified |
| Adjudication (b): independent oracle, ambiguous subset | `oracle.mjs` (`ts.resolveModuleName` from the corpus's node_modules) over every edge whose join point has >1 on-disk candidate | 0 oracle disagreements, or each one classified as a table fix + re-run (ATT-INV-1) |
| Edge corpora (read-only, per-corpus tables — FEAS-2) | edge-corpus: 11 binds / 2 parks (`../../../../escape/x`, `../../../../../vv`) / 2 new ref forms (export-from `'./y'`, require `'./z'`); edge-corpus2: 12 binds (adds `./c`→`c/index.ts`) / 3 parks (adds `./foo2`) / 2 new ref forms. Dynamic `import()` + `import=require`: NOT in the corpora — proven by S3 fixtures + the S6 extended temp fixture | every line matches |
| Blast-radius regression | `blast-radius <fn in imported file>` before/after (CLI --json, dependent set); plus the S4 tests | identical dependent SET (size + membership; File-row depths may shift, Decision G) |
| BlastRadius R4 ceiling | MCP File-start blast-radius on studio's widest-fan-in file | < 25K chars, truncation diagnostic when cut (BR-2) |
| PageRank top-20 | `pagerank.top` cache joined to nodes, before/after, studio + crew; PLUS `important_symbols` against the untouched BEFORE db opened by the AFTER binary | 0 File/Import in after top-20; 0 File/Import from the stale cache (BR-1) |
| Communities | `clusters` count before/after; one Import-heavy community's `top_symbols` before/after | counts recorded; exemplars identical (ATT-INV-3) |
| Resolver timing | studio release run, resolve phase wall-clock; S9 synthetic corpus timed in release (wall-clock lives HERE, not in the unit suite — ATT-INV-5) | seconds, not minutes |
| Bench | `bench-before` vs lane `--release` on axios+studio+crew | receipts diffed; only expected fields move |

### Results (measured 2026-08-28 by the executing agent; commands in the lane report)

BEFORE = main checkout `target/release/wicked-estate` on `measure/*-before.db`; AFTER = lane
`target/{debug,release}/wicked-estate` on `measure/*-after.db`. Adjudication scripts:
`<lane>/measure/adjudicate.py` (part a) + `<lane>/measure/oracle.mjs` (part b,
`ts.resolveModuleName`, typescript 5.9.3 from wicked-studio's node_modules).

| Measurement | studio | crew | edge-corpus | edge-corpus2 |
|---|---|---|---|---|
| File→File `relative-import` edges added | **+1,362** | **+430** | 11 | 12 |
| Unresolved relative pairs (distinct file+spec) | 1,291 → **42** (−97%) | 426 → **20** (−95%) | 11 → 2 | 13 → 3 |
| Unresolved relative rows (location-keyed, lane E) | 1,311 → 77 | 439 → 28 | 11 → 2 | 13 → 3 |
| Adjudication (a) exact-join existence | **1362/1362** | **430/430** | 11/11 | 12/12 |
| Adjudication (b) ambiguous join points → oracle | 0 cases | 0 cases | 2/2 agree | 2/2 agree |
| `edges_by_kind[imports]` | 2,259 → 3,701 | 1,020 → 1,473 | 11 → 24 | 13 → 27 |

Notes: the raw-row counts rise vs the review's projection because S3 ADDS ref sites
(export-from / require / dynamic import) — the pair counts are the honest measure (lane E).
The real corpora have ZERO ambiguous join points (no `a.ts`+`a/index.ts` siblings, no
`x.ts`+`x.css` stem collisions at any bound join point), so part (b) ran on the synthetic
corpora's 4 ambiguous cases — 4/4 oracle agreement (`./a`→`a.ts` over `a/index.ts`,
`./b`→`b.ts` over `b.css`, both corpora).

Corpus per-line tables: edge-corpus **11 binds / 2 parks** (escape ×2); edge-corpus2
**12 binds / 3 parks** (escape ×2 + `./foo2`); `./c`→`c/index.ts`, `./q.js`→`q.ts`,
`./foo.d.ts`, `./index`, `./utils/index`, css/json literals all bound; export-from `'./y'`
and require `'./z'` produce refs AND bind. Dynamic `import()`/`import=require` proven by the
S3 fixtures + the S6 temp-fixture test (14 binds / 3 parks) — not present in the read-only
corpora (FEAS-2), as re-derived.

**Blast-radius cross-binary parity (the gate that caught FEAS-1's second defect):** the
contains-only transit rule shipped first DROPPED file-scope caller Files (top-level call
sites are attributed to the File symbol) — studio `apiBase` lost 27 test files (134→107).
Rule corrected to any-non-Imports-source-edge (commit `07d4c24`); after the fix the dependent
SETs are IDENTICAL cross-binary: studio `apiBase` 134/134, `threadKey` 88/88, `makeView`
110/110; crew `missingArtifacts`, `run_bounded` identical. File-start blast-radius on
`src/api/client.ts` (studio) returns 238 importer files, bounded: 164 kept +
`truncated_dependents: 74` under the 25K CLI bound.

**Rank:** AFTER top-20 has **0 File/Import** rows on studio AND crew (BEFORE: studio 4/20,
crew 5/20 with `import/json/` at #1). Stale-cache live check: the lane binary on the UNTOUCHED
`crew-before.db` serves `Method delete` at #1 — `Import json` is cleaned at cache-read (BR-1);
the BEFORE binary on the same db still shows it. crew's top symbol changes identity as owned.

**Consumers:** entrypoints (2480/2480 studio, 1669/1669 crew) and dead-code (2149/2149,
1604/1604) unchanged. Leaves +28 studio / +5 crew — NEW Import nodes minted by the S3 capture
forms (an effect of capture coverage, not of File→File edges). Communities: 73→58 studio,
31→29 crew (recorded; exemplar behavior pinned by the ATT-INV-3 test).

**Timing:** studio relative-import resolve phase (release, one-off `Instant`, reverted):
**2.64 ms** for 44,233 refs → 1,435 edges (dedup to 1,362 stored). The committed S9 guard:
20k files / 100k refs, probes ≤ refs × 14, exactly one map build; release run of that test
finishes in under a second (see lane report). The review's O(refs × files) projection for this
corpus was ~2.3 s.

**Bench (S10):** `bench-before` (built at d7d3b58) vs the lane `--release` bench on
axios v1.7.9 + studio + crew — receipt committed as `docs/benchmarks/capability-report.md`
(+ new `coverage-matrix.md`). Only expected fields moved: `resolver_breakdown` gains the
`relative-import` row in the HIGH band (231 / 1362 / 430 edges @ 0.900), `unresolved_ref_count`
falls, crew `query_symbol` json→delete (owned), studio `blast_radius_node_count` unchanged
(171, same start). axios `blast_radius_node_count` 98→108 at the bench's DEPTH-3 cap is a
horizon effect — import edges shorten paths, pulling caller-container Files inside the cap;
the depth-12 dependent SET of the same symbol is identical cross-binary (156 rows). Bench
suites green (10 unit + 5 integration).

**Deviations from the plan's letter (recorded):** (1) the S3 per-form assertions live in a NEW
integration test file `crates/wicked-estate-extract/tests/js_ts_import_captures.rs` rather
than in `treesitter.rs`'s unit module — the extraction-gaps lane owns `treesitter.rs`, and S3's
file list allowed only `.scm` + fixtures, so the tests went into an additive file. (2)
`multi_repo::labelled_relative_imports_match_plain` builds its own fixture instead of extending
`make_repo` — the shared fixture's file set is pinned by the other tests' assertions. (3) The
S4 contains-only transit rule was replaced mid-lane by the any-non-Imports-source-edge rule
(commit 07d4c24) after the cross-binary gate caught file-scope caller Files being dropped —
Decision G's parity argument now rests on "every HEAD File dependent is the source of the
non-Imports edge the walk reached it by". (4) `leaves` counts rise slightly (+28 studio,
+5 crew): the S3 capture forms mint NEW Import nodes (leaves), an effect of capture coverage,
not of File→File edges — entrypoints/dead-code are byte-identical.

---

## 6. Not in scope (with finding ids)

- Per-dependent confidence / `resolved_by` in CLI `--json` and MCP `BlastRadius` (PER-4): dependents
  are nodes; path-min confidence is a separate change. The S4 diagnostic covers the File cut.
- Store-level node-kind stop in traversal (PER-1 residual): `max_nodes` budget spent on transit
  Files; needs SQLite CTE + MemStore + Postgres + conformance — other seam.
- `unresolved_refs` location-keyed over-count and `resolve_all` telemetry key (D01-9 repro, D01-11
  audit, engine defect #3): unresolved-accounting lane.
- `file_matches_module`'s relative branch still has the `..`-underflow and suffix defects for
  `Calls` import-map scoping (D01-4 baseline): unowned by any lane — flagged in merge notes (§11).
- Python `relative_import` (D01-7): deferred, `python.scm` is the extraction-gaps lane's file.
- `tsconfig.paths`/`baseUrl` aliases, `package.json` `main`/`exports`, directory imports without
  an index file: bare/alias specifiers are outside "relative imports".
- Importer whose ref was parked and whose target is added later (D01-7 audit residual of J).
- Re-resolving importers' `Calls` refs when a target file is merely MODIFIED (pre-existing
  limitation, `lib.rs:18-25`): Decision J deliberately scopes forcing to DELETED targets
  (ATT-INV-2) — this lane's File→File edges need nothing more; the broader Calls staleness is a
  separate change with its own O(fan-in) cost/measurement question.
- Shared-by-specifier Import node identity (`repo_scope.rs:22-27` wart): unchanged.
- Pre-existing ignored doctests in resolve and extract (engine defect #5): reported as unchanged.

---

## 7. Falsifier

The plan is wrong if any of these holds after S10: (1) any adjudicated File→File edge points at a
file the on-disk TS/Node rules would not pick — where "TS/Node rules" means the INDEPENDENT
`ts.resolveModuleName` oracle for every ambiguous join point, not the plan's own tables (a single
unexplained miss or unexplained oracle disagreement fails "100%", ATT-INV-1); (2)
`blast_radius_by_name` of a function in an imported file changes its dependent SET (size or
membership) between the same repo with and without the import line, using the same binary — AND
the cross-binary §5 row diverges (the contains-aware rule makes HEAD-vs-lane parity the claim,
FEAS-1); (3) a File or Import node appears in the after top-20 of studio or crew, or is served
from a stale pre-upgrade cache by the lane binary (BR-1); (4) the resolve phase on studio exceeds
2 s in release, or the 20k/100k synthetic corpus exceeds probes ≤ refs × 14 / one map build
(ATT-INV-5); (5) `multi_repo::unlabelled_indexing_is_unchanged` or the new labelled test
diverges; (6) any pre-existing test count drops or a test is `#[ignore]`d; (7) a MCP File-start
blast-radius payload exceeds 25K chars, or an Import-heavy community's `top_symbols` change
identity (BR-2, ATT-INV-3); (8) modifying (not deleting) an imported hub file forces any importer
re-extraction (deleted-only scope, ATT-INV-2).

---

## 8. Merge notes for the other lanes

- **Slice literal** (`crates/wicked-estate/src/lib.rs:923-928`): this lane adds ONE line inside the
  literal (`&relative,`) plus one `let relative = RelativeImportResolver::new(scope.as_deref());`
  two lines above it. The resolver-precision lane adds its own single line; merge is textual.
- **`lib.rs` regions touched:** `:18-25` module doc, `:683-710` (importer collection before the
  deleted-removal batch + forced-set handling inside the changed/unchanged split — deleted
  targets only, Decision J), `:916-928` (slice), `:1108-1120` (`blast_radius_by_name`),
  `:1201-1213` (`important_symbols` cache-read kind filter, BR-1). NOT touched: `:937-946`
  (unresolved accounting — lane E). Conflicts with lane E are file-level only.
- **Core:** one additive method on `Subgraph` in `core/src/query.rs` (contains-aware, reads only
  `nodes`/`edges`/`depths`). No trait, enum, or serde change; `TraversalSpec::blast_radius`
  untouched.
- **Rank crate:** `pagerank_inner` signature (crate-internal) gains the excluded-id set; public
  `PageRank::rank` unchanged; `cluster_summary.rs:105` switches to the crate-internal unfiltered
  score fn (behavior-preserving).
- **Retrieve regions:** BlastRadius dependents (`:826-847`, transit rule + `cap_rows_to_budget`),
  ContextBundle unranked tail (`:1649-1653`, kind filter). `main.rs`: blast-radius `--json` 25K
  bound + graph-view `excluded` trim.
- **Extract:** only `typescript.scm`, `tsx.scm`, `javascript.scm` + new fixtures under
  `tests/fixtures/{typescript,tsx,javascript}/`. `languages.toml`, `treesitter.rs`, and every
  other `.scm` untouched. `LanguageSpec` untouched.
- **Resolve:** new module + `pub mod`/re-export in `lib.rs`; `dir_of`, `NameResolver`,
  `ScopedNameResolver`, `ImportMapResolver`, `file_matches_module`, `normalise_relative_path`,
  `RulesBridge`, synthesizer untouched. New dependency `toml = "0.8"`.
- **Unowned defect to assign (§11):** `file_matches_module` relative branch (`resolve/src/lib.rs:262-274`,
  `:287-300`) keeps the false-bind class for `Calls` scoping; the resolver-precision lane is the
  natural owner since it edits the neighbouring helpers.
- **Docs:** `docs/ENGINE-CONTRACT.md` (+1 line), `docs/plan/WAVE-PLAN.md:106` (W2.6 wording),
  `docs/benchmarks/capability-report.md` (regenerated receipt). Nobody else claims these.
- **Integration order:** if lane E's unresolved definition lands first, re-run the §5 unresolved
  counts on the merged base; the pair-keyed count is the one that survives either order.

## Round-1 review fixes (2026-08-28)

- **R1-CORR-1 / RI-R1-1 (blocking):** `parse_spec_ext` byte-sliced `last_seg[1..]` — any
  relative specifier whose last segment leads with a multi-byte char panicked and aborted the
  whole index run. Now char-wise (`chars().skip(1)`); unit tests cover bind/park/known-ext/
  unknown-ext/dotfile non-ASCII forms (`relative_import.rs` loader + resolver tests).
- **R1-CORR-2:** graph-view `excluded` restoration — see the Decision H correction above.
- **RI-R1-2:** the Decision B registry cross-check test promised by this plan (and claimed in
  the S1 implementer report) had never been written — `language_names()` was orphan pub API.
  Now real: `import_conventions_languages_exist_in_registry` +
  `tsx_and_js_importers_bind_through_the_real_registry` in
  `crates/wicked-estate/tests/relative_imports.rs`. The S1 report's tests_run claim was wrong.
- **REV1-IMPORT-START:** `Subgraph::code_dependents` zeroed blast-radius for an IMPORT-node
  start on untouched pre-upgrade DBs (every reached File's only source-edges are Imports →
  all dropped; HEAD returned 95 importer Files for `react` on studio). Import nodes now take
  the keep-everything arm with File — the importing files ARE the blast radius of a dependency
  node. Core unit test: `import_start_keeps_importer_files` (query.rs). Cross-binary §5 check
  (Import-node start added to the protocol): `blast-radius react` on an UNTOUCHED copy of
  `studio-before.db` — HEAD release binary 95 Files, lane debug binary 95 Files (parity
  restored; the pre-fix lane binary returned "no resolved dependents"). On `studio-after.db`
  (File→File edges present) the lane binary returns 240 Files — direct plus TRANSITIVE
  importers, the documented File/Import-start semantics change, all rows File nodes.
