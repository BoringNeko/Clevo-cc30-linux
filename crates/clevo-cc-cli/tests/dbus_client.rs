//! End-to-end test: CLI as a D-Bus client of the daemon.
//!
//! Hosts a `clevod` object on a private session bus and drives it through
//! [`clevo_cc_cli::dbus::DbusClient`] and the CLI command layer, proving the
//! CLI can operate without any direct hardware access.
//!
//! Skipped when no session bus is present.

use std::sync::Arc;

use clevo_cc_cli::{commands, CliError, Commands, FanCommand, PowerCommand, ProfileCommand};
use clevo_transport::MockTransport;
use clevod::dbus::CcDaemon;
use clevod::policy::AllowAll;
use clevod::Service;

const FIXTURE: &str = include_str!("../../../crates/clevod/tests/fixtures/test.fixture");

fn no_session_bus() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("skipping: no session bus; run under `dbus-run-session -- cargo test`");
        true
    } else {
        false
    }
}

/// Host a permissive daemon under a unique name and return a client for it.
fn host_client(name: &str) -> clevo_cc_cli::dbus::DbusClient {
    host_client_with_fixture(name, FIXTURE)
}

fn host_client_with_fixture(name: &str, fixture: &str) -> clevo_cc_cli::dbus::DbusClient {
    let mock = MockTransport::from_fixture_str(fixture).expect("fixture");
    host_service(name, Service::new(Box::new(mock)))
}

fn host_service(name: &str, service: Service) -> clevo_cc_cli::dbus::DbusClient {
    let service = Arc::new(service);

    // The blocking test owns its own runtime for the server side.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let name_owned = name.to_string();
    std::thread::spawn(move || {
        runtime.block_on(async move {
            let _server = zbus::connection::Builder::session()
                .expect("session bus builder")
                .name(name_owned.as_str())
                .expect("valid name")
                .serve_at(
                    clevod::DBUS_PATH,
                    CcDaemon::with_authorizer(service, Arc::new(AllowAll)),
                )
                .expect("serve object")
                .build()
                .await
                .expect("server connection");
            ready_tx.send(()).expect("signal ready");
            // Keep the server alive until the test process exits.
            std::future::pending::<()>().await;
        });
    });
    ready_rx.recv().expect("server ready");

    // The CLI client normally connects to the system bus; for the test we need
    // a session-bus client. `DbusClient::session()` provides exactly that.
    clevo_cc_cli::dbus::DbusClient::session_with_name(name).expect("client")
}

fn run(
    client: &clevo_cc_cli::dbus::DbusClient,
    command: &Commands,
) -> (Result<(), CliError>, String) {
    let mut out = Vec::new();
    let result = commands::run_dbus(client, command, None, &mut out);
    (result, String::from_utf8(out).expect("utf-8"))
}

#[test]
fn connect_selects_the_session_bus() {
    if no_session_bus() {
        return;
    }
    // `--dbus-session` must reach the daemon hosted on the session bus.
    let _client = clevo_cc_cli::dbus::DbusClient::connect(true).expect("session client");
}

#[test]
fn cli_reads_status_from_daemon() {
    if no_session_bus() {
        return;
    }
    let client = host_client("org.clevo.CC.cli.status");
    // Prime the cache, then read.
    client.poll().expect("poll");
    let (result, out) = run(&client, &Commands::Fan(FanCommand::Status));
    result.expect("status");
    assert!(out.contains("CPU"), "output: {out}");
    assert!(out.contains("rpm=4667"), "output: {out}");
    assert!(out.contains("GPU2 n/a"), "output: {out}");
}

#[test]
fn cli_sets_modes_through_daemon() {
    if no_session_bus() {
        return;
    }
    let client = host_client("org.clevo.CC.cli.set");

    let (result, out) = run(
        &client,
        &Commands::Fan(FanCommand::SetMode {
            mode: "max".into(),
            apply: true,
        }),
    );
    result.expect("set fan mode");
    assert!(out.contains("121/1 = 1"), "output: {out}");

    let (result, out) = run(
        &client,
        &Commands::Profile(ProfileCommand::Set {
            value: 2,
            apply: true,
        }),
    );
    result.expect("set perf mode");
    assert!(out.contains("121/25 = 2"), "output: {out}");
}

#[test]
fn cli_dry_run_does_not_write() {
    if no_session_bus() {
        return;
    }
    let client = host_client("org.clevo.CC.cli.dryrun");
    let (result, out) = run(
        &client,
        &Commands::Fan(FanCommand::SetMode {
            mode: "max".into(),
            apply: false,
        }),
    );
    result.expect("dry run");
    assert!(out.contains("dry run"), "output: {out}");
    // The daemon's mode is still unset.
    assert_eq!(client.status().unwrap().fan_mode, 255);
}

#[test]
fn cli_capabilities_and_profiles_use_gcmd_over_dbus() {
    if no_session_bus() {
        return;
    }
    let fixture = include_str!("../../../fixtures/gcmd.fixture");
    let client = host_client_with_fixture("org.clevo.CC.cli.gcmd", fixture);
    for command in [
        Commands::Capabilities,
        Commands::Profile(ProfileCommand::List),
    ] {
        let (result, out) = run(&client, &command);
        result.unwrap();
        assert!(out.contains("source          : GCMD"), "{out}");
        assert!(out.contains("2 performance    supported"), "{out}");
    }
    assert_eq!(client.set_perf_mode("performance").unwrap(), 2);
    let rejected = host_client_with_fixture(
        "org.clevo.CC.cli.gcmdRejected",
        &fixture.replace("04300000", "00300000"),
    );
    assert!(rejected.set_perf_mode("performance").is_err());
    for apply in [false, true] {
        let (result, _) = run(
            &rejected,
            &Commands::Profile(ProfileCommand::Set { value: 2, apply }),
        );
        assert!(matches!(result, Err(CliError::Invalid(_))));
    }
    assert_eq!(rejected.status().unwrap().perf_mode, 255);
}

