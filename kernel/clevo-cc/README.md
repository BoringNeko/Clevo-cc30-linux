# clevo-cc kernel module

GPL-2.0-only. Out-of-tree ACPI platform driver that binds `ACPI\CLV0001` and
exposes fan monitoring and fan-mode control through `hwmon` and sysfs.

## What it does

- Probes the DCHU device and checks that `_DSM` exists.
- Builds the `_DSM` call with a **real ACPI Package** argument, which
  `acpi_call` cannot construct. The Arg3 shape depends on the command family:
  - reads (`PK*`, e.g. 12/13): `Package { Buffer(256) }`
  - writes (`SCMD`, e.g. 121): `Package { Integer((sub << 24) | value) }`
- Derives rpm from the command-12 rotation period with the Control Center
  formula `2156250 / period`.

## Interfaces

| Interface | Kind | Status |
|---|---|---|
| `hwmon fan1_input` / `fan2_input` | read | implemented (CPU, GPU1 rpm) |
| `sysfs fan_mode` | rw | `auto` / `quiet` / `max` / `maxq` |
| `sysfs fan_curve` | read | implemented read-only (command 13) |
| `sysfs perf_mode` | rw | `quiet` / `pwrsaving` / `performance` / `entertainment` |

`fan_mode` values map to `121/1`: `auto`=0, `max`=1, `maxq`=5, `quiet`=8.
`perf_mode` values map to `121/25`: quiet=0, pwrsaving=1, performance=2,
entertainment=3.

`fan_mode`/`perf_mode` report the last value written this session (fan_mode
defaults to `auto`, perf_mode to `unknown`); the firmware does not report the
current mode reliably.

## Reserved / not implemented

- **`silent` (fan_mode value 3)** — the DSDT branch is empty; would do nothing.
- **`custom` (value 6)** — needs a curve written first; paired with the
  graphics editor.
- **Custom curve write (command 14)** — the writable side of `fan_curve`.
  Deferred to the graphical editor so the four-point curve can be validated
  before being sent. See `docs/hardware-notes.md` §13.1 for the byte layout.
- **Fan offset `121/14`** — verified ineffective under thermal protection.
- **TurboFan (`121/25` bit 6) / DTT (bit 7)** — modifiers of the performance
  mode, not exposed yet.
- **Mode persistence** — the driver only sends the EC command; persistence is
  planned for `clevod`, not the kernel.

## Build

The CachyOS kernel is built with Clang + ThinLTO, so build with `LLVM=1`:

```bash
cd kernel/clevo-cc
make -C /usr/lib/modules/$(uname -r)/build M=$PWD LLVM=1
```

## Test (reversible)

```bash
sudo insmod clevo-cc.ko
dmesg | tail
cat /sys/devices/platform/CLV0001:00/fan_mode       # auto/quiet/max/maxq
cat /sys/devices/platform/CLV0001:00/fan_curve      # current curve (read-only)
cat /sys/class/hwmon/hwmon*/fan1_input              # CPU rpm
cat /sys/class/hwmon/hwmon*/fan2_input              # GPU1 rpm
echo max | sudo tee /sys/devices/platform/CLV0001:00/fan_mode
sudo rmmod clevo_cc
```

If `_DSM` returns `0x80000002` the probe still loads but reads return
`-EOPNOTSUPP`; check `dmesg`.

## Install via DKMS (later slice)

`dkms.conf` is provided; packaging is not wired up yet.
