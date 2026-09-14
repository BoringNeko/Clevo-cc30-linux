//! Scriptable CLI for the Clevo control center.
//!
//! Safety rules enforced here:
//!
//! * The default transport is the offline [`MockTransport`]. The `acpi-call`
//!   backend is not implemented in phase 1 and is rejected with a clear error.
//! * Every write command is a **dry run** unless `--apply` is passed; even then
//!   the transport's own verification gate (e.g. `allow_write`) still applies.
//! * Commands never touch ACPI/EC/hidraw/sysfs directly — all hardware access
//!   goes through [`Transport`].
//!
//! The CLI is structured as a library (`run`) plus a thin `main`, so command
//! dispatch can be tested against a mock transport without spawning a process.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod commands;
pub mod dbus;
pub mod snapshot;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use clevo_transport::{MockTransport, Transport};

/// Top-level CLI definition.
#[derive(Debug, Parser)]
#[command(
    name = "clevo-cc",
    version,
    about = "Clevo control center (read-only by default)"
)]
pub struct Cli {
    /// Which transport backend to use.
    #[arg(long, value_enum, default_value_t = TransportChoice::Mock, global = true)]
    pub transport: TransportChoice,

    /// Fixture file used by the mock transport.
    #[arg(long, global = true)]
    pub fixture: Option<PathBuf>,

    /// Talk to `clevod` on the per-user session bus instead of the system bus.
    ///
    /// Use this together with a daemon started with `--session-bus` (for
    /// offline/testing): `dbus-run-session -- clevo-cc --transport dbus
    /// --dbus-session fan status`.
    #[arg(long, global = true)]
    pub dbus_session: bool,

    /// The sub-command to run.
    #[command(subcommand)]
    pub command: Commands,
}

/// Selectable transport backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TransportChoice {
    /// Replay recorded fixtures; never touches hardware.
    Mock,
    /// Linux `/proc/acpi/call` (read-only: commands 12 and 13 only).
    AcpiCall,
    /// Kernel `clevo-cc` driver sysfs/hwmon (read/write; no `acpi_call`).
    Driver,
    /// Talk to the `clevod` daemon over D-Bus (no direct hardware access).
    Dbus,
}

/// Available sub-commands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Check OS/library state without changing anything.
    Doctor,
    /// Print the machine's advertised capabilities (page 7).
    Capabilities,
    /// Fan-related operations.
    #[command(subcommand)]
    Fan(FanCommand),
    /// Performance-profile operations.
    #[command(subcommand)]
    Profile(ProfileCommand),
}

/// Fan sub-commands.
#[derive(Debug, Subcommand)]
pub enum FanCommand {
    /// Print current fan speeds, duty and raw temperatures.
    Status,
    /// Print the configured fan curve.
    Curve,
    /// Continuously refresh the fan status until interrupted.
    Watch {
        /// Refresh interval in milliseconds.
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
        /// Number of samples to print, then exit (`0` = run until interrupted).
        #[arg(long, default_value_t = 0)]
        count: u64,
        /// Print one sample as machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Set the fan mode (`121/1`).
    SetMode {
        /// `auto` or `quiet`.
        mode: String,
        /// Actually perform the write (otherwise dry run).
        #[arg(long)]
        apply: bool,
    },
}

/// Performance-profile sub-commands.
#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// List available performance modes.
    List,
    /// Set the performance mode (`121/25`).
    Set {
        /// Numeric mode value (`0..=3`), validated against capabilities.
        value: u8,
        /// Actually perform the write (otherwise dry run).
        #[arg(long)]
        apply: bool,
    },
}

/// Errors surfaced by the CLI.
#[derive(Debug)]
pub enum CliError {
    /// Fixture loading or parsing failed.
    Fixture(String),
    /// The requested transport is not available yet.
    TransportUnavailable(String),
    /// An operation failed at the transport layer.
    Transport(clevo_transport::TransportError),
    /// A response could not be decoded.
    Protocol(clevo_proto::ProtoError),
    /// Writing command output failed.
    Io(std::io::Error),
    /// The user asked for something invalid (bad mode, unsupported profile).
    Invalid(String),
    /// The daemon (D-Bus backend) returned an error.
    Dbus(crate::dbus::DbusError),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fixture(msg) => write!(f, "fixture error: {msg}"),
            Self::TransportUnavailable(msg) => write!(f, "transport unavailable: {msg}"),
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Protocol(err) => write!(f, "protocol error: {err}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Invalid(msg) => write!(f, "{msg}"),
            Self::Dbus(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<clevo_transport::TransportError> for CliError {
    fn from(value: clevo_transport::TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<clevo_proto::ProtoError> for CliError {
    fn from(value: clevo_proto::ProtoError) -> Self {
        Self::Protocol(value)
    }
}

impl From<std::io::Error> for CliError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<crate::dbus::DbusError> for CliError {
    fn from(value: crate::dbus::DbusError) -> Self {
        Self::Dbus(value)
    }
}

/// Build the transport selected by `cli`, or fail for unimplemented backends.
pub fn build_transport(cli: &Cli) -> Result<Box<dyn Transport>, CliError> {
    match cli.transport {
        TransportChoice::Mock => {
            let text = match &cli.fixture {
                Some(path) => std::fs::read_to_string(path)
                    .map_err(|e| CliError::Fixture(format!("{}: {e}", path.display())))?,
                None => String::new(),
            };
            let mock = MockTransport::from_fixture_str(&text).map_err(|e| {
                CliError::Fixture(format!(
                    "{}: {e}",
                    cli.fixture
                        .as_ref()
                        .map_or("<empty>", |p| { p.to_str().unwrap_or("<fixture>") })
                ))
            })?;
            Ok(Box::new(mock))
        }
        TransportChoice::AcpiCall => Ok(Box::new(clevo_transport::AcpiCallTransport::new())),
        TransportChoice::Driver => Ok(Box::new(clevo_transport::DriverTransport::new())),
        TransportChoice::Dbus => Err(CliError::TransportUnavailable(
            "the dbus backend is handled separately from raw transports".into(),
        )),
    }
}
