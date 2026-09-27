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
    use clevo_proto::command::{CMD_FAN_CURVE_WRITE, CMD_MAIN, SUB_FAN_MODE};
    use clevo_proto::constants::PAYLOAD_LEN;
    use clevo_proto::fan_curve::{FanCurve, FanPoint};
    use clevo_transport::{MockTransport, Transport, TransportError, TransportKind};
    use std::sync::{Arc, Mutex};

    fn mock(fixture: &str) -> Box<dyn clevo_transport::Transport> {
        Box::new(MockTransport::from_fixture_str(fixture).expect("fixture"))
    }

    const FIXTURE: &str = include_str!("../tests/fixtures/test.fixture");

    /// A recorded `(command, payload)` exchange.
    type RecordedCall = (u32, [u8; PAYLOAD_LEN]);

    #[derive(Clone, Default)]
    struct RecordingTransport {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
        fail_command: Option<u32>,
    }

    impl RecordingTransport {
        fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Transport for RecordingTransport {
        fn execute(
            &self,
            command: u32,
            payload: &[u8; PAYLOAD_LEN],
        ) -> Result<Vec<u8>, TransportError> {
            self.calls.lock().unwrap().push((command, *payload));
            if self.fail_command == Some(command) {
                return Err(TransportError::Io(format!("command {command} failed")));
            }
            Ok(Vec::new())
        }

        fn read_app_settings(
            &self,
            _page: u8,
            _offset: u16,
            _len: u16,
        ) -> Result<Vec<u8>, TransportError> {
            Err(TransportError::Unsupported("test transport".into()))
        }

        fn write_app_settings(
            &self,
            _page: u8,
            _offset: u16,
            _data: &[u8],
        ) -> Result<(), TransportError> {
            Err(TransportError::Unsupported("test transport".into()))
        }

        fn kind(&self) -> TransportKind {
            TransportKind::Mock
        }

        fn writable(&self) -> bool {
            true
        }
    }

    fn curve() -> FanCurve {
        let point = |temp, duty_pct| FanPoint { temp, duty_pct };
        FanCurve {
            cpu: [point(40, 20), point(55, 40), point(75, 70), point(100, 100)],
            gpu1: [point(40, 20), point(55, 40), point(75, 70), point(100, 100)],
            gpu2: [point(0, 0); 4],
        }
    }

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
    fn saved_curve_round_trips_through_config() {
        let transport = RecordingTransport::default();
        let service = Service::new(Box::new(transport.clone()));
        let expected = curve();

        service.set_curve(&expected).unwrap();

        let config = service.to_config();
        assert_eq!(config.schema_version, config::SCHEMA_VERSION);
        assert_eq!(config.fan_mode, Some(6));
        assert_eq!(config.fan_curve.map(|wire| wire.to_curve()), Some(expected));
    }

    #[test]
    fn saved_curve_is_written_before_custom_mode() {
        let transport = RecordingTransport::default();
        let service = Service::new(Box::new(transport.clone()));
        let config = config::Config {
            fan_mode: Some(6),
            fan_curve: Some(config::FanCurveWire::from_curve(&curve())),
            ..config::Config::default()
        };

        assert!(service.apply_saved(&config).is_empty());
        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, CMD_FAN_CURVE_WRITE.get());
        assert_eq!(calls[1].0, CMD_MAIN.get());
        assert_eq!(calls[1].1[3], SUB_FAN_MODE);
        assert_eq!(calls[1].1[0], 6);
    }

    #[test]
    fn saved_curve_is_not_rewritten_for_a_normal_fan_mode() {
        let transport = RecordingTransport::default();
        let service = Service::new(Box::new(transport.clone()));
        let config = config::Config {
            fan_mode: Some(8),
            fan_curve: Some(config::FanCurveWire::from_curve(&curve())),
            ..config::Config::default()
        };

        assert!(service.apply_saved(&config).is_empty());
        let calls = transport.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, CMD_MAIN.get());
        assert_eq!(calls[0].1[3], SUB_FAN_MODE);
        assert_eq!(calls[0].1[0], 8);
        assert_eq!(service.state().lock().unwrap().saved_curve, Some(curve()));
    }

    #[test]
    fn curve_only_restore_selects_custom_mode() {
        let transport = RecordingTransport::default();
        let service = Service::new(Box::new(transport.clone()));
        let config = config::Config {
            fan_curve: Some(config::FanCurveWire::from_curve(&curve())),
            ..config::Config::default()
        };

        assert!(service.apply_saved(&config).is_empty());
        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, CMD_FAN_CURVE_WRITE.get());
        assert_eq!(calls[1].0, CMD_MAIN.get());
        assert_eq!(calls[1].1[0], 6);
    }

    #[test]
    fn failed_curve_restore_does_not_select_custom_mode() {
        let transport = RecordingTransport {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_command: Some(CMD_FAN_CURVE_WRITE.get()),
        };
        let service = Service::new(Box::new(transport.clone()));
        let config = config::Config {
            fan_mode: Some(6),
            fan_curve: Some(config::FanCurveWire::from_curve(&curve())),
            ..config::Config::default()
        };

        let failures = service.apply_saved(&config);
        assert_eq!(transport.calls().len(), 1);
        assert!(failures.iter().any(|failure| failure.contains("fan_curve")));
        assert_eq!(service.state().lock().unwrap().fan_mode, None);
    }

    #[test]
    fn curve_json_is_well_formed() {
        let service = Service::new(mock(FIXTURE));
        let info = service.read_curve().unwrap();
        let json = dbus::curve_to_json(&info);
        assert!(json.starts_with("{\"fan_count\":2"));
        assert!(json.contains("\"cpu\":[[40,25],[60,36],[80,53],[100,100]]"));
    }

    /// A temp config path under a unique directory (no external crate needed).
    fn temp_config(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!(
                "clevod-persist-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ))
            .join("clevod.toml")
    }

    #[test]
    fn a_successful_curve_write_is_persisted() {
        let transport = RecordingTransport::default();
        let path = temp_config("curve");
        let service =
            Service::new(Box::new(transport.clone())).with_config_path(Some(path.clone()));

        service.set_curve(&curve()).unwrap();

        let saved = config::load(&path).expect("saved config");
        assert_eq!(saved.schema_version, config::SCHEMA_VERSION);
        assert_eq!(saved.fan_mode, Some(6));
        assert_eq!(saved.fan_curve.map(|wire| wire.to_curve()), Some(curve()));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_failed_curve_write_is_not_persisted() {
        let transport = RecordingTransport {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_command: Some(CMD_FAN_CURVE_WRITE.get()),
        };
        let path = temp_config("curve-fail");
        let service =
            Service::new(Box::new(transport.clone())).with_config_path(Some(path.clone()));

        assert!(service.set_curve(&curve()).is_err());
        assert!(!path.exists(), "a rejected write must not touch the file");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn fan_and_perf_mode_writes_are_persisted() {
        let transport = RecordingTransport::default();
        let path = temp_config("modes");
        let service =
            Service::new(Box::new(transport.clone())).with_config_path(Some(path.clone()));

        service.set_fan_mode("max").unwrap();
        service.set_perf_mode("performance").unwrap();

        let saved = config::load(&path).expect("saved config");
        assert_eq!(saved.fan_mode, Some(1));
        assert_eq!(saved.perf_mode, Some(2));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn persistence_preserves_fields_the_daemon_does_not_manage() {
        let transport = RecordingTransport::default();
        let path = temp_config("preserve");
        let service =
            Service::new(Box::new(transport.clone())).with_config_path(Some(path.clone()));

        // A file the user hand-edited with a TDP class and apply_on_start.
        let original = config::Config {
            cpu_tdp_class: Some("47W".to_string()),
            apply_on_start: false,
            ..config::Config::default()
        };
        config::save(&path, &original).unwrap();

        service.apply_saved(&original);
        service.set_fan_mode("quiet").unwrap();

        let saved = config::load(&path).expect("saved config");
        assert_eq!(saved.fan_mode, Some(8));
        assert_eq!(saved.cpu_tdp_class.as_deref(), Some("47W"));
        assert!(!saved.apply_on_start);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn replaying_saved_settings_does_not_rewrite_the_file() {
        let transport = RecordingTransport::default();
        let path = temp_config("replay");
        let service =
            Service::new(Box::new(transport.clone())).with_config_path(Some(path.clone()));

        let cfg = config::Config {
            fan_mode: Some(6),
            fan_curve: Some(config::FanCurveWire::from_curve(&curve())),
            ..config::Config::default()
        };
        config::save(&path, &cfg).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        assert!(service.apply_saved(&cfg).is_empty());

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "startup replay must not rewrite the file");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_service_without_a_path_never_writes() {
        // The default (mock / CLI) service has no config path: writes must
        // succeed without touching the filesystem.
        let transport = RecordingTransport::default();
        let service = Service::new(Box::new(transport.clone()));
        assert!(service.set_fan_mode("max").is_ok());
        assert_eq!(service.to_config().fan_mode, Some(1));
    }
}
