#!/usr/bin/env bash
#
# S5 — real-machine ACPI verification.
#
# Read-only: dumps the ACPI tables, decompiles them, and reports whether the
# DCHU device and _DSM GUID are present. It does NOT load acpi_call and does
# NOT write to the hardware.
#
# Usage:
#   ./scripts/verify-acpi.sh [output-dir]
#
# Requirements (Arch/CachyOS):
#   sudo pacman -S --needed acpica
#
set -euo pipefail

OUT_DIR="${1:-acpi-verify}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUID_UPPER="93F224E4-FBDC-4BBF-ADD6-DB71BDC0AFAD"
GUID_LOWER="93f224e4-fbdc-4bbf-add6-db71bdc0afad"

echo "== S5 ACPI verification =="
echo "output dir: $OUT_DIR"

if ! command -v acpidump >/dev/null 2>&1; then
    echo "ERROR: acpidump not found. Install with: sudo pacman -S acpica" >&2
    exit 1
fi
if ! command -v iasl >/dev/null 2>&1; then
    echo "ERROR: iasl not found. Install with: sudo pacman -S acpica" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"
cd "$OUT_DIR"

echo
echo "== 1/4 dumping ACPI tables (requires root) =="
sudo acpidump -b

echo
echo "== 2/4 decompiling =="
shopt -s nullglob
for f in dsdt.dat DSDT.dat ssdt*.dat SSDT*.dat; do
    [ -e "$f" ] || continue
    iasl -d "$f" >/dev/null 2>&1 || true
done
shopt -u nullglob

echo
echo "== 3/4 looking for CLV0001 / CLV0002 =="
grep -n -i "CLV0001\|CLV0002" ./*.dsl || echo "(no CLV device found in DSL)"

echo
echo "== 4/4 looking for the DCHU _DSM GUID =="
grep -n -i "$GUID_UPPER\|$GUID_LOWER" ./*.dsl || echo "(GUID not found; may be constructed at runtime)"

echo
echo "== summary files =="
ls -la ./*.dsl 2>/dev/null || echo "(no .dsl produced)"

echo
echo "Done. Please send back:"
echo "  - the CLV0001 / _DSM grep output above"
echo "  - the Device(CLV*) block and its Method(_DSM,...) signature"
echo "  - any OperationRegion (EmbeddedControl) FAN fields"
