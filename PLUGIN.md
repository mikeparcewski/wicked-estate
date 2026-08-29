# Authoring a wicked-estate language plugin

Languages compiled into wicked-estate live in `crates/wicked-estate-extract` (the `LANG_TABLE`).
A **plugin** is the opposite: a tree-sitter grammar loaded **at runtime** from a plugins directory,
never compiled into the core. Two reasons to use one:

- **Drop-in, no rebuild.** Add a language by dropping a directory into the plugins folder.
- **License isolation.** The grammar is a separate binary artifact, never linked into the (MIT)
  core at build time — so a grammar under an incompatible license (GPL, etc.) can be used without
  its license affecting wicked-estate. The user obtains/builds the plugin and drops it in.

A worked example lives in [`examples/plugins/nginx`](examples/plugins/nginx).

## Anatomy

A plugin is a directory with three files:

```
<plugins_dir>/<name>/
  plugin.toml          # manifest
  lib<name>.dylib      # compiled tree-sitter grammar (.so on Linux, .dll on Windows)
  <name>.scm           # @code_* extraction query
```

The plugins directory is `$WICKED_ESTATE_PLUGINS` if set, otherwise `~/.wicked-estate/plugins`.
At startup wicked-estate scans it. Precedence is three-tiered
([ADR-010](docs/adr/ADR-010-plugin-overrides.md)): **built-in < query-only override < full grammar
override**. A plugin with no override fields is *additive* — lookups check built-in languages
first, then plugins, so an additive plugin cannot shadow a built-in. Override plugins (below)
deliberately can. A plugin that fails to load (missing symbol, unreadable query, incompatible ABI)
is **skipped with a warning** rather than aborting; a broken *override* query additionally falls
back to the built-in, loudly.

## `plugin.toml`

```toml
name       = "nginx"                 # what `for_language("nginx")` matches
extensions = ["nginxconf", "conf"]   # file extensions (no dot) for extension dispatch
library    = "libnginx"              # base name; .so/.dylib/.dll + `lib` prefix tried automatically
symbol     = "tree_sitter_nginx"     # exported C grammar fn (default: tree_sitter_<name>)
query      = "nginx.scm"             # extraction query, relative to this dir
abi        = 14                      # informational; the runtime loads ABI 13–15
license    = "Apache-2.0"            # informational; may differ from the MIT core
caps       = ["symbols"]             # informational
```

`library` is required unless `override_query` is set (a query-only override loads no native code).
Two more fields exist for overriding built-ins — `override_query` and `override` — described in
the next section. Unknown manifest keys are warned about by name (a typo like `override-query`
is visible, never silently dropped).

## Overriding a built-in language (ADR-010)

Built-in queries have gaps, and grammars age. Overrides let you patch either locally, without
waiting for a release. Two tiers:

### Query-only override — shipped grammar, your query

A plugin directory with just a manifest and a `.scm`:

```
<plugins_dir>/ts-patch/
  plugin.toml
  typescript.scm       # your query — REPLACES the built-in one
```

```toml
name           = "ts-patch"
query          = "typescript.scm"
override_query = "typescript"        # the built-in LANG_TABLE entry to override
```

No shared library, no `dlopen` — the shipped grammar runs your query. Activation is
manifest-only: dropping the directory in is the opt-in. Start by copying the shipped `.scm`
(`crates/wicked-estate-extract/src/queries/<lang>.scm`) and adding patterns — the override is a
**wholesale replacement**, not a merge: a pattern the built-in has and your file lacks is a
construct you stop extracting (announced by design, so keep the superset).

Overrides match **per language entry**, not per family: `typescript` and `tsx` are separate
entries, so patching TS everywhere usually means two override directories. `override_query` must
name a built-in tree-sitter language; naming a grammarless built-in (jcl, hlasm), an unknown
language, or another plugin's language is a loud skip. Extension claims on a query-only override
are ignored — dispatch follows the built-in entry.

### Full grammar override — your grammar AND your query

Belt-and-braces double opt-in, never silent:

1. `override = true` in `plugin.toml` (with a normal `library`/`symbol`/`query` triple whose
   `name` is the built-in language), **and**
2. the language named in `WICKED_ESTATE_PLUGIN_OVERRIDE` (comma-separated exact names, e.g.
   `WICKED_ESTATE_PLUGIN_OVERRIDE=typescript,tsx`).

