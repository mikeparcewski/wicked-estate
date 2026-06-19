#!/usr/bin/env bash
# Build the nginx example plugin's shared library from the generated parser.
#
# Produces libnginx.<dylib|so> next to this script — the same directory as plugin.toml / nginx.scm,
# which together form the drop-in plugin. No Rust, no link against wicked-estate: a plugin is just a
# compiled tree-sitter grammar + a query + a manifest.
set -euo pipefail
cd "$(dirname "$0")"

case "$(uname -s)" in
  Darwin) EXT=dylib ;;
  MINGW*|MSYS*|CYGWIN*) EXT=dll ;;
  *) EXT=so ;;
esac

CC="${CC:-cc}"
"$CC" -shared -fPIC -O2 -w -I src -o "libnginx.${EXT}" src/parser.c
echo "built $(pwd)/libnginx.${EXT}"
echo
echo "Install:  cp -r \"$(pwd)\" \"\${WICKED_ESTATE_PLUGINS:-\$HOME/.wicked-estate/plugins}/nginx\""
