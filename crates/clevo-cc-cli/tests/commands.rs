//! CLI command tests driven entirely by the mock transport.

use clevo_cc_cli::commands;
use clevo_cc_cli::{CliError, FanCommand, ProfileCommand};
use clevo_transport::{MockTransport, Transport};

const EXAMPLE: &str = include_str!("../../../fixtures/example.fixture");

fn mock() -> MockTransport {
    MockTransport::from_fixture_str(EXAMPLE).expect("fixture parses")
}

fn run<F>(f: F) -> (Result<(), CliError>, String)
where
    F: FnOnce(&dyn Transport, &mut Vec<u8>) -> Result<(), CliError>,
{
    let transport = mock();
    let mut out = Vec::new();
    let result = f(&transport, &mut out);
    (result, String::from_utf8(out).expect("utf-8 output"))
}

#[test]
fn doctor_reports_mock_and_no_changes() {
    let (result, out) = run(|t, out| commands::run_doctor(t, out));
    result.unwrap();
    assert!(out.contains("transport    : Mock"));
    assert!(out.contains("writable     : false"));
    assert!(out.contains("no changes were made."));
}

#[test]
fn fan_status_prints_recorded_values() {
    let (result, out) = run(|t, out| commands::run_fan(t, &FanCommand::Status, out));
    result.unwrap();
    assert!(out.contains("CPU"));
    assert!(out.contains("rpm=4667"));
    assert!(out.contains("period_raw=462"));
    assert!(out.contains("GPU1"));
    // The example fixture declares fan_count = 2, so the third channel is n/a.
    assert!(out.contains("GPU2 n/a"));
}

#[test]
fn fan_curve_prints_four_points() {
    let (result, out) = run(|t, out| commands::run_fan(t, &FanCommand::Curve, out));
    result.unwrap();
    assert!(out.contains("fan count : 2"));
    assert!(out.contains("(100C,100%)"));
}

#[test]
fn set_mode_without_apply_is_dry_run() {
    let command = FanCommand::SetMode {
        mode: "quiet".to_string(),
        apply: false,
    };
    let (result, out) = run(|t, out| commands::run_fan(t, &command, out));
    result.unwrap();
    assert!(out.contains("dry run"));
    assert!(out.contains("sub 1"));
}

#[test]
fn set_mode_with_apply_is_refused_by_mock() {
    let command = FanCommand::SetMode {
        mode: "auto".to_string(),
        apply: true,
    };
    let (result, _) = run(|t, out| commands::run_fan(t, &command, out));
    assert!(matches!(result, Err(CliError::Transport(_))));
}

#[test]
fn unknown_fan_mode_is_invalid() {
    let command = FanCommand::SetMode {
        mode: "turbo".to_string(),
        apply: false,
    };
    let (result, _) = run(|t, out| commands::run_fan(t, &command, out));
    assert!(matches!(result, Err(CliError::Invalid(_))));
}

#[test]
fn custom_fan_mode_is_accepted() {
    let command = FanCommand::SetMode {
        mode: "custom".to_string(),
        apply: false,
    };
    let (result, out) = run(|t, out| commands::run_fan(t, &command, out));
    result.unwrap();
    assert!(out.contains("would set fan mode to 6"), "out: {out}");
}

#[test]
fn set_curve_without_apply_prints_the_payload() {
    let command = FanCommand::SetCurve {
        cpu: "40,20 55,40 75,70 95,100".to_string(),
        gpu1: None,
        gpu2: None,
        apply: false,
    };
    let (result, out) = run(|t, out| commands::run_fan(t, &command, out));
    result.unwrap();
    assert!(out.contains("dry run"), "out: {out}");
    // T2/D2 and T3/D3 land in slots 2..6; duty is raw 0..255.
    assert!(out.contains("[2]=55 [3]=102 [4]=75 [5]=179"), "out: {out}");
}

#[test]
fn set_curve_rejects_non_increasing_temperatures() {
    let command = FanCommand::SetCurve {
        cpu: "40,20 60,40 60,70 95,100".to_string(),
        gpu1: None,
        gpu2: None,
        apply: false,
    };
    let (result, _) = run(|t, out| commands::run_fan(t, &command, out));
    assert!(matches!(result, Err(CliError::Invalid(_))));
}

