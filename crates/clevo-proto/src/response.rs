//! Parsing of DCHU response messages.
//!
//! A response is [`RSP_LEN`] (`0x40C`) bytes. Per
//! `ControlCenter-RE/docs/02-DCHU-WMI协议参考.md` §2.2:
//!
//! ```text
//! 0x008   4   record count n (LE u32)
//! 0x00C ...   records, each: u16 tag; u16 length; u8 data[length]
//! ```
//!
//! `GetDCHU_Data_Integer` returns the `u32` of the first record with `tag == 0`;
//! `GetDCHU_Data_Buffer` returns that record's data verbatim. Parsing is strict:
//! truncation, overrun and missing records are reported rather than guessed at.

use crate::constants::{RECORD_TAG_RESULT, RSP_LEN, RSP_OFF_RECORDS, RSP_OFF_RECORD_COUNT};
use crate::error::ProtoError;

/// A single `(tag, data)` record inside a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record<'a> {
    /// Record tag; the result record uses [`RECORD_TAG_RESULT`].
    pub tag: u16,
    /// Record payload bytes.
    pub data: &'a [u8],
}

/// Parse a response buffer into its records.
///
/// The buffer must contain at least the bytes up to [`RSP_OFF_RECORDS`]; extra
/// trailing bytes beyond the declared records are ignored (a real 0x40C buffer
/// carries zero padding).
pub fn parse_response(buf: &[u8]) -> Result<Vec<Record<'_>>, ProtoError> {
    if buf.len() < RSP_OFF_RECORDS {
        return Err(ProtoError::BufferTooShort {
            got: buf.len(),
            need: RSP_OFF_RECORDS,
        });
    }

    let count = read_le_u32(&buf[RSP_OFF_RECORD_COUNT..RSP_OFF_RECORDS]) as usize;

    let mut records = Vec::with_capacity(count.min(16));
    let mut offset = RSP_OFF_RECORDS;
    for _ in 0..count {
        let header_end = offset + 4;
        if header_end > buf.len() {
            return Err(ProtoError::RecordOverrun { offset, length: 4 });
        }

        let tag = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
        let length = u16::from_le_bytes([buf[offset + 2], buf[offset + 3]]) as usize;
        let data_start = header_end;
        let data_end = data_start + length;

        if data_end > buf.len() {
            return Err(ProtoError::RecordOverrun { offset, length });
        }

        records.push(Record {
            tag,
            data: &buf[data_start..data_end],
        });
        offset = data_end;
    }

    Ok(records)
}

/// Parse a full-size (`RSP_LEN`) response buffer.
pub fn parse_full_response(buf: &[u8]) -> Result<Vec<Record<'_>>, ProtoError> {
    if buf.len() < RSP_LEN {
        return Err(ProtoError::BufferTooShort {
            got: buf.len(),
            need: RSP_LEN,
        });
    }
    parse_response(buf)
}

/// Return the data of the first record with `tag == 0`.
pub fn result_record(buf: &[u8]) -> Result<&[u8], ProtoError> {
    parse_response(buf)?
        .into_iter()
        .find(|record| record.tag == RECORD_TAG_RESULT)
        .map(|record| record.data)
        .ok_or(ProtoError::NoResultRecord)
}

/// `GetDCHU_Data_Integer` semantics: the little-endian `u32` of the result record.
pub fn response_integer(buf: &[u8]) -> Result<u32, ProtoError> {
    let data = result_record(buf)?;
    if data.len() < 4 {
        return Err(ProtoError::ResultRecordTooShort {
            got: data.len(),
            need: 4,
        });
    }
    Ok(read_le_u32(&data[..4]))
}

/// `GetDCHU_Data_Buffer` semantics: the raw bytes of the result record.
pub fn response_first_record(buf: &[u8]) -> Result<&[u8], ProtoError> {
    result_record(buf)
}

fn read_le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Firmware status meaning "this command is not supported on this machine".
pub const DSM_NOT_SUPPORTED: u32 = 0x8000_0002;

/// Value command `14` (fan-curve write) returns on success.
///
/// Documented in `docs/hardware-notes.md` §7 and confirmed live: the EC accepts
/// the curve and answers `0x14` (20), *not* the command number the `SCMD`
/// family uses.
pub const DSM_CURVE_WRITE_OK: u32 = 0x14;

