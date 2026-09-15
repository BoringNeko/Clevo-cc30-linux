#!/usr/bin/env bash
#
# run-ui.sh - launch the Tauri UI with the display/rendering preferences chosen
# in Settings -> Compatibility.
#
# The app itself now reads these preferences at startup (see
# ui/src-tauri/src/prefs.rs, apply_launch_env), so this is only a convenience
# for forcing the environment explicitly while developing. It is not required
# for the installed binary.
#
# Usage:
#   scripts/run-ui.sh [--dev|--build] [-- <extra tauri args>]
#
#   --dev     run `tauri dev` (default)
#   --build   run `tauri build`
#
# Everything after `--` is passed to the Tauri CLI unchanged.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI_DIR="$ROOT/ui"
PREFS="${XDG_CONFIG_HOME:-$HOME/.config}/clevo-cc/ui-launch.json"

MODE="--dev"
EXTRA=()
while [ $# -gt 0 ]; do
    case "$1" in
        --dev) MODE="--dev" ;;
        --build) MODE="--build" ;;
        --) shift; EXTRA=("$@"); break ;;
        *) EXTRA+=("$1") ;;
    esac
    shift
done

# Defaults match the app.
backend="auto"
software="false"

if [ -f "$PREFS" ]; then
    # Parse the two fields without requiring jq.
    backend="$(sed -n 's/.*"backend"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$PREFS" | head -n1)"
    software="$(sed -n 's/.*"software_rendering"[[:space:]]*:[[:space:]]*\(true\|false\).*/\1/p' "$PREFS" | head -n1)"
    backend="${backend:-auto}"
    software="${software:-false}"
fi

echo "clevo-cc UI: backend=$backend software_rendering=$software (from $PREFS)"

case "$backend" in
    wayland) export GDK_BACKEND=wayland ;;
    x11) export GDK_BACKEND=x11 ;;
    auto|"") ;; # leave the toolkit default
    *) echo "unknown backend '$backend'; using auto" >&2 ;;
esac

# Force the DMA-BUF transport onto shared memory. On the NVIDIA proprietary
# driver WebKitGTK fails to allocate a GBM buffer and aborts with `Gdk Error 71`
# before the window appears, on every compositor. This keeps the GL compositor
# (and thus blur + hardware acceleration) alive; the backend sets the same
# variable. Software rendering is the heavier fallback that disables it all.
if [ "$software" = "true" ]; then
    export WEBKIT_DISABLE_DMABUF_RENDERER=1
else
    export WEBKIT_DMABUF_RENDERER_FORCE_SHM="${WEBKIT_DMABUF_RENDERER_FORCE_SHM:-1}"
fi

cd "$UI_DIR"
if [ "$MODE" = "--build" ]; then
    exec pnpm tauri build "${EXTRA[@]}"
else
    exec pnpm tauri dev "${EXTRA[@]}"
fi
