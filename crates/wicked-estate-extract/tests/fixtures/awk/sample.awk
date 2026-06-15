#!/usr/bin/env awk -f
# sample.awk — integration corpus fixture for the awk extractor
# Processes a CSV-ish log file: timestamp,level,message
# Usage: awk -f sample.awk access.log

BEGIN {
    FS      = ","
    OFS     = "\t"
    errors  = 0
    warns   = 0
    infos   = 0
    total   = 0
    print "=== Log Summary ==="
}

# Skip header line
NR == 1 { next }

# Count lines by log level (pattern rule 1)
$2 == "ERROR" {
    errors++
    total++
    log_record($1, $2, $3)
}

# Accumulate non-error lines (pattern rule 2)
$2 != "ERROR" {
    if ($2 == "WARN")  warns++
    if ($2 == "INFO")  infos++
    total++
}

# ---------------------------------------------------------------------------
# log_record — emit a formatted line for notable records
# ---------------------------------------------------------------------------
function log_record(ts, lvl, msg,    formatted) {
    formatted = sprintf("[%s] %-5s %s", ts, lvl, msg)
    print formatted
}

# ---------------------------------------------------------------------------
# pct — compute percentage, guarding against divide-by-zero
# ---------------------------------------------------------------------------
function pct(part, whole) {
    if (whole == 0) return "0.0"
    return sprintf("%.1f", (part / whole) * 100)
}

END {
    print "---"
    print "Total lines : " total
    print "Errors      : " errors " (" pct(errors, total) "%)"
    print "Warnings    : " warns  " (" pct(warns,  total) "%)"
    print "Info        : " infos  " (" pct(infos,  total) "%)"
}
