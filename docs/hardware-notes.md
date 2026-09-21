# Hardware notes — ACPI verification (S5)

Machine: **COLORFUL P15 23** (DMI `sys_vendor=COLORFUL`, `product_name=P15 23`,
`board_name=P15 23`), CachyOS, kernel-provided ACPI tables.

Method: read-only `acpidump -b` + `iasl -d`, then manual inspection of
`dsdt.dsl`. No `acpi_call`, no writes. Raw artifacts live in `acpi-verify/`
(git-ignored; regenerate with `scripts/verify-acpi.sh`).

## 1. Device confirmed present

`dsdt.dsl:101997` defines the DCHU device:

```asl
Device (DCHU)
{
    Name (_HID, "CLV0001")
    Name (_UID, One)
    Method (_DSM, 4, Serialized) { ... }
}
```

`CLV0002` also exists (`dsdt.dsl:107501`).

## 2. `_DSM` GUID confirmed

`dsdt.dsl:102043`:

```asl
If ((Arg0 == ToUUID ("93f224e4-fbdc-4bbf-add6-db71bdc0afad")))
```

This is exactly `ACPIBIOS_READ` from the reference. The two other GUIDs appear
as raw buffers in the same method:

| Bytes at `_DSM` | GUID | Use in DSDT |
|---|---|---|
| `34 79 6D 0B B5 1E 3E 45 58 25 D9 45 07 2A 45 AA` | `{0B6D7934-…}` (WRITE) | acquires EC mutex, returns `0x80000002` (stub) |
| `27 EF 76 60 0E 16 86 46 35 E5 92 4A 50 B4 DE AA` | `{6076EF27-…}` (EVENT) | returns `0x80000002` (stub) |

## 3. `_DSM` signature and argument layout (verified)

```asl
Method (_DSM, 4, Serialized)
{
    If ((Arg0 == ToUUID ("93f224e4-...")))
    {
        Local1 = Acquire (EC.PATM, 0x64);
        If ((Local1 == Zero)) {
            Switch (Arg2) { ... }
        }
    }
}
```

- `Arg0` = UUID (Buffer 16).
- `Arg1` = Revision (passed through to helpers).
- `Arg2` = **FunctionIndex = command number** (dispatch key).
- `Arg3` = **Package**; element `[0]` is the 256-byte payload Buffer.
  Confirmed by `BUFF = DerefOf (Arg2 [Zero])` inside `PK0E`/`PK04`.
- Return value is the 256-byte `BUFF` (a Buffer), or an integer status.

> **Design correction**: the Windows bridge sent a raw 256-byte buffer; the ACPI
> method expects a *Package containing that buffer* as `Arg3`. The `acpi_call`
> transport must pass `Arg3` as `{ buffer }`.

## 4. Command dispatch map (verified)

`_DSM` `Arg2` → helper (nested inside `Device(DCHU)`):

| Command (dec) | Command (hex) | Helper | Meaning |
|---|---|---|---|
| 4 | `0x04` | `PK04` | EC/GPU/panel misc (per sub-command `BUFF[0]`) |
| 7 | `0x07` | `PK07` | battery package |
| **12** | `0x0C` | `PK0C` | **fan status** |
| **13** | `0x0D` | `PK0D` | **fan curve read** |
| **14** | `0x0E` | `PK0E` | **fan curve write** |
| 17 | `0x11` | `PK11` | OEM/custom id |
| 1,5,6,8,9,10,16,18,50,51,52,56,57,59,60,61,62,63,65,66,67,69,70,71,73,75,77,78,79,80,81,82,83,84,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,105,114,115,116,117,118,119,120,121,122,123,125,126,127 | | `GCMD` | general command family |
| 19,20,29,31,32,33,34,35,38,39,42,44,46,47,48,49,50,52,55,86,87,88,90,91,94,95,99,100,101,102,103,104,107,108,109,110,115,118,120,121,122,123,124,125,126,127 | | `SCMD` | secondary command family |

The fan commands and command `121`/`0x79` are all reachable.

