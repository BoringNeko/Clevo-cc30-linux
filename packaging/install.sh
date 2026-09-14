#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# clevo-cc-linux installer.
#
# Installs, in order:
#   1. the kernel driver via DKMS (unless --no-driver),
#   2. the clevod daemon and the D-Bus policy + PolicyKit action,
#   3. the systemd unit,
#   4. the udev rule and the clevo-cc group (unless --no-udev),
#   5. optionally the desktop UI (--ui; needs a prebuilt binary).
#
# Every step is reversible with packaging/uninstall.sh. Nothing is enabled or
# started unless --enable is passed, and nothing is written to hardware.
#
# Usage:
#   sudo packaging/install.sh [options]
#
# Options:
#   --prefix DIR     install prefix (default /usr)
#   --version VER    version string for DKMS (default from Cargo.toml)
#   --bin-dir DIR    install prebuilt clevod/clevo-cc from DIR instead of building
#   --no-driver      skip the kernel driver
#   --no-udev        skip the udev rule and group
#   --ui             install the desktop UI (build it first if missing)
#   --no-ui-build    with --ui, install an existing UI build without building
#   --enable         enable + start clevod after installing
#   --dry-run        print actions without changing anything
#   -h, --help       this help

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REPO_ROOT

PREFIX="/usr"
VERSION=""
BIN_DIR=""
WITH_DRIVER=1
WITH_UDEV=1
WITH_UI=0
UI_BUILD=1
ENABLE=0
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

# Locate the Rust toolchain.
#
# Portable rules, no machine-specific paths:
#   1. respect CARGO_HOME / RUSTUP_HOME if the caller set them;
#   2. else, when run through sudo, use the invoking account (SUDO_USER) so the
#      build reuses that user's toolchain and target cache;
#   3. else use the current user's defaults.
# If none has a toolchain, the build step fails with instructions (or use
# --bin-dir to install prebuilt binaries).
if [[ -n "${CARGO_HOME:-}" ]]; then
    CARGO_HOME_DIR="$CARGO_HOME"
elif [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
    CARGO_HOME_DIR="$(getent passwd "$SUDO_USER" | cut -d: -f6)/.cargo"
else
    CARGO_HOME_DIR="${HOME}/.cargo"
fi

if [[ -n "${RUSTUP_HOME:-}" ]]; then
    RUSTUP_HOME_DIR="$RUSTUP_HOME"
elif [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
    RUSTUP_HOME_DIR="$(getent passwd "$SUDO_USER" | cut -d: -f6)/.rustup"
else
    RUSTUP_HOME_DIR="${HOME}/.rustup"
fi

find_cargo() {
    if command -v cargo >/dev/null 2>&1; then
        command -v cargo
        return 0
    fi
    if [[ -x "${CARGO_HOME_DIR}/bin/cargo" ]]; then
        printf '%s\n' "${CARGO_HOME_DIR}/bin/cargo"
        return 0
    fi
    return 1
}

# Put the detected toolchain dir on PATH and export the standard Rust homes so
# cargo's own subprocesses (rustc, linkers, the rustup shim) resolve too.
if [[ -z "$(command -v cargo 2>/dev/null || true)" && -d "${CARGO_HOME_DIR}/bin" ]]; then
    PATH="${CARGO_HOME_DIR}/bin:${PATH}"
    export PATH
fi
[[ -d "$RUSTUP_HOME_DIR" ]] && export RUSTUP_HOME="$RUSTUP_HOME_DIR"
[[ -d "$CARGO_HOME_DIR" ]] && export CARGO_HOME="$CARGO_HOME_DIR"

run() {
    if (( DRY_RUN )); then
        printf '  [dry-run] %s\n' "$*"
    else
        "$@"
    fi
}

write_file() {
    # write_file <dest> ; content on stdin
    local dest="$1"
    if (( DRY_RUN )); then
        printf '  [dry-run] install file %s\n' "$dest"
        cat >/dev/null
        return
    fi
    local tmp
    tmp="$(mktemp)"
    cat >"$tmp"
    install -Dm644 "$tmp" "$dest"
    rm -f "$tmp"
}

usage() { sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while (( $# )); do
    case "$1" in
        --prefix)     PREFIX="$2"; shift 2 ;;
        --version)    VERSION="$2"; shift 2 ;;
        --bin-dir)    BIN_DIR="$2"; shift 2 ;;
        --no-driver)  WITH_DRIVER=0; shift ;;
        --no-udev)    WITH_UDEV=0; shift ;;
        --ui)         WITH_UI=1; shift ;;
        --no-ui-build) UI_BUILD=0; shift ;;
        --enable)     ENABLE=1; shift ;;
        --dry-run)    DRY_RUN=1; shift ;;
        -h|--help)    usage; exit 0 ;;
        *)            die "unknown option: $1 (try --help)" ;;
    esac
