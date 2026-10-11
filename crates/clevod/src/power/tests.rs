use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn standard_percentage_is_submitted_in_one_write_without_a_separate_newline() {
    #[derive(Default)]
    struct Store(Vec<Vec<u8>>);
    impl Write for Store {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.push(bytes.to_vec());
            if std::str::from_utf8(bytes)
                .unwrap()
                .trim()
                .parse::<u8>()
                .is_err()
            {
                return Err(std::io::Error::from_raw_os_error(22));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut store = Store::default();
    write_percent(&mut store, 80).unwrap();
    assert_eq!(store.0, vec![b"80\n".to_vec()]);
}

struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "clevo-power-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn put(&self, path: &str, value: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }

    fn battery(&self, start: Option<u8>, end: Option<u8>) {
        self.put("class/power_supply/BAT0/type", "Battery\n");
        self.put("class/power_supply/BAT0/present", "1\n");
        if let Some(start) = start {
            self.put(
                "class/power_supply/BAT0/charge_control_start_threshold",
                &format!("{start}\n"),
            );
        }
        if let Some(end) = end {
            self.put(
                "class/power_supply/BAT0/charge_control_end_threshold",
                &format!("{end}\n"),
            );
        }
    }

    fn power(&self) -> Power {
        Power::with_sysfs(&self.0)
    }

    fn native(&self, enabled: bool, start: u8, end: u8) {
        self.put("class/power_supply/BAT0/device/path", "\\_SB_.BAT0\n");
        self.put("devices/platform/CLV0001:00/flexicharger", &format!(
            "battery=\\_SB_.BAT0\nenabled={}\nstart={start}\nend={end}\nstart_values=40 50 60 70 80 95\nend_values=60 70 80 90 100\n", u8::from(enabled)));
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn reads_battery_ac_health_without_mixing_energy_and_charge_units() {
    let tree = Tree::new();
    tree.battery(None, None);
    tree.put("class/power_supply/AC/type", "Mains\n");
    tree.put("class/power_supply/AC/online", "1\n");
    tree.put("class/power_supply/BAT0/capacity", "67\n");
    tree.put("class/power_supply/BAT0/status", "Charging\n");
    tree.put("class/power_supply/BAT0/energy_full", "40000000\n");
    tree.put("class/power_supply/BAT0/charge_full_design", "5000000\n");
    let first = tree.power().snapshot();
    assert_eq!(first.ac_online, Some(true));
    assert_eq!(first.batteries[0].capacity, Some(67));
    assert_eq!(first.batteries[0].health_percent, None);
    assert_eq!(first.batteries[0].thresholds, None);
    tree.put("class/power_supply/BAT0/energy_full_design", "50000000\n");
    assert_eq!(
        tree.power().snapshot().batteries[0].health_percent,
        Some(80.0)
    );
}

#[test]
fn malformed_values_stay_unknown_instead_of_zero() {
    let tree = Tree::new();
    tree.battery(None, None);
    tree.put("class/power_supply/BAT0/capacity", "255\n");
    tree.put("class/power_supply/BAT0/cycle_count", "bad\n");
    let snapshot = tree.power().snapshot();
    assert_eq!(snapshot.batteries[0].capacity, None);
    assert_eq!(snapshot.batteries[0].cycle_count, None);
    assert_eq!(snapshot.batteries[0].errors.len(), 2);
    assert_eq!(snapshot.ac_online, None);
}

#[test]
fn rejects_unsupported_absent_invalid_and_traversal_without_creating_files() {
    let tree = Tree::new();
    tree.battery(None, None);
    let power = tree.power();
    for name in ["BAT0", "../BAT0", "/tmp/BAT0", "BAT0/other"] {
        assert!(power
            .set_thresholds(
                name,
                ChargeThresholds {
                    enabled: None,
                    start: None,
                    end: 80
                }
            )
            .is_err());
    }
    assert!(!tree
        .0
        .join("class/power_supply/BAT0/charge_control_end_threshold")
        .exists());
    tree.battery(Some(40), Some(80));
    for value in [
        ChargeThresholds {
            enabled: None,
            start: Some(80),
            end: 80,
        },
        ChargeThresholds {
            enabled: None,
            start: None,
            end: 0,
        },
        ChargeThresholds {
            enabled: None,
            start: None,
            end: 101,
        },
    ] {
        assert!(power.set_thresholds("BAT0", value).is_err());
    }
    tree.put("class/power_supply/BAT0/present", "0\n");
    assert!(power
        .set_thresholds(
            "BAT0",
            ChargeThresholds {
                enabled: None,
                start: Some(50),
                end: 90
            }
        )
        .is_err());
    assert_eq!(power.snapshot().batteries[0].thresholds.unwrap().end, 80);
    tree.put("class/power_supply/BAT0/present", "unknown\n");
    assert!(power
        .set_thresholds(
            "BAT0",
            ChargeThresholds {
                enabled: None,
                start: Some(50),
                end: 90
            }
        )
        .is_err());
    assert_eq!(power.snapshot().batteries[0].thresholds.unwrap().end, 80);
}

#[test]
fn applies_end_only_and_both_directions_of_charge_window() {
    let tree = Tree::new();
    tree.battery(None, Some(100));
    let power = tree.power();
    assert_eq!(
        power
            .set_thresholds(
                "BAT0",
                ChargeThresholds {
                    enabled: None,
                    start: None,
                    end: 80
                }
            )
            .unwrap()
            .end,
        80
    );
    assert!(power
        .set_thresholds(
            "BAT0",
            ChargeThresholds {
                enabled: None,
                start: Some(40),
                end: 80
            }
        )
        .is_err());
    tree.battery(Some(40), Some(80));
    for value in [
        ChargeThresholds {
            enabled: None,
            start: Some(85),
            end: 95,
        },
        ChargeThresholds {
            enabled: None,
            start: Some(20),
            end: 30,
        },
    ] {
        assert_eq!(power.set_thresholds("BAT0", value).unwrap(), value);
        assert_eq!(power.snapshot().batteries[0].thresholds, Some(value));
    }
    assert!(power
        .set_thresholds(
            "BAT0",
            ChargeThresholds {
                enabled: None,
                start: None,
                end: 10
            }
        )
        .is_err());
}

#[test]
fn rejects_clamped_readback_and_restores_previous_window() {
    let tree = Tree::new();
    tree.battery(Some(40), Some(80));
    let power = tree.power();
    let mut calls = 0;
    let result = power.set_thresholds_using(
        "BAT0",
        ChargeThresholds {
            enabled: None,
            start: Some(50),
            end: 90,
        },
        |path, from, to| {
            calls += 1;
            if calls == 1 {
                write_window(
                    path,
                    from,
                    ChargeThresholds {
                        enabled: None,
                        start: to.start,
                        end: 85,
                    },
                )
            } else {
                write_window(path, from, to)
            }
        },
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("previous thresholds restored"));
    assert_eq!(calls, 2);
    assert_eq!(
        power.snapshot().batteries[0].thresholds,
        Some(ChargeThresholds {
            enabled: None,
            start: Some(40),
            end: 80
        })
    );
}

#[test]
fn partial_write_failure_restores_and_reports_restore_failure_explicitly() {
    let tree = Tree::new();
    tree.battery(Some(40), Some(80));
    let power = tree.power();
    let mut calls = 0;
    let result = power.set_thresholds_using(
        "BAT0",
        ChargeThresholds {
            enabled: None,
            start: Some(50),
            end: 90,
        },
        |path, from, to| {
            calls += 1;
            if calls == 1 {
                write_window(
                    path,
                    from,
                    ChargeThresholds {
                        enabled: None,
                        start: to.start,
                        end: from.end,
                    },
                )?;
                Err("injected end failure".into())
            } else {
                write_window(path, from, to)
            }
        },
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("previous thresholds restored"));
    assert_eq!(
        power.snapshot().batteries[0].thresholds.unwrap().start,
        Some(40)
    );
    let mut calls = 0;
    let result = power.set_thresholds_using(
        "BAT0",
        ChargeThresholds {
            enabled: None,
            start: Some(50),
            end: 90,
        },
        |_, _, _| {
            calls += 1;
            Err(format!("injected failure {calls}"))
        },
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("restoring previous thresholds failed"));
}

#[test]
fn prefers_clevo_provider_over_aggregate_platform_profile() {
    let tree = Tree::new();
    tree.put("firmware/acpi/platform_profile", "quiet\n");
    tree.put(
        "firmware/acpi/platform_profile_choices",
        "quiet balanced performance\n",
    );
    tree.put(
        "class/platform-profile/platform-profile-0/name",
        "clevo-cc\n",
    );
    tree.put(
        "class/platform-profile/platform-profile-0/profile",
        "performance\n",
    );
    tree.put(
        "class/platform-profile/platform-profile-0/choices",
        "low-power quiet balanced performance\n",
    );
    let profile = tree.power().snapshot().platform_profile;
    assert_eq!(profile.current.as_deref(), Some("performance"));
    assert_eq!(profile.driver.as_deref(), Some("clevo-cc"));
    assert_eq!(profile.choices.len(), 4);
}

#[test]
fn service_persists_and_replays_only_successful_thresholds_and_honors_apply_on_start() {
    let tree = Tree::new();
    tree.battery(Some(40), Some(80));
    let mock = clevo_transport::MockTransport::from_fixture_str("").unwrap();
    let path = tree.0.join("config/clevod.toml");
    let service = crate::Service::new(Box::new(mock))
        .with_power(tree.power())
        .with_config_path(Some(path.clone()));
    let target = ChargeThresholds {
        enabled: None,
        start: Some(50),
        end: 90,
    };
    service
        .set_charge_thresholds("BAT0", target.start, target.end)
        .unwrap();
    let saved = crate::config::load(&path).unwrap();
    assert_eq!(saved.charge_thresholds["BAT0"], target);
    let bytes = fs::read(&path).unwrap();
    assert!(service.set_charge_thresholds("BAT0", Some(95), 80).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    tree.battery(Some(40), Some(80));
    assert!(service.apply_saved(&saved).is_empty());
    assert_eq!(
        service.power_snapshot().batteries[0].thresholds,
        Some(target)
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    tree.battery(Some(40), Some(80));
    let disabled = crate::config::Config {
        apply_on_start: false,
        ..saved
    };
    service.apply_saved(&disabled);
    assert_eq!(
        service.power_snapshot().batteries[0]
            .thresholds
            .unwrap()
            .end,
        80
    );
}

#[test]
fn desktop_mode_changes_are_observed_and_persisted_even_when_fan_polling_fails() {
    use clevo_transport::{Transport, TransportError, TransportKind, PAYLOAD_BYTES};
    use std::sync::atomic::AtomicU8;
    use std::sync::Arc;
    struct ExternalMode(Arc<AtomicU8>);
    impl Transport for ExternalMode {
        fn kind(&self) -> TransportKind {
            TransportKind::Driver
        }
        fn execute(
            &self,
            command: u32,
            _: &[u8; PAYLOAD_BYTES],
        ) -> Result<Vec<u8>, TransportError> {
            assert_eq!(
                command, 12,
                "polling must not write performance mode or query capabilities"
            );
            Err(TransportError::Io("injected fan failure".into()))
        }
        fn current_modes(&self) -> Result<(Option<u8>, Option<u8>), TransportError> {
            Ok((None, Some(self.0.load(Ordering::SeqCst))))
        }
        fn read_app_settings(&self, _: u8, _: u16, _: u16) -> Result<Vec<u8>, TransportError> {
            unreachable!()
        }
        fn write_app_settings(&self, _: u8, _: u16, _: &[u8]) -> Result<(), TransportError> {
            panic!("unexpected hardware write")
        }
    }
    let tree = Tree::new();
    let path = tree.0.join("config.toml");
    let baseline = crate::config::Config {
        perf_mode: Some(3),
        ..Default::default()
    };
    crate::config::save(&path, &baseline).unwrap();
    let current = Arc::new(AtomicU8::new(2));
    let service = crate::Service::new(Box::new(ExternalMode(current.clone())))
        .with_power(tree.power())
        .with_config_path(Some(path.clone()));
    service.set_baseline(&baseline);
    assert!(service.poll_fan().is_err());
    assert_eq!(service.state().lock().unwrap().perf_mode, Some(2));
    assert_eq!(crate::config::load(&path).unwrap().perf_mode, Some(2));
    fs::write(&path, "# unchanged-mode marker\n").unwrap();
    assert!(service.poll_fan().is_err());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "# unchanged-mode marker\n"
    );
    current.store(3, Ordering::SeqCst);
    assert!(service.poll_fan().is_err());
    assert_eq!(crate::config::load(&path).unwrap().perf_mode, Some(3));
}

#[test]
fn native_control_is_bound_to_firmware_battery_and_standard_attributes_take_precedence() {
    let tree = Tree::new();
    tree.battery(None, None);
    tree.native(false, 95, 100);
    let snapshot = tree.power().snapshot();
    let battery = &snapshot.batteries[0];
    assert_eq!(battery.thresholds.unwrap().enabled, Some(false));
    assert_eq!(
        battery.charge_control.as_ref().unwrap().start_values,
        vec![40, 50, 60, 70, 80, 95]
    );
    assert!(battery.start_supported && battery.end_supported);
    tree.put("class/power_supply/BAT0/device/path", "\\_SB_.BAT1\n");
    assert!(!tree.power().snapshot().batteries[0].end_supported);
    let request = ChargeThresholds {
        enabled: Some(true),
        start: Some(40),
        end: 80,
    };
    assert!(matches!(
        tree.power().set_thresholds("BAT0", request),
        Err(PowerError::Unsupported(_))
    ));
    tree.native(false, 95, 100);
    tree.battery(Some(20), Some(70));
    let snapshot = tree.power().snapshot();
    assert_eq!(snapshot.batteries[0].thresholds.unwrap().start, Some(20));
    assert!(snapshot.batteries[0].charge_control.is_none());
    assert!(matches!(
        tree.power().set_thresholds("BAT0", request),
        Err(PowerError::Unsupported(_))
    ));
}

#[test]
fn native_rejects_malformed_replies_and_unsupported_discrete_values_without_writing() {
    let tree = Tree::new();
    tree.battery(None, None);
    tree.native(false, 95, 100);
    let native_path = tree.0.join("devices/platform/CLV0001:00/flexicharger");
    let original = fs::read_to_string(&native_path).unwrap();
    let target = ChargeThresholds {
        enabled: Some(true),
        start: Some(41),
        end: 80,
    };
    assert!(matches!(
        tree.power().set_thresholds("BAT0", target),
        Err(PowerError::Invalid(_))
    ));
    assert_eq!(fs::read_to_string(&native_path).unwrap(), original);
    for malformed in [
        original.replace("enabled=0", "enabled=2"),
        original.replace("start_values=40 50 60 70 80 95", "start_values="),
        original.replace("start_values=40 50", "start_values=50 40"),
        original.replace("end_values=60", "end_values=101"),
        original.replace("start=95", "start=100"),
        original.replace("start=95", "start=94"),
        original.replace("battery=\\_SB_.BAT0", "battery="),
        format!("{original}end=80\n"),
    ] {
        fs::write(&native_path, malformed).unwrap();
        let snapshot = tree.power().snapshot();
        assert!(!snapshot.batteries[0].end_supported);
        assert!(snapshot.errors[0].contains("FlexiCharger"));
    }
}

#[test]
fn native_write_enables_and_disable_preserves_window_and_failed_confirmation_restores_state() {
    let tree = Tree::new();
    tree.battery(None, None);
    tree.native(false, 95, 100);
    let battery = tree.0.join("class/power_supply/BAT0");
    let target = ChargeThresholds {
        enabled: None,
        start: Some(40),
        end: 80,
    };
    let power = tree.power();
    let applied = power
        .set_native_using(&battery, target, |_, value| {
            tree.native(value.enabled.unwrap(), value.start.unwrap(), value.end);
            Ok(())
        })
        .unwrap();
    assert_eq!(applied.enabled, Some(true));
    let off = ChargeThresholds {
        enabled: Some(false),
        ..applied
    };
    assert_eq!(
        power
            .set_native_using(&battery, off, |_, value| {
                tree.native(value.enabled.unwrap(), value.start.unwrap(), value.end);
                Ok(())
            })
            .unwrap(),
        off
    );
    let mut calls = 0;
    let err = power
        .set_native_using(&battery, target, |_, value| {
            calls += 1;
            tree.native(
                value.enabled.unwrap(),
                value.start.unwrap(),
                if calls == 1 { 90 } else { value.end },
            );
            Ok(())
        })
        .unwrap_err();
    assert_eq!(calls, 2);
    assert!(err.to_string().contains("enable state restored"));
    assert_eq!(power.snapshot().batteries[0].thresholds, Some(off));
}

#[test]
fn native_enable_state_round_trips_in_config_and_old_windows_default_to_unspecified() {
    let old = crate::config::parse(
        "schema_version = 7\n[charge_thresholds.BAT0]\nstart = 40\nend = 80\n",
    )
    .unwrap();
    assert_eq!(old.charge_thresholds["BAT0"].enabled, None);
    let mut config = old;
    config.charge_thresholds.get_mut("BAT0").unwrap().enabled = Some(false);
    assert_eq!(
        crate::config::parse(&crate::config::to_toml(&config).unwrap()).unwrap(),
        config
    );
}

#[test]
fn rejected_native_write_with_original_readback_does_not_claim_restore_failed() {
    let tree = Tree::new();
    tree.battery(None, None);
    tree.native(false, 95, 100);
    let battery = tree.0.join("class/power_supply/BAT0");
    let mut writes = 0;
    let error = tree
        .power()
        .set_native_using(
            &battery,
            ChargeThresholds {
                enabled: Some(true),
                start: Some(40),
                end: 80,
            },
            |_, _| {
                writes += 1;
                Err("Invalid argument (os error 22)".into())
            },
        )
        .unwrap_err();
    assert_eq!(writes, 1);
    let message = error.to_string();
    assert!(message.contains("Invalid argument"));
    assert!(message.contains("readback confirms previous thresholds and enable state"));
    assert!(!message.contains("restoring previous thresholds failed"));
    assert_eq!(
        tree.power().snapshot().batteries[0]
            .thresholds
            .unwrap()
            .enabled,
        Some(false)
    );
}
