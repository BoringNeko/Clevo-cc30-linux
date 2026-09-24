//! Headless HTTP bridge for the Electron shell.
//!
//! With `--serve` the binary stops being a Tauri app and becomes a small JSON
//! API the Electron main process proxies to the renderer. It exists so the
//! Electron build can reuse all of this crate's command logic (D-Bus, PolicyKit,
//! wallpapers) without a second, Node-side implementation.
//!
//! Protocol
//! --------
//! * On startup the server prints one line to stdout:
//!   `CLEVO_CC_BACKEND <port> <token>`, then keeps serving. The Electron main
//!   process parses that handshake and records the token.
//! * `POST /invoke` with `{"command": "...", "args": {...}}` and the header
//!   `x-clevo-token: <token>` runs the command and replies
//!   `{"ok": true, "value": ...}` or `{"ok": false, "error": "..."}`.
//! * The server binds `127.0.0.1` only, and the token plus a strict CORS
//!   allow-list keep other local pages from driving hardware writes.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

use crate::commands;

/// Run the bridge, blocking forever. Prints the handshake line first.
pub fn run() -> std::io::Result<()> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let port = listener.local_addr()?.port();
    let token = make_token();

    println!("CLEVO_CC_BACKEND {port} {token}");
    std::io::stdout().flush()?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let token = token.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle(stream, &token) {
                        eprintln!("clevo-cc-ui --serve: request failed: {e}");
                    }
                });
            }
            Err(e) => eprintln!("clevo-cc-ui --serve: accept failed: {e}"),
        }
    }
    Ok(())
}

/// A per-run random token so only the process that read the handshake can call.
fn make_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as u128;
    // Mix in the address of a stack local for a little ASLR entropy; this token
    // only guards a loopback port, it is not a cryptographic secret.
    let local = 0u8;
    let addr = &local as *const u8 as u128;
    format!("{now:032x}{pid:08x}{seq:08x}{addr:016x}")
}

fn handle(mut stream: TcpStream, token: &str) -> std::io::Result<()> {
    let request = read_request(&mut stream)?;

    // The renderer loads the app from `file://` in production, so `Origin` is
    // `null`; allow that and the dev server, but nothing else.
    let origin = request.header("origin").unwrap_or_default();
    let allowed_origin = match origin.as_str() {
        "" | "null" => Some("null".to_string()),
        o if o.starts_with("http://localhost:") || o.starts_with("http://127.0.0.1:") => {
            Some(o.to_string())
        }
        _ => None,
    };

    if request.method != "POST" || request.path != "/invoke" {
        return reply(
            &mut stream,
            404,
            None,
            &json!({ "ok": false, "error": "not found" }),
        );
    }
    if request.header("x-clevo-token").as_deref() != Some(token) {
        return reply(
            &mut stream,
            403,
            allowed_origin.as_deref(),
            &json!({ "ok": false, "error": "bad token" }),
        );
    }

    let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
    let command = body.get("command").and_then(Value::as_str).unwrap_or("");
    let args = body
        .get("args")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    let response = match dispatch(command, &args) {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    reply(&mut stream, 200, allowed_origin.as_deref(), &response)
}

/// Run a command by name, mirroring the Tauri handler list.
///
/// Only the shell-agnostic commands appear here; `show_main_window`,
/// `hide_main_window` and `quit_app` are handled by the Electron main process.
fn dispatch(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "get_fan_snapshot" => ok(commands::get_fan_snapshot()?),
        "poll_fan" => ok(commands::poll_fan()?),
        "get_hardware_usage" => ok(commands::get_hardware_usage()?),
        "get_fan_curve" => ok(commands::get_fan_curve()?),
        "set_fan_mode" => {
            let mode = str_arg(args, "mode")?;
            ok(commands::set_fan_mode(mode)?)
        }
        "set_perf_mode" => {
            let mode = str_arg(args, "mode")?;
            ok(commands::set_perf_mode(mode)?)
        }
        "set_fan_curve" => {
            let curve = args
                .get("curve")
                .cloned()
                .ok_or_else(|| "missing curve".to_string())?;
            let curve: crate::dbus::FanCurve =
                serde_json::from_value(curve).map_err(|e| format!("invalid curve: {e}"))?;
            commands::set_fan_curve(curve)?;
            Ok(Value::Null)
        }
        "get_launch_prefs" => ok(commands::get_launch_prefs()),
        "set_launch_prefs" => {
            let prefs = args
                .get("prefs")
                .cloned()
                .ok_or_else(|| "missing prefs".to_string())?;
            let prefs: crate::prefs::LaunchPrefs =
                serde_json::from_value(prefs).map_err(|e| format!("invalid prefs: {e}"))?;
            commands::set_launch_prefs(prefs)?;
            Ok(Value::Null)
        }
        "save_wallpaper" => ok(commands::save_wallpaper(
            str_arg(args, "dataBase64")?,
            str_arg(args, "ext")?,
        )?),
        "load_wallpaper" => ok(commands::load_wallpaper()),
        "clear_wallpaper" => ok(commands::clear_wallpaper()),
        "save_logo" => ok(commands::save_logo(
            str_arg(args, "dataBase64")?,
            str_arg(args, "ext")?,
        )?),
        "load_logo" => ok(commands::load_logo()),
        "clear_logo" => ok(commands::clear_logo()),
        "read_image_path" => ok(commands::read_image_path(str_arg(args, "path")?)),
        other => Err(format!("unknown command: {other}")),
    }
}

fn str_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing argument: {key}"))
}

/// Infallible variant for values that always serialise (plain scalars, `null`,
/// string-keyed structs). Keeps the `dispatch` arms free of `?` noise.
fn ok<T: serde::Serialize>(value: T) -> Result<Value, String> {
    Ok(serde_json::to_value(value).unwrap_or(Value::Null))
}

/// A parsed HTTP request (enough for this bridge).
struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).cloned()
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn reply(
    stream: &mut TcpStream,
    status: u16,
    origin: Option<&str>,
    body: &Value,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        payload.len()
    );
    // Only answer CORS when we accepted the Origin; a rejected page gets no
    // permission and its script cannot read the response.
    if let Some(origin) = origin {
        head.push_str(&format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Headers: content-type, x-clevo-token\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\n"
        ));
    }
    head.push_str("\r\n");

    stream.write_all(head.as_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_long_and_changes_between_calls() {
        let a = make_token();
        let b = make_token();
        assert_ne!(a, b);
        assert!(a.len() >= 32, "token too short: {a}");
    }

    /// Unknown commands and missing arguments fail with a message rather than
    /// panicking, so a typo in the renderer shows up as a UI error.
    #[test]
    fn dispatch_rejects_unknown_commands() {
        let err = dispatch("nope", &json!({})).unwrap_err();
        assert!(err.contains("unknown command"));
    }

    #[test]
    fn dispatch_validates_arguments_before_touching_hardware() {
        let err = dispatch("set_fan_mode", &json!({})).unwrap_err();
        assert!(err.contains("missing argument: mode"));

        let err = dispatch("set_fan_curve", &json!({})).unwrap_err();
        assert!(err.contains("missing curve"));
    }

    #[test]
    fn invalid_curve_json_is_reported_not_passed_on() {
        let err = dispatch("set_fan_curve", &json!({ "curve": { "cpu": 1 } })).unwrap_err();
        assert!(err.contains("invalid curve"), "got: {err}");
    }
}