done

[[ $EUID -eq 0 || $DRY_RUN -eq 1 ]] || die "must run as root (or use --dry-run)"

# Resolve the version from the workspace Cargo.toml when not given.
if [[ -z "$VERSION" ]]; then
    VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_ROOT}/Cargo.toml" | head -n1)"
fi
[[ -n "$VERSION" ]] || die "could not determine version; pass --version"

log "clevo-cc-linux ${VERSION} -> prefix ${PREFIX}${DESTDIR:+ (DESTDIR=$DESTDIR)}"

# --- 1. kernel driver (DKMS) -------------------------------------------------
if (( WITH_DRIVER )); then
    if ! command -v dkms >/dev/null 2>&1; then
        warn "dkms not found; install it or re-run with --no-driver"
    else
        log "installing the kernel driver via DKMS"
        src="/usr/src/clevo-cc-${VERSION}"
        run install -d "${DESTDIR}${src}"
        if (( DRY_RUN )); then
            printf '  [dry-run] copy kernel/clevo-cc -> %s and substitute @VERSION@\n' "${src}"
        else
            cp -a "${REPO_ROOT}/kernel/clevo-cc/." "${src}/"
            rm -f "${src}/clevo-cc.ko" "${src}/"*.o "${src}/"*.mod* \
                  "${src}/Module.symvers" "${src}/modules.order"
            sed -i "s/@VERSION@/${VERSION}/g" "${src}/dkms.conf"
            dkms remove -m clevo-cc -v "${VERSION}" --all 2>/dev/null || true
            dkms add -m clevo-cc -v "${VERSION}"
            dkms build -m clevo-cc -v "${VERSION}"
            dkms install -m clevo-cc -v "${VERSION}"
            # Load it now if the firmware device is present (best effort).
            modprobe clevo-cc 2>/dev/null || \
                warn "clevo-cc built but not loaded (device absent or in use); it will load on reboot"
        fi
    fi
fi

# --- 2. daemon + CLI ---------------------------------------------------------
if [[ -n "$BIN_DIR" ]]; then
    log "using prebuilt binaries from ${BIN_DIR}"
    daemon_bin="${BIN_DIR}/clevod"
    cli_bin="${BIN_DIR}/clevo-cc"
    if (( ! DRY_RUN )); then
        [[ -f "$daemon_bin" && -f "$cli_bin" ]] || die "no clevod/clevo-cc in --bin-dir ${BIN_DIR}"
    fi
