# Recon plan — plugin-override lane (user parser-plugin overrides)

Base: `764622f` on `feat/plugin-override`. Planner synthesis of four recon lenses
(history / consumers / tests / risks); every load-bearing citation below re-verified against
files opened in this worktree this session. Governing findings: the additive-only precedence
("a plugin never shadows a built-in", `crates/wicked-estate-extract/src/plugin.rs:20-21`,
`PLUGIN.md:26-28`) is commit-message law (`de24d66`, "built-ins always win"), not ADR law — no
ADR among ADR-001..008 (`ls docs/adr/`) covers the runtime plugin system, so the override design
is a NEW decision record, not an amendment. Every doc-04 gap was query-level and unpatchable
without a release; those specific gaps are now closed in-tree (`bda76b7` = HEAD~1), but the class
remains: a user cannot patch the next query gap, swap a newer grammar, or claim an extension
without a release.

## 1. Findings acted on (citations = files opened in this worktree)

| # | Finding | Evidence |
|---|---|---|
| F1 | Additive-only precedence is exactly two fall-through sites: `for_language` checks LANG_TABLE, falls to `plugin::find_by_name` only on miss; `extractor_for_extension` matches LANG_TABLE extensions and **delegates to `for_language(entry.name)`**, falling to `plugin::find_by_extension` only on miss. One hook inside `for_language`'s LANG_TABLE branch therefore covers both built-in dispatch paths. | `treesitter.rs:1218-1231` (Query::new `.ok()?`, plugin fallback), `:1263-1276` (`return TreeSitterExtractor::for_language(entry.name)`) |
| F2 | The current broken-query failure mode is silent graph **deletion**, not just "language dies": `Query::new(..).ok()?` returns `None` (`treesitter.rs:1216-1218` doc, `.ok()?` in `for_language` and `from_grammar`), the index pipeline `filter_map`s the `None` out of `ext_map` (`wicked-estate/src/lib.rs:678-687`), files of that extension fail the `supported` filter (`lib.rs:701-719`), and previously-indexed files are classified deleted and purged. No warning fires anywhere on that path. An override MUST NOT inherit this. | files opened; recon (consumers lens) traced the deleted-classification at `lib.rs:744-779` |
| F3 | Digest precedent: `load_extra_edge_rules` digests sorted raw name+bytes of every rule file — "ANY edit — including one that breaks parsing — changes the digest" — compared against per-repo `repo_scope::meta_key(repo, "extra_rules_digest")`, mismatch → `force_full`. But its key is written **at the check site** (`lib.rs:617`). | `lib.rs:367-419` (fn + `paths.sort()`), `:604-617` |
| F4 | The id_scheme gate (ADR-002 amendment, this very base commit) explicitly critiques check-site writes: "Writing at the check site would let a crash mid-run leave a DB stamped with the new scheme whose rows are still old — permanently mixed... Written last... idempotent." Key written only at run end (`lib.rs:1083-1087`) and the gate-guarded no-change early return (`:798-806`). | `lib.rs:619-644, 798-806, 1083-1087` |
| F5 | The plugin registry is a process-wide lazy `OnceLock` (`plugin.rs:91-96`); env is snapshot at first access; the existing test pins one-test-per-process (`tests/plugin_loader.rs:8-9` per recon, structure confirmed). `LoadedPlugin` caches `query_src` for the process lifetime (`plugin.rs:70-77`). | `plugin.rs` full read |
| F6 | `PluginManifest.library` is a required `String` (`plugin.rs:50`) and `load_one` unconditionally `resolve_library` + dlopens (`plugin.rs:137-155`); a query-only override (shipped grammar, user query, **no shared library**) cannot parse or load today. Manifest is permissive serde — an `override_query` typo would be silently dropped. | `plugin.rs` full read |
| F7 | `load_all` iterates `read_dir` with no sort and `find_by_name` returns first match (`plugin.rs:99-101, 111-131`) — two dirs overriding one language would pick a nondeterministic winner; any registry-order digest would flap. | `plugin.rs` full read |
| F8 | `plugins list` prints name/exts/license only, free-form (`main.rs:3456-3487`); recon (consumers lens) found **no machine parser** of this output anywhere in crew/garden/studio — free to extend. Crew DOES regex-parse `stats` **stdout** (`graph.ts:226-230`, recon) — stats format and stdout generally must not change; all notices go to stderr (MCP is stdio JSON-RPC). | `main.rs` opened; consumer facts from recon |
| F9 | An override changes the node/edge **set**, not identity: SymbolIds derive from the logical name path + role maps (`treesitter.rs:1354` `SYMBOL_ID_SCHEME`, `:1356` `def_suffix`, `:1454` `def_nodekind`); constructs captured by both queries keep identical ids. Digest-forced re-extraction is the correct mechanism; **no** `SYMBOL_ID_SCHEME` bump. | files opened; ADR-002 |
| F10 | Fixture premise verified at base: `typescript.scm` (226 lines) and `tsx.scm` contain **no** `internal_module`/`module_declaration` pattern (grep, 0 hits) — TS `namespace Util {}` mints no node today — while the `namespace` role already maps with zero Rust change (`treesitter.rs:1359` → `Suffix::Type`, `:1463` → `NodeKind::Namespace`). The doc-04 constructs are unusable as fixtures (closed by `bda76b7`, the merged extraction-gaps lane — `docs/recon/extraction-gaps.md` is in-tree at base). | greps + files opened this session |
| F11 | Built-ins' only guard against a broken query is the compile-time test `every_wired_query_compiles` (`treesitter.rs:2950`), which can never cover user-provided files. | file opened |
| F12 | Crew's clean-HEAD refresh skip fires on git evidence alone (`wicked-crew graph.ts:618-639`, recon consumers lens); override files live outside every repo, so an override edit produces no git evidence — crew serves the stale graph until a force refresh. The digest gate is necessary but not sufficient for crew-managed graphs. | recon (consumers lens); operational rule recorded in ADR + PLUGIN.md, crew change out of scope |
| F13 | Baselines green at 764622f (recon tests lens, commands recorded there): `cargo test -p wicked-estate-extract` = 362 unit + 108 integration + 3 doctests, 0 failed; `cargo test -p wicked-estate` = 51 + 22 + 80, 0 failed. Counts go stale with new test files — re-run and restate at the end. | recon (tests lens) |

