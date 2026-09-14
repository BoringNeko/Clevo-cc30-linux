//! Capability bitmap parsing (`page 7`, 256 bytes).
//!
//! From `ControlCenter-RE/docs/02-DCHU-WMI协议参考.md` §4.3. `page7[0..1]` is
//! the feature-table version, big-endian, which selects the parsing layout:
//!
//! Version `0x0000`:
//! ```text
//! page7[17] bit0..3 = supports quiet/pwrsaving/performance/entertainment
//!           bit4..7 = PowerModeUI_ID
//! ```
//!
//! Version `0x0100` adds TurboFan, MSHybrid switch, NVIDIA PowerOff and slow
//! fan flags. Unknown versions are reported, never guessed at.

use crate::constants::PAYLOAD_LEN;
use crate::error::ProtoError;

/// Feature-table version, read from `page7[0..1]` (big-endian).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page7Version {
    /// Legacy table (`0x0000`): only the four power-mode bits.
    V0,
    /// Extended table (`0x0100`): power modes plus hardware feature flags.
    V1,
}

/// Which power/situational modes the firmware advertises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerModeSupport {
    /// Quiet mode (`0`).
    pub quiet: bool,
    /// Power-saving mode (`1`).
    pub power_saving: bool,
    /// Performance mode (`2`).
    pub performance: bool,
    /// Entertainment mode (`3`).
    pub entertainment: bool,
}

impl PowerModeSupport {
    /// True if the mode with the given numeric value is supported.
    pub fn supports(&self, mode_value: u8) -> bool {
        match mode_value {
            0 => self.quiet,
            1 => self.power_saving,
            2 => self.performance,
            3 => self.entertainment,
            _ => false,
        }
    }
}

/// Parsed capability information from `page 7`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Detected feature-table version.
    pub version: Page7Version,
    /// Advertised power modes.
    pub power_modes: PowerModeSupport,
    /// `PowerModeUI_ID` (`page7[17]` bits 4..7).
    pub power_mode_ui_id: u8,
    /// Turbo fan supported (`page7[16]` bit4, V1 only).
    pub turbo_fan: bool,
    /// MSHybrid dGPU/iGPU switching supported (`page7[18]` bit0, V1 only).
    pub ms_hybrid_switch: bool,
    /// NVIDIA PowerOff supported (`page7[19]` bit6, V1 only).
    pub nvidia_power_off: bool,
    /// Slow fan supported (`page7[20]` bit0, V1 only).
    pub slow_fan: bool,
}

/// Parse a 256-byte `page 7` buffer.
///
/// Returns [`ProtoError::UnknownPage7Version`] for any version other than the
/// two documented values.
pub fn parse_capabilities(page7: &[u8]) -> Result<Capabilities, ProtoError> {
    if page7.len() < PAYLOAD_LEN {
        return Err(ProtoError::BufferTooShort {
            got: page7.len(),
            need: PAYLOAD_LEN,
        });
    }

    let raw_version = u16::from_be_bytes([page7[0], page7[1]]);
    let version = match raw_version {
        0x0000 => Page7Version::V0,
        0x0100 => Page7Version::V1,
        other => return Err(ProtoError::UnknownPage7Version(other)),
    };

    let modes = page7[17];
    let power_modes = PowerModeSupport {
        quiet: modes & 0b0000_0001 != 0,
        power_saving: modes & 0b0000_0010 != 0,
        performance: modes & 0b0000_0100 != 0,
        entertainment: modes & 0b0000_1000 != 0,
    };
    let power_mode_ui_id = modes >> 4;

    let (turbo_fan, ms_hybrid_switch, nvidia_power_off, slow_fan) = match version {
        Page7Version::V0 => (false, false, false, false),
        Page7Version::V1 => (
            page7[16] & 0b0001_0000 != 0,
            page7[18] & 0b0000_0001 != 0,
            page7[19] & 0b0100_0000 != 0,
            page7[20] & 0b0000_0001 != 0,
        ),
    };

    Ok(Capabilities {
        version,
        power_modes,
        power_mode_ui_id,
        turbo_fan,
        ms_hybrid_switch,
        nvidia_power_off,
        slow_fan,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(version: [u8; 2]) -> [u8; PAYLOAD_LEN] {
        let mut p = [0u8; PAYLOAD_LEN];
        p[0] = version[0];
        p[1] = version[1];
        p
    }

    #[test]
    fn parses_v0_power_modes() {
        let mut p = base([0x00, 0x00]);
        p[17] = 0b0101; // quiet + performance
        let caps = parse_capabilities(&p).unwrap();
        assert_eq!(caps.version, Page7Version::V0);
        assert!(caps.power_modes.quiet);
        assert!(!caps.power_modes.power_saving);
        assert!(caps.power_modes.performance);
        assert!(!caps.power_modes.entertainment);
        assert!(caps.power_modes.supports(0));
        assert!(caps.power_modes.supports(2));
        assert!(!caps.power_modes.supports(1));
        assert!(!caps.power_modes.supports(9));
    }

    #[test]
    fn v0_ignores_v1_feature_bytes() {
        let mut p = base([0x00, 0x00]);
        p[16] = 0b0001_0000;
        p[18] = 0b0000_0001;
        p[19] = 0b0100_0000;
        p[20] = 0b0000_0001;
        let caps = parse_capabilities(&p).unwrap();
        assert!(!caps.turbo_fan);
        assert!(!caps.ms_hybrid_switch);
        assert!(!caps.nvidia_power_off);
        assert!(!caps.slow_fan);
    }

    #[test]
    fn parses_v1_feature_flags() {
        let mut p = base([0x01, 0x00]);
        p[16] = 0b0001_0000;
        p[17] = 0b1111;
        p[18] = 0b0000_0001;
        p[19] = 0b0100_0000;
        p[20] = 0b0000_0001;
        let caps = parse_capabilities(&p).unwrap();
        assert_eq!(caps.version, Page7Version::V1);
        assert!(caps.turbo_fan);
        assert!(caps.ms_hybrid_switch);
        assert!(caps.nvidia_power_off);
        assert!(caps.slow_fan);
        assert!(caps.power_modes.quiet);
        assert!(caps.power_modes.entertainment);
    }

    #[test]
    fn v1_requires_exact_bits() {
        let mut p = base([0x01, 0x00]);
        p[16] = 0b0000_1000; // bit3, not bit4
        p[18] = 0b0000_0010; // bit1, not bit0
        p[19] = 0b0010_0000; // bit5, not bit6
        p[20] = 0b0000_0010; // bit1, not bit0
        let caps = parse_capabilities(&p).unwrap();
        assert!(!caps.turbo_fan);
        assert!(!caps.ms_hybrid_switch);
        assert!(!caps.nvidia_power_off);
        assert!(!caps.slow_fan);
    }

    #[test]
    fn power_mode_ui_id_is_high_nibble() {
        let mut p = base([0x00, 0x00]);
        p[17] = 0b1010_0001;
        let caps = parse_capabilities(&p).unwrap();
        assert_eq!(caps.power_mode_ui_id, 0b1010);
    }

    #[test]
    fn unknown_version_is_reported() {
        let p = base([0x02, 0x00]);
        assert_eq!(
            parse_capabilities(&p),
            Err(ProtoError::UnknownPage7Version(0x0200))
        );
    }

    #[test]
    fn short_page_is_rejected() {
        assert_eq!(
            parse_capabilities(&[0u8; 4]).unwrap_err(),
            ProtoError::BufferTooShort {
                got: 4,
                need: PAYLOAD_LEN
            }
        );
    }
}
