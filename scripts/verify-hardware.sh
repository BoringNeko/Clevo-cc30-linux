#!/usr/bin/env bash
#
# verify-hardware.sh - real-machine verification of the fan control stack.
#
# Runs the checks that cannot be done offline, in increasing order of risk, and
# stops before anything irreversible:
#
#   1. ACPI read-only   acpi_call fan status / fan curve (no writes)
#   2. Driver read-only hwmon rpm + temperature, sysfs curve read
#   3. Mode round-trip  fan_mode auto -> max -> auto (reversible)
#   4. Curve round-trip write a curve, read it back, restore the original
#
# Step 4 is the one that needs care: it saves the current curve first and
# restores it at the end, and it leaves the fan mode at `auto` so the machine
# returns to firmware control even if the restore is skipped.
#
# Usage:
#   sudo scripts/verify-hardware.sh --step 1   # read-only ACPI only
#   sudo scripts/verify-hardware.sh --step 4   # the full run (default)
#   scripts/verify-hardware.sh --yes          # do not prompt (CI-ish use)
#
# Requirements: root (for acpi_call / sysfs), the acpi_call module for step 1,
# and the clevo-cc module loaded for steps 2-4.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

STEP=4
ASSUME_YES=0
while [ $# -gt 0 ]; do
    case "$1" in
        --step) STEP="${2:?}"; shift 2 ;;
        --yes)  ASSUME_YES=1; shift ;;
        -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

PLATFORM="${CLEVO_PLATFORM:-/sys/devices/platform/CLV0001:00}"
DSM_PATH='\_SB.DCHU._DSM'
GUID='e424f293dcfbbf4badd6db71bdc0afad'
CLI="$ROOT/target/release/clevo-cc"

# Resolve this driver's hwmon directory once; steps 2-4 all need it and may be
# run individually with --step.
find_hwmon() {
    local dir
    for dir in /sys/class/hwmon/hwmon*; do
        [ -e "$dir/fan1_input" ] || continue
        case "$(cat "$dir/name" 2>/dev/null)" in
            clevo_cc) printf '%s' "$dir"; return 0 ;;
        esac
    done
    return 1
}
HWMON=""
if [ -d "$PLATFORM" ]; then
    HWMON="$(find_hwmon || true)"
fi

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
die()   { red "ERROR: $*"; exit 1; }

pause() {
    [ "$ASSUME_YES" = 1 ] && return 0
    printf '%s' "$* [y/N] "
    read -r reply
    case "$reply" in y|Y|yes) return 0 ;; *) return 1 ;; esac
}

step_header() {
    echo
    bold "=============================================================="
    bold "== Step $1: $2"
    bold "=============================================================="
}

if [ "$(id -u)" -ne 0 ]; then
    die "run as root (acpi_call and sysfs need it):  sudo $0 $*"
fi

# Preflight: report what is actually loadable so the run is not a mystery.
# `test -w` is useless here: root passes it for any file. Read the mode bits.
attr_mode() {
    local perms
    [ -e "$1" ] || { echo '<absent>'; return; }
    perms="$(stat -c '%A' "$1" 2>/dev/null)"   # e.g. -rw-r--r-- or -r--r--r--
    # Characters 2-10 are the permission triplets; any 'w' means writable.
    case "${perms:1}" in
        *w*) echo writable ;;
        *)   echo read-only ;;
    esac
}
echo "preflight:"
printf '  platform device : %s\n' "$([ -d "$PLATFORM" ] && echo "$PLATFORM" || echo '<absent>')"
printf '  hwmon           : %s\n' "${HWMON:-<absent>}"
printf '  acpi_call       : %s\n' "$([ -e /proc/acpi/call ] && echo loaded || echo '<not loaded>')"
printf '  fan_mode        : %s\n' "$(attr_mode "$PLATFORM/fan_mode")"
printf '  fan_curve       : %s\n' "$(attr_mode "$PLATFORM/fan_curve")"
if [ -e "$PLATFORM/fan_curve" ]; then
    case "$(attr_mode "$PLATFORM/fan_curve")" in
        writable) : ;;
        *) echo "  hint: fan_curve is read-only, so the loaded module predates the"
           echo "        command-14 support. Load the freshly built module:"
           echo "          sudo rmmod clevo_cc && sudo insmod kernel/clevo-cc/clevo-cc.ko" ;;
    esac
fi
echo

if [ ! -x "$CLI" ]; then
    echo "building the CLI..."
    (cd "$ROOT" && cargo build --release -p clevo-cc-cli) || die "cargo build failed"
fi

# ---------------------------------------------------------------------------
# Step 1: ACPI read-only (acpi_call)
# ---------------------------------------------------------------------------
if [ "$STEP" -ge 1 ]; then
    step_header 1 "ACPI read-only (acpi_call, commands 12 and 13)"
    if [ ! -e /proc/acpi/call ]; then
        echo "skipping: /proc/acpi/call missing."
        echo "  load it with:  sudo modprobe acpi_call"
    else
        echo "-- fan status (command 12):"
        "$CLI" --transport acpi-call fan status || red "   ...failed"
        echo
        echo "-- fan curve (command 13):"
        "$CLI" --transport acpi-call fan curve || red "   ...failed"
        echo
        echo "Check: the temperatures here must be plausible for this machine at"
        echo "idle. Compare against your other sensors if you can; this is the"
        echo "one place the raw-vs-Celsius question is settled on real hardware."
        echo
        echo "-- raw reply (for the record):"
        printf '%s' "$DSM_PATH b$GUID 0 12 b00" > /proc/acpi/call
        sleep 0.3
        head -c 300 /proc/acpi/call; echo
    fi
fi

