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
| `hwmon temp1_input` / `temp2_input` | read | implemented (CPU, GPU1 °C; absent when the EC reports 0) |
| `sysfs fan_mode` | rw | `auto` / `quiet` / `max` / `maxq` / `custom` |
| `sysfs fan_curve` | rw | read (command 13) and write (command 14) |
| `sysfs perf_mode` | rw | `quiet` / `pwrsaving` / `performance` / `entertainment` |

`fan_mode` values map to `121/1`: `auto`=0, `max`=1, `maxq`=5, `custom`=6,
`quiet`=8. `perf_mode` values map to `121/25`: quiet=0, pwrsaving=1,
performance=2, entertainment=3.

`fan_mode`/`perf_mode` report the last value written this session (fan_mode
defaults to `auto`, perf_mode to `unknown`); the firmware does not report the
current mode reliably.

### Writing a curve

`fan_curve` reads as `fan_count=<n> kb_type=<k>` followed by one line per fan
with four `temp,duty` points (duty raw `0..255`). Writing accepts the same
per-fan lines:

```bash
echo "cpu: 0,0 55,102 75,178 0,0" | sudo tee fan_curve
echo custom | sudo tee fan_mode      # make the EC actually use it
```

- Only points 2 and 3 are sent (command 14), matching the Windows stack; the EC
  keeps its own first and last point.
- A fan line whose points are all zero is skipped, so a two-fan machine does
  not have to invent points for a fan it does not have.
- Write points in **raw duty** (`0..255`), the same unit the read side emits.
- Writing does not by itself select the custom curve: `echo custom > fan_mode`
  afterwards. Selecting custom without writing a curve makes the EC use
  whatever curve it already had.

## Reserved / not implemented

- **`silent` (fan_mode value 3)** — the DSDT branch is empty; would do nothing.
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
cat /sys/devices/platform/CLV0001:00/fan_mode       # auto/quiet/max/maxq/custom
cat /sys/devices/platform/CLV0001:00/fan_curve      # current curve
cat /sys/class/hwmon/hwmon*/fan1_input              # CPU rpm
cat /sys/class/hwmon/hwmon*/fan2_input              # GPU1 rpm
cat /sys/class/hwmon/hwmon*/temp1_input             # CPU temperature (m°C)
echo max | sudo tee /sys/devices/platform/CLV0001:00/fan_mode
sudo rmmod clevo_cc
```

If `_DSM` returns `0x80000002` the probe still loads but reads return
`-EOPNOTSUPP`; check `dmesg`.

## Install via DKMS (later slice)

`dkms.conf` is provided; packaging is not wired up yet.