## 5. Command 12 — fan status (`PK0C`), verified offsets

Built as a 0x100 buffer; EC fields copied in:

| Buffer offset | Word/Byte | EC field | Meaning |
|---|---|---|---|
| `0x00` | W000 | — | zero |
| `0x02` | W001 | `RPM1` | CPU fan RPM |
| `0x04` | W002 | `RPM2` | GPU1 fan RPM |
| `0x06` | W003 | `RPM3` | GPU2 fan RPM |
| `0x08` | W004 | `BPV0` | (battery/board value) |
| `0x0A` | W005 | EC `0xB1 0x19` block | dword |
| `0x0E` | W006 | `BPR0` | (board) |
| `0x10` | W007 | EC `0xC0 0x03` byte | CPU temp |
| `0x11` | W008 | EC `0xC0 0x02` byte | GPU1 temp |
| `0x12` | W009 | EC `0xC0 0x02` byte | (same source) |
| `0x13` | W010 | EC `0xC0 0x00` byte | GPU2 temp |
| `0x14` | W011 | EC `0xC0 0x00` byte | GPU2 temp |
| `0x15` | W012 | EC `0xC0 0x00` byte | GPU2 temp |
| `0x16` | W013 | EC `0xC0 0x01` byte | (fan/thermal) |
| `0x17` | W014 | EC `0xC0 0x01` byte | (fan/thermal) |
| `0x18` | W015 | EC `0xC0 0x01` byte | (fan/thermal) |

Note: `RPM1/2/3` are 16-bit EC fields; the buffer stores them **little-endian**.
The reference doc said "big-endian: a[3] + a[2]<<8"; on this machine the value is
placed as a native `WordField` (LE). This must be reconciled in `fan_status.rs`
before enabling real reads.

## 6. Command 13 — fan curve read (`PK0D`), verified offsets

| Buffer offset | EC field |
|---|---|
| `0x0B` | `KBBH` (keyboard backlight brightness?) |
| `0x0C` | `FANQ` |
| `0x0F` | `KBTP` (keyboard type) |
| `0x10` | `F1T1` |
| `0x11` | `F1D1` |
| `0x12` | `F1T2` |
| `0x13` | `F1D2` |
| `0x14` | `F1T3` |
| `0x15` | `F1D3` |
| `0x16` | `F1T4` |
| `0x17` | `F1D4` |
| `0x18..0x1F` | `F2T1..F2D4` |
| `0x20..0x27` | `F3T1..F3D4` |
| `0x2B` | `KPCR` |

**Important**: this machine has **four real curve points** (T1..T4, D1..D4), not
three plus a synthetic `(100,100)`. The reference's "fixed 4th point" is actually
a real EC field here. `F1/F2/F3` are CPU / GPU1 / GPU2.

## 7. Command 14 — fan curve write (`PK0E`), verified offsets

Receives `BUFF = DerefOf(Arg2[0])` and writes:

| Incoming offset | EC field |
|---|---|
| `0x02` | `F1T2` |
| `0x03` | `F1D2` |
| `0x04` | `F1T3` |
| `0x05` | `F1D3` |
| `0x06..0x09` | `F2T2/F2D2/F2T3/F2D3` |
| `0x0A..0x0D` | `F3T2/F3D2/F3T3/F3D3` |
| `0x0E..0x1D` (words) | `F1R1/F1R2/F1R3`, `F2R1/F2R2/F2R3`, `F3R1/F3R2/F3R3` |

Returns `0x14` (20). T1/D1 and T4/D4 are not sent in the write payload.

## 8. Command 121 (`0x79`) — sub-commands (verified)

In `GCMD`/`SCMD`, `Arg1` is the command and `ARGS = Arg2` (an Integer here):
`sub = (ARGS >> 24) & 0xFF`, `value = ARGS & 0xFFFFFF`. This matches the
protocol's `payload[3] = sub` little-endian encoding.

