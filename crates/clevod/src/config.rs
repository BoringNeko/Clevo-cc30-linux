//! Local configuration persistence (design decision D9).
//!
//! The kernel driver and `acpi_call` only send EC commands; the EC forgets
//! everything on reboot. `clevod` therefore owns persistence: it stores the
//! last user-chosen fan mode and performance mode in a versioned TOML file and
//! re-applies them on startup.
//!
//! Dual-track note (D9): the original Control Center also mirrored some settings
//! into EC `page 0..7` via AppSettings. That channel has no verified accessor on
//! this machine yet, so only the local file track is implemented. The schema is
//! versioned so an EC-backed track can be added later without breaking files.
//!
//! This module is pure (no I/O in the parsing/merging logic) except for the
//! explicit `load`/`save` helpers, so it is unit tested without a filesystem.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current on-disk schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// Persisted settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// On-disk schema version; a newer file is not loaded.
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    /// Last fan mode (`121/1` value) chosen by the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan_mode: Option<u8>,
    /// Last performance mode (`121/25` value) chosen by the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perf_mode: Option<u8>,
    /// TDP class of the installed CPU, used to convert the raw CPU temperature.
    ///
    /// One of `35W`, `47W`, `65W`, `84W`, `91W`, or absent for "unknown"
    /// (which reports the raw byte unconverted). The default is the COLORFUL
    /// P15 23's class; set it to your own CPU's TDP for correct readings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_tdp_class: Option<String>,
    /// Whether to re-apply the saved modes on daemon startup.
    #[serde(default = "default_true")]
    pub apply_on_start: bool,
}

/// Parse a configured TDP class string.
///
/// Unknown or absent values fall back to `TdpClass::Unknown`, which reports the
/// raw CPU temperature byte unconverted rather than inventing a conversion.
pub fn parse_tdp_class(text: Option<&str>) -> clevo_proto::TdpClass {
    use clevo_proto::TdpClass;
    match text.map(str::trim).map(str::to_ascii_uppercase).as_deref() {
        Some("35W") => TdpClass::W35,
        Some("47W") | Some("45W") => TdpClass::W47,
        Some("65W") => TdpClass::W65,
        Some("84W") | Some("88W") => TdpClass::W84,
        Some("91W") => TdpClass::W91,
        _ => TdpClass::Unknown,
    }
}

fn default_true() -> bool {
    true
}

fn current_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            fan_mode: None,
            perf_mode: None,
            cpu_tdp_class: None,
            apply_on_start: true,
        }
    }
}

/// Errors from loading/saving the config.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read or written.
    Io(std::io::Error),
    /// The file could not be parsed as TOML.
    Parse(toml::de::Error),
    /// The file was written by a newer schema version.
    UnsupportedVersion(u32),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "config i/o error: {e}"),
            Self::Parse(e) => write!(f, "config parse error: {e}"),
            Self::UnsupportedVersion(v) => {
                write!(
                    f,
                    "config schema version {v} is newer than {SCHEMA_VERSION}"
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Parse config text, rejecting newer schema versions.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(text).map_err(ConfigError::Parse)?;
    if config.schema_version > SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedVersion(config.schema_version));
    }
    Ok(config)
}

/// Serialize config to TOML.
pub fn to_toml(config: &Config) -> Result<String, ConfigError> {
    toml::to_string_pretty(config)
        .map_err(|e| ConfigError::Io(std::io::Error::other(format!("serialize config: {e}"))))
}

/// Load config from `path`, returning defaults if the file does not exist.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(ConfigError::Io(e)),
    }
}

/// Atomically write config to `path` (write to a temp file, then rename).
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let text = to_toml(config)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Default config path: `/etc/clevo-cc/clevod.toml`.
pub fn default_path() -> PathBuf {
    PathBuf::from("/etc/clevo-cc/clevod.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let config = Config {
            schema_version: SCHEMA_VERSION,
            fan_mode: Some(8),
            perf_mode: Some(2),
            cpu_tdp_class: Some("47W".to_string()),
            apply_on_start: true,
        };
        let text = to_toml(&config).unwrap();
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn defaults_are_valid() {
        let parsed = parse("").unwrap();
        assert_eq!(parsed, Config::default());
        assert!(parsed.apply_on_start);
        assert!(parsed.fan_mode.is_none());
    }

    #[test]
    fn newer_schema_is_rejected() {
        let text = format!("schema_version = {}", SCHEMA_VERSION + 1);
        assert!(matches!(
            parse(&text),
            Err(ConfigError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = std::env::temp_dir().join("clevod-test-missing");
        let _ = std::fs::remove_file(&dir);
        let config = load(&dir).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join("clevod-test-save");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clevod.toml");
        let config = Config {
            fan_mode: Some(0),
            perf_mode: Some(1),
            ..Config::default()
        };
        save(&path, &config).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, config);
        std::fs::remove_file(&path).unwrap();
    }
}
