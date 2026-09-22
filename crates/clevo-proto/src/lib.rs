//! Pure protocol layer for the Clevo DCHU / ACPI `_DSM` hardware channel.
//!
//! This crate contains **no I/O**: it only builds and parses the byte-level
//! messages that the Windows `InsydeDCHU.dll` + `AcpiBridge.sys` stack used to
//! exchange with the firmware. All constants are defined exactly once in
//! [`constants`]; no other module may copy magic numbers.
//!
//! Scope of the first phase: fan status (`12`), fan curve read (`13`), fan
//! curve write (`14`), fan mode (`121/1`) and power mode (`121/25`).
//!
//! The caller is responsible for actually delivering the built request through
//! a transport and for all safety/permission policy. This crate is deliberately
//! side-effect free so it can be unit tested without any hardware.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod capability;
pub mod command;
pub mod constants;
pub mod error;
pub mod fan_curve;
pub mod fan_status;
pub mod message;
pub mod response;

pub use capability::{Capabilities, Page7Version, PowerModeSupport};
pub use command::Command;
pub use error::ProtoError;
pub use fan_curve::{FanCurve, FanCurveInfo, FanPoint};
pub use fan_status::{FanStatus, TdpClass};
