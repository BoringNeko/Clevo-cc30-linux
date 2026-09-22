# clevo-cc-linux

**English** | [简体中文](README.md)

A Linux rewrite of the Clevo (蓝天) Control Center. Based on the DCHU / ACPI
`_DSM` protocol reverse-engineered from the vendor driver, it monitors fan speed
and controls the fan and performance modes.
This project is a re-reverse-engineered implementation of
[clevo-v250rnd-linux](https://github.com/BoringNeko/clevo-v250rnd-linux) with a
UI added.

> Verified on COLORFUL P15 23 / CachyOS / Fedora 44.
> Check your DSDT against [`docs/hardware-notes.md`](docs/hardware-notes.md) for
> other models.
> Other Clevo barebones are not guaranteed to work.

## Features

- **Monitoring**: fan speed (CPU / GPU1), CPU/GPU temperature in Celsius, and the
  fan curve.
- **Control**: fan mode (`auto` / `quiet` / `max` / `maxq` / `customize` — the UI
  label for the firmware's `custom`), a **custom four-point fan curve**
  (command 14, which selects that mode when written; the curve card is only
  shown while it is active, with both lines draggable and save / restore /
  factory-default buttons) and performance mode (`quiet` / `pwrsaving` /
  `performance` / `entertainment`), through the kernel driver and PolicyKit,
  fully reversible.
- **Daemon**: `clevod` is the only long-lived process that touches the hardware.
  It serves `org.clevo.CC` on the system D-Bus, caches readings and persists
  choices.
- **Desktop UI**: a Tauri 2 app that talks only over D-Bus; glass dashboard,
  custom title bar, an editable fan curve, light/dark themes, custom accent
  colour/logo/wallpaper, a built-in colour picker, display settings (aspect
  ratio / resolution / zoom, all remembered across restarts) and compatibility
  options.
- **System tray**: a native menu that switches the performance mode (`▶` marks
  the active one), opens the control center and quits; closing the window hides
  it to the tray and keeps the process running.
- **Safe by default**: writes are off by default; the `acpi_call` transport is
  read-only; unverified firmware constants are clearly marked.
- **Offline-testable**: no hardware required, everything is tested against
  hand-written fixtures (166 Rust + 28 UI-backend + 178 frontend tests).

> Fan control (speed, temperature, curve read/write, fan and performance modes)
> has been verified item by item on real hardware; the record is in
> [`docs/hardware-notes.md`](docs/hardware-notes.md), with the write-path
> details in §7.2 and §10.4.

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
sudo packaging/install.sh                 # driver (DKMS) + daemon + D-Bus/polkit/systemd/udev/man
sudo packaging/install.sh --enable        # and start clevod now
sudo packaging/install.sh --enable --ui   # start clevod and install the UI too
sudo packaging/uninstall.sh               # full rollback
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

## Notes

Built with Opencode + Deepseek-V4.1-Flash.

## License

Split licensing: `kernel/` is **GPL-2.0-only**; everything else is
**MIT OR Apache-2.0**. See [`LICENSES/`](LICENSES/).

No vendor code is copied; the reverse-engineering reference under
`ControlCenter-RE/` is read-only.
