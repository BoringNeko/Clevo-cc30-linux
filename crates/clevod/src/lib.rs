//! `clevod` — the privileged Clevo control-center daemon.
//!
//! The daemon is the **only** long-lived process that talks to the hardware. It
//! polls the EC through a [`clevo_transport::Transport`], caches the last known
//! state with explicit freshness, persists user choices, and exposes everything
//! over the system D-Bus (`org.clevo.CC`). CLI and UI must go through D-Bus
//! rather than reaching the hardware directly.
//!
//! Module map:
//! * [`config`]  — versioned TOML persistence (pure + `load`/`save`).
//! * [`state`]   — cached state model with `fresh`/`stale`/`unknown`.
//! * [`service`] — poll/read/write control flow, transport-agnostic.
//! * [`dbus`]    — the `org.clevo.CC` interface (thin layer over [`service`]).
//!
//! Everything except [`dbus`] and the binary is unit tested without a bus.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod dbus;
pub mod policy;
pub mod service;
pub mod state;

pub use service::{fan_mode_name, fan_mode_value, perf_mode_name, perf_mode_value, Service};

/// The well-known D-Bus name the daemon claims.
pub const DBUS_NAME: &str = "org.clevo.CC";

/// Object path of the daemon's single managed object.
pub const DBUS_PATH: &str = "/org/clevo/CC";

/// D-Bus interface name.
pub const DBUS_INTERFACE: &str = "org.clevo.CC";

#[cfg(test)]
mod tests {
    use super::*;
    use clevo_transport::MockTransport;

    fn mock(fixture: &str) -> Box<dyn clevo_transport::Transport> {
        Box::new(MockTransport::from_fixture_str(fixture).expect("fixture"))
    }

    const FIXTURE: &str = include_str!("../tests/fixtures/test.fixture");

    #[test]
    fn mode_name_helpers_round_trip() {
        for name in ["auto", "max", "maxq", "quiet"] {
            let v = fan_mode_value(name).unwrap();
            assert_eq!(fan_mode_name(v), Some(name));
        }
        for name in ["quiet", "pwrsaving", "performance", "entertainment"] {
            let v = perf_mode_value(name).unwrap();
            assert_eq!(perf_mode_name(v), Some(name));
        }
    }

    #[test]
    fn unknown_modes_are_rejected() {
        assert!(fan_mode_value("bogus").is_none());
        assert!(perf_mode_value("bogus").is_none());
    }

    #[test]
    fn service_applies_and_records_modes() {
        let service = Service::new(mock(FIXTURE));
        assert_eq!(service.set_fan_mode("max").unwrap(), 1);
        assert_eq!(service.set_perf_mode("performance").unwrap(), 2);
        let cfg = service.to_config();
        assert_eq!(cfg.fan_mode, Some(1));
        assert_eq!(cfg.perf_mode, Some(2));
    }

    #[test]
    fn service_polls_and_caches_fan_state() {
        let service = Service::new(mock(FIXTURE));
        service.poll_fan().unwrap();
        let state = service.state();
        let state = state.lock().unwrap();
        assert_eq!(state.fan.freshness, state::Freshness::Fresh);
        assert_eq!(state.fan.cpu.rpm, 4667);
        assert!(!state.fan.gpu2.available);
    }

    #[test]
    fn service_rejects_unknown_mode_before_io() {
        let service = Service::new(mock(FIXTURE));
        assert!(matches!(
            service.set_fan_mode("nope"),
            Err(service::ServiceError::UnknownMode(_))
        ));
    }

    #[test]
    fn saved_modes_are_reapplied() {
        let service = Service::new(mock(FIXTURE));
        let cfg = config::Config {
            fan_mode: Some(8),
            perf_mode: Some(0),
            ..config::Config::default()
        };
        assert!(service.apply_saved(&cfg).is_empty());
        assert_eq!(service.state().lock().unwrap().fan_mode, Some(8));
        assert_eq!(service.state().lock().unwrap().perf_mode, Some(0));
    }

    #[test]
    fn curve_json_is_well_formed() {
        let service = Service::new(mock(FIXTURE));
        let info = service.read_curve().unwrap();
        let json = dbus::curve_to_json(&info);
        assert!(json.starts_with("{\"fan_count\":2"));
        assert!(json.contains("\"cpu\":[[40,25],[60,36],[80,53],[100,100]]"));
    }
}
