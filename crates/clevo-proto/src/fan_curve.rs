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
//!           Rnn = round((D(n+1) - Dn) / (T(n+1) - Tn) / 100 * 255 * 16)
//!               = round((D(n+1) - Dn) * 40.8 / (T(n+1) - Tn))
//! ```
//!
//! Note the asymmetry, faithfully reproduced: the read side exposes all four
//! points; the write side starts at point 2 (T1/D1 and T4/D4 are not sent).
//! Duty is modelled as a percentage (`0..=100`).
//!
//! The slope is a **raw-duty** slope scaled by 16: duty is stored as `0..255`
//! on the wire, so a rise of `d` percentage points across `Δt` degrees is
//! `d/100*255/Δt` raw units per degree, and the firmware wants that in 16ths.
//! An earlier revision divided the percentage delta by `Δt` and multiplied by
//! `2.55 * 16`, which is the same expression only when `d = 100`; for any other
//! delta it was wrong by a factor of `100/d`.

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

    // Slopes, big-endian u16, three per fan. Only R2 (the T2->T3 segment) is
    // written: the write fully specifies those two points, while R1 and R3
    // depend on T1/T4, which command 14 does not carry and the EC owns. The
    // kernel driver's encoder does the same, so sending our own idea of R1/R3
    // would overwrite the EC's with a slope derived from points it never
    // received.
    //
    // A channel whose middle points are zero is skipped entirely: it has no
    // meaningful slope and the EC keeps its own curve for it.
    let absent = |points: &[FanPoint; CURVE_POINTS]| points[1].temp == 0 && points[2].temp == 0;
    if !absent(&curve.cpu) {
        write_slope(&mut payload, 16, curve.cpu[1], curve.cpu[2])?;
    }
    if !absent(&curve.gpu1) {
        write_slope(&mut payload, 22, curve.gpu1[1], curve.gpu1[2])?;
    }
    if !absent(&curve.gpu2) {
        write_slope(&mut payload, 28, curve.gpu2[1], curve.gpu2[2])?;
    }

    Ok(payload)
}

