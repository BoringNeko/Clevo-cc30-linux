//! Errors produced by a [`crate::Transport`].
//!
//! These are deliberately distinct from [`clevo_proto::ProtoError`]: a
//! protocol error means the bytes were malformed, while a transport error means
//! the request could not be delivered or was refused by policy/firmware.

use core::fmt;

/// Result type used throughout the transport layer.
pub type TransportResult<T> = Result<T, TransportError>;

/// Failure modes of a hardware transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The capability is not supported by this machine or transport.
    Unsupported(String),

    /// The caller lacks permission for this operation.
    PermissionDenied,

    /// The operation exceeded its time budget.
    Timeout,

    /// An underlying I/O error occurred.
    Io(String),

    /// The operation requires verified ACPI layout / firmware ABI that has not
    /// been confirmed on real hardware. Writes must fail with this rather than
    /// silently succeeding.
    NotVerified(String),

    /// The firmware rejected the command with a status code.
    FirmwareRejected(i32),

    /// The response could not be parsed or had an unexpected shape.
    MalformedResponse(String),

    /// No recorded response matched the request in a replay transport.
    NoFixtureMatch(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::PermissionDenied => write!(f, "permission denied"),
            Self::Timeout => write!(f, "operation timed out"),
            Self::Io(msg) => write!(f, "i/o error: {msg}"),
            Self::NotVerified(what) => write!(f, "not verified on real hardware: {what}"),
            Self::FirmwareRejected(code) => write!(f, "firmware rejected command (status {code})"),
            Self::MalformedResponse(msg) => write!(f, "malformed response: {msg}"),
            Self::NoFixtureMatch(key) => write!(f, "no recorded response for {key}"),
        }
    }
}

impl std::error::Error for TransportError {}
