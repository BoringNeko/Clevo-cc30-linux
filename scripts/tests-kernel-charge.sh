#!/usr/bin/env bash
# Exercise actual FlexiCharger helpers with a firmware emulator; no hardware I/O.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
awk '/^struct clevo_charge_window/ { emit=1 } emit { print } emit && /^};/ { emit=0 }' "$ROOT/kernel/clevo-cc/clevo-cc.c" >"$tmp/window.c"
awk '/^static (int|bool) clevo_cc_charge_(contains|values_valid|read|write|matches|set)\(/ { emit=1 } emit { print } emit && /^}/ { emit=0 }' "$ROOT/kernel/clevo-cc/clevo-cc.c" >"$tmp/charge.c"
awk '/^static ssize_t flexicharger_store\(/ { emit=1 } emit { print } emit && /^}/ { emit=0 }' "$ROOT/kernel/clevo-cc/clevo-cc.c" >"$tmp/store.c"
cc -std=c11 -Wall -Wextra -Werror -I"$tmp" "$ROOT/scripts/tests-kernel-charge.c" -o "$tmp/test"
"$tmp/test"
