//! Kernel-driver transport: talk to the in-tree `clevo-cc` platform driver.
//!
//! The driver exposes a higher-level sysfs/hwmon interface than raw `_DSM`
//! commands, so this transport **synthesizes** standard DCHU reply buffers from
//! the driver's files. That lets the rest of the stack (`clevo-proto` parsers,
//! `clevod`, the CLI) work unchanged while gaining a *writable* backend that
//! does not need `/proc/acpi/call` or the `acpi_call` module.
//!
//! File mapping:
//!
//! | Command | Driver interface |
//! |---|---|
//! | `12` fan status | hwmon `fan1_input`, `fan2_input` |
//! | `13` fan curve  | sysfs `fan_curve` |
//! | `14` fan curve write | sysfs `fan_curve` |
//! | `121/1` fan mode | sysfs `fan_mode` |
//! | `121/25` perf mode | sysfs `perf_mode` |
//!
//! Only the values the driver actually reports are filled in; temperature and
//! duty fields are left at zero (which the parser surfaces as "not reported")
//! rather than invented.

use crate::error::{TransportError, TransportResult};
use crate::{Transport, TransportKind, PAYLOAD_BYTES};

/// Default platform device path for the `CLV0001` ACPI device.
pub const DEFAULT_PLATFORM_PATH: &str = "/sys/devices/platform/CLV0001:00";

/// `clevo-cc` transport backed by the kernel driver's sysfs and hwmon.
#[derive(Debug, Clone)]
pub struct DriverTransport {
    platform: std::path::PathBuf,
}

impl Default for DriverTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl DriverTransport {
    /// Use the default platform device path.
    pub fn new() -> Self {
        Self {
            platform: std::path::PathBuf::from(DEFAULT_PLATFORM_PATH),
        }
    }