## 2. Decisions (all explicit — no TBD)

**D1 — New `docs/adr/ADR-009-plugin-overrides.md`, not an amendment.** ADR-001..008 exist; none
covers runtime plugins (F-history). Style: ADR-002-amendment convention — a `Resolves:` line
citing the review lineage (doc-04: every gap query-level, unpatchable without a release), quote
the two superseded sentences verbatim (`plugin.rs:20-21`, `PLUGIN.md:26-28`) and `de24d66`'s
"built-ins always win", then state the three-tier precedence:
**built-in < query-only override < full grammar override**. The ADR names the terminology split
explicitly: "runtime grammar plugin" (this feature, PLUGIN.md) vs the W6.1 "extractor plugin"
(`.wicked-estate-extractors/`, `Provenance::Extractor`) — adjacent digest keys, different features.

**D2 — Query-only override activation = manifest-only (`override_query = "<lang>"`), as the brief
mandates.** The hermeticity exposure (a populated `~/.wicked-estate/plugins` silently changes
built-in-language `cargo test` results on that machine) is real but mitigated, not re-designed:
(a) loud stderr notice on every index run and at registry load; (b) every NEW test in this lane
pins `WICKED_ESTATE_PLUGINS` to a controlled temp dir; (c) the exposure is named in PLUGIN.md and
the ADR. Existing language tests are not edited (F13 counts stay comparable; see merge notes).

**D3 — Digest write timing = write-LAST (id_scheme discipline), not the extra_rules check-site
write.** The brief says "mirror extra_rules_digest"; the repo's own newest precedent documents why
the check-site write is a crash hole (F4, `lib.rs:626-635`). Mirror the INPUT design (F3: sorted,
raw bytes, empty-when-none, per-repo `repo_scope::meta_key`); adopt the id_scheme TIMING: write
the key at run end (`lib.rs:1083-1087` region) and in the gate-guarded no-change early return
(`:798-806` pattern). ADR records this divergence with the `lib.rs:626-635` citation.

