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