The manifest flag alone is INERT (visible as such in `plugins list`); the env var alone does
nothing. Armed, the plugin grammar + query replace the built-in pair for that language, and the
plugin's extension claims are honored — except a claim on an extension **owned by a different
built-in** (e.g. a typescript override claiming `py`), which is refused loudly unless that
owner is *also* named in `WICKED_ESTATE_PLUGIN_OVERRIDE`. The double opt-in is per captured
language, not per plugin.

### Safety rules (both tiers)

- **Eager compile, loud fallback.** Override queries compile at registry load (query-only:
  against the built-in grammar; grammar tier: against the plugin's own grammar). A failed
  compile prints a `QUERY-OVERRIDE:` / `GRAMMAR-OVERRIDE:` marker and the built-in extractor
  stays in use — an override can never make a built-in language unavailable or silently delete
  indexed files.
- **Duplicates disable each other.** Two directories overriding one language — same mode or
  mixed — are both refused, loudly, and shown as `DISABLED: duplicate` in `plugins list`.
- **Everything is announced.** Registry load prints per-plugin notices; every index run prints
  `query override active: <lang> <- <dir>` / `GRAMMAR OVERRIDE active: <lang> <- <dir>` on
  stderr; `wicked-estate plugins list` shows each override's state (active / FAILED / armed /
  INERT / DISABLED).

### Re-extraction, audit, and operational caveats

- **Any override change forces a full re-extraction — of every repo on this machine.** The
  effective override set is digested into a per-repo `plugin_overrides` meta key (the audit
  record of what a graph was extracted with); the set itself is machine-global, so after any
  override edit/add/remove/arm/disarm, **every repo in every DB on this machine re-extracts
  fully at its next index**, loudly announced. Symbol ids are stable across it.
- **Restart long-lived processes.** The registry loads once per process; the watcher and the
  MCP server keep running the old query + old digest (consistently) until restarted.
- **Staleness reporting cannot see override drift** — it derives from file mtimes. After any
  override change, explicitly re-index every repo/DB. Crew-managed graphs additionally skip
  refresh on a clean HEAD: force a refresh there.
- **Overrides change results machine-locally.** A populated plugins dir with an override
  manifest changes built-in-language extraction on that machine — that is the feature. If a
  CLI run and your editor's MCP server disagree, check `WICKED_ESTATE_PLUGIN_OVERRIDE` in each
  environment; the state-change marker prints the old→new descriptor diff to make the skew
  diagnosable.

## The query

Standard wicked-estate `@code_*` convention — definitions as `@code_<kind>.def` + `@code_<kind>.name`,
calls as `@call` + `@call.function`. Valid kinds: `function`, `method`, `class`, `struct`, `enum`,
`interface`, `module`, `namespace`, `constructor`, `constant`, `variable`, `field`/`property`,
`type`. Example:

```scheme
(block name: (identifier) @code_module.name) @code_module.def
```

## Building the grammar into a shared library

A plugin's library is just a compiled tree-sitter parser exporting `tree_sitter_<name>`. The
simplest path (no Rust required):

```sh
# 1. author grammar.js, then generate the parser (ABI 13–15; 14 is the safe default)
npx tree-sitter generate --abi 14
# 2. compile to a shared library
cc -shared -fPIC -O2 -I src -o libnginx.dylib src/parser.c   # + src/scanner.c if present
```

If your grammar already exists as a Rust crate, build a `cdylib` that re-exports its
`tree_sitter_<name>` symbol instead. Either way the result is a single `.so`/`.dylib`/`.dll`.

The grammar's tree-sitter **ABI must be 13–15** (the wicked-estate runtime is tree-sitter 0.25).
The loader verifies this and rejects anything outside the range.

## Install & use

```sh
mkdir -p ~/.wicked-estate/plugins
cp -r examples/plugins/nginx ~/.wicked-estate/plugins/nginx   # after ./build.sh
wicked-estate index ./my-config --db graph.db                 # nginx files now recognised
```

## Safety

Loading a plugin `dlopen`s native code, which is inherently unsafe. wicked-estate validates the
grammar's ABI after loading and keeps the library resident for the process lifetime so the grammar
function pointer stays valid. Only install plugins you trust — a plugin is native code running in
your process.
