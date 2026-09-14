//! Fan status package parsing (command `12`).
//!
//! Layout from `ControlCenter-RE/docs/02-DCHU-WMI协议参考.md` §4.1:
//!
//! ```text
//! [2..3]  CPU rpm, big-endian: (a[2] << 8) | a[3]
//! [4..5]  GPU1 rpm
//! [6..7]  GPU2 rpm
//! [16]    CPU duty
//! [18]    CPU temperature (raw; needs CalCPUTemp conversion)
//! [19]    GPU1 duty      [21] GPU1 temperature
//! [22]    GPU2 duty      [24] GPU2 temperature
//! ```
//!
//! Live verification on a COLORFUL P15 23: the reply is a **42-byte** buffer
//! (not 256) and the RPM fields are big-endian (observed ~452–473 rpm, changing
//! with load). Only the RPM fields are confirmed; the duty/temperature offsets
//! above are inherited from the reference and remain **unverified** on this
//! firmware. Temperature conversion (`CalCPUTemp`) is not reproduced here.
//!
//! Parsing therefore requires only [`MIN_FAN_STATUS_LEN`] bytes so the real
//! 42-byte reply is accepted. See `docs/hardware-notes.md` §10.2.

use crate::error::ProtoError;

/// Minimum command `12` reply length needed to read every field below.
pub const MIN_FAN_STATUS_LEN: usize = 25;

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
    pub cpu_rpm: u16,
    /// GPU1 fan period raw value.
    pub gpu1_rpm: u16,
    /// GPU2 fan period raw value.
    pub gpu2_rpm: u16,
    /// CPU fan duty (0..=255, raw).
    pub cpu_duty: u8,
    /// Raw CPU temperature byte (needs CalCPUTemp conversion).
    pub cpu_temp_raw: u8,
    /// GPU1 fan duty.
    pub gpu1_duty: u8,
    /// Raw GPU1 temperature byte.
    pub gpu1_temp_raw: u8,
    /// GPU2 fan duty.
    pub gpu2_duty: u8,
    /// Raw GPU2 temperature byte.
    pub gpu2_temp_raw: u8,
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
        cpu_rpm: read_be_u16(payload, 2),
        gpu1_rpm: read_be_u16(payload, 4),
        gpu2_rpm: read_be_u16(payload, 6),
        cpu_duty: payload[16],
        cpu_temp_raw: payload[18],
        gpu1_duty: payload[19],
        gpu1_temp_raw: payload[21],
        gpu2_duty: payload[22],
        gpu2_temp_raw: payload[24],
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

    fn sample() -> [u8; MIN_FAN_STATUS_LEN] {
        let mut p = [0u8; MIN_FAN_STATUS_LEN];
        p[2] = 0x01;
        p[3] = 0xCE; // 462 rpm, as observed live
        p[4] = 0x01;
        p[5] = 0xD9; // 473 rpm
        p[6] = 0x00;
        p[7] = 0x00;
        p[16] = 200;
        p[18] = 55;
        p[19] = 180;
        p[21] = 60;
        p[22] = 170;
        p[24] = 65;
        p
    }

    #[test]
    fn parses_all_fields() {
        let status = parse_fan_status(&sample()).unwrap();
        assert_eq!(
            status,
            FanStatus {
                cpu_rpm: 462,
                gpu1_rpm: 473,
                gpu2_rpm: 0,
                cpu_duty: 200,
                cpu_temp_raw: 55,
                gpu1_duty: 180,
                gpu1_temp_raw: 60,
                gpu2_duty: 170,
                gpu2_temp_raw: 65,
            }
        );
    }

    #[test]
    fn rpm_uses_big_endian() {
        let mut p = [0u8; MIN_FAN_STATUS_LEN];
        p[2] = 0x01;
        p[3] = 0x02;
        assert_eq!(parse_fan_status(&p).unwrap().cpu_rpm, 0x0102);
    }

    #[test]
    fn live_42_byte_reply_is_accepted() {
        let mut p = [0u8; 42];
        p[2] = 0x01;
        p[3] = 0xC4;
        p[4] = 0x01;
        p[5] = 0xD0;
        let status = parse_fan_status(&p).unwrap();
        assert_eq!(status.cpu_rpm, 452);
        assert_eq!(status.gpu1_rpm, 464);
    }

    #[test]
    fn short_payload_is_rejected() {
        let err = parse_fan_status(&[0u8; 24]).unwrap_err();
        assert_eq!(
            err,
            ProtoError::BufferTooShort {
                got: 24,
                need: MIN_FAN_STATUS_LEN
            }
        );
    }
}
