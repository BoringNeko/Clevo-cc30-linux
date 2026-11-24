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

use std::num::NonZeroU64;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing::{info, warn};

use clevo_transport::{
    AcpiCallTransport, AcpiKeyboard, DriverTransport, HidKeyboard, Keyboard, MockTransport,
    Transport,
};
use clevod::dbus::CcDaemon;
use clevod::{config, DBUS_NAME, DBUS_PATH};

const DEFAULT_INTERVAL_MS: u64 = 2_000;
const P15_MIN_FAN_INTERVAL_MS: u64 = 60_000;

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

    /// Fan poll interval in milliseconds (default 2000; no recurring full-status polls on P15 23).
    #[arg(long)]
    interval_ms: Option<NonZeroU64>,

    /// Disable background fan sampling and mode synchronization.
    #[arg(long)]
    no_poll: bool,
}

fn is_p15_23(vendor: &str, product: &str) -> bool {
    vendor.trim().eq_ignore_ascii_case("COLORFUL") && product.trim().eq_ignore_ascii_case("P15 23")
}

fn host_is_p15_23() -> bool {
    match (
        std::fs::read_to_string("/sys/class/dmi/id/sys_vendor"),
        std::fs::read_to_string("/sys/class/dmi/id/product_name"),
    ) {
        (Ok(vendor), Ok(product)) => is_p15_23(&vendor, &product),
        _ => false,
    }
}

fn fan_poll_intervals(
    requested: Option<NonZeroU64>,
    is_p15: bool,
    rpm_only: bool,
) -> (Option<u64>, u64) {
    if is_p15 && !rpm_only {
        let interval = requested.map(|value| value.get().max(P15_MIN_FAN_INTERVAL_MS));
        (interval, interval.unwrap_or(P15_MIN_FAN_INTERVAL_MS))
    } else {
        let interval = requested.map_or(DEFAULT_INTERVAL_MS, NonZeroU64::get);
        (Some(interval), interval)
    }
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

    // Persist only when the hardware is real. A `--mock` run replays a fixture
    // and must not write user choices into the live config: they would be
    // re-applied to the EC by the next real daemon start.
    let persist_path = if args.mock.is_some() {
        info!("mock transport: configuration persistence disabled");
        None
    } else {
        Some(args.config.clone())
    };
    let is_p15 = args.mock.is_none() && host_is_p15_23();
    let service = clevod::Service::new(transport).with_config_path(persist_path);
    let rpm_only = service.uses_fan_periods();
    let (fan_interval_ms, min_fan_interval_ms) =
        fan_poll_intervals(args.interval_ms, is_p15, rpm_only);
    if rpm_only {
        info!(
            background_fan_interval_ms = ?fan_interval_ms.filter(|_| !args.no_poll),
            "using direct fan periods without command 12"
        );
    } else if is_p15 {
        info!(
            background_fan_interval_ms = ?fan_interval_ms.filter(|_| !args.no_poll),
            min_fan_interval_ms,
            "COLORFUL P15 23: limiting EC fan telemetry to avoid gpe6E interrupt bursts"
        );
    }
    let service = service.with_fan_poll_interval_ms(min_fan_interval_ms);
    let firmware_curve = service.read_curve().ok();
    let firmware_keyboard_type = firmware_curve.as_ref().map(|curve| curve.kb_type);
    if let Some(curve) = firmware_curve {
        service.set_keyboard_firmware_type(Some(curve.kb_type));
        info!(
            kb_type = curve.kb_type,
            "firmware keyboard capability detected"
        );
    } else {
        service.set_keyboard_firmware_type(None);
    }
    let keyboard = if args.mock.is_some() {
        None
    } else if args.driver && matches!(firmware_keyboard_type, Some(6 | 22)) {
        match AcpiKeyboard::discover() {
            Ok(Some(keyboard)) => {
                info!(path = %keyboard.snapshot().info.path, "ACPI-DCHU RGB15 keyboard backend found");
                Some(Box::new(keyboard) as Box<dyn Keyboard>)
            }
            Ok(None) => {
                warn!(
                    "firmware advertises RGB15 but the clevo-cc keyboard_rgb sysfs node is missing"
                );
                None
            }
            Err(err) => {
                warn!(%err, "could not probe ACPI-DCHU keyboard RGB backend");
                None
            }
        }
    } else {
        match HidKeyboard::discover() {
            Ok(Some(keyboard)) => {
                info!("ITE 048d:8910 keyboard RGB controller found");
                Some(Box::new(keyboard) as Box<dyn Keyboard>)
            }
            Ok(None) => {
                warn!("no compatible keyboard RGB controller found; RGB controls disabled");
                None
            }
            Err(err) => {
                warn!(%err, "could not probe keyboard RGB controller; RGB controls disabled");
                None
            }
        }
    };
    let service = Arc::new(service.with_keyboard(keyboard));
    info!(
        kind = ?service.kind(),
        writable = service.writable(),
        config = %args.config.display(),
        "clevod starting"
    );

    for failure in service.apply_saved(&cfg) {
        warn!("could not re-apply saved setting: {failure}");
    }

    if !args.no_poll {
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            loop {
                let poll = Arc::clone(&service);
                match tokio::task::spawn_blocking(move || poll.poll_fan()).await {
                    Ok(Err(err)) => warn!("fan poll failed: {err}"),
                    Err(err) => warn!("fan poll task failed: {err}"),
                    _ => {}
                }
                let Some(interval_ms) = fan_interval_ms else {
                    break;
                };
                tokio::time::sleep(Duration::from_millis(interval_ms)).await;
            }
        });
    }
    if is_p15 && !args.no_poll && fan_interval_ms.unwrap_or(u64::MAX) > DEFAULT_INTERVAL_MS {
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            loop {
                let sync = Arc::clone(&service);
                if let Err(err) = tokio::task::spawn_blocking(move || sync.sync_modes()).await {
                    warn!("mode sync task failed: {err}");
                }
                tokio::time::sleep(Duration::from_millis(DEFAULT_INTERVAL_MS)).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_the_affected_dmi_model() {
        assert!(is_p15_23("COLORFUL\n", "P15 23\n"));
        assert!(!is_p15_23("COLORFUL", "P15 24"));
        assert!(!is_p15_23("CLEVO", "P15 23"));
    }

    #[test]
    fn affected_model_has_no_recurring_sampling_by_default_and_clamps_opt_in() {
        assert_eq!(fan_poll_intervals(None, true, false), (None, 60_000));
        assert_eq!(
            fan_poll_intervals(NonZeroU64::new(2_000), true, false),
            (Some(60_000), 60_000)
        );
        assert_eq!(
            fan_poll_intervals(NonZeroU64::new(120_000), true, false),
            (Some(120_000), 120_000)
        );
        assert_eq!(fan_poll_intervals(None, false, false), (Some(2_000), 2_000));
        assert!(Args::try_parse_from(["clevod", "--interval-ms", "0"]).is_err());
    }

    #[test]
    fn direct_periods_allow_recurring_sampling_on_p15() {
        assert_eq!(fan_poll_intervals(None, true, true), (Some(2_000), 2_000));
        assert_eq!(
            fan_poll_intervals(NonZeroU64::new(3_000), true, true),
            (Some(3_000), 3_000)
        );
    }
}
