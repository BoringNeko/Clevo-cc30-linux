//! Best-effort, read-only diagnostics. A failed probe never hides later checks.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clevo_transport::{Transport, TransportKind};
use serde::Serialize;

use crate::dbus::DbusClient;
use crate::{commands, Cli, CliError, Commands, TransportChoice};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Pass,
    Warn,
    Fail,
    Skip,
}

#[cfg(all(test, target_os = "linux"))]
mod tests;

#[derive(Debug, Serialize)]
struct Check {
    id: String,
    status: Status,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<String>,
}

#[derive(Default, Serialize)]
struct Summary {
    pass: usize,
    warn: usize,
    fail: usize,
    skip: usize,
}

#[derive(Serialize)]
struct Report {
    report_version: u8,
    transport: String,
    os: &'static str,
    arch: &'static str,
    changes_made: bool,
    checks: Vec<Check>,
}

impl Report {
    fn new(transport: impl Into<String>) -> Self {
        Self {
            report_version: 1,
            transport: transport.into(),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            changes_made: false,
            checks: Vec::new(),
        }
    }

    fn add(
        &mut self,
        id: impl Into<String>,
        status: Status,
        detail: impl Into<String>,
        suggestion: &str,
    ) {
        self.checks.push(Check {
            id: id.into(),
            status,
            detail: detail.into(),
            suggestion: (!suggestion.is_empty()).then(|| suggestion.into()),
        });
    }

    fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for check in &self.checks {
            match check.status {
                Status::Pass => summary.pass += 1,
                Status::Warn => summary.warn += 1,
                Status::Fail => summary.fail += 1,
                Status::Skip => summary.skip += 1,
            }
        }
        summary
    }

    fn write(&self, json: bool, out: &mut dyn Write) -> Result<bool, CliError> {
        let summary = self.summary();
        if json {
            let mut value = serde_json::to_value(self).map_err(io::Error::other)?;
            value["summary"] = serde_json::to_value(&summary).map_err(io::Error::other)?;
            serde_json::to_writer_pretty(&mut *out, &value).map_err(io::Error::other)?;
            writeln!(out)?;
        } else {
            writeln!(out, "clevo-cc doctor")?;
            writeln!(out, "transport    : {}", self.transport)?;
            writeln!(out, "os / arch    : {} / {}", self.os, self.arch)?;
            for check in &self.checks {
                writeln!(out, "[{:?}] {}: {}", check.status, check.id, check.detail)?;
                if let Some(suggestion) = &check.suggestion {
                    writeln!(out, "  suggestion: {suggestion}")?;
                }
            }
            writeln!(
                out,
                "summary: {} pass, {} warn, {} fail, {} skip",
                summary.pass, summary.warn, summary.fail, summary.skip
            )?;
            writeln!(out, "no changes were made.")?;
        }
        Ok(summary.fail == 0)
    }
}

/// Run diagnostics before normal command dispatch. Returns false for failures;
/// warnings and optional unsupported features do not produce a failure exit.
pub fn run(cli: &Cli, out: &mut dyn Write) -> Result<bool, CliError> {
    let Commands::Doctor { config, json } = &cli.command else {
        return Err(CliError::Invalid("expected doctor command".into()));
    };
    let mut report = Report::new(format!("{:?}", cli.transport));
    if cli.transport == TransportChoice::Mock {
        report.add(
            "system",
            Status::Skip,
            "offline mock: host hardware, services and configuration are not probed",
            "Use --transport dbus doctor to diagnose the installed system.",
        );
    } else {
        System::new(PathBuf::from("/"), cli.transport).collect(config, &mut report);
        match DbusClient::diagnostics(cli.dbus_session) {
            Ok(client) => probe_dbus(&client, cli.transport == TransportChoice::Dbus, &mut report),
            Err(error) => report.add("dbus.connection", if cli.transport == TransportChoice::Dbus { Status::Fail } else { Status::Warn }, error.to_string(), "Check the selected bus and run: systemctl status clevod; journalctl -u clevod -b --no-pager. For a test daemon use --dbus-session."),
        }
    }
    if cli.transport != TransportChoice::Dbus {
        match crate::build_transport(cli) {
            Ok(transport) => probe_transport(transport.as_ref(), &mut report),
            Err(error) => report.add(
                "transport",
                Status::Fail,
                error.to_string(),
                "Check --fixture and its contents for the mock backend.",
            ),
        }
    }
    report.write(*json, out)
}

