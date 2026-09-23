#!/usr/bin/env bash
#
# electron-e2e.sh - run the real Electron shell against the built backend.
#
# This is the "does it actually work" smoke test for the Electron build:
#
#   1. builds the headless backend and the frontend if needed,
#   2. starts the backend exactly as the Electron main process does (--serve),
#      reads its handshake, and drives the HTTP bridge the renderer uses,
#   3. with a display, launches the real Electron app and checks it stays up.
#
# It needs no hardware and no daemon: the bridge contract is exercised without
# `clevod`, and a missing daemon comes back as a readable error (which is what
# the UI shows). The *data* path through a real daemon is covered separately by
# the Rust integration tests (`ui/src-tauri/tests/daemon_integration.rs`).
#
# Usage:
#   scripts/electron-e2e.sh                 # full check (windowed if a display)
#   scripts/electron-e2e.sh --headless      # skip the windowed launch
#
# Requires: node, the Electron runtime (`cd ui && pnpm install`), and a display
# for the windowed part.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

HEADLESS=0
[[ "${1:-}" == "--headless" ]] && HEADLESS=1

fail=0
pass() { printf '  ok   %s\n' "$1"; }
bad()  { printf '  FAIL %s\n' "$1" >&2; fail=1; }
skip() { printf '  skip %s\n' "$1"; }

BACKEND="$ROOT/ui/src-tauri/target/electron/release/clevo-cc-ui"
ELECTRON_BIN="$ROOT/ui/node_modules/electron/dist/electron"

# --- prerequisites ----------------------------------------------------------
command -v node >/dev/null 2>&1 || { bad "node not found"; exit 1; }
if [[ ! -x "$ELECTRON_BIN" ]]; then
    # Not an error: the runtime is a large optional download. The rest of the
    # suite covers the backend and the JS contract.
    skip "Electron runtime not installed (cd ui && pnpm install)"
    exit 0
fi

# --- build ------------------------------------------------------------------
if [[ ! -x "$BACKEND" ]]; then
    printf '==> building the headless backend\n'
    ( cd ui/src-tauri && CARGO_TARGET_DIR=target/electron cargo build --release --locked --no-default-features ) \
        || { bad "backend build failed"; exit 1; }
fi
if [[ ! -f "$ROOT/ui/dist/index.html" ]]; then
    printf '==> building the frontend\n'
    ( cd ui && ./node_modules/.bin/vite build ) || { bad "frontend build failed"; exit 1; }
fi
pass "backend and frontend are built"

# --- backend + bridge (no Electron, no daemon) ------------------------------
HANDSHAKE_FILE="$(mktemp)"
"$BACKEND" --serve >"$HANDSHAKE_FILE" 2>/dev/null &
backend_pid=$!
sleep 1

line="$(head -n1 "$HANDSHAKE_FILE")"
port="$(echo "$line" | awk '{print $2}')"
token="$(echo "$line" | awk '{print $3}')"

if [[ -z "$port" || "$token" == "" ]]; then
    bad "backend did not print a handshake (got: ${line:-none})"
    kill "$backend_pid" 2>/dev/null
    rm -f "$HANDSHAKE_FILE"
    exit 1
fi
pass "backend handshake: port $port"

# The security paths: no token and a wrong token must both be refused.
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$port/invoke" \
    -d '{"command":"get_launch_prefs","args":{}}')"
[[ "$code" == "403" ]] && pass "no token -> 403" || bad "no token returned HTTP $code (expected 403)"

code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "x-clevo-token: wrong" \
    "http://127.0.0.1:$port/invoke" -d '{"command":"get_launch_prefs","args":{}}')"
[[ "$code" == "403" ]] && pass "wrong token -> 403" || bad "wrong token returned HTTP $code (expected 403)"

# A local command (no daemon needed): the launch preferences.
reply="$(curl -s -X POST -H "x-clevo-token: $token" "http://127.0.0.1:$port/invoke" \
    -d '{"command":"get_launch_prefs","args":{}}')"
if echo "$reply" | grep -q '"ok":true'; then
    pass "bridge answered get_launch_prefs: $reply"
else
    bad "bridge did not answer get_launch_prefs: $reply"
fi

# A daemon command: with a real clevod running this returns data; without one
# it must come back as a readable error, not a hang or a crash. Either is fine
# here - what matters is that the bridge survives and answers.
reply="$(curl -s -X POST -H "x-clevo-token: $token" "http://127.0.0.1:$port/invoke" \
    -d '{"command":"get_fan_snapshot","args":{}}')"
if echo "$reply" | grep -q '"ok":true'; then
    rpm="$(echo "$reply" | sed -n 's/.*"cpu":{"available":[a-z]*,"rpm":\([0-9]*\).*/\1/p')"
    pass "live daemon reached: get_fan_snapshot ok (cpu rpm=${rpm:-?})"
elif echo "$reply" | grep -q '"ok":false'; then
    pass "no daemon reported as an error: $(echo "$reply" | head -c 90)…"
else
    bad "get_fan_snapshot returned neither data nor an error: $reply"
fi

proc_alive=1
kill -0 "$backend_pid" 2>/dev/null || proc_alive=0
[[ "$proc_alive" == "1" ]] && pass "backend survived the bad calls" || bad "backend died"

kill "$backend_pid" 2>/dev/null
wait "$backend_pid" 2>/dev/null
rm -f "$HANDSHAKE_FILE"

# --- windowed launch --------------------------------------------------------
if [[ "$HEADLESS" == "1" || ( -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ) ]]; then
    skip "windowed launch (no display or --headless)"
    exit "$fail"
fi

printf '==> launching the Electron app for a 5s smoke run\n'
log="$(mktemp)"
( cd ui && timeout 5 "$ELECTRON_BIN" . --no-sandbox ) >"$log" 2>&1
rc=$?
# `timeout` kills it with 124, which means the app stayed up: success.
if [[ "$rc" == "0" || "$rc" == "124" ]]; then
    if grep -qiE 'could not start the clevo-cc backend|Cannot find module|Uncaught|Segmentation' "$log"; then
        bad "Electron logged a startup error:"
        sed 's/^/       /' "$log" >&2
    else
        pass "Electron started and stayed up for 5s"
    fi
else
    bad "Electron exited early (rc=$rc):"
    sed 's/^/       /' "$log" >&2
fi
rm -f "$log"

exit "$fail"
