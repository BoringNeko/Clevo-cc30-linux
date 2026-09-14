//! D-Bus integration tests.
//!
//! Host the daemon's object in-process on a private bus and drive it through a
//! real D-Bus client proxy. This exercises the exact wire interface a UI or CLI
//! would use, without touching hardware (the service is backed by a replay
//! fixture).
//!
//! The tests require a session bus. They are skipped (with a message) when
//! `DBUS_SESSION_BUS_ADDRESS` is unset, so `cargo test` still passes in a bare
//! container. `dbus-run-session -- cargo test -p clevod` provides one.

use std::sync::Arc;

use futures_util::StreamExt;

use clevo_transport::MockTransport;
use clevod::dbus::CcDaemon;
use clevod::policy::{AllowAll, DenyAll, PolicyKitAuthorizer};
use clevod::{Service, DBUS_INTERFACE, DBUS_NAME, DBUS_PATH};

const FIXTURE: &str = include_str!("fixtures/test.fixture");

fn service() -> Arc<Service> {
    let mock = MockTransport::from_fixture_str(FIXTURE).expect("fixture");
    Arc::new(Service::new(Box::new(mock)))
}

fn no_session_bus() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("skipping: no session bus; run under `dbus-run-session -- cargo test`");
        true
    } else {
        false
    }
}

/// Host `daemon` under `name` on the session bus, returning the server guard.
async fn host(name: &str, authorizer: Arc<dyn clevod::policy::Authorizer>) -> zbus::Connection {
    zbus::connection::Builder::session()
        .expect("session bus builder")
        .name(name)
        .expect("valid name")
        .serve_at(DBUS_PATH, CcDaemon::with_authorizer(service(), authorizer))
        .expect("serve object")
        .build()
        .await
        .expect("server connection")
}

/// Build a client proxy (property caching disabled) for `name`.
async fn proxy(name: &str) -> zbus::Proxy<'_> {
    let client = zbus::Connection::session()
        .await
        .expect("client connection");
    zbus::proxy::Builder::new(&client)
        .destination(name)
        .expect("destination")
        .path(DBUS_PATH)
        .expect("path")
        .interface(DBUS_INTERFACE)
        .expect("interface")
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .expect("proxy")
}

#[tokio::test]
async fn daemon_serves_properties_and_methods() {
    if no_session_bus() {
        return;
    }
    let _server = host(DBUS_NAME, Arc::new(AllowAll)).await;
    let proxy = proxy(DBUS_NAME).await;

    // Read-only start state.
    let freshness: String = proxy.get_property("FanFreshness").await.unwrap();
    assert_eq!(freshness, "unknown");
    let writable: bool = proxy.get_property("Writable").await.unwrap();
    assert!(writable, "fixture allows writes");

    // Poll refreshes the cached rpm.
    proxy.call_method("Poll", &()).await.expect("poll");
    let freshness: String = proxy.get_property("FanFreshness").await.unwrap();
    assert_eq!(freshness, "fresh");
    let cpu_rpm: u32 = proxy.get_property("CpuRpm").await.unwrap();
    assert_eq!(cpu_rpm, 4770);

    // Writes go through the service's validation.
    let applied: u8 = proxy
        .call_method("SetFanMode", &("max",))
        .await
        .expect("set fan mode")
        .body()
        .deserialize()
        .unwrap();
    assert_eq!(applied, 1);
    let fan_mode: u8 = proxy.get_property("FanMode").await.unwrap();
    assert_eq!(fan_mode, 1);

    let applied: u8 = proxy
        .call_method("SetPerfMode", &("performance",))
        .await
        .expect("set perf mode")
        .body()
        .deserialize()
        .unwrap();
    assert_eq!(applied, 2);

    // Unknown modes are rejected as a D-Bus error, not a panic.
    assert!(proxy.call_method("SetFanMode", &("bogus",)).await.is_err());

    // Curve method returns JSON.
    let curve: String = proxy
        .call_method("GetCurve", &())
        .await
        .expect("get curve")
        .body()
        .deserialize()
        .unwrap();
    assert!(
        curve.contains("\"fan_count\":2"),
        "unexpected curve json: {curve}"
    );
}

#[tokio::test]
async fn writes_are_denied_without_authorization() {
    if no_session_bus() {
        return;
    }
    let _server = host("org.clevo.CC.deny", Arc::new(DenyAll)).await;
    let proxy = proxy("org.clevo.CC.deny").await;

    // Reads still work.
    proxy.call_method("Poll", &()).await.expect("poll allowed");
    let _: u32 = proxy.get_property("CpuRpm").await.unwrap();

    // Writes are refused.
    let err = proxy
        .call_method("SetFanMode", &("max",))
        .await
        .expect_err("write must be denied");
    let text = err.to_string();
    assert!(
        text.contains("AccessDenied") || text.contains("not authorized"),
        "unexpected error: {text}"
    );

    let err = proxy
        .call_method("SetPerfMode", &("performance",))
        .await
        .expect_err("write must be denied");
    let text = err.to_string();
    assert!(
        text.contains("AccessDenied") || text.contains("not authorized"),
        "unexpected error: {text}"
    );
}

#[tokio::test]
async fn fan_changed_signal_is_emitted() {
    if no_session_bus() {
        return;
    }
    let _server = host("org.clevo.CC.signal", Arc::new(AllowAll)).await;
    let proxy = proxy("org.clevo.CC.signal").await;

    let mut stream = proxy
        .receive_signal("FanChanged")
        .await
        .expect("subscribe FanChanged");

    proxy.call_method("Poll", &()).await.expect("poll");

    let signal = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .expect("signal within timeout")
        .expect("a signal");

    let (cpu, gpu): (u32, u32) = signal.body().deserialize().expect("decode body");
    assert_eq!(cpu, 4770);
    assert_eq!(gpu, 0);
}

#[tokio::test]
async fn default_authorizer_denies_non_root_without_polkit() {
    if no_session_bus() {
        return;
    }
    // A bus-less authorizer applies the "root only" fallback. On a normal
    // developer session the uid is non-zero, so writes must be denied.
    let _server = host(
        "org.clevo.CC.polkit",
        Arc::new(PolicyKitAuthorizer::without_bus()),
    )
    .await;
    let proxy = proxy("org.clevo.CC.polkit").await;

    let uid: u32 = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    if uid == 0 {
        // Running as root: writes are allowed by the fallback.
        proxy
            .call_method("SetFanMode", &("max",))
            .await
            .expect("root may write");
    } else {
        assert!(proxy.call_method("SetFanMode", &("max",)).await.is_err());
    }
}
