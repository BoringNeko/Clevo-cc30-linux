//! Protocol constants, defined exactly once.
//!
//! Every value here is taken from the reverse-engineering reference
//! (`ControlCenter-RE/docs/02-DCHU-WMI协议参考.md` §2.1 and §2.2) and must not
//! be duplicated elsewhere. Values marked "firmware ABI" are unverified on real
//! hardware and must be confirmed against the local DSDT before any write.

/// ACPI `_HID` of the DCHU data device.
pub const DEVICE_HID: &str = "CLV0001";

/// Method GUID `{93F224E4-FBDC-4BBF-ADD6-DB71BDC0AFAD}` (`ACPIBIOS_READ`),
/// stored as the exact 16 bytes that appear in both the original
/// `InsydeDCHU.dll` packet header and the machine's raw AML (verified on a
/// COLORFUL P15 23; see `docs/hardware-notes.md`).
pub const DSM_GUID: [u8; 16] = [
    0xe4, 0x24, 0xf2, 0x93, 0xdc, 0xfb, 0xbf, 0x4b, 0xad, 0xd6, 0xdb, 0x71, 0xbd, 0xc0, 0xaf, 0xad,
];

/// ASCII name of the ACPI method invoked by the bridge driver.
pub const METHOD_NAME: [u8; 4] = *b"_DSM";

/// Full ACPI path of the DCHU `_DSM` method, verified on a COLORFUL P15 23.
pub const ACPI_DSM_PATH: &str = "\\_SB.DCHU._DSM";

/// Method revision sent in the request header.
pub const METHOD_REVISION: u32 = 0;

/// Firmware ABI constant stored at request offset `0x1C`.
///
/// Unverified on real hardware; see design document §0.3 U1.
pub const CONST_0X0104: u16 = 0x0104;

/// Firmware ABI constant stored at request offset `0x1E`.
///
/// Unverified on real hardware; see design document §0.3 U1.
pub const CONST_0X01000002: u32 = 0x0100_0002;

/// Total size of a DCHU request message in bytes (`0x420`).
pub const REQ_LEN: usize = 0x420;

/// Total size of a DCHU response message in bytes (`0x40C`).
pub const RSP_LEN: usize = 0x40C;

/// Offset of the command field inside a request.
pub const REQ_OFF_COMMAND: usize = 0x18;

/// Offset of the fixed word constant inside a request.
pub const REQ_OFF_CONST_0X0104: usize = 0x1C;

/// Offset of the fixed dword constant inside a request.
pub const REQ_OFF_CONST_0X01000002: usize = 0x1E;

/// Offset of the 256-byte caller payload inside a request.
pub const PAYLOAD_OFF: usize = 0x022;

/// Length of the caller payload in bytes.
pub const PAYLOAD_LEN: usize = 256;

/// Offset of the record count inside a response.
pub const RSP_OFF_RECORD_COUNT: usize = 0x008;

/// Offset at which the record array starts inside a response.
pub const RSP_OFF_RECORDS: usize = 0x00C;

/// `tag` value whose record carries the command result.
pub const RECORD_TAG_RESULT: u16 = 0;
