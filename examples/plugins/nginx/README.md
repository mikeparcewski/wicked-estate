# nginx — example wicked-estate language plugin

A worked example of a **runtime language plugin**: a tree-sitter grammar that wicked-estate loads at
startup from a plugins directory, **without being compiled into the core**. Adding a language this
way needs no rebuild of wicked-estate, and the grammar's license never touches the (MIT) core — this
example is intentionally **Apache-2.0** to make that point (a GPL grammar would work identically).

## What a plugin is

Just three things in a directory — a compiled grammar, a query, and a manifest:

```
nginx/
  plugin.toml      # manifest (name, extensions, library, symbol, query, abi, license)
  libnginx.dylib   # the compiled tree-sitter grammar (.so on Linux, .dll on Windows)
  nginx.scm        # the @code_* extraction query
```

(`grammar.js` + `src/` here are the grammar source and the generated parser used to build the
library; only the three files above need to ship in the installed plugin.)

## Build & install

```sh
./build.sh                       # produces libnginx.<dylib|so> in this dir
# install into the plugins folder ($WICKED_ESTATE_PLUGINS, default ~/.wicked-estate/plugins):
cp -r . "${WICKED_ESTATE_PLUGINS:-$HOME/.wicked-estate/plugins}/nginx"
```

That's it — wicked-estate now recognises nginx:

```sh
wicked-estate index ./my-nginx-config --db graph.db   # .nginxconf / .conf files
```

The loader consults built-in languages first, then additive plugins like this one — overriding a
built-in takes explicit override fields in the manifest (see PLUGIN.md, "Overriding a built-in
language"). Incompatible or unloadable plugins are skipped with a warning rather than aborting.

## Authoring your own

See [`PLUGIN.md`](../../../PLUGIN.md) in the repo root for the full guide (manifest fields, the ABI
requirement, the `@code_*` query convention, and the license model).

This grammar is deliberately minimal (blocks + directives) — enough to demonstrate the mechanism,
not a complete nginx grammar.
