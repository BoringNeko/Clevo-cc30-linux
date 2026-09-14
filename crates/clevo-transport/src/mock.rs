//! Replayable mock transport driven by recorded fixtures.
//!
//! The fixture format is a small line-oriented text format, chosen so the crate
//! stays dependency-free and so fixtures are reviewable and diffable in git.
//! See `fixtures/README.md` for the grammar and provenance rules.
//!
//! Safety: a mock never touches hardware. Writes are refused with
//! [`TransportError::NotVerified`] unless the fixture explicitly records
//! `allow_write = true`, so a recording made read-only cannot be used to drive
//! a write by accident.

use std::collections::HashMap;

use crate::error::{TransportError, TransportResult};
use crate::{Transport, TransportKind, PAYLOAD_BYTES};

/// Provenance metadata for a recording.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixtureMeta {
    /// Machine model (e.g. `NH5xAx`).
    pub model: String,
    /// BIOS version.
    pub bios: String,
    /// Recording date (ISO-8601).
    pub date: String,
    /// Whether this recording is permitted to drive write operations.
    pub allow_write: bool,
}

/// A single recorded command/response exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureEntry {
    /// The DCHU command number.
    pub command: u32,
    /// The 256-byte request payload, or `None` for a wildcard match.
    pub payload: Option<[u8; PAYLOAD_BYTES]>,
    /// The recorded response bytes.
    pub response: Vec<u8>,
}

/// A recorded AppSettings page image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureAppSettings {
    /// Page number.
    pub page: u8,
    /// Byte offset within the page.
    pub offset: u16,
    /// Page bytes.
    pub data: Vec<u8>,
}

/// A parsed fixture file.
#[derive(Debug, Clone, Default)]
pub struct Fixture {
    /// Provenance metadata.
    pub meta: FixtureMeta,
    /// Recorded command exchanges, in file order.
    pub entries: Vec<FixtureEntry>,
    /// Recorded AppSettings page images.
    pub app_settings: Vec<FixtureAppSettings>,
}

/// Key used to look up recorded responses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LookupKey {
    command: u32,
    /// FNV-1a digest of the payload; `None` means wildcard.
    payload_digest: Option<u64>,
}

/// A [`Transport`] that replays recorded responses.
#[derive(Debug, Clone)]
pub struct MockTransport {
    meta: FixtureMeta,
    exact: HashMap<LookupKey, Vec<u8>>,
    wildcard: HashMap<u32, Vec<u8>>,
    app_settings: HashMap<(u8, u16), Vec<u8>>,
}

impl MockTransport {
    /// Build a mock from a parsed [`Fixture`].
    pub fn from_fixture(fixture: Fixture) -> Self {
        let mut exact = HashMap::new();
        let mut wildcard = HashMap::new();
        for entry in fixture.entries {
            match entry.payload {
                Some(payload) => {
                    exact.insert(
                        LookupKey {
                            command: entry.command,
                            payload_digest: Some(fnv1a(&payload)),
                        },
                        entry.response,
                    );
                }
                None => {
                    wildcard.insert(entry.command, entry.response);
                }
            }
        }
        let app_settings = fixture
            .app_settings
            .into_iter()
            .map(|entry| ((entry.page, entry.offset), entry.data))
            .collect();
        Self {
            meta: fixture.meta,
            exact,
            wildcard,
            app_settings,
        }
    }

    /// Parse a fixture from text and build a mock.
    pub fn from_fixture_str(text: &str) -> TransportResult<Self> {
        Ok(Self::from_fixture(parse_fixture(text)?))
    }

    /// Recording provenance.
    pub fn meta(&self) -> &FixtureMeta {
        &self.meta
    }

    /// Register a canned AppSettings page for reads.
    pub fn insert_app_settings(&mut self, page: u8, offset: u16, data: Vec<u8>) {
        self.app_settings.insert((page, offset), data);
    }
}

impl Transport for MockTransport {
    fn execute(&self, command: u32, payload: &[u8; PAYLOAD_BYTES]) -> TransportResult<Vec<u8>> {
        if let Some(response) = self.exact.get(&LookupKey {
            command,
            payload_digest: Some(fnv1a(payload)),
        }) {
            return Ok(response.clone());
        }
        if let Some(response) = self.wildcard.get(&command) {
            return Ok(response.clone());
        }
        Err(TransportError::NoFixtureMatch(format!(
            "command {command} payload {}",
            hex(payload)
        )))
    }

    fn read_app_settings(&self, page: u8, offset: u16, len: u16) -> TransportResult<Vec<u8>> {
        let key = (page, offset);
        match self.app_settings.get(&key) {
            Some(data) => {
                if usize::from(len) > data.len() {
                    return Err(TransportError::MalformedResponse(format!(
                        "appsettings page {page} offset {offset}: requested {len} bytes, recorded {}",
                        data.len()
                    )));
                }
                Ok(data[..usize::from(len)].to_vec())
            }
            None => Err(TransportError::NoFixtureMatch(format!(
                "appsettings page {page} offset {offset}"
            ))),
        }
    }