| Sub | Handler | Meaning |
|---|---|---|
| `0x01` | `ECMD 0x02 0x00 0xD7 <mask>` | fan mode: value 0→`0x02`, 1→`0x10`, 2→`0x08`, 5→`0x01`, 6→`0x04`, 7→`0x20`, 8→`0x40` (bitmask on EC) |
| `0x05` | `ECKS` bit4 | |
| `0x07` | | |
| `0x19` (25) | `CPCM`, `APPM[value]`, EC `0x03 0x00 0xD8 0x01` | **performance mode**, value must be `< 0x04` and `PSF4 & 0x04` supported |
| `0x1A` (26) | | |

The reference says fan mode `0`=auto, `8`=quiet; the DSDT shows the mapping is a
bitmask, **not** a direct enum. `SetWMI(121,1,mode)` writes one bit per mode;
`0` clears all bits (auto). This needs care in the implementation.

## 9. Capability gate

- `_STA` returns `0x0F` only if `(PSF0 & 0x80) != 0` and `OSYS >= 0x07DC`.
- Performance mode sub `0x19` requires `PSF4 & 0x04`.
- These are candidates for the `page7` capability substitute until the
  AppSettings channel is verified.

## 10. LIVE READ VERIFICATION (acpi_call, read-only) — SUCCESS

`acpi_call` works for reads. The decisive correction: the `_DSM` Arg0 UUID bytes
are **not** what the reference doc's prose suggested. Extracted directly from the
raw AML (`dsdt.dat` at offset ~506998):

```
e4 24 f2 93 dc fb bf 4b ad d6 db 71 bd c0 af ad
```

This is exactly the 16-byte header found in the original `InsydeDCHU.dll`
(reference README §4.3), confirming the mapping. (An earlier attempt used
`e4 f2 24 93 …`, which was wrong.)

Working call:

```
\_SB.DCHU._DSM b e424f293dcfbbf4badd6db71bdc0afad 0 12 b00
```

`_STA` → `0xf`, both cmd 12 and cmd 13 return buffers. **`Arg3` as a bare
Buffer is accepted on the read path** (the `DerefOf(Arg2[0])` only executes on
the write path, command 14). Command 2 returns `AE_AML_BUFFER_LIMIT` (needs a
larger buffer), which is expected.

### 10.1 Command 13 (fan curve) — live sample

```
FANQ [0x0c] = 2                 -> fan_count = 2 (CPU + GPU1; GPU2 absent)
KBTP [0x0f] = 6                 -> keyboard type 6
F1 CPU : (40,63) (60,91) (80,135) (100,255)   [T,D] D is raw 0..255
F2 GPU1: (40,63) (60,91) (80,135) ( 99,255)
F3 GPU2: all zero
```

Confirmed: **four real curve points** T1..T4/D1..D4, D normalised 0..255, and
`fan_count` is authoritative (2 on this machine). The reference doc's synthetic
`(100,100)` 4th point is wrong for this firmware.

### 10.2 Command 12 (fan status) — live sample under load (CONFIRMED endianness)

Sampled 5 times with two `yes` processes running. Reply is a stable **42-byte**
buffer. Key values:

| Sample | `[2..3]` BE | `[4..5]` BE | `[6..7]` |
|---|---|---|---|
| 1 | 462 | 473 | 0 |
| 4 | 452 | 464 | 0 |
| 5 | 452 | 464 | 0 |

- **RPM is big-endian**: `rpm = (b[2] << 8) | b[3]`. Interpreted little-endian
  the values would be ~50000 rpm, which is physically impossible; big-endian
  gives ~450–470 rpm, which changes with load. This matches the reference docs.
- `[6..7] = 0` on every sample → only two fans, consistent with command 13's
  `fan_count = 2`.
- The reply is **42 bytes**, not the 0x100 the DSDT builds. The remaining
  offsets do **not** line up with `PK0C`'s field list, so duty/temperature
  offsets are treated as **unverified** and are not exposed yet.

**Action for `fan_status.rs`**: keep `cpu_rpm`/`gpu1_rpm` as big-endian,
`gpu2_rpm` will read 0 on this machine; mark duty/temp as unverified rather than
claiming them.

