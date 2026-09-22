//! Integration test for the read-only `acpi_call` transport.
//!
//! The transport is exercised through its injectable fixed-reply hook so this
//! test never touches `/proc` or real hardware. The procfs code path itself is
//! covered by the unit tests in the module.

use clevo_proto::constants::PAYLOAD_LEN;
use clevo_proto::fan_curve::parse_curve;
use clevo_proto::fan_status::parse_fan_status;
use clevo_transport::{AcpiCallTransport, Transport, TransportError, TransportKind};

fn buffer_reply(bytes: &[u8]) -> String {
    format!(
        "{{{}}}",
        bytes
            .iter()
            .map(|b| format!("0x{b:02x}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

#[test]
fn fan_status_round_trip() {
    let mut reply_bytes = vec![0u8; 42];
    reply_bytes[2] = 0x01;
    reply_bytes[3] = 0xCE; // 462
    reply_bytes[4] = 0x01;
    reply_bytes[5] = 0xD9; // 473

    let transport = AcpiCallTransport::with_fixed_reply(buffer_reply(&reply_bytes));
    let raw = transport
        .execute(12, &[0u8; PAYLOAD_LEN])
        .expect("command 12");
    let payload = clevo_proto::response::response_first_record(&raw).expect("record");
    let status = parse_fan_status(payload).expect("parse");
    assert_eq!(status.cpu_period, 462);
    assert_eq!(status.gpu1_period, 473);
}

#[test]
fn fan_curve_round_trip() {
    let mut reply_bytes = vec![0u8; 42];
    reply_bytes[0x0c] = 2;
    reply_bytes[0x0e] = 2;
    reply_bytes[0x0f] = 6;
    reply_bytes[0x10] = 40;
    reply_bytes[0x11] = 63;
    reply_bytes[0x12] = 60;
    reply_bytes[0x13] = 91;

    let transport = AcpiCallTransport::with_fixed_reply(buffer_reply(&reply_bytes));
    let raw = transport
        .execute(13, &[0u8; PAYLOAD_LEN])
        .expect("command 13");
    let payload = clevo_proto::response::response_first_record(&raw).expect("record");
    let info = parse_curve(payload).expect("parse");
    assert_eq!(info.fan_count, 2);
    assert_eq!(info.kb_type, 6);
    assert_eq!(info.curve.cpu[0].temp, 40);
}

#[test]
fn write_command_is_rejected() {
    let transport = AcpiCallTransport::with_fixed_reply("{}");
    let err = transport
        .execute(14, &[0u8; PAYLOAD_LEN])
        .expect_err("write command must be rejected");
    assert!(matches!(err, TransportError::Unsupported(_)));
}

#[test]
fn missing_proc_entry_reports_unsupported() {
    let transport = AcpiCallTransport::with_path("/nonexistent/acpi/call");
    let err = transport
        .execute(12, &[0u8; PAYLOAD_LEN])
        .expect_err("must fail");
    assert!(matches!(err, TransportError::Unsupported(_)));
}

#[test]
fn backend_is_read_only() {
    let transport = AcpiCallTransport::with_fixed_reply("{}");
    assert!(!transport.writable());
    assert_eq!(transport.kind(), TransportKind::AcpiCall);
}
