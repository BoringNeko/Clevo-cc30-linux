//! End-to-end test: CLI as a D-Bus client of the daemon.
//!
//! Hosts a `clevod` object on a private session bus and drives it through
//! [`clevo_cc_cli::dbus::DbusClient`] and the CLI command layer, proving the
//! CLI can operate without any direct hardware access.
//!
//! Skipped when no session bus is present.

use std::sync::Arc;

use clevo_cc_cli::{commands, CliError, Commands, FanCommand, ProfileCommand};
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
    let mock = MockTransport::from_fixture_str(FIXTURE).expect("fixture");
    let service = Arc::new(Service::new(Box::new(mock)));

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
    assert!(out.contains("rpm=4770"), "output: {out}");
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
