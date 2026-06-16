#!/usr/bin/env bash
# Layer-3 (heavy-repo) validation for community detection.
#
# Operator-run, NOT CI: indexes a real repo, runs Louvain, and reports the quality metrics a human
# reviews — community count, modularity, max-community fraction (the mega-community gate), and the
# top symbols per cluster so you can eyeball whether clusters align to real modules.
#
# Usage:
#   scripts/community-validation/validate-clusters.sh <repo-path> [resolution] [db-path]
#
# Targets (what "good" looks like):
#   - wicked-estate self-index: clusters should align to crate boundaries (extract/store/core/...),
#     modularity > 0.6, max-fraction < 0.30.
#   - Apache Kafka (~200k symbols, stress): NO community > ~5k symbols; max-fraction stays small.
#     This is the case where the old union-find backend produced one mega-community.
#   - React (TypeScript, semantic mode with --embeddings + fastembed): the named-pair oracle —
#     useState/useReducer cluster together; ReactDOM.render lands elsewhere.
set -euo pipefail

REPO="${1:?usage: validate-clusters.sh <repo-path> [resolution] [db-path]}"
RES="${2:-1.0}"
DB="${3:-/tmp/we-cluster-validate.db}"
BIN="${WICKED_ESTATE_BIN:-./target/release/wicked-estate}"

if [ ! -x "$BIN" ]; then
  BIN="./target/debug/wicked-estate"
fi

echo "Indexing $REPO → $DB (binary: $BIN)"
"$BIN" index "$REPO" --db "$DB" | tail -3

echo "Clustering at resolution=$RES"
"$BIN" clusters --json --resolution "$RES" --db "$DB" 2>/dev/null \
  | python3 "$(dirname "$0")/report.py"
