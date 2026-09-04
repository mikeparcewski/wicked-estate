# Changelog

## [Unreleased]

_Nothing yet._

## [0.16.1] — 2026-09-03

### Added
- **`SqliteStore::checkpoint_truncate()` + `WalCheckpointStats` (store).** A TRUNCATE WAL
  checkpoint that is a strict subset of `compact()` — no pruning, no `VACUUM` — cheap enough for
  the single-writer actor's idle tick. Two-phase (`PASSIVE` → `TRUNCATE`) with the busy handler
  disabled during the attempt, so a concurrent `open_readonly` holder (gate-hook subprocesses)
  makes it return `busy: true` and defer — it never blocks the writer thread and never disturbs a
  reader's snapshot. One-line forwarders on `MemoryEngine` and `KnowledgeEngine` (and a
  default-no-op `MemStore::checkpoint_truncate` returning the `-1` no-WAL sentinel stats on
  non-WAL backends such as Postgres), so all three estate WALs (`core`/graph, `mem`, `knowledge`) can be checkpointed by
  their owning engines.

### Changed
- **`PRAGMA wal_autocheckpoint=512` on `SqliteStore::open()`.** Halves SQLite's default
  1000-page threshold as the backstop against WAL starvation when passive auto-checkpoints keep
  landing while readers ride the WAL — observed in the field as `-wal` files outgrowing their
  databases (core.db 3.35MB vs 4.19MB WAL).

## [0.16.0] — 2026-09-02

