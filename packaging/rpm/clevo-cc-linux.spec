# SPDX-License-Identifier: MIT OR Apache-2.0
#
# RPM spec for clevo-cc-linux. Builds the daemon, CLI and (if present) the UI,
# and ships the kernel driver as DKMS sources that are compiled per-kernel on
# install.
#
#   rpmbuild -bb packaging/rpm/clevo-cc-linux.spec
# or via packaging/build-packages.sh rpm

%define _build_id_links none
# No debuginfo subpackage: it is noise for an end-user package and rpmbuild
# would otherwise emit one by default.
%global debug_package %{nil}

# Whether to package the UI. build-packages.sh sets this based on whether the
# binary was built; default on for a plain rpmbuild.
%{!?with_ui:%global with_ui 1}

Name:           clevo-cc-linux
Version:        %{version}
Release:        1%{?dist}
Summary:        Clevo control center (fan and performance mode)

License:        MIT AND Apache-2.0 AND GPL-2.0-only
URL:            https://github.com/BoringNeko/Clevo-cc30-linux
Source0:        %{name}-%{version}.tar.gz

# Rust/cargo are expected to be present on the build host (rpmbuild cannot
# resolve a toolchain dependency across distributions). Runtime deps below.
Requires:       dbus
Requires:       polkit
Requires:       dkms
Requires:       kmod

%description
Privileged daemon and CLI for reading fan speed and controlling the fan and
performance modes of Clevo DCHU ACPI laptops, using the reverse-engineered _DSM
protocol. Reads are open to local users; writes are authorized through
PolicyKit. The kernel driver is built per-kernel through DKMS on install; the
graphical UI is included when built.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked -p clevod -p clevo-cc-cli

%install
install -Dm755 target/release/clevod   %{buildroot}%{_bindir}/clevod
install -Dm755 target/release/clevo-cc %{buildroot}%{_bindir}/clevo-cc
install -Dm644 packaging/dbus/org.clevo.CC.conf     %{buildroot}%{_sysconfdir}/dbus-1/system.d/org.clevo.CC.conf
install -Dm644 packaging/polkit/org.clevo.CC.policy %{buildroot}%{_datadir}/polkit-1/actions/org.clevo.CC.policy
install -Dm644 packaging/systemd/clevod.service     %{buildroot}/usr/lib/systemd/system/clevod.service
install -Dm644 packaging/udev/99-clevo-cc.rules     %{buildroot}/usr/lib/udev/rules.d/99-clevo-cc.rules
install -Dm644 packaging/man/clevod.8   %{buildroot}%{_mandir}/man8/clevod.8
install -Dm644 packaging/man/clevo-cc.1 %{buildroot}%{_mandir}/man1/clevo-cc.1

# Kernel driver sources for DKMS.
install -d %{buildroot}/usr/src/clevo-cc-%{version}
cp -a kernel/clevo-cc/. %{buildroot}/usr/src/clevo-cc-%{version}/
rm -f %{buildroot}/usr/src/clevo-cc-%{version}/*.ko \
      %{buildroot}/usr/src/clevo-cc-%{version}/*.o \
      %{buildroot}/usr/src/clevo-cc-%{version}/*.mod* \
      %{buildroot}/usr/src/clevo-cc-%{version}/Module.symvers \
      %{buildroot}/usr/src/clevo-cc-%{version}/modules.order
sed -i 's/@VERSION@/%{version}/g' %{buildroot}/usr/src/clevo-cc-%{version}/dkms.conf

# UI, when a prebuilt binary is present (build-packages.sh passes --with ui).
if [ -x ui/src-tauri/target/release/clevo-cc-ui ]; then
  install -Dm755 ui/src-tauri/target/release/clevo-cc-ui %{buildroot}%{_bindir}/clevo-cc-ui
  install -Dm644 packaging/desktop/org.clevo.cc.ui.desktop \
      %{buildroot}%{_datadir}/applications/org.clevo.cc.ui.desktop
  for s in 32x32 128x128 128x128@2x; do
    case "$s" in 32x32) px=32;; 128x128) px=128;; 128x128@2x) px=256;; esac
    if [ -f ui/src-tauri/icons/$s.png ]; then
      install -Dm644 ui/src-tauri/icons/$s.png \
          %{buildroot}%{_datadir}/icons/hicolor/${px}x${px}/apps/org.clevo.cc.ui.png
    fi
  done
fi

%post
# Build and install the module for the running kernel, if DKMS is present.
dkms add -m clevo-cc -v %{version} >/dev/null 2>&1 || :
if [ -d "/lib/modules/$(uname -r)/build" ]; then
  dkms build -m clevo-cc -v %{version} >/dev/null 2>&1 || :
  dkms install -m clevo-cc -v %{version} >/dev/null 2>&1 || :
  modprobe clevo-cc >/dev/null 2>&1 || :
fi
systemctl daemon-reload >/dev/null 2>&1 || :
/usr/bin/udevadm control --reload-rules >/dev/null 2>&1 || :

%preun
if [ $1 -eq 0 ]; then systemctl disable --now clevod.service >/dev/null 2>&1 || :; fi

%postun
systemctl daemon-reload >/dev/null 2>&1 || :
/usr/bin/udevadm control --reload-rules >/dev/null 2>&1 || :

%triggerin -- kernel
dkms add -m clevo-cc -v %{version} >/dev/null 2>&1 || :
dkms build -m clevo-cc -v %{version} >/dev/null 2>&1 || :
dkms install -m clevo-cc -v %{version} >/dev/null 2>&1 || :

%files
%license LICENSES/MIT.txt LICENSES/Apache-2.0.txt LICENSES/GPL-2.0.txt
%{_bindir}/clevod
%{_bindir}/clevo-cc
%if %{with_ui}
%{_bindir}/clevo-cc-ui
%endif
%config(noreplace) %{_sysconfdir}/dbus-1/system.d/org.clevo.CC.conf
%{_datadir}/polkit-1/actions/org.clevo.CC.policy
/usr/lib/systemd/system/clevod.service
/usr/lib/udev/rules.d/99-clevo-cc.rules
%{_mandir}/man8/clevod.8*
%{_mandir}/man1/clevo-cc.1*
%if %{with_ui}
%{_datadir}/applications/org.clevo.cc.ui.desktop
%{_datadir}/icons/hicolor/*/apps/org.clevo.cc.ui.png
%endif
/usr/src/clevo-cc-%{version}

%changelog
* Sun Sep 14 2025 BoringNeko <noreply@github.com> - 0.1.0-1
- Initial package.
