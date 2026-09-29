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
    use clevo_transport::{
        AcpiKeyboard, Color, KeyboardMode, MockKeyboard, MockTransport, Transport, TransportError,
        TransportKind,
    };
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

        /// The recorded calls that actually change hardware state.
        ///
        /// The daemon also *reads* the curve to snapshot the factory table, and
        /// these tests are about write ordering: filtering to the two write
        /// commands keeps them asserting the thing they name.
        fn writes(&self) -> Vec<RecordedCall> {
            self.calls()
                .into_iter()
                .filter(|(command, _)| {
                    *command == CMD_FAN_CURVE_WRITE.get() || *command == CMD_MAIN.get()
                })
                .collect()
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
    fn keyboard_writes_are_cached_and_persistable() {
        let service =
            Service::new(mock(FIXTURE)).with_keyboard(Some(Box::new(MockKeyboard::new())));
        let red = Color { r: 255, g: 0, b: 0 };
        service.set_keyboard_mode("static").unwrap();
        service.set_keyboard_brightness(2).unwrap();
        service.set_keyboard_zone("left", red).unwrap();
        service.set_keyboard_key(5, 19, red).unwrap();

        let saved = service.to_config().keyboard.expect("keyboard config");
        assert_eq!(saved.mode, "static");
        assert_eq!(saved.brightness, 2);
        assert!(saved.keys.iter().any(|key| key.row == 5 && key.col == 19));

        let snapshot = service.keyboard_snapshot().unwrap();
        assert_eq!(snapshot.mode, KeyboardMode::Static);
        assert_eq!(snapshot.keys[0][0], red);
        assert_eq!(snapshot.keys[5][19], red);
    }

    #[test]
    fn rgb15_keyboard_restore_writes_one_physical_zone() {
        let path = std::env::temp_dir().join(format!("clevo-daemon-rgb15-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "").unwrap();
        let keyboard = AcpiKeyboard::with_path(&path);
        let service = Service::new(mock(FIXTURE)).with_keyboard(Some(Box::new(keyboard)));
        let mut config = config::Config::default();
        config.keyboard = Some(config::KeyboardConfig {
            mode: "wave".into(),
            brightness: 3,
            keys: vec![
                config::KeyboardKeyWire {
                    row: 0,
                    col: 2,
                    color: config::KeyboardColorWire { r: 255, g: 0, b: 0 },
                },
                config::KeyboardKeyWire {
                    row: 0,
                    col: 8,
                    color: config::KeyboardColorWire { r: 0, g: 255, b: 0 },
                },
                config::KeyboardKeyWire {
                    row: 0,
                    col: 19,
                    color: config::KeyboardColorWire { r: 0, g: 0, b: 255 },
                },
            ],
        });

        assert!(service.apply_saved(&config).is_empty());
        let snapshot = service.keyboard_snapshot().unwrap();
        assert_eq!(snapshot.mode, KeyboardMode::Wave);
        assert_eq!(snapshot.brightness, 3);
        assert_eq!(snapshot.keys[0][2], Color { r: 255, g: 0, b: 0 });
        assert_eq!(snapshot.keys[0][8], Color { r: 255, g: 0, b: 0 });
        assert_eq!(snapshot.keys[0][19], Color { r: 255, g: 0, b: 0 });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mode wave\n");
        let _ = std::fs::remove_file(path);
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
        let calls = transport.writes();
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
        let calls = transport.writes();
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
        let calls = transport.writes();
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
        assert_eq!(
            transport.writes().len(),
            1,
            "no custom-mode write after a failed curve"
        );
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

    #[test]
    fn factory_curve_is_unknown_until_captured() {
        let service = Service::new(mock(FIXTURE));
        assert!(service.factory_curve().is_none());
    }

    #[test]
    fn capture_records_the_ec_curve_and_persists_it() {
        let path = temp_config("factory");
        let service = Service::new(mock(FIXTURE)).with_config_path(Some(path.clone()));

        service.capture_factory_curve();

        let expected = service.read_curve().unwrap().curve;
        assert_eq!(service.factory_curve(), Some(expected));
        let saved = config::load(&path).unwrap();
        assert_eq!(
            saved.factory_curve.map(|wire| wire.to_curve()),
            Some(expected),
            "the snapshot must survive a restart"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn capture_is_a_no_op_once_recorded() {
        let path = temp_config("factory-once");
        let service = Service::new(mock(FIXTURE)).with_config_path(Some(path.clone()));

        service.capture_factory_curve();
        let first = service.factory_curve();
        // A second call must not re-read (or overwrite) the stored snapshot.
        service.capture_factory_curve();
        assert_eq!(service.factory_curve(), first);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn capture_does_not_mislabel_a_migrated_user_curve_as_factory() {
        // A warm restart: the EC still holds the curve the daemon last saved, so
        // what it reports is the user's table, not the shipped one. Capturing it
        // would put a user curve behind 还原默认, which must not happen.
        let path = temp_config("factory-migrated");
        let service = Service::new(mock(FIXTURE)).with_config_path(Some(path.clone()));
        // The exact curve the fixture EC reports, as if the daemon had written it.
        let ec_curve = service.read_curve().unwrap().curve;
        let cfg = config::Config {
            fan_curve: Some(config::FanCurveWire::from_curve(&ec_curve)),
            ..config::Config::default()
        };

        service.apply_saved(&cfg);

        assert!(
            service.factory_curve().is_none(),
            "the EC's saved user curve must not be recorded as the factory default"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn capture_records_the_shipped_curve_after_a_cold_boot() {
        // A cold boot: the EC forgot the saved curve and reports the shipped
        // table, which differs from what was saved. That difference is the
        // signal that the shipped curve is available to capture.
        let path = temp_config("factory-cold");
        let service = Service::new(mock(FIXTURE)).with_config_path(Some(path.clone()));
        let cfg = config::Config {
            fan_curve: Some(config::FanCurveWire::from_curve(&curve())),
            ..config::Config::default()
        };

        service.apply_saved(&cfg);

        let expected = service.read_curve().unwrap().curve;
        assert_eq!(service.factory_curve(), Some(expected));
        assert_ne!(
            service.factory_curve(),
            Some(curve()),
            "the saved user curve must not be captured as the default"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_curve_write_captures_the_factory_curve_first() {
        // On a fresh service the EC still holds the shipped curve; writing a
        // custom one must snapshot it before overwriting.
        let path = temp_config("factory-write");
        let service = Service::new(mock(FIXTURE)).with_config_path(Some(path.clone()));

        service.set_curve(&curve()).unwrap();

        let expected = service.read_curve().unwrap().curve;
        assert_eq!(service.factory_curve(), Some(expected));
        // And it is not the curve we just wrote.
        assert_ne!(service.factory_curve(), Some(curve()));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
