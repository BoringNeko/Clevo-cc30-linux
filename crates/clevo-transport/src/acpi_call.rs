//! Linux `/proc/acpi/call` transport — **read-only phase**.
//!
//! This backend talks to the firmware's `_DSM` method through the `acpi_call`
//! kernel module. It is deliberately restricted:
//!
//! * Only the **verified read commands** may be issued (command `12` fan status
//!   and command `13` fan curve). Every other command is rejected with
//!   [`TransportError::Unsupported`], so this backend can never write hardware
//!   even if a caller asks it to.
//! * AppSettings reads/writes are not implemented (no verified accessor yet).
//! * The ACPI path and GUID are taken from `clevo-proto` constants, which were
//!   verified against the machine's raw AML.
//!
//! The wire format of `acpi_call`: write a command string to the file, then
//! read back a textual representation such as
//! `{0x00,0x01,...}` (buffer), `0x80000002` (integer) or `Error: ...`.
//! `acpi_call`'s default `BUFFER_SIZE` is 256, so the input is capped near 512
//! characters; fan payloads here are all-zero and short.

use clevo_proto::constants::{ACPI_DSM_PATH, DSM_GUID};

use crate::error::{TransportError, TransportResult};
use crate::{Transport, TransportKind, PAYLOAD_BYTES};

/// Default location of the `acpi_call` procfs entry.
pub const DEFAULT_CALL_PATH: &str = "/proc/acpi/call";

/// DCHU commands this backend is allowed to issue. All are read-only.
pub const READ_ONLY_COMMANDS: &[u32] = &[12, 13];

/// A read-only transport backed by the `acpi_call` kernel module.
#[derive(Debug, Clone)]
pub struct AcpiCallTransport {
    path: String,
    io: CallIoKind,
}

/// How the transport talks to `acpi_call`; separated so tests can inject a fake
/// procfs without touching the real one.
#[derive(Debug, Clone)]
enum CallIoKind {
    Procfs,
    /// Test hook: a single reply is returned for every call.
    Fixed(String),
}

impl Default for AcpiCallTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpiCallTransport {
    /// Use the default `/proc/acpi/call` path.
    pub fn new() -> Self {
        Self {
            path: DEFAULT_CALL_PATH.to_string(),
            io: CallIoKind::Procfs,
        }
    }

    /// Use a custom path (useful for non-default mounts).
    pub fn with_path(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            io: CallIoKind::Procfs,
        }
    }

    /// Test constructor: always returns `reply` instead of touching a file.
    pub fn with_fixed_reply(reply: impl Into<String>) -> Self {
        Self {
            path: DEFAULT_CALL_PATH.to_string(),
            io: CallIoKind::Fixed(reply.into()),
        }
    }

    /// The configured call path.
    pub fn path(&self) -> &str {
        &self.path
    }

    fn ensure_read_only(command: u32) -> TransportResult<()> {
        if READ_ONLY_COMMANDS.contains(&command) {
            Ok(())
        } else {
            Err(TransportError::Unsupported(format!(
                "acpi-call backend is read-only; command {command} is not in the \
                 verified allowlist {READ_ONLY_COMMANDS:?}"
            )))
        }
    }
}

impl Transport for AcpiCallTransport {
    fn execute(&self, command: u32, _payload: &[u8; PAYLOAD_BYTES]) -> TransportResult<Vec<u8>> {
        Self::ensure_read_only(command)?;

        let call = build_read_call_string(command).ok_or_else(|| {
            TransportError::Io(format!(
                "call string for command {command} exceeds acpi_call's {MAX_CALL_LEN}-char limit"
            ))
        })?;
        let reply = self.run(&call)?;
        let payload = parse_reply(&reply)?;
        Ok(wrap_as_dchu_response(&payload))
    }

    fn read_app_settings(&self, page: u8, offset: u16, len: u16) -> TransportResult<Vec<u8>> {
        Err(TransportError::Unsupported(format!(
            "AppSettings read (page {page} offset {offset} len {len}) has no verified \
             accessor on this backend"
        )))
    }

    fn write_app_settings(&self, _page: u8, _offset: u16, _data: &[u8]) -> TransportResult<()> {
        Err(TransportError::Unsupported(
            "acpi-call backend is read-only".to_string(),
        ))
    }

    fn kind(&self) -> TransportKind {
        TransportKind::AcpiCall
    }

    fn writable(&self) -> bool {
        false
    }
}

impl AcpiCallTransport {
    fn run(&self, call: &str) -> TransportResult<String> {
        match &self.io {
            CallIoKind::Fixed(reply) => Ok(reply.clone()),
            CallIoKind::Procfs => self.run_procfs(call),
        }
    }

    fn run_procfs(&self, call: &str) -> TransportResult<String> {
        use std::io::{Read, Write};

        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&self.path)
                .map_err(|e| match e.kind() {
                    std::io::ErrorKind::NotFound => TransportError::Unsupported(format!(
                        "{} not found; is the acpi_call module loaded?",
                        self.path
                    )),
                    std::io::ErrorKind::PermissionDenied => TransportError::PermissionDenied,
                    _ => TransportError::Io(format!("open {}: {e}", self.path)),
                })?;

