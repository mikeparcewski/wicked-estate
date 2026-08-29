# Recon plan — plugin-override lane (user parser-plugin overrides)

Base: `764622f` on `feat/plugin-override`. Planner synthesis of four recon lenses
(history / consumers / tests / risks); every load-bearing citation below re-verified against
files opened in this worktree this session. **Revision 2 (2026-08-29)**: revised in place after
the adversarial attack round — resolves majors I1, I2, I3, I4/BR-1, PO-ATK-1..4 and folds the
minors that intersect the same text (I5-I8, BR-2..6, PO-ATK-5..8); see §8 for the issue→fix map.
No objection rejected. Governing findings: the additive-only precedence
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
| F2 | The current broken-query failure mode is silent graph **deletion**, not just "language dies": `Query::new(..).ok()?` returns `None` (`treesitter.rs:1216-1218` doc, `.ok()?` in `for_language` and `from_grammar`), the index pipeline `filter_map`s the `None` out of `ext_map` (`wicked-estate/src/lib.rs:678-687`), files of that extension fail the `supported` filter (`lib.rs:701-719`), and previously-indexed files are classified deleted and purged. No warning fires anywhere on that path. An override MUST NOT inherit this — **on either tier: `from_grammar` has the same `.ok()?` (`treesitter.rs:1234-1246`), so the grammar-override arm needs the same eager-compile fallback wire, not just the query-only arm.** | files opened; recon (consumers lens) traced the deleted-classification at `lib.rs:744-779` |
| F3 | Digest precedent: `load_extra_edge_rules` digests sorted raw name+bytes of every rule file — "ANY edit — including one that breaks parsing — changes the digest" — compared against per-repo `repo_scope::meta_key(repo, "extra_rules_digest")`, mismatch → `force_full`. But its key is written **at the check site** (`lib.rs:617`). | `lib.rs:367-419` (fn + `paths.sort()`), `:604-617` |
| F4 | The id_scheme gate (ADR-002 amendment, this very base commit) explicitly critiques check-site writes: "Writing at the check site would let a crash mid-run leave a DB stamped with the new scheme whose rows are still old — permanently mixed... Written last... idempotent." Key written only at run end (`lib.rs:1083-1087`) and the gate-guarded no-change early return (`:798-806`). | `lib.rs:619-644, 798-806, 1083-1087` |
| F5 | The plugin registry is a process-wide lazy `OnceLock` (`plugin.rs:91-96`); env is snapshot at first access; the existing test pins one-test-per-process (`tests/plugin_loader.rs:8-9`). `LoadedPlugin` caches `query_src` for the process lifetime (`plugin.rs:70-77`). **Consequence for tests: one registry CONFIGURATION per test binary — a second plugins-dir/env configuration in the same file can never take effect.** | `plugin.rs` full read |
| F6 | `PluginManifest.library` is a required `String` (`plugin.rs:50`) and `load_one` unconditionally `resolve_library` + dlopens (`plugin.rs:137-155`); a query-only override (shipped grammar, user query, **no shared library**) cannot parse or load today. Manifest is permissive serde — an `override_query` typo would be silently dropped. `extensions` is a free-form per-plugin list (`plugin.rs:45-47`) that can name ANY built-in entry's extensions. | `plugin.rs` full read |
| F7 | `load_all` iterates `read_dir` with no sort and `find_by_name` returns first match (`plugin.rs:99-101, 111-131`) — two dirs overriding one language would pick a nondeterministic winner; any registry-order digest would flap. | `plugin.rs` full read |
| F8 | `plugins list` prints name/exts/license only, free-form (`main.rs:3456-3487`); recon (consumers lens) found **no machine parser** of this output anywhere in crew/garden/studio — free to extend. Crew DOES regex-parse `stats` **stdout** (`graph.ts:226-230`, recon) — stats format and stdout generally must not change; all notices go to stderr (MCP is stdio JSON-RPC). | `main.rs` opened; consumer facts from recon |
| F9 | An override changes the node/edge **set**, not identity: SymbolIds derive from the logical name path + role maps (`treesitter.rs:1354` `SYMBOL_ID_SCHEME`, `:1356` `def_suffix`, `:1454` `def_nodekind`); constructs captured by both queries keep identical ids. Digest-forced re-extraction is the correct mechanism; **no** `SYMBOL_ID_SCHEME` bump. | files opened; ADR-002 |
| F10 | Fixture premise verified at base: `typescript.scm` (226 lines) and `tsx.scm` contain **no** `internal_module`/`module_declaration` pattern (grep, 0 hits) — TS `namespace Util {}` mints no node today — while the `namespace` role already maps with zero Rust change (`treesitter.rs:1359` → `Suffix::Type`, `:1463` → `NodeKind::Namespace`). The doc-04 constructs are unusable as fixtures (closed by `bda76b7`). **Neither measurement corpus contains a TS `namespace` declaration (grep over wicked-studio + wicked-crew, node_modules excluded: 0 files)** — so a corpus diff with the namespace override is expected-zero, not demonstrative. | greps + files opened this session; attack PO-ATK-3 corpus grep |
| F11 | Built-ins' only guard against a broken query is the compile-time test `every_wired_query_compiles` (`treesitter.rs:2950`), which can never cover user-provided files. | file opened |
| F12 | Crew's clean-HEAD refresh skip fires on git evidence alone (`wicked-crew graph.ts:618-639`, recon consumers lens); override files live outside every repo, so an override edit produces no git evidence — crew serves the stale graph until a force refresh. The digest gate is necessary but not sufficient for crew-managed graphs. | recon (consumers lens) |
| F13 | Baselines green at 764622f (recon tests lens, commands recorded there): `cargo test -p wicked-estate-extract` = 362 unit + 108 integration + 3 doctests, 0 failed; `cargo test -p wicked-estate` = 51 + 22 + 80, 0 failed. Counts go stale with new test files — re-run and restate at the end. | recon (tests lens) |
| F14 | **The agent-eval bench harness is unpinned**: `wicked-estate-bench` indexes its corpus through `wicked_estate::index_path` (`crates/wicked-estate-bench/src/capability.rs:279, :494`) with zero `WICKED_ESTATE_PLUGINS` references anywhere in the crate (grep = 0 hits) — after this change a dev machine's manifest-only query override would silently move built-in-language bench/capability numbers and the regenerated `docs/benchmarks/` baselines. The bench crate is on no MUST-NOT-TOUCH list and no other lane owns it. The bench binary is its own process (`src/main.rs:17`), so an env pin at startup is OnceLock-safe. | files opened this revision; attack I4/BR-1 |
| F15 | `LangEntry` and `LANG_TABLE` are private to the treesitter module (`treesitter.rs:509, :524` — no `pub`), so plugin.rs cannot eagerly compile an override query "against the built-in grammar" without a new `pub(crate)` accessor in treesitter.rs. | file opened this revision; attack PO-ATK-6 |

