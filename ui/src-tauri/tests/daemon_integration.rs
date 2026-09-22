//! End-to-end test: the UI backend as a client of the real daemon.
//!
//! Hosts a `clevod` object on a private session bus (backed by a replay
//! fixture) and drives it through [`clevo_cc_ui::dbus::DaemonClient`], proving
//! the Tauri commands' data path works without hardware.
//!
//! Skipped when no session bus is present; run under
//! `dbus-run-session -- cargo test`.

use std::sync::Arc;

use clevo_cc_ui::dbus::DaemonClient;
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

fn start_daemon(name: &str) {
    start_daemon_with(name, Arc::new(AllowAll));
}

fn start_daemon_with(name: &str, authorizer: Arc<dyn clevod::policy::Authorizer>) {
    let mock = MockTransport::from_fixture_str(FIXTURE).expect("fixture");
    let service = Arc::new(Service::new(Box::new(mock)));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let (tx, rx) = std::sync::mpsc::channel();
    let name = name.to_string();
    std::thread::spawn(move || {
        runtime.block_on(async move {
            let _server = zbus::connection::Builder::session()
                .expect("session bus")
                .name(name.as_str())
                .expect("name")
                .serve_at(
                    clevod::DBUS_PATH,
                    CcDaemon::with_authorizer(service, authorizer),
                )
                .expect("serve")
                .build()
                .await
                .expect("server");
            tx.send(()).expect("ready");
            std::future::pending::<()>().await;
        });
    });
    rx.recv().expect("daemon ready");
}

#[test]
fn snapshot_from_real_daemon() {
    if no_session_bus() {
        return;
    }
    // The UI client targets org.clevo.CC; host the daemon under that name.
    start_daemon(clevo_cc_ui::dbus::DBUS_NAME);

    let client = DaemonClient::session().expect("client");
    client.poll().expect("poll");
    let snap = client.snapshot().expect("snapshot");
    assert_eq!(snap.cpu.rpm, 4667);
    assert_eq!(snap.freshness, "fresh");
    assert_eq!(snap.fan_count, 2);
    assert!(snap.cpu.available);
    assert!(!snap.gpu2.available);
    assert!(snap.writable);

    let curve = client.curve().expect("curve");
    assert_eq!(curve.fan_count, 2);
    assert_eq!(curve.cpu.len(), 4);
}

#[test]
fn writes_go_through_the_daemon() {
    if no_session_bus() {
        return;
    }
    start_daemon("org.clevo.CC.write");

    let client = DaemonClient::session_with_name("org.clevo.CC.write").expect("client");
    // The fixture allows writes.
    assert_eq!(client.set_fan_mode("max").expect("set fan mode"), 1);
    assert_eq!(
        client.set_perf_mode("performance").expect("set perf mode"),
        2
    );

    let snap = client.snapshot().expect("snapshot");
    assert_eq!(snap.fan_mode, 1);
    assert_eq!(snap.perf_mode, 2);
}

#[test]
fn write_denial_is_reported_as_an_error() {
    if no_session_bus() {
        return;
    }
    // A daemon that denies every write, as a stricter PolicyKit policy would.
    start_daemon_with("org.clevo.CC.deny", Arc::new(clevod::policy::DenyAll));

    let client = DaemonClient::session_with_name("org.clevo.CC.deny").expect("client");
    let err = client
        .set_fan_mode("max")
        .expect_err("write must be denied");
    assert!(
        err.message.contains("AccessDenied") || err.message.contains("not authorized"),
        "unexpected error: {}",
        err.message
    );
}
