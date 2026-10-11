#!/usr/bin/env bash
# Compile the real driver profile helpers against a firmware mock.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
awk '/^enum clevo_perf_mode/ { emit=1 } emit { print } emit && /^};/ { emit=0 }' "$ROOT/kernel/clevo-cc/clevo-cc.c" >"$tmp/enums.c"
awk '/^static (int|void) clevo_cc_(perf_supported|set_perf_mode|profile_get|profile_set|profile_choices)\(/ { emit=1 } emit { print } emit && /^}/ { emit=0 }' "$ROOT/kernel/clevo-cc/clevo-cc.c" >"$tmp/perf.c"
cc -std=c11 -Wall -Wextra -Werror -I"$tmp" "$ROOT/scripts/tests-kernel-profile.c" -o "$tmp/test"
"$tmp/test"
