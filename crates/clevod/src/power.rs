//! Linux power interfaces and the driver's named FlexiCharger control.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub use clevo_proto::power::{
    Battery, ChargeControl, ChargeThresholds, PlatformProfile, PowerSnapshot,
};

mod native;
use native::NativeCharge;

/// Keep unsupported hardware distinct from invalid requests and I/O failures.
#[derive(Debug)]
pub enum PowerError {
    /// The battery or threshold control is unavailable.
    Unsupported(String),
    /// Request values or the battery name are invalid.
    Invalid(String),
    /// A read, write, readback or recovery failed.
    Io(String),
}

impl std::fmt::Display for PowerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Invalid(message) | Self::Io(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for PowerError {}

/// Serialized sysfs access. Tests supply an isolated sysfs tree.
pub struct Power {
    sys: PathBuf,
    lock: Mutex<()>,
}

impl Power {
    /// Read the real `/sys` tree.
    pub fn system() -> Self {
        Self::with_sysfs("/sys")
    }

    /// Use an explicit sysfs root, primarily for offline tests.
    pub fn with_sysfs(sys: impl Into<PathBuf>) -> Self {
        Self {
            sys: sys.into(),
            lock: Mutex::new(()),
        }
    }

    /// Read all available power information without writes.
    pub fn snapshot(&self) -> PowerSnapshot {
        let _guard = self.lock.lock().unwrap();
        let mut result = PowerSnapshot {
            available: true,
            ..Default::default()
        };
        match fs::read_dir(self.sys.join("class/power_supply")) {
            Ok(entries) => {
                for entry in entries {
                    let path = match entry {
                        Ok(entry) => entry.path(),
                        Err(error) => {
                            result.errors.push(error.to_string());
                            continue;
                        }
                    };
                    match fs::read_to_string(path.join("type")) {
                        Ok(kind) if kind.trim() == "Battery" => {
                            result.batteries.push(read_battery(&path))
                        }
                        Ok(_) => {
                            if let Some(online) = read_text(&path, "online", &mut result.errors)
                                .and_then(|text| match text.as_str() {
                                    "0" => Some(false),
                                    "1" => Some(true),
                                    _ => {
                                        result.errors.push(format!(
                                            "{}: invalid online={text}",
                                            path.display()
                                        ));
                                        None
                                    }
                                })
                            {
                                result.ac_online =
                                    Some(result.ac_online.unwrap_or(false) || online);
                            }
                        }
                        Err(error) => result.errors.push(format!("{}: {error}", path.display())),
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => result.errors.push(format!("power_supply: {error}")),
        }
        result.batteries.sort_by(|a, b| a.name.cmp(&b.name));
        if result
            .batteries
            .iter()
            .any(|battery| battery.present && !battery.end_supported)
        {
            match NativeCharge::read(&self.sys) {
                Ok(Some(native)) => {
                    for battery in &mut result.batteries {
                        let path = self.sys.join("class/power_supply").join(&battery.name);
                        if battery.present
                            && !battery.end_supported
                            && native.matches_battery(&path)
                        {
                            battery.thresholds = Some(native.thresholds);
                            battery.start_supported = true;
                            battery.end_supported = true;
                            battery.charge_control = Some(native.control());
                        }
                    }
                }
                Ok(None) | Err(PowerError::Unsupported(_)) => {}
                Err(error) => result.errors.push(format!("FlexiCharger: {error}")),
            }
        }
        result.platform_profile = self.profile();
        result
    }

    fn profile(&self) -> PlatformProfile {
        // Prefer the provider belonging to this driver on multi-provider kernels.
        if let Ok(entries) = fs::read_dir(self.sys.join("class/platform-profile")) {
            for entry in entries.flatten() {
                if fs::read_to_string(entry.path().join("name"))
                    .is_ok_and(|name| name.trim() == "clevo-cc")
                {
                    return read_profile(
                        &entry.path(),
                        "profile",
                        "choices",
                        Some("clevo-cc".into()),
                    );
                }
            }
        }
        let driver = fs::read_to_string(
            self.sys
                .join("devices/platform/CLV0001:00/platform_profile_available"),
        )
        .ok()
        .filter(|text| text.trim() == "1")
        .map(|_| "clevo-cc".into());
        read_profile(
            &self.sys.join("firmware/acpi"),
            "platform_profile",
            "platform_profile_choices",
            driver,
        )
    }

    /// Set standard or native thresholds, check readback and restore on failure.
    pub fn set_thresholds(
        &self,
        name: &str,
        requested: ChargeThresholds,
    ) -> Result<ChargeThresholds, PowerError> {
        self.set_thresholds_using(name, requested, write_window)
    }

    fn set_thresholds_using(
        &self,
        name: &str,
        requested: ChargeThresholds,
        mut write: impl FnMut(&Path, ChargeThresholds, ChargeThresholds) -> Result<(), String>,
    ) -> Result<ChargeThresholds, PowerError> {
        let _guard = self.lock.lock().unwrap();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(PowerError::Invalid("invalid battery name".into()));
        }
        validate_thresholds(requested).map_err(PowerError::Invalid)?;
        let path = self.sys.join("class/power_supply").join(name);
        if fs::read_to_string(path.join("type"))
            .map_err(|error| PowerError::Io(error.to_string()))?
            .trim()
            != "Battery"
        {
            return Err(PowerError::Invalid(
                "requested power supply is not a battery".into(),
            ));
        }
        let before = read_battery(&path);
        if !before.present {
            return Err(PowerError::Unsupported(
                "battery presence is unavailable".into(),
            ));
        }
        if !before.end_supported {
            return self.set_native_using(&path, requested, native::write);
        }
        if requested.enabled.is_some() {
            return Err(PowerError::Unsupported(
                "enable state is only supported by FlexiCharger".into(),
            ));
        }
        if !before.end_supported || (requested.start.is_some() && !before.start_supported) {
            return Err(PowerError::Unsupported(
                "standard charge thresholds are not supported by this battery".into(),
            ));
        }
        let old = before.thresholds.ok_or_else(|| {
            PowerError::Io(format!(
                "cannot read current charge thresholds: {:?}",
                before.errors
            ))
        })?;
        if before.start_supported && old.start.is_none() {
            return Err(PowerError::Io(
                "cannot read the start threshold; no changes made".into(),
            ));
        }
        let target = ChargeThresholds {
            enabled: None,
            start: requested.start.or(old.start),
            end: requested.end,
        };
        validate_thresholds(target).map_err(PowerError::Invalid)?;
        if target == old {
            return Ok(old);
        }
        if let Err(error) = write(&path, old, target).and_then(|()| {
            if read_battery(&path).thresholds == Some(target) {
                Ok(())
            } else {
                Err("threshold readback did not match the requested values".into())
            }
        }) {
            let current = read_battery(&path).thresholds;
            let rollback = current
                .ok_or_else(|| "current thresholds unreadable".to_string())
                .and_then(|current| write(&path, current, old))
                .and_then(|()| {
                    if read_battery(&path).thresholds == Some(old) {
                        Ok(())
                    } else {
                        Err("restore readback mismatch".into())
                    }
                });
            return Err(PowerError::Io(match rollback {
                Ok(()) => format!("{error}; previous thresholds restored"),
                Err(restore) => format!("{error}; restoring previous thresholds failed: {restore}; inspect {name} charging settings"),
            }));
        }
        Ok(target)
    }

    fn set_native_using(
        &self,
        battery: &Path,
        requested: ChargeThresholds,
        mut write: impl FnMut(&Path, ChargeThresholds) -> Result<(), String>,
    ) -> Result<ChargeThresholds, PowerError> {
        let native = NativeCharge::read(&self.sys)?.ok_or_else(|| {
            PowerError::Unsupported(
                "current Linux driver does not provide charge threshold control".into(),
            )
        })?;
        if !native.matches_battery(battery) {
            return Err(PowerError::Unsupported(
                "FlexiCharger does not control this battery".into(),
            ));
        }
        let target = ChargeThresholds {
            enabled: Some(requested.enabled.unwrap_or(true)),
            start: requested.start.or(native.thresholds.start),
            end: requested.end,
        };
        validate_thresholds(target).map_err(PowerError::Invalid)?;
        if !native.start_values.contains(&target.start.unwrap())
            || !native.end_values.contains(&target.end)
        {
            return Err(PowerError::Invalid(format!(
                "firmware allows start={:?}, end={:?}",
                native.start_values, native.end_values
            )));
        }
        if target == native.thresholds {
            return Ok(target);
        }
        let readback = || -> Result<ChargeThresholds, String> {
            NativeCharge::read(&self.sys)
                .map_err(|error| error.to_string())?
                .map(|value| value.thresholds)
                .ok_or_else(|| "native threshold control disappeared".into())
        };
        if let Err(error) = write(&native.path, target).and_then(|()| {
            if readback()? == target {
                Ok(())
            } else {
                Err("native threshold readback mismatch".into())
            }
        }) {
            if readback().is_ok_and(|current| current == native.thresholds) {
                return Err(PowerError::Io(format!(
                    "{error}; readback confirms previous thresholds and enable state"
                )));
            }
            let rollback = write(&native.path, native.thresholds).and_then(|()| {
                if readback()? == native.thresholds {
                    Ok(())
                } else {
                    Err("native restore readback mismatch".into())
                }
            });
            return Err(PowerError::Io(match rollback {
                Ok(()) => format!("{error}; previous thresholds and enable state restored"),
                Err(restore) => format!("{error}; restoring previous thresholds failed: {restore}; inspect charging settings"),
            }));
        }
        Ok(target)
    }
}

/// Check the percentage range and ordering before any write or replay.
pub fn validate_thresholds(value: ChargeThresholds) -> Result<(), String> {
    if value.end == 0 || value.end > 100 || value.start.is_some_and(|start| start >= value.end) {
        return Err("charge thresholds require 0 <= start < end <= 100".into());
    }
    Ok(())
}

fn read_text(path: &Path, name: &str, errors: &mut Vec<String>) -> Option<String> {
    match fs::read_to_string(path.join(name)) {
        Ok(text) if !text.trim().is_empty() => Some(text.trim().into()),
        Ok(_) => {
            errors.push(format!("{name}: empty value"));
            None
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            errors.push(format!("{name}: {error}"));
            None
        }
    }
}

fn number(path: &Path, name: &str, errors: &mut Vec<String>) -> Option<u64> {
    read_text(path, name, errors).and_then(|text| match text.parse() {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(format!("{name}: {error}"));
            None
        }
    })
}

fn percent(path: &Path, name: &str, errors: &mut Vec<String>) -> Option<u8> {
    number(path, name, errors).and_then(|value| {
        if value <= 100 {
            Some(value as u8)
        } else {
            errors.push(format!("{name}: {value} outside 0..100"));
            None
        }
    })
}

fn read_battery(path: &Path) -> Battery {
    let mut errors = Vec::new();
    let present = match number(path, "present", &mut errors) {
        Some(1) => true,
        Some(0) => false,
        None if !path.join("present").exists() => true,
        _ => {
            errors.push("battery presence could not be confirmed".into());
            false
        }
    };
    let capacity = percent(path, "capacity", &mut errors);
    let status = read_text(path, "status", &mut errors);
    let health = read_text(path, "health", &mut errors);
    let cycle_count = number(path, "cycle_count", &mut errors);
    let start_supported = path.join("charge_control_start_threshold").exists();
    let end_supported = path.join("charge_control_end_threshold").exists();
    let start = percent(path, "charge_control_start_threshold", &mut errors);
    let end = percent(path, "charge_control_end_threshold", &mut errors);
    let mut health_percent = None;
    for unit in ["energy", "charge"] {
        let full = number(path, &format!("{unit}_full"), &mut errors);
        let design = number(path, &format!("{unit}_full_design"), &mut errors);
        if let (Some(full), Some(design)) = (full, design) {
            if design != 0 {
                health_percent = Some((full as f64 / design as f64 * 100.0).min(100.0));
                break;
            }
        }
    }
    Battery {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        present,
        capacity,
        status,
        health,
        health_percent,
        cycle_count,
        thresholds: end.map(|end| ChargeThresholds {
            enabled: None,
            start,
            end,
        }),
        charge_control: None,
        start_supported,
        end_supported,
        errors,
    }
}

fn read_profile(
    path: &Path,
    current: &str,
    choices: &str,
    driver: Option<String>,
) -> PlatformProfile {
    let mut errors = Vec::new();
    let current = read_text(path, current, &mut errors);
    let choices = read_text(path, choices, &mut errors)
        .map(|text| {
            text.split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let reason = if !errors.is_empty() {
        Some(errors.join("; "))
    } else if choices.is_empty() {
        Some("platform_profile is not provided by this kernel/platform driver".into())
    } else {
        None
    };
    PlatformProfile {
        driver,
        current,
        choices,
        reason,
    }
}

fn write_window(path: &Path, from: ChargeThresholds, to: ChargeThresholds) -> Result<(), String> {
    let start = to.start.filter(|start| Some(*start) != from.start);
    let end = (to.end != from.end).then_some(to.end);
    let operations = if start.is_some_and(|start| start >= from.end) {
        [
            ("charge_control_end_threshold", end),
            ("charge_control_start_threshold", start),
        ]
    } else {
        [
            ("charge_control_start_threshold", start),
            ("charge_control_end_threshold", end),
        ]
    };
    for (attr, value) in operations {
        if let Some(value) = value {
            let target = path.join(attr);
            fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&target)
                .and_then(|mut file| write_percent(&mut file, value))
                .map_err(|error| format!("{}: {error}", target.display()))?;
        }
    }
    Ok(())
}

fn write_percent(writer: &mut impl Write, value: u8) -> std::io::Result<()> {
    let text = format!("{value}\n");
    writer.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests;