### Fixed
- **Multi-file symbol contributions in the store (#152 / #153).** A symbol legitimately contributed
  by more than one file (a header prototype + its definition) no longer flaps `nodes.file`
  last-write-wins, no longer loses the surviving definition when one contributing file is removed
  (`remove_file` now deletes CONTRIBUTIONS and re-homes the node to its definition-preferred
  primary, generalizing the Import keep/re-home precedent), and the digest skip is harmless by
  construction afterward. New `node_files` contribution table on all three backends (shared
  contract suite; migration seeds one contribution per existing node on open, conflict-safe under
  concurrent opens); SQLite's FTS shadow is rebuilt on re-home. Zero SymbolId churn.
  **Upgrade note:** the per-repo `indexed_version` gate forces a full re-extract on each repo's
  first index under 0.16.0, which repopulates full multi-file provenance.

### Added
- **D6d: C++ free-function prototype emission (#140), landed under the recorded M4 identity decision — Option A, one logical symbol (ADR-002 third amendment, which carries the decision record and the measured S11 evidence).** Free-function prototypes at translation-unit / namespace scope (`int freestanding();` in a header) now emit nodes — header-heavy C/C++ codebases regain their declared API surface (S11, measured on the bench tree-sitter corpus: +222 prototype nodes, 192 of them the `ts_*` public C API; on that C-layout corpus they mint standalone declaration-primary nodes — the id-JOIN class the store fix protects is same-module pairs, per ADR-002's third amendment). **Zero id churn — no scheme bump:** a prototype mints the SAME SymbolId its definition already mints (module strips one extension), JOINING the existing node; the store's multi-file contribution table (#152) records per-file provenance, keeps the DEFINITION contribution as the node's primary location/kind, and re-homes the node when either file is removed — the F7 flap/delete/digest-skip triad that deferred D6d is dead. A header-only prototype with no definition mints the id alone as a declaration-primary node. Mechanics: per-parent anchored pattern set in `cpp.scm` (translation_unit / preproc_ifdef / preproc_if / declaration_list / template_declaration — pure `.scm` data, per `docs/recon/extraction-gaps.md` §D6(d); `.h` routes to cpp, so C headers ride it), with the adversarial-review guards: body-local prototypes and body-local most-vexing-parse object declarations sit under compound_statement and match nothing. New generic capture role `@code_<kind>.decl` — identical identity to `.def`, marks the record as a DECLARATION contribution (`is_declaration` metadata; D6b member prototypes now ride it too, making member proto/def primaries definition-preferred instead of lexicographic). Accepted residuals recorded in ADR-002 + `cpp.scm` (TU-scope most-vexing-parse, preproc-inside-body leak, un-braced `extern "C"`, pointer-returning prototypes) — and two named NOT-fixed: the overload disambiguator still collapses `f()`/`f(int)` into one id (`identity_disambiguator_is_none`, a scheme change), and the `.h`(ts-cpp)/`.c`(ts-c) cross-grammar seam does not unify a C header prototype with its `.c` definition (pinned in `crates/wicked-estate/tests/free_proto_emission.rs`). The three M4-gated pins are flipped/retired per their embedded instructions.

## [0.15.2] — 2026-09-01

### Fixed
- **Parked references back-fill when their target arrives** (#141 / #150). A reference to a
  not-yet-indexed target parked forever — incremental indexing silently under-built the graph vs a
  full re-index. A later (re-)extraction of the missing definition now re-resolves parked refs into
  real edges (`BACKFILL: re-resolved N` in the ingest log), idempotently (edge counts stable under
  re-touch; differential-equivalent to a fresh full index), honestly on ambiguity (a same-basename or
  near-name arrival re-resolves nothing — the row stays parked), and scope-safely on labelled runs
  (repo B defining repo A's awaited name never consumes A's parked row). The back-fill support
  contract runs on all three backends (Mem/Sqlite always; Postgres gated on `TEST_POSTGRES_URL`).

## [0.15.1] — 2026-08-29

### Added
- **`rules.recall` MCP tool (arch-R2/AW-4)** — faceted, severity-ordered recall of conformance `Rule` nodes (`PAT-*`/`POL-*`), the wire twin of wicked-governance's Rust-only `recall_rules`. Facets: `language`/`layer`/`framework` (wildcard — a rule without the facet applies to all values), `severity`/`rule_type` (exact), `scope` (subtree prefix, e.g. `wiki:architecture`), `limit` (default 100, max 500 — truncation is a loud diagnostic). Retired rules are withdrawn; foreign `Rule` nodes (rules-engine extractor output) are counted, never errored on; an empty result is a diagnostic, never `isError` (R1). Raises the unconditional estate tool floor 10 → 11 (24 tools with all domains). Response-cacheable (pure graph read).
- **`knowledge.recall {scope_prefix}` + `knowledge.coverage {scope_prefix}` (arch-R5/AW-8)** — optional subtree filter mirroring `memory.recall`'s wire contract exactly (omitted/`null` = no filtering, the previous behavior; `""` = root subtree = everything; a present non-string is `-32602`). The predicate is pushed into the FTS candidate query pre-`limit` (top-k isolation) and re-checked on every hydrated candidate (covers the vector lane). New knowledge writes stamp the canonical scope on `Node.scope`; nodes written before this release carry scope only in metadata and surface under a canonical-prefix scoped recall only via the vector lane (a trailing-colon kind-wildcard prefix such as `"wiki:"` skips the store pushdown, so FTS still sees them) — re-ingest for full scoped-FTS visibility. Scoping convention: wiki guidance ingests under `wiki:<area>` scopes with stable source URIs (`wiki://<page>#<anchor>` or repo-relative doc paths) and embeds the enforceable twin's `PAT-`/`POL-` id in chunk text.
- **ADR-012 (arch-R8/AW-11)** — the rule-authorship contract: git-tracked docs are the source of truth, promotion to an enforceable rule happens only via a human-merged doc PR + `wicked-core rules ingest`, and there is deliberately NO `rules.write` (or any rule-mutation) MCP tool. Conformance-tested against the actual advertised tool registry (`crates/wicked-estate-mcp/tests/rules_surface.rs`).
- **Plugin-manifest version guard (DT-14, #144)** — the Claude-plugin manifest (`plugins/wicked-estate/.claude-plugin/plugin.json`) had shipped `0.13.1` across the 0.14.x/0.15.0 releases because nothing bumped it and nothing checked it. New release guard `crates/wicked-estate/tests/plugin_manifest_version.rs` fails whenever the manifest version ≠ the workspace version — red on every `cargo test --workspace` and in the release workflow's guard step — and `.github/workflows/release.yml` now bumps the manifest alongside `Cargo.toml` and stages it into the release commit. `RELEASING.md` step 1b names both mechanisms. The marketplace listing (`.claude-plugin/marketplace.json`) carries the same plugin's version field, was missed by that fix, and had been stuck at `0.13.1` — re-synced to `0.15.1` here (not yet guard-covered; the guard checks `plugin.json` only).

### Changed
- **Rust API (breaking for 0.15.0 library consumers):** `KnowledgeApi::recall`/`KnowledgeEngine::recall` gain a `scope_prefix: Option<&str>` parameter; `KnowledgeApi::coverage` and `KnowledgeEngine::count`/`all_nodes` gain the same. MCP wire stays backward-compatible (the new params are optional). Under strict cargo 0.x semver a signature change calls for 0.16.0; this ships in 0.15.1 because the supported surface is the MCP wire — Rust consumers of the `wicked-estate-*` library crates pinning `^0.15` must update call sites on upgrade.

### Fixed
- **`RulesInventory` Rule-node blindness (arch-R2)** — the tool ignored individual `NodeKind::Rule` nodes while its description claimed "(RuleSet, Rule)". It now reports `rule_nodes: {total, in_rule_sets, ungrouped}`, emits a diagnostic pointing ungrouped rules at `rules.recall`, and its description matches what it returns.
- **Docs told the truth about 0.15.0 (#142, #144):** `docs/MIGRATION-0.15.md` documented symbol-id scheme 2 while 0.15.0 shipped scheme 3 (all four refs fixed; scheme 2 remains only as historical context, and the benchmarks re-baseline note is retitled to scheme 3). MCP onboarding (`docs/mcp-integration.md`, `plugins/wicked-estate/README.md`) claimed a single `cargo install wicked-estate` puts `wicked-estate-mcp` on PATH — corrected to `cargo install wicked-estate wicked-estate-mcp` (separate crates, one `[[bin]]` each). The migration runbook now states that the forced full re-extract PRESERVES injected/synthetic non-extractor nodes (delete paths are strictly file-keyed). `AGENTS.md`/`CLAUDE.md` agent anchors point at the real docs tree instead of a dead `./wiki` path. The product site scopes the team-runtime claim honestly (`wicked-estate-mcp` is SQLite-only today, fails loud under `team`) and gains a verified "What v0.15.0 ships" section.

## [0.15.0] — 2026-08-29

### Removed
- **`MethodResolutionSynthesizer` and the synthesizer precision monitor** (`measure_synth_precision`, `SynthPrecision`, `SYNTH_PRECISION_FLOOR`) from `wicked-estate-resolve`. The synthesizer's emit set was a strict subset of `ScopedNameResolver`'s Calls path at lower confidence (0.5 < 0.60), so it could never add an edge (0 synthetic edges across the studio/crew measurement corpora); it was never in any production resolver slice on any branch, and the precision monitor had no bench consumer despite WAVE-PLAN/ADR-007 claiming otherwise (see ADR-007's superseding note, 2026-08-28). `Provenance::Synthesizer` stays (overlay consumer). **Public-API removal: under cargo 0.x semver the next release must be `0.15.0`, not `0.14.7`.**
- **The edges-only `resolve_all` wrapper** from `wicked-estate-resolve` (#130). `resolve_all_with_coverage` is the single entry point — per-ref coverage accounting is not optional, and a second entry point that silently discarded it invited exactly the over-count #125 fixed. Second public-API removal confirming the 0.15.0 minor bump.

### Changed
- **Site (#121/#122/#124/#128):** scroll-snap no longer freezes the page and the platform band no longer overflows (#121); both band thresholds account for the fixed topbar (#122); the four strata sections fit the screen they snap to (#124); the released long tail is covered and the never-shipped "7 resolution tiers" copy is gone (#128).
- **Extraction gaps (review doc 04):** `.h` headers now route to the C++ grammar only (single owner in both the routing table and `languages.toml` — previously dual-listed under C); Swift gains `extends` capability; member prototypes, `attr_*` accessor methods and initializers are extracted as callable definitions; a def-name suffix channel strips call-site punctuation without touching the other name channels. Known cross-kind SymbolId collisions (Go const-vs-struct-field, C++ free-function-vs-member-method before scheme 2) are pinned as executable known-defect tests. Re-indexing affected repos adds behavior-bearing nodes.
- **Type-nested definition identity (symbol-id scheme 3, ADR-002 amendment — supersedes the unreleased scheme 2 in place; a released 0.14.x DB migrates 1→3 in one pass).** Definition SymbolIds now nest under their enclosing/owning types: `Repo.save` renders `…/Repo#save().` instead of `…/save().`. Previously every same-named definition in one module minted the SAME id and the store's `ON CONFLICT(symbol)` upsert collapsed them into one node — `this.save()` then "resolved" into the merged node at 0.65 (adversarial-review findings D03-1/D03-2/PER-7). Scheme 2 nested only under enclosing EMITTED definitions; scheme 3 adds two generic query-capture roles (`.anchor` non-emitting containment anchor, `.owner` spliced owner type name — pure `.scm` data, zero per-language Rust) so the following id classes ALSO churn from module-flat to type-nested: **Rust impl-block methods** (`impl Foo` and `impl Trait for Foo` both under `Foo#`), **Go receiver methods** (`func (r *T) M()` → `T#M().`), **Ruby singleton members** (`class << self` bodies and `def self.m` converge on `C#self#m().`, distinct from instance `def m`), **C++ out-of-line members** (`void Foo::reset()` → `Foo#reset().`, template qualifiers included), **TS/JS object-valued class fields** (the field is a Term def, so its literal members split module-flat instead of merging with the class's real methods), and **Python ORM fields** (real owner chain, no equal-range anchor truncation). Function-local and plain object-literal definitions deliberately stay module-flat (the chain truncates at any Method/Function/Term container); remaining residuals are fixture-pinned in ADR-002's "Accepted residuals", including the **live C++ cross-file hazard**: a header member prototype and its `.cpp` out-of-line definition now mint ONE id across TWO files, so `nodes.file` flaps last-write-wins, `remove_file` deletes by file, and the digest skip never re-extracts the survivor (`cpp_member_proto_def_cross_file_single_id_hazard`, pending the header/impl identity decision M4), and the **C++ namespace-qualifier cross-kind collision**: the qualifier grammar cannot separate class from namespace qualifiers, so a namespace-qualified free-function definition (`void ns::helper(int) {}`) mints the same id as an in-namespace `void helper() {}` definition with CONFLICTING kinds (Method vs Function) — the node's kind flaps last-write-wins (`cpp_namespace_qualified_free_fn_cross_kind_collision_known_defect`, folded into the same M4 decision). The cross-kind collisions pinned by the extraction-gaps entry above (Go const-vs-field, C++ free-function-vs-member) are flipped to distinctness regression guards by schemes 2/3. **Migration:** a previously-indexed repo is fully re-extracted on its next `index` (per-repo-label `id_scheme` meta key; loud stderr line; `--force` equivalent). The gate fires PER REPO LABEL and the warning is CLI-only — a multi-repo DB holds mixed schemes until every label re-indexes, and the MCP server surfaces nothing, so verify via the CLI or the per-label `id_scheme` keys. The key is written only after the re-extraction completes, so an interrupted migration re-fires the gate. On collision-heavy files, Calls edges drop and unresolved counts rise — the removed edges are the 0.65 false-precision merges, now honestly parked. NOT carried over: annotations on churned ids (orphaned), overlay/memory xedges (epoch-dropped — **re-inject overlay xedges / re-run the bus and command injection pipelines**, which typically land on handler METHODS, exactly the Rust-impl/Go-receiver classes scheme 3 churns), embeddings (re-run `--embeddings`), agent-held `--symbol` ids from `resolve --json` (stale), and SCIP edges — re-run `wicked-estate scip <root>` after the migration. Do not run pre-scheme binaries of the same version against a migrated DB (re-index `--force` if one did). **First index of each repo under 0.15.0 forces full re-extraction via the per-repo `indexed_version` gate; the `id_scheme` gate is the crash-idempotent same-version backstop — see the migration runbook: [`docs/MIGRATION-0.15.md`](docs/MIGRATION-0.15.md) and ADR-002 §Migration.**
- **Resolver precision (engine defect #2):** `NameResolver` no longer binds a ref to ANY unique same-name node — `Import` nodes are rejected as targets for every ref kind, `Calls` refs reject a kind deny-list (Interface/Trait/TypeAlias/Enum/Field/Parameter/File/Namespace + rules-engine kinds), and a cross-language-family guard (family table = `languages.toml` `family` field: typescript/tsx/javascript/svelte/vue = one `javascript` family) blocks e.g. python→typescript Calls edges. `ScopedNameResolver`/`ImportMapResolver` adopt the Import exclusion + family guard. `dir_of` returns `""` for root-level files, so two root files rank same-dir (0.62) and root-level `./x` import-map refs bind. `RulesBridgeResolver` is now wired into the `index` slice (`rules-engine:*` InvokedBy edges are produced under `index`; previously never). **Existing DBs keep stale edges until a full re-extract:** `index` only re-resolves changed files; the fix takes effect on the next `CARGO_PKG_VERSION` bump (which forces full re-extract) or a manual `wicked-estate index --force`.

### Added
- **Relative JS/TS import binding (`RelativeImportResolver`).** Quoted relative specifiers (`./x`, `../y`) in JS/TS/TSX now bind to their target File node as `Imports` edges (`resolved_by = relative-import`, ImportMap tier with a per-edge 0.9 override; exact joined-path match, root-guarded, ambiguity parks). Direct importers of DELETED files are re-extracted in the same `index` run so their refs re-park instead of silently losing edges; blast-radius gains a contains-aware File transit rule and `ranked_symbols` filters `File`/`Import` nodes. See `docs/ENGINE-CONTRACT.md` §3.1.

### Fixed
- **Shared `Import` nodes no longer dangle when their owning file is deleted — target-aware `remove_file` (#132).** `remove_file` deleted every node owned by the file, including a `NodeKind::Import` node that OTHER files' edges still referenced — those edges dangled until the next full re-index. The store seam is now target-aware: an Import node with surviving inbound edges (one survivor predicate, computed once per call) is kept and re-homed instead of deleted. The `GraphStore` conformance kit gained the shared-import battery (incl. history + FTS-row and batch-delete variants) and every store impl passes it.
- **Code call sites no longer bind to JSON data keys — data-language family rows (#133).** `json` had no `languages.toml` row, so the cross-language-family guard could not see it and `NameResolver` bound e.g. every zod `.optional()` call to a JSON key named `optional` (402 such Calls edges on the closure corpus). `json` and the IaC logical languages now carry their own manifest rows with own-name families — the guard closes the class as data, zero resolver code. Same PR: `unresolved_refs` rows gain `start_byte`/`end_byte` columns (explicit ALTERs on sqlite + postgres) so a parked site is byte-exact, not line-only. The manifest count lands at 114 languages.
- **'Unresolved' was over-counted — repeat sites of a resolved relationship were persisted as unresolved.** Edges dedup to one row per `(source, target, kind)`, but persistence marked every ref whose exact location was not on a surviving edge as unresolved: the 2nd..Nth call from one function to one target, and repeat imports of one module, were all written to `unresolved_refs` (wicked-studio: 6,317 of 38,536 unresolved Calls rows were this artifact; `blast-radius apiFetch` reported "49 unresolved call(s)" for a fully resolved relationship — now 0). The definition now lives in one place (`docs/ENGINE-CONTRACT.md` §2.1: a reference is unresolved iff no resolver emitted an edge attributed to it — same `(location, kind)` — per site) and is computed once, in `resolve_all_with_coverage`, for persistence, the telemetry counter (which under-counted and now rises), blast-radius coverage, and `stats` (which now prints `unresolved=N`). Genuinely unbound references keep every per-site row. Existing graphs are fully rebuilt on the next `index` under the new version; a same-version binary (dev builds) must `wicked-estate index <path> --force` once — an ordinary incremental re-index only rewrites changed files and would silently mix the two definitions.

## [0.14.6] — 2026-08-25

### Added
- **Many repos in ONE graph — `wicked-estate index <path> --db <f> --repo <name>`** (alias `--as`). A labelled run namespaces every path it stores as `<name>/…`, which is what makes `files.path`, `nodes.file` and the path-embedded SymbolIds unique per repo; it scopes the delete-sweep and the resolver's candidate set to that repo, and records provenance under `repo:<name>:commit|branch|remote|dirty` instead of the singular `repo_*` keys (which no longer clobber). `stats` reports every repo with its own file count and git state. No schema migration. Omit `--repo` and behaviour is unchanged — proved by `tests/multi_repo.rs::unlabelled_indexing_is_unchanged`. **Co-location, not linkage: edges do not resolve across repos.** `wicked-estate scip` takes the same `--repo` (SCIP documents are repo-relative; against a multi-repo graph an un-scoped ingest correlated nothing and reported `0 precise edges` — it now refuses).

### Fixed
- **Silent destruction of the first repo when two were indexed into one `--db`.** `files.path` is a relative path and SymbolIds embed it, so a second repo sharing `src/index.ts` overwrote the first's rows and the delete-sweep removed the rest — no error, no warning, `query alpha` → 0 matches. Indexing now REFUSES, before writing anything, any run that would overwrite another repo's content: an un-labelled second repo, an un-labelled index into a labelled graph, a `--repo` label already bound to a different repo, or the same repo under a second label. Every refusal names the conflict and the fix. Repo identity is the git `origin` remote **plus the indexed root's position inside the work tree** (so a moved or re-cloned checkout is one repo, while two packages of one monorepo — one `origin`, both with `src/index.ts` — are two and each needs its own label), and the canonical root path outside git.

## [0.14.5] — 2026-08-12

### Added
- **`WICKED_RUNTIME` profile seam (foundation team profile):** one switch flips the foundation between `local` (zero-infra SQLite, the default) and `team` (self-hosted shared Postgres via `WICKED_STORE_URL`). New `resolve_store_spec`/`resolve_store_spec_from` in wicked-estate-store (priority: explicit `--db` > team profile > `WICKED_ESTATE_DB` > default; unknown profiles and team-without-postgres-URL fail loud). Wired into the `wicked-estate` CLI (new opt-in `postgres` feature builds the factory arm in) and `wicked-estate-mcp` (which fails loud under team — its async graph path and memory/knowledge stores are SQLite-only today; named follow-up). `deploy/docker-compose.team.yml` + `docs/team-runtime.md` document bring-up and the honest coverage matrix. Functional gate: `tests/team_runtime.rs` runs profile resolution → factory → full GraphStore conformance against a real Postgres in the CI postgres job.

### Fixed
- **PostgresStore torn read (locked decision #8):** `begin_batch`/`commit_batch` now map to ONE real transaction at `READ COMMITTED` — previously they were no-ops (`transactional_batch: false` under `shared_writers: true`), so a concurrent reader could observe a partially-written graph batch. All statements issued while a batch is open (reads included) ride the batch transaction, preserving read-your-own-writes for the resolver's mid-batch `SymbolIndex` lookups. `StoreCapabilities.transactional_batch` is now `true`. New deterministic two-connection concurrency test (`postgres_batch_commits_atomically_no_torn_reads`) proves a mid-batch reader sees old-or-new, never partial.
- Postgres conformance cleanup now also drops `symbol_gen`, so re-runs against the same database no longer inherit sticky `had_node` markers that skew epoch assertions.

## [0.14.2] — 2026-07-30

### Added
- `blast-radius --json` — machine-readable impact: target, dependents (id/name/kind/file/1-based line), and the honesty contract's unresolved-call count. JSON mode suppresses staleness/version notices so stdout is exactly one document.
- `graph-view --focus <symbol>` — seed the connected slice at a named symbol (exact-name match, up to the display limit); `--focus` without a value errors instead of silently no-oping.

## [0.14.1] — 2026-07-29

### Fixed
- `graph-view` returns a CONNECTED slice: seed with the top-ranked core, expand breadth-first along Calls/Imports edges (same filters, one 6-expansion budget per frontier node), backfill from the ranking — a plain top-N-by-PageRank slice rendered as scattered islands (observed: 51 nodes / 23 edges; now 50 / 63). `--limit 0` returns an empty slice instead of panicking.

## [0.14.0] — 2026-07-27

### Added
- `graph-view` CLI subcommand — symbol-level code graph rendered via the estate service
- SLC-001 store connection-lifecycle integration test (WAL mode + clean drop), hardened through three review rounds

### Changed
- DoD criteria checked off against existing evidence artifacts (docs); site astro 6 → 7

## [0.13.0] — 2026-07-06

### Added
- Unified MCP server: single binary exposes 23 tools across estate, memory, and knowledge domains
- Absorbs wicked-memory (6 tools: memory.capture/recall/reflect/erase/learn/coverage)
- Absorbs wicked-knowledge (7 tools: knowledge.ingest/write/relate/recall/coverage/relate_code/recall_about_code)
- Absorbs wicked-overlay (XedgeStore cross-engine search layer)
- wicked-estate-memory-core crate: MemoryApi trait, CaptureRequest, RecallQuery types
- wicked-estate-memory-api shim crate for clean re-exports
- SC-009 integration test: all 4 v0.12.x fixture DBs open in unified server
- Schema conformance tests (CONF-*) against frozen v0.12.x golden schemas
- ENV-001..006 subprocess environment variable tests
- 6 skill bundle resources accessible via MCP resources/list
- expedition prompt via MCP prompts/get

### Changed
- Workspace version: 0.12.0 → 0.13.0
- MCP server tool count: 10 estate tools + 6 memory + 7 knowledge = 23 total
- Default DB path: `.wicked-estate/graph.db` (relative to CWD)

### Deprecated
- wicked-memory: absorbed into wicked-estate; repository will be archived
- wicked-knowledge: absorbed into wicked-estate; repository will be archived
- wicked-overlay: absorbed into wicked-estate; repository will be archived

## [0.12.0] — 2025-12-01

Initial public release with 10 estate tools via MCP.