    fn write_app_settings(&self, page: u8, offset: u16, _data: &[u8]) -> TransportResult<()> {
        if !self.meta.allow_write {
            return Err(TransportError::NotVerified(format!(
                "appsettings write page {page} offset {offset}"
            )));
        }
        // Writes are accepted but intentionally not persisted: a replay fixture
        // must remain immutable so repeated test runs are deterministic.
        Err(TransportError::Unsupported(
            "mock transport does not persist writes".to_string(),
        ))
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Mock
    }

    fn writable(&self) -> bool {
        self.meta.allow_write
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Parse the fixture text format.
///
/// Grammar (one directive per line, `#` starts a comment):
/// ```text
/// model = "..."
/// bios = "..."
/// date = "..."
/// allow_write = true|false
/// exec <command-hex> <payload-hex | *> <response-hex>
/// appsettings <page> <offset> <hex-data>
/// ```
pub fn parse_fixture(text: &str) -> TransportResult<Fixture> {
    let mut fixture = Fixture::default();

    for (line_no, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let err = |msg: String| {
            TransportError::MalformedResponse(format!("fixture line {}: {msg}", line_no + 1))
        };

        if let Some(value) = line.strip_prefix("model") {
            fixture.meta.model = parse_string(value).map_err(err)?;
        } else if let Some(value) = line.strip_prefix("bios") {
            fixture.meta.bios = parse_string(value).map_err(err)?;
        } else if let Some(value) = line.strip_prefix("date") {
            fixture.meta.date = parse_string(value).map_err(err)?;
        } else if let Some(value) = line.strip_prefix("allow_write") {
            let value = value.trim_start_matches([' ', '=']).trim();
            fixture.meta.allow_write = match value {
                "true" => true,
                "false" => false,
                other => {
                    return Err(err(format!(
                        "allow_write must be true/false, got {other:?}"
                    )))
                }
            };
        } else if let Some(rest) = line.strip_prefix("exec") {
            fixture.entries.push(parse_exec(rest).map_err(err)?);
        } else if let Some(rest) = line.strip_prefix("appsettings") {
            fixture
                .app_settings
                .push(parse_appsettings(rest).map_err(err)?);
        } else {
            return Err(err(format!("unknown directive: {line:?}")));
        }
    }

    Ok(fixture)
}

fn parse_exec(rest: &str) -> Result<FixtureEntry, String> {
    let mut parts = rest.split_whitespace();
    let command = parts.next().ok_or("exec: missing command")?;
    let payload = parts.next().ok_or("exec: missing payload")?;
    let response = parts.next().ok_or("exec: missing response")?;
    if parts.next().is_some() {
        return Err("exec: too many fields".to_string());
    }

    let command =
        u32::from_str_radix(command, 16).map_err(|e| format!("exec: bad command: {e}"))?;

    let payload = if payload == "*" {
        None
    } else {
        let bytes = parse_hex(payload)?;
        let payload: [u8; PAYLOAD_BYTES] = bytes
            .try_into()
            .map_err(|_| format!("exec: payload must be {PAYLOAD_BYTES} bytes"))?;
        Some(payload)
    };

    Ok(FixtureEntry {
        command,
        payload,
        response: parse_hex(response)?,
    })
}

fn parse_appsettings(rest: &str) -> Result<FixtureAppSettings, String> {
    let mut parts = rest.split_whitespace();
    let page = parts.next().ok_or("appsettings: missing page")?;
    let offset = parts.next().ok_or("appsettings: missing offset")?;
    let data = parts.next().ok_or("appsettings: missing data")?;
    if parts.next().is_some() {
        return Err("appsettings: too many fields".to_string());
    }
    Ok(FixtureAppSettings {
        page: u8::from_str_radix(page, 16).map_err(|e| format!("appsettings: bad page: {e}"))?,
        offset: u16::from_str_radix(offset, 16)
            .map_err(|e| format!("appsettings: bad offset: {e}"))?,
        data: parse_hex(data)?,
    })
}

fn parse_string(value: &str) -> Result<String, String> {
    let value = value.trim_start_matches([' ', '=']).trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or_else(|| format!("expected a quoted string, got {value:?}"))?;
    Ok(value.to_string())
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn parse_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err(format!("hex string has odd length: {value:?}"));
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|e| format!("bad hex: {e}")))
        .collect()
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
    use clevo_proto::constants::RSP_LEN;

