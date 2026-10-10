//! Exercise actual CLI dispatch, especially failures before backend connection.

use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_clevo-cc"))
}

#[test]
fn mock_doctor_is_offline_and_json_is_machine_readable() {
    let output = cli()
        .args(["doctor", "--json"])
        .env(
            "DBUS_SYSTEM_BUS_ADDRESS",
            "unix:path=/nonexistent-clevo-doctor-bus",
        )
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["transport"], "Mock");
    assert_eq!(report["summary"]["fail"], 0);
    assert_eq!(report["changes_made"], false);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| !check["id"].as_str().unwrap().starts_with("dbus.")));
}

#[test]
fn unavailable_bus_still_prints_full_report_and_fails() {
    let output = cli()
        .args([
            "--transport",
            "dbus",
            "doctor",
            "--json",
            "--config",
            "/nonexistent-clevo-doctor-config.toml",
        ])
        .env(
            "DBUS_SYSTEM_BUS_ADDRESS",
            "unix:path=/nonexistent-clevo-doctor-bus",
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let checks = report["checks"].as_array().unwrap();
    assert!(checks
        .iter()
        .any(|check| check["id"] == "dbus.connection" && check["status"] == "fail"));
    assert!(checks.iter().any(|check| check["id"] == "config.schema"));
    assert!(checks.iter().any(|check| check["id"] == "platform_profile"));
    assert_eq!(report["changes_made"], false);
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_fixture_is_a_diagnostic_failure_not_an_early_error() {
    let output = cli()
        .args(["--fixture", "/nonexistent-clevo-doctor.fixture", "doctor"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("[Fail] transport:"));
    assert!(text.contains("summary:"));
    assert!(text.contains("no changes were made."));
    assert!(output.stderr.is_empty());
}