/// Diagnose an already constructed transport, including offline fixtures.
pub fn run_transport(transport: &dyn Transport, out: &mut dyn Write) -> Result<(), CliError> {
    let choice = match transport.kind() {
        TransportKind::Mock => TransportChoice::Mock,
        TransportKind::Driver => TransportChoice::Driver,
        TransportKind::AcpiCall => TransportChoice::AcpiCall,
    };
    let mut report = Report::new(format!("{:?}", choice));
    if choice != TransportChoice::Mock {
        System::new(PathBuf::from("/"), choice)
            .collect(&clevod::config::default_path(), &mut report);
    }
    probe_transport(transport, &mut report);
    report.write(false, out).map(|_| ())
}

/// Diagnose an already connected daemon without performing writes or Poll.
pub fn run_connected(
    client: &DbusClient,
    config: &Path,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut report = Report::new("Dbus");
    System::new(PathBuf::from("/"), TransportChoice::Dbus).collect(config, &mut report);
    probe_dbus(client, true, &mut report);
    report.write(json, out).map(|_| ())
}

fn probe_transport(transport: &dyn Transport, report: &mut Report) {
    report.add(
        "transport",
        Status::Pass,
        format!(
            "{:?}; writable API={} (actual permissions are checked separately)",
            transport.kind(),
            transport.writable()
        ),
        "",
    );
    if transport.kind() == TransportKind::Mock {
        report.add(
            "fixture",
            Status::Pass,
            "fixture parsed; writes refused; no hardware queries",
            "",
        );
        return;
    }
    let advice = "Check driver/module permissions; inspect journalctl -k -b for ACPI/_DSM errors. Do not bypass validation with raw EC writes.";
    match commands::read_fan_status(transport) {
        Ok(_) => report.add(
            "hardware.fan_status",
            Status::Pass,
            "command 12 returned a valid fan status",
            "",
        ),
        Err(error) => report.add(
            "hardware.fan_status",
            Status::Fail,
            error.to_string(),
            advice,
        ),
    }
    match commands::read_fan_curve(transport) {
        Ok(curve) => {
            report.add(
                "hardware.fan_curve",
                Status::Pass,
                format!(
                    "command 13: fan_count={}, kb_type={}",
                    curve.fan_count, curve.kb_type
                ),
                "",
            );
            if matches!(curve.kb_type, 6 | 22)
                && report
                    .checks
                    .iter()
                    .any(|check| check.id == "keyboard.rgb15" && check.status != Status::Pass)
            {
                report.add("hardware.keyboard", Status::Warn, format!("firmware advertises RGB15 (kb_type={}) but keyboard_rgb is missing", curve.kb_type), "Check the loaded driver version and reload the matching module; inspect journalctl -k -b.");
            }
        }
        Err(error) => report.add(
            "hardware.fan_curve",
            Status::Fail,
            error.to_string(),
            advice,
        ),
    }
    match transport.read_capabilities() {
        Ok(caps) => report.add(
            "hardware.capabilities",
            Status::Pass,
            format!(
                "source={}; supported performance modes={:?}",
                if caps.gcmd.is_some() {
                    "GCMD"
                } else {
                    "AppSettings page 7"
                },
                caps.power_modes
            ),
            "",
        ),
        Err(error) => report.add(
            "hardware.capabilities",
            Status::Fail,
            error.to_string(),
            advice,
        ),
    }
}

