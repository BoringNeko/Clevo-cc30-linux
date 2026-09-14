//! Fan curve package parsing/encoding (commands `13` and `14`).
//!
//! Verified against a live COLORFUL P15 23 (see `docs/hardware-notes.md` §10.1)
//! and the machine's DSDT (`PK0D`/`PK0E`).
//!
//! Read layout (command `13`; the live reply is 42 bytes):
//!
//! ```text
//! [0x0c]  FANQ  -> fan count (2 on this machine)
//! [0x0e]  EC 0xD7 value (init mode)
//! [0x0f]  KBTP  -> keyboard type
//! [0x10]=F1T1 [0x11]=F1D1 [0x12]=F1T2 [0x13]=F1D2
//! [0x14]=F1T3 [0x15]=F1D3 [0x16]=F1T4 [0x17]=F1D4      CPU
//! [0x18..0x1f] = F2T1..F2D4                            GPU1
//! [0x20..0x27] = F3T1..F3D4                            GPU2 (often absent)
//! ```
//!
//! There are **four real curve points** (`T1..T4`, `D1..D4`); the reference
//! doc's synthetic fixed `(100,100)` fourth point does not exist on this
//! firmware. Duty is stored raw `0..255`.
//!
//! Write layout (command `14`), from `PK0E`:
//!
//! ```text
//! [2]=F1T2 [3]=F1D2 [4]=F1T3 [5]=F1D3
//! [6..9]  = F2T2/F2D2/F2T3/F2D3
//! [10..13]= F3T2/F3D2/F3T3/F3D3
//! [14..31]= slopes R1/R2/R3 per fan, big-endian u16:
//!           Rnn = round((D(n+1) - Dn) / (T(n+1) - Tn) * 2.55 * 16)
//! ```
//!
//! Note the asymmetry, faithfully reproduced: the read side exposes all four
//! points; the write side starts at point 2 (T1/D1 and T4/D4 are not sent).
//! Duty is modelled as a percentage (`0..=100`).

use crate::error::ProtoError;

/// Number of points in a fan curve.
pub const CURVE_POINTS: usize = 4;

/// Minimum command `13` reply length needed to read all three fans.
pub const MIN_CURVE_LEN: usize = 0x28;

/// A single `(temperature °C, duty percent)` point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanPoint {
    /// Temperature in degrees Celsius.
    pub temp: u8,
    /// Fan duty as a percentage (`0..=100`).
    pub duty_pct: u8,
}

/// A four-point fan curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanCurve {
    /// CPU fan curve (`F1`).
    pub cpu: [FanPoint; CURVE_POINTS],
    /// GPU1 fan curve (`F2`).
    pub gpu1: [FanPoint; CURVE_POINTS],
    /// GPU2 fan curve (`F3`); zeroed on machines with fewer fans.
    pub gpu2: [FanPoint; CURVE_POINTS],
}

/// Parsed command `13` package: curve plus machine metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanCurveInfo {
    /// Number of fans reported by the firmware (`FANQ`, `[0x0c]`).
    pub fan_count: u8,
    /// Initial/current fan mode (`[0x0e]`).
    pub init_mode: u8,
    /// Keyboard type code (`KBTP`, `[0x0f]`).
    pub kb_type: u8,
    /// The fan curve.
    pub curve: FanCurve,
}

/// Parse a command `13` payload.
///
/// Requires only [`MIN_CURVE_LEN`] bytes so the live 42-byte reply is accepted.
pub fn parse_curve(payload: &[u8]) -> Result<FanCurveInfo, ProtoError> {
    if payload.len() < MIN_CURVE_LEN {
        return Err(ProtoError::BufferTooShort {
            got: payload.len(),
            need: MIN_CURVE_LEN,
        });
    }

    Ok(FanCurveInfo {
        fan_count: payload[0x0c],
        init_mode: payload[0x0e],
        kb_type: payload[0x0f],
        curve: FanCurve {
            cpu: read_curve(payload, 0x10),
            gpu1: read_curve(payload, 0x18),
            gpu2: read_curve(payload, 0x20),
        },
    })
}