    /// Use an explicit platform device path (tests, non-standard installs).
    pub fn with_platform_path(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            platform: path.into(),
        }
    }

    fn attr(&self, name: &str) -> std::path::PathBuf {
        self.platform.join(name)
    }

    fn read_attr(&self, name: &str) -> TransportResult<String> {
        std::fs::read_to_string(self.attr(name))
            .map(|s| s.trim().to_string())
            .map_err(|e| TransportError::Io(format!("{}: {e}", self.attr(name).display())))
    }

    fn write_attr(&self, name: &str, value: &str) -> TransportResult<()> {
        std::fs::write(self.attr(name), format!("{value}\n"))
            .map_err(|e| TransportError::Io(format!("{}: {e}", self.attr(name).display())))
    }

    /// Locate this driver's hwmon directory under `self.platform/hwmon`.
    fn hwmon_dir(&self) -> TransportResult<std::path::PathBuf> {
        let base = self.attr("hwmon");
        let entries = std::fs::read_dir(&base)
            .map_err(|e| TransportError::Io(format!("{}: {e}", base.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("fan1_input").exists() {
                return Ok(path);
            }
        }
        Err(TransportError::Io(format!(
            "no hwmon fan inputs under {}",
            base.display()
        )))
    }

    fn read_rpm(&self, which: &str) -> TransportResult<u32> {
        let dir = self.hwmon_dir()?;
        let path = dir.join(which);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| TransportError::Io(format!("{}: {e}", path.display())))?;
        text.trim()
            .parse()
            .map_err(|e| TransportError::MalformedResponse(format!("{}: {e}", path.display())))
    }

    /// Build a synthetic command `12` reply from hwmon.
    ///
    /// The DCHU reply carries a rotation *period*, and `clevo-proto` converts it
    /// back to rpm with `period = RPM_PERIOD_SCALE / rpm`. hwmon already reports
    /// rpm, so we invert that formula here; writing the rpm directly into the
    /// period field would apply the conversion twice and under-report the speed.
    ///
    /// hwmon does not expose fan duty or temperatures, so those bytes stay zero.
    /// `parse_fan_status` reads a zero temperature as "not reported", so the UI
    /// shows no temperature rather than a false 0 °C.
    fn fan_status_reply(&self) -> TransportResult<Vec<u8>> {
        let cpu_rpm = self.read_rpm("fan1_input")?;
        let gpu_rpm = self.read_rpm("fan2_input").unwrap_or(0);
        let mut payload = vec![0u8; 42];
        payload[2..4].copy_from_slice(&rpm_to_period(cpu_rpm).to_be_bytes());
        payload[4..6].copy_from_slice(&rpm_to_period(gpu_rpm).to_be_bytes());
        Ok(wrap_payload(&payload))
    }

    /// Write a command `14` payload to the sysfs `fan_curve` attribute.
    ///
    /// Decodes the on-wire layout the protocol layer produced and emits the
    /// driver's textual form (the inverse of [`Self::fan_curve_reply`]) so the
    /// driver can hand it to the EC unchanged.
    fn write_fan_curve(&self, payload: &[u8; PAYLOAD_BYTES]) -> TransportResult<Vec<u8>> {
        let text = decode_curve_for_driver(payload)?;
        self.write_attr("fan_curve", text.trim_end())?;
        Ok(wrap_integer(14))
    }

    /// Build a synthetic command `13` reply from the sysfs curve file.
    fn fan_curve_reply(&self) -> TransportResult<Vec<u8>> {
        let text = self.read_attr("fan_curve")?;
        let mut payload = vec![0u8; 0x28];
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("fan_count=") {
                let mut parts = rest.split_whitespace();
                let count: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let kb: u8 = parts
                    .find_map(|s| s.strip_prefix("kb_type="))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                payload[0x0c] = count;
                payload[0x0f] = kb;
            } else if let Some((name, points)) = line.split_once(':') {
                let offset = match name.trim() {
                    "cpu" => 0x10,
                    "gpu1" => 0x18,
                    "gpu2" => 0x20,
                    _ => continue,
                };
                let mut bytes = Vec::new();
                for pair in points.split_whitespace() {
                    for value in pair.split(',') {
                        if let Ok(v) = value.parse::<u8>() {
                            bytes.push(v);
                        }
                    }
                }
                for (i, value) in bytes.into_iter().take(8).enumerate() {
                    payload[offset + i] = value;
                }
            }
        }
        Ok(wrap_payload(&payload))
    }

    /// Handle a `121` main-command write by dispatching on the sub-command.
    ///
    /// `build_subcommand_payload` stores the sub-command in `payload[3]` and the
    /// scalar value's low byte in `payload[0]`.
    fn main_command(&self, payload: &[u8; PAYLOAD_BYTES]) -> TransportResult<Vec<u8>> {
        let sub = payload[3];
        let value = u32::from(payload[0]);
        match sub {
            1 => {
                let name = fan_mode_name(value)?;
                self.write_attr("fan_mode", name)?;
            }
            25 => {
                let name = perf_mode_name(value)?;
                self.write_attr("perf_mode", name)?;
            }
            other => {
                return Err(TransportError::Unsupported(format!(
                    "121/{other} is not supported by the driver transport"
                )))
            }
        }
        Ok(wrap_integer(121))
    }
}

/// Decode a command `14` payload into the driver's `fan_curve` text form.
///
/// The on-wire payload only carries points 2 and 3 (`T1`/`D1` and `T4`/`D4` are
/// not sent by the Windows stack, and the EC keeps its own first and last
/// points), so the text form reports the two points it can honestly describe.
/// Slopes are not needed by the driver: it recomputes nothing and passes the
/// points through.
pub fn decode_curve_for_driver(payload: &[u8; PAYLOAD_BYTES]) -> Result<String, TransportError> {
    let mut out = String::from("fan_count=3\n");
    for (fan, base) in [("cpu", 2usize), ("gpu1", 6), ("gpu2", 10)] {
        let t2 = payload[base];
        let d2 = payload[base + 1];
        let t3 = payload[base + 2];
        let d3 = payload[base + 3];
        if t2 == 0 && t3 == 0 {
            // An untouched fan: keep it out of the write so the EC retains its
            // own curve for that channel.
            out.push_str(&format!("{fan}: 0,0 0,0 0,0 0,0\n"));
            continue;
        }
        if t3 <= t2 {
            return Err(TransportError::MalformedResponse(format!(
                "{fan}: curve write has non-increasing temperatures T2={t2} T3={t3}"
            )));
        }
        out.push_str(&format!(
            "{fan}: 0,0 {t2},{d2} {t3},{d3} 0,0\n"
        ));
    }
    Ok(out)
}