fn probe_dbus(client: &DbusClient, selected: bool, report: &mut Report) {
    let missing = if selected { Status::Fail } else { Status::Warn };
    report.add(
        "dbus.connection",
        Status::Pass,
        "selected bus connected",
        "",
    );
    let client = match client.diagnostic_service() {
        Ok(Some(instance)) => {
            report.add(
                "dbus.service",
                Status::Pass,
                "org.clevo.CC owns its bus name; no service activation requested",
                "",
            );
            instance
        }
        Ok(None) => {
            report.add("dbus.service", missing, "org.clevo.CC has no owner on the selected bus", "Run systemctl status clevod and journalctl -u clevod -b --no-pager; start the installed service with sudo systemctl enable --now clevod. For a test daemon check --session-bus and --dbus-session.");
            return;
        }
        Err(error) => {
            report.add(
                "dbus.service",
                missing,
                error.to_string(),
                "Check D-Bus availability and policy; inspect journalctl -u clevod -b --no-pager.",
            );
            return;
        }
    };
    if !selected {
        return;
    }
    match client.status() {
        Ok(status) => {
            report.add(
                "daemon.status",
                Status::Pass,
                format!(
                    "writable={}, curve_writable={}, fan_count={}, fan_mode={}, perf_mode={}",
                    status.writable,
                    status.curve_writable,
                    status.fan_count,
                    status.fan_mode,
                    status.perf_mode
                ),
                "",
            );
            report.add("daemon.telemetry", if status.freshness == "fresh" { Status::Pass } else { Status::Warn }, format!("fan cache freshness={}; doctor does not call Poll", status.freshness), if status.freshness == "fresh" { "" } else { "Inspect journalctl -u clevod -b for polling errors; an unknown cache may indicate polling is disabled." });
        }
        Err(error) => report.add(
            "daemon.status",
            Status::Fail,
            error.to_string(),
            "Check that CLI and daemon versions match; inspect journalctl -u clevod -b --no-pager.",
        ),
    }
    match client.get_capabilities() {
        Ok(caps) => report.add("daemon.capabilities", Status::Pass, format!("source={}; supported performance modes={:?}", if caps.gcmd.is_some() { "GCMD" } else { "AppSettings page 7" }, caps.power_modes), ""),
        Err(error) => report.add("daemon.capabilities", Status::Fail, error.to_string(), "Inspect journalctl -u clevod -b and journalctl -k -b for GCMD/_DSM errors; rebuild and reload the matching driver if necessary."),
    }
    match client.keyboard_available() {
        Ok(true) => report.add("daemon.keyboard", Status::Pass, "daemon reports an available keyboard RGB backend", ""),
        Ok(false) => match client.keyboard_firmware_type() {
            Ok(6 | 22) => report.add("daemon.keyboard", Status::Warn, "firmware advertises RGB15 but the daemon has no available backend", "Check keyboard_rgb and the loaded driver version; restart clevod after replacing the module."),
            Ok(kind) => report.add("daemon.keyboard", Status::Skip, format!("no keyboard RGB backend; firmware kb_type={kind}"), ""),
            Err(error) => report.add("daemon.keyboard", Status::Warn, error.to_string(), "Check CLI/daemon versions."),
        },
        Err(error) => report.add("daemon.keyboard", Status::Warn, error.to_string(), "Check CLI/daemon versions."),
    }
}

struct System {
    root: PathBuf,
    choice: TransportChoice,
}

impl System {
    fn new(root: PathBuf, choice: TransportChoice) -> Self {
        Self { root, choice }
    }

    fn path(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        if self.root == Path::new("/") {
            path.to_path_buf()
        } else {
            self.root.join(path.strip_prefix("/").unwrap_or(path))
        }
    }

    fn read(
        &self,
        report: &mut Report,
        id: &str,
        path: &str,
        missing: Status,
        suggestion: &str,
    ) -> Option<String> {
        match fs::read_to_string(self.path(path)) {
            Ok(text) if !text.trim().is_empty() => {
                let text = text.trim().to_string();
                report.add(id, Status::Pass, format!("{path}: {text}"), "");
                Some(text)
            }
            Ok(_) => {
                report.add(id, Status::Warn, format!("{path}: empty"), suggestion);
                None
            }
            Err(error) => {
                let suggestion = if error.kind() == io::ErrorKind::PermissionDenied {
                    "Access denied. Prefer --transport dbus for hardware access; check the clevo-cc group/udev rule for direct access, or run doctor with sudo when privileged read evidence is needed."
                } else {
                    suggestion
                };
                report.add(
                    id,
                    if error.kind() == io::ErrorKind::NotFound {
                        missing
                    } else if missing == Status::Fail {
                        Status::Fail
                    } else {
                        Status::Warn
                    },
                    format!("{path}: {error}"),
                    suggestion,
                );
                None
            }
        }
    }