fn validate_curve(curve: &FanCurve) -> Result<(), ProtoError> {
    for (fan, points) in [
        ("cpu", &curve.cpu),
        ("gpu1", &curve.gpu1),
        ("gpu2", &curve.gpu2),
    ] {
        // A channel the write does not carry is not validated.
        //
        // Command 14 sends only points 2 and 3; the EC owns the first and last.
        // A channel whose middle points are zero is therefore left alone, which
        // is what the transport and the kernel both key on. Checking all four
        // points here instead rejected curves that are perfectly writable: a
        // machine with two fans can hold leftovers in GPU2 (say T2=50, T3=70,
        // but T1=T4=0), and the strict increase rule then failed on T3 > T4 -
        // making a curve read from the EC impossible to send back.
        if points[1].temp == 0 && points[2].temp == 0 {
            continue;
        }
        for (i, point) in points.iter().enumerate() {
            if point.duty_pct > 100 {
                return Err(ProtoError::InvalidCurve {
                    fan: fan.to_string(),
                    reason: format!("point {i} duty {}% exceeds 100%", point.duty_pct),
                });
            }
        }
        // The pair the write actually carries must increase; T4 is the EC's and
        // may be zero on a channel that is only partly populated.
        if points[2].temp <= points[1].temp {
            return Err(ProtoError::InvalidCurve {
                fan: fan.to_string(),
                reason: format!(
                    "temperatures must strictly increase: T2={} >= T3={}",
                    points[1].temp, points[2].temp
                ),
            });
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

/// `round((D(n+1) - Dn) / (T(n+1) - Tn) / 100 * 255 * 16)` in raw-duty units.
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
    // Duty is a percentage here, but the EC stores 0..255 and the slope is
    // scaled by 16.
    let dd = i32::from(to.duty_pct) - i32::from(from.duty_pct);
    let value = (dd as f64 / dt as f64) / 100.0 * 255.0 * 16.0;
    Ok(value.round().clamp(0.0, f64::from(u16::MAX)) as u16)
}

/// Convert a raw duty byte (`0..255`) to a percentage, rounding to nearest.
///
/// Used by the fan-curve path, where duty genuinely is stored raw on the wire.
pub fn raw_duty_to_pct(raw: u8) -> u8 {
    ((u32::from(raw) * 100 + 127) / 255) as u8
}

/// Convert a duty percentage to the on-wire raw byte.
pub fn pct_to_raw_duty(pct: u8) -> u8 {
    ((u32::from(pct) * 255 + 50) / 100) as u8
}

impl FanCurve {
    /// The curve for `fan` (`"cpu"`, `"gpu1"` or `"gpu2"`), or `None`.
    pub fn fan(&self, fan: &str) -> Option<&[FanPoint; CURVE_POINTS]> {
        match fan {
            "cpu" => Some(&self.cpu),
            "gpu1" => Some(&self.gpu1),
            "gpu2" => Some(&self.gpu2),
            _ => None,
        }
    }

    /// Mutable access to the curve for `fan`, or `None` if the name is unknown.
    pub fn fan_mut(&mut self, fan: &str) -> Option<&mut [FanPoint; CURVE_POINTS]> {
        match fan {
            "cpu" => Some(&mut self.cpu),
            "gpu1" => Some(&mut self.gpu1),
            "gpu2" => Some(&mut self.gpu2),
            _ => None,
        }
    }
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
    fn encodes_the_middle_slope_only() {
        // Only R2 (T2->T3) is written: it is the one segment the write fully
        // specifies. R1 and R3 depend on T1/T4, which command 14 does not
        // carry, so sending our own values would overwrite the EC's.
        let payload = encode_curve(&default_curve()).unwrap();
        let r2 = compute_slope(default_curve().cpu[1], default_curve().cpu[2]).unwrap();
        assert_eq!(&payload[16..18], &r2.to_be_bytes());
        // The neighbours stay zero for the EC to keep.
        assert_eq!(&payload[14..16], &[0, 0], "R1 must not be sent");
        assert_eq!(&payload[18..20], &[0, 0], "R3 must not be sent");

        let gpu2_r2 = compute_slope(default_curve().gpu2[1], default_curve().gpu2[2]).unwrap();
        assert_eq!(&payload[28..30], &gpu2_r2.to_be_bytes());
    }

    #[test]
    fn slope_formula_matches_reference() {
        // (36 - 25) / (60 - 40) / 100 * 255 * 16
        //   = 0.55 raw-%/°C * 40.8 = 22.44 -> 22
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
    fn slope_matches_the_control_center_expression() {
        // The reference formula in raw-duty space:
        //   round((raw(D2) - raw(D1)) / (T2 - T1) * 16)
        // where raw(p) = p * 255 / 100. Compute it independently to pin the
        // implementation to the EC's units rather than to a coincidence.
        let cases = [
            (
                FanPoint {
                    temp: 40,
                    duty_pct: 25,
                },
                FanPoint {
                    temp: 60,
                    duty_pct: 36,
                },
            ),
            (
                FanPoint {
                    temp: 60,
                    duty_pct: 36,
                },
                FanPoint {
                    temp: 80,
                    duty_pct: 53,
                },
            ),
            (
                FanPoint {
                    temp: 80,
                    duty_pct: 53,
                },
                FanPoint {
                    temp: 100,
                    duty_pct: 100,
                },
            ),
            (
                FanPoint {
                    temp: 30,
                    duty_pct: 0,
                },
                FanPoint {
                    temp: 31,
                    duty_pct: 1,
                },
            ),
            (
                FanPoint {
                    temp: 30,
                    duty_pct: 40,
                },
                FanPoint {
                    temp: 90,
                    duty_pct: 45,
                },
            ),
        ];
        for (from, to) in cases {
            let expected = {
                let raw = |p: FanPoint| f64::from(p.duty_pct) / 100.0 * 255.0;
                let dt = f64::from(to.temp) - f64::from(from.temp);
                ((raw(to) - raw(from)) / dt * 16.0).round() as u16
            };
            let got = compute_slope(from, to).unwrap();
            assert_eq!(
                got, expected,
                "slope {:?} -> {:?}: got {got}, expected {expected}",
                from, to
            );
        }
    }

    #[test]
    fn slope_formula_is_the_reference_expression_in_raw_units() {
        // The reference gives `* 2.55 * 16` on the percentage delta; the
        // implementation computes it in raw-duty units. `/100*255*16` and
        // `*2.55*16` are the same expression, so the two must agree for every
        // delta. This pins that equivalence instead of asserting a difference
        // that does not exist.
        for (from_pct, to_pct) in [(25u8, 36u8), (10, 20), (50, 100), (0, 1)] {
            for (t1, t2) in [(40u8, 60u8), (30, 31), (40, 90)] {
                let from = FanPoint {
                    temp: t1,
                    duty_pct: from_pct,
                };
                let to = FanPoint {
                    temp: t2,
                    duty_pct: to_pct,
                };
                let reference = (((f64::from(to_pct) - f64::from(from_pct))
                    / (f64::from(t2) - f64::from(t1)))
                    * 2.55
                    * 16.0)
                    .round() as u16;
                assert_eq!(
                    compute_slope(from, to).unwrap(),
                    reference,
                    "slope {from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn accessors_select_the_right_fan() {
        let mut curve = default_curve();
        assert_eq!(curve.fan("cpu").unwrap()[0].temp, 40);
        assert_eq!(curve.fan("gpu1").unwrap().len(), CURVE_POINTS);
        assert!(curve.fan("nope").is_none());
        curve.fan_mut("gpu2").unwrap()[0].temp = 33;
        assert_eq!(curve.gpu2[0].temp, 33);
        assert!(curve.fan_mut("nope").is_none());
    }

    #[test]
    fn encode_rejects_non_increasing_middle_temperatures() {
        // T2 and T3 are the pair the write carries, so these must increase.
        let mut curve = default_curve();
        curve.cpu[2].temp = curve.cpu[1].temp;
        assert!(matches!(
            encode_curve(&curve).unwrap_err(),
            ProtoError::InvalidCurve { .. }
        ));
    }

    #[test]
    fn encode_accepts_a_partly_populated_channel() {
        // A two-fan machine can hold leftovers in GPU2 whose first and last
        // points are zero (T2=50, T3=70, T1=T4=0). The write does not carry
        // T1/T4, so this is perfectly writable - and rejecting it made a curve
        // read from the EC impossible to send back.
        let mut curve = default_curve();
        curve.gpu2 = [
            FanPoint {
                temp: 0,
                duty_pct: 0,
            },
            FanPoint {
                temp: 50,
                duty_pct: 39,
            },
            FanPoint {
                temp: 70,
                duty_pct: 67,
            },
            FanPoint {
                temp: 0,
                duty_pct: 0,
            },
        ];
        let payload = encode_curve(&curve).expect("a writable channel must encode");
        assert_eq!(payload[10], 50);
        assert_eq!(payload[12], 70);
    }

    #[test]
    fn encode_skips_a_channel_with_no_middle_points() {
        // Middle points zero means "leave this channel alone", matching what
        // the driver and the kernel do.
        let mut curve = default_curve();
        curve.gpu2 = [FanPoint {
            temp: 0,
            duty_pct: 0,
        }; 4];
        let payload = encode_curve(&curve).unwrap();
        assert_eq!(&payload[10..14], &[0, 0, 0, 0]);
        assert_eq!(&payload[28..30], &[0, 0], "no slope without points");
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
