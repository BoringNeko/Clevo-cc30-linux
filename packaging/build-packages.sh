#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Build distribution packages for clevo-cc-linux.
#
#   deb      clevo-cc-linux + clevo-cc-linux-dkms (+ -ui if built)
#   rpm      clevo-cc-linux (+ ui if built), %post drives dkms
#   appimage clevo-cc-ui only (the daemon/driver are system-level; an AppImage
#            cannot install a kernel module, a D-Bus service or PolicyKit rules)
#
# Debian packages are assembled directly with dpkg-deb, so no debhelper / dh is
# needed (it is not available outside Debian). packaging/debian/ is kept for
# people building inside a real Debian environment.
#
# Usage:
#   packaging/build-packages.sh [deb] [rpm] [appimage] [all]
#
# Requirements (by target):
#   deb       dpkg (dpkg-deb), cargo
#   rpm       rpm-tools (rpmbuild), cargo
#   appimage  appimagetool, pnpm + the Tauri build deps
#
# Output goes to dist/.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REPO_ROOT
readonly DIST="${REPO_ROOT}/dist"
PKGVER="$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_ROOT}/Cargo.toml" | head -n1)"
readonly PKGVER
readonly ARCH_DEB="amd64"

# Build scratch space lives next to the repo, not in /tmp (which is often a
# small tmpfs and can fill up with a full source-tree copy).
readonly WORK_ROOT="${REPO_ROOT}/dist/.work"
mkdir -p "$WORK_ROOT"
workdir() { mktemp -d "${WORK_ROOT}/XXXXXX"; }

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

build_rust() {
    log "building daemon + CLI (release)"
    ( cd "${REPO_ROOT}" && cargo build --release --locked -p clevod -p clevo-cc-cli )
}

ui_present() { [[ -x "${REPO_ROOT}/ui/src-tauri/target/release/clevo-cc-ui" ]]; }

# ---------------------------------------------------------------- deb --------
# Populate a package root with the shared files (daemon, CLI, integration).
deb_stage_main() {
    local root="$1"
    install -Dm755 "${REPO_ROOT}/target/release/clevod"   "$root/usr/bin/clevod"
    install -Dm755 "${REPO_ROOT}/target/release/clevo-cc" "$root/usr/bin/clevo-cc"
    install -Dm644 "${SCRIPT_DIR}/dbus/org.clevo.CC.conf"     "$root/usr/share/dbus-1/system.d/org.clevo.CC.conf"
    install -Dm644 "${SCRIPT_DIR}/polkit/org.clevo.CC.policy" "$root/usr/share/polkit-1/actions/org.clevo.CC.policy"
    install -Dm644 "${SCRIPT_DIR}/systemd/clevod.service"     "$root/lib/systemd/system/clevod.service"
    install -Dm644 "${SCRIPT_DIR}/udev/99-clevo-cc.rules"     "$root/lib/udev/rules.d/99-clevo-cc.rules"
    install -Dm644 "${SCRIPT_DIR}/man/clevod.8"   "$root/usr/share/man/man8/clevod.8"
    install -Dm644 "${SCRIPT_DIR}/man/clevo-cc.1" "$root/usr/share/man/man1/clevo-cc.1"
    install -Dm644 "${REPO_ROOT}/LICENSES/MIT.txt"          "$root/usr/share/doc/clevo-cc-linux/copyright.MIT.txt"
    install -Dm644 "${REPO_ROOT}/LICENSES/Apache-2.0.txt"   "$root/usr/share/doc/clevo-cc-linux/copyright.Apache-2.0.txt"
    install -Dm644 "${REPO_ROOT}/LICENSES/GPL-2.0.txt"      "$root/usr/share/doc/clevo-cc-linux/copyright.GPL-2.0.txt"
}

