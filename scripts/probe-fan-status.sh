#!/usr/bin/env bash
#
# S5 targeted re-probe of command 12 (fan status), read-only.
#
# Goal: resolve why the live reply (42 bytes, RPM1/2/3 = 0) does not match
# PK0C's declared 0x100 buffer layout.
#
# Read-only: command 12 only reads EC fields.
#
set -euo pipefail

DSM="\\_SB.DCHU._DSM"
GUID="e424f293dcfbbf4badd6db71bdc0afad"

if [ ! -e /proc/acpi/call ]; then echo "ERROR: /proc/acpi/call missing" >&2; exit 1; fi
if [ "$(id -u)" -ne 0 ]; then echo "ERROR: run as root" >&2; exit 1; fi

call_raw() {
    printf '%s' "$1" > /proc/acpi/call
    sleep 0.2
    cat /proc/acpi/call
}

decode() {
    python3 - "$1" <<'PY'
import sys, re
raw = sys.argv[1]
m = re.search(r'\{([^}]*)\}', raw)
if not m:
    print("   (non-buffer):", raw.strip())
    sys.exit(0)
vals = [int(x, 16) for x in re.findall(r'0x([0-9a-fA-F]+)', m.group(1))]
print(f"   length={len(vals)}")
if len(vals) >= 8:
    def le(i): return vals[i] | (vals[i+1] << 8)
    def be(i): return (vals[i] << 8) | vals[i+1]
    print(f"   [2..3] LE={le(2):5}  BE={be(2):5}")
    print(f"   [4..5] LE={le(4):5}  BE={be(4):5}")
    print(f"   [6..7] LE={le(6):5}  BE={be(6):5}")
for i in range(0, min(len(vals), 0x1A)):
    print(f"   [{i:#04x}]={vals[i]:#04x}", end="")
    if i % 8 == 7: print()
print()
PY
}

echo "== command 12, 5 samples =="
for i in 1 2 3 4 5; do
    r=$(call_raw "${DSM} b${GUID} 0 12 b00")
    echo "sample ${i}: ${r}"
done

echo
echo "== decode of last sample =="
decode "$(call_raw "${DSM} b${GUID} 0 12 b00")"

echo
echo "== command 13 decode for reference =="
decode "$(call_raw "${DSM} b${GUID} 0 13 b00")"

echo
echo "Done. Paste everything above."
echo "For a loaded-fan sample:"
echo "  yes > /dev/null & yes > /dev/null & sleep 20; sudo $0; kill %1 %2"
