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
At startup wicked-estate scans it; lookups check built-in languages **first**, then plugins, so a
plugin never shadows a built-in. A plugin that fails to load (missing symbol, unreadable query,
incompatible ABI) is **skipped with a warning** rather than aborting.

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
