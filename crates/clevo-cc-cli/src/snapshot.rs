//! Presentation model for fan status, shared by `status` and `watch`.
//!
//! Fan availability is derived from the command `13` `fan_count` value: on this
//! machine only two fan channels exist (`CPU`, `GPU1`), and readings for
//! channels at or beyond `fan_count` are marked **unavailable** rather than
//! reported as if they were real zeroes.
//!
//! Command `12` reports the fan **rotation period**, not rpm; the Control
//! Center formula (`clevo_proto::fan_status::period_raw_to_rpm`) is applied for
//! display while the raw value is kept alongside.

use clevo_proto::fan_status::{period_raw_to_rpm, raw_duty_to_pct};
use clevo_proto::FanStatus;

/// Number of fan channels the protocol can encode.
pub const MAX_FANS: usize = 3;

/// One fan channel's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanReading {
    /// Rotation period raw value from the EC.
    pub period_raw: u16,
    /// Speed in rpm derived via the Control Center formula.
    pub rpm: u32,
    /// Fan duty as a percentage (`0..=100`).
    pub duty_pct: u8,
    /// Temperature in degrees Celsius, or `None` when the EC reports none.
    pub temp_c: Option<u8>,
    /// Whether this channel exists on this machine.
    pub available: bool,
}

/// A complete fan status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanSnapshot {
    /// CPU fan (channel 1).
    pub cpu: FanReading,
    /// GPU1 fan (channel 2).
    pub gpu1: FanReading,
    /// GPU2/third fan (channel 3); usually absent.
    pub gpu2: FanReading,
}

impl FanSnapshot {
    /// Build a snapshot from a status package and the known fan count.
    ///
    /// Channels with index `>= fan_count` (1-based) are marked unavailable.
    /// A `fan_count` of `0` means "unknown", in which case all channels are
    /// treated as available so raw data is never hidden.
    pub fn from_status(status: &FanStatus, fan_count: u8) -> Self {
        let available = |index: usize| fan_count == 0 || (index as u8) <= fan_count;
        let read = |period: u16, duty: u8, temp: Option<u8>, index: usize| FanReading {
            period_raw: period,
            rpm: period_raw_to_rpm(period),
            duty_pct: raw_duty_to_pct(duty),
            temp_c: temp,
            available: available(index),
        };
        Self {
            cpu: read(status.cpu_period, status.cpu_duty, status.cpu_temp_c, 1),
            gpu1: read(status.gpu1_period, status.gpu1_duty, status.gpu1_temp_c, 2),
            gpu2: read(status.gpu2_period, status.gpu2_duty, status.gpu2_temp_c, 3),
        }
    }