#[test]
fn set_curve_rejects_wrong_point_count() {
    let command = FanCommand::SetCurve {
        cpu: "40,20 60,40".to_string(),
        gpu1: None,
        gpu2: None,
        apply: false,
    };
    let (result, _) = run(|t, out| commands::run_fan(t, &command, out));
    assert!(matches!(result, Err(CliError::Invalid(_))));
}

#[test]
fn set_curve_rejects_duty_above_100() {
    let command = FanCommand::SetCurve {
        cpu: "40,20 55,40 75,101 95,100".to_string(),
        gpu1: None,
        gpu2: None,
        apply: false,
    };
    let (result, _) = run(|t, out| commands::run_fan(t, &command, out));
    assert!(matches!(result, Err(CliError::Invalid(_))));
}

#[test]
fn set_curve_with_apply_is_refused_by_mock() {
    let command = FanCommand::SetCurve {
        cpu: "40,20 55,40 75,70 95,100".to_string(),
        gpu1: None,
        gpu2: None,
        apply: true,
    };
    let (result, _) = run(|t, out| commands::run_fan(t, &command, out));
    assert!(matches!(result, Err(CliError::Transport(_))));
}

#[test]
fn profile_list_shows_supported_modes() {
    let (result, out) = run(|t, out| commands::run_profile(t, &ProfileCommand::List, out));
    result.unwrap();
    assert!(out.contains("v1 (0x0100)"));
    assert!(out.contains("2 performance    supported"));
}

#[test]
fn profile_set_out_of_range_is_invalid() {
    let (result, _) = run(|t, out| {
        commands::run_profile(
            t,
            &ProfileCommand::Set {
                value: 9,
                apply: false,
            },
            out,
        )
    });
    assert!(matches!(result, Err(CliError::Invalid(_))));
}

#[test]
fn profile_set_unsupported_mode_is_invalid() {
    // The fixture advertises all four modes, so use a transport whose page7
    // only supports quiet to prove capability gating.
    let mut text = String::from(
        "model = \"EXAMPLE\"\nbios = \"x\"\ndate = \"2026-09-12\"\nallow_write = false\n",
    );
    let mut page7 = [0u8; 256];
    page7[0] = 0x01;
    page7[1] = 0x00;
    page7[17] = 0b0000_0001; // quiet only
    text.push_str(&format!("appsettings 07 0000 {}\n", hex(&page7)));
    let transport = MockTransport::from_fixture_str(&text).unwrap();
    let mut out = Vec::new();
    let result = commands::run_profile(
        &transport,
        &ProfileCommand::Set {
            value: 2,
            apply: false,
        },
        &mut out,
    );
    assert!(matches!(result, Err(CliError::Invalid(_))));
}

#[test]
fn profile_set_without_apply_is_dry_run() {
    let (result, out) = run(|t, out| {
        commands::run_profile(
            t,
            &ProfileCommand::Set {
                value: 2,
                apply: false,
            },
            out,
        )
    });
    result.unwrap();
    assert!(out.contains("dry run"));
    assert!(out.contains("sub 25"));
}

#[test]
fn watch_emits_requested_number_of_samples() {
    let (result, out) = run(|t, out| {
        commands::run_fan(
            t,
            &FanCommand::Watch {
                interval_ms: 0,
                count: 3,
                json: false,
            },
            out,
        )
    });
    result.unwrap();
    assert_eq!(out.lines().count(), 3);
    assert_eq!(out.matches("CPU=").count(), 3);
}

#[test]
fn watch_json_marks_absent_fan_null() {
    let (result, out) = run(|t, out| {
        commands::run_fan(
            t,
            &FanCommand::Watch {
                interval_ms: 0,
                count: 1,
                json: true,
            },
            out,
        )
    });
    result.unwrap();
    assert!(out.contains("\"cpu\":{\"rpm\":4667,\"period_raw\":462"));
    assert!(out.contains("\"gpu2\":null"));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
