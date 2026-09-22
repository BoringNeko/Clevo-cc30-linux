#!/usr/bin/env bash
#
# watch-ui.sh - build and launch the UI against the *running* clevod.
#
# This is the quick way to eyeball the dashboard: it does not touch the
# hardware itself, it only asks the already-running daemon (over the system
# bus) for whatever state it has. Writes still go through the daemon and its
# PolicyKit gate, exactly like the installed app.
#
# Usage:
#   scripts/watch-ui.sh          # tauri dev (rebuilds on change)
#   scripts/watch-ui.sh --release  # run the release binary instead
#
# Requires: the daemon running (`systemctl status clevod`) and the module
# loaded (`lsmod | grep clevo`). If the daemon answers with an old property set,
# restart it first: `sudo systemctl restart clevod`.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI_DIR="$ROOT/ui"

MODE="dev"
[ "${1:-}" = "--release" ] && MODE="release"

# Same workarounds the app applies for WebKitGTK on the NVIDIA proprietary
# driver (see scripts/run-ui.sh): force DMA-BUF onto shared memory so the GL
# compositor (and thus the blur) survives.
export WEBKIT_DMABUF_RENDERER_FORCE_SHM="${WEBKIT_DMABUF_RENDERER_FORCE_SHM:-1}"

# `tauri dev` runs `pnpm install` first. In a non-interactive shell pnpm asks
# whether to purge node_modules and aborts without a TTY; CI=true takes the
# default (reuse what is there), which is what we want since deps are installed.
export CI="${CI:-true}"

echo "== daemon =="
if systemctl is-active --quiet clevod; then
    echo "   clevod: active"
else
    echo "   clevod: NOT running - start it: sudo systemctl enable --now clevod" >&2
fi
echo "   FanMode      = $(busctl --system get-property org.clevo.CC /org/clevo/CC org.clevo.CC FanMode 2>/dev/null || echo '?')"
echo "   Writable     = $(busctl --system get-property org.clevo.CC /org/clevo/CC org.clevo.CC Writable 2>/dev/null || echo '?')"
echo "   CurveWritable= $(busctl --system get-property org.clevo.CC /org/clevo/CC org.clevo.CC CurveWritable 2>/dev/null || echo 'MISSING (old daemon? restart clevod)')"
echo

cd "$UI_DIR"
if [ "$MODE" = "release" ]; then
    exec pnpm tauri build --no-bundle
else
    exec pnpm tauri dev
fi