else
    log "building the daemon and CLI"
    if (( DRY_RUN )); then
        printf '  [dry-run] cargo build --release -p clevod -p clevo-cc-cli\n'
    else
        cargo_bin="$(find_cargo)" || die \
            "cargo not found. Install Rust, build first (cargo build --release -p clevod -p clevo-cc-cli), or pass --bin-dir DIR"
        build_cmd=("$cargo_bin" build --release --manifest-path "${REPO_ROOT}/Cargo.toml"
                   -p clevod -p clevo-cc-cli)
        # Under sudo, build as the invoking account so the toolchain, target
        # cache and file ownership are the user's, then install as root.
        if [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
            sudo -u "$SUDO_USER" \
                env CARGO_HOME="$CARGO_HOME_DIR" RUSTUP_HOME="$RUSTUP_HOME_DIR" \
                    PATH="${CARGO_HOME_DIR}/bin:${PATH}" \
                "${build_cmd[@]}" \
                || die "build failed"
        else
            "${build_cmd[@]}" || die "build failed"
        fi
    fi
    daemon_bin="${REPO_ROOT}/target/release/clevod"
    cli_bin="${REPO_ROOT}/target/release/clevo-cc"
    if (( ! DRY_RUN )); then
        [[ -x "$daemon_bin" ]] || die "clevod was not built"
        [[ -x "$cli_bin" ]] || die "clevo-cc was not built"
    fi
fi
log "installing ${PREFIX}/bin/clevod and ${PREFIX}/bin/clevo-cc"
run install -Dm755 "$daemon_bin" "${DESTDIR}${PREFIX}/bin/clevod"
run install -Dm755 "$cli_bin" "${DESTDIR}${PREFIX}/bin/clevo-cc"

# --- 3. D-Bus policy + PolicyKit action -------------------------------------
log "installing D-Bus policy and PolicyKit actions"
write_file "${DESTDIR}${DBUS_DIR}/org.clevo.CC.conf"   < "${SCRIPT_DIR}/dbus/org.clevo.CC.conf"
write_file "${DESTDIR}${POLKIT_DIR}/org.clevo.CC.policy" < "${SCRIPT_DIR}/polkit/org.clevo.CC.policy"

# --- 4. config directory -----------------------------------------------------
log "preparing ${CONFIG_DIR}"
run install -d "${DESTDIR}${CONFIG_DIR}"
if [[ ! -e "${DESTDIR}${CONFIG_DIR}/clevod.toml" ]]; then
    if (( DRY_RUN )); then
        printf '  [dry-run] create default %s/clevod.toml\n' "${CONFIG_DIR}"
    else
        # The daemon rewrites this with schema metadata on first save.
        printf '# clevo-cc daemon configuration. Managed by clevod.\n' \
            > "${DESTDIR}${CONFIG_DIR}/clevod.toml"
    fi
fi

# --- 5. systemd unit ---------------------------------------------------------
log "installing the systemd unit"
write_file "${DESTDIR}${UNIT_DIR}/clevod.service" < "${SCRIPT_DIR}/systemd/clevod.service"
if (( DRY_RUN )); then
    printf '  [dry-run] systemctl daemon-reload\n'
elif command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
fi

# --- 5.5 man pages -----------------------------------------------------------
log "installing man pages"
if (( DRY_RUN )); then
    printf '  [dry-run] install man pages into %s\n' "${MAN_DIR}"
else
    install -Dm644 "${SCRIPT_DIR}/man/clevod.8"   "${DESTDIR}${MAN_DIR}/man8/clevod.8"
    install -Dm644 "${SCRIPT_DIR}/man/clevo-cc.1" "${DESTDIR}${MAN_DIR}/man1/clevo-cc.1"
    if command -v mandb >/dev/null 2>&1; then
        mandb -q "${DESTDIR}${MAN_DIR}" 2>/dev/null || true
    fi
fi

# --- 6. udev rule + group ----------------------------------------------------
if (( WITH_UDEV )); then
    log "installing the udev rule and the ${GROUP} group"
    if (( DRY_RUN )); then
        printf '  [dry-run] groupadd -r %s (if missing)\n' "$GROUP"
        printf '  [dry-run] install udev rule\n'
    else
        getent group "$GROUP" >/dev/null || groupadd -r "$GROUP"
        install -Dm644 "${SCRIPT_DIR}/udev/99-clevo-cc.rules" \
            "${DESTDIR}${UDEV_DIR}/99-clevo-cc.rules"
        if command -v udevadm >/dev/null 2>&1; then
            udevadm control --reload-rules || true
            udevadm trigger --subsystem-match=platform || true
        fi
        printf '  to allow direct sysfs access, add your user to the group:\n'
        # shellcheck disable=SC2016  # $USER is shown literally for the user to run
        printf '    sudo usermod -aG %s "$USER"   (then log in again)\n' "$GROUP"
    fi
fi

# --- 7. optional UI ----------------------------------------------------------
if (( WITH_UI )); then
    ui_bin="${REPO_ROOT}/ui/src-tauri/target/release/clevo-cc-ui"
    if [[ ! -x "$ui_bin" && "$UI_BUILD" == "1" ]]; then
        log "building the desktop UI (pnpm tauri build)"
        if (( DRY_RUN )); then
            printf '  [dry-run] pnpm install && pnpm tauri build (in ui/)\n'
        else
            pnpm_bin="$(command -v pnpm || true)"
            [[ -n "$pnpm_bin" ]] || die "pnpm not found; install it or build the UI yourself (cd ui && pnpm tauri build)"
            # Build as the invoking account under sudo, like the Rust build.
            if [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
                sudo -u "$SUDO_USER" \
                    env CARGO_HOME="${CARGO_HOME_DIR:-}" RUSTUP_HOME="${RUSTUP_HOME_DIR:-}" \
                        PATH="${CARGO_HOME_DIR}/bin:${PATH}" \
                    sh -c "cd '${REPO_ROOT}/ui' && '${pnpm_bin}' install --frozen-lockfile && '${pnpm_bin}' tauri build --no-bundle" \
                    || die "UI build failed"
            else
                ( cd "${REPO_ROOT}/ui" \
                    && "$pnpm_bin" install --frozen-lockfile \
                    && "$pnpm_bin" tauri build --no-bundle ) \
                    || die "UI build failed"
            fi
        fi
    fi
    if [[ ! -x "$ui_bin" && "$DRY_RUN" -eq 0 ]]; then
        warn "UI binary not found at $ui_bin; build it with: cd ui && pnpm tauri build"
    else
        log "installing the desktop UI"
        run install -Dm755 "$ui_bin" "${DESTDIR}${PREFIX}/bin/clevo-cc-ui"
        run install -Dm644 "${SCRIPT_DIR}/desktop/org.clevo.cc.ui.desktop" \
            "${DESTDIR}${APPS_DIR}/org.clevo.cc.ui.desktop"
        # Install the app icon at the sizes Tauri ships.
        for size in 32x32 128x128 128x128@2x; do
            src_icon="${REPO_ROOT}/ui/src-tauri/icons/${size}.png"
            [[ -f "$src_icon" ]] || continue
            case "$size" in
                32x32)     px=32 ;;
                128x128)   px=128 ;;
                128x128@2x) px=256 ;;
            esac
            run install -Dm644 "$src_icon" \
                "${DESTDIR}${ICONS_DIR}/${px}x${px}/apps/org.clevo.cc.ui.png"
        done
        if (( ! DRY_RUN )) && command -v update-desktop-database >/dev/null 2>&1; then
            update-desktop-database "${DESTDIR}${APPS_DIR}" 2>/dev/null || true
        fi
        if (( ! DRY_RUN )) && command -v gtk-update-icon-cache >/dev/null 2>&1; then
            gtk-update-icon-cache -qtf "${DESTDIR}${ICONS_DIR}" 2>/dev/null || true
        fi
    fi
fi

# --- 8. enable ---------------------------------------------------------------
if (( ENABLE )); then
    if (( DRY_RUN )); then
        printf '  [dry-run] systemctl enable --now clevod.service\n'
    elif command -v systemctl >/dev/null 2>&1; then
        systemctl enable --now clevod.service
    fi
fi

echo
log "done."
cat <<EOF

Next steps:
  - start now:            sudo systemctl enable --now clevod
  - check:                systemctl status clevod
  - CLI through the bus:  clevo-cc --transport dbus fan status
  - graphical UI:         run the clevo-cc-ui binary
  - uninstall everything: sudo packaging/uninstall.sh
EOF