    fn response_with_u32(value: u32) -> Vec<u8> {
        let mut buf = vec![0u8; RSP_LEN];
        buf[0x08..0x0C].copy_from_slice(&1u32.to_le_bytes());
        buf[0x0C..0x0E].copy_from_slice(&0u16.to_le_bytes());
        buf[0x0E..0x10].copy_from_slice(&4u16.to_le_bytes());
        buf[0x10..0x14].copy_from_slice(&value.to_le_bytes());
        buf
    }

    fn payload() -> [u8; PAYLOAD_BYTES] {
        [0u8; PAYLOAD_BYTES]
    }

    #[test]
    fn parses_metadata() {
        let text = r#"
            # comment
            model = "NH5xAx"
            bios  = "1.07.05"
            date  = "2026-09-12"
            allow_write = false
        "#;
        let fixture = parse_fixture(text).unwrap();
        assert_eq!(fixture.meta.model, "NH5xAx");
        assert_eq!(fixture.meta.bios, "1.07.05");
        assert_eq!(fixture.meta.date, "2026-09-12");
        assert!(!fixture.meta.allow_write);
    }

    #[test]
    fn parses_wildcard_exec() {
        let fixture = parse_fixture("exec 0c * 0102").unwrap();
        assert_eq!(fixture.entries.len(), 1);
        assert_eq!(fixture.entries[0].command, 12);
        assert_eq!(fixture.entries[0].payload, None);
        assert_eq!(fixture.entries[0].response, vec![0x01, 0x02]);
    }

    #[test]
    fn parses_exact_exec_payload() {
        let payload_hex = "00".repeat(PAYLOAD_BYTES);
        let line = format!("exec 79 {payload_hex} aabb");
        let fixture = parse_fixture(&line).unwrap();
        assert_eq!(fixture.entries[0].command, 0x79);
        assert_eq!(fixture.entries[0].payload, Some([0u8; PAYLOAD_BYTES]));
        assert_eq!(fixture.entries[0].response, vec![0xAA, 0xBB]);
    }

    #[test]
    fn rejects_wrong_payload_length() {
        assert!(parse_fixture("exec 0c 00 aabb").is_err());
    }

    #[test]
    fn rejects_unknown_directive() {
        assert!(parse_fixture("frobnicate yes").is_err());
    }

    #[test]
    fn mock_replays_wildcard() {
        let mut text = String::from("allow_write = false\n");
        text.push_str(&format!("exec 0c * {}\n", hex(&response_with_u32(7))));
        let mock = MockTransport::from_fixture_str(&text).unwrap();
        let raw = mock.execute(12, &payload()).unwrap();
        assert_eq!(clevo_proto::response::response_integer(&raw).unwrap(), 7);
    }

    #[test]
    fn mock_matches_exact_payload_only() {
        let payload_hex = "00".repeat(PAYLOAD_BYTES);
        let text = format!("exec 79 {payload_hex} aabb\n");
        let mock = MockTransport::from_fixture_str(&text).unwrap();
        assert_eq!(mock.execute(0x79, &payload()).unwrap(), vec![0xAA, 0xBB]);

        let other = [1u8; PAYLOAD_BYTES];
        assert!(matches!(
            mock.execute(0x79, &other).unwrap_err(),
            TransportError::NoFixtureMatch(_)
        ));
    }

    #[test]
    fn mock_miss_is_reported() {
        let mock = MockTransport::from_fixture_str("").unwrap();
        assert!(matches!(
            mock.execute(12, &payload()).unwrap_err(),
            TransportError::NoFixtureMatch(_)
        ));
    }

    #[test]
    fn write_is_refused_when_not_allowed() {
        let mock = MockTransport::from_fixture_str("allow_write = false\n").unwrap();
        assert!(!mock.writable());
        assert!(matches!(
            mock.write_app_settings(1, 0, &[1]).unwrap_err(),
            TransportError::NotVerified(_)
        ));
    }

    #[test]
    fn write_is_unsupported_even_when_allowed() {
        let mock = MockTransport::from_fixture_str("allow_write = true\n").unwrap();
        assert!(mock.writable());
        assert!(matches!(
            mock.write_app_settings(1, 0, &[1]).unwrap_err(),
            TransportError::Unsupported(_)
        ));
    }

    #[test]
    fn app_settings_read_and_bounds_check() {
        let mut mock = MockTransport::from_fixture_str("").unwrap();
        mock.insert_app_settings(7, 0, vec![1, 2, 3, 4]);
        assert_eq!(mock.read_app_settings(7, 0, 4).unwrap(), vec![1, 2, 3, 4]);
        assert!(matches!(
            mock.read_app_settings(7, 0, 5).unwrap_err(),
            TransportError::MalformedResponse(_)
        ));
        assert!(matches!(
            mock.read_app_settings(6, 0, 1).unwrap_err(),
            TransportError::NoFixtureMatch(_)
        ));
    }

    #[test]
    fn kind_is_mock() {
        assert_eq!(
            MockTransport::from_fixture_str("").unwrap().kind(),
            TransportKind::Mock
        );
    }
}
