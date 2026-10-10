use super::*;
use clevo_transport::error::TransportResult;
use clevo_transport::{MockTransport, TransportError, PAYLOAD_BYTES};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "clevo-doctor-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn put(&self, path: &str, contents: &str) {
        let path = self.0.join(path.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn system(&self, choice: TransportChoice) -> System {
        System::new(self.0.clone(), choice)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn check<'a>(report: &'a Report, id: &str) -> &'a Check {
    report
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

#[test]
fn optional_interfaces_are_skipped_and_rgb15_does_not_require_ite() {
    let fixture = Fixture::new();
    fixture.put("/sys/devices/platform/CLV0001:00/keyboard_rgb", "");
    fixture.put("/sys/class/power_supply/BAT0/type", "Battery\n");
    fixture.put("/sys/class/power_supply/BAT0/capacity", "83\n");
    fixture.put("/sys/class/power_supply/BAT0/status", "Discharging\n");
    let mut report = Report::new("Dbus");
    fixture
        .system(TransportChoice::Dbus)
        .collect(Path::new("/etc/clevo-cc/clevod.toml"), &mut report);
    assert_eq!(report.summary().fail, 0);
    assert_eq!(check(&report, "keyboard.rgb15").status, Status::Pass);
    assert_eq!(check(&report, "keyboard.ite").status, Status::Skip);
    assert_eq!(check(&report, "acpi_call.interface").status, Status::Skip);
    assert_eq!(check(&report, "battery.BAT0.capacity").status, Status::Pass);
    assert_eq!(
        check(&report, "battery.BAT0.charge_control_end_threshold").status,
        Status::Skip
    );
    assert_eq!(check(&report, "platform_profile").status, Status::Skip);
    assert_eq!(check(&report, "config.schema").status, Status::Skip);
}

#[test]
fn missing_driver_keeps_later_configuration_and_power_checks() {
    let fixture = Fixture::new();
    fixture.put("/etc/clevo-cc/clevod.toml", "perf_mode = 2\n");
    let mut report = Report::new("Driver");
    fixture
        .system(TransportChoice::Driver)
        .collect(Path::new("/etc/clevo-cc/clevod.toml"), &mut report);
    assert!(report.summary().fail > 0);
    assert_eq!(check(&report, "driver.binding").status, Status::Fail);
    assert_eq!(check(&report, "driver.fans").status, Status::Fail);
    assert_eq!(check(&report, "battery").status, Status::Skip);
    assert_eq!(check(&report, "config.schema").status, Status::Pass);
}

#[test]
fn kernel_configuration_distinguishes_disabled_from_unknown() {
    let fixture = Fixture::new();
    fixture.put("/proc/sys/kernel/osrelease", "test-kernel\n");
    for (text, status) in [
        (None, Status::Warn),
        (Some("# CONFIG_ACPI is not set\n"), Status::Fail),
        (Some("CONFIG_ACPI=y\n"), Status::Pass),
    ] {
        if let Some(text) = text {
            fixture.put("/boot/config-test-kernel", text);
        }
        let mut report = Report::new("Driver");
        fixture.system(TransportChoice::Driver).kernel(&mut report);
        assert_eq!(check(&report, "kernel.acpi").status, status);
    }
}

#[test]
fn acpi_status_accepts_decimal_and_hex_but_rejects_disabled_device() {
    let fixture = Fixture::new();
    for (text, fail) in [
        ("15\n", false),
        ("0x0f\n", false),
        ("0\n", true),
        ("malformed\n", true),
    ] {
        fixture.put("/sys/bus/acpi/devices/CLV0001:00/status", text);
        let mut report = Report::new("Driver");
        fixture
            .system(TransportChoice::Driver)
            .collect(Path::new("/missing.toml"), &mut report);
        assert_eq!(
            report
                .checks
                .iter()
                .any(|check| check.id == "acpi.device_status" && check.status == Status::Fail),
            fail
        );
    }
}

#[test]
fn config_parser_handles_legacy_invalid_and_newer_schemas_without_writes() {
    let fixture = Fixture::new();
    let cases = [
        (
            "schema_version = 1\nperf_mode = 2\n".to_string(),
            Status::Pass,
        ),
        ("perf_mode = \"wrong type\"\n".to_string(), Status::Fail),
        ("invalid = [\n".to_string(), Status::Fail),
        (
            format!("schema_version = {}\n", clevod::config::SCHEMA_VERSION + 1),
            Status::Fail,
        ),
    ];
    for (text, expected) in cases {
        fixture.put("/etc/clevo-cc/clevod.toml", &text);
        let mut report = Report::new("Dbus");
        fixture
            .system(TransportChoice::Dbus)
            .installation(Path::new("/etc/clevo-cc/clevod.toml"), &mut report);
        assert_eq!(check(&report, "config.schema").status, expected);
        assert_eq!(
            fs::read_to_string(fixture.0.join("etc/clevo-cc/clevod.toml")).unwrap(),
            text
        );
        assert!(!fixture.0.join("etc/clevo-cc/clevod.toml.tmp").exists());
    }
}

#[test]
fn active_group_membership_uses_process_credentials_not_group_member_list() {
    let fixture = Fixture::new();
    fixture.put("/etc/group", "clevo-cc:x:987:user\n");
    for (groups, expected) in [("1000", Status::Skip), ("1000 987", Status::Pass)] {
        fixture.put("/proc/self/status", &format!("Name:\tprivate\nUid:\t1000 1000 1000 1000\nGid:\t1000 1000 1000 1000\nGroups:\t{groups}\n"));
        let mut report = Report::new("Driver");
        fixture
            .system(TransportChoice::Driver)
            .installation(Path::new("/missing.toml"), &mut report);
        assert_eq!(check(&report, "permissions.group").status, expected);
        assert!(!check(&report, "permissions.identity")
            .detail
            .contains("private"));
    }
}

#[test]
fn hidraw_probe_matches_vendor_product_without_opening_the_device() {
    let fixture = Fixture::new();
    fixture.put(
        "/sys/class/hidraw/hidraw7/device/uevent",
        "HID_ID=0003:0000048D:00008910\n",
    );
    fixture.put(
        "/sys/class/hidraw/hidraw8/device/uevent",
        "HID_ID=0003:0000048D:00008911\n",
    );
    fixture.put("/dev/hidraw7", "unchanged");
    let mut report = Report::new("Driver");
    fixture
        .system(TransportChoice::Driver)
        .keyboard(&mut report);
    assert_eq!(check(&report, "keyboard.ite").status, Status::Pass);
    assert_eq!(
        report
            .checks
            .iter()
            .filter(|check| check.id == "keyboard.ite")
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("dev/hidraw7")).unwrap(),
        "unchanged"
    );
}