# Write a DEBIAN/control file.
deb_control() {
    local root="$1" name="$2" deps="$3" desc="$4"
    local size
    size="$(du -sk "$root" | cut -f1)"
    install -d "$root/DEBIAN"
    cat > "$root/DEBIAN/control" <<EOF
Package: ${name}
Version: ${PKGVER}-1
Architecture: ${ARCH_DEB}
Maintainer: BoringNeko <noreply@github.com>
Installed-Size: ${size}
Depends: ${deps}
Section: utils
Priority: optional
Homepage: https://github.com/BoringNeko/Clevo-cc30-linux
Description: ${desc}
EOF
}

build_deb() {
    command -v dpkg-deb >/dev/null || die "dpkg-deb not found (install dpkg)"
    build_rust

    log "building .deb"
    local work
    work="$(workdir)"
    mkdir -p "${DIST}"

    # --- clevo-cc-linux -------------------------------------------------------
    local main="${work}/main"
    deb_stage_main "$main"
    deb_control "$main" "clevo-cc-linux" \
        "dbus, polkitd | policykit-1, clevo-cc-linux-dkms" \
        "Clevo control center (fan and performance mode)"
    # Enable the daemon and reload udev on install/removal.
    cat > "$main/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
    /usr/bin/udevadm control --reload-rules >/dev/null 2>&1 || true
    /usr/bin/udevadm trigger --subsystem-match=platform >/dev/null 2>&1 || true
fi
exit 0
EOF
    cat > "$main/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
    systemctl disable --now clevod.service >/dev/null 2>&1 || true
fi
exit 0
EOF
    chmod 0755 "$main/DEBIAN/postinst" "$main/DEBIAN/prerm"
    dpkg-deb --build --root-owner-group "$main" \
        "${DIST}/clevo-cc-linux_${PKGVER}-1_${ARCH_DEB}.deb"

    # --- clevo-cc-linux-dkms --------------------------------------------------
    local dkms="${work}/dkms"
    install -d "$dkms/usr/src/clevo-cc-${PKGVER}"
    cp -a "${REPO_ROOT}/kernel/clevo-cc/." "$dkms/usr/src/clevo-cc-${PKGVER}/"
    rm -f "$dkms/usr/src/clevo-cc-${PKGVER}/"*.ko \
          "$dkms/usr/src/clevo-cc-${PKGVER}/"*.o \
          "$dkms/usr/src/clevo-cc-${PKGVER}/"*.mod* \
          "$dkms/usr/src/clevo-cc-${PKGVER}/Module.symvers" \
          "$dkms/usr/src/clevo-cc-${PKGVER}/modules.order"
    sed -i "s/@VERSION@/${PKGVER}/g" "$dkms/usr/src/clevo-cc-${PKGVER}/dkms.conf"
    deb_control "$dkms" "clevo-cc-linux-dkms" "dkms" \
        "clevo-cc ACPI platform driver (DKMS)"
    cat > "$dkms/DEBIAN/postinst" <<EOF
#!/bin/sh
set -e
if [ "\$1" = "configure" ] && command -v dkms >/dev/null 2>&1; then
    dkms add -m clevo-cc -v ${PKGVER} >/dev/null 2>&1 || true
    if [ -d "/lib/modules/\$(uname -r)/build" ]; then
        dkms build -m clevo-cc -v ${PKGVER} >/dev/null 2>&1 || true
        dkms install -m clevo-cc -v ${PKGVER} >/dev/null 2>&1 || true
        modprobe clevo-cc >/dev/null 2>&1 || true
    fi
fi
exit 0
EOF
    cat > "$dkms/DEBIAN/prerm" <<EOF
#!/bin/sh
set -e
if [ "\$1" = "remove" ] || [ "\$1" = "purge" ]; then
    modprobe -r clevo-cc >/dev/null 2>&1 || true
    command -v dkms >/dev/null 2>&1 && \
        dkms remove -m clevo-cc -v ${PKGVER} --all >/dev/null 2>&1 || true
fi
exit 0
EOF
    chmod 0755 "$dkms/DEBIAN/postinst" "$dkms/DEBIAN/prerm"
    dpkg-deb --build --root-owner-group "$dkms" \
        "${DIST}/clevo-cc-linux-dkms_${PKGVER}-1_${ARCH_DEB}.deb"

    # --- clevo-cc-linux-ui (only when built) ----------------------------------
    if ui_present; then
        local ui="${work}/ui"
        install -Dm755 "${REPO_ROOT}/ui/src-tauri/target/release/clevo-cc-ui" \
            "$ui/usr/bin/clevo-cc-ui"
        install -Dm644 "${SCRIPT_DIR}/desktop/org.clevo.cc.ui.desktop" \
            "$ui/usr/share/applications/org.clevo.cc.ui.desktop"
        for s in 32x32 128x128 128x128@2x; do
            case "$s" in 32x32) px=32;; 128x128) px=128;; 128x128@2x) px=256;; esac
            [[ -f "${REPO_ROOT}/ui/src-tauri/icons/${s}.png" ]] || continue
            install -Dm644 "${REPO_ROOT}/ui/src-tauri/icons/${s}.png" \
                "$ui/usr/share/icons/hicolor/${px}x${px}/apps/org.clevo.cc.ui.png"
        done
        deb_control "$ui" "clevo-cc-linux-ui" "clevo-cc-linux" \
            "Graphical control center for Clevo"
        dpkg-deb --build --root-owner-group "$ui" \
            "${DIST}/clevo-cc-linux-ui_${PKGVER}-1_${ARCH_DEB}.deb"
    else
        warn "UI not built; skipping clevo-cc-linux-ui.deb (build it with: cd ui && pnpm tauri build)"
    fi

    rm -rf "$work"
    log "wrote $(find "${DIST}" -maxdepth 1 -name '*.deb' | wc -l) .deb to dist/"
}