    /// Iterate over `(name, reading)` in channel order.
    pub fn readings(&self) -> [(&'static str, &FanReading); MAX_FANS] {
        [
            ("CPU", &self.cpu),
            ("GPU1", &self.gpu1),
            ("GPU2", &self.gpu2),
        ]
    }
}

/// Render a temperature for display; absent values are explicit.
fn temp_text(temp_c: Option<u8>) -> String {
    match temp_c {
        Some(t) => format!("{t}C"),
        None => "n/a".to_string(),
    }
}

/// Render a snapshot as a single-line human-readable row.
pub fn format_row(snapshot: &FanSnapshot) -> String {
    let mut parts = Vec::new();
    for (name, reading) in snapshot.readings() {
        if reading.available {
            parts.push(format!(
                "{name}={}rpm/{}%/{}",
                reading.rpm,
                reading.duty_pct,
                temp_text(reading.temp_c)
            ));
        } else {
            parts.push(format!("{name}=n/a"));
        }
    }
    parts.join("  ")
}

/// Render a snapshot as a minimal JSON object (no serde dependency).
pub fn format_json(snapshot: &FanSnapshot) -> String {
    let mut fields = Vec::new();
    for (name, reading) in snapshot.readings() {
        let key = name.to_ascii_lowercase();
        if reading.available {
            let temp = match reading.temp_c {
                Some(t) => t.to_string(),
                None => "null".to_string(),
            };
            fields.push(format!(
                "\"{key}\":{{\"rpm\":{},\"period_raw\":{},\"duty_pct\":{},\"temp_c\":{}}}",
                reading.rpm, reading.period_raw, reading.duty_pct, temp
            ));
        } else {
            fields.push(format!("\"{key}\":null"));
        }
    }
    format!("{{{}}}", fields.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clevo_proto::FanStatus;

    fn status() -> FanStatus {
        FanStatus {
            cpu_period: 452, // ~4770 rpm
            gpu1_period: 0,
            gpu2_period: 0,
            cpu_duty: 63, // ~25%
            gpu1_duty: 0,
            gpu2_duty: 0,
            cpu_temp_c: Some(37),
            gpu1_temp_c: Some(33),
            gpu2_temp_c: None,
        }
    }

    #[test]
    fn two_fan_machine_marks_gpu2_unavailable() {
        let snap = FanSnapshot::from_status(&status(), 2);
        assert!(snap.cpu.available);
        assert!(snap.gpu1.available);
        assert!(!snap.gpu2.available);
    }

    #[test]
    fn rpm_is_derived_from_period() {
        let snap = FanSnapshot::from_status(&status(), 2);
        assert_eq!(snap.cpu.rpm, 4770);
        assert_eq!(snap.cpu.period_raw, 452);
        assert_eq!(snap.gpu1.rpm, 0);
    }

    #[test]
    fn duty_is_a_percentage() {
        let snap = FanSnapshot::from_status(&status(), 2);
        assert_eq!(snap.cpu.duty_pct, 25);
        assert_eq!(snap.gpu1.duty_pct, 0);
    }

    #[test]
    fn absent_temperature_is_not_zero() {
        let snap = FanSnapshot::from_status(&status(), 2);
        assert_eq!(snap.cpu.temp_c, Some(37));
        // GPU2 is absent on this machine, so the whole channel is null.
        assert_eq!(snap.gpu2.temp_c, None);
        // An available channel that reports 0 must still surface as null-ish,
        // not as a real 0 °C: build one and check the JSON.
        let mut status = status();
        status.gpu1_temp_c = None;
        let snap = FanSnapshot::from_status(&status, 2);
        assert_eq!(snap.gpu1.temp_c, None);
        assert!(
            format_json(&snap)
                .contains("\"gpu1\":{\"rpm\":0,\"period_raw\":0,\"duty_pct\":0,\"temp_c\":null}"),
            "json: {}",
            format_json(&snap)
        );
    }

    #[test]
    fn absent_temperature_renders_as_na_in_rows() {
        let mut status = status();
        status.cpu_temp_c = None;
        let snap = FanSnapshot::from_status(&status, 2);
        assert!(format_row(&snap).contains("CPU=4770rpm/25%/n/a"));
    }

    #[test]
    fn unknown_fan_count_keeps_all_available() {
        let snap = FanSnapshot::from_status(&status(), 0);
        assert!(snap.gpu2.available);
    }

    #[test]
    fn row_marks_unavailable_channel() {
        let snap = FanSnapshot::from_status(&status(), 2);
        let row = format_row(&snap);
        assert!(row.contains("CPU=4770rpm/25%/37C"), "row: {row}");
        assert!(row.contains("GPU1=0rpm/0%/33C"), "row: {row}");
        assert!(row.contains("GPU2=n/a"), "row: {row}");
    }

    #[test]
    fn json_uses_null_for_unavailable_channel() {
        let snap = FanSnapshot::from_status(&status(), 2);
        let json = format_json(&snap);
        assert!(json.contains("\"cpu\":{\"rpm\":4770,\"period_raw\":452"));
        assert!(json.contains("\"gpu2\":null"));
        assert!(!json.contains("\"gpu2\":{"));
    }
}
