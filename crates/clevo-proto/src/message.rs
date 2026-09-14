//! Construction of DCHU request messages.
//!
//! A request is [`REQ_LEN`] (`0x420`) bytes laid out as described in
//! `ControlCenter-RE/docs/02-DCHU-WMI协议参考.md` §2.1:
//!
//! ```text
//! 0x000  16   MethodGUID (DSM_GUID)
//! 0x010   4   MethodRevision (0)
//! 0x014   4   ASCII "_DSM"
//! 0x018   4   command (LE u32)
//! 0x01C   2   0x0104   (firmware ABI, unverified)
//! 0x01E   4   0x01000002 (firmware ABI, unverified)
//! 0x022 256   caller payload
//! 0x122 ...   zero padding
//! ```

use crate::command::Command;
use crate::constants::{
    CONST_0X01000002, CONST_0X0104, DSM_GUID, METHOD_NAME, METHOD_REVISION, PAYLOAD_LEN,
    PAYLOAD_OFF, REQ_LEN, REQ_OFF_COMMAND, REQ_OFF_CONST_0X01000002, REQ_OFF_CONST_0X0104,
};
use crate::error::ProtoError;

/// Build a full DCHU request message for `command` carrying `payload`.
///
/// The returned array is always [`REQ_LEN`] bytes; unused space is zero.
pub fn build_request(command: Command, payload: &[u8; PAYLOAD_LEN]) -> [u8; REQ_LEN] {
    let mut buf = [0u8; REQ_LEN];

    buf[0x00..0x10].copy_from_slice(&DSM_GUID);
    buf[0x10..0x14].copy_from_slice(&METHOD_REVISION.to_le_bytes());
    buf[0x14..0x18].copy_from_slice(&METHOD_NAME);
    buf[REQ_OFF_COMMAND..REQ_OFF_COMMAND + 4].copy_from_slice(&command.get().to_le_bytes());
    buf[REQ_OFF_CONST_0X0104..REQ_OFF_CONST_0X0104 + 2]
        .copy_from_slice(&CONST_0X0104.to_le_bytes());
    buf[REQ_OFF_CONST_0X01000002..REQ_OFF_CONST_0X01000002 + 4]
        .copy_from_slice(&CONST_0X01000002.to_le_bytes());
    buf[PAYLOAD_OFF..PAYLOAD_OFF + PAYLOAD_LEN].copy_from_slice(payload);

    buf
}

/// Build an all-zero payload, used by pure read commands such as `12`/`13`.
pub const fn empty_payload() -> [u8; PAYLOAD_LEN] {
    [0u8; PAYLOAD_LEN]
}

/// Build a payload for `SetWMI(command, sub, value)`.
///
/// The Windows callers encode the 32-bit `value` little-endian and then
/// overwrite the most-significant byte (`payload[3]`) with the sub-command,
/// per `02-DCHU-WMI协议参考.md` §2.1.
pub fn build_subcommand_payload(value: u32, sub: u8) -> [u8; PAYLOAD_LEN] {
    let mut payload = [0u8; PAYLOAD_LEN];
    let encoded = value.to_le_bytes();
    payload[0] = encoded[0];
    payload[1] = encoded[1];
    payload[2] = encoded[2];
    payload[3] = sub;
    payload
}

/// Copy a byte slice into a fixed-size payload, rejecting wrong lengths.
///
/// Intended for data-carrying writes such as the fan curve (`14`), where the
/// caller already produced exactly [`PAYLOAD_LEN`] bytes.
pub fn payload_from_slice(bytes: &[u8]) -> Result<[u8; PAYLOAD_LEN], ProtoError> {
    let payload: [u8; PAYLOAD_LEN] =
        bytes
            .try_into()
            .map_err(|_| ProtoError::InvalidPayloadLength {
                got: bytes.len(),
                expected: PAYLOAD_LEN,
            })?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{DSM_GUID, PAYLOAD_OFF};

    #[test]
    fn request_has_fixed_length() {
        let req = build_request(Command(12), &empty_payload());
        assert_eq!(req.len(), REQ_LEN);
    }

    #[test]
    fn request_header_fields_are_placed_correctly() {
        let req = build_request(Command(121), &empty_payload());

        assert_eq!(&req[0x00..0x10], &DSM_GUID);
        assert_eq!(&req[0x10..0x14], &[0, 0, 0, 0]);
        assert_eq!(&req[0x14..0x18], b"_DSM");
        assert_eq!(&req[0x18..0x1C], &121u32.to_le_bytes());
        assert_eq!(&req[0x1C..0x1E], &0x0104u16.to_le_bytes());
        assert_eq!(&req[0x1E..0x22], &0x0100_0002u32.to_le_bytes());
    }

    #[test]
    fn request_embeds_payload_and_zero_pads() {
        let mut payload = empty_payload();
        payload[0] = 0xAB;
        payload[PAYLOAD_LEN - 1] = 0xCD;

        let req = build_request(Command(14), &payload);

        assert_eq!(req[PAYLOAD_OFF], 0xAB);
        assert_eq!(req[PAYLOAD_OFF + PAYLOAD_LEN - 1], 0xCD);
        assert!(req[PAYLOAD_OFF + PAYLOAD_LEN..].iter().all(|&b| b == 0));
    }

    #[test]
    fn subcommand_overwrites_most_significant_byte() {
        let payload = build_subcommand_payload(2, 25);

        assert_eq!(payload[0], 2);
        assert_eq!(payload[1], 0);
        assert_eq!(payload[2], 0);
        assert_eq!(payload[3], 25);
        assert!(payload[4..].iter().all(|&b| b == 0));
    }

    #[test]
    fn subcommand_preserves_lower_value_bytes() {
        let payload = build_subcommand_payload(0x0000_00FF, 1);
        assert_eq!(payload[0], 0xFF);
        assert_eq!(payload[1], 0x00);
        assert_eq!(payload[2], 0x00);
        assert_eq!(payload[3], 1);
    }

    #[test]
    fn payload_from_slice_accepts_exact_length() {
        let bytes = vec![7u8; PAYLOAD_LEN];
        let payload = payload_from_slice(&bytes).expect("exact length must be accepted");
        assert!(payload.iter().all(|&b| b == 7));
    }

    #[test]
    fn payload_from_slice_rejects_wrong_length() {
        let err = payload_from_slice(&[0u8; 255]).unwrap_err();
        assert_eq!(
            err,
            ProtoError::InvalidPayloadLength {
                got: 255,
                expected: PAYLOAD_LEN
            }
        );
    }
}