struct QueryOnly {
    inner: MockTransport,
    queries: Mutex<Vec<u32>>,
}

impl Transport for QueryOnly {
    fn execute(&self, command: u32, payload: &[u8; PAYLOAD_BYTES]) -> TransportResult<Vec<u8>> {
        assert!(matches!(command, 12 | 13 | 122 | 96 | 16 | 60 | 82));
        self.queries.lock().unwrap().push(command);
        if command == 12 {
            Err(TransportError::Io("injected firmware error".into()))
        } else {
            self.inner.execute(command, payload)
        }
    }

    fn read_app_settings(&self, _: u8, _: u16, _: u16) -> TransportResult<Vec<u8>> {
        Err(TransportError::Unsupported("AppSettings skipped".into()))
    }

    fn write_app_settings(&self, _: u8, _: u16, _: &[u8]) -> TransportResult<()> {
        panic!("doctor attempted a write")
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Driver
    }
}

#[test]
fn failed_hardware_query_does_not_hide_gcmd_and_never_uses_write_commands() {
    let transport = QueryOnly {
        inner: MockTransport::from_fixture_str(include_str!("../../../../fixtures/gcmd.fixture"))
            .unwrap(),
        queries: Mutex::new(Vec::new()),
    };
    let mut report = Report::new("Driver");
    probe_transport(&transport, &mut report);
    assert_eq!(check(&report, "hardware.fan_status").status, Status::Fail);
    assert_eq!(check(&report, "hardware.capabilities").status, Status::Pass);
    assert!(check(&report, "hardware.capabilities")
        .detail
        .contains("GCMD"));
    assert_eq!(
        *transport.queries.lock().unwrap(),
        [12, 13, 122, 96, 16, 60, 82]
    );
}

#[test]
fn json_is_complete_and_exit_status_depends_on_failures_only() {
    let mut report = Report::new("Driver");
    report.add("optional", Status::Skip, "unsupported", "");
    report.add("warning", Status::Warn, "permissions", "check permissions");
    let mut out = Vec::new();
    assert!(report.write(true, &mut out).unwrap());
    report.add("broken", Status::Fail, "firmware error", "read logs");
    out.clear();
    assert!(!report.write(true, &mut out).unwrap());
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["report_version"], 1);
    assert_eq!(value["changes_made"], false);
    assert_eq!(value["summary"]["fail"], 1);
    assert_eq!(value["checks"][2]["suggestion"], "read logs");
}
