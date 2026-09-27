#!/usr/bin/env bash
#
# tests-install-ui-replace.sh - the installer must stop a running desktop UI
# before copying over it.
#
# `cp` cannot overwrite a binary that is currently executing: the write fails
# with ETXTBSY ("text file busy") and the install aborts partway through the app
# tree, leaving it half-updated. The installer now stops the running UI first.
#
# The installer cannot be run here (it writes to /usr and needs root), so
# `stop_running_ui` is extracted and driven through stub `pgrep`/`pkill`/`sleep`.
# No real process is touched.
#
# Usage: scripts/tests-install-ui-replace.sh

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL="$ROOT/packaging/install.sh"

fail=0
pass() { printf '  ok   %s\n' "$1"; }
bad() { printf '  FAIL %s\n' "$1" >&2; fail=1; }

BODY="$(awk '/^stop_running_ui\(\)/,/^}/' "$INSTALL")"
if [[ -z "$BODY" ]]; then
    echo "could not extract stop_running_ui from $INSTALL" >&2
    exit 1
fi

# Run the function with a stubbed process table.
#
# $1: how many `pgrep` calls must still report a match before it clears
#     ("0" = never running; "always" = never exits)
# Prints the pkill calls and any warnings.
run_case() {
    local running="$1"
    local pgrep_calls=0

    (
        # `pgrep -f PATTERN` -> 0 (match) or 1 (no match).
        pgrep() {
            case "$running" in
                0) return 1 ;;
                always) return 0 ;;
                *)
                    pgrep_calls=$((pgrep_calls + 1))
                    if (( pgrep_calls <= running )); then return 0; else return 1; fi
                    ;;
            esac
        }
        pkill() { printf 'pkill %s\n' "$*"; return 0; }
        sleep() { :; }
        log() { printf 'log: %s\n' "$*"; }
        warn() { printf 'warn: %s\n' "$*"; }
        export -f pgrep pkill 2>/dev/null || true

        eval "$BODY"
        stop_running_ui "/usr/bin/clevo-cc-ui( |\$)" "Tauri UI"
    )
}

echo "install.sh: replacing a running desktop UI"

# 1. Nothing running: no pkill, no noise.
out="$(run_case 0)"
if grep -q 'pkill' <<<"$out"; then
    bad "a stopped UI must not be killed; got: $out"
else
    pass "a stopped UI is left alone"
fi

# 2. Running and exits promptly: killed once, no warning.
out="$(run_case 1)"
if grep -q 'pkill' <<<"$out"; then
    pass "a running UI is stopped before the copy"
else
    bad "a running UI must be stopped; got: $out"
fi
if grep -q 'still running' <<<"$out"; then
    bad "a UI that exits must not warn; got: $out"
else
    pass "a UI that exits promptly does not warn"
fi

# 3. Still running after the wait: warn, so the copy failure is explained.
out="$(run_case always)"
if grep -q 'still running' <<<"$out"; then
    pass "a UI that will not exit is reported"
else
    bad "a stuck UI must be reported; got: $out"
fi

# 4. The stop happens before the copy in the Electron installer.
if awk '/^install_electron_ui\(\)/,/^}/' "$INSTALL" \
    | grep -A4 'stop_running_ui' >/dev/null; then
    pass "the Electron installer stops the UI first"
else
    bad "install_electron_ui must stop the UI before copying"
fi

# 5. The copy unlinks before writing, so a lingering mapping cannot fail it.
if grep -q 'cp -a --remove-destination' "$INSTALL"; then
    pass "the app tree is copied with --remove-destination"
else
    bad "the app tree copy must use --remove-destination"
fi

# 6. A stale build must be rebuilt, not silently reused.
#
# The old guards only built when the artifact was missing, so after the first
# build a UI change never reached the installation. Both UI installers must check
# freshness, not mere existence.
if grep -q '\-newer' "$INSTALL"; then
    pass "the UI build guards check freshness (-newer), not just existence"
else
    bad "the UI build guards must not rely on the artifact merely existing"
fi
if grep -q 'unpacked_stale' "$INSTALL" && grep -q 'backend_stale' "$INSTALL"; then
    pass "both the Electron app tree and its backend are freshness-checked"
else
    bad "the Electron app tree and backend must both be freshness-checked"
fi

if [[ "$fail" -eq 0 ]]; then
    echo "== install.sh UI-replace checks PASSED"
else
    echo "== install.sh UI-replace checks FAILED" >&2
fi
exit "$fail"
