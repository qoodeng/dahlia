#!/bin/bash
# CI's checks for the ship workflow. Each failing command prints only the tail of its output:
# Rust puts the real errors last, after thousands of lines of vendored GPUI warnings.
set -uo pipefail

run() {
    local log
    log=$(mktemp)
    if "$@" >"$log" 2>&1; then
        echo "ok: $*"
    else
        echo "FAILED: $*"
        tail -n 150 "$log"
        return 1
    fi
}

run cargo fmt --check &&
    run cargo clippy --locked --all-targets -- -D warnings &&
    run cargo test --locked
