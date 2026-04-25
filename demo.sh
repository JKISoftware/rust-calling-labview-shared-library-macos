#!/usr/bin/env bash
# Run the reproducer in each demonstration mode and print the exit code
# observed for each. The default mode and the first two workarounds
# (`forget`, `nodelete`) are expected to produce SIGSEGV (exit 139) on
# macOS; only `exit` produces a clean exit (0).
set -u
cd "$(dirname "$0")"

cargo build -q

bin=./target/debug/rust-calling-labview-shared-library-macos

run_mode() {
    local mode="$1"
    local label="$2"
    printf '\n========================================================================\n'
    printf '=== %s\n' "$label"
    printf '========================================================================\n'
    if [ -z "$mode" ]; then
        "$bin"
    else
        WORKAROUND="$mode" "$bin"
    fi
    printf 'EXIT CODE: %d\n' "$?"
}

run_mode ""         'default — no workaround (expect SIGSEGV, exit 139)'
run_mode "forget"   'WORKAROUND=forget — skip Library::drop dlclose (expect 139)'
run_mode "nodelete" 'WORKAROUND=nodelete — RTLD_NODELETE (expect 139)'
run_mode "exit"     'WORKAROUND=exit — libc::_exit, skip destructors (expect 0)'
