# clevo-cc-linux

**English** | [简体中文](README.md)

A Linux rewrite of the Clevo (蓝天) Control Center. Based on the DCHU / ACPI
`_DSM` protocol reverse-engineered from the vendor driver, it provides fan and
performance control, keyboard RGB, battery charge thresholds and desktop power integration.
This project is a re-reverse-engineered implementation of
[clevo-v250rnd-linux](https://github.com/BoringNeko/clevo-v250rnd-linux) with a
UI added.

> Verified on COLORFUL P15 23 / CachyOS / Fedora 44.
> Check your DSDT against [`docs/hardware-notes.md`](docs/hardware-notes.md) for
> other models.
> Other Clevo barebones are not guaranteed to work.

## UI Preview

![overview](https://r2.boring520.top/file/clevo/1791721295919_overview.png)

![fan](https://r2.boring520.top/file/clevo/1791721296723_fan.png)

![RGB](https://r2.boring520.top/file/clevo/1791721191886_RGB.png)

![power](https://r2.boring520.top/file/clevo/1791721217689_power.png)

![settings](https://r2.boring520.top/file/clevo/1791721188766_settings.png)

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
- **Capability discovery**: bypasses unavailable AppSettings EC pages 0..7 using
  GCMD `122/96/16/60/82` for `capabilities` and `profile list`. Performance-mode
  writes validate support first; unrecoverable fields remain unknown.
- **Daemon**: `clevod` is the only long-lived process that touches the hardware.
  It serves `org.clevo.CC` on the system D-Bus, caches readings and persists
  choices.
- **Survives a reboot**: after every successful write, `clevod` stores the fan
  mode, performance mode, custom curve, keyboard settings and charge thresholds
  in `/etc/clevo-cc/clevod.toml` and
  replays them on startup (the EC forgets on power loss; the curve is written
  before `custom` is selected). "Restore default" uses the machine's **real
  factory curve**, snapshotted from the EC on a cold boot before anything was
  written — not a hardcoded table — and a warm restart is never mistaken for a
  cold one.
- **Keyboard RGB**: hardware-selects either per-key ITE `048d:8910` USB HID
  control or the verified single-zone RGB15 ACPI-DCHU path. The local path also
  exposes the standard `clevo::kbd_backlight` LED class device for KDE and
  PowerDevil, with raw brightness `0..191` and an approximately 0.3-second
  software colour fade; `clevod` persists the user configuration.
- **Battery and desktop power**: real battery status and health, authorized
  standard charging thresholds and native FlexiCharger windows with an enable
  switch, firmware value validation, readback and persistence, and kernel
  `platform_profile` integration with the four performance modes. Unsupported
  hardware is reported explicitly. Native read/write of an enabled 40–80% window
  is verified on the local machine; a complete charging cycle and reboot replay
  still need hardware acceptance testing. See [`docs/power.md`](docs/power.md).
- **Desktop UI**: a React frontend that runs under either **Tauri 2** (default)
  or **Electron**, talking only over D-Bus; glass dashboard, custom title bar, an
  overview, fan, RGB and power pages, an editable fan curve, light/dark themes,
  custom accent colour/logo/wallpaper, a
  built-in colour picker, display settings (aspect ratio / resolution / zoom, all
  remembered across restarts) and compatibility options. Electron avoids the
  WebKitGTK crashes on the NVIDIA driver — see [`docs/electron.md`](docs/electron.md).
- **System tray**: a native menu that switches the performance mode (`▶` marks
  the active one), opens the control center and quits; closing the window returns
  to the tray and keeps the process running.
- **Login autostart**: a switch in Settings > Application starts the current
  user's UI in the tray at login, with no main window. Both shells support it;
  see [`docs/autostart.md`](docs/autostart.md).
- **Read-only diagnostics**: `clevo-cc --transport dbus doctor` checks hardware,
  services, permissions and configuration, including a full report on connection
  failures and structured `--json` output. See [`docs/doctor.md`](docs/doctor.md).
- **Safe by default**: writes are off by default; the `acpi_call` transport is
  read-only; unverified firmware constants are clearly marked.
- **Offline-testable**: protocol fixtures, isolated sysfs trees and private
  D-Bus sessions cover reads, writes and rejection paths without hardware.

> Fan control (speed, temperature, curve read/write, fan and performance modes)
> has been verified item by item on real hardware; the record is in
> [`docs/hardware-notes.md`](docs/hardware-notes.md), with the write-path
> details in §7.2 and §10.4.

## Architecture

```
UI (Tauri/Electron + React) ─┐
CLI (--transport dbus)      ─┴─▶ org.clevo.CC (D-Bus, PolicyKit) ─▶ clevod
                                                               ├─▶ kernel driver / acpi_call ─▶ firmware
                                                               └─▶ power_supply / platform_profile
```

- The UI and the `dbus` CLI backend use the daemon. The `driver` and `acpi-call`
  CLI backends also allow direct access and diagnostics; CLI threshold writes
  require D-Bus.
- D-Bus writes require PolicyKit authorization. Threshold writes check readback
  and attempt to restore the previous settings on failure.

## Tech stack

| Part | Technology |
|---|---|
| Protocol / transport / CLI / daemon | Rust (`zbus`, `tokio`, `serde`) |
| Kernel driver | C (ACPI platform driver, GPL-2.0-only, DKMS) |
| Desktop UI | React + TypeScript + Vite + MUI; Tauri 2 or Electron shell |
| Integration | D-Bus, PolicyKit, systemd, udev, DKMS |

## Install

```bash
sudo packaging/install.sh                 # driver (DKMS) + daemon + D-Bus/polkit/systemd/udev/man
sudo packaging/install.sh --enable        # and start clevod now
sudo packaging/install.sh --enable --ui   # start clevod and install the Tauri 2 UI
sudo packaging/install.sh --enable --electron  # ...or the Electron UI (recommended on NVIDIA)
sudo packaging/uninstall.sh               # remove software; does not restore previous charge settings
```

Preview first with `packaging/install.sh --dry-run`. Distribution packages (deb,
rpm, AppImage) are built with `packaging/build-packages.sh` (output in `dist/`);
an Arch/CachyOS `PKGBUILD` is also provided. See
[`docs/install.md`](docs/install.md) for the full guide (options, upgrade,
unprivileged sysfs access, troubleshooting).

## Power Quick Check

```bash
clevo-cc --transport dbus power status --json
# Use only if status advertises native BAT0 control with allowed 40/80 values.
clevo-cc --transport dbus power set-thresholds BAT0 --start 40 --end 80 --enabled true
# Add --apply after reviewing the dry run to set it through PolicyKit.
```

The power page places external power and desktop profiles on the left, with
battery readings and charge controls on the right. Thresholds do not actively
discharge a battery already above the stop percentage. A KDE plugged-in icon
does not prove charging current; see [`docs/power.md`](docs/power.md).

## Documentation

Run `clevo-cc --transport dbus doctor` for read-only hardware, service, permission
and configuration diagnostics. It prints a full report even when the daemon is
unavailable. Add `--json` for structured output or `--config PATH` to match a
custom daemon configuration. Exit status is 1 for failed checks, 0 otherwise;
optional unsupported features are skipped. The default mock backend stays offline.

| Document | Contents |
|---|---|
| [`docs/hardware-notes.md`](docs/hardware-notes.md) | Reverse-engineered protocol facts and verification |
| [`docs/install.md`](docs/install.md) | Install / upgrade / uninstall |
| [`docs/doctor.md`](docs/doctor.md) | Read-only diagnostics, JSON reports and exit status (Chinese) |
| [`docs/power.md`](docs/power.md) | Battery controls, desktop profiles and hardware acceptance (Chinese) |
| [`docs/support-matrix.md`](docs/support-matrix.md) | Support matrix and distribution notes |
| [`docs/desktop-ui.md`](docs/desktop-ui.md) | Desktop UI pages, settings and tray guide (Chinese) |
| [`docs/autostart.md`](docs/autostart.md) | Tray-only UI login autostart |
| [`docs/electron.md`](docs/electron.md) | Electron shell: architecture, build, install |

## Notes

Built with Opencode + Deepseek-V4.1-Flash.

## License

Split licensing: `kernel/` is **GPL-2.0-only**; everything else is
**MIT OR Apache-2.0**. See [`LICENSES/`](LICENSES/).

No vendor code is copied; the reverse-engineering reference under
`ControlCenter-RE/` is read-only.
