#!/usr/bin/env bash
set -euo pipefail

readonly LOG_LEVEL="${LOG_LEVEL:-info}"
readonly MAX_RETRIES=3

log() {
    local level="$1"; shift
    echo "[$(date -u +%FT%TZ)] [$level] $*" >&2
}

retry() {
    local -r cmd="$1"; shift
    local attempt=0
    until "$cmd" "$@"; do
        attempt=$(( attempt + 1 ))
        if [[ $attempt -ge $MAX_RETRIES ]]; then
            log error "Command failed after $MAX_RETRIES attempts: $cmd"
            return 1
        fi
        log warn "Attempt $attempt failed, retrying in ${attempt}s..."
        sleep "$attempt"
    done
}

check_dependencies() {
    local -a missing=()
    for dep in curl jq git; do
        command -v "$dep" &>/dev/null || missing+=("$dep")
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        log error "Missing dependencies: ${missing[*]}"
        return 1
    fi
    log info "All dependencies present"
}

fetch_json() {
    local url="$1"
    log info "Fetching $url"
    retry curl -fsSL "$url"
}

main() {
    check_dependencies
    local data
    data="$(fetch_json "https://api.example.com/status")"
    log info "Status: $(echo "$data" | jq -r '.status')"
}

main "$@"
