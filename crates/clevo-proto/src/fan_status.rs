//! Fan status package parsing (command `12`).
//!
//! The reply layout on this firmware (COLORFUL P15 23, INSYDE BIOS) was
//! **verified live** and is the authoritative source for this module; see
//! `docs/hardware-notes.md` §10.2 and §10.4.
//!
//! ```text
//! [2..3]   CPU  rpm, big-endian: (a[2] << 8) | a[3]
//! [4..5]   GPU1 rpm, big-endian
//! [6..7]   GPU2 rpm, big-endian (always 0 on this two-fan machine)
//! [16]     CPU  duty   (raw 0..255; 255 = 100%)
//! [17]     GPU1 duty
//! [18]     GPU2 duty
//! [19]     CPU  temperature, degrees Celsius (direct, no conversion)
//! [20]     GPU1 temperature
//! [21]     GPU2 temperature
//! ```
//!
//! Two corrections against the reverse-engineering reference
//! (`ControlCenter-RE/docs/02-DCHU-WMI协议参考.md` §4.1), both measured:
//!
//! 1. The reference placed the three temperatures at `[18]`, `[21]` and `[24]`
//!    with duties interleaved between them. The live reply is contiguous:
//!    duty triple at `[16..18]`, temperature triple at `[19..21]`.
//! 2. The reference's "raw temperature" needs no `CalCPUTemp` conversion on this
//!    firmware — the value is already degrees Celsius. Observed while idle
//!    (~37..45 °C) and under load (~80..95 °C on a 45 W part with a 100 °C
//!    limit); a raw EC register would not track a real thermal curve.
//!
//! A raw temperature of `0` means "not reported" (the channel is absent or the
//! EC has nothing to say); it is surfaced as [`Option::None`] rather than as a
//! plausible-looking 0 °C.
//!
//! Parsing requires only [`MIN_FAN_STATUS_LEN`] bytes so the real 42-byte reply
//! is accepted.

use crate::error::ProtoError;

/// Minimum command `12` reply length needed to read every field below.
pub const MIN_FAN_STATUS_LEN: usize = 22;

/// A fan speed as reported by command `12`.
///
/// The EC stores the **rotation period**, not rpm. The original Control Center
/// converts it for display (`UpdateUI_CPUFan` in `Page_system_monitor.cs`):
///
/// ```text
/// displayed_rpm = 60 / (5.565217391304348e-05 * raw) * 2
///               = 2_159_999.9 / raw
/// ```
///
/// A raw value of 452 therefore corresponds to roughly 4770 rpm. Raw 0 means
/// the fan is stopped and is displayed as 0 (the UI guards the division).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanStatus {
    /// CPU fan period raw value (see type docs for the rpm conversion).
    pub cpu_period: u16,
    /// GPU1 fan period raw value.
    pub gpu1_period: u16,
    /// GPU2 fan period raw value.
    pub gpu2_period: u16,
    /// CPU fan duty (`0..=255`, raw; 255 = 100%).
    pub cpu_duty: u8,
    /// GPU1 fan duty.
    pub gpu1_duty: u8,
    /// GPU2 fan duty.
    pub gpu2_duty: u8,
    /// CPU temperature in degrees Celsius (`None` when the EC reports 0).
    pub cpu_temp_c: Option<u8>,
    /// GPU1 temperature in degrees Celsius (`None` when the EC reports 0).
    pub gpu1_temp_c: Option<u8>,
    /// GPU2 temperature in degrees Celsius (`None` when the EC reports 0).
    pub gpu2_temp_c: Option<u8>,
}

impl FanStatus {
    /// CPU fan speed in rpm, derived from the stored rotation period.
    pub fn cpu_rpm(&self) -> u32 {
        period_raw_to_rpm(self.cpu_period)
    }

    /// GPU1 fan speed in rpm.
    pub fn gpu1_rpm(&self) -> u32 {
        period_raw_to_rpm(self.gpu1_period)
    }

    /// GPU2 fan speed in rpm.
    pub fn gpu2_rpm(&self) -> u32 {
        period_raw_to_rpm(self.gpu2_period)
    }
}

/// Interpret a raw temperature byte: `0` means "not reported".
fn temp(raw: u8) -> Option<u8> {
    (raw != 0).then_some(raw)
}

/// Parse a command `12` payload into [`FanStatus`].
///
/// The live firmware returns a short buffer (42 bytes on a COLORFUL P15 23)
/// rather than the 256 bytes the DSDT builds, so only the offsets actually read
/// here must be present. See `docs/hardware-notes.md` §10.2.
pub fn parse_fan_status(payload: &[u8]) -> Result<FanStatus, ProtoError> {
    if payload.len() < MIN_FAN_STATUS_LEN {
        return Err(ProtoError::BufferTooShort {
            got: payload.len(),
            need: MIN_FAN_STATUS_LEN,
        });
    }

    Ok(FanStatus {
        cpu_period: read_be_u16(payload, 2),
        gpu1_period: read_be_u16(payload, 4),
        gpu2_period: read_be_u16(payload, 6),
        cpu_duty: payload[16],
        gpu1_duty: payload[17],
        gpu2_duty: payload[18],
        cpu_temp_c: temp(payload[19]),
        gpu1_temp_c: temp(payload[20]),
        gpu2_temp_c: temp(payload[21]),
    })
}

fn read_be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

