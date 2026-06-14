#!/usr/bin/env bash
# =============================================================================
# blast-check-pre-commit.sh — W7.2 semantic blast-radius pre-commit gate
# =============================================================================
#
# PURPOSE
#   Extracts changed symbol names from staged diffs, runs `wicked-estate
#   blast-radius <name>` against .wicked-estate/graph.db for each, sums the
#   resolved-dependent counts, and compares the total against a configurable
#   budget.
#
#   The gate is SEMANTIC (symbol-level), not file-LOC counting. It will only
#   block when the code graph is populated and `wicked-estate` is built. If
#   either is absent it FAILS OPEN with a notice — infra absence never
#   blocks a developer.
#
# BUDGET BEHAVIOUR
#   CI_BLAST_BUDGET   max total resolved dependents (default: 50)
#   70% of budget →  WARN (non-blocking, printed to stderr)
#  100% of budget →  BLOCK (exit 1 with details)
#   graph/binary missing → FAIL-OPEN (notice printed, exit 0)
#
# NOTE ON PRECISION
#   The gate surfaces the `coverage:` line emitted by `wicked-estate
#   blast-radius` verbatim for every symbol.  That line always reads:
#     "coverage: N resolved dependent(s); M unresolved call(s) — best-effort
#      static resolution, MAY be incomplete (precise tier pending)"
#   This means the dependent count shown is a LOWER BOUND.  Do not treat
#   a count of 0 as "safe to change" — read the coverage line.
#
# INSTALL
#   Symlink into your repo's git hook directory:
#
#     ln -sf "$(pwd)/scripts/blast-check-pre-commit.sh" .git/hooks/pre-commit
#     chmod +x .git/hooks/pre-commit
#
#   Or, if you use a hooks manager (lefthook, husky, etc.) just point it at
#   this script.
#
# ENVIRONMENT VARIABLES
#   CI_BLAST_BUDGET        integer, max resolved dependents before BLOCK
#                          (default: 50)
#   CI_BLAST_DB            path to the SQLite graph db
#                          (default: .wicked-estate/graph.db)
#   CI_BLAST_BINARY        path to the wicked-estate binary
#                          (default: first `wicked-estate` on PATH, then
#                           ./target/release/wicked-estate,
#                           ./target/debug/wicked-estate)
#   CI_BLAST_SKIP          set to any non-empty value to skip this hook
#   CI_BLAST_TIMEOUT       seconds before a single blast-radius call is
#                          abandoned (default: 10, gate fails open on timeout)
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# 0. Kill switch
# ---------------------------------------------------------------------------
if [[ -n "${CI_BLAST_SKIP:-}" ]]; then
    echo "[blast-check] CI_BLAST_SKIP set — skipping (fail-open)" >&2
    exit 0
fi

# ---------------------------------------------------------------------------
# 1. Configuration
# ---------------------------------------------------------------------------
BUDGET="${CI_BLAST_BUDGET:-50}"
DB="${CI_BLAST_DB:-.wicked-estate/graph.db}"
TIMEOUT_SEC="${CI_BLAST_TIMEOUT:-10}"

# Locate the binary; any absence → fail-open
resolve_binary() {
    if [[ -n "${CI_BLAST_BINARY:-}" ]]; then
        if [[ -x "${CI_BLAST_BINARY}" ]]; then
            echo "${CI_BLAST_BINARY}"
            return 0
        fi
        echo "[blast-check] CI_BLAST_BINARY='${CI_BLAST_BINARY}' not executable — skipping (fail-open)" >&2
        return 1
    fi
    if command -v wicked-estate >/dev/null 2>&1; then
        echo "wicked-estate"
        return 0
    fi
    for candidate in ./target/release/wicked-estate ./target/debug/wicked-estate; do
        if [[ -x "${candidate}" ]]; then
            echo "${candidate}"
            return 0
        fi
    done
    return 1
}

BINARY=""
if ! BINARY="$(resolve_binary)"; then
    echo "[blast-check] 'wicked-estate' binary not found (build with 'cargo build --release') — skipping (fail-open)" >&2
    exit 0
fi

if [[ ! -f "${DB}" ]]; then
    echo "[blast-check] graph db '${DB}' not found (run 'wicked-estate index .') — skipping (fail-open)" >&2
    exit 0
fi

# ---------------------------------------------------------------------------
# 2. Collect changed function/class/impl symbol names from the staged diff
#    Best-effort grep over the diff's "+" lines.  We look for the most
#    common top-level definition patterns across languages supported by
#    wicked_estate (Rust fn/struct/enum/trait/impl, Python def/class,
#    JS/TS function/class, Go func, Java/C# class).
#    This is intentionally a heuristic surface — the graph resolves the
#    semantic blast radius; we only need plausible symbol names to query it.
# ---------------------------------------------------------------------------
STAGED_DIFF="$(git diff --cached -U0 2>/dev/null || true)"

if [[ -z "${STAGED_DIFF}" ]]; then
    echo "[blast-check] no staged changes — nothing to check" >&2
    exit 0
fi

# Extract candidate symbol names from added/changed lines only.
# We capture the first identifier after common definition keywords.
SYMBOLS="$(
    echo "${STAGED_DIFF}" \
    | grep '^+' \
    | grep -v '^+++' \
    | grep -oE '(^[+]\s*)(pub\s+)?(async\s+)?(fn|struct|enum|trait|impl|def|class|function|func|type|interface)\s+([A-Za-z_][A-Za-z0-9_]*)' \
    | grep -oE '[A-Za-z_][A-Za-z0-9_]*$' \
    | sort -u \
    || true
)"