# ---------------------------------------------------------------------------
# Step 2: driver read-only
# ---------------------------------------------------------------------------
if [ "$STEP" -ge 2 ]; then
    step_header 2 "Kernel driver read-only (hwmon + sysfs)"
    if [ ! -d "$PLATFORM" ]; then
        echo "skipping: $PLATFORM not present."
        echo "  load it with:  sudo modprobe clevo-cc"
    else
        for f in fan_mode perf_mode; do
            if [ -r "$PLATFORM/$f" ]; then
                printf '%-12s %s\n' "$f" "$(cat "$PLATFORM/$f")"
            else
                printf '%-12s %s\n' "$f" "<unreadable: $(
                    [ -e "$PLATFORM/$f" ] && echo 'permission denied' || echo 'absent')>"
            fi
        done
        if [ -n "$HWMON" ]; then
            echo "-- hwmon in $HWMON"
            for f in fan1_input fan2_input temp1_input temp2_input; do
                [ -e "$HWMON/$f" ] || continue
                value="$(cat "$HWMON/$f" 2>/dev/null || echo "<unreadable>")"
                printf '  %-12s %s\n' "$f" "$value"
            done
            echo "  (temp*_input is millidegrees; unreadable = the EC reports none)"
            if [ ! -e "$HWMON/temp1_input" ]; then
                echo "  note: this module has no temperature channels - it predates"
                echo "        the cmd-12 offset fix. Load the freshly built one to test"
                echo "        temperatures:  sudo insmod kernel/clevo-cc/clevo-cc.ko"
            fi
        else
            echo "-- no clevo_cc hwmon directory found"
        fi
        echo
        echo "-- current curve:"
        cat "$PLATFORM/fan_curve" 2>/dev/null || red "   ...unreadable"
    fi
fi

# ---------------------------------------------------------------------------
# Step 3: fan mode round-trip (reversible)
# ---------------------------------------------------------------------------
if [ "$STEP" -ge 3 ]; then
    step_header 3 "Fan mode round-trip: auto -> max -> auto"
    if [ ! -w "$PLATFORM/fan_mode" ]; then
        echo "skipping: $PLATFORM/fan_mode not writable"
    elif pause "This briefly forces the fans to full speed. Continue?"; then
        original="$(cat "$PLATFORM/fan_mode")"
        echo "current: $original"

        echo "-> max"
        echo max > "$PLATFORM/fan_mode" || red "   write failed"
        sleep 4
        cat "$PLATFORM/fan_mode"
        [ -n "$HWMON" ] && cat "$HWMON/fan1_input" 2>/dev/null | sed 's/^/   fan1_input = /'

        echo "-> auto (restore)"
        echo auto > "$PLATFORM/fan_mode" || red "   write failed"
        sleep 3
        cat "$PLATFORM/fan_mode"
        [ -n "$HWMON" ] && cat "$HWMON/fan1_input" 2>/dev/null | sed 's/^/   fan1_input = /'
        green "Expected: max is clearly faster than auto, and the mode returns to auto."
    else
        echo "skipped"
    fi
fi

# ---------------------------------------------------------------------------
# Step 4: curve write round-trip (saves and restores)
# ---------------------------------------------------------------------------
if [ "$STEP" -ge 4 ]; then
    step_header 4 "Custom curve round-trip (command 14)"
    echo "This is the newest path. It saves the current curve, writes a test"
    echo "curve, reads it back, and restores the original. The fan mode is left"
    echo "at 'auto' at the end so the firmware is always back in control."
    if [ ! -w "$PLATFORM/fan_curve" ]; then
        echo "skipping: $PLATFORM/fan_curve not writable"
    elif pause "Write a test curve to the EC?"; then
        backup="$(cat "$PLATFORM/fan_curve")"
        echo "-- saved current curve:"
        echo "$backup" | sed 's/^/   /'

        # A deliberately distinctive curve: 45°C/30% and 70°C/80%.
        test_curve="cpu: 0,0 45,76 70,204 0,0
gpu1: 0,0 45,76 70,204 0,0"

        echo "-- writing test curve"
        printf '%s\n' "$test_curve" > "$PLATFORM/fan_curve" \
            || red "   write failed (check dmesg)"

        sleep 1
        echo "-- read back:"
        readback="$(cat "$PLATFORM/fan_curve")"
        echo "$readback" | sed 's/^/   /'

        # Compare only the two points we set (the EC owns T1/T4).
        if printf '%s' "$readback" | grep -qE 'cpu: 0,0 45,(7[0-9]|8[0-9]) 70,(19[0-9]|20[0-9])'; then
            green "CURVE WRITE VERIFIED: the EC accepted and returned the new points"
        else
            red "MISMATCH: the EC did not return the points that were written."
            red "The curve may have been rejected or reordered - inspect the"
            red "read-back above against the test curve before trusting this path."
        fi

        echo
        if pause "Restore the saved curve now?"; then
            printf '%s\n' "$backup" > "$PLATFORM/fan_curve" || red "   restore failed"
            sleep 1
            echo "-- after restore:"
            cat "$PLATFORM/fan_curve" | sed 's/^/   /'
        else
            red "NOT RESTORED - run: printf '%s\\n' \"\$backup\" > $PLATFORM/fan_curve"
        fi

        echo "-- returning fan mode to auto"
        echo auto > "$PLATFORM/fan_mode" 2>/dev/null || true
        cat "$PLATFORM/fan_mode"
    else
        echo "skipped"
    fi
fi

echo
bold "=============================================================="
bold "== done"
bold "=============================================================="
echo "Please relay the output above, especially:"
echo "  * step 1 temperatures vs. your other sensors"
echo "  * step 2 temp*_input values"
echo "  * step 4 whether the read-back matched the test curve"