/// Scale factor from the Control Center UI source: `60 / 5.565217391304348e-05 * 2`.
const RPM_PERIOD_SCALE: f64 = 60.0 / 5.565_217_391_304_348e-05 * 2.0;

/// Convert a command-12 period raw value to revolutions per minute.
///
/// This mirrors the original Control Center UI (`60 / (5.565217391304348e-05 *
/// raw) * 2`). Raw 0 (fan stopped) maps to 0 rather than dividing by zero.
pub fn period_raw_to_rpm(raw: u16) -> u32 {
    if raw == 0 {
        return 0;
    }
    (RPM_PERIOD_SCALE / f64::from(raw)).round() as u32
}

/// Convert an rpm value back into the rotation period command `12` carries.
///
/// Inverse of [`period_raw_to_rpm`]; used by transports that read rpm from a
/// source that already converted it (e.g. the kernel driver's hwmon).
pub fn rpm_to_period_raw(rpm: u32) -> u16 {
    if rpm == 0 {
        return 0;
    }
    let period = RPM_PERIOD_SCALE / f64::from(rpm);
    period.round().clamp(0.0, f64::from(u16::MAX)) as u16
}

/// Convert a raw duty byte to a percentage, rounding to the nearest integer.
pub fn raw_duty_to_pct(raw: u8) -> u8 {
    ((u32::from(raw) * 100 + 127) / 255) as u8
}

/// Convert a duty percentage to the on-wire byte.
pub fn pct_to_raw_duty(pct: u8) -> u8 {
    ((u32::from(pct) * 255 + 50) / 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_to_rpm_matches_control_center_math() {
        // 2156250 / 452 ~= 4770
        assert_eq!(period_raw_to_rpm(452), 4770);
        // 2156250 / 1254 ~= 1719
        assert_eq!(period_raw_to_rpm(1254), 1719);
        // zero is guarded
        assert_eq!(period_raw_to_rpm(0), 0);
    }

    #[test]
    fn rpm_to_period_is_the_inverse() {
        for rpm in [0u32, 1000, 2089, 3718, 6074] {
            let raw = rpm_to_period_raw(rpm);
            if rpm == 0 {
                assert_eq!(period_raw_to_rpm(raw), 0);
            } else {
                let back = period_raw_to_rpm(raw) as i64;
                assert!(
                    (back - rpm as i64).abs() <= rpm as i64 / 50 + 2,
                    "rpm {rpm} -> period {raw} -> back {back}"
                );
            }
        }
    }

    fn sample() -> [u8; MIN_FAN_STATUS_LEN] {
        let mut p = [0u8; MIN_FAN_STATUS_LEN];
        p[2] = 0x01;
        p[3] = 0xCE; // 462 period raw, as observed live
        p[4] = 0x01;
        p[5] = 0xD9; // 473 period raw
        p[6] = 0x00;
        p[7] = 0x00;
        p[16] = 200; // CPU duty
        p[17] = 180; // GPU1 duty
        p[18] = 0; // GPU2 duty (absent channel)
        p[19] = 55; // CPU temp
        p[20] = 60; // GPU1 temp
        p[21] = 0; // GPU2 temp (absent channel)
        p
    }

    #[test]
    fn parses_all_fields() {
        let status = parse_fan_status(&sample()).unwrap();
        assert_eq!(
            status,
            FanStatus {
                cpu_period: 462,
                gpu1_period: 473,
                gpu2_period: 0,
                cpu_duty: 200,
                gpu1_duty: 180,
                gpu2_duty: 0,
                cpu_temp_c: Some(55),
                gpu1_temp_c: Some(60),
                gpu2_temp_c: None,
            }
        );
    }

    #[test]
    fn rpm_uses_big_endian() {
        let mut p = [0u8; MIN_FAN_STATUS_LEN];
        p[2] = 0x01;
        p[3] = 0x02;
        assert_eq!(parse_fan_status(&p).unwrap().cpu_period, 0x0102);
    }

    #[test]
    fn zero_temperature_is_reported_as_absent() {
        let mut p = [0u8; MIN_FAN_STATUS_LEN];
        p[19] = 0; // CPU reports nothing
        p[20] = 42; // GPU1 reports 42 °C
        let status = parse_fan_status(&p).unwrap();
        assert_eq!(status.cpu_temp_c, None);
        assert_eq!(status.gpu1_temp_c, Some(42));
    }

    #[test]
    fn live_42_byte_reply_is_accepted() {
        let mut p = [0u8; 42];
        p[2] = 0x01;
        p[3] = 0xC4;
        p[4] = 0x01;
        p[5] = 0xD0;
        let status = parse_fan_status(&p).unwrap();
        assert_eq!(status.cpu_period, 452);
        assert_eq!(status.gpu1_period, 464);
        assert_eq!(status.cpu_rpm(), 4770);
    }

    #[test]
    fn short_payload_is_rejected() {
        let err = parse_fan_status(&[0u8; 21]).unwrap_err();
        assert_eq!(
            err,
            ProtoError::BufferTooShort {
                got: 21,
                need: MIN_FAN_STATUS_LEN
            }
        );
    }

    #[test]
    fn duty_conversion_roundtrips() {
        for pct in 0..=100u8 {
            assert!((raw_duty_to_pct(pct_to_raw_duty(pct)) as i32 - i32::from(pct)).abs() <= 1);
        }
    }
}