# ---------------------------------------------------------------- rpm --------
build_rpm() {
    command -v rpmbuild >/dev/null || die "rpmbuild not found (install rpm-tools)"
    build_rust

    log "building .rpm"
    local top
    top="$(workdir)"
    mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

    local stage="$top/stage/clevo-cc-linux-${PKGVER}"
    mkdir -p "$stage"
    # Prefer the tracked tree (small, no target/); fall back to a filtered copy.
    if ( cd "${REPO_ROOT}" && git rev-parse --is-inside-work-tree >/dev/null 2>&1 ); then
        ( cd "${REPO_ROOT}" && git archive --format=tar HEAD ) | tar -x -C "$stage"
    else
        ( cd "${REPO_ROOT}" && tar -cf - \
            --exclude=./target --exclude=./dist --exclude=./ui/node_modules \
            --exclude=./ui/src-tauri/target --exclude=./.git . ) | tar -x -C "$stage"
    fi
    # The UI binary and generated icons are git-ignored; copy them in so the
    # spec can package the UI when it exists.
    if ui_present; then
        install -Dm755 "${REPO_ROOT}/ui/src-tauri/target/release/clevo-cc-ui" \
            "$stage/ui/src-tauri/target/release/clevo-cc-ui"
        for s in 32x32 128x128 128x128@2x; do
            [[ -f "${REPO_ROOT}/ui/src-tauri/icons/${s}.png" ]] || continue
            install -Dm644 "${REPO_ROOT}/ui/src-tauri/icons/${s}.png" \
                "$stage/ui/src-tauri/icons/${s}.png"
        done
    fi

    tar -czf "$top/SOURCES/clevo-cc-linux-${PKGVER}.tar.gz" -C "$top/stage" "clevo-cc-linux-${PKGVER}"
    cp "${SCRIPT_DIR}/rpm/clevo-cc-linux.spec" "$top/SPECS/"

    local with_ui=0
    ui_present && with_ui=1
    # --nodeps skips BuildRequires resolution: this may run on a non-RPM host
    # (e.g. Ubuntu CI) where cargo/rust are installed via apt, not as rpm
    # packages. The toolchain is already present, which is what matters.
    rpmbuild --nodeps \
        --define "_topdir $top" --define "version ${PKGVER}" \
        --define "with_ui ${with_ui}" \
        -bb "$top/SPECS/clevo-cc-linux.spec"
    mkdir -p "${DIST}"
    find "$top/RPMS" -name '*.rpm' -exec cp {} "${DIST}/" \;
    rm -rf "$top"
    log "wrote $(find "${DIST}" -maxdepth 1 -name '*.rpm' | wc -l) .rpm to dist/"
}

