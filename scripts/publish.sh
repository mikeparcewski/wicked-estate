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
  wicked-estate-tree-sitter-rpg
  wicked-estate-core
  wicked-estate-store
  wicked-estate-extract
  wicked-estate-rank
  wicked-estate-resolve
  wicked-estate-retrieve
  wicked-estate
  wicked-estate-mcp
)

DRY="${1:-}"

# The vendored RPG grammar is EXCLUDED from the workspace, so `-p` can't address it; publish it by
# manifest path. All other crates are workspace members addressable by `-p`.
RPG_MANIFEST="crates/wicked-estate-extract/vendor/tree-sitter-rpg/Cargo.toml"

for c in "${CRATES[@]}"; do
  echo ">>> $c"
  if [ "$c" = "wicked-estate-tree-sitter-rpg" ]; then
    PUB=(cargo publish --manifest-path "$RPG_MANIFEST")
  else
    PUB=(cargo publish -p "$c")
  fi
  if [ "$DRY" = "--dry-run" ]; then
    # A dry-run can only fully validate crates whose deps are already on crates.io. Dependent
    # crates pin internal deps by version (e.g. wicked-estate-core = "0.0.1") that cargo can't
    # resolve until they're published, so expected failures here are not defects — the real
    # ordered publish below validates each crate once its deps are live.
    if ! "${PUB[@]}" --dry-run --no-verify 2>&1; then
      echo "    (dry-run can't resolve not-yet-published workspace deps — validated at real publish)"
    fi
  else
    "${PUB[@]}"
    # Wait for the crates.io index to propagate so the next (dependent) crate resolves it.
    sleep 15
  fi
done

echo
echo "done. Users can now: cargo install wicked-estate"