## 2. Decisions (all explicit — no TBD)

**D1 — New `docs/adr/ADR-010-plugin-overrides.md`, not an amendment.** ADR-001..008 exist; none
covers runtime plugins (F-history). Style: ADR-002-amendment convention — a `Resolves:` line
citing the review lineage (doc-04: every gap query-level, unpatchable without a release), quote
the two superseded sentences verbatim (`plugin.rs:20-21`, `PLUGIN.md:26-28`) and `de24d66`'s
"built-ins always win", then state the three-tier precedence:
**built-in < query-only override < full grammar override**. The ADR names the terminology split
explicitly: "runtime grammar plugin" (this feature, PLUGIN.md) vs the W6.1 "extractor plugin"
(`.wicked-estate-extractors/`, `Provenance::Extractor`) — adjacent digest keys, different features.
The ADR also owns two behavior changes explicitly: (a) the override gate makes plugin-registry
loading unconditional on every index run — plugins already ran under dlopen-at-first-use; this
moves first use to index start (PO-ATK-8b); (b) the bench hermeticity pin (D16).

**D2 — Query-only override activation = manifest-only (`override_query = "<lang>"`), as the brief
mandates.** The hermeticity exposure (a populated `~/.wicked-estate/plugins` silently changes
built-in-language results on that machine) is real but mitigated, not re-designed:
(a) loud stderr notice on every index run and at registry load; (b) every NEW test in this lane
pins `WICKED_ESTATE_PLUGINS` to a controlled temp dir; (c) the exposure is named in PLUGIN.md and
the ADR; (d) **the bench harness — the repo's truth oracle — is pinned in this lane (D16, S6b)**,
closing the one consumer where the exposure would silently corrupt published baselines. Residual
exposure after this lane: existing fleet language tests only (`tests/languages.rs`, other-lane
owned — see merge notes).

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
little; force_full is simple and honest. Cost stated honestly (BR-2): the descriptor is
machine-global while the gate key is per-repo, so ANY override change forces a full re-extraction
of EVERY repo in every DB on this machine at its next index — including repos with zero files of
the overridden language. ADR + PLUGIN.md say exactly that ("every repo on this machine re-extracts
fully at its next index"); a language-scoped refinement is named in not-in-scope as a recorded
future option.

**D7 — Broken override queries: compile EAGERLY at registry load ON BOTH TIERS; loud stderr; the
built-in extractor is always the fallback — never `None` for a built-in because of an override.**
- Query-only tier: compile the override .scm against the built-in grammar (via the new
  `pub(crate)` accessor, F15/S2); on failure eprintln `QUERY-OVERRIDE: <lang> override at <dir>
  failed to compile: <err> — using built-in query`, register failed-with-reason; lookups return
  the built-in extractor.
- **Grammar tier (I2): an ARMED grammar override's query is eagerly compiled against its OWN
  plugin grammar at registry load; on failure eprintln `GRAMMAR-OVERRIDE: <lang> override at <dir>
  failed to compile: <err> — built-in grammar and query in use`, the override is DISARMED
  (fall back to built-in grammar + built-in query) and dropped from the D5 effective set.** The
  armed arm in `for_language` (S3) consults only overrides that survived eager compilation, so the
  `from_grammar` `.ok()?` path (F2) is never reached with a user query for a built-in language.
A failed override on either tier is OUT of the effective set (D5), so a graph previously
extracted under it honestly re-extracts under the built-in. Additional cheap guard: a compiled
override whose query contains zero recognized `@code_*`/`@call` capture roles gets a loud warning
(compiles-but-useless case). The existing silent `.ok()?` for built-ins and additive plugins is
NOT widened in this lane (scope; see not-in-scope).

**D8 — Override semantics = wholesale query replacement.** The override .scm REPLACES the built-in
query for that language (users start by copying the shipped .scm and adding patterns). Merge/append
semantics would need pattern-level identity and dedup rules — complexity with no brief requirement.
Documented in PLUGIN.md; the fixture tests prove replacement (an override missing a built-in
pattern loses that construct — asserted, see S7(a) mechanism).

**D9 — Override matches per LANG_TABLE entry, not per language family.** `typescript` and `tsx`
are separate entries; overriding one does not touch the other. Per-family grouping would need new
data; per-entry is honest and simple. PLUGIN.md documents the caveat (patching TS usually means
two override dirs, one per entry). `override_query` stays a single string per manifest (brief's
shape). Per-entry matching is also what makes S7(a)'s single-configuration replacement proof legal
(one dir overriding `typescript`, another overriding `tsx` — not a D11 duplicate).

**D10 — `WICKED_ESTATE_PLUGIN_OVERRIDE` = comma-separated exact language names; no wildcard; read
once at first registry access (OnceLock semantics pinned in the ADR).** Belt-and-braces means
explicit naming. Separator is comma (never `:`/`;` — platform-dependent). Full grammar override of
lang L is armed iff `override = true` in that plugin's manifest AND L appears in the env list.
Extension claims of built-in-owned extensions are honored ONLY for an armed grammar override —
query-only overrides can never add or claim extensions. **Cross-language capture rule (I3): an
armed override's claimed extension that is owned by a LANG_TABLE entry OTHER than the overridden
language is REFUSED loudly — the claim is dropped and stderr names the extension and its built-in
owner (`GRAMMAR-OVERRIDE: extension 'py' is owned by built-in 'python' — claim dropped (name
'python' in WICKED_ESTATE_PLUGIN_OVERRIDE to allow)`) — unless that owning language is ALSO named
in `WICKED_ESTATE_PLUGIN_OVERRIDE`.** A plugin overriding typescript can never silently hijack
`.py`; the double opt-in is per captured language, not per plugin. Stated in ADR-010 + PLUGIN.md;
unit-tested (S2).

**D11 — Same-language override collision: disable BOTH, loud stderr naming both dirs — and this
holds ACROSS MODES (BR-4).** Two dirs overriding one language in the same mode, or one dir with
`override_query = "typescript"` and another with an armed grammar override for typescript, are
all duplicate refusals: the effective set stays one-entry-per-language (matching the D5 descriptor
shape), and no silent grammar-beats-query surprise appears when the env var later disarms the
grammar tier. First-match-wins over unsorted `read_dir` (F7) is nondeterministic; deterministic
refusal beats a silent arbitrary winner. `load_all` additionally sorts entries by path (mirrors
`lib.rs:383` `paths.sort()`) so the registry, `plugins list`, and the D5 descriptor are stable.
Refused-duplicate overrides are visible in `plugins list` (S5 fifth state).

**D12 — Manifest evolution: `library` becomes `Option<String>`; `load_one` enforces "library
required unless `override_query` is set" with the existing loud per-plugin skip.** New fields:
`override_query: Option<String>`, `override` (bool, default false; serde-renamed, `override` is a
Rust keyword). Unknown manifest keys are captured via `#[serde(flatten)]` into a map and warned
about by name (so `override-query` typos are visible) — NOT `deny_unknown_fields`, which would
break existing user manifests carrying stray keys. The nginx example manifest stays byte-valid.
**Target validation (BR-3): `override_query` must name a LANG_TABLE entry — a value naming a
grammarless built-in (jcl/hlasm, dispatched via `is_grammarless_ext` outside LANG_TABLE), an
unknown language, or another plugin's language is a loud per-plugin skip (`override_query '<x>'
names no built-in grammar language`) that keeps the plugin out of the effective set.** Unit-tested
(S2).

**D13 — Notices: stderr only, two sites.** (1) Registry load (`load_all`): per-plugin
load/skip/override-compile-failure/duplicate-refusal/ext-claim-refusal notices — once per process,
the existing eprintln convention (`plugin.rs:127`). (2) The index gate site in `lib.rs` (reached
by every index run): per-run `query override active: <lang> <- <plugin dir>` / `GRAMMAR OVERRIDE
active: <lang> <- <plugin dir>` lines plus, on mismatch, `PLUGIN-OVERRIDE state changed: forcing
full re-extraction` **followed by the differing descriptor lines (`- <old line>` / `+ <new
line>`, D5 line format) so the CLI-vs-editor env-skew footgun is diagnosable from the output
(I7)** — the EXTRA-EDGE/VERSION-CHANGE house style. Never stdout (MCP stdio protocol; crew parses
stats stdout, F8). CLI arms that never construct extractors (stats/query/resolve) stay silent; the
ADR says so, so "loud startup notice" is not read as "on every invocation".

**D14 — One registry CONFIGURATION per test binary: file boundaries equal configuration
boundaries (I1/PO-ATK-1).** F5 makes a second plugins-dir/env configuration in the same process
unreachable — so no test file may assert against more than one env/dir configuration. Every
multi-configuration flow is either (a) split into one integration-test file per configuration, or
(b) routed through the CLI-subprocess harness (`CARGO_BIN_EXE_wicked-estate`, the
`index_bad_path_cli.rs` pattern), where each invocation is a fresh process. An in-process
edit-then-reindex digest test would additionally be wrong-by-construction (cached query vs edited
file) — digest/stderr assertions are subprocess-only. The full-grammar dlopen legs inherit the
`cc`-skip pattern from `plugin_loader.rs`; the arming decision itself is a pure function with
in-crate unit tests so the double-opt-in matrix is proven even without `cc`. The S7 file list
below names one configuration per file.

**D15 — Fixture = TypeScript `namespace` (`internal_module`).** Verified missing at base (F10),
zero Rust change needed for its roles (F10), owned by no other lane (extraction-gaps merged at
HEAD~1; its scope was Go/Ruby/Java/C#/Swift/C++ + `.h` routing). A control test pins the premise:
built-in typescript extraction of the fixture yields NO namespace def (with `WICKED_ESTATE_PLUGINS`
pinned to an empty temp dir) — if someone later adds `internal_module` to `typescript.scm`, the
control fails loudly instead of the override test passing vacuously.

**D16 — Bench hermeticity pin (I4/BR-1).** `wicked-estate-bench` is the repo's truth oracle
("must not regress") and indexes with no plugin pinning (F14). This lane pins
`WICKED_ESTATE_PLUGINS` to a fresh empty temp dir at bench `main()` startup — before any registry
access; the bench binary is its own process so OnceLock timing is safe — with one stderr line
(`bench: WICKED_ESTATE_PLUGINS pinned to empty dir for hermetic baselines`). Unconditional: bench
numbers must never depend on who runs them; anyone researching plugin-influenced numbers edits the
harness deliberately. Named in ADR-010 as the bench-exposure closure.

## 3. Steps

Ordering: S1 (ADR) and S2 (registry) first — S3-S5 program against S2's public functions.
Each step compiles green per-crate (`cargo build/test/clippy -p <crate>`, lane CARGO_TARGET_DIR).

**S1 — ADR-010.**
- Files: `docs/adr/ADR-010-plugin-overrides.md` (new).
- Change: precedence model (D1), safety rules (D7 both-tier fallback, ABI gate unchanged, D10
  double opt-in + cross-language ext-claim refusal, D11 cross-mode duplicates, D12 target
  validation), digest design + write-last divergence from extra_rules (D3-D6, incl. the
  fleet-wide force_full cost sentence), OnceLock/restart semantics (D4, D10), per-entry matching
  (D9), replacement semantics (D8), the crew clean-HEAD operational rule (F12), staleness
  blindness (BR-5: MCP staleness reporting derives from file mtimes and cannot surface override
  drift — after any override change, explicitly re-index every repo/DB; crew: force refresh),
  hermeticity exposure + bench pin (D2, D16), unconditional-registry-load-per-index-run sentence
  (D1/PO-ATK-8b), terminology disambiguation (D1).
- Tests: n/a (prose); its claims are pinned by S6/S7 tests.
- Deletes: nothing (the superseded sentences are deleted in S2/S6, same PR).

**S2 — Manifest + override registry (`plugin.rs` + a treesitter.rs accessor).**
- Files: `crates/wicked-estate-extract/src/plugin.rs`,
  `crates/wicked-estate-extract/src/treesitter.rs` (accessor only — F15/PO-ATK-6: new
  `pub(crate) fn builtin_language(name: &str) -> Option<tree_sitter::Language>` over the private
  LANG_TABLE; the precedence rewiring of treesitter.rs stays in S3).
- Change: D12 manifest fields + flatten-warn + override_query target validation (LANG_TABLE-entry
  names only, loud skip otherwise); sort `load_all` entries (D11); new
  `QueryOverride { lang, dir, query_src, compiled: Result<(), String> }` registry list — compiled
  eagerly against the built-in grammar at load (D7, via `builtin_language`), loud on failure,
  zero-role warning; **grammar-tier eager compile (I2): an armed grammar override's query is
  compiled against its own plugin grammar at load; failure → GRAMMAR-OVERRIDE marker + disarm +
  out of the effective set (D7)**; grammar override arming as a pure fn
  `grammar_override_armed(manifest_flag, env_list, lang)` (D10, D14); **cross-language ext-claim
  refusal (I3/D10): claimed extensions owned by a different LANG_TABLE entry are dropped loudly
  unless that owner is also named in the env list**; duplicate-override refusal incl. cross-mode
  (D11); public seam: `override_query_for(lang)`, `grammar_override_for_name(lang)` /
  `..._for_ext(ext)`, and `override_state() -> String` (the D5 canonical descriptor built from
  CACHED bytes, D4). Rewrite the module doc — delete "so a plugin never shadows a built-in"
  (`plugin.rs:20-21`) and correct the false "skip with a warning" scope note.
- Tests: in-crate unit tests — manifest parsing (query-only / grammar / legacy nginx manifest
  byte-compat / unknown-key warning / bad override_query target: grammarless, unknown,
  plugin-name), arming pure fn (all four signal combinations), **ext-claim ownership refusal
  (foreign-owned ext dropped; allowed when the owner is also named; non-built-in ext passes)**,
  descriptor determinism (sorted, stable), duplicate refusal (same-mode and cross-mode).
- Deletes: the `library`-required contract; the never-shadows doc sentence.

**S3 — Lookup precedence (`crates/wicked-estate-extract/src/treesitter.rs`).**
- Files: `treesitter.rs`.
- Change: inside `for_language`'s LANG_TABLE branch (`:1219-1227`): consult
  `plugin::grammar_override_for_name` (armed AND eagerly-compiled-ok → plugin grammar + plugin
  query; a disarmed-by-compile-failure override never reaches this arm, D7/I2), else
  `plugin::override_query_for` (compiled-ok → built-in grammar + override query; failed → built-in
  query — never `None` because of an override, D7). In `extractor_for_extension`: before the
  LANG_TABLE ext match, honor an ARMED grammar override's SURVIVING extension claims (post
  D10/I3 ownership filter); everything else unchanged (built-in ext match already delegates to
  `for_language`, F1, so query-only overrides need no ext-site change). All precedence decisions
  live in the S2 plugin.rs functions — the two sites cannot drift.
- Tests: proven by S7 integration files; `every_wired_query_compiles` untouched.
- Deletes: nothing (the fall-through remains for non-built-in plugins).

**S4 — Digest gate + audit key (`crates/wicked-estate/src/lib.rs`).**
- Files: `crates/wicked-estate/src/lib.rs`.
- Change: next to the extra_rules gate (`:604-617`): read
  `repo_scope::meta_key(repo, "plugin_overrides")`, compare to `plugin::override_state()`;
  mismatch → `force_full = true` + D13 per-run stderr lines including the old→new descriptor-line
  diff (I7). Key written ONLY at run end (with the id_scheme key, `:1083-1087`) and in the
  gate-guarded no-change early return (`:798-806` pattern) — D3.
- Tests: S7(d) subprocess tests; label-scoped-key assertion in the same file (multi_repo.rs
  pattern, own test file — multi_repo.rs untouched).
- Deletes: nothing (additive gate; no schema change — meta is generic k/v).

**S5 — `plugins list` override column (`crates/wicked-estate/src/main.rs:3456-3487`).**
- Files: `main.rs`.
- Change: per plugin, append override status — five states: `override=query(<lang>)`,
  `override=query(<lang>) FAILED: <err> — built-in in use`, `override=grammar(<lang>) [armed]`,
  `override=grammar(<lang>) [INERT — not named in WICKED_ESTATE_PLUGIN_OVERRIDE]`, and
  **`override=<mode>(<lang>) DISABLED: duplicate of <other dir>` (PO-ATK-8a — a D11 refusal must
  be visible, not silently absent)**. Query-only (library-less) plugins are listed. Showing INERT
  matters: the double opt-in means a manifest alone must visibly do nothing.
- Tests: asserted inside S7(d).
- Deletes: nothing (no parser consumes the old format, F8).

**S6 — Docs.**
- Files: `PLUGIN.md`, `FEATURES.md`, `README.md`, `docs/add-lang.md`,
  `examples/plugins/nginx/README.md`.
- Change: rewrite the precedence paragraph (`PLUGIN.md:26-28`); new "Overriding a built-in
  language" section — both tiers, manifest examples, safety rules (incl. the I3 ext-claim rule),
  replacement semantics (D8), per-entry caveat (D9), the re-extraction consequence stated
  fleet-wide ("every repo on this machine re-extracts fully at its next index", BR-2) + one-key
  audit trail (D5/D6), restart caveat for MCP/watcher (D4), staleness-reporting blindness +
  re-index remedy (BR-5), crew force-refresh remedy (F12), hermeticity exposure (D2),
  CLI-vs-editor env-skew footgun (now diagnosable from the D13 descriptor diff). DELETE list
  (corrected per PO-ATK-7 — verified this revision): the never-shadows sentences live at
  `PLUGIN.md:27`, `FEATURES.md:123`, `examples/plugins/nginx/README.md:36` ONLY. `README.md` and
  `docs/add-lang.md` carry no such sentence — they move to the UPDATE list (pointer to the new
  override section). Grep at execution time remains authoritative over these line numbers.
- Tests: the acceptance grep is SCOPED to the user-facing doc surfaces (PO-ATK-2 — the recon plan
  and the ADR quote the superseded sentence by design and must keep it). Verbatim command,
  recorded in the lane report:
  `grep -rn "never shadows" PLUGIN.md FEATURES.md README.md docs/add-lang.md examples/plugins/nginx/README.md`
  must return 0 hits. (`docs/recon/` and `docs/adr/` are deliberately excluded.)
- Deletes: the additive-only invariant from the three doc surfaces that carry it.

**S6b — Bench hermeticity pin (D16, I4/BR-1).**
- Files: `crates/wicked-estate-bench/src/main.rs`.
- Change: at the top of `main()` (`:17`), before any indexing: set `WICKED_ESTATE_PLUGINS` to a
  fresh empty temp dir, one stderr notice line. Own process → OnceLock-safe (F14). No other bench
  logic touched; capability.rs/community_metrics.rs/memory_recall.rs unchanged.
- Tests: `cargo build -p wicked-estate-bench` green; the pin is asserted by inspection in the
  lane report (env set before first `index_path` reach — single entry point).
- Deletes: nothing.

**S7 — Tests. One registry configuration per test FILE (D14); every file pins
`WICKED_ESTATE_PLUGINS`; multi-configuration flows live in the CLI subprocess harness only.**
- Files (new, one configuration each):
  - `crates/wicked-estate-extract/tests/query_override.rs`
  - `crates/wicked-estate-extract/tests/builtin_misses_namespace.rs` (control)
  - `crates/wicked-estate-extract/tests/no_shadow.rs`
  - `crates/wicked-estate-extract/tests/override_manifest_only_inert.rs`
  - `crates/wicked-estate-extract/tests/override_env_only_inert.rs`
  - `crates/wicked-estate-extract/tests/override_both_signals.rs`
  - `crates/wicked-estate/tests/plugin_override_cli.rs`
- (a) `query_override.rs` — ONE configuration proving both capture and replacement (PO-ATK-1
  mechanism: two LANG_TABLE entries, one registry): the plugins dir holds TWO override dirs —
  a SUPERSET override for `typescript` (built-in typescript.scm content + an `internal_module`
  pattern) and a NAMESPACE-ONLY override for `tsx` (legal under D9 per-entry matching; not a D11
  duplicate). Test 1 (.ts fixture `namespace Util { export function f() {} }`): namespace def
  present AND function def present (superset capture). Test 2 (.tsx fixture, same content):
  namespace def present AND function def ABSENT (wholesale replacement, D8). Both tests share the
  single process-wide configuration.
- (b) `builtin_misses_namespace.rs` (control): `WICKED_ESTATE_PLUGINS` = empty temp dir;
  built-in `for_language("typescript")` over the fixture yields NO namespace def — pins the
  D15 premise.
- (c) `no_shadow.rs` — two explicit legs, neither vacuous (PO-ATK-4): (i) no-cc leg: a
  LIBRARY-LESS manifest with `name = "typescript"` and no override flags is REFUSED by the D12
  rule — assert `find_by_name("typescript").is_none()` after registry load AND built-in
  extraction of the fixture unchanged (this asserts D12 itself, not an accidentally-empty
  registry); (ii) cc-gated leg (plugin_loader.rs skip pattern): build the nginx example dylib,
  manifest `name = "typescript"`, `symbol = "tree_sitter_nginx"`, `extensions = ["ts"]`, NO
  override flags → assert `for_language("typescript")` AND `extractor_for_extension("ts")` both
  return the BUILT-IN extractor (loaded non-override plugin never shadows; unarmed ext claim not
  honored).
- (c') Full-override gate, one configuration per file (I1): `override_manifest_only_inert.rs` —
  cc-gated dylib, `override = true`, env UNSET → built-in query in use (namespace probe absent),
  `plugins list`-level state INERT asserted at the registry API; `override_env_only_inert.rs` —
  cc-gated dylib, `override` absent/false, env names typescript → inert the same way;
  `override_both_signals.rs` — cc-gated, `override = true` AND env names typescript → plugin
  grammar in use, PLUS the armed-ext-claim assertion (claimed `ts` honored at
  `extractor_for_extension`) PLUS the I3 refusal (a claimed foreign-owned extension, e.g. `py`,
  is NOT honored — `.py` still dispatches to built-in python). Each file skips-with-eprintln
  without `cc`; the four-way arming matrix is fully covered by S2's pure-fn unit tests regardless
  of `cc` (D14).
- (d) `plugin_override_cli.rs` (subprocess, `CARGO_BIN_EXE_wicked-estate` — each invocation a
  fresh process, so multi-configuration flows are legal here): (1) broken query-only override
  .scm → index exits 0, stderr carries `QUERY-OVERRIDE: ... failed to compile`, DB still contains
  built-in typescript extraction (language alive, prior files NOT deleted — pins F2's fix);
  (1b) **grammar-tier bad query (I2, cc-gated leg): armed grammar override whose .scm does not
  compile against the plugin grammar → stderr carries `GRAMMAR-OVERRIDE: ... failed to compile`,
  built-in language alive, no files deleted**; (2) digest cycle: index with override → namespace
  node present, `plugin_overrides` meta key set; make a SEMANTIC byte edit to the .scm (remove
  the internal_module pattern — never a bare `touch`: the D4/D5 digest is over bytes, and the
  evidence must be both the marker AND a node-set diff, PO-ATK-5) → second index stderr carries
  `PLUGIN-OVERRIDE state changed` plus the old→new descriptor-line diff (I7), namespace node
  GONE, key changed; re-add the pattern → third index re-fires, namespace node back; remove the
  override dir → fourth index reverts to the built-in node set (no stale override-minted nodes);
  (3) `plugins list` states: query/FAILED/DISABLED-duplicate legs unconditional (no dylib
  needed); grammar armed/INERT legs cc-gated with the plugin_loader.rs skip pattern (I8);
  (4) label-scoped key: `--repo`-labelled index writes `repo:<label>:plugin_overrides`, unscoped
  key absent (multi_repo.rs:241/528-530 pattern). Every invocation sets `WICKED_ESTATE_PLUGINS`
  explicitly on the child env.
- Baseline hygiene (BR-6): before the F13 re-run, record the machine's plugins-dir state
  (`echo "$WICKED_ESTATE_PLUGINS"; ls ~/.wicked-estate/plugins` — currently absent on this
  machine) in the lane report; if populated, run the baseline with `WICKED_ESTATE_PLUGINS`
  pointed at an empty dir on the command line so falsifier (5) is not confounded.
- Deletes: nothing; `plugin_loader.rs` untouched and must stay green (F13 counts restated after).

**S8 — Measurements (brief's protocol; commands recorded verbatim in the lane report; every
command pins `WICKED_ESTATE_PLUGINS` explicitly — BR-6).**
- BEFORE = `/Users/michael.parcewski/Projects/wicked/wicked-estate/target/release/wicked-estate`
  (read-only); AFTER = lane debug binary. DBs under `<scratchpad>/ws/plugin-override/measure/`.
- (0) **No-override parity (I6): BEFORE vs AFTER, empty plugins dir, BOTH corpora (wicked-studio
  AND wicked-crew) — node-count-by-kind must be identical**, measuring the compat claim
  "no-override users see zero behavior change" instead of asserting it.
- (1a) **Demonstrative override diff (PO-ATK-3): on the S7(a) namespace-bearing FIXTURE repo** —
  with vs without the typescript namespace override (AFTER binary; node count by kind, sqlite3,
  `.schema` first; kinds stored as JSON strings like `'"namespace"'`); expected: namespace nodes
  appear only in the override run.
- (1b) **Corpus no-regression diff (PO-ATK-3): wicked-studio with the SUPERSET override active vs
  control — EXPECTED-ZERO node-by-kind diff** (F10: the corpus has no `namespace` declarations;
  zero diff here is the PASS condition — the override is active, announced on stderr, and changes
  nothing it shouldn't).
- (2) stderr of the override-active index runs captured to files (registry-load + gate-site
  notices).
- (3) digest-forced re-extract proven with a SEMANTIC byte edit (PO-ATK-5, never `touch`):
  remove the internal_module pattern from the override .scm → next index logs the state-change
  marker + descriptor diff and re-extracts (source-file digests unchanged, namespace nodes gone —
  compare `plugin_overrides` key before/after).
- (4) `plugins list` output captured for query/FAILED/DISABLED states (and grammar armed/INERT
  when the cc dylib is available on this machine).

## 4. Compatibility + migration

- **Stored graphs**: no schema change anywhere — `plugin_overrides` is a row in the existing meta
  k/v table. Absent key ≡ empty descriptor ≡ no overrides, so every existing DB and every
  no-override user sees zero behavior change (measured, S8(0)) and no spurious `force_full`.
- **First index after enabling/editing/removing an override**: full re-extraction — and because
  the descriptor is machine-global while the key is per-repo, EVERY repo in every DB on the
  machine re-extracts fully at its next index (BR-2; loudly announced, stated in ADR + PLUGIN.md).
  Symbol ids are stable across it (F9); annotations/memories keyed by symbols survive as in the
  id_scheme migration.
- **Existing plugins/manifests**: `library` relaxing to `Option` is backward-compatible (every
  existing manifest sets it); nginx example untouched and its loader test must stay green.
- **Crew**: the clean-HEAD skip means override edits are invisible to crew-managed graphs until a
  force refresh — documented operational rule (S6); a wicked-crew follow-up is recorded in merge
  notes, NOT implemented here (cross-repo).
- **MCP staleness reporting** (BR-5): derives from indexed_root file mtimes
  (`wicked-estate-mcp/src/main.rs:190-209`) and cannot surface override drift; a graph extracted
  under an old override state reports fresh until its next index run. Documented remedy (ADR +
  PLUGIN.md): after any override change, re-index every repo/DB explicitly; crew: force refresh.
- **Bench harness**: pinned hermetic in this lane (D16/S6b) — bench numbers cannot move with a
  dev machine's plugins dir.
- **Long-running processes** (watcher, MCP): OnceLock semantics — restart required to pick up
  override changes; until restart the process runs old query + old digest, consistently (D4).
- **Consumers of stderr/stdout**: all new output is stderr; `stats` stdout untouched (F8).

## 5. Falsifier

The keystone claim is "an override changes extraction output and the store re-extracts honestly."
Falsified if: indexing the S7(a) fixture repo with the active typescript query override yields a
node set identical to the control (override not wired), OR the control test (S7(b)) finds the
namespace already captured by the built-in (premise dead — pick the next probe construct:
`declare module`, Ruby `define_method`; both grepped absent from their .scm at base but
unverified against grammars), OR a semantic byte edit to the override .scm between two CLI index
runs does NOT produce the state-change marker + descriptor diff + re-extraction (gate not wired /
digest not over cached bytes), OR a broken override query — on EITHER tier — leaves the DB
missing previously-indexed typescript files (F2 inherited — the exact defect D7 exists to
prevent). Secondary: `cargo test -p` counts from F13 must be green post-change with the new files
added, with the machine's plugins-dir state recorded alongside the run (and pinned empty if
populated, BR-6) so a flipped pre-existing test is evidence the never-shadow removal leaked into
non-override paths — not a dev-machine plugin artifact. Doc falsifier: the SCOPED S6 grep
(command verbatim in S6) must return 0 hits.

## 6. Not in scope

- Widening the loud-failure fix to the existing silent `.ok()?` for built-ins and additive
  (non-override) plugin queries — same code path, pre-existing, worth an issue, not this lane.
- **Moving the `extra_rules_digest` write from its check site (`lib.rs:617`) to write-last (I5)**:
  same defect class as F4 but a pre-existing sibling outside this lane's deliverables; recorded
  here as a deliberate omission with a follow-up issue to file (see merge notes) rather than a
  silent skip.
- Any Edge/Node schema or `resolved_by` change (brief; recon confirmed zero value).
- `SYMBOL_ID_SCHEME` bump (F9 — override changes the set, not identity).
- crew-side clean-HEAD skip awareness of the plugins dir (cross-repo; recorded below).
- MCP-surface override auditability tool (CLI `plugins list` + meta key satisfy the brief).
- Language-scoped force instead of `force_full` (D6) — named future refinement: fire the gate
  only when the repo's supported file set contains an extension of an overridden LANG_TABLE
  entry (BR-2); feasible because force_full is consumed after the file walk.
- Reload-without-restart for the OnceLock registry.
- MUST-NOT-TOUCH honored: version files, `lsp.rs`, resolve crate, `remove_file` paths, built-in
  `.scm` files — none appear in any step. (S2's treesitter.rs accessor and S3's lookup rewiring
  touch only the constructor/lookup region; `remove_file` call sites untouched.)

## 7. Merge notes for other lanes / deviations

- **extraction-gaps lane**: already merged at HEAD~1 (`bda76b7`); no live conflict. Standing
  coupling: if ANY future lane adds `internal_module` to `typescript.scm`, S7(b)'s control test
  fails by design — the fix is to move the override fixture to the next probe construct, not to
  delete the control.
- **tests/languages.rs and built-in .scm files**: not edited here (extraction-gaps ownership per
  program brief). Consequence accepted: existing built-in language tests remain unpinned against a
  dev machine's plugins dir (D2 residual). Recommend a program-level follow-up: pin
  `WICKED_ESTATE_PLUGINS` in the fleet tests' process — touches a file this lane must not own.
  The bench harness, previously in the same unowned bucket, is now closed IN this lane (S6b).
- **wicked-crew follow-up (record only)**: `graph.ts:618-639` clean-HEAD skip is blind to
  plugin-dir changes; file a crew issue to include plugin-override state in its skip evidence, or
  keep the documented force-refresh remedy.
- **extra_rules_digest check-site write (I5, record only)**: `lib.rs:617` carries the crash hole
  the id_scheme gate documents; file a wicked-estate issue to move it to write-last — not fixed
  here (outside deliverables; see not-in-scope).
- **method-identity / id-scheme region** (`treesitter.rs:1354-1370`): read-only here; S2 adds only
  a `pub(crate)` LANG_TABLE accessor and S3 touches only the constructor/lookup region
  (`:1218-1276`).
- **Line-number drift**: citations are at `764622f`; S2/S3/S4 edits shift lines — the lane report
  re-cites post-change.

## 8. Attack-resolution ledger (revision 2)

| Issue | Resolution |
|---|---|
| I1 / PO-ATK-1 (major) | D14 rewritten: file boundaries = configuration boundaries. S7(a) replacement proof now uses two LANG_TABLE entries (typescript superset + tsx namespace-only) in ONE configuration — legal per D9, not a D11 duplicate. Full-override gate split into three one-configuration files (S7(c')). Multi-config flows live only in the CLI subprocess harness (S7(d)). |
| I2 (major) | D7 extended to the grammar tier: armed override's query eagerly compiled against its own plugin grammar at load; failure → GRAMMAR-OVERRIDE marker + disarm to built-in grammar+query + dropped from the effective set. F2 updated (from_grammar `.ok()?` named). New cc-gated subprocess test leg S7(d)(1b): bad grammar-tier query → language alive, no deletions. |
| I3 (major) | D10 cross-language capture rule: a claimed extension owned by a different LANG_TABLE entry is refused loudly unless that owner is also named in the env var. S2 unit tests + S7(c') integration assertion (.py not hijacked). In ADR + PLUGIN.md. |
| I4 / BR-1 (major) | New D16 + step S6b: `wicked-estate-bench/src/main.rs` pins `WICKED_ESTATE_PLUGINS` to an empty temp dir at startup (own process, OnceLock-safe), stderr notice, named in ADR-010. F14 records the evidence. |
| PO-ATK-2 (major) | S6 grep scoped to the five user-facing doc surfaces, verbatim command recorded; docs/recon + docs/adr deliberately excluded (they quote the superseded sentence by design). |
| PO-ATK-3 (major) | S8 split: (1a) demonstrative diff on the namespace-bearing fixture repo; (1b) wicked-studio superset-override run reframed as EXPECTED-ZERO no-regression diff with the pass condition stated. F10 records the corpus grep (0 namespace declarations in both corpora). |
| PO-ATK-4 (major) | no_shadow.rs given two explicit legs: no-cc leg asserts the D12 refusal (find_by_name none + built-in unchanged — a real assertion, not a vacuous pass); cc-gated leg loads a real dylib named typescript with an unarmed ts claim and asserts built-in wins at BOTH lookup sites. Armed-ext-claim (honored) + foreign-ext (refused) assertions added to override_both_signals.rs. |
| I5 (minor) | Recorded as deliberate not-in-scope + follow-up issue in merge notes. |
| I6 (minor) | S8(0): BEFORE-vs-AFTER no-override parity on BOTH corpora. |
| I7 (minor) | D13/S4/S7(d)(2): state-change marker prints the old→new descriptor-line diff; asserted. |
| I8 (minor) | S7(d)(3): grammar armed/INERT list legs cc-gated with the plugin_loader.rs skip pattern; query/FAILED/DISABLED legs unconditional. |
| BR-2 (minor) | Fleet-wide force_full cost stated in D6, §4, ADR, PLUGIN.md; language-scoped refinement named in not-in-scope. |
| BR-3 (minor) | D12/S2: override_query must name a LANG_TABLE entry; loud skip + unit tests for grammarless/unknown/plugin-name targets. |
| BR-4 (minor) | D11: same-language collision across modes is also a duplicate refusal; cross-mode case in the S2 unit test. |
| BR-5 (minor) | Staleness-reporting blindness + explicit re-index remedy documented (S1 ADR + S6 PLUGIN.md + §4). |
| BR-6 (minor) | Plugins-dir state recorded (and pinned if populated) alongside the F13 baseline re-run and all S8 commands; falsifier (5) de-confounded. |
| PO-ATK-5 (minor) | "touch" eliminated: S7(d)(2) and S8(3) specify a semantic byte edit with a node-set diff as evidence. |
| PO-ATK-6 (minor) | F15 + S2 file list: `pub(crate) fn builtin_language` accessor in treesitter.rs; choice named in S2. |
| PO-ATK-7 (minor) | S6 delete list corrected to PLUGIN.md:27 / FEATURES.md:123 / nginx README:36 (verified this revision); README + add-lang moved to the update list. |
| PO-ATK-8 (minor) | (a) fifth `plugins list` state DISABLED-duplicate (S5, asserted S7(d)(3)); (b) unconditional-registry-load sentence owned by ADR-010 (D1). |