if [[ -z "${SYMBOLS}" ]]; then
    echo "[blast-check] no recognisable top-level symbol definitions in staged diff — skipping" >&2
    exit 0
fi

SYMBOL_COUNT="$(echo "${SYMBOLS}" | wc -l | tr -d ' ')"
echo "[blast-check] checking ${SYMBOL_COUNT} changed symbol(s) against graph (budget: ${BUDGET} resolved dependents)" >&2

# ---------------------------------------------------------------------------
# 3. Run blast-radius per symbol, sum resolved dependents, surface coverage
# ---------------------------------------------------------------------------
TOTAL_DEPENDENTS=0
CHECKED=0
SKIPPED_TIMEOUT=0

while IFS= read -r sym; do
    [[ -z "${sym}" ]] && continue

    # Run with a timeout; on timeout → warn and skip (fail-open for this symbol)
    BR_OUTPUT=""
    if command -v timeout >/dev/null 2>&1; then
        BR_OUTPUT="$(timeout "${TIMEOUT_SEC}" "${BINARY}" blast-radius "${sym}" --db "${DB}" 2>/dev/null || true)"
    else
        # macOS: gtimeout from coreutils, or use python as a timeout wrapper
        BR_OUTPUT="$(
            python3 -c "
import subprocess, sys
result = subprocess.run(
    ['${BINARY}', 'blast-radius', '${sym}', '--db', '${DB}'],
    capture_output=True, text=True, timeout=${TIMEOUT_SEC}
)
sys.stdout.write(result.stdout)
" 2>/dev/null || true)"
    fi

    if [[ -z "${BR_OUTPUT}" ]]; then
        # Binary ran but returned nothing, or timed out — fail-open for this symbol
        SKIPPED_TIMEOUT=$((SKIPPED_TIMEOUT + 1))
        echo "[blast-check]   ${sym}: timeout or no output — skipped (fail-open)" >&2
        continue
    fi

    CHECKED=$((CHECKED + 1))

    # Extract the resolved-dependent count from output lines like:
    #   "7 symbol(s) depend on 'foo':"        → count = 7
    #   "no resolved dependents for 'foo'"    → count = 0
    DEPS=0
    COUNT_LINE="$(echo "${BR_OUTPUT}" | grep -E "^[0-9]+ symbol\(s\) depend on" || true)"
    if [[ -n "${COUNT_LINE}" ]]; then
        DEPS="$(echo "${COUNT_LINE}" | grep -oE '^[0-9]+')"
    fi

    TOTAL_DEPENDENTS=$((TOTAL_DEPENDENTS + DEPS))

    # Surface the coverage line verbatim — this is the honest disclaimer
    COV_LINE="$(echo "${BR_OUTPUT}" | grep '^coverage:' || true)"
    if [[ -n "${COV_LINE}" ]]; then
        echo "[blast-check]   ${sym}: ${DEPS} resolved dependent(s)" >&2
        echo "[blast-check]          ${COV_LINE}" >&2
    else
        echo "[blast-check]   ${sym}: ${DEPS} resolved dependent(s)" >&2
    fi
done <<< "${SYMBOLS}"

# ---------------------------------------------------------------------------
# 4. Budget evaluation
# ---------------------------------------------------------------------------
if [[ "${CHECKED}" -eq 0 ]]; then
    if [[ "${SKIPPED_TIMEOUT}" -gt 0 ]]; then
        echo "[blast-check] all symbols timed out — skipping (fail-open)" >&2
    else
        echo "[blast-check] no symbols yielded graph results — skipping (fail-open)" >&2
    fi
    exit 0
fi

# Compute 70% warn threshold (integer arithmetic, round down)
WARN_AT=$(( BUDGET * 70 / 100 ))

echo "" >&2
echo "[blast-check] total resolved dependents across ${CHECKED} symbol(s): ${TOTAL_DEPENDENTS} / ${BUDGET}" >&2

if [[ "${TOTAL_DEPENDENTS}" -ge "${BUDGET}" ]]; then
    echo "" >&2
    echo "  BLOCKED: blast-radius exceeds budget (${TOTAL_DEPENDENTS} >= ${BUDGET})." >&2
    echo "" >&2
    echo "  This is a best-effort semantic estimate (static resolution, MAY be" >&2
    echo "  incomplete — see coverage lines above).  The gate exists to surface" >&2
    echo "  high-impact changes early, not to imply false precision." >&2
    echo "" >&2
    echo "  Options:" >&2
    echo "    1. Split the change into smaller commits." >&2
    echo "    2. Increase the budget: CI_BLAST_BUDGET=<N> git commit ..." >&2
    echo "    3. Skip (emergency only): CI_BLAST_SKIP=1 git commit ..." >&2
    echo "" >&2
    exit 1
elif [[ "${TOTAL_DEPENDENTS}" -ge "${WARN_AT}" ]]; then
    echo "" >&2
    echo "  WARNING: blast-radius at $(( TOTAL_DEPENDENTS * 100 / BUDGET ))% of budget" >&2
    echo "  (${TOTAL_DEPENDENTS} / ${BUDGET}).  Consider splitting this change." >&2
    echo "" >&2
    # Warn only — do not block
    exit 0
else
    echo "  OK ($(( TOTAL_DEPENDENTS * 100 / BUDGET ))% of budget)" >&2
    exit 0
fi
