#!/usr/bin/env bash
#
# run-tests.sh - run the full offline test suite exactly as CI does.
#
# Usage:
#   scripts/run-tests.sh            # everything that can run without hardware
#   scripts/run-tests.sh --rust     # Rust workspace only
#   scripts/run-tests.sh --ui       # Tauri UI only
#   scripts/run-tests.sh --kernel   # build the kernel module only
#
# Nothing here touches the hardware: the Rust and UI suites run against
# hand-written fixtures, and the kernel step only compiles the module.
#
# Requirements: cargo, rustfmt, clippy, node + pnpm (UI), a session bus for the
# D-Bus integration tests (dbus-run-session), kernel headers (kernel step).

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WANT_RUST=0
WANT_UI=0
WANT_KERNEL=0
if [ $# -eq 0 ]; then
    WANT_RUST=1; WANT_UI=1; WANT_KERNEL=1
else
    for arg in "$@"; do
        case "$arg" in
            --rust)   WANT_RUST=1 ;;
            --ui)     WANT_UI=1 ;;
            --kernel) WANT_KERNEL=1 ;;
            -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
            *) echo "unknown option: $arg" >&2; exit 2 ;;
        esac
    done
fi

FAILED=0
step() {
    echo
    echo "=============================================================="
    echo "== $*"
    echo "=============================================================="
}
# Run a command, remember failure, keep going so one run shows every problem.
try() {
    if "$@"; then
        echo "-- OK: $*"
    else
        echo "!! FAILED: $*" >&2
        FAILED=1
    fi
}

# D-Bus integration tests are skipped (with a message) when no session bus is
# available; `dbus-run-session` provides a private one, matching CI.
bust() {
    if command -v dbus-run-session >/dev/null 2>&1; then
        dbus-run-session -- "$@"
    else
        echo "note: dbus-run-session not found; D-Bus tests will self-skip"
        "$@"
    fi
}

if [ "$WANT_RUST" = 1 ]; then
    step "Rust: formatting"
    try cargo fmt --all -- --check

    step "Rust: clippy (deny warnings)"
    try cargo clippy --workspace --all-targets --all-features -- -D warnings

    step "Rust: workspace tests"
    try bust cargo test --workspace
fi

if [ "$WANT_UI" = 1 ]; then
    if ! command -v node >/dev/null 2>&1; then
        echo "!! skipping UI: node not found" >&2
        FAILED=1
    else
        step "UI: frontend dependencies"
        if [ ! -d ui/node_modules ]; then
            try pnpm --dir ui install --frozen-lockfile
        fi

        step "UI: typecheck"
        try ui/node_modules/.bin/tsc --noEmit -p ui/tsconfig.json

        step "UI: frontend tests"
        try bash -c "cd ui && ./node_modules/.bin/vitest run"

        step "UI: Rust backend tests (incl. D-Bus)"
        try bash -c "cd ui/src-tauri && $(command -v dbus-run-session >/dev/null 2>&1 \
            && echo dbus-run-session -- || true) cargo test"
    fi
fi

# Offline (needs no root and touches no files): the installer must restart a
# daemon that is already running, or an upgrade keeps serving the old binary.
step "Packaging: install.sh activation logic"
try scripts/tests-install-restart.sh

if [ "$WANT_KERNEL" = 1 ]; then
    step "Kernel: build the clevo-cc module (from clean)"
    if [ -d "/lib/modules/$(uname -r)/build" ]; then
        # Build in a *copy*, from clean. Building in place reuses stale .o files
        # and hides syntax errors: a broken source tree built 'successfully'
        # because the object was left over from before the breakage, while DKMS
        # (which builds fresh in /usr/src) failed. Copying also keeps the tree
        # free of intermediate files.
        if try bash -c '
            set -e
            tmp="$(mktemp -d)"
            trap "rm -rf \"$tmp\"" EXIT
            cp kernel/clevo-cc/clevo-cc.c kernel/clevo-cc/Makefile "$tmp/"
            cd "$tmp"
            make KERNELRELEASE="$(uname -r)" >/dev/null
            test -f clevo-cc.ko
        '; then
            echo "   (clean build produced clevo-cc.ko)"
        fi
    else
        echo "!! skipping kernel: no headers for $(uname -r)" >&2
        echo "   install them (linux-headers / kernel-devel) to build the module"
    fi
fi

echo
echo "=============================================================="
if [ "$FAILED" = 0 ]; then
    echo "== ALL SELECTED CHECKS PASSED"
else
    echo "== SOME CHECKS FAILED (see !! above)"
fi
echo "=============================================================="
exit "$FAILED"
