#!/usr/bin/env bash
#
# curve-test.sh - fan-curve write stress test on real hardware.
#
# Exercises command 14 through the kernel driver's sysfs interface:
#   1. backs the current curve up
#   2. writes a known curve N times, verifying each channel by read-back
#   3. restores the original
#
# Each fan line is written in its own sysfs call and checked individually, so a
# failure names the exact channel. Verification reads the value back; it does
# not trust the shell's exit status, because a multi-line `printf > file` is
# split into several write() calls and the exit code only reflects the last.
#
# The test curve uses T2 < T3 for every channel, including an absent fan, so a
# placeholder channel cannot be mistaken for a failure.
#
# Usage:
#   sudo scripts/curve-test.sh [rounds]     # default 3
#
# Requires: the clevo-cc module loaded (with fan_curve write support) and root.

set -uo pipefail

P="${CLEVO_PLATFORM:-/sys/devices/platform/CLV0001:00}"
ROUNDS="${1:-3}"

# A curve that every channel can accept (strictly increasing T2 < T3).
TEST_CPU="cpu: 0,0 50,100 70,170 0,0"
TEST_GPU1="gpu1: 0,0 55,110 75,180 0,0"
# Expected middle points after a successful write.
CPU_RE='^cpu: [0-9]+,[0-9]+ 50,100 70,170 [0-9]+,[0-9]+$'
GPU1_RE='^gpu1: [0-9]+,[0-9]+ 55,110 75,180 [0-9]+,[0-9]+$'

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: run as root (fan_curve is 0644 root-owned): sudo $0 $*" >&2
    exit 2
fi
if [ ! -e "$P/fan_curve" ]; then
    echo "ERROR: $P/fan_curve not found. Is clevo-cc loaded?" >&2
    exit 2
fi

write_line() {
    # One sysfs write per call; return the real result.
    printf '%s\n' "$1" > "$P/fan_curve" 2>/dev/null
}

fan_of() { printf '%s' "$1" | grep -oE '^[a-z0-9]+'; }

dmesg_tail() {
    dmesg 2>/dev/null | grep -iE "fan_curve|_DSM function" | tail -"${1:-3}"
}

echo "########## 备份 ##########"
before="$(cat "$P/fan_curve")"
printf '%s\n' "$before"
printf '%s\n' "$before" > /tmp/clevo-curve-backup.txt
echo "(备份写入 /tmp/clevo-curve-backup.txt)"
echo

fail=0
for round in $(seq 1 "$ROUNDS"); do
    echo "########## 轮次 $round/$ROUNDS ##########"

    for line in "$TEST_CPU" "$TEST_GPU1"; do
        fan="$(fan_of "$line")"
        if write_line "$line"; then
            echo "  写 $fan : OK"
        else
            echo "  写 $fan : FAILED"
            dmesg_tail 2 | sed 's/^/      /'
            fail=1
        fi
    done
    sleep 1

    after="$(cat "$P/fan_curve")"
    printf '  读回:\n%s\n' "$after" | sed 's/^/    /'

    if printf '%s\n' "$after" | grep -qE "$CPU_RE"; then
        echo "  cpu  校验: OK"
    else
        echo "  cpu  校验: FAILED"
        fail=1
    fi
    if printf '%s\n' "$after" | grep -qE "$GPU1_RE"; then
        echo "  gpu1 校验: OK"
    else
        echo "  gpu1 校验: FAILED"
        fail=1
    fi
    echo
done

echo "########## 还原 ##########"
# One sysfs write per line; a placeholder channel with unusable points is
# legitimately refused and must not fail the run.
while IFS= read -r line; do
    [ -z "$line" ] && continue
    case "$line" in
        cpu:*|gpu1:*|gpu2:*) ;;
        *) continue ;;
    esac
    fan="$(fan_of "$line")"
    if write_line "$line"; then
        echo "  还原 $fan : OK"
    else
        echo "  还原 $fan : 被拒绝（该通道无可用曲线，属正常）"
    fi
done <<< "$before"
sleep 1

echo "  最终曲线:"
cat "$P/fan_curve" | sed 's/^/    /'

echo
if [ "$fail" = 0 ]; then
    echo ">>> 全部 $ROUNDS 轮通过"
else
    echo ">>> 有失败；dmesg 摘要："
    dmesg_tail 10
fi

echo
echo ">>> 内核健康检查（应全部为 0）："
for pat in "usercopy_abort" "kernel BUG" "Oops" "ACPI Error"; do
    printf '    %-16s %s\n' "$pat" "$(dmesg 2>/dev/null | grep -icE "$pat")"
done
