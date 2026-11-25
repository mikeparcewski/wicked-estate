#!/bin/sh
# Regenerates tests/fixtures/store_v0_35_10.db (wicked-estate#601): a graph exactly as the
# wicked-estate 0.35.10 release CLI writes it, so the read-only currency check is tested against a
# real older writer's store, not a hand-dropped table.
# usage: make_store_v0_35_10.sh <wicked-estate 0.35.10 binary> <out.db>
# Needs sqlite3. Runs in a scratch directory with a scratch HOME; nothing outside it is touched.
set -eu
# Both paths are resolved before the script changes directory, so relative ones work.
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
OUT=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
"$BIN" version | grep -qx 'wicked-estate 0.35.10' || { echo "need the 0.35.10 CLI" >&2; exit 1; }
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
mkdir -p "$W/home" "$W/repo/src"
printf '%s\n' 'export function alpha() { return beta(); }' 'export function beta() { return 1; }' > "$W/repo/src/a.ts"
(cd "$W" && env -i PATH=/usr/bin:/bin HOME="$W/home" WICKED_HOME="$W/home/.wicked" \
  WICKED_ESTATE_EMIT_PROGRAM=wicked-bus-absent WICKED_ESTATE_EMIT_DEADLETTER="$W/emit.ndjson" \
  GIT_CEILING_DIRECTORIES="$W" "$BIN" index repo --db graph.db >/dev/null 2>&1)
# The one change after the CLI: the absolute indexed root (a scratch path) becomes '/repo'. A
# value, not the shape; VACUUM drops the old value's page copy.
sqlite3 "$W/graph.db" "UPDATE meta SET v='/repo' WHERE k='indexed_root'; VACUUM;"
cp "$W/graph.db" "$OUT"