### 10.3 Command 12 reports a rotation PERIOD, not rpm (resolved)

The original Control Center UI (`Page_system_monitor.cs::UpdateUI_CPUFan`)
converts the raw value before display:

```csharp
num = 60.0 / (5.565217391304348E-05 * num) * 2.0;   // = 2_156_250 / raw
```

So the EC stores the fan rotation **period**; rpm = 2 156 250 / raw. A raw 452
therefore means ~4770 rpm, matching what Windows Control Center shows ("a few
thousand"). Raw 0 (fan stopped) is displayed as 0.

Implemented as `clevo_proto::fan_status::period_raw_to_rpm`.

Temperature is also converted in the CC source (`RWReg.cs::CalCPUTemp`) using a
TDP-class-dependent piecewise formula; the TDP class is looked up from
`cpu.ini` by CPU model. Not reproduced yet — `temp_raw` is displayed as-is and
labelled unverified.

## 11. Resolved and still-open items

Resolved since the original list:

- [x] RPM endianness — big-endian, and it is a rotation period (`2156250/raw`).
- [x] Fan-mode value semantics — `121/1` buttons (auto/max/silent/maxq/custom/quiet).
- [x] `_DSM` Arg3 shape — Package{Buffer} for reads, Package{Integer} for 121.
- [x] Command 12/13 live reads and command 121 writes.

Still open:

- [ ] `121/25` value semantics beyond `< 4` (the internal `APPM = {2,3,1,0}`
      mapping) — the logical values work; only the EC-side mapping is unmapped.
- [ ] The AppSettings channel (`0x32240C`) — no equivalent `_DSM` accessor
      found in the DSDT yet; needed for `page 0..7` persistence and capability
      probing.
- [ ] Duty/temperature offsets of command 12 — the 42-byte reply does not match
      `PK0C`'s declared layout, so these remain unverified and are labelled as
      such in the CLI.
- [ ] `CalCPUTemp` temperature conversion — needs the machine's TDP class.
- [ ] Custom curve write (command 14) — byte layout known, not yet tested.

## 12. S5.6 read-only `acpi_call` backend

`AcpiCallTransport` (in `clevo-transport`) issues `_DSM` through
`/proc/acpi/call` but is hard-restricted:

- Only commands **12** and **13** are allowed (`READ_ONLY_COMMANDS`); every other
  command — including the write commands 14 and 121 — is rejected with
  `Unsupported` *before any I/O*.
- `write_app_settings` always returns `Unsupported`; `read_app_settings` too
  (no verified accessor).
- `writable()` returns `false`.
- The call string is built from the verified `ACPI_DSM_PATH` (`\_SB.DCHU._DSM`)
  and `DSM_GUID` constants:

  ```
  \_SB.DCHU._DSM b e424f293dcfbbf4badd6db71bdc0afad 0 <cmd> b<256 bytes>
  ```

Run it with:

```bash
sudo modprobe acpi_call
sudo target/debug/clevo-cc --transport acpi-call fan status
sudo target/debug/clevo-cc --transport acpi-call fan curve
```

The transport reads/writes `/proc/acpi/call` via a small internal hook so tests
can inject a fixed reply; the procfs path is exercised only on real hardware.

### 12.1 acpi_call integration quirks (all handled in code)

1. **Input length cap**: this DKMS build uses `BUFFER_SIZE = 256`, so a single
   write to `/proc/acpi/call` is limited to 511 characters. A full 256-byte
   payload (512 hex chars) cannot be sent. Read commands ignore Arg3, so
   `execute()` sends a one-byte payload (`b00`).
2. **Single read only**: `acpi_proc_read` serves the result once and then resets
   its buffer. More importantly, `read_to_end`/`read_to_string` observe an
   immediate EOF on this procfs file and return nothing, so the code uses a
   single `read()` into a fixed buffer.
3. **Truncated reply**: the `acpi_call` result buffer is 256 bytes, so a long
   `_DSM` reply is cut off before the closing `}`. `parse_reply` accepts both
   closed and truncated `{...` forms and strips a trailing NUL.
4. **Raw payload**: calling `_DSM` directly returns the bare 256-byte payload,
   not the DCHU `tag/len` record envelope the Windows bridge produced.
   `wrap_as_dchu_response` re-creates a single `tag == 0` record so the protocol
   layer is identical for both transports.

### 12.2 Successful live read

```
$ sudo ./target/debug/clevo-cc --transport acpi-call fan status
CPU rpm      : 1254
GPU1 rpm     : 0
CPU temp raw : 37
...
$ sudo ./target/debug/clevo-cc --transport acpi-call fan curve
fan count : 2
cpu : (40C,25%) (60C,36%) (80C,53%) (100C,100%)
```

## 13. S6 kernel driver — `_DSM` Arg3 shape (verified)

The `clevo-cc` out-of-tree driver (`kernel/clevo-cc`, GPL-2.0-only) binds
`ACPI\CLV0001` and calls `_DSM` with a real ACPI object, which `acpi_call`
cannot build.

**The Arg3 shape depends on the command family** (both verified live):

| Command family | Examples | Arg3 shape |
|---|---|---|
| `PK*` (reads) | 12, 13 | `Package { Buffer(256) }` — DSDT does `DerefOf(Arg2[0])` |
| `SCMD` (writes) | 121 | `Package { Integer(value) }` — DSDT does `ARGS = Arg2` and `Index()` |

Failure modes observed while finding this:

- Bare `Integer` for 121 → `AE_AML_OPERAND_TYPE` at `Index`:
  "Needed [Buffer/String/Package], found [Integer]".
- `Package { Buffer(256) }` for 121: `ToInteger` of a >8-byte buffer yields 0,
  so `ARGS` became 0 and the command silently did nothing.
- `Package { Integer }` for 121 works.

**Return codes**: the `SCMD`/`GCMD` families return the **command number**
itself on success (e.g. `0x79` for 121). `0x80000002` means unsupported. The
driver treats "return == function" as success.

**Fan mode** (`121` sub 1), matching the original Control Center fan page
buttons (`RB_FAN_*_Click` in `Page_system_monitor.cs`):

| value | button |
|---|---|
| 0 | auto |
| 1 | MAXIMUM (full speed) |
| 3 | silent |
| 5 | max-q |
| 6 | custom |
| 8 | slow/quiet |

Verified idle effect:

```
auto:  fan1_input = 3718
max:   fan1_input = 6108   (the machine's ~6k ceiling)
auto:  fan1_input = 3428
```

`fan_mode` sysfs reflects only what the driver has written this session; the
firmware does not expose the current mode reliably.

### 13.3 Performance mode (`121` sub 25) — verified

`perf_mode` accepts the four logical values; the firmware applies its internal
`APPM = {2,3,1,0}` mapping to the EC:

| value | mode |
|---|---|
| 0 | quiet |
| 1 | pwrsaving |
| 2 | performance |
| 3 | entertainment |

The DSDT gates the handler on `PSF4 & 0x04` and requires value `< 4`; bit 6
requests TurboFan and bit 7 requests DTT. All four plain modes were accepted
live (idle rpm varied 1817–2093), invalid input returned EINVAL, and no ACPI
errors were logged. TurboFan/DTT are reserved.

### 13.1 Driver surface (implemented vs. reserved)

`clevo-cc` exposes:

| Interface | Kind | Status |
|---|---|---|
| `hwmon fan1_input` / `fan2_input` | read | implemented (CPU, GPU1) |
| `sysfs fan_mode` | rw | implemented: `auto`(0) / `quiet`(8) / `max`(1) / `maxq`(5) |
| `sysfs fan_curve` | read | implemented read-only (command 13) |
| `sysfs perf_mode` | rw | implemented: `quiet`(0) / `pwrsaving`(1) / `performance`(2) / `entertainment`(3) |

**Reserved, not implemented yet:**

- `fan_mode` value `3` (silent) — the DSDT branch for `121/1 = 3` is empty, so
  it is intentionally not exposed; it would do nothing on this firmware.
- `fan_mode` value `6` (custom) — selects "use the custom curve" but has no
  effect until a curve is written with command 14. Reserved for the graphical
  curve editor.
- **Custom curve write (command 14)** — the writable side of `fan_curve`.
  Deferred to the Tauri UI work so the four-point curve can be edited and
  validated before being sent. When implemented it must:
  - send `Arg3 = Package { Buffer(256) }` (the `PK*` shape, same as command 13);
  - encode `[2]=CPU.T2 [3]=CPU.D2*255/100 [4]=CPU.T3 [5]=CPU.D3*255/100`,
    then GPU1 at `[6..9]`, GPU2 at `[10..13]`, and the big-endian slopes
    `R12/R23/R34` at `[14..31]`;
  - validate strictly increasing temperatures and duty `0..100`;
  - be tested against the machine because the EC may reject or reorder points.
- **Fan offset (command 14 via `121/14`)** — verified ineffective under thermal
  protection; not exposed.
- **TurboFan (`121/25` bit 6) / DTT (bit 7)** — modifiers of the performance
  mode; reserved, not exposed. The plain performance modes (0..3) are
  implemented (see §13.3).
- **Silent/MaxQ buttons parity** — `maxq` is exposed; silent is not (see above).

### 13.2 Persistence caveat

The original Control Center persists the fan mode to AppSettings (`SetAPPData`)
in addition to sending `121/1`. The driver only sends the EC command; on reboot
the EC returns to its own default. Persisting the mode is planned for `clevod`
(local TOML + optional EC page), not the kernel driver.

## 14. PolicyKit authorization — verified on real hardware

Tested on a COLORFUL P15 23 running CachyOS with polkit 127, `clevod` on the
system bus using the driver backend (writable).

### 14.1 D-Bus signatures that actually work

- `CheckAuthorization((sa{sv})sa{ss}us)` — the subject is the **struct**
  `(sa{sv})`, not a bare `a{sv}`; a bare map is rejected with
  `InvalidArgs: Type ... does not match expected type ((sa{sv})...)`.
- The details argument is `a{ss}` (string→string), not `a{sv}`.
- The reply is `(bba{ss})` (authorized, challenge, details); decoding it as
  `a{sv}` fails with a signature mismatch.
- The `unix-process` subject needs a **real start time** (22nd field of
  `/proc/<pid>/stat`), not `0`.

### 14.2 Results

| Case | Result |
|---|---|
| action not installed | deny (no error) |
| action installed, explicit allow rule for the user | allow, hardware changed (2087 → 6074 rpm) |
| rule disabled, no temporary authorization | `Access denied`, hardware unchanged |
| root caller | allowed by the root-only fallback when polkit is unreachable |

### 14.3 `AllowUserInteraction` behaviour (re-investigated)

An earlier test suggested flag 1 (`AllowUserInteraction`) authorized an
unix-process subject with no agent and no matching rule. Re-testing with all
temporary authorizations revoked shows that was a **stale temporary
authorization** (`polkit.temporary_authorization_id`, `tmpauthz*`), not a
property of the flag:

- flag 0 (no interaction): returns `authorized=false` immediately
  (`polkit.result=auth_admin_keep`) — a clean denial.
- flag 1 (interaction): **blocks** waiting for an authentication agent, and only
  succeeds if one is reachable.

Interactive prompting is therefore safe to enable. Because a blocked call would
otherwise hang the daemon when no agent exists (headless systems), the authorizer
sets flag 1 and wraps the call in a 60 s `tokio::time::timeout`; on timeout it
treats the result as "authority unavailable" and applies the root-only fallback
(so a non-root caller is denied rather than left waiting).

### 14.4 Interactive prompt verified end to end

With a real authentication agent running (verified on KDE/GNOME-class agents;
these two desktops are the supported targets), a non-root D-Bus call to
`SetFanMode`:

1. resolves the caller to a valid `unix-process` subject
   (`GetConnectionUnixProcessID` + the pid's `/proc/<pid>/stat` start time),
2. reaches PolicyKit, which prompts the user,
3. after the correct password returns `authorized=true` and applies the write
   (verified: `auto` -> `max`, hardware changed).

A second write is not prompted again while the temporary authorization from
`auth_admin_keep` is retained. When no agent is present the call blocks and times
out, and the root-only fallback denies a non-root caller.

The caller is resolved to a login session via logind (`GetSessionByPID`, with a
`ListSessions` fallback for short-lived callers) and PolicyKit is given a
`unix-session` subject when possible, so `auth_admin_keep` can be scoped to the
session. Whether a second write is reused without a prompt depends on the
desktop's agent; KDE and GNOME are the supported targets.

### 14.5 Bus policy

The D-Bus policy must **allow** unprivileged callers to send the write methods
so they can reach the daemon's PolicyKit gate. Denying writes in the bus policy
would prevent any authentication prompt from ever appearing. Authorization is
enforced by PolicyKit inside the daemon, not by the bus.

## 15. WebKitGTK on the NVIDIA proprietary driver — `Gdk Error 71` (verified)

Host: **COLORFUL P15 23**, RTX 4060 Laptop (Ada) on the **NVIDIA proprietary
driver 610.57.04**, WebKitGTK **2.52.5**, KDE Plasma on Wayland.

### 15.1 Symptom

The Tauri UI never reaches the first frame. GTK aborts during toolkit startup:

```
Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.
```

It reproduces on **every** compositor and session type (KDE, GNOME, wlroots;
Wayland and X11), so the wlroots-specific notes in
[`support-matrix.md`](support-matrix.md) §5.4 are not the whole story. The
failure is in WebKit's EGL/GBM buffer allocation, not in the compositor: with
the proprietary driver WebKit cannot obtain a valid GBM buffer format and takes
down the process before mapping the window.

### 15.2 Two environment switches, very different blast radius

| Variable | Effect |
|---|---|
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | Tear down the whole DMA-BUF renderer, **including the accelerated compositor**. The app starts, but `backdrop-filter` stops working (cards render too transparent) and painting falls back to CPU. |
| `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` | Keep the GL compositor; only the buffer **transport** is switched to shared memory. The GBM allocation is bypassed, so the startup crash disappears **while hardware acceleration and blur stay enabled**. |

`WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1` and `..._BUFFER_FORMAT` exist too but did
**not** fix the crash here (`DISABLE_GBM` reproduced `Error 71` unchanged;
`FORCE_SHM` was the only switch that both started and kept acceleration).

### 15.3 Decision

The UI backend (`ui/src-tauri/src/prefs.rs::apply_launch_env`) and
`scripts/run-ui.sh` **always** set `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` unless it
is already present in the environment. This is a workaround for the driver, not
a policy choice: it is harmless on drivers that do not need it, and it keeps the
frosted-glass dashboard accelerated.

The Settings → Compatibility "software rendering" switch is retained only as a
heavier fallback for other broken drivers; it selects the
`WEBKIT_DISABLE_DMABUF_RENDERER=1` sledgehammer and is no longer the default.

Verification: a minimal WebKitGTK 4.1 page (Gtk + WebKitWebView) crashes with
`Error 71` under the defaults, stays up with `FORCE_SHM=1`, and still returns a
WebGL context (so the GPU path is alive) under `FORCE_SHM=1`.

### 15.4 The NVIDIA exit crash (web process `eglTerminate`)

Same host, driver bumped to **615.71.09**, WebKitGTK **2.52.6**. The UI starts and
renders fine, but closing it dumps a core:

```
PID: 23825 (WebKitWebProces)   TID: 23932 (SkiaGPUWorker)
Signal: 11 (SEGV) si_code: SEGV_MAPERR
#0 libnvidia-eglcore.so.615.71.09 + 0x707419
#1-#6 libwebkit2gtk-4.1.so.0
#7 __call_tls_dtors (libc.so.6)
```

and the main thread:

```
#2 libnvidia-glsi.so.615.71.09 + 0x40155
#3 _nv004glsi
...
#18 exit (libc.so.6)
```

**This is not the §15.1 bug under a new stack trace**, and `FORCE_SHM` does not
cover it: it reproduces with and without `WEBKIT_DMABUF_RENDERER_FORCE_SHM`.

#### Root cause (measured)

The web process crashes *while exiting*, in the EGL teardown that the TLS
destructors run. It is not a leak in the app and not about orphaning: the process
is already inside `do_exit` when the crash happens.

Watching the web process right after the window closes:

```
t=0.0s 704035:I ppid=850 thr=28 wchan=do_exit
t=0.5s 704035:I ppid=850 thr=28 wchan=do_exit
...
t=5.0s 704035:gone
```

It sits in state `I` (idle) on `do_exit` for ~5 s with 28 threads winding down.
Letting it finish makes the crash *more* likely, not less — waiting for the
children to exit on their own produced 2 coredumps where cutting them off
produced 0. So there is nothing the parent can do on the way out: any run of the
normal teardown hits it.

#### Fix

Keep Skia off the GPU for the web process (`WEBKIT_SKIA_ENABLE_CPU_RENDERING=1`,
set by `ui/src-tauri/src/prefs.rs::apply_launch_env`). The GPU state that
`eglTerminate` trips over is then never set up.

Measured on the real app, window closed, children allowed to finish exiting:

| Env | Coredumps |
|---|---|
| default | **8/8** |
| `WEBKIT_SKIA_ENABLE_CPU_RENDERING=1` | **0/8** |

The accelerated compositor stays up, so the frosted `backdrop-filter` cards are
unaffected. Screenshot diff of the running UI (1600x900) against the default:

| Comparison | RMSE |
|---|---|
| default vs `SKIA_ENABLE_CPU_RENDERING` | **2.0 %** (visually identical) |
| default vs `WEBKIT_DISABLE_DMABUF_RENDERER` | 4.6 % (blur visibly gone) |

Candidates that also stop the crash but cost more: `WEBKIT_DISABLE_COMPOSITING_MODE=1`
(0/4 coredumps) and `WEBKIT_DISABLE_DMABUF_RENDERER=1` (0/4) — both disable the
accelerated compositor, so the glass effect is lost.
`WEBKIT_HARDWARE_ACCELERATION_POLICY=NEVER` and `FORCE_SHM` do **not** help
(2/2 crashes each).

Counted by how many NVIDIA fds the web process holds: 5 with the GPU teardown
present (crashes) versus 2 for the CPU paths (clean).

#### Guard

`scripts/check-webview-teardown.sh` reproduces this without hardware: it starts
the app, records its WebKit children, closes the window, lets them exit, and fails
if any child outlives the app or any coredump appears. Run it after any change to
the UI's launch environment or to its WebKit/Tauri dependencies.

#### What does *not* work

- **`WEBKIT_EXEC_PATH` helper wrappers** (with `WEBKIT_QUIT_FAST=1`). The
  variable does not exist on WebKitGTK 2.52.6 — the library only reads
  `WEBKIT_DISABLE_DMABUF_RENDERER`, `WEBKIT_DMABUF_RENDERER_*`,
  `WEBKIT_INJECTED_BUNDLE_PATH`, `WEBKIT_PROCESS_MODEL_*`, `WEBKIT_GST_*` and
  friends — and its helper directory `/usr/lib/webkit2gtk-4.1` is hardcoded.
  Children were launched straight from there and ignored the wrappers.
  `LD_PRELOAD` is not an alternative: `ld.so` does not pass it to re-exec'd
  helpers.
- **Closing the WebView from a `WindowEvent` hook** (`CloseRequested` or
  `Destroyed`). Both fire too late or are irrelevant; the child still runs its
  teardown and crashes.
- **Waiting for the children before exiting.** They crash on their own schedule,
  and waiting made it worse (2 crashes vs 0 when cut short).

