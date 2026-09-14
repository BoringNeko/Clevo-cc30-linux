#!/usr/bin/env bash
#
# S5 targeted probe: dump command 12 and decode every possible RPM field.
#
# Read-only. Run once at idle and once with the fans clearly spinning, then
# compare: the correct offset/endianness will change with load and land in the
# low thousands.
#
set -euo pipefail

DSM="\\_SB.DCHU._DSM"
GUID="e424f293dcfbbf4badd6db71bdc0afad"

if [ ! -e /proc/acpi/call ]; then echo "ERROR: load acpi_call first" >&2; exit 1; fi
if [ "$(id -u)" -ne 0 ]; then echo "ERROR: run as root" >&2; exit 1; fi

call() {
    printf '%s' "${DSM} b${GUID} 0 12 b00" > /proc/acpi/call
    sleep 0.3
    cat /proc/acpi/call
}

echo "== command 12 raw =="
RAW=$(call)
echo "$RAW"
echo
echo "== decoded =="
python3 - "$RAW" <<'PY'
import sys, re
raw = sys.argv[1]
# acpi_call prints "{0x.., 0x..," possibly truncated; parse all 0xNN tokens.
vals = [int(x, 16) for x in re.findall(r'0x([0-9a-fA-F]{1,2})', raw)]
print(f"length = {len(vals)}")
print("\n== all 16-bit LE pairs (offset: LE / BE) ==")
for i in range(0, len(vals) - 1):
    le = vals[i] | (vals[i + 1] << 8)
    be = (vals[i] << 8) | vals[i + 1]
    tag = ""
    for v, n in [(le, "LE"), (be, "BE")]:
        if 800 <= v <= 8000:
            tag += f"  {n} plausible"
    print(f"  [{i:#04x}:{i+1:#04x}]  LE={le:6}  BE={be:6}{tag}")
print("\n== bytes ==")
for i, v in enumerate(vals):
    print(f"  [{i:#04x}]={v:#04x}", end="")
    if i % 8 == 7:
        print()
print()
PY