**D4 — Digest input = the registry's CACHED bytes (what extraction will actually use), never a
fresh disk read at index time.** A disk-read digest + OnceLock-cached query (F5) would stamp a NEW
digest over a graph extracted with the OLD query in any long-lived process (watcher, MCP) —
permanently wrong. Cached-bytes digest keeps old-query/old-digest consistent; a restart picks up
both together. "Restart to pick up override edits" is documented in PLUGIN.md. (Tests are
subprocess-based anyway — D14 — so no test needs in-process reload.)

**D5 — One per-repo meta key, `plugin_overrides`, is both the gate and the audit record.** Value =
canonical descriptor of the EFFECTIVE override set: sorted lines
`<lang>|<mode:query|grammar>|<plugin dir basename>|<sha256[..16] of cached query bytes (+ dylib
bytes for grammar mode)>`; empty string when none active. Gate compares the whole value (mismatch
→ `force_full` + loud stderr). Covers every honesty case with one mechanism: .scm edit, dylib
swap, override added/removed, env-var flip (grammar mode is in the effective set only when armed),
broken override falling back (drops out of the effective set → re-extract under built-in). Absent
key ≡ empty ≡ no overrides → pre-feature DBs and no-override users force nothing. This satisfies
the brief's "recorded so extraction provenance is auditable" without touching Edge/Node — recon
(consumers) confirmed nothing filters on `resolved_by = "tree-sitter"`; changing it buys nothing.

