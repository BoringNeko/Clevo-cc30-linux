#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# clevo-cc-linux uninstaller. Reverses packaging/install.sh.
#
# Removes the DKMS module, daemon, UI, systemd unit, D-Bus policy, PolicyKit
# action and udev rule. The clevod config (/etc/clevo-cc) and the clevo-cc group
# are kept unless --purge is passed, because they may hold user choices.
#
# Usage:
#   sudo packaging/uninstall.sh [options]
#
# Options:
#   --prefix DIR   install prefix used at install time (default /usr)
#   --version VER  DKMS version to remove (default from Cargo.toml)
#   --purge        also remove /etc/clevo-cc and the clevo-cc group
#   --dry-run      print actions without changing anything
#   -h, --help     this help

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REPO_ROOT

PREFIX="/usr"
VERSION=""
PURGE=0
DRY_RUN=0

readonly DESTDIR="${DESTDIR:-}"
readonly GROUP="clevo-cc"
readonly CONFIG_DIR="/etc/clevo-cc"
readonly UNIT_DIR="${PREFIX}/lib/systemd/system"
readonly DBUS_DIR="${PREFIX}/share/dbus-1/system.d"
readonly POLKIT_DIR="${PREFIX}/share/polkit-1/actions"
readonly UDEV_DIR="${PREFIX}/lib/udev/rules.d"
readonly MAN_DIR="${PREFIX}/share/man"
readonly APPS_DIR="${PREFIX}/share/applications"
readonly ICONS_DIR="${PREFIX}/share/icons/hicolor"

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

run() {
    if (( DRY_RUN )); then printf '  [dry-run] %s\n' "$*"; else "$@"; fi
}

usage() { sed -n '2,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while (( $# )); do
    case "$1" in
        --prefix)  PREFIX="$2"; shift 2 ;;
        --version) VERSION="$2"; shift 2 ;;
        --purge)   PURGE=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *)         die "unknown option: $1 (try --help)" ;;
    esac
done

[[ $EUID -eq 0 || $DRY_RUN -eq 1 ]] || die "must run as root (or use --dry-run)"

if [[ -z "$VERSION" ]]; then
    VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_ROOT}/Cargo.toml" | head -n1)"
fi
[[ -n "$VERSION" ]] || die "could not determine version; pass --version"

log "removing clevo-cc-linux ${VERSION} from ${PREFIX}${DESTDIR:+ (DESTDIR=$DESTDIR)}"

# --- stop + disable the service ---------------------------------------------
if command -v systemctl >/dev/null 2>&1; then
    run systemctl disable --now clevod.service || true
fi

# --- systemd unit ------------------------------------------------------------
run rm -f "${DESTDIR}${UNIT_DIR}/clevod.service"
if (( DRY_RUN )); then
    printf '  [dry-run] systemctl daemon-reload\n'
elif command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
fi

# --- binaries ----------------------------------------------------------------
run rm -f "${DESTDIR}${PREFIX}/bin/clevod" "${DESTDIR}${PREFIX}/bin/clevo-cc" \
    "${DESTDIR}${PREFIX}/bin/clevo-cc-ui" "${DESTDIR}${PREFIX}/bin/clevo-cc-ui-electron"

# --- Electron app tree -------------------------------------------------------
run rm -rf "${DESTDIR}${PREFIX}/lib/clevo-cc-ui-electron"

# --- man pages ---------------------------------------------------------------
run rm -f "${DESTDIR}${MAN_DIR}/man8/clevod.8" "${DESTDIR}${MAN_DIR}/man1/clevo-cc.1"

# --- desktop entry + icons ---------------------------------------------------
run rm -f "${DESTDIR}${APPS_DIR}/org.clevo.cc.ui.desktop"
run rm -f "${DESTDIR}${APPS_DIR}/org.clevo.cc.ui.electron.desktop"
run rm -f "${DESTDIR}${ICONS_DIR}"/*/apps/org.clevo.cc.ui.png
run rm -f "${DESTDIR}${ICONS_DIR}"/*/apps/org.clevo.cc.ui.electron.png
if (( DRY_RUN )); then
    printf '  [dry-run] update-desktop-database + gtk-update-icon-cache\n'
else
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "${DESTDIR}${APPS_DIR}" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -qtf "${DESTDIR}${ICONS_DIR}" 2>/dev/null || true
    fi
fi

# --- bus + polkit ------------------------------------------------------------
run rm -f "${DESTDIR}${DBUS_DIR}/org.clevo.CC.conf"
run rm -f "${DESTDIR}${POLKIT_DIR}/org.clevo.CC.policy"

# --- udev rule ---------------------------------------------------------------
run rm -f "${DESTDIR}${UDEV_DIR}/99-clevo-cc.rules"
if (( DRY_RUN )); then
    printf '  [dry-run] udevadm control --reload-rules\n'
elif command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
fi

# --- kernel driver -----------------------------------------------------------
if command -v dkms >/dev/null 2>&1; then
    log "removing the DKMS module"
    run dkms remove -m clevo-cc -v "${VERSION}" --all || true
    run rm -rf "${DESTDIR}/usr/src/clevo-cc-${VERSION}"
    # Unload the module if it is still live (ignore if in use or absent).
    run modprobe -r clevo-cc || true
else
    warn "dkms not found; skipping module removal"
fi

# --- purge -------------------------------------------------------------------
if (( PURGE )); then
    log "purging config and group"
    run rm -rf "${DESTDIR}${CONFIG_DIR}"
    if (( DRY_RUN )); then
        printf '  [dry-run] groupdel %s\n' "$GROUP"
    else
        groupdel "$GROUP" 2>/dev/null || true
    fi
else
    printf '  kept %s (use --purge to remove) and the %s group\n' "$CONFIG_DIR" "$GROUP"
fi

echo
log "done."
