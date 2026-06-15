#!/usr/bin/env fish
# sample.fish — integration corpus fixture for the fish extractor
# Demonstrates functions, variable scoping, and typical fish idioms.

# ---------------------------------------------------------------------------
# log — write a labelled message to stderr
# ---------------------------------------------------------------------------
function log
    set -l level $argv[1]
    set -e argv[1]
    printf '[%s] %s\n' $level "$argv" >&2
end

# ---------------------------------------------------------------------------
# check_deps — verify each required command is available
# ---------------------------------------------------------------------------
function check_deps
    set -l required curl jq git
    set -l missing

    for cmd in $required
        if not command -q $cmd
            set -a missing $cmd
        end
    end

    if test (count $missing) -gt 0
        log error "Missing tools: $missing"
        return 1
    end

    log info "All dependencies present"
end

# ---------------------------------------------------------------------------
# fetch_and_parse — download JSON from a URL and emit one record per line
# ---------------------------------------------------------------------------
function fetch_and_parse
    # Local variables do not leak into the caller's scope.
    set -l url      $argv[1]
    set -l out_dir  $argv[2]
    set -l tmp_file (mktemp /tmp/fish-sample-XXXXXX.json)

    log info "Fetching $url"
    if not curl -fsSL $url -o $tmp_file
        log error "curl failed for $url"
        rm -f $tmp_file
        return 1
    end

    mkdir -p $out_dir
    jq -c '.items[]' $tmp_file > $out_dir/records.ndjson
    set -l count (jq '.items | length' $tmp_file)
    log info "Wrote $count records to $out_dir/records.ndjson"

    rm -f $tmp_file
end

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
check_deps
or exit 1

set -l target_dir /tmp/fish-output
fetch_and_parse https://example.com/data.json $target_dir
log info "Finished — output in $target_dir"
