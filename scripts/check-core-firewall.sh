#!/bin/sh
# CI firewall (PR-2 / architecture B′): `wicked-estate-core` must NEVER depend on any memory crate.
# The memory interface seam (`wicked-estate-memory-api`) and any `wicked-memory*` impl sit DOWNSTREAM
# of core; if core ever pulls one in, the lean code-graph engine has been contaminated. Fails CI.
set -eu
cd "$(dirname "$0")/.."
tree="$(cargo tree -p wicked-estate-core -e normal 2>/dev/null || true)"
if printf '%s\n' "$tree" | grep -Eiq 'wicked-memory|wicked-estate-memory-api'; then
  echo "FIREWALL VIOLATION: wicked-estate-core depends on a memory crate:" >&2
  printf '%s\n' "$tree" | grep -Ei 'wicked-memory|wicked-estate-memory-api' >&2
  exit 1
fi
echo "OK: wicked-estate-core has zero dependency on memory crates."