# ----------------------------------------------------------- appimage --------
build_appimage() {
    local appdir="${DIST}/AppDir"
    log "building AppImage for the UI"
    if ui_present; then
        : # already built
    elif command -v pnpm >/dev/null; then
        ( cd "${REPO_ROOT}/ui" && pnpm install --frozen-lockfile && pnpm tauri build --no-bundle )
    else
        die "UI binary not built and pnpm missing; run 'cd ui && pnpm tauri build' first"
    fi

    # Prefer a system appimagetool, else a copy in packaging/appimage/ (any
    # name matching appimagetool*).
    local tool=""
    if command -v appimagetool >/dev/null 2>&1; then
        tool="$(command -v appimagetool)"
    else
        tool="$(find "${SCRIPT_DIR}/appimage" -maxdepth 1 -iname 'appimagetool*' -type f 2>/dev/null | head -n1)"
    fi
    [[ -n "$tool" && -x "$tool" ]] || die "appimagetool not found (put appimagetool-*.AppImage in packaging/appimage/)"

    rm -rf "$appdir"; mkdir -p "$appdir/usr/bin"
    install -Dm755 "${REPO_ROOT}/ui/src-tauri/target/release/clevo-cc-ui" "$appdir/usr/bin/clevo-cc-ui"
    install -Dm644 "${SCRIPT_DIR}/desktop/org.clevo.cc.ui.desktop" \
        "$appdir/usr/share/applications/org.clevo.cc.ui.desktop"
    install -Dm644 "${REPO_ROOT}/ui/src-tauri/icons/128x128.png" \
        "$appdir/usr/share/icons/hicolor/128x128/apps/org.clevo.cc.ui.png"
    ln -sf usr/share/applications/org.clevo.cc.ui.desktop "$appdir/org.clevo.cc.ui.desktop"
    cp "${REPO_ROOT}/ui/src-tauri/icons/icon.png" "$appdir/org.clevo.cc.ui.png"
    install -d "$appdir/usr/share/metainfo"
    # AppRun is what the AppImage runtime executes; keep the compatibility
    # options working by reading the same launch prefs the app itself reads.
    cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
# Locate this AppImage so bundled libs (none today) could be added later.
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="${HERE}/usr/bin:${PATH}"
exec "${HERE}/usr/bin/clevo-cc-ui" "$@"
APPRUN
    chmod 0755 "$appdir/AppRun"

    # appimagetool reads VERSION/ARCH from the environment.
    env VERSION="$PKGVER" ARCH=x86_64 "$tool" "$appdir" \
        "${DIST}/clevo-cc-ui-${PKGVER}-x86_64.AppImage"
    log "wrote ${DIST}/clevo-cc-ui-${PKGVER}-x86_64.AppImage"
}

main() {
    [[ $# -eq 0 ]] && set -- all
    for target in "$@"; do
        case "$target" in
            deb)      build_deb ;;
            rpm)      build_rpm ;;
            appimage) build_appimage ;;
            all)      build_deb; build_rpm; build_appimage ;;
            *)        die "unknown target: $target (deb|rpm|appimage|all)" ;;
        esac
    done
    log "done. artifacts in ${DIST}/"
}

main "$@"
