//! The [`Transport`] abstraction.
//!
//! A transport is the *only* component allowed to talk to the hardware. UI and
//! CLI code must go through this trait (directly for now, via `clevod` later).
//!
//! Implementations must never invent success: unsupported capabilities,
//! unverified firmware ABIs and firmware rejections are reported as structured
//! [`TransportError`] values.

use crate::error::TransportResult;

pub mod acpi_call;
pub mod driver;
pub mod error;
pub mod mock;

pub use acpi_call::AcpiCallTransport;
pub use driver::DriverTransport;
pub use error::TransportError;
pub use mock::{Fixture, FixtureEntry, FixtureMeta, MockTransport};

/// Which backend a transport is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Replay from recorded fixtures; never touches hardware.
    Mock,
    /// Linux `/proc/acpi/call` backend (read-only).
    AcpiCall,
    /// The in-tree `clevo-cc` kernel driver's sysfs/hwmon interface (read/write).
    Driver,
}

/// The command payload size, re-exported for callers.
pub const PAYLOAD_BYTES: usize = clevo_proto::constants::PAYLOAD_LEN;

/// A channel that can execute DCHU commands and read/write app settings.
///
/// Implementations must be `Send + Sync` so a single transport can be shared by
/// a multi-threaded daemon (`clevod`) and polled from async tasks.
pub trait Transport: Send + Sync {
    /// Execute a DCHU command with a 256-byte payload.
    ///
    /// Returns the raw response bytes (nominally `RSP_LEN` long) for the caller
    /// to parse with `clevo-proto`.
    fn execute(&self, command: u32, payload: &[u8; PAYLOAD_BYTES]) -> TransportResult<Vec<u8>>;

    /// Read `len` bytes from AppSettings `page` at `offset`.
    fn read_app_settings(&self, page: u8, offset: u16, len: u16) -> TransportResult<Vec<u8>>;

    /// Write `data` to AppSettings `page` at `offset`.
    fn write_app_settings(&self, page: u8, offset: u16, data: &[u8]) -> TransportResult<()>;

    /// Identify the backend.
    fn kind(&self) -> TransportKind;

    /// Whether this transport can perform write operations at all.
    ///
    /// Callers should surface this to the user rather than letting a write fail
    /// unexpectedly.
    fn writable(&self) -> bool {
        false
    }

    /// Read the fan and performance modes currently in effect, when the backend
    /// can report them.
    ///
    /// Returns `(fan_mode, perf_mode)` as `121/1` and `121/25` values. A
    /// transport that cannot observe the current mode returns `(None, None)`,
    /// which is honest: the firmware does not always expose it, and callers must
    /// not guess.
    fn current_modes(&self) -> TransportResult<(Option<u8>, Option<u8>)> {
        Ok((None, None))
    }
}
