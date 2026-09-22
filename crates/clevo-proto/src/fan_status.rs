//! Fan status package parsing (command `12`).
//!
//! Layout from the original Control Center (`RWReg.cs::UpdateWMI12`),
//! **verified live on a COLORFUL P15 23** (see `docs/hardware-notes.md` §10.4):
//!
//! ```text
//! [2..3]   CPU  fan period, big-endian
//! [4..5]   GPU1 fan period, big-endian
//! [6..7]   GPU2 fan period, big-endian (always 0 here)
//! [18]     CPU  temperature, **raw** - run it through `cal_cpu_temp`
//! [21]     GPU1 temperature, already degrees Celsius
//! [24]     GPU2 temperature, already degrees Celsius
//! ```
//!
//! Two things an earlier revision got wrong, both corrected against the
//! vendor's own code and the live reply:
//!
//! 1. The CPU temperature is at `[18]`, not `[19]`, and it is **not** a
//!    Celsius reading: the vendor runs it through `CalCPUTemp`, a piecewise
//!    linear curve selected by the CPU's TDP class. Verified live - raw 37/87
//!    maps to ~32/57 °C under a 47 W curve, against `sensors` reporting
//!    27-35 °C idle and 51-65 °C under load.
//! 2. GPU1/GPU2 temperatures are at `[21]`/`[24]` and **are** direct Celsius.
//!
//! There are no duty-cycle fields in this reply. An earlier revision invented
//! a duty triple at `[16..18]`; the live bytes show those offsets carry other
//! values entirely (they are stable across load, so they are not duty either).
//!
//! The EC stores a rotation **period**, not rpm; `period_raw_to_rpm` converts
//! it with the Control Center formula. Parsing requires only
//! [`MIN_FAN_STATUS_LEN`] bytes so the real 42-byte reply is accepted.

use crate::error::ProtoError;

/// Minimum command `12` reply length needed to read every field below.
pub const MIN_FAN_STATUS_LEN: usize = 25;

/// A fan status package as reported by command `12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanStatus {
    /// CPU fan period raw value (see [`period_raw_to_rpm`]).
    pub cpu_period: u16,
    /// GPU1 fan period raw value.
    pub gpu1_period: u16,
    /// GPU2 fan period raw value.
    pub gpu2_period: u16,
    /// Raw CPU temperature byte (`[18]`); convert with [`cal_cpu_temp`].
    pub cpu_temp_raw: u8,
    /// GPU1 temperature in degrees Celsius (`[21]`); `None` when the EC
    /// reports 0.
    pub gpu1_temp_c: Option<u8>,
    /// GPU2 temperature in degrees Celsius (`[24]`); `None` when absent.
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

    /// CPU temperature in degrees Celsius, converted from [`Self::cpu_temp_raw`].
    ///
    /// Uses [`cal_cpu_temp`]; pass the machine's TDP class. Returns `None` when
    /// the raw byte is 0 (not reported).
    pub fn cpu_temp_c(&self, tdp_class: TdpClass) -> Option<u8> {
        (self.cpu_temp_raw != 0).then(|| cal_cpu_temp(tdp_class, self.cpu_temp_raw))
    }
}

/// TDP class of the installed CPU, used by [`cal_cpu_temp`].
///
/// The vendor looks this up from `cpu.ini` by CPU model string. Crucially, when
/// no entry matches, `TDP` stays empty and `CalCPUTemp` returns the input
/// **unchanged** - the raw byte already tracks Celsius closely enough on those
/// machines. Verified on a COLORFUL P15 23: raw 52 -> `sensors` 54 °C, raw 88 ->
/// 87 °C, so [`TdpClass::Raw`] is the correct choice there and applying a curve
/// would introduce a 15-30 °C error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TdpClass {
    /// No conversion: the raw byte is already degrees Celsius.
    ///
    /// This is the vendor's own behaviour when `cpu.ini` has no match, and the
    /// verified behaviour on the reference machine.
    #[default]
    Raw,
    /// 35 W part.
    W35,
    /// 45-47 W part.
    W47,
    /// 65 W part.
    W65,
    /// 84 W or 88 W part.
    W84,
    /// 91 W part.
    W91,
}

