#!/usr/bin/env bash
#
# setup-ui-env.sh - detect (and optionally help install) the Tauri 2 UI toolchain.
#
# Default behaviour is DETECT ONLY: it reports what is present and prints the
# install commands for whatever is missing, but never runs a package manager or
# sudo itself. Pass --install to let it install the JavaScript devDependencies
# in ui/ (the only step that does not need root).
#
# This matches the project rule that nothing changes the system by default.

set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI_DIR="$ROOT/ui"
DO_INSTALL=0
PKG_MANAGER=""

for arg in "$@"; do
    case "$arg" in
        --install) DO_INSTALL=1 ;;
        --package-manager=*) PKG_MANAGER="${arg#*=}" ;;
        -h|--help)
            cat <<'USAGE'
Usage: scripts/setup-ui-env.sh [--install] [--package-manager=npm|pnpm]

  --install                 Install the JavaScript devDependencies in ui/.
  --package-manager=NAME     npm or pnpm (default: pnpm if present, else npm).

Without --install every check is read-only and missing system packages are
reported with the install command for this distribution; nothing is installed.
USAGE
            exit 0
            ;;
        *)
            echo "unknown argument: $arg" >&2
            exit 2
            ;;
    esac
done

ok=0
missing=0

report() {
    # report <label> <command output or empty>
    local label="$1" value="$2"
    if [ -n "$value" ]; then
        printf '  \033[32mok\033[0m    %-32s %s\n' "$label" "$value"
        ok=$((ok + 1))
    else
        printf '  \033[31mMISS\033[0m  %-32s\n' "$label"
        missing=$((missing + 1))
    fi
}

have() { command -v "$1" >/dev/null 2>&1 && "$@" 2>/dev/null | head -n1; }

distro_id() {
    # shellcheck disable=SC1091
    . /etc/os-release 2>/dev/null || true
    echo "${ID:-unknown}"
}

echo "== Toolchain =="
report "rustc" "$(command -v rustc >/dev/null 2>&1 && rustc --version)"
report "cargo" "$(command -v cargo >/dev/null 2>&1 && cargo --version)"
report "node" "$(command -v node >/dev/null 2>&1 && node --version)"
report "npm" "$(command -v npm >/dev/null 2>&1 && npm --version)"
# pnpm is optional.
if command -v pnpm >/dev/null 2>&1; then
    printf '  \033[32mok\033[0m    %-32s %s\n' "pnpm (optional)" "$(pnpm --version)"
else
    printf '  \033[33mskip\033[0m  %-32s %s\n' "pnpm (optional)" "not installed"
fi

echo
echo "== Tauri 2 system libraries (pkg-config) =="
for pc in webkit2gtk-4.1 javascriptcoregtk-4.1 gtk+-3.0 libsoup-3.0 glib-2.0; do
    version="$(pkg-config --modversion "$pc" 2>/dev/null)"
    report "$pc" "$version"
done

echo
echo "== Optional: system tray =="
tray="$(pkg-config --modversion ayatana-appindicator3-0.1 2>/dev/null)"
if [ -n "$tray" ]; then
    printf '  \033[32mok\033[0m    %-32s %s\n' "ayatana-appindicator3-0.1" "$tray"
else
    printf '  \033[33mskip\033[0m  %-32s %s\n' "ayatana-appindicator3-0.1" "tray disabled"
fi

echo
echo "== Tauri CLI =="
if [ -f "$UI_DIR/node_modules/.bin/tauri" ]; then
    printf '  \033[32mok\033[0m    %-32s %s\n' "@tauri-apps/cli (local)" "present"
elif command -v cargo-tauri >/dev/null 2>&1; then
    printf '  \033[33minfo\033[0m  %-32s %s\n' "cargo-tauri (global)" "present"
else
    printf '  \033[31mMISS\033[0m  %-32s %s\n' "@tauri-apps/cli (devDependency)" "run this script with --install"
fi

echo
if [ "$missing" -eq 0 ]; then
    echo "All required dependencies are present."
else
    echo "$missing required dependency group(s) missing."
    case "$(distro_id)" in
        arch|cachyos|manjaro|endeavouros)
            echo "  Install (Arch family):"
            echo "    sudo pacman -S --needed webkit2gtk-4.1 gtk3 libsoup3 base-devel"
            echo "    # tray support (optional): sudo pacman -S libayatana-appindicator"
            ;;
        debian|ubuntu|linuxmint|pop)
            echo "  Install (Debian family):"
            echo "    sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev build-essential"
            echo "    # tray support (optional): sudo apt install libayatana-appindicator3-dev"
            ;;
        fedora|rhel|centos)
            echo "  Install (Fedora family):"
            echo "    sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel"
            echo "    # tray support (optional): sudo dnf install libayatana-appindicator-gtk3-devel"
            ;;
        *)
            echo "  See https://tauri.app/start/prerequisites/ for your distribution."
            ;;
    esac
fi

if [ "$DO_INSTALL" -eq 1 ]; then
    if [ ! -d "$UI_DIR" ]; then
        echo "ui/ directory does not exist; cannot install JavaScript dependencies." >&2
        exit 1
    fi
    if [ -z "$PKG_MANAGER" ]; then
        if command -v pnpm >/dev/null 2>&1; then
            PKG_MANAGER=pnpm
        else
            PKG_MANAGER=npm
        fi
    fi
    echo
    echo "Installing JavaScript devDependencies in ui/ with $PKG_MANAGER ..."
    (cd "$UI_DIR" && "$PKG_MANAGER" install)
fi