            file.write_all(call.as_bytes())
                .map_err(|e| match e.kind() {
                    std::io::ErrorKind::PermissionDenied => TransportError::PermissionDenied,
                    _ => TransportError::Io(format!("write {}: {e}", self.path)),
                })?;
        }

        // acpi_call serves its result on the first read() and then resets the
        // buffer, so we must issue a single read() (not read_to_end /
        // read_to_string, which loop and see EOF immediately on this procfs
        // file).
        let mut file = std::fs::File::open(&self.path).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => TransportError::PermissionDenied,
            _ => TransportError::Io(format!("open {} for read: {e}", self.path)),
        })?;
        let mut buf = [0u8; 4096];
        let n = file
            .read(&mut buf)
            .map_err(|e| TransportError::Io(format!("read {}: {e}", self.path)))?;
        // Drop a trailing NUL that acpi_call includes, then trim whitespace.
        let text = String::from_utf8_lossy(&buf[..n]);
        Ok(text.trim_matches('\0').trim().to_string())
    }
}

/// Build the `acpi_call` command string for a read command.
///
/// Format: `<path> b<guid-hex> 0 <command> b<payload-hex>`. `acpi_call` accepts
/// `b<hex>` as an ACPI_BUFFER argument; the read path in the firmware tolerates
/// a bare buffer for Arg3 (verified live; see `docs/hardware-notes.md` §10).
///
/// `acpi_call` is built with `BUFFER_SIZE = 256`, so a single write is capped at
/// 511 characters. A full 256-byte payload (512 hex chars) therefore cannot be
/// sent. The read commands ignore the payload entirely, so only a short payload
/// is emitted. Returns an error if the command needs more than the driver limit
/// (which cannot happen for the current read allowlist).
pub fn build_call_string(command: u32, payload: &[u8; PAYLOAD_BYTES]) -> String {
    let guid = hex(DSM_GUID.as_slice());
    let payload_hex = hex(payload.as_slice());
    format!("{ACPI_DSM_PATH} b{guid} 0 {command} b{payload_hex}")
}

/// The maximum number of characters `acpi_call` accepts in one write.
pub const MAX_CALL_LEN: usize = 511;

/// Build a minimal read call string with a one-byte payload.
///
/// The read path ignores Arg3, and one byte keeps the request comfortably under
/// [`MAX_CALL_LEN`]. Returns `None` if even that would be too long.
pub fn build_read_call_string(command: u32) -> Option<String> {
    let guid = hex(DSM_GUID.as_slice());
    let call = format!("{ACPI_DSM_PATH} b{guid} 0 {command} b00");
    (call.len() <= MAX_CALL_LEN).then_some(call)
}

/// Parse an `acpi_call` reply into raw bytes.
///
/// Accepts a buffer reply (`{0x.., 0x..}`). The `acpi_call` result buffer is
/// only 256 bytes, so a long reply can be truncated before the closing `}`;
/// both closed and truncated forms are accepted. Integer and error replies are
/// mapped to structured errors.
pub fn parse_reply(reply: &str) -> TransportResult<Vec<u8>> {
    let trimmed = reply.trim().trim_matches('\0').trim();
    if trimmed.is_empty() {
        return Err(TransportError::MalformedResponse(
            "empty reply from acpi_call".to_string(),
        ));
    }

    if let Some(rest) = trimmed.strip_prefix("Error:") {
        return Err(TransportError::Io(format!(
            "acpi_call method error:{}",
            rest
        )));
    }

    if let Some(body) = trimmed.strip_prefix('{') {
        let inner = body.strip_suffix('}').unwrap_or(body);
        if inner.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut bytes = Vec::new();
        for token in inner.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let token = token
                .strip_prefix("0x")
                .or_else(|| token.strip_prefix("0X"))
                .unwrap_or(token);
            let byte = u8::from_str_radix(token, 16).map_err(|e| {
                TransportError::MalformedResponse(format!("bad byte {token:?} in reply: {e}"))
            })?;
            bytes.push(byte);
        }
        return Ok(bytes);
    }

    // Integer reply (e.g. 0x80000002) — the method did not return a buffer.
    if let Some(hexval) = trimmed.strip_prefix("0x") {
        let value = i64::from_str_radix(hexval, 16).unwrap_or(0);
        return Err(TransportError::FirmwareRejected(value as i32));
    }

    Err(TransportError::MalformedResponse(format!(
        "unrecognized acpi_call reply: {trimmed}"
    )))
}

