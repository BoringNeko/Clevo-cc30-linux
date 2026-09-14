# clevo-cc-linux

**English** | [简体中文](README.md)

A Linux rewrite of the Clevo (蓝天) Control Center. Based on the DCHU / ACPI
`_DSM` protocol reverse-engineered from the vendor driver, it monitors fan speed
and controls the fan and performance modes.

> Phase 1 is complete and verified on real hardware (COLORFUL P15 23 / CachyOS).
> The protocol is verified only on Clevo-style laptops; check your DSDT against
> [`docs/hardware-notes.md`](docs/hardware-notes.md) for other models.

## Features

- **Monitoring**: fan speed (CPU / GPU1), raw temperature and duty, and the fan
  curve. Works through either the kernel driver or the read-only `acpi_call`
  backend.
- **Control**: fan mode (`auto` / `quiet` / `max` / `maxq`) and performance mode
  (0..3), through the kernel driver and PolicyKit, fully reversible.
- **Daemon**: `clevod` is the only long-lived process that touches the hardware.
  It serves `org.clevo.CC` on the system D-Bus, caches readings and persists
  choices.
- **Desktop UI**: a Tauri 2 app that talks only over D-Bus; glass dashboard,
  light/dark themes, custom accent colour/logo/wallpaper, compatibility options.
- **Safe by default**: writes are off by default; the `acpi_call` transport is
  read-only; unverified firmware constants are clearly marked.
- **Offline-testable**: no hardware required, everything is tested against
  hand-written fixtures (129 Rust + 45 frontend tests).

## Architecture

```
UI (Tauri/React)  ─┐
CLI               ─┼─▶ org.clevo.CC (D-Bus, PolicyKit) ─▶ clevod ─▶ kernel driver / acpi_call ─▶ EC
```

- Only `clevod` touches the hardware; the CLI and UI are D-Bus clients.
- Writes are a single point of entry, go through PolicyKit, and are reversible.

## Tech stack

| Part | Technology |
|---|---|
| Protocol / transport / CLI / daemon | Rust (`zbus`, `tokio`, `serde`) |
| Kernel driver | C (ACPI platform driver, GPL-2.0-only, DKMS) |
| Desktop UI | Tauri 2 + React + TypeScript + Vite + MUI |
| Integration | D-Bus, PolicyKit, systemd, udev, DKMS |

## Install

```bash
sudo packaging/install.sh            # driver (DKMS) + daemon + D-Bus/polkit/systemd/udev/man
sudo packaging/install.sh --enable   # and start clevod now
sudo packaging/uninstall.sh          # full rollback
```

Preview first with `packaging/install.sh --dry-run`. Distribution packages (deb,
rpm, AppImage) are built with `packaging/build-packages.sh` (output in `dist/`);
an Arch/CachyOS `PKGBUILD` is also provided. See
[`docs/install.md`](docs/install.md) for the full guide (options, upgrade,
unprivileged sysfs access, troubleshooting).

## Documentation

| Document | Contents |
|---|---|
| [`docs/hardware-notes.md`](docs/hardware-notes.md) | Reverse-engineered protocol facts and verification |
| [`docs/install.md`](docs/install.md) | Install / upgrade / uninstall |
| [`docs/support-matrix.md`](docs/support-matrix.md) | Support matrix and distribution notes |

## License

Split licensing: `kernel/` is **GPL-2.0-only**; everything else is
**MIT OR Apache-2.0**. See [`LICENSES/`](LICENSES/).

No vendor code is copied; the reverse-engineering reference under
`ControlCenter-RE/` is read-only.