    fn entries(&self, report: &mut Report, id: &str, path: &str) -> Vec<PathBuf> {
        match fs::read_dir(self.path(path)) {
            Ok(entries) => {
                let mut paths = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => paths.push(entry.path()),
                        Err(error) => report.add(
                            id,
                            Status::Warn,
                            error.to_string(),
                            "Check directory read permissions.",
                        ),
                    }
                }
                paths.sort();
                paths
            }
            Err(error) => {
                report.add(
                    id,
                    if error.kind() == io::ErrorKind::NotFound {
                        Status::Skip
                    } else {
                        Status::Warn
                    },
                    format!("{path}: {error}"),
                    "",
                );
                Vec::new()
            }
        }
    }

    fn collect(&self, config: &Path, report: &mut Report) {
        if !cfg!(target_os = "linux") {
            report.add(
                "system",
                Status::Skip,
                "Linux system checks unavailable on this OS",
                "Run diagnostics on the target Linux machine.",
            );
            return;
        }
        self.kernel(report);
        for field in ["sys_vendor", "product_name", "board_name"] {
            self.read(
                report,
                &format!("dmi.{field}"),
                &format!("/sys/class/dmi/id/{field}"),
                Status::Warn,
                "DMI is unavailable; use firmware/ACPI information to identify the machine.",
            );
        }
        let required = if self.choice == TransportChoice::Driver {
            Status::Fail
        } else {
            Status::Warn
        };
        if let Some(status) = self.read(report, "acpi.device", "/sys/bus/acpi/devices/CLV0001:00/status", required, "Check CLV0001 in the firmware/DSDT and journalctl -k -b; this driver is verified only on supported Clevo models.") {
            let value = status.strip_prefix("0x").map_or_else(|| status.parse::<u32>().ok(), |hex| u32::from_str_radix(hex, 16).ok());
            if value != Some(15) {
                report.add("acpi.device_status", required, format!("_STA={status}; expected 15 (0x0f)"), "Check firmware device availability before using this driver.");
            }
        }
        for module in ["clevo_cc", "acpi_call"] {
            let present = self.path(format!("/sys/module/{module}")).is_dir();
            report.add(
                format!("module.{module}"),
                if present { Status::Pass } else { Status::Skip },
                if present {
                    "loaded"
                } else {
                    "not loaded (may be optional or built into the kernel)"
                },
                "",
            );
        }
        match fs::read_link(self.path("/sys/devices/platform/CLV0001:00/driver")) {
            Ok(path) if path.file_name().is_some_and(|name| name == "clevo-cc") => report.add("driver.binding", Status::Pass, format!("bound to {}", path.display()), ""),
            Ok(path) => report.add("driver.binding", required, format!("bound to another driver: {}", path.display()), "Inspect the platform driver binding and journalctl -k -b before changing drivers."),
            Err(error) => report.add("driver.binding", required, format!("no readable clevo-cc binding: {error}"), "Install the driver for the running kernel, check its headers/DKMS status, then inspect journalctl -k -b."),
        }
        for attr in ["fan_mode", "perf_mode"] {
            let path = format!("/sys/devices/platform/CLV0001:00/{attr}");
            self.read(
                report,
                &format!("driver.{attr}"),
                &path,
                if self.choice == TransportChoice::Driver {
                    Status::Fail
                } else {
                    Status::Skip
                },
                "Check that the current clevo-cc module is loaded; inspect journalctl -k -b.",
            );
            if self.path(&path).exists() {
                self.permission(
                    report,
                    &format!("permissions.{attr}"),
                    &self.path(&path),
                    self.choice == TransportChoice::Driver,
                );
            }
        }
        let hwmon = self.entries(
            report,
            "driver.hwmon",
            "/sys/devices/platform/CLV0001:00/hwmon",
        );
        for dir in &hwmon {
            if let Some(name) = dir.file_name().and_then(|name| name.to_str()) {
                self.read(
                    report,
                    "driver.fan1_input",
                    &format!("/sys/devices/platform/CLV0001:00/hwmon/{name}/fan1_input"),
                    required,
                    "Check driver hwmon registration and read permissions.",
                );
            }
        }
        if hwmon.is_empty() && self.choice == TransportChoice::Driver {
            report.add(
                "driver.fans",
                Status::Fail,
                "no driver hwmon fan inputs found",
                "Check the running driver and journalctl -k -b.",
            );
        }
        let acpi = self.path("/proc/acpi/call");
        report.add(
            "acpi_call.interface",
            if acpi.exists() {
                Status::Pass
            } else if self.choice == TransportChoice::AcpiCall {
                Status::Fail
            } else {
                Status::Skip
            },
            if acpi.exists() {
                "/proc/acpi/call available; supports read-only fan and GCMD queries"
            } else {
                "/proc/acpi/call absent; not needed by the driver/D-Bus driver backend"
            },
            if self.choice == TransportChoice::AcpiCall {
                "Install/load acpi_call for the running kernel, or use --transport dbus/driver."
            } else {
                ""
            },
        );
        if acpi.exists() && self.choice == TransportChoice::AcpiCall {
            self.permission(report, "permissions.acpi_call", &acpi, true);
        }
        self.keyboard(report);
        self.power(report);
        self.installation(config, report);
    }

    fn kernel(&self, report: &mut Report) {
        let Some(release) = self.read(
            report,
            "kernel.release",
            "/proc/sys/kernel/osrelease",
            Status::Warn,
            "Check that procfs is mounted.",
        ) else {
            return;
        };
        // The release becomes a path component, so reject malformed input.
        if release.contains('/') || release.contains("..") {
            report.add(
                "kernel.config",
                Status::Warn,
                "invalid kernel release",
                "Check procfs.",
            );
            return;
        }
        let mut contents = None;
        for path in [
            format!("/boot/config-{release}"),
            format!("/lib/modules/{release}/build/.config"),
            format!("/usr/lib/modules/{release}/build/.config"),
        ] {
            if let Ok(text) = fs::read_to_string(self.path(&path)) {
                contents = Some((path, text));
                break;
            }
        }
        if contents.is_none() && self.path("/proc/config.gz").exists() {
            match std::process::Command::new("gzip")
                .arg("-cd")
                .arg(self.path("/proc/config.gz"))
                .output()
            {
                Ok(output) if output.status.success() => {
                    contents = Some((
                        "/proc/config.gz".into(),
                        String::from_utf8_lossy(&output.stdout).into_owned(),
                    ))
                }
                _ => report.add(
                    "kernel.config_reader",
                    Status::Warn,
                    "could not decompress /proc/config.gz",
                    "Install gzip or inspect a readable /boot/config-<kernel> file.",
                ),
            }
        }
        match contents {
            Some((path, text)) if text.lines().any(|line| line == "CONFIG_ACPI=y") => report.add("kernel.acpi", Status::Pass, format!("CONFIG_ACPI=y ({path})"), ""),
            Some((path, _)) => report.add("kernel.acpi", Status::Fail, format!("CONFIG_ACPI=y not found in {path}"), "Use a kernel built with CONFIG_ACPI=y."),
            None => report.add("kernel.acpi", Status::Warn, "CONFIG_ACPI unknown: running kernel config not readable", "Inspect /proc/config.gz or /boot/config-<kernel>; missing config evidence does not prove ACPI is disabled."),
        }
        let headers = self.path(format!("/lib/modules/{release}/build"));
        report.add("kernel.headers", if headers.is_dir() { Status::Pass } else { Status::Warn }, format!("{}: {}", headers.display(), if headers.is_dir() { "available" } else { "missing" }), if headers.is_dir() { "" } else { "Install headers matching the running kernel before rebuilding DKMS; if the kernel was upgraded, reboot into the matching installed kernel." });
    }

    fn keyboard(&self, report: &mut Report) {
        let rgb = self.path("/sys/devices/platform/CLV0001:00/keyboard_rgb");
        let has_rgb = rgb.exists();
        report.add(
            "keyboard.rgb15",
            if has_rgb { Status::Pass } else { Status::Skip },
            if has_rgb {
                "keyboard_rgb sysfs interface available"
            } else {
                "RGB15 interface not provided; hardware may use another backend"
            },
            "",
        );
        if has_rgb {
            self.permission(report, "permissions.keyboard_rgb", &rgb, false);
        }
        let mut compatible = 0;
        for device in self.entries(report, "keyboard.hidraw", "/sys/class/hidraw") {
            match fs::read_to_string(device.join("device/uevent")) {
                Ok(text) => {
                    let matches = text
                        .lines()
                        .find_map(|line| line.strip_prefix("HID_ID="))
                        .is_some_and(|id| {
                            let fields = id.split(':').collect::<Vec<_>>();
                            fields.len() == 3
                                && u32::from_str_radix(fields[1], 16) == Ok(0x048d)
                                && u32::from_str_radix(fields[2], 16) == Ok(0x8910)
                        });
                    if matches {
                        compatible += 1;
                        let node = self
                            .path(Path::new("/dev").join(device.file_name().unwrap_or_default()));
                        report.add(
                            "keyboard.ite",
                            Status::Pass,
                            format!("ITE 048d:8910 at {}", node.display()),
                            "",
                        );
                        self.permission(report, "permissions.hidraw", &node, false);
                    }
                }
                Err(error) => report.add(
                    "keyboard.hidraw",
                    Status::Warn,
                    format!("{}: {error}", device.display()),
                    "Check hidraw sysfs read permissions.",
                ),
            }
        }
        if compatible == 0 {
            report.add(
                "keyboard.ite",
                Status::Skip,
                "no ITE 048d:8910 found; this is normal on RGB15 machines",
                "",
            );
        }
    }

    fn power(&self, report: &mut Report) {
        let mut batteries = 0;
        for device in self.entries(report, "power_supply", "/sys/class/power_supply") {
            match fs::read_to_string(device.join("type")) {
                Ok(kind) if kind.trim() == "Battery" => {
                    batteries += 1;
                    let name = device.file_name().unwrap_or_default().to_string_lossy();
                    for attr in [
                        "capacity",
                        "status",
                        "health",
                        "charge_control_start_threshold",
                        "charge_control_end_threshold",
                    ] {
                        self.read(report, &format!("battery.{name}.{attr}"), &format!("/sys/class/power_supply/{name}/{attr}"), Status::Skip, "This battery driver may not provide the attribute; do not infer EC charging support from its absence.");
                    }
                }
                Ok(_) => {}
                Err(error) => report.add(
                    "power_supply.type",
                    Status::Warn,
                    format!("{}: {error}", device.display()),
                    "Check power_supply read permissions.",
                ),
            }
        }
        if batteries == 0 {
            report.add(
                "battery",
                Status::Skip,
                "no battery reported by power_supply",
                "",
            );
        }
        for attr in ["platform_profile", "platform_profile_choices"] {
            self.read(report, attr, &format!("/sys/firmware/acpi/{attr}"), Status::Skip, "Optional desktop power-profile interface is not provided by this kernel/platform driver.");
        }
    }

    fn permission(&self, report: &mut Report, id: &str, path: &Path, required: bool) {
        #[cfg(target_os = "linux")]
        {
            // faccessat2 checks effective credentials and ACLs without opening
            // an attribute for writing or exercising a hardware operation.
            use rustix::fs::{accessat, Access, AtFlags, CWD};
            match accessat(CWD, path, Access::WRITE_OK, AtFlags::EACCESS) {
                Ok(()) => report.add(id, Status::Pass, format!("{}: effective-user write access allowed (not exercised)", path.display()), ""),
                Err(error) => report.add(id, if required { Status::Warn } else { Status::Skip }, format!("{}: no effective-user write access ({error}); daemon access is separate", path.display()), if self.choice == TransportChoice::Dbus { "Direct sysfs/hidraw access is optional for D-Bus clients; writes are authorized by the daemon through PolicyKit." } else { "Prefer --transport dbus. For deliberate direct access check the clevo-cc group/udev rule and log in again after group changes." }),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (path, required);
            report.add(
                id,
                Status::Skip,
                "effective-user permission checks require Linux",
                "",
            );
        }
    }

    fn installation(&self, config: &Path, report: &mut Report) {
        let identity = self.read(
            report,
            "permissions.identity",
            "/proc/self/status",
            Status::Warn,
            "Check procfs permissions.",
        );
        // Keep the identity report compact rather than dumping process details.
        if let Some(text) = &identity {
            if let Some(check) = report.checks.last_mut() {
                check.detail = text
                    .lines()
                    .filter(|line| {
                        line.starts_with("Uid:")
                            || line.starts_with("Gid:")
                            || line.starts_with("Groups:")
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
            }
        }
        match fs::read_to_string(self.path("/etc/group")) {
            Ok(text) => {
                let group = text.lines().find(|line| line.starts_with("clevo-cc:"));
                match group
                    .and_then(|line| line.split(':').nth(2))
                    .and_then(|gid| gid.parse::<u32>().ok())
                {
                    Some(gid) => {
                        let member = identity.as_deref().is_some_and(|text| {
                            text.lines().any(|line| {
                                if let Some(groups) = line.strip_prefix("Groups:") {
                                    groups
                                        .split_whitespace()
                                        .any(|value| value.parse::<u32>() == Ok(gid))
                                } else if let Some(gids) = line.strip_prefix("Gid:") {
                                    gids.split_whitespace()
                                        .nth(1)
                                        .is_some_and(|value| value.parse::<u32>() == Ok(gid))
                                } else {
                                    false
                                }
                            })
                        });
                        report.add("permissions.group", if member { Status::Pass } else { Status::Skip }, format!("clevo-cc gid={gid}; effective process member={member}; membership is optional for D-Bus clients"), "");
                    }
                    None => report.add(
                        "permissions.group",
                        Status::Skip,
                        "clevo-cc group absent; optional for D-Bus clients",
                        "For direct access use the installer-provided group and udev rule.",
                    ),
                }
            }
            Err(error) => report.add(
                "permissions.group",
                Status::Warn,
                format!("/etc/group: {error}"),
                "Check group database availability.",
            ),
        }
        let rules = [
            "/etc/udev/rules.d/99-clevo-cc.rules",
            "/usr/lib/udev/rules.d/99-clevo-cc.rules",
            "/lib/udev/rules.d/99-clevo-cc.rules",
        ];
        let rule = rules.iter().find(|path| {
            fs::read_to_string(self.path(path))
                .is_ok_and(|text| text.contains("CLV0001") && text.contains("clevo-cc"))
        });
        report.add(
            "permissions.udev",
            if rule.is_some() {
                Status::Pass
            } else {
                Status::Skip
            },
            rule.map_or(
                "installer udev rule not found; optional for D-Bus clients",
                |path| *path,
            ),
            "",
        );
        for (id, paths) in [
            (
                "installation.service",
                vec![
                    "/etc/systemd/system/clevod.service",
                    "/usr/lib/systemd/system/clevod.service",
                    "/lib/systemd/system/clevod.service",
                ],
            ),
            (
                "installation.dbus_policy",
                vec![
                    "/etc/dbus-1/system.d/org.clevo.CC.conf",
                    "/usr/share/dbus-1/system.d/org.clevo.CC.conf",
                ],
            ),
            (
                "installation.polkit",
                vec!["/usr/share/polkit-1/actions/org.clevo.CC.policy"],
            ),
        ] {
            let installed = paths.iter().find(|path| self.path(path).is_file());
            report.add(id, if installed.is_some() { Status::Pass } else { Status::Warn }, installed.map_or("installed file not found", |path| *path), if installed.is_none() { "Check the installation prefix and packaging/install.sh; diagnostics do not install or restart anything." } else { "" });
        }
        let path = self.path(config);
        if let Some(parent) = path.parent() {
            report.add("config.directory", if parent.is_dir() { Status::Pass } else { Status::Warn }, format!("{}: {}", parent.display(), if parent.is_dir() { "exists" } else { "absent" }), if parent.is_dir() { "" } else { "Check the daemon --config path and installation; the daemon creates this directory when saving." });
        }
        match fs::read_to_string(&path) {
            Ok(text) => match clevod::config::parse(&text) {
                Ok(_) => report.add("config.schema", Status::Pass, format!("{}: accepted by daemon schema {} parser (legacy migrations stay in memory)", config.display(), clevod::config::SCHEMA_VERSION), ""),
                Err(error) => report.add("config.schema", Status::Fail, format!("{}: {error}", config.display()), "Fix TOML/field types, or use the matching daemon version for a newer schema; doctor never rewrites configuration."),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => report.add("config.schema", Status::Skip, format!("{}: absent; daemon uses defaults", config.display()), ""),
            Err(error) => report.add("config.schema", Status::Warn, format!("{}: unreadable ({error})", config.display()), "Check the daemon --config path; if root-owned, run doctor with sudo to validate it. Permission failure does not imply invalid TOML."),
        }
    }
}
