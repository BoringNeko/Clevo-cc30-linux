#!/usr/bin/env bash
#
# tests-electron-bridge.sh - guard the Electron <-> backend contract.
#
# The Electron main process (ui/electron/main.cjs) and the headless backend
# (ui/src-tauri/src/serve.rs) agree on two things that live in different
# languages and cannot be type-checked across the boundary:
#
#   1. the handshake line the backend prints on startup:
#        CLEVO_CC_BACKEND <port> <token>
#   2. the endpoint and header the bridge listens on:
#        POST /invoke  with  x-clevo-token
#
# If either side renames something, the packaged app fails at runtime with no
# compile error. This script extracts the literals from both sources and fails
# when they drift. It runs without Electron and without the daemon.
#
# Usage: scripts/tests-electron-bridge.sh

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAIN_JS="$ROOT/ui/electron/main.cjs"
PRELOAD_JS="$ROOT/ui/electron/preload.cjs"
SERVE_RS="$ROOT/ui/src-tauri/src/serve.rs"

fail=0
pass() { printf '  ok   %s\n' "$1"; }
bad()  { printf '  FAIL %s\n' "$1" >&2; fail=1; }

for f in "$MAIN_JS" "$PRELOAD_JS" "$SERVE_RS"; do
    [[ -f "$f" ]] || { bad "missing $f"; exit 1; }
done

# --- handshake prefix -------------------------------------------------------
# serve.rs prints: println!("CLEVO_CC_BACKEND {port} {token}")
# main.cjs looks for a line starting with: CLEVO_CC_BACKEND
HANDSHAKE_RS="$(grep -oE '"CLEVO_CC_BACKEND[^"]*"' "$SERVE_RS" | head -n1 | tr -d '"')"
if [[ "$HANDSHAKE_RS" == "CLEVO_CC_BACKEND "* ]]; then
    pass "backend prints the handshake line"
else
    bad "backend handshake literal not found in serve.rs (got: ${HANDSHAKE_RS:-none})"
fi

if grep -q 'startsWith("CLEVO_CC_BACKEND")' "$MAIN_JS"; then
    pass "main.cjs recognises the handshake prefix"
else
    bad "main.cjs does not match the handshake prefix"
fi

# --- endpoint and token header ---------------------------------------------
# serve.rs: path "/invoke", header "x-clevo-token"
if grep -q '"/invoke"' "$SERVE_RS" && grep -q 'x-clevo-token' "$SERVE_RS"; then
    pass "backend serves /invoke with x-clevo-token"
else
    bad "backend is missing the /invoke path or x-clevo-token header"
fi

if grep -q 'path: "/invoke"' "$MAIN_JS"; then
    pass "main.cjs posts to /invoke"
else
    bad "main.cjs does not post to /invoke"
fi

if grep -q '"x-clevo-token"' "$MAIN_JS"; then
    pass "main.cjs sends x-clevo-token"
else
    bad "main.cjs does not send x-clevo-token"
fi

# --- preload shape ----------------------------------------------------------
# The renderer detects Electron by this global (src/api/bridge.ts).
if grep -q '__CLEVO_ELECTRON__' "$PRELOAD_JS"; then
    pass "preload exposes __CLEVO_ELECTRON__"
else
    bad "preload does not expose __CLEVO_ELECTRON__"
fi

if grep -q '__CLEVO_ELECTRON__' "$ROOT/ui/src/api/bridge.ts"; then
    pass "bridge.ts reads __CLEVO_ELECTRON__"
else
    bad "bridge.ts does not read __CLEVO_ELECTRON__"
fi

# --- backend resource in electron-builder config ---------------------------
# package.json must ship the headless binary as an extra resource or the
# packaged app cannot start.
if grep -q '"clevo-cc-ui"' "$ROOT/ui/package.json" && grep -q 'extraResources' "$ROOT/ui/package.json"; then
    pass "electron-builder ships the backend binary"
else
    bad "ui/package.json does not ship the clevo-cc-ui backend"
fi

rd="$ROOT/ui/dist/index.html"
if [[ -f "$rd" ]]; then
    # The bundle is loaded by Electron over `file://`, where an absolute
    # `/assets/...` URL resolves to the filesystem root and 404s. Vite must emit
    # relative paths (`base: "./"`). This bit the Electron build once: the window
    # stayed open but rendered a blank page.
    if grep -qE '(src|href)="/assets/' "$rd"; then
        bad "ui/dist/index.html uses absolute /assets paths; set base: './' in vite.config.ts"
    else
        pass "ui/dist uses relative asset paths (file:// safe)"
    fi
fi

# The backend binary must be discoverable where main.cjs looks for it.
if grep -q 'target/electron/release' "$ROOT/ui/electron/main.cjs"; then
    pass "main.cjs looks for the headless build under target/electron/release"
else
    bad "main.cjs does not look for the headless backend build"
fi

# electron-builder must package the *headless* backend from the exclusive
# target/electron path. Pointing extraResources at target/release/ shipped the
# Tauri binary instead, which ignores --serve: the app then died after the
# backend handshake timed out (a 15 s "flash and quit").
if grep -q '"src-tauri/target/electron/release/clevo-cc-ui"' "$ROOT/ui/package.json"; then
    pass "electron-builder packages the headless backend"
else
    bad "ui/package.json extraResources does not use target/electron/release/clevo-cc-ui"
fi

if grep -q 'target/release' "$ROOT/ui/package.json"; then
    bad "ui/package.json still references target/release (collides with the Tauri binary)"
else
    pass "electron-builder config does not touch target/release"
fi

# If an Electron app tree has been built, its bundled backend must answer
# --serve. This is the check that would have caught the flash-and-quit.
BUNDLED="$ROOT/ui/release/linux-unpacked/resources/clevo-cc-ui"
if [[ -x "$BUNDLED" ]]; then
    # The backend binds a random port and exits when its stdout closes, so a
    # short timeout is enough to capture the handshake line.
    handshake="$(timeout 5 "$BUNDLED" --serve 2>/dev/null | head -n1 || true)"
    if [[ "$handshake" == CLEVO_CC_BACKEND* ]]; then
        pass "bundled backend speaks --serve (headless build)"
    else
        bad "bundled backend is not the headless build (no handshake; got: ${handshake:-none})"
    fi
else
    skip "no Electron app tree built; skipping bundled-backend check"
fi

exit "$fail"
