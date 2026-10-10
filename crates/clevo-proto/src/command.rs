//! Strongly typed DCHU command numbers.
//!
//! Only the first-phase commands are defined. Using a newtype prevents raw
//! `u32`s from being passed around and accidentally misinterpreted.

use core::fmt;

/// A DCHU command number (the `command` argument of the Windows API).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Command(pub u32);

/// Read the fan status / duty / temperature package (256 bytes).
pub const CMD_FAN_STATUS: Command = Command(12);

/// Read the fan curve table and machine capabilities (256 bytes).
pub const CMD_FAN_CURVE_READ: Command = Command(13);

/// Write the fan curve table (256 bytes).
pub const CMD_FAN_CURVE_WRITE: Command = Command(14);

/// The main multi-purpose command family; semantics depend on the sub-command.
pub const CMD_MAIN: Command = Command(121);

/// `CMD_MAIN` sub-command selecting the fan mode (`0` auto, `8` quiet).
pub const SUB_FAN_MODE: u8 = 1;

/// `CMD_MAIN` sub-command selecting the power/situational mode (`0..=3`).
pub const SUB_POWER_MODE: u8 = 25;

/// Read PSF2 feature flags through GCMD.
pub const CMD_FEATURES: Command = Command(122);
/// Read PSF4 flags, including the performance-mode gate (bit 2).
pub const CMD_EXTENDED_FEATURES: Command = Command(96);
/// Read the supported application bitmap (PSF5).
pub const CMD_SUPPORTED_APPS: Command = Command(16);
/// Read the GPU power status through GCMD (not a capability bitmap).
pub const CMD_GPU_POWER_STATUS: Command = Command(60);
/// Read PSF1 platform flags, including the default power-conservation mode.
pub const CMD_PLATFORM_FEATURES: Command = Command(82);

/// Read-only queries used when the AppSettings channel is unavailable.
pub const GCMD_CAPABILITY_COMMANDS: [Command; 5] = [
    CMD_FEATURES,
    CMD_EXTENDED_FEATURES,
    CMD_SUPPORTED_APPS,
    CMD_GPU_POWER_STATUS,
    CMD_PLATFORM_FEATURES,
];

impl Command {
    /// Returns the raw numeric command value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<Command> for u32 {
    fn from(value: Command) -> Self {
        value.0
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