/// Encode a fan curve into a command `14` payload (0-based, 256 bytes).
///
/// Returns [`ProtoError::InvalidCurve`] if any point is malformed or if the
/// curve's temperatures are not strictly increasing.
pub fn encode_curve(curve: &FanCurve) -> Result<[u8; 256], ProtoError> {
    validate_curve(curve)?;

    let mut payload = [0u8; 256];

    // T2/D2/T3/D3 for each fan, starting at point index 1.
    write_pair(&mut payload, 2, curve.cpu[1]);
    write_pair(&mut payload, 4, curve.cpu[2]);
    write_pair(&mut payload, 6, curve.gpu1[1]);
    write_pair(&mut payload, 8, curve.gpu1[2]);
    write_pair(&mut payload, 10, curve.gpu2[1]);
    write_pair(&mut payload, 12, curve.gpu2[2]);

    // Slopes R1 (point1->2), R2 (2->3), R3 (3->4), big-endian u16.
    write_slope(&mut payload, 14, curve.cpu[0], curve.cpu[1])?;
    write_slope(&mut payload, 16, curve.cpu[1], curve.cpu[2])?;
    write_slope(&mut payload, 18, curve.cpu[2], curve.cpu[3])?;
    write_slope(&mut payload, 20, curve.gpu1[0], curve.gpu1[1])?;
    write_slope(&mut payload, 22, curve.gpu1[1], curve.gpu1[2])?;
    write_slope(&mut payload, 24, curve.gpu1[2], curve.gpu1[3])?;
    write_slope(&mut payload, 26, curve.gpu2[0], curve.gpu2[1])?;
    write_slope(&mut payload, 28, curve.gpu2[1], curve.gpu2[2])?;
    write_slope(&mut payload, 30, curve.gpu2[2], curve.gpu2[3])?;

    Ok(payload)
}

