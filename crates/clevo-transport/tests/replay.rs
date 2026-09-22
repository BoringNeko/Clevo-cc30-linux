//! End-to-end replay of the bundled placeholder fixture through both crates.

use clevo_proto::command::CMD_FAN_CURVE_READ;
use clevo_proto::constants::PAYLOAD_LEN;
use clevo_proto::fan_curve::parse_curve;
use clevo_proto::fan_status::parse_fan_status;
use clevo_proto::message::empty_payload;
use clevo_proto::response::response_first_record;
use clevo_transport::{MockTransport, Transport, TransportKind};

const EXAMPLE: &str = include_str!("../../../fixtures/example.fixture");

#[test]
fn example_fixture_replays_fan_status() {
    let mock = MockTransport::from_fixture_str(EXAMPLE).expect("fixture parses");
    assert_eq!(mock.kind(), TransportKind::Mock);
    assert_eq!(mock.meta().model, "EXAMPLE");
    assert!(!mock.meta().allow_write);

    let raw = mock.execute(12, &empty_payload()).expect("command 12");
    let payload = response_first_record(&raw).expect("result record");
    let status = parse_fan_status(payload).expect("fan status parses");
    assert_eq!(status.cpu_period, 462);
    assert_eq!(status.gpu1_period, 473);
    assert_eq!(status.cpu_duty, 200);
    assert_eq!(status.cpu_temp_c, Some(55));
}

#[test]
fn example_fixture_replays_fan_curve() {
    let mock = MockTransport::from_fixture_str(EXAMPLE).expect("fixture parses");

    let raw = mock
        .execute(CMD_FAN_CURVE_READ.get(), &empty_payload())
        .expect("command 13");
    let payload = response_first_record(&raw).expect("result record");
    let info = parse_curve(payload).expect("curve parses");

    assert_eq!(info.fan_count, 2);
    assert_eq!(info.init_mode, 2);
    assert_eq!(info.curve.cpu[0].temp, 40);
    assert_eq!(info.curve.cpu[2].temp, 80);
    assert_eq!(info.curve.cpu[3].duty_pct, 100);
}

#[test]
fn example_fixture_refuses_writes() {
    let mock = MockTransport::from_fixture_str(EXAMPLE).expect("fixture parses");
    let payload = [0u8; PAYLOAD_LEN];
    assert!(matches!(
        mock.write_app_settings(1, 0, &payload).unwrap_err(),
        clevo_transport::TransportError::NotVerified(_)
    ));
}
