#!/bin/bash

log_message() {
    local level="$1"
    local msg="$2"
    echo "[${level}] ${msg}"
}

validate_input() {
    local input="$1"
    if [ -z "$input" ]; then
        log_message "ERROR" "Input is empty"
        return 1
    fi
    return 0
}

process_file() {
    local path="$1"
    if validate_input "$path"; then
        log_message "INFO" "Processing: $path"
        wc -l "$path"
    fi
}

main() {
    local file="${1:-/dev/stdin}"
    process_file "$file"
    log_message "INFO" "Done"
}

main "$@"