/// Convert an rpm value from hwmon into the rotation period the DCHU reply is
/// expected to carry.
///
/// `clevo_proto::fan_status::period_raw_to_rpm` uses `rpm = 2_156_250 / period`,
/// so the inverse is `period = 2_156_250 / rpm`. rpm 0 (stopped) maps to 0, which
/// the parser also treats as 0.
pub fn rpm_to_period(rpm: u32) -> u16 {
    clevo_proto::fan_status::rpm_to_period_raw(rpm)
}

/// Map a `121/1` value to the name the driver's `fan_mode` accepts.
fn fan_mode_name(value: u32) -> TransportResult<&'static str> {
    match value {
        0 => Ok("auto"),
        1 => Ok("max"),
        5 => Ok("maxq"),
        8 => Ok("quiet"),
        other => Err(TransportError::Unsupported(format!(
            "fan mode {other} has no driver name"
        ))),
    }
}

/// Map a `121/25` value to the name the driver's `perf_mode` accepts.
fn perf_mode_name(value: u32) -> TransportResult<&'static str> {
    match value {
        0 => Ok("quiet"),
        1 => Ok("pwrsaving"),
        2 => Ok("performance"),
        3 => Ok("entertainment"),
        other => Err(TransportError::Unsupported(format!(
            "performance mode {other} has no driver name"
        ))),
    }
}

/// Wrap a payload in a response with a single `tag == 0` record.
fn wrap_payload(payload: &[u8]) -> Vec<u8> {
    wrap_record(payload)
}

/// Wrap a `u32` as a response with a single `tag == 0` record.
fn wrap_integer(value: u32) -> Vec<u8> {
    wrap_record(&value.to_le_bytes())
}

