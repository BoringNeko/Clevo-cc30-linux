//! Error type for pure protocol failures.

use core::fmt;

/// Errors produced while building or parsing DCHU messages.
///
/// The variants are intentionally specific so callers (and tests) can tell a
/// truncated buffer from a malformed record from a semantic violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    /// A supplied buffer was shorter than the protocol requires.
    BufferTooShort {
        /// Actual length seen.
        got: usize,
        /// Minimum length required.
        need: usize,
    },

    /// A record's declared length runs past the end of the response buffer.
    RecordOverrun {
        /// Byte offset at which the offending record started.
        offset: usize,
        /// Declared record length.
        length: usize,
    },

    /// No record with `tag == 0` was present in the response.
    NoResultRecord,

    /// The result record was too short to read the requested scalar.
    ResultRecordTooShort {
        /// Length of the result record data.
        got: usize,
        /// Number of bytes required.
        need: usize,
    },

    /// A payload's length did not match the fixed protocol length.
    InvalidPayloadLength {
        /// Actual length seen.
        got: usize,
        /// Expected length.
        expected: usize,
    },

    /// A fan curve was semantically invalid.
    InvalidCurve {
        /// Which fan curve (or `slope`) was rejected.
        fan: String,
        /// Human-readable reason.
        reason: String,
    },

    /// `page 7` declared a feature-table version this crate does not know.
    UnknownPage7Version(u16),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort { got, need } => {
                write!(f, "buffer too short: got {got} bytes, need at least {need}")
            }
            Self::RecordOverrun { offset, length } => write!(
                f,
                "record at offset {offset} declares length {length} which overruns the buffer"
            ),
            Self::NoResultRecord => write!(f, "response contains no tag==0 result record"),
            Self::ResultRecordTooShort { got, need } => write!(
                f,
                "result record is too short: got {got} bytes, need {need}"
            ),
            Self::InvalidPayloadLength { got, expected } => {
                write!(f, "payload length must be {expected}, got {got}")
            }
            Self::InvalidCurve { fan, reason } => {
                write!(f, "invalid {fan} fan curve: {reason}")
            }
            Self::UnknownPage7Version(v) => {
                write!(f, "unknown page 7 feature-table version 0x{v:04X}")
            }
        }
    }
}

impl std::error::Error for ProtoError {}