/// Whether `value` is a success status for `command`.
///
/// Success is family-dependent, both verified live:
///
/// | family | success value |
/// |---|---|
/// | `SCMD`/`GCMD` (e.g. `121`) | the command number itself |
/// | fan-curve write (`14`) | `0x14` (20) |
/// | any | [`DSM_NOT_SUPPORTED`] means unsupported |
///
/// Treating only `value == command` as success made a working curve write look
/// like a failure: the EC accepted the curve and returned 20.
pub fn is_success_status(command: u32, value: u32) -> bool {
    value == command || (command == 14 && value == DSM_CURVE_WRITE_OK)
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn curve_write_accepts_20() {
        // Verified live: command 14 answers 0x14.
        assert!(is_success_status(14, 0x14));
        assert!(is_success_status(14, 14));
    }

    #[test]
    fn main_command_accepts_its_own_number() {
        assert!(is_success_status(121, 121));
        // 20 is not success for 121.
        assert!(!is_success_status(121, 20));
    }

    #[test]
    fn unsupported_is_never_success() {
        assert!(!is_success_status(14, DSM_NOT_SUPPORTED));
        assert!(!is_success_status(121, DSM_NOT_SUPPORTED));
    }

    #[test]
    fn other_values_are_not_success() {
        assert!(!is_success_status(14, 0));
        assert!(!is_success_status(14, 21));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(count: u32) -> Vec<u8> {
        let mut buf = vec![0u8; RSP_OFF_RECORDS];
        buf[RSP_OFF_RECORD_COUNT..RSP_OFF_RECORDS].copy_from_slice(&count.to_le_bytes());
        buf
    }

    fn push_record(buf: &mut Vec<u8>, tag: u16, data: &[u8]) {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&(data.len() as u16).to_le_bytes());
        buf.extend_from_slice(data);
    }

    #[test]
    fn parses_single_result_record() {
        let mut buf = header(1);
        push_record(&mut buf, 0, &[0x2A, 0x00, 0x00, 0x00]);
        buf.resize(RSP_LEN, 0);

        let records = parse_response(&buf).expect("well-formed buffer");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tag, 0);
        assert_eq!(records[0].data, &[0x2A, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn parses_multiple_records_in_order() {
        let mut buf = header(2);
        push_record(&mut buf, 1, &[0xAA]);
        push_record(&mut buf, 0, &[0x01, 0x02]);
        buf.resize(RSP_LEN, 0);

        let records = parse_response(&buf).expect("well-formed buffer");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tag, 1);
        assert_eq!(records[1].tag, 0);
        assert_eq!(records[1].data, &[0x01, 0x02]);
    }

    #[test]
    fn response_integer_reads_first_result_record() {
        let mut buf = header(1);
        push_record(&mut buf, 0, &0x1234_5678u32.to_le_bytes());
        buf.resize(RSP_LEN, 0);

        assert_eq!(response_integer(&buf).unwrap(), 0x1234_5678);
    }

    #[test]
    fn response_integer_ignores_non_result_records() {
        let mut buf = header(2);
        push_record(&mut buf, 9, &[0xFF, 0xFF, 0xFF, 0xFF]);
        push_record(&mut buf, 0, &5u32.to_le_bytes());
        buf.resize(RSP_LEN, 0);

        assert_eq!(response_integer(&buf).unwrap(), 5);
    }

    #[test]
    fn zero_records_reports_no_result() {
        let buf = header(0);
        assert_eq!(parse_response(&buf).unwrap(), Vec::new());
        assert_eq!(response_integer(&buf), Err(ProtoError::NoResultRecord));
    }

    #[test]
    fn missing_result_record_is_reported() {
        let mut buf = header(1);
        push_record(&mut buf, 1, &[0u8; 4]);
        buf.resize(RSP_LEN, 0);

        assert_eq!(response_integer(&buf), Err(ProtoError::NoResultRecord));
    }

    #[test]
    fn short_buffer_is_rejected() {
        let err = parse_response(&[0u8; 4]).unwrap_err();
        assert_eq!(
            err,
            ProtoError::BufferTooShort {
                got: 4,
                need: RSP_OFF_RECORDS
            }
        );
    }

    #[test]
    fn truncated_record_data_is_rejected() {
        let mut buf = header(1);
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&100u16.to_le_bytes());
        buf.extend_from_slice(&[0u8; 10]);

        let err = parse_response(&buf).unwrap_err();
        assert_eq!(
            err,
            ProtoError::RecordOverrun {
                offset: RSP_OFF_RECORDS,
                length: 100
            }
        );
    }

    #[test]
    fn declared_count_exceeding_buffer_is_reported() {
        let mut buf = header(5);
        push_record(&mut buf, 0, &[0u8; 4]);
        push_record(&mut buf, 0, &[0u8; 4]);
        buf.resize(RSP_OFF_RECORDS + 16, 0);

        let err = parse_response(&buf).unwrap_err();
        assert_eq!(
            err,
            ProtoError::RecordOverrun {
                offset: RSP_OFF_RECORDS + 8 + 8,
                length: 4
            }
        );
    }

    #[test]
    fn result_record_shorter_than_u32_is_rejected() {
        let mut buf = header(1);
        push_record(&mut buf, 0, &[0x01, 0x02]);
        buf.resize(RSP_LEN, 0);

        assert_eq!(
            response_integer(&buf),
            Err(ProtoError::ResultRecordTooShort { got: 2, need: 4 })
        );
    }

    #[test]
    fn full_response_requires_full_length() {
        let mut buf = header(1);
        push_record(&mut buf, 0, &[0u8; 4]);
        assert!(buf.len() < RSP_LEN);

        assert_eq!(
            parse_full_response(&buf).unwrap_err(),
            ProtoError::BufferTooShort {
                got: buf.len(),
                need: RSP_LEN
            }
        );
    }

    #[test]
    fn response_first_record_returns_raw_bytes() {
        let mut buf = header(1);
        push_record(&mut buf, 0, &[1, 2, 3, 4, 5, 6]);
        buf.resize(RSP_LEN, 0);

        assert_eq!(response_first_record(&buf).unwrap(), &[1, 2, 3, 4, 5, 6]);
    }
}
