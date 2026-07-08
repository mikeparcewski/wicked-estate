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

# Topological order: every crate's internal deps appear before it. The vendored tree-sitter
# grammars (wicked-estate-tree-sitter-*) are deps of wicked-estate-extract, so they go first.
CRATES=(
  wicked-estate-tree-sitter-rpg
  wicked-estate-tree-sitter-vb6
  wicked-estate-tree-sitter-vba
  wicked-estate-tree-sitter-vbscript
  wicked-estate-tree-sitter-cfml
  wicked-estate-tree-sitter-abl
  wicked-estate-tree-sitter-foxpro
  wicked-estate-tree-sitter-informix4gl
  wicked-estate-tree-sitter-lotusscript
  wicked-estate-tree-sitter-powerscript
  wicked-estate-tree-sitter-crystal-formula
  wicked-estate-core
  wicked-estate-memory-core
  wicked-estate-memory-api
  wicked-estate-observe
  wicked-estate-store
  wicked-estate-extract
  wicked-estate-rank
  wicked-estate-resolve
  wicked-estate-retrieve
  wicked-estate-overlay
  wicked-estate-knowledge
  wicked-estate-memory
  wicked-estate
  wicked-estate-mcp
)

DRY="${1:-}"

# ── Skip non-publishable crates ─────────────────────────────────────────────────────────────────
# A crate with `publish = false` in its Cargo.toml (the wicked-estate-memory-api re-export shim and,
# today, wicked-estate-mcp) makes `cargo publish` HARD-ERROR ("cannot be published"). Left unhandled
# that aborts this whole ordered release even after every publishable crate succeeded. Detect
# publishability from `cargo metadata` (authoritative) rather than grepping Cargo.toml: cargo
# serializes `publish = false` as an EMPTY `publish` array; `null` (absent) or a non-empty list
# (e.g. ["crates-io"]) means publishable. Filtering at RUNTIME (vs. deleting entries) means the
# instant a crate's `publish = false` is removed it publishes again — in topological order, no edit here.
PY="$(command -v python3 || command -v python || true)"
[ -n "$PY" ] || { echo "ERROR: python3/python required for publish=false detection" >&2; exit 1; }

# Space-padded list of workspace members with `publish = false` (one metadata call; members publish via -p).
WORKSPACE_NOPUBLISH=" $(cargo metadata --no-deps --format-version 1 | "$PY" -c '
import json, sys
d = json.load(sys.stdin)
for p in d["packages"]:
    pub = p.get("publish")
    if isinstance(pub, list) and not pub:   # [] == publish = false
        print(p["name"])
' | tr "\n" " ") "

# Exit 0 when the named package in a STANDALONE manifest sets `publish = false`. Used for the
# vendored tree-sitter grammars, which are excluded from the workspace metadata queried above.
manifest_is_nopublish() {
  cargo metadata --no-deps --format-version 1 --manifest-path "$1" 2>/dev/null | "$PY" -c '
import json, sys
name = sys.argv[1]
try:
    d = json.load(sys.stdin)
    pkgs = [p for p in d["packages"] if p["name"] == name]
    pub = pkgs[0].get("publish") if pkgs else None
except Exception:
    pub = None                              # cannot determine → treat as publishable (let publish decide)
sys.exit(0 if (isinstance(pub, list) and not pub) else 1)
' "$2"
}

# Vendored grammars are EXCLUDED from the workspace, so `-p` can't address them; publish each by its
# manifest path (vendor/tree-sitter-<suffix>/Cargo.toml). Other crates are members addressable by `-p`.
for c in "${CRATES[@]}"; do
  echo ">>> $c"
  # --allow-dirty: `cargo publish` regenerates each crate's Cargo.lock during its build/verify step
  # (deps drift between releases), which trips the git-clean check on a fresh CI checkout. The lock
  # is not part of a library's published package, so the upload still matches the tagged source.
  case "$c" in
    wicked-estate-tree-sitter-*)
      suffix="${c#wicked-estate-tree-sitter-}"
      manifest="crates/wicked-estate-extract/vendor/tree-sitter-${suffix}/Cargo.toml"
      if manifest_is_nopublish "$manifest" "$c"; then
        echo "    skip — \`publish = false\` in $manifest (not publishable to crates.io)"
        continue
      fi
      PUB=(cargo publish --manifest-path "$manifest" --allow-dirty)
      ;;
    *)
      case "$WORKSPACE_NOPUBLISH" in
        *" $c "*)
          echo "    skip — \`publish = false\` (not publishable to crates.io)"
          continue
          ;;
      esac
      PUB=(cargo publish -p "$c" --allow-dirty)
      ;;
  esac
  if [ "$DRY" = "--dry-run" ]; then
    # A dry-run can only fully validate crates whose deps are already on crates.io. Dependent
    # crates pin internal deps by version (e.g. wicked-estate-core = "0.0.1") that cargo can't
    # resolve until they're published, so expected failures here are not defects — the real
    # ordered publish below validates each crate once its deps are live.
    if ! "${PUB[@]}" --dry-run --no-verify 2>&1; then
      echo "    (dry-run can't resolve not-yet-published workspace deps — validated at real publish)"
    fi
  else
    # Resumable + rate-limit-aware: skip crates already on crates.io (so re-running resumes a
    # partial publish), and retry through crates.io's NEW-crate rate limit (~1 per 10 min).
    published=0
    for attempt in $(seq 1 30); do
      if "${PUB[@]}" 2>/tmp/we-publish.err; then published=1; break; fi
      if grep -qiE "already uploaded|already exists" /tmp/we-publish.err; then
        echo "    already published — skipping"; published=1; break
      fi
      if grep -qi "429 Too Many Requests" /tmp/we-publish.err; then
        echo "    rate-limited (attempt $attempt) — waiting 120s"; sleep 120
      else
        echo "    ERROR publishing $c:"; cat /tmp/we-publish.err; exit 1
      fi
    done
    [ "$published" -eq 1 ] || { echo "    gave up on $c after 30 retries"; exit 1; }
    # Wait for the crates.io index to propagate so the next (dependent) crate resolves it.
    sleep 15
  fi
done

echo
echo "done. Users can now: cargo install wicked-estate"
