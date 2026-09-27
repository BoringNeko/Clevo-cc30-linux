#!/usr/bin/env bash
#
# tests-install-driver-reload.sh - the installer must switch a live kernel module
# to the freshly installed one.
#
# `modprobe clevo-cc` is a no-op when the module is already loaded, so an upgrade
# used to leave the old code running while the new .ko sat on disk. The sysfs
# attributes kept their old shape (`fan_curve` stayed read-only) and the write
# failure looked like a permissions bug rather than a stale module. The installer
# now compares srcversions and reloads on a mismatch.
#
# The installer cannot be run here (it writes to /usr and needs root), so the
# `reload_driver_if_stale` function is extracted and driven through stubs: a fake
# /sys/module tree (via SYS_MODULE_DIR), a fake `modinfo`, and recording
# `rmmod`/`modprobe`. No real module is touched.
#
# Usage: scripts/tests-install-driver-reload.sh

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL="$ROOT/packaging/install.sh"

fail=0
pass() { printf '  ok   %s\n' "$1"; }
bad() { printf '  FAIL %s\n' "$1" >&2; fail=1; }

# Extract the function body (from its definition to the next top-level `while`).
BODY="$(awk '/^reload_driver_if_stale\(\)/,/^}/' "$INSTALL")"
if [[ -z "$BODY" ]]; then
    echo "could not extract reload_driver_if_stale from $INSTALL" >&2
    exit 1
fi

# Run the function against a fake environment.
#
# $1: loaded srcversion, or "" when the module is not loaded
# $2: installed srcversion reported by modinfo
# $3: srcversion to report after a successful reload
# $4: "ok" to let rmmod/modprobe succeed, "fail" to make them fail
# Prints: the log/warn lines and the sequence of module operations.
run_case() {
    # Distinct names: the function under test declares `local installed`, which
    # would shadow a stub variable of the same name.
    STUB_LOADED="$1"
    STUB_INSTALLED="$2"
    STUB_AFTER="$3"
    STUB_RELOAD="$4"
    local sysdir ops
    sysdir="$(mktemp -d)"
    ops="$(mktemp)"

    if [[ -n "$STUB_LOADED" ]]; then
        mkdir -p "${sysdir}/clevo_cc"
        printf '%s\n' "$STUB_LOADED" >"${sysdir}/clevo_cc/srcversion"
    fi

    (
        SYS_MODULE_DIR="$sysdir"
        warn() { printf 'warn: %s\n' "$*"; }
        log() { printf 'log: %s\n' "$*"; }

        modinfo() {
            # Called as `modinfo -F srcversion clevo-cc`; report the installed
            # value supplied to the case.
            printf '%s\n' "$STUB_INSTALLED"
        }
        rmmod() {
            printf 'rmmod %s\n' "$*" >>"$ops"
            [[ "$STUB_RELOAD" == "ok" ]] || return 1
            # The module is now unloaded.
            rm -f "${SYS_MODULE_DIR}/clevo_cc/srcversion"
            return 0
        }
        modprobe() {
            printf 'modprobe %s\n' "$*" >>"$ops"
            [[ "$STUB_RELOAD" == "ok" ]] || return 1
            mkdir -p "${SYS_MODULE_DIR}/clevo_cc"
            printf '%s\n' "${STUB_AFTER:-$STUB_INSTALLED}" >"${SYS_MODULE_DIR}/clevo_cc/srcversion"
            return 0
        }
        export -f modinfo rmmod modprobe 2>/dev/null || true

        eval "$BODY"
        reload_driver_if_stale
        echo "--- ops ---"
        cat "$ops"
    )
    rm -rf "$sysdir" "$ops"
}

echo "install.sh: reloading a stale clevo-cc module"

# 1. Loaded and installed match: nothing is reloaded.
out="$(run_case AAAA AAAA AAAA ok)"
if grep -q 'rmmod' <<<"$out"; then
    bad "matching srcversions must not reload; got: $out"
else
    pass "a matching module is left alone"
fi

# 2. Loaded is older than installed: rmmod then modprobe.
out="$(run_case OLD NEW NEW ok)"
if grep -q 'rmmod clevo_cc' <<<"$out" && grep -q 'modprobe clevo-cc' <<<"$out"; then
    pass "a stale module is reloaded (rmmod + modprobe)"
else
    bad "a stale module must be reloaded; got: $out"
fi

# 3. After reload, the srcversion must be the installed one (reported as success).
out="$(run_case OLD NEW NEW ok)"
if grep -q 'reloaded clevo-cc (srcversion NEW)' <<<"$out"; then
    pass "a successful reload reports the new srcversion"
else
    bad "a successful reload must report the new srcversion; got: $out"
fi

# 4. rmmod fails (module in use): warn with the manual command, do not claim success.
out="$(run_case OLD NEW NEW fail)"
if grep -q 'could not reload clevo-cc' <<<"$out" && grep -q 'rmmod clevo_cc && sudo modprobe clevo-cc' <<<"$out"; then
    pass "a failed reload warns and gives the manual command"
else
    bad "a failed reload must warn with guidance; got: $out"
fi
if grep -q 'reloaded clevo-cc (srcversion' <<<"$out"; then
    bad "a failed reload must not claim success; got: $out"
else
    pass "a failed reload does not claim success"
fi

# 5. Not loaded at all: just modprobe, never rmmod.
out="$(run_case "" NEW NEW ok)"
if grep -q 'modprobe clevo-cc' <<<"$out" && ! grep -q 'rmmod' <<<"$out"; then
    pass "an unloaded module is only modprobed"
else
    bad "an unloaded module must not be rmmod'ed; got: $out"
fi

# 6. Reload happened but the version still does not match: warn, do not lie.
out="$(run_case OLD NEW STILLOLD ok)"
if grep -q 'expected NEW' <<<"$out"; then
    pass "a reload that did not take effect is reported"
else
    bad "a reload that did not take effect must be reported; got: $out"
fi

if [[ "$fail" -eq 0 ]]; then
    echo "== install.sh driver-reload checks PASSED"
else
    echo "== install.sh driver-reload checks FAILED" >&2
fi
exit "$fail"