fn validate_curve(curve: &FanCurve) -> Result<(), ProtoError> {
    for (fan, points) in [
        ("cpu", &curve.cpu),
        ("gpu1", &curve.gpu1),
        ("gpu2", &curve.gpu2),
    ] {
        for (i, point) in points.iter().enumerate() {
            if point.duty_pct > 100 {
                return Err(ProtoError::InvalidCurve {
                    fan: fan.to_string(),
                    reason: format!("point {i} duty {}% exceeds 100%", point.duty_pct),
                });
            }
        }
        for i in 0..CURVE_POINTS - 1 {
            if points[i + 1].temp <= points[i].temp {
                return Err(ProtoError::InvalidCurve {
                    fan: fan.to_string(),
                    reason: format!(
                        "temperatures must strictly increase: T{}={} >= T{}={}",
                        i + 1,
                        points[i].temp,
                        i + 2,
                        points[i + 1].temp
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Read four `(T, D)` points: T at `offset+0,+2,+4,+6`, D at `+1,+3,+5,+7`.
fn read_curve(payload: &[u8], offset: usize) -> [FanPoint; CURVE_POINTS] {
    let mut points = [FanPoint {
        temp: 0,
        duty_pct: 0,
    }; CURVE_POINTS];
    for (i, point) in points.iter_mut().enumerate() {
        point.temp = payload[offset + i * 2];
        point.duty_pct = raw_duty_to_pct(payload[offset + i * 2 + 1]);
    }
    points
}

/// Write `(T, D)` at `offset` and `offset+1`.
fn write_pair(payload: &mut [u8], offset: usize, point: FanPoint) {
    payload[offset] = point.temp;
    payload[offset + 1] = pct_to_raw_duty(point.duty_pct);
}

fn write_slope(
    payload: &mut [u8],
    offset: usize,
    from: FanPoint,
    to: FanPoint,
) -> Result<(), ProtoError> {
    let slope = compute_slope(from, to)?;
    payload[offset..offset + 2].copy_from_slice(&slope.to_be_bytes());
    Ok(())
}

/// `round((D(n+1) - Dn) / (T(n+1) - Tn) * 2.55 * 16)` in percent space.
///
/// Returns [`ProtoError::InvalidCurve`] when the temperatures are equal.
pub fn compute_slope(from: FanPoint, to: FanPoint) -> Result<u16, ProtoError> {
    let dt = i32::from(to.temp) - i32::from(from.temp);
    if dt <= 0 {
        return Err(ProtoError::InvalidCurve {
            fan: String::from("slope"),
            reason: format!(
                "temperatures must strictly increase: {} -> {}",
                from.temp, to.temp
            ),
        });
    }
    let dd = i32::from(to.duty_pct) - i32::from(from.duty_pct);
    let value = (dd as f64 / dt as f64) * 2.55 * 16.0;
    Ok(value.round() as u16)
}

/// Convert a raw duty byte to a percentage, rounding to the nearest integer.
fn raw_duty_to_pct(raw: u8) -> u8 {
    ((u32::from(raw) * 100 + 127) / 255) as u8
}

/// Convert a duty percentage to the on-wire byte.
fn pct_to_raw_duty(pct: u8) -> u8 {
    ((u32::from(pct) * 255 + 50) / 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_curve() -> FanCurve {
        let template = [
            FanPoint {
                temp: 40,
                duty_pct: 25,
            },
            FanPoint {
                temp: 60,
                duty_pct: 36,
            },
            FanPoint {
                temp: 80,
                duty_pct: 53,
            },
            FanPoint {
                temp: 100,
                duty_pct: 100,
            },
        ];
        FanCurve {
            cpu: template,
            gpu1: template,
            gpu2: template,
        }
    }

    /// The live command-13 reply captured from the machine.
    fn live_reply() -> [u8; 42] {
        let mut p = [0u8; 42];
        // [0x0c]=2 fan count, [0x0e]=2 init mode, [0x0f]=6 kb type
        p[0x0c] = 2;
        p[0x0e] = 2;
        p[0x0f] = 6;
        // CPU: (40,63)(60,91)(80,135)(100,255)
        p[0x10] = 40;
        p[0x11] = 63;
        p[0x12] = 60;
        p[0x13] = 91;
        p[0x14] = 80;
        p[0x15] = 135;
        p[0x16] = 100;
        p[0x17] = 255;
        // GPU1: (40,63)(60,91)(80,135)(99,255)
        p[0x18] = 40;
        p[0x19] = 63;
        p[0x1a] = 60;
        p[0x1b] = 91;
        p[0x1c] = 80;
        p[0x1d] = 135;
        p[0x1e] = 99;
        p[0x1f] = 255;
        // GPU2: zero
        p
    }

    #[test]
    fn parses_live_reply_metadata() {
        let info = parse_curve(&live_reply()).unwrap();
        assert_eq!(info.fan_count, 2);
        assert_eq!(info.init_mode, 2);
        assert_eq!(info.kb_type, 6);
    }

    #[test]
    fn parses_live_reply_curves() {
        let info = parse_curve(&live_reply()).unwrap();
        assert_eq!(info.curve.cpu[0].temp, 40);
        assert_eq!(info.curve.cpu[0].duty_pct, 25); // 63/255 ~= 25%
        assert_eq!(info.curve.cpu[1].temp, 60);
        assert_eq!(info.curve.cpu[3].temp, 100);
        assert_eq!(info.curve.cpu[3].duty_pct, 100); // 255
        assert_eq!(info.curve.gpu1[3].temp, 99);
        assert_eq!(
            info.curve.gpu2[0],
            FanPoint {
                temp: 0,
                duty_pct: 0
            }
        );
    }

    #[test]
    fn short_curve_payload_is_rejected() {
        assert_eq!(
            parse_curve(&[0u8; 0x27]).unwrap_err(),
            ProtoError::BufferTooShort {
                got: 0x27,
                need: MIN_CURVE_LEN
            }
        );
    }

    #[test]
    fn encodes_t2_d2_t3_d3_into_slots() {
        let payload = encode_curve(&default_curve()).unwrap();
        assert_eq!(payload[2], 60); // CPU.T2
        assert_eq!(payload[3], pct_to_raw_duty(36)); // CPU.D2
        assert_eq!(payload[4], 80); // CPU.T3
        assert_eq!(payload[5], pct_to_raw_duty(53)); // CPU.D3
        assert_eq!(payload[6], 60); // GPU1.T2
        assert_eq!(payload[10], 60); // GPU2.T2
    }

    #[test]
    fn encodes_slopes_big_endian() {
        let payload = encode_curve(&default_curve()).unwrap();
        let r1 = compute_slope(default_curve().cpu[0], default_curve().cpu[1]).unwrap();
        assert_eq!(&payload[14..16], &r1.to_be_bytes());
    }

    #[test]
    fn slope_formula_matches_reference() {
        // (36 - 25) / (60 - 40) * 2.55 * 16 = 0.55 * 2.55 * 16 = 22.44 -> 22
        let slope = compute_slope(
            FanPoint {
                temp: 40,
                duty_pct: 25,
            },
            FanPoint {
                temp: 60,
                duty_pct: 36,
            },
        )
        .unwrap();
        assert_eq!(slope, 22);
    }

    #[test]
    fn encode_rejects_non_increasing_temperature() {
        let mut curve = default_curve();
        curve.cpu[1].temp = 40;
        assert!(matches!(
            encode_curve(&curve).unwrap_err(),
            ProtoError::InvalidCurve { .. }
        ));
    }

    #[test]
    fn encode_rejects_duty_above_100() {
        let mut curve = default_curve();
        curve.cpu[1].duty_pct = 101;
        assert!(matches!(
            encode_curve(&curve).unwrap_err(),
            ProtoError::InvalidCurve { .. }
        ));
    }

    #[test]
    fn compute_slope_rejects_equal_temperatures() {
        let p = FanPoint {
            temp: 50,
            duty_pct: 10,
        };
        assert!(matches!(
            compute_slope(p, p).unwrap_err(),
            ProtoError::InvalidCurve { .. }
        ));
    }

    #[test]
    fn duty_roundtrip_is_within_one_percent() {
        for pct in 0..=100u8 {
            assert!((raw_duty_to_pct(pct_to_raw_duty(pct)) as i32 - i32::from(pct)).abs() <= 1);
        }
    }
}
