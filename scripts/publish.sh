#!/usr/bin/env bash
# Publish wicked-estate to crates.io in dependency order.
#
# Prereq: `cargo login <token>` (a crates.io API token with publish scope).
# crates.io publishes are IRREVERSIBLE — a version can be yanked, never deleted.
# Bump [workspace.package] version in the root Cargo.toml before re-publishing.
#
# Usage:
#   ./scripts/publish.sh             # real publish (uploads to crates.io)
#   ./scripts/publish.sh --dry-run   # package each crate without uploading
set -euo pipefail
cd "$(dirname "$0")/.."

# Topological order: every crate's internal deps appear before it.
CRATES=(
  wicked-estate-core
  wicked-estate-store
  wicked-estate-extract
  wicked-estate-rank
  wicked-estate-resolve
  wicked-estate-retrieve
  wicked-estate
  wicked-estate-mcp
  wicked-estate-bench
)

DRY="${1:-}"

for c in "${CRATES[@]}"; do
  echo ">>> $c"
  if [ "$DRY" = "--dry-run" ]; then
    # Dependent crates can't verify-build against unpublished deps; skip verify in dry-run.
    cargo publish -p "$c" --dry-run --no-verify
  else
    cargo publish -p "$c"
    # Wait for the crates.io index to propagate so the next (dependent) crate resolves it.
    sleep 15
  fi
done

echo
echo "done. Users can now: cargo install wicked-estate"
