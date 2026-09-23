// SPDX-License-Identifier: MIT OR Apache-2.0
//
// End-to-end test of the Electron backend bridge: spawn the real `--serve`
// binary, parse its handshake, and drive it over HTTP. This is the contract the
// Electron main process (`ui/electron/main.cjs`) relies on, so a change to the
// handshake line or the reply shape fails here rather than in the packaged app.
//
// The test needs no hardware and no daemon: it only exercises `get_launch_prefs`
// (a local file) plus the rejection paths, which never reach D-Bus.
//
// Only the headless build has `--serve`; under the Tauri shell the binary rejects
// it, so the whole file is skipped there. Run it with:
//   cargo test --no-default-features --test electron_bridge

#![cfg(not(feature = "tauri-shell"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The built binary under test.
fn binary() -> std::path::PathBuf {
    // `CARGO_BIN_EXE_<name>` is set by cargo for integration tests.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_clevo-cc-ui"))
}

/// A minimal HTTP POST that returns (status, body).
fn post(port: u16, token: Option<&str>, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to bridge");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let auth = token
        .map(|t| format!("x-clevo-token: {t}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST /invoke HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();

    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();

    let status: u16 = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

#[test]
fn bridge_handshake_and_invoke() {
    // Keep the launch prefs out of the developer's real config.
    let cfg = std::env::temp_dir().join(format!("clevo-bridge-test-{}", std::process::id()));
    std::fs::create_dir_all(&cfg).unwrap();

    let mut child = Command::new(binary())
        .arg("--serve")
        .env("XDG_CONFIG_HOME", &cfg)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn --serve backend");

    // Read the handshake line.
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read handshake");
    let mut parts = line.split_whitespace();
    assert_eq!(parts.next(), Some("CLEVO_CC_BACKEND"));
    let port: u16 = parts.next().expect("port").parse().expect("numeric port");
    let token = parts.next().expect("token").to_string();
    assert!(token.len() >= 32);

    // A good call returns a value.
    let (status, body) = post(
        port,
        Some(&token),
        r#"{"command":"get_launch_prefs","args":{}}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"ok\":true"), "body: {body}");
    assert!(body.contains("backend"), "body: {body}");

    // A missing or wrong token is rejected before any work happens.
    let (status, _) = post(port, None, r#"{"command":"get_launch_prefs","args":{}}"#);
    assert_eq!(status, 403);
    let (status, _) = post(
        port,
        Some("wrong"),
        r#"{"command":"get_launch_prefs","args":{}}"#,
    );
    assert_eq!(status, 403);

    // Unknown commands report an error rather than a value.
    let (status, body) = post(port, Some(&token), r#"{"command":"nope","args":{}}"#);
    assert_eq!(status, 200);
    assert!(body.contains("\"ok\":false"), "body: {body}");

    child.kill().ok();
    child.wait().ok();
    let _ = std::fs::remove_dir_all(&cfg);
}