/// Wrap a raw `_DSM` payload as a DCHU response buffer.
///
/// The Windows stack's `AcpiBridge.sys` turned the raw `_DSM` buffer into a DCHU
/// response with a `tag/len` record table. Calling `_DSM` directly returns the
/// bare payload, so this re-creates the one-record envelope that
/// `clevo-proto::response` expects.
pub fn wrap_as_dchu_response(payload: &[u8]) -> Vec<u8> {
    use clevo_proto::constants::{
        RECORD_TAG_RESULT, RSP_LEN, RSP_OFF_RECORDS, RSP_OFF_RECORD_COUNT,
    };

    // header (record count + record header) + payload, zero-padded to RSP_LEN.
    let needed = RSP_OFF_RECORDS + 4 + payload.len();
    let len = needed.max(RSP_LEN);
    let mut buf = vec![0u8; len];
    buf[RSP_OFF_RECORD_COUNT..RSP_OFF_RECORDS].copy_from_slice(&1u32.to_le_bytes());
    buf[RSP_OFF_RECORDS..RSP_OFF_RECORDS + 2].copy_from_slice(&RECORD_TAG_RESULT.to_le_bytes());
    buf[RSP_OFF_RECORDS + 2..RSP_OFF_RECORDS + 4]
        .copy_from_slice(&(payload.len() as u16).to_le_bytes());
    buf[RSP_OFF_RECORDS + 4..RSP_OFF_RECORDS + 4 + payload.len()].copy_from_slice(payload);
    buf
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_string_uses_verified_guid_and_path() {
        let payload = [0u8; PAYLOAD_BYTES];
        let call = build_call_string(12, &payload);
        assert!(call.starts_with("\\_SB.DCHU._DSM b"));
        assert!(call.contains("e424f293dcfbbf4badd6db71bdc0afad"));
        assert!(call.contains(" 0 12 b"));
        // ends with 256 zero bytes = 512 hex chars
        assert!(call.ends_with(&"00".repeat(PAYLOAD_BYTES)));
    }

    #[test]
    fn full_payload_call_exceeds_the_driver_limit() {
        // Documents why execute() uses a short payload: a full 256-byte payload
        // cannot be written to acpi_call (BUFFER_SIZE=256 -> 511-char cap).
        let call = build_call_string(12, &[0u8; PAYLOAD_BYTES]);
        assert!(call.len() > MAX_CALL_LEN);
    }

    #[test]
    fn short_read_call_fits_the_driver_limit() {
        for command in READ_ONLY_COMMANDS {
            let call = build_read_call_string(*command).expect("fits");
            assert!(call.len() <= MAX_CALL_LEN);
            assert!(call.ends_with(&format!(" 0 {command} b00")));
        }
    }

    #[test]
    fn parses_buffer_reply() {
        let reply = "{0x00, 0x01, 0xC4, 0x1D}";
        assert_eq!(parse_reply(reply).unwrap(), vec![0x00, 0x01, 0xC4, 0x1D]);
    }

    #[test]
    fn parses_truncated_buffer_reply() {
        // acpi_call's 256-byte result buffer can truncate the closing brace.
        let reply = "{0x00, 0x01, 0xC4, 0x1D,";
        assert_eq!(parse_reply(reply).unwrap(), vec![0x00, 0x01, 0xC4, 0x1D]);
    }

    #[test]
    fn parses_reply_with_trailing_nul() {
        let reply = "{0x00, 0x01}\0";
        assert_eq!(parse_reply(reply).unwrap(), vec![0x00, 0x01]);
    }

    #[test]
    fn parses_empty_buffer() {
        assert_eq!(parse_reply("{}").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn integer_reply_is_firmware_rejected() {
        let err = parse_reply("0x80000002").unwrap_err();
        assert_eq!(err, TransportError::FirmwareRejected(0x80000002u32 as i32));
    }

    #[test]
    fn error_reply_is_io_error() {
        let err = parse_reply("Error: AE_AML_BUFFER_LIMIT").unwrap_err();
        assert!(matches!(err, TransportError::Io(_)));
    }

    #[test]
    fn malformed_byte_is_rejected() {
        assert!(matches!(
            parse_reply("{0xZZ}").unwrap_err(),
            TransportError::MalformedResponse(_)
        ));
    }

    #[test]
    fn unrecognized_reply_is_rejected() {
        assert!(matches!(
            parse_reply("not called").unwrap_err(),
            TransportError::MalformedResponse(_)
        ));
    }

    #[test]
    fn execute_is_restricted_to_read_only_commands() {
        let t = AcpiCallTransport::with_path("/nonexistent");
        let payload = [0u8; PAYLOAD_BYTES];
        // 14 is a write command -> rejected before any I/O.
        assert!(matches!(
            t.execute(14, &payload).unwrap_err(),
            TransportError::Unsupported(_)
        ));
        // 121 is the main write family -> rejected.
        assert!(matches!(
            t.execute(121, &payload).unwrap_err(),
            TransportError::Unsupported(_)
        ));
    }

    #[test]
    fn writes_are_always_refused() {
        let t = AcpiCallTransport::new();
        assert!(!t.writable());
        assert!(matches!(
            t.write_app_settings(1, 0, &[1]).unwrap_err(),
            TransportError::Unsupported(_)
        ));
        assert!(matches!(
            t.read_app_settings(7, 0, 256).unwrap_err(),
            TransportError::Unsupported(_)
        ));
    }

    #[test]
    fn kind_is_acpi_call() {
        assert_eq!(AcpiCallTransport::new().kind(), TransportKind::AcpiCall);
    }
}