fn wrap_record(data: &[u8]) -> Vec<u8> {
    // Response layout: [8..12] record count (LE), records at 12:
    // u16 tag, u16 length, data.
    let mut buf = Vec::with_capacity(12 + 4 + data.len());
    buf.extend_from_slice(&[0u8; 8]);
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // tag
    buf.extend_from_slice(&(data.len() as u16).to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

impl Transport for DriverTransport {
    fn execute(&self, command: u32, payload: &[u8; PAYLOAD_BYTES]) -> TransportResult<Vec<u8>> {
        match command {
            12 => self.fan_status_reply(),
            13 => self.fan_curve_reply(),
            14 => self.write_fan_curve(payload),
            121 => self.main_command(payload),
            other => Err(TransportError::Unsupported(format!(
                "command {other} is not supported by the driver transport"
            ))),
        }
    }

    fn read_app_settings(&self, _page: u8, _offset: u16, _len: u16) -> TransportResult<Vec<u8>> {
        Err(TransportError::Unsupported(
            "the driver does not expose AppSettings".into(),
        ))
    }

    fn write_app_settings(&self, _page: u8, _offset: u16, _data: &[u8]) -> TransportResult<()> {
        Err(TransportError::Unsupported(
            "the driver does not expose AppSettings".into(),
        ))
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Driver
    }

    fn writable(&self) -> bool {
        true
    }

    /// Read the driver's current `fan_mode` and `perf_mode` sysfs values.
    ///
    /// These reflect what the driver last wrote (or its load default), not the
    /// EC's power-on state, but they are the only observable mode on this
    /// backend and are far better than reporting nothing.
    fn current_modes(&self) -> TransportResult<(Option<u8>, Option<u8>)> {
        let fan = self
            .read_attr("fan_mode")
            .ok()
            .and_then(|name| fan_mode_value(&name));
        let perf = self
            .read_attr("perf_mode")
            .ok()
            .and_then(|name| perf_mode_value(&name));
        Ok((fan, perf))
    }
}

/// Map a driver `fan_mode` name to its `121/1` value.
fn fan_mode_value(name: &str) -> Option<u8> {
    match name {
        "auto" => Some(0),
        "max" => Some(1),
        "maxq" => Some(5),
        "quiet" => Some(8),
        _ => None,
    }
}

/// Map a driver `perf_mode` name to its `121/25` value.
fn perf_mode_value(name: &str) -> Option<u8> {
    match name {
        "quiet" => Some(0),
        "pwrsaving" => Some(1),
        "performance" => Some(2),
        "entertainment" => Some(3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clevo_proto::command::{
        CMD_FAN_CURVE_READ, CMD_FAN_STATUS, CMD_MAIN, SUB_FAN_MODE, SUB_POWER_MODE,
    };
    use clevo_proto::fan_curve::parse_curve;
    use clevo_proto::fan_status::parse_fan_status;
    use clevo_proto::message::{build_subcommand_payload, empty_payload};
    use clevo_proto::response::{response_first_record, response_integer};

    struct FakeSysfs {
        root: std::path::PathBuf,
    }

    impl FakeSysfs {
        fn new(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("clevo-driver-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("hwmon/hwmon0")).unwrap();
            std::fs::write(root.join("hwmon/hwmon0/fan1_input"), "3718\n").unwrap();
            std::fs::write(root.join("hwmon/hwmon0/fan2_input"), "1788\n").unwrap();
            std::fs::write(root.join("fan_mode"), "auto\n").unwrap();
            std::fs::write(root.join("perf_mode"), "unknown\n").unwrap();
            std::fs::write(
                root.join("fan_curve"),
                "fan_count=2 kb_type=6\n\
                 cpu: 40,63 60,91 80,135 100,255\n\
                 gpu1: 40,63 60,91 80,135 99,255\n\
                 gpu2: 0,0 0,3 0,5 0,0\n",
            )
            .unwrap();
            Self { root }
        }

        fn transport(&self) -> DriverTransport {
            DriverTransport::with_platform_path(&self.root)
        }
    }

    impl Drop for FakeSysfs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn rejects_unsupported_commands() {
        let fake = FakeSysfs::new("unsupported");
        let t = fake.transport();
        assert!(matches!(
            t.execute(99, &empty_payload()),
            Err(TransportError::Unsupported(_))
        ));
    }

    #[test]
    fn writes_curve_to_sysfs() {
        use clevo_proto::fan_curve::{encode_curve, FanCurve, FanPoint};

        let fake = FakeSysfs::new("curvewrite");
        let t = fake.transport();
        let point = |temp, duty_pct| FanPoint { temp, duty_pct };
        let curve = FanCurve {
            cpu: [
                point(40, 20),
                point(55, 40),
                point(75, 70),
                point(95, 100),
            ],
            gpu1: [
                point(45, 25),
                point(60, 45),
                point(80, 75),
                point(99, 100),
            ],
            gpu2: [point(0, 0); 4],
        };
        let payload = encode_curve(&curve).unwrap();
        let reply = t.execute(14, &payload).unwrap();
        assert_eq!(response_integer(&reply).unwrap(), 14);

        let written = std::fs::read_to_string(fake.root.join("fan_curve")).unwrap();
        // Points 2 and 3 are surfaced; the write carries raw duty bytes.
        assert!(written.contains("cpu: 0,0 55,"), "written: {written}");
        assert!(written.contains("75,"), "written: {written}");
        assert!(written.contains("gpu1: 0,0 60,"), "written: {written}");
    }

    #[test]
    fn curve_write_rejects_non_increasing_temperatures() {
        use clevo_proto::fan_curve::{encode_curve, FanCurve, FanPoint};

        let fake = FakeSysfs::new("curvebad");
        let t = fake.transport();
        let point = |temp, duty_pct| FanPoint { temp, duty_pct };
        // encode_curve validates, so build the payload by hand to reach the
        // driver's own guard.
        let mut payload = encode_curve(&FanCurve {
            cpu: [point(40, 20), point(55, 40), point(75, 70), point(95, 100)],
            gpu1: [point(45, 25), point(60, 45), point(80, 75), point(99, 100)],
            gpu2: [point(0, 0); 4],
        })
        .unwrap();
        // Force T3 <= T2 for the CPU channel.
        payload[4] = payload[2];
        assert!(matches!(
            t.execute(14, &payload).unwrap_err(),
            TransportError::MalformedResponse(_)
        ));
    }

    #[test]
    fn reads_fan_status_from_hwmon() {
        use clevo_proto::fan_status::period_raw_to_rpm;

        let fake = FakeSysfs::new("status");
        let t = fake.transport();
        assert!(t.writable());
        let reply = t.execute(CMD_FAN_STATUS.get(), &empty_payload()).unwrap();
        let status = parse_fan_status(response_first_record(&reply).unwrap()).unwrap();
        // The reply carries a period; converting it back must recover the rpm
        // hwmon reported (3718 and 1788) within the granularity of the 16-bit
        // period field, not a double-converted value.
        let cpu = status.cpu_rpm();
        let gpu = status.gpu1_rpm();
        assert!((cpu as i64 - 3718).abs() < 50, "cpu back = {cpu}");
        assert!((gpu as i64 - 1788).abs() < 50, "gpu back = {gpu}");
        assert_eq!(period_raw_to_rpm(status.cpu_period), cpu);
        // hwmon has no temperature source, so the driver reports none.
        assert_eq!(status.cpu_temp_c, None);
    }

    #[test]
    fn rpm_to_period_inverts_the_parser() {
        use clevo_proto::fan_status::period_raw_to_rpm;
        for rpm in [0u32, 1000, 2089, 3718, 6074] {
            let raw = rpm_to_period(rpm);
            if rpm == 0 {
                assert_eq!(period_raw_to_rpm(raw), 0);
            } else {
                // The 42-byte field is coarse; allow a small rounding error.
                let back = period_raw_to_rpm(raw) as i64;
                assert!(
                    (back - rpm as i64).abs() <= rpm as i64 / 50 + 2,
                    "rpm {rpm} -> period {raw} -> back {back}"
                );
            }
        }
    }
    #[test]
    fn reads_curve_from_sysfs() {
        let fake = FakeSysfs::new("curve");
        let t = fake.transport();
        let reply = t
            .execute(CMD_FAN_CURVE_READ.get(), &empty_payload())
            .unwrap();
        let info = parse_curve(response_first_record(&reply).unwrap()).unwrap();
        assert_eq!(info.fan_count, 2);
        assert_eq!(info.kb_type, 6);
        assert_eq!(info.curve.cpu[0].temp, 40);
        assert_eq!(info.curve.cpu[3].temp, 100);
        assert_eq!(info.curve.gpu1[3].temp, 99);
    }

    #[test]
    fn writes_fan_mode() {
        let fake = FakeSysfs::new("fanmode");
        let t = fake.transport();
        let payload = build_subcommand_payload(1, SUB_FAN_MODE); // max
        let reply = t.execute(CMD_MAIN.get(), &payload).unwrap();
        assert_eq!(response_integer(&reply).unwrap(), 121);
        assert_eq!(
            std::fs::read_to_string(fake.root.join("fan_mode"))
                .unwrap()
                .trim(),
            "max"
        );
    }

    #[test]
    fn writes_perf_mode() {
        let fake = FakeSysfs::new("perfmode");
        let t = fake.transport();
        let payload = build_subcommand_payload(2, SUB_POWER_MODE); // performance
        t.execute(CMD_MAIN.get(), &payload).unwrap();
        assert_eq!(
            std::fs::read_to_string(fake.root.join("perf_mode"))
                .unwrap()
                .trim(),
            "performance"
        );
    }

    #[test]
    fn rejects_unknown_mode_values() {
        let fake = FakeSysfs::new("badmode");
        let t = fake.transport();
        let payload = build_subcommand_payload(3, SUB_FAN_MODE); // silent: no driver name
        let err = t.execute(CMD_MAIN.get(), &payload).unwrap_err();
        assert!(matches!(err, TransportError::Unsupported(_)));
    }

    #[test]
    fn curve_write_is_now_supported() {
        use clevo_proto::fan_curve::{encode_curve, FanCurve, FanPoint};

        let fake = FakeSysfs::new("curveok");
        let t = fake.transport();
        let point = |temp, duty_pct| FanPoint { temp, duty_pct };
        let curve = FanCurve {
            cpu: [point(40, 20), point(55, 40), point(75, 70), point(95, 100)],
            gpu1: [point(45, 25), point(60, 45), point(80, 75), point(99, 100)],
            gpu2: [point(0, 0); 4],
        };
        let payload = encode_curve(&curve).unwrap();
        assert!(t.execute(14, &payload).is_ok());
    }

    #[test]
    fn reads_current_modes_from_sysfs() {
        let fake = FakeSysfs::new("modes");
        let t = fake.transport();
        // FakeSysfs writes fan_mode="auto" and perf_mode="unknown".
        let (fan, perf) = t.current_modes().unwrap();
        assert_eq!(fan, Some(0)); // auto
        assert_eq!(perf, None); // "unknown" is not a known mode
    }
}