**D6 — Digest scope on change = `force_full` (all languages), mirroring extra_rules.** A
language-scoped force has no precedent, needs its own override-removed purge logic, and saves
little; force_full is simple and honest. Cost (first index after any override change re-extracts
everything; lands on crew's 10-min bound once) recorded in the ADR.

**D7 — Broken override .scm: compile EAGERLY at registry load against the built-in grammar; on
failure eprintln `QUERY-OVERRIDE: <lang> override at <dir> failed to compile: <err> — using
built-in query` and register the override as failed-with-reason.** `for_language` /
`extractor_for_extension` never return `None` for a built-in language because of an override
(F2 is the scar). A failed override is OUT of the effective set (D5), so a graph previously
extracted under it honestly re-extracts under the built-in. Additional cheap guard: a compiled
override whose query contains zero recognized `@code_*`/`@call` capture roles gets a loud warning
(compiles-but-useless case). The existing silent `.ok()?` for built-ins and additive plugins is
NOT widened in this lane (scope; see not-in-scope).

**D8 — Override semantics = wholesale query replacement.** The override .scm REPLACES the built-in
query for that language (users start by copying the shipped .scm and adding patterns). Merge/append
semantics would need pattern-level identity and dedup rules — complexity with no brief requirement.
Documented in PLUGIN.md; the fixture test proves replacement (an override missing a built-in
pattern loses that construct — asserted).

**D9 — Override matches per LANG_TABLE entry, not per language family.** `typescript` and `tsx`
are separate entries; overriding one does not touch the other. Per-family grouping would need new
data; per-entry is honest and simple. PLUGIN.md documents the caveat (patching TS usually means
two override dirs, one per entry). `override_query` stays a single string per manifest (brief's
shape).

**D10 — `WICKED_ESTATE_PLUGIN_OVERRIDE` = comma-separated exact language names; no wildcard; read
once at first registry access (OnceLock semantics pinned in the ADR).** Belt-and-braces means
explicit naming. Separator is comma (never `:`/`;` — platform-dependent). Full grammar override of
lang L is armed iff `override = true` in that plugin's manifest AND L appears in the env list.
Extension claims of built-in-owned extensions are honored ONLY for an armed grammar override —
query-only overrides can never add or claim extensions.

**D11 — Two plugin dirs overriding the same language: disable BOTH, loud stderr naming both
dirs.** First-match-wins over unsorted `read_dir` (F7) is nondeterministic; deterministic refusal
beats a silent arbitrary winner. `load_all` additionally sorts entries by path (mirrors
`lib.rs:383` `paths.sort()`) so the registry, `plugins list`, and the D5 descriptor are stable.

**D12 — Manifest evolution: `library` becomes `Option<String>`; `load_one` enforces "library
required unless `override_query` is set" with the existing loud per-plugin skip.** New fields:
`override_query: Option<String>`, `override` (bool, default false; serde-renamed, `override` is a
Rust keyword). Unknown manifest keys are captured via `#[serde(flatten)]` into a map and warned
about by name (so `override-query` typos are visible) — NOT `deny_unknown_fields`, which would
break existing user manifests carrying stray keys. The nginx example manifest stays byte-valid.

**D13 — Notices: stderr only, two sites.** (1) Registry load (`load_all`): per-plugin
load/skip/override-compile-failure notices — once per process, the existing eprintln convention
(`plugin.rs:127`). (2) The index gate site in `lib.rs` (reached by every index run): per-run
`query override active: <lang> <- <plugin dir>` / `GRAMMAR OVERRIDE active: <lang> <- <plugin
dir>` lines plus `PLUGIN-OVERRIDE state changed: forcing full re-extraction` on mismatch — the
EXTRA-EDGE/VERSION-CHANGE house style. Never stdout (MCP stdio protocol; crew parses stats
stdout, F8). CLI arms that never construct extractors (stats/query/resolve) stay silent; the ADR
says so, so "loud startup notice" is not read as "on every invocation".

**D14 — Tests are one process per env configuration; anything asserting stderr or the digest gate
spawns the CLI binary (`CARGO_BIN_EXE_wicked-estate`, the `index_bad_path_cli.rs` pattern).** F5
makes multi-scenario env testing in one process order-dependent by construction, and an in-process
edit-then-reindex digest test would be wrong-by-construction (cached query vs edited file). The
full-grammar 'both signals' integration case inherits the `cc`-skip pattern from
`plugin_loader.rs`; the gating decision itself is a pure function with in-crate unit tests so the
double-opt-in logic is proven even without `cc`.

**D15 — Fixture = TypeScript `namespace` (`internal_module`).** Verified missing at base (F10),
zero Rust change needed for its roles (F10), owned by no other lane (extraction-gaps merged at
HEAD~1; its scope was Go/Ruby/Java/C#/Swift/C++ + `.h` routing). A control test pins the premise:
built-in typescript extraction of the fixture yields NO namespace def (with `WICKED_ESTATE_PLUGINS`
pinned to an empty temp dir) — if someone later adds `internal_module` to `typescript.scm`, the
control fails loudly instead of the override test passing vacuously.

## 3. Steps

Ordering: S1 (ADR) and S2 (registry) first — S3-S5 program against S2's public functions.
Each step compiles green per-crate (`cargo build/test/clippy -p <crate>`, lane CARGO_TARGET_DIR).

**S1 — ADR-009.**
- Files: `docs/adr/ADR-009-plugin-overrides.md` (new).
- Change: precedence model (D1), safety rules (D7 fallback, ABI gate unchanged, D10 double opt-in,
  D11 duplicates), digest design + write-last divergence from extra_rules (D3-D6), OnceLock/restart
  semantics (D4, D10), per-entry matching (D9), replacement semantics (D8), the crew clean-HEAD
  operational rule (F12), the hermeticity exposure (D2), terminology disambiguation (D1).
- Tests: n/a (prose); its claims are pinned by S6/S7 tests.
- Deletes: nothing (the superseded sentences are deleted in S2/S6, same PR).

**S2 — Manifest + override registry (`crates/wicked-estate-extract/src/plugin.rs`).**
- Files: `plugin.rs`.
- Change: D12 manifest fields + flatten-warn; sort `load_all` entries (D11); new
  `QueryOverride { lang, dir, query_src, compiled: Result<(), String> }` registry list — compiled
  eagerly against the built-in grammar at load (D7), loud on failure, zero-role warning; grammar
  override arming as a pure fn `grammar_override_armed(manifest_flag, env_list, lang)` (D10, D14);
  duplicate-override refusal (D11); public seam: `override_query_for(lang)`,
  `grammar_override_for_name(lang)` / `..._for_ext(ext)`, and `override_state() -> String`
  (the D5 canonical descriptor built from CACHED bytes, D4). Rewrite the module doc — delete
  "so a plugin never shadows a built-in" (`plugin.rs:20-21`) and correct the false
  "skip with a warning" scope note (query-compile failures now warn for overrides, D7).
- Tests: in-crate unit tests — manifest parsing (query-only / grammar / legacy nginx manifest
  byte-compat / unknown-key warning), arming pure fn (all four signal combinations), descriptor
  determinism (sorted, stable), duplicate refusal.
- Deletes: the `library`-required contract; the never-shadows doc sentence.

**S3 — Lookup precedence (`crates/wicked-estate-extract/src/treesitter.rs`).**
- Files: `treesitter.rs`.
- Change: inside `for_language`'s LANG_TABLE branch (`:1219-1227`): consult
  `plugin::grammar_override_for_name` (armed → plugin grammar + plugin query), else
  `plugin::override_query_for` (compiled-ok → built-in grammar + override query; failed → built-in
  query — never `None` because of an override, D7). In `extractor_for_extension`: before the
  LANG_TABLE ext match, honor an ARMED grammar override's extension claims (D10); everything else
  unchanged (built-in ext match already delegates to `for_language`, F1, so query-only overrides
  need no ext-site change). All precedence decisions live in the S2 plugin.rs functions — the two
  sites cannot drift.
- Tests: proven by S7 integration files (a)-(c); `every_wired_query_compiles` untouched.
- Deletes: nothing (the fall-through remains for non-built-in plugins).

**S4 — Digest gate + audit key (`crates/wicked-estate/src/lib.rs`).**
- Files: `crates/wicked-estate/src/lib.rs`.
- Change: next to the extra_rules gate (`:604-617`): read
  `repo_scope::meta_key(repo, "plugin_overrides")`, compare to `plugin::override_state()`;
  mismatch → `force_full = true` + D13 per-run stderr lines. Key written ONLY at run end
  (with the id_scheme key, `:1083-1087`) and in the gate-guarded no-change early return
  (`:798-806` pattern) — D3.
- Tests: S7(d) subprocess tests; label-scoped-key assertion in the same file (multi_repo.rs
  pattern, own test file — multi_repo.rs untouched).
- Deletes: nothing (additive gate; no schema change — meta is generic k/v).

**S5 — `plugins list` override column (`crates/wicked-estate/src/main.rs:3456-3487`).**
- Files: `main.rs`.
- Change: per plugin, append override status: `override=query(<lang>)`,
  `override=query(<lang>) FAILED: <err> — built-in in use`, `override=grammar(<lang>) [armed]`,
  `override=grammar(<lang>) [INERT — not named in WICKED_ESTATE_PLUGIN_OVERRIDE]`. Query-only
  override plugins (no library) are listed. Showing INERT matters: the double opt-in means a
  manifest alone must visibly do nothing.
- Tests: asserted inside S7(d).
- Deletes: nothing (no parser consumes the old format, F8).

**S6 — Docs.**
- Files: `PLUGIN.md`, `FEATURES.md`, `README.md`, `docs/add-lang.md`,
  `examples/plugins/nginx/README.md`.
- Change: rewrite the precedence paragraph (`PLUGIN.md:26-28`); new "Overriding a built-in
  language" section — both tiers, manifest examples, safety rules, replacement semantics (D8),
  per-entry caveat (D9), the re-extraction consequence + one-key audit trail (D5/D6), restart
  caveat for MCP/watcher (D4), crew force-refresh remedy (F12), hermeticity exposure (D2),
  CLI-vs-editor env-skew footgun (recon risks lens: shell exports the env, the editor-launched
  server does not → alternating full re-extracts). Delete every "never shadows" sentence
  (FEATURES.md:122-124, README.md:135-139, add-lang.md:12-14, nginx README:26-27 — per recon
  consumer sweep; grep at execution time to catch strays).
- Tests: n/a; `grep -rn "never shadows" --include='*.md' .` must return 0 hits (recorded).
- Deletes: the additive-only invariant from every doc surface.

**S7 — Tests (new files; each its own process; all pin `WICKED_ESTATE_PLUGINS`).**
- Files (new): `crates/wicked-estate-extract/tests/query_override.rs`,
  `.../tests/builtin_misses_namespace.rs` (control), `.../tests/no_shadow.rs`,
  `.../tests/full_override_gate.rs`, `crates/wicked-estate/tests/plugin_override_cli.rs`.
- (a) `query_override.rs`: temp plugins dir, manifest `override_query = "typescript"` (no
  library), .scm = built-in typescript.scm content + an `internal_module` namespace pattern;
  fixture `namespace Util { export function f() {} }` → asserts the namespace def IS extracted
  and the function still is (replacement with superset query); a second assertion with a
  namespace-only override .scm proves replacement semantics (function def absent) — D8.
- (b) `builtin_misses_namespace.rs` (control): `WICKED_ESTATE_PLUGINS` = empty temp dir;
  built-in `for_language("typescript")` over the same fixture yields NO namespace def — pins the
  D15 premise (fails loudly if typescript.scm ever gains the pattern; see merge notes).
- (c) `no_shadow.rs`: plugin dir with `name = "typescript"`, NO override flags (a would-be
  shadow) → built-in wins: namespace def absent, built-in constructs present.
  `full_override_gate.rs`: manifest-only and env-only combinations are inert (built-in query in
  use — asserted via the namespace probe); the both-signals dlopen case reuses the
  plugin_loader.rs `cc` pattern (skip-with-eprintln when no `cc`); the four-way gate matrix is
  fully covered by S2's pure-fn unit tests regardless of `cc` (D14).
- (d) `plugin_override_cli.rs` (subprocess, `CARGO_BIN_EXE_wicked-estate`): (1) broken override
  .scm → index exits 0, stderr carries `QUERY-OVERRIDE: ... failed to compile`, DB still contains
  built-in typescript extraction (language alive, prior files NOT deleted — pins F2's fix);
  (2) digest cycle: index with override → namespace node present, `plugin_overrides` meta key set;
  edit the .scm → second index stderr carries `PLUGIN-OVERRIDE state changed`, re-extraction
  observed (sqlite3/store probe), key changed; remove the override → third index re-fires, graph
  reverts to built-in node set (no stale override-minted nodes); (3) `plugins list` shows the D5
  override column states incl. INERT; (4) label-scoped key: `--repo`-labelled index writes
  `repo:<label>:plugin_overrides`, unscoped key absent (multi_repo.rs:241/528-530 pattern, per
  recon).
- Deletes: nothing; `plugin_loader.rs` untouched and must stay green (F13 counts restated after).

**S8 — Measurements (brief's protocol; commands recorded verbatim in the lane report).**
- BEFORE = `/Users/michael.parcewski/Projects/wicked/wicked-estate/target/release/wicked-estate`
  (read-only); AFTER = lane debug binary. Corpus: wicked-studio (TS, exercises the typescript
  entry). DBs under `<scratchpad>/ws/plugin-override/measure/`.
- (1) Extraction diff with/without the namespace override on wicked-studio (node count by kind,
  sqlite3, `.schema` first; kinds stored as JSON strings like `'"namespace"'`); (2) stderr of the
  override-active index run captured to a file; (3) digest-forced re-extract proven: touch the
  override .scm → next index logs the state-change marker and re-extracts (file digests unchanged,
  nodes re-minted — compare `plugin_overrides` key before/after); (4) `plugins list` output
  captured for query/grammar/INERT/FAILED states.

## 4. Compatibility + migration

- **Stored graphs**: no schema change anywhere — `plugin_overrides` is a row in the existing meta
  k/v table. Absent key ≡ empty descriptor ≡ no overrides, so every existing DB and every
  no-override user sees zero behavior change and no spurious `force_full` (F3 semantics mirrored).
- **First index after enabling/editing/removing an override**: one full re-extraction of that
  repo, by design (D6) — the honest cost, logged loudly. Symbol ids are stable across it (F9);
  annotations/memories keyed by symbols survive as in the id_scheme migration (store `had_node`
  survives `remove_file`, per recon consumers lens).
- **Existing plugins/manifests**: `library` relaxing to `Option` is backward-compatible (every
  existing manifest sets it); nginx example untouched and its loader test must stay green.
- **Crew**: the clean-HEAD skip means override edits are invisible to crew-managed graphs until a
  force refresh — documented operational rule (S6); a wicked-crew follow-up (plugin-dir evidence in
  its skip decision) is recorded in merge notes, NOT implemented here (cross-repo).
- **Long-running processes** (watcher, MCP): OnceLock semantics — restart required to pick up
  override changes; until restart the process runs old query + old digest, consistently (D4).
- **Consumers of stderr/stdout**: all new output is stderr; `stats` stdout untouched (F8).

## 5. Falsifier

The keystone claim is "an override changes extraction output and the store re-extracts honestly."
Falsified if: indexing the S7(a) fixture repo with the active typescript query override yields a
node set identical to the control (override not wired), OR the control test (S7(b)) finds the
namespace already captured by the built-in (premise dead — pick the next probe construct:
`declare module`, Ruby `define_method`; both grepped absent from their .scm at base but
unverified against grammars), OR editing the override .scm between two CLI index runs does NOT
produce the state-change marker + re-extraction (gate not wired / digest not over cached bytes),
OR a broken override .scm leaves the DB missing previously-indexed typescript files (F2
inherited — the exact defect D7 exists to prevent). Secondary: `cargo test -p` counts from F13
must be green post-change with the new files added; any existing test that flips is evidence the
never-shadow removal leaked into non-override paths.

## 6. Not in scope

- Widening the loud-failure fix to the existing silent `.ok()?` for built-ins and additive
  (non-override) plugin queries — same code path, pre-existing, worth an issue, not this lane.
- Any Edge/Node schema or `resolved_by` change (brief; recon confirmed zero value).
- `SYMBOL_ID_SCHEME` bump (F9 — override changes the set, not identity).
- crew-side clean-HEAD skip awareness of the plugins dir (cross-repo; recorded below).
- MCP-surface override auditability tool (CLI `plugins list` + meta key satisfy the brief).
- Language-scoped (per-extension) force instead of `force_full` (D6).
- Reload-without-restart for the OnceLock registry.
- MUST-NOT-TOUCH honored: version files, `lsp.rs`, resolve crate, `remove_file` paths, built-in
  `.scm` files — none appear in any step.

## 7. Merge notes for other lanes / deviations

- **extraction-gaps lane**: already merged at HEAD~1 (`bda76b7`); no live conflict. Standing
  coupling: if ANY future lane adds `internal_module` to `typescript.scm`, S7(b)'s control test
  fails by design — the fix is to move the override fixture to the next probe construct, not to
  delete the control.
- **tests/languages.rs and built-in .scm files**: not edited here (extraction-gaps ownership per
  program brief). Consequence accepted: existing built-in language tests remain unpinned against a
  dev machine's plugins dir (D2 exposure). Recommend a program-level follow-up: pin
  `WICKED_ESTATE_PLUGINS` in the fleet tests' process (one-line env guard) — touches a file this
  lane must not own.
- **wicked-crew follow-up (record only)**: `graph.ts:618-639` clean-HEAD skip is blind to
  plugin-dir changes; file a crew issue to include plugin-override state in its skip evidence, or
  keep the documented force-refresh remedy.
- **method-identity / id-scheme region** (`treesitter.rs:1354-1370`): read-only here; S3 touches
  only the constructor/lookup region (`:1218-1276`).
- **Line-number drift**: citations are at `764622f`; S3/S4 edits shift lines — the lane report
  re-cites post-change.