#[test]
fn doctor_queries_existing_daemon_without_polling_or_writing() {
    if no_session_bus() {
        return;
    }
    let client = host_client_with_fixture(
        "org.clevo.CC.cli.doctor",
        include_str!("../../../fixtures/gcmd.fixture"),
    );
    let before = client.status().unwrap();
    let (result, text) = run(
        &client,
        &Commands::Doctor {
            config: "/nonexistent-clevo-doctor-config.toml".into(),
            json: true,
        },
    );
    result.unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    for id in ["dbus.service", "daemon.status", "daemon.capabilities"] {
        assert!(
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["id"] == id && check["status"] == "pass"),
            "{text}"
        );
    }
    assert_eq!(client.status().unwrap(), before);
    assert_eq!(report["changes_made"], false);
}

#[test]
fn doctor_reports_missing_service_without_activation() {
    if no_session_bus() {
        return;
    }
    let client =
        clevo_cc_cli::dbus::DbusClient::session_with_name("org.clevo.CC.cli.missingDoctor")
            .unwrap();
    let (result, text) = run(
        &client,
        &Commands::Doctor {
            config: "/nonexistent-clevo-doctor-config.toml".into(),
            json: true,
        },
    );
    result.unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "dbus.service" && check["status"] == "fail"));
    assert!(client.diagnostic_service().unwrap().is_none());
}

#[test]
fn power_reads_validates_and_applies_only_explicit_charge_changes() {
    if no_session_bus() {
        return;
    }
    struct Tree(std::path::PathBuf);
    impl Drop for Tree {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let tree = Tree(std::env::temp_dir().join(format!("clevo-cli-power-{}", std::process::id())));
    std::fs::create_dir(&tree.0).unwrap();
    let battery = tree.0.join("class/power_supply/BAT0");
    std::fs::create_dir_all(&battery).unwrap();
    std::fs::write(battery.join("type"), "Battery\n").unwrap();
    std::fs::write(battery.join("capacity"), "67\n").unwrap();
    std::fs::write(battery.join("charge_control_end_threshold"), "100\n").unwrap();
    let client = host_service(
        "org.clevo.CC.cli.power",
        Service::new(Box::new(MockTransport::from_fixture_str(FIXTURE).unwrap()))
            .with_power(clevod::power::Power::with_sysfs(&tree.0)),
    );
    let (result, out) = run(
        &client,
        &Commands::Power(PowerCommand::Status { json: true }),
    );
    result.unwrap();
    let snapshot: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(snapshot["batteries"][0]["capacity"], 67);
    for apply in [false, true] {
        let (result, out) = run(
            &client,
            &Commands::Power(PowerCommand::SetThresholds {
                enabled: None,
                battery: "BAT0".into(),
                start: None,
                end: 80,
                apply,
            }),
        );
        result.unwrap();
        assert_eq!(out.contains("dry run"), !apply);
        assert_eq!(
            std::fs::read_to_string(battery.join("charge_control_end_threshold"))
                .unwrap()
                .trim(),
            if apply { "80" } else { "100" }
        );
    }
    for (name, start, end) in [
        ("BAT0", Some(40), 80),
        ("BAT0", None, 101),
        ("../BAT0", None, 80),
    ] {
        let (result, _) = run(
            &client,
            &Commands::Power(PowerCommand::SetThresholds {
                enabled: None,
                battery: name.into(),
                start,
                end,
                apply: true,
            }),
        );
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(battery.join("charge_control_end_threshold"))
                .unwrap()
                .trim(),
            "80"
        );
    }
    std::fs::remove_file(battery.join("charge_control_end_threshold")).unwrap();
    std::fs::create_dir_all(battery.join("device")).unwrap();
    std::fs::write(battery.join("device/path"), "\\_SB_.BAT0\n").unwrap();
    let control = tree.0.join("devices/platform/CLV0001:00/flexicharger");
    std::fs::create_dir_all(control.parent().unwrap()).unwrap();
    let original = "battery=\\_SB_.BAT0\nenabled=0\nstart=95\nend=100\nstart_values=40 50 60 70 80 95\nend_values=60 70 80 90 100\n";
    std::fs::write(&control, original).unwrap();
    for (start, end, enabled, apply, succeeds) in [
        (40, 80, Some(true), false, true),
        (40, 80, None, false, true),
        (41, 80, Some(true), false, false),
        (40, 85, Some(true), false, false),
        (95, 100, Some(false), true, true),
    ] {
        let (result, out) = run(
            &client,
            &Commands::Power(PowerCommand::SetThresholds {
                battery: "BAT0".into(),
                start: Some(start),
                end,
                enabled,
                apply,
            }),
        );
        assert_eq!(result.is_ok(), succeeds, "{out}");
        if succeeds && !apply {
            assert!(out.contains("enabled=Some(true)"));
        }
        assert_eq!(std::fs::read_to_string(&control).unwrap(), original);
    }
}