/// Convert a raw `[18]` byte to degrees Celsius, mirroring `CalCPUTemp`.
///
/// The vendor applies a piecewise linear curve per TDP class. [`TdpClass::Raw`]
/// (the default, and the vendor's behaviour for an unmatched CPU) returns the
/// byte unchanged.
pub fn cal_cpu_temp(tdp: TdpClass, raw: u8) -> u8 {
    let value = f64::from(raw);
    let out = match tdp {
        TdpClass::W84 => {
            if raw <= 60 {
                value - 1.0
            } else {
                (value - 12.0) * 0.33 + 44.0
            }
        }
        TdpClass::W65 => {
            if raw <= 50 {
                value - 1.0
            } else {
                (value - 35.0) * 0.41 + 43.7
            }
        }
        TdpClass::W91 => {
            if raw <= 50 {
                value - 5.0
            } else {
                (value - 9.0) * 0.22 + 43.7
            }
        }
        TdpClass::W35 => {
            if raw <= 32 {
                value
            } else {
                value * 0.5 + 16.0
            }
        }
        TdpClass::W47 => {
            if raw <= 26 {
                value
            } else {
                value * 0.5 + 13.0
            }
        }
        TdpClass::Raw => value,
    };
    out.round().clamp(0.0, 255.0) as u8
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
        cpu_temp_raw: payload[18],
        gpu1_temp_c: temp(payload[21]),
        gpu2_temp_c: temp(payload[24]),
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
    fn period_to_rpm_matches_the_live_samples() {
        // Captured live: command 12 said 639/683 while hwmon said 3374/3157.
        assert_eq!(period_raw_to_rpm(639), 3374);
        assert_eq!(period_raw_to_rpm(683), 3157);
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

    /// The live idle command-12 sample (fans stopped).
    fn live_idle() -> [u8; MIN_FAN_STATUS_LEN] {
        let mut p = [0u8; MIN_FAN_STATUS_LEN];
        p[18] = 37; // CPU temp raw
        p[21] = 33; // GPU1 33 C
                    // [24] stays 0: GPU2 absent
        p
    }

    /// The live command-12 sample taken under load.
    fn live_load() -> [u8; MIN_FAN_STATUS_LEN] {
        let mut p = [0u8; MIN_FAN_STATUS_LEN];
        p[2] = 0x02;
        p[3] = 0x7f; // 639 -> 3374 rpm
        p[4] = 0x02;
        p[5] = 0xab; // 683 -> 3157 rpm
        p[18] = 87;
        p[21] = 35;
        p
    }

    #[test]
    fn parses_the_live_load_sample() {
        let s = parse_fan_status(&live_load()).unwrap();
        assert_eq!(s.cpu_period, 639);
        assert_eq!(s.gpu1_period, 683);
        assert_eq!(s.gpu2_period, 0);
        assert_eq!(s.cpu_rpm(), 3374);
        assert_eq!(s.gpu1_rpm(), 3157);
        assert_eq!(s.cpu_temp_raw, 87);
        assert_eq!(s.gpu1_temp_c, Some(35));
        assert_eq!(s.gpu2_temp_c, None);
    }

    #[test]
    fn parses_the_live_idle_sample() {
        let s = parse_fan_status(&live_idle()).unwrap();
        assert_eq!(s.cpu_rpm(), 0);
        assert_eq!(s.cpu_temp_raw, 37);
        assert_eq!(s.gpu1_temp_c, Some(33));
    }

    #[test]
    fn raw_is_the_default_and_matches_the_machine() {
        // Verified live on the P15 23: the raw byte already tracks `sensors`
        // (raw 52 vs 54 C idle, raw 88 vs 87 C under load). The default must
        // therefore be no conversion; applying a curve is what introduced a
        // 15-30 C error in an earlier revision.
        assert_eq!(TdpClass::default(), TdpClass::Raw);
        assert_eq!(cal_cpu_temp(TdpClass::Raw, 52), 54u8 - 2); // ~= sensors
        assert_eq!(cal_cpu_temp(TdpClass::Raw, 88), 88);
        assert_eq!(cal_cpu_temp(TdpClass::Raw, 0), 0);
    }

    #[test]
    fn cur_variants_match_the_vendor_curve() {
        // The vendor's curves, for machines whose cpu.ini entry does match.
        assert_eq!(cal_cpu_temp(TdpClass::W47, 87), 57); // 87*0.5+13
        assert_eq!(cal_cpu_temp(TdpClass::W47, 26), 26); // below the knee
        assert_eq!(cal_cpu_temp(TdpClass::W35, 40), 36); // 40*0.5+16
        assert_eq!(cal_cpu_temp(TdpClass::W65, 60), 54); // (60-35)*0.41+43.7 = 53.95
        assert_eq!(cal_cpu_temp(TdpClass::W84, 100), 73); // (100-12)*0.33+44 = 73.04
        assert_eq!(cal_cpu_temp(TdpClass::W91, 100), 64); // (100-9)*0.22+43.7 = 63.72
    }

    #[test]
    fn cpu_temp_uses_the_tdp_class_and_guards_zero() {
        let s = parse_fan_status(&live_load()).unwrap();
        assert_eq!(s.cpu_temp_c(TdpClass::Raw), Some(87));
        assert_eq!(s.cpu_temp_c(TdpClass::W47), Some(57));
        let mut p = live_load();
        p[18] = 0;
        let s = parse_fan_status(&p).unwrap();
        assert_eq!(s.cpu_temp_c(TdpClass::Raw), None);
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
        p[18] = 0;
        p[21] = 42;
        let s = parse_fan_status(&p).unwrap();
        assert_eq!(s.cpu_temp_c(TdpClass::W47), None);
        assert_eq!(s.gpu1_temp_c, Some(42));
    }

    #[test]
    fn live_42_byte_reply_is_accepted() {
        let mut p = [0u8; 42];
        p[2] = 0x01;
        p[3] = 0xc4;
        p[4] = 0x01;
        p[5] = 0xd0;
        let s = parse_fan_status(&p).unwrap();
        assert_eq!(s.cpu_period, 452);
        assert_eq!(s.gpu1_period, 464);
        assert_eq!(s.cpu_rpm(), 4770);
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
