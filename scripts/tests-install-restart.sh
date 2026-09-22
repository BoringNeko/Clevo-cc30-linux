#!/usr/bin/env bash
#
# tests-install-restart.sh - the installer must switch a live daemon to the new
# binary.
#
# This is a regression guard. `systemctl enable --now` starts a stopped unit but
# leaves a running one alone, so an install over a live daemon used to leave the
# old process serving until the next boot. The symptom is an error that points
# at the source rather than at the stale process, and it has cost real debugging
# time more than once.
#
# The installer cannot be run here (it writes to /usr and needs root), so the
# activate section is exercised through a fake `systemctl` that records what was
# asked of it. Only that section is extracted, so no files are touched.
#
# Usage: scripts/tests-install-restart.sh

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL="$ROOT/packaging/install.sh"

fail=0
pass() { printf '  ok   %s\n' "$1"; }
bad() { printf '  FAIL %s\n' "$1" >&2; fail=1; }

# Extract the activate section and run it with the given stub state.
#
# $1: "active" or "inactive" - what `systemctl is-active` reports
# $2: extra installer flags (e.g. --enable)
run_section() {
    local state="$1" flags="${2:-}"
    local log
    log="$(mktemp)"

    # The section starts at the marker and ends at the `echo` before "done.".
    local body
    body="$(awk '/# --- 8\. activate/,/^echo$/' "$INSTALL" | sed '$d')"

    (
        # Minimal scaffolding the section expects.
        DRY_RUN=0
        ENABLE=0
        warn() { printf 'warn: %s\n' "$*"; }
        log() { printf 'log: %s\n' "$*"; }
        # shellcheck disable=SC2034
        for f in $flags; do
            case "$f" in
                --enable) ENABLE=1 ;;
            esac
        done

        systemctl() {
            case "$1" in
                is-active) [ "$state" = "active" ] ;;
                restart)   echo "restart" >>"$log" ;;
                enable)    echo "enable" >>"$log" ;;
            esac
        }
        export -f systemctl 2>/dev/null || true

        eval "$body"
        cat "$log"
    )
    rm -f "$log"
}

echo "install.sh: activating the daemon"

# A running daemon must be restarted, with or without --enable.
for flags in "" "--enable"; do
    label="${flags:-（no flags）}"
    out="$(run_section active "$flags")"
    if grep -q restart <<<"$out"; then
        pass "an active daemon is restarted ($label)"
    else
        bad "an active daemon was NOT restarted ($label); got: $out"
    fi
done

# A stopped daemon is only started when --enable asks for it.
out="$(run_section inactive "")"
if [ -z "$out" ]; then
    pass "a stopped daemon is left alone without --enable"
else
    bad "without --enable a stopped daemon must not be started; got: $out"
fi

out="$(run_section inactive "--enable")"
if grep -q enable <<<"$out"; then
    pass "a stopped daemon is enabled and started with --enable"
else
    bad "with --enable a stopped daemon must be started; got: $out"
fi

if [ "$fail" -eq 0 ]; then
    echo "== install.sh activation checks PASSED"
else
    echo "== install.sh activation checks FAILED" >&2
fi
exit "$fail"
