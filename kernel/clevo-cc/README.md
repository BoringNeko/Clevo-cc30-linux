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
| `hwmon temp1_input` | read | implemented (GPU1 °C; `-ENODATA` when the EC reports none) |
| `sysfs fan_mode` | rw | `auto` / `quiet` / `max` / `maxq` / `custom` |
| `sysfs fan_curve` | rw | read (command 13) and write (command 14) |
| `sysfs raw_status` / `raw_curve` | read | diagnostic hex dumps (for re-verifying offsets) |
| `sysfs perf_mode` | rw | `quiet` / `pwrsaving` / `performance` / `entertainment` |
| `sysfs keyboard_rgb` | rw | RGB15 single-zone color, mode and brightness (percent) |
| LED class `clevo::kbd_backlight` | rw | standard raw keyboard brightness (`0..191`) |

`fan_mode` values map to `121/1`: `auto`=0, `max`=1, `maxq`=5, `custom`=6,
`quiet`=8. `perf_mode` values map to `121/25`: quiet=0, pwrsaving=1,
performance=2, entertainment=3.

`fan_mode`/`perf_mode` report the last value written this session (fan_mode
defaults to `auto`, perf_mode to `unknown`); the firmware does not report the
current mode reliably.

`keyboard_rgb` accepts named operations and keeps the last values in its read
back:

```bash
echo "all ff0000" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "brightness 100" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "mode static" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
cat /sys/devices/platform/CLV0001:00/keyboard_rgb
```

Colors are `RRGGBB`, and `brightness` is a **percentage** (`0..100`). On the
verified COLORFUL P15 23 firmware, `kb_type=6` has one physical RGB15 channel,
so the whole keyboard changes together. The legacy `left`, `middle`, and
`right` spellings are accepted as aliases for `all`; they are not independent
zones. Colors, and the `brightness` percentage, are reported back by `cat`.

Color writes through `all RRGGBB` use a short software fade with 24 intermediate
steps; the transition takes about 0.3 seconds and remains a single physical
channel.

The LED class device is available at
`/sys/class/leds/clevo::kbd_backlight/brightness`. It exposes the raw
`0..191` brightness byte accepted by the RGB15 command, giving desktop
power-management tools 192 requested levels. The named `keyboard_rgb`
interface uses a friendlier `0..100` percentage over the same analog channel
(`100` = the EC maximum `191`). RGB color and effect selection remain on
`keyboard_rgb`.

For a direct hardware experiment only, stop `clevod` and use `probe 0..2` to
send the vendor's raw F0/F1/F2 selectors without updating the cached state:

```bash
echo "probe 0 ff0000" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "probe 1 00ff00" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "probe 2 0000ff" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
```

This is diagnostic-only. If the last command makes the entire keyboard blue,
the hardware is single-zone; if separate physical areas remain red, green, and
blue, the model has usable multi-zone selectors and the normal driver can be
extended accordingly.

The raw brightness byte can also be tested without changing the cached state:

```bash
echo "raw-brightness 192" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "raw-brightness 224" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "raw-brightness 255" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
```

Stop at the first abnormal result and restore `191` or use `mode off`.

### Temperatures

`temp1_input` is the **GPU** and is already degrees Celsius. The CPU
temperature is **not** exposed: its raw byte (offset `[18]`) has to go through
the vendor's `CalCPUTemp` piecewise curve, which is selected by the CPU's TDP
class. The kernel cannot learn that reliably, so `clevod` applies it in
userspace. Read `raw_status` (or command 12) and use
`clevo_proto::cal_cpu_temp` if you need it from a script.

### Writing a curve

`fan_curve` reads as `fan_count=<n> kb_type=<k>` followed by one line per fan
with four `temp,duty` points (duty raw `0..255`). Writing accepts the same
per-fan lines:

```bash
echo "cpu: 0,0 55,102 75,178 0,0" | sudo tee fan_curve
echo custom | sudo tee fan_mode      # make the EC actually use it
```

- Only points 2 and 3 are sent (command 14), matching the Windows stack; the EC
  keeps its own first and last point, and they keep their read-back values.
- **Command 14 replaces the whole table**, so the driver does a
  read-modify-write: it reads the current curve first and merges only the
  channels you name. Writing `cpu` alone leaves `gpu1` untouched.
- Write points in **raw duty** (`0..255`), the same unit the read side emits.
- **Duty unit matters here and only here.** Above this attribute duty is a
  percentage; the raw `0..255` form is the EC's. A converter that applies itself
  twice turns 100% into 255 and the write is rejected.
- The informational `fan_count=` / `kb_type=` line the read side emits may be
  echoed back: the parser recognises it before requiring the `:` that every fan
  line carries. It used to reject it, which made *every* write that went through
  a read-modify-write fail with `-EINVAL`.
- `temp` must strictly increase; a channel whose points cannot be encoded is
  rejected *unless* it already equals what the EC holds (so a corrupt table can
  still be repaired).
- Write one channel per sysfs call and check the read-back: a multi-line write
  is split by the shell into several calls, and the exit status then only
  reflects the last one.
- Writing does not by itself select the custom curve: `echo custom > fan_mode`
  afterwards. Selecting custom without writing a curve makes the EC use
  whatever curve it already had.

The write path is stress-tested on hardware by `scripts/curve-test.sh`.

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
cat /sys/class/hwmon/hwmon*/temp1_input             # GPU temperature (m°C)
cat /sys/devices/platform/CLV0001:00/raw_status     # raw cmd-12 bytes
echo max | sudo tee /sys/devices/platform/CLV0001:00/fan_mode
echo "all ff0000" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "mode wave" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
cat /sys/devices/platform/CLV0001:00/keyboard_rgb
echo "mode off" | sudo tee /sys/devices/platform/CLV0001:00/keyboard_rgb
sudo rmmod clevo_cc
```

The RGB15 status word enables the available keyboard channel together with the
color and brightness commands. `mode off` is the reversible cleanup operation;
it does not change the cached color or brightness level.

If `_DSM` returns `0x80000002` the probe still loads but reads return
`-EOPNOTSUPP`; check `dmesg`.

## Install via DKMS (later slice)

`dkms.conf` is provided; packaging is not wired up yet.
