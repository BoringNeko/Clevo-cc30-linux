//! `clevod` entry point.
//!
//! Usage:
//! ```text
//! clevod [--mock <fixture>] [--config <path>] [--no-poll]
//! ```
//!
//! With `--mock`, the daemon serves a replayed fixture instead of real hardware
//! and never writes. This makes the D-Bus surface testable offline and is the
//! safe default when no machine is present.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing::{info, warn};

use clevo_transport::{AcpiCallTransport, DriverTransport, MockTransport, Transport};
use clevod::dbus::CcDaemon;
use clevod::{config, DBUS_NAME, DBUS_PATH};

#[derive(Debug, Parser)]
#[command(name = "clevod", version, about = "Clevo control-center daemon")]
struct Args {
    /// Replay a recorded fixture instead of touching hardware.
    #[arg(long)]
    mock: Option<PathBuf>,

    /// Use the `clevo-cc` kernel driver sysfs backend (read/write).
    ///
    /// This is the enabled write path. Without it the daemon falls back to the
    /// read-only `acpi_call` backend.
    #[arg(long)]
    driver: bool,

    /// Configuration file path.
    #[arg(long, default_value_os_t = config::default_path())]
    config: PathBuf,

    /// Bind to the per-user session bus instead of the system bus (testing).
    #[arg(long)]
    session_bus: bool,

    /// Poll interval in milliseconds.
    #[arg(long, default_value_t = 2000)]
    interval_ms: u64,

    /// Do not start the background poll loop.
    #[arg(long)]
    no_poll: bool,
}

fn build_transport(args: &Args) -> Result<Box<dyn Transport>, String> {
    if let Some(path) = &args.mock {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mock = MockTransport::from_fixture_str(&text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(Box::new(mock));
    }

    if args.driver {
        if !cfg!(target_os = "linux") {
            return Err("the driver backend is Linux-only".into());
        }
        return Ok(Box::new(DriverTransport::new()));
    }

    if cfg!(target_os = "linux") {
        Ok(Box::new(AcpiCallTransport::new()))
    } else {
        Err("real hardware transport is Linux-only; use --mock".into())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let transport = match build_transport(&args) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let cfg = match config::load(&args.config) {
        Ok(c) => c,
        Err(err) => {
            warn!("{err}; using defaults");
            config::Config::default()
        }
    };

    let service = Arc::new(clevod::Service::new(transport));
    info!(
        kind = ?service.kind(),
        writable = service.writable(),
        "clevod starting"
    );

    for failure in service.apply_saved(&cfg) {
        warn!("could not re-apply saved setting: {failure}");
    }

    if !args.no_poll {
        let service = Arc::clone(&service);
        let interval = Duration::from_millis(args.interval_ms);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                if let Err(err) = service.poll_fan() {
                    warn!("fan poll failed: {err}");
                }
            }
        });
    }

    let daemon = CcDaemon::new(service);
    let builder = if args.session_bus {
        zbus::connection::Builder::session()
    } else {
        zbus::connection::Builder::system()
    };
    let connection = match builder {
        Ok(builder) => match builder.name(DBUS_NAME) {
            Ok(builder) => match builder.serve_at(DBUS_PATH, daemon) {
                Ok(builder) => builder.build().await,
                Err(err) => Err(err),
            },
            Err(err) => Err(err),
        },
        Err(err) => Err(err),
    };

    let _connection = match connection {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: could not take {DBUS_NAME} on the bus: {err}");
            return ExitCode::FAILURE;
        }
    };

    info!("serving {DBUS_NAME} at {DBUS_PATH}");

    if tokio::signal::ctrl_c().await.is_ok() {
        info!("interrupted, shutting down");
    }

    ExitCode::SUCCESS
}
