#!/usr/bin/env bash
#
# check-webview-teardown.sh - regression guard for the WebKit exit leak.
#
# Background
# ----------
# The Tauri UI embeds a WebKitGTK web view. If the app exits without letting
# WebKit tear its child processes down, `WebKitWebProcess` and
# `WebKitNetworkProcess` are orphaned (reparented to systemd) and keep running.
# On the NVIDIA proprietary driver those orphans then crash in the EGL teardown
# (`libnvidia-eglcore` -> `libnvidia-glsi`), dumping a ~50-90 MB core and
# raising DrKonqi on every exit.
#
# The leak is measurable: count the app's WebKit children shortly after it
# exits. A correct build leaves none behind.
#
# Note on closing: the app hides to the tray when its window is closed, so this
# script closes the window (verifying it stays alive) and then terminates the
# app explicitly to exercise the real exit path.
#
# Usage
# -----
#   scripts/check-webview-teardown.sh [path-to-clevo-cc-ui]
#
# Without an argument the release build is used; build it first with
#   cargo build --release --manifest-path ui/src-tauri/Cargo.toml
#
# Requires: a running graphical session (X11 or Wayland), xdotool, and
# Linux /proc. Coredump counting is best-effort (needs coredumpctl).

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${1:-$ROOT/ui/src-tauri/target/release/clevo-cc-ui}"
RUNS="${RUNS:-3}"
SETTLE="${SETTLE:-12}"

if [ ! -x "$BIN" ]; then
    echo "error: $BIN is not executable; build it first" >&2
    exit 2
fi

if ! command -v xdotool >/dev/null 2>&1; then
    echo "error: xdotool is required to close the window" >&2
    exit 2
fi

have_coredumpctl=1
command -v coredumpctl >/dev/null 2>&1 || have_coredumpctl=0

coredumps() {
    [ "$have_coredumpctl" = 1 ] || { echo 0; return; }
    coredumpctl list --no-pager 2>/dev/null | grep -c WebKitWebProces
}

echo "binary : $BIN"
echo "runs   : $RUNS"
echo

fail=0
for i in $(seq 1 "$RUNS"); do
    sandbox="$(mktemp -d "${TMPDIR:-/tmp}/clevo-teardown.XXXXXX")"
    mkdir -p "$sandbox/config/clevo-cc" "$sandbox/runtime"
    chmod 700 "$sandbox/runtime"
    echo '{"backend":"auto","software_rendering":false}' \
        > "$sandbox/config/clevo-cc/ui-launch.json"

    before="$(coredumps)"
    env XDG_CONFIG_HOME="$sandbox/config" \
        XDG_RUNTIME_DIR="$sandbox/runtime" \
        "$BIN" >"$sandbox/ui.log" 2>&1 &
    app=$!

    sleep "$SETTLE"

    # The app's own WebKit children, so we never count another app's.
    kids="$(pgrep -P "$app" 2>/dev/null | tr '\n' ' ')"
    webkit_kids=""
    for k in $kids; do
        case "$(cat "/proc/$k/comm" 2>/dev/null)" in
            WebKit*) webkit_kids="$webkit_kids $k" ;;
        esac
    done

    # Close the window the way a user does: the app must hide to the tray, not
    # quit. `windowclose` asks the window manager to close, which is what the
    # title bar's close button and the WM's own close both do.
    for wid in $(xdotool search --name "Clevo Control Center" 2>/dev/null); do
        xdotool windowclose "$wid" 2>/dev/null
    done
    sleep 2
    still_up=0
    kill -0 "$app" 2>/dev/null && still_up=1
    still_mapped=0
    if [ -n "$(xdotool search --name "Clevo Control Center" 2>/dev/null)" ]; then
        still_mapped=1
    fi
    if [ "$still_up" -ne 1 ]; then
        echo "  (warning: the app exited on window close; expected it to hide)" >&2
    fi
    if [ "$still_mapped" -eq 1 ]; then
        echo "  (warning: a window is still mapped after close)" >&2
    fi

    # Now quit for real, the way the tray's "退出" item does.
    kill -TERM "$app" 2>/dev/null
    for _ in $(seq 1 30); do
        kill -0 "$app" 2>/dev/null || break
        sleep 0.5
    done
    if kill -0 "$app" 2>/dev/null; then
        kill -KILL "$app" 2>/dev/null
    fi
    wait "$app" 2>/dev/null
    rc=$?

    # Give WebKit a moment, then see whether anything was left behind.
    sleep 1
    leaked=0
    for k in $webkit_kids; do
        kill -0 "$k" 2>/dev/null && leaked=$((leaked + 1))
    done

    sleep 6
    after="$(coredumps)"
    new_cores=$((after - before))

    status="ok"
    if [ "$leaked" -ne 0 ] || [ "$new_cores" -ne 0 ]; then
        status="FAIL"
        fail=1
    fi
    printf 'run %s: rc=%-4s webkit_children=%-2s leaked=%-2s coredumps=%-2s %s\n' \
        "$i" "$rc" "$(echo $webkit_kids | wc -w)" "$leaked" "$new_cores" "$status"

    # Clean up anything the app leaked, so later runs are unaffected.
    for k in $webkit_kids; do
        kill -KILL "$k" 2>/dev/null
    done
    rm -rf "$sandbox"
done

echo
if [ "$fail" -eq 0 ]; then
    echo "PASS: no WebKit child outlived the app and no coredump was produced."
    exit 0
fi
echo "FAIL: WebKit children leaked and/or coredumps were produced." >&2
echo "      The app must let WebKit tear its web/network processes down on exit;" >&2
echo "      see docs/hardware-notes.md (NVIDIA WebKitGTK exit crash)." >&2
exit 1
