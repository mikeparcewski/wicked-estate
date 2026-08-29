# ADR-010 — Plugin overrides of built-in languages

**Status:** Accepted · **Date:** 2026-08-29 · **Lane:** plugin-override
**Implements:** `crates/wicked-estate-extract/src/plugin.rs` (registry + precedence),
`crates/wicked-estate-extract/src/treesitter.rs` (lookup sites),
`crates/wicked-estate/src/lib.rs` (digest gate), `PLUGIN.md` (user guide)
**Resolves:** the adversarial-review doc-04 finding class — every doc-04 gap was query-level and
unpatchable without a release: a user could not patch a query gap locally, swap a newer grammar,
or claim an extension for a built-in language.

## Context

Runtime language plugins (drop-in tree-sitter grammars, `PLUGIN.md`) were deliberately
additive-only. The superseded invariant, verbatim:

> Lookups in `TreeSitterExtractor` consult `LANG_TABLE` first, then loaded plugins — so a plugin
> never shadows a built-in.

(`plugin.rs` module doc; `PLUGIN.md`: "lookups check built-in languages **first**, then plugins, so
a plugin never shadows a built-in".) Commit `de24d66` made it law: "built-ins always win". No ADR
among ADR-001..008 covers the runtime plugin system, so this is a **new decision record**, not an
amendment.

Terminology: this ADR is about **runtime grammar plugins** (`PLUGIN.md`, dlopen'd tree-sitter
grammars + `.scm` queries). It is distinct from the W6.1 **extractor plugin** SDK
(`.wicked-estate-extractors/*.toml` drop-in edge rules, `Provenance::Extractor`). The two have
adjacent digest meta keys (`extra_rules_digest` vs `plugin_overrides`) and are different features.

## Decision

Three-tier precedence, replacing additive-only:

**built-in < query-only override < full grammar override**

1. **Query-only override** (manifest: `override_query = "<lang>"`). A plugin directory may carry
   only a manifest + a `.scm`: the **shipped grammar** with a **user query**. No shared library is
   required or loaded. Activation is manifest-only, as the review brief mandates.
2. **Full grammar override** — explicit double opt-in, never silent: `override = true` in
   `plugin.toml` **AND** the language named in `WICKED_ESTATE_PLUGIN_OVERRIDE` (comma-separated
   exact language names; no wildcard). Flips precedence for that language: plugin grammar + plugin
   query replace the built-in pair. Extension claims of built-in-owned extensions are honored only
   under this double opt-in.
3. Non-override plugins keep the old semantics exactly: a plugin never shadows a built-in.

Override semantics are **wholesale query replacement** (users start by copying the shipped `.scm`);
merge/append semantics would need pattern-level identity and are not offered. Overrides match **per
`LANG_TABLE` entry**, not per language family — `typescript` and `tsx` are separate entries, so
patching TS across both usually means two override directories.

### Safety rules

- **Eager compile + loud fallback on BOTH tiers.** The pre-existing failure mode for a broken
  built-in query is silent graph deletion: `Query::new(..).ok()?` yields `None`, the index
  pipeline drops the language from `ext_map`, its files fail the `supported` filter, and
  previously-indexed files are classified deleted and purged — with no warning. An override must
  not inherit this. A query-only override's `.scm` is compiled eagerly at registry load against
  the built-in grammar; on failure a loud `QUERY-OVERRIDE: … failed to compile … — using built-in
  query` marker fires and lookups return the built-in extractor. An **armed grammar override's**
  query is compiled eagerly against its **own plugin grammar** at load; on failure a loud
  `GRAMMAR-OVERRIDE: … failed to compile … — built-in grammar and query in use` marker fires, the
  override is disarmed, and it drops out of the effective set. Either way: **never `None` for a
  built-in language because of an override.** (The pre-existing silent `.ok()?` for built-ins and
  additive plugins is unchanged here — a recorded follow-up.)
- **ABI validation unchanged.** Grammar overrides pass the same dlopen + ABI 13–15 gate as any
  plugin; failure is the existing loud per-plugin skip.
- **Cross-language extension capture.** An armed grammar override's claimed extension that is
  owned by a `LANG_TABLE` entry **other** than the overridden language is refused loudly (the
  claim is dropped; stderr names the extension and its built-in owner) unless that owning
  language is **also** named in `WICKED_ESTATE_PLUGIN_OVERRIDE`. The double opt-in is per
  captured language, not per plugin — a typescript override can never silently hijack `.py`.
- **Query-only overrides never claim extensions.** Extension dispatch already delegates to the
  overridden language's entry; claimed extensions on a query-only manifest are ignored with a
  warning.
- **Override target validation.** `override_query` must name a `LANG_TABLE` entry. A value naming
  a grammarless built-in (jcl/hlasm), an unknown language, or another plugin's language is a loud
  per-plugin skip.
- **Duplicate refusal, including cross-mode.** Two directories overriding one language — same
  mode or one query-only plus one armed grammar override — disable **both**, loudly, naming both
  directories. `load_all` sorts directory entries so the registry, `plugins list`, and the
  descriptor are deterministic. Refused duplicates are visible in `plugins list` as
  `DISABLED: duplicate of <other dir>`.

### Honest re-extraction: the `plugin_overrides` meta key

One per-repo meta key, `plugin_overrides`, is both the gate and the audit record. Its value is the
canonical descriptor of the **effective** override set: sorted lines

```
<lang>|<mode:query|grammar>|<plugin dir basename>|<16-hex digest of cached query bytes (+ dylib bytes for grammar mode)>
```

empty string when none is active; an absent key is equivalent to empty, so pre-feature DBs and
no-override users force nothing. On any index run the stored key is compared to the live
descriptor; a mismatch prints `PLUGIN-OVERRIDE state changed: forcing full re-extraction` plus the
differing descriptor lines (`- old` / `+ new`) and sets `force_full`. This covers every honesty
case with one mechanism: `.scm` edit, dylib swap, override added/removed, env-var flip (grammar
mode is in the effective set only when armed), and a broken override falling back (it drops out of
the effective set, so a graph extracted under it re-extracts under the built-in). The descriptor
doubles as the audit trail the brief requires — extraction provenance is auditable from
`plugins list` plus this key, with **no Edge/Node schema change** and no `resolved_by` change.

**Divergence from the `extra_rules_digest` precedent — write-LAST.** The extra-rules gate writes
its key at the check site; the id-scheme gate (ADR-002 amendment) documents why that is a crash
hole: "Writing at the check site would let a crash mid-run leave a DB stamped with the new scheme
whose rows are still old — permanently mixed … Written last … idempotent." This gate mirrors the
extra-rules **input** design (sorted, raw bytes, empty-when-none, per-repo
`repo_scope::meta_key`) but adopts the id-scheme **timing**: the key is written only at run end
and in the gate-guarded no-change early return. (Moving the pre-existing `extra_rules_digest`
write to write-last is a recorded follow-up, not done here.)

**Digest input = the registry's cached bytes**, never a fresh disk read at index time. A disk-read
digest plus an `OnceLock`-cached query would let a long-lived process stamp a NEW digest over a
graph extracted with the OLD query — permanently wrong. Cached-bytes digests keep old-query /
old-digest consistent; a restart picks both up together.

**Cost, stated honestly:** the descriptor is machine-global while the key is per-repo, so ANY
override change forces a full re-extraction of **every repo in every DB on this machine** at its
next index — loudly announced. `SYMBOL_ID_SCHEME` does not bump: an override changes the node
**set**, not identity; symbol ids are stable across the re-extraction. A language-scoped force is
a named future refinement.

### Notices

All notices are **stderr only** (MCP speaks JSON-RPC on stdio; crew regex-parses `stats` stdout).
Two sites: (1) registry load — per-plugin load/skip/compile-failure/duplicate/ext-claim-refusal
lines, once per process; (2) the index gate — per-run `query override active: <lang> <- <plugin
dir>` / `GRAMMAR OVERRIDE active: <lang> <- <plugin dir>` lines plus the state-change marker with
the descriptor diff (which makes the CLI-vs-editor env-skew footgun diagnosable from output). CLI
arms that never construct extractors (`stats`, `query`, `resolve`) stay silent — "loud startup
notice" does not mean "on every invocation". `plugins list` shows override status per plugin:
active query / query FAILED / grammar armed / grammar INERT (manifest flag without the env var) /
DISABLED duplicate.

### Behavior changes owned by this ADR

- **Plugin-registry loading is unconditional on every index run.** The gate must read
  `override_state()` before classifying files, so first plugin access (including dlopen of any
  installed plugins) moves from first-lookup to index start. Plugins already ran under
  dlopen-at-first-use; only the timing moves.
- **Bench hermeticity pin.** The agent-eval bench harness (`wicked-estate-bench`) is the repo's
  truth oracle and indexed with whatever plugins the machine had. With overrides able to change
  built-in-language output, a stray manifest could silently move published baselines. The bench
  binary now pins `WICKED_ESTATE_PLUGINS` to a fresh empty temp dir at `main()` startup
  (own process, so `OnceLock` timing is safe), with one stderr notice. Unconditional: bench
  numbers must never depend on who runs them.

### Operational limits (documented, not solved here)

- **`OnceLock` / restart semantics.** The registry and `WICKED_ESTATE_PLUGIN_OVERRIDE` are read
  once per process. Long-running processes (watcher, MCP server) need a restart to pick up
  override changes; until then they run old query + old digest **consistently** (the digest is
  over cached bytes).
- **Staleness-reporting blindness.** MCP staleness derives from indexed-root file mtimes and
  cannot surface override drift: a graph extracted under an old override state reports fresh
  until its next index run. Remedy: after any override change, explicitly re-index every repo/DB.
- **Crew's clean-HEAD skip** fires on git evidence alone; override files live outside every repo,
  so an override edit produces no git evidence and crew serves the stale graph until a force
  refresh. Recorded as a wicked-crew follow-up.
- **Hermeticity exposure.** A populated plugins dir with an `override_query` manifest silently
  changes built-in-language results on that machine — that is the feature, and it is why the
  notices are loud, every test in this lane pins `WICKED_ESTATE_PLUGINS`, and the bench harness
  is pinned. Residual exposure: pre-existing fleet language tests are not pinned (other-lane
  ownership; recorded follow-up).

## Consequences

Users can patch a query gap locally the day they find it, swap a newer grammar behind an explicit
double opt-in, and audit exactly what an existing graph was extracted with. The price is a
machine-global full re-extraction on any override change (loud, id-stable) and restart-to-reload
semantics in long-lived processes.
