#!/bin/sh
# Regenerate codegraph-1.6.4.sql from repo/ with the pinned codegraph bundle.
# usage: regen.sh <extracted codegraph 1.6.4 bundle dir>
# Runs on a scratch copy with a scratch home, no inherited environment, telemetry off.
set -eu
BUNDLE=$1
HERE=$(cd "$(dirname "$0")" && pwd)
W=$(mktemp -d)
mkdir -p "$W/home/tmp"
cp -R "$HERE/repo" "$W/tree"
env -i PATH=/usr/bin:/bin HOME="$W/home" TMPDIR="$W/home/tmp" CODEGRAPH_TELEMETRY=0 DO_NOT_TRACK=1 \
  CODEGRAPH_NO_UPDATE_CHECK=1 CI=1 "$BUNDLE/node" --liftoff-only \
  --disable-warning=ExperimentalWarning --max-semi-space-size=16 \
  "$BUNDLE/lib/dist/bin/codegraph.js" init -y "$W/tree" >/dev/null
DB="file:$W/tree/.codegraph/codegraph.db?immutable=1"
{
  echo "-- codegraph 1.6.4 (darwin-arm64 release bundle, pinned) output for tests/fixtures/codegraph/repo,"
  echo "-- written by 'codegraph init -y <copy of repo>' and dumped with the sqlite3 shell: the four tables"
  echo "-- estate reads (files, nodes, edges, unresolved_refs), their exact schema, every row. Regenerate"
  echo "-- with tests/fixtures/codegraph/regen.sh after any fixture-source change."
  for t in files nodes edges unresolved_refs; do
    sqlite3 "$DB" "select sql||';' from sqlite_master where type='table' and name='$t'"
  done
  for t in files nodes edges unresolved_refs; do
    sqlite3 "$DB" ".mode insert $t" "select * from $t order by rowid"
  done
} > "$HERE/codegraph-1.6.4.sql"
rm -rf "$W"
