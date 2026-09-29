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

use clevo_proto::fan_curve::{FanCurve, FanPoint, CURVE_POINTS};
use clevo_transport::{Color, KeyboardMode, KeyboardSnapshot, KEYBOARD_COLS, KEYBOARD_ROWS};
use serde::{Deserialize, Serialize};

/// Current on-disk schema version.
///
/// Version 5 changed `keyboard.brightness` from the vendor's 0..4 scale to a
/// 0..100 percentage; v4 files are migrated on load.
pub const SCHEMA_VERSION: u32 = 5;

/// A fan-curve point in the versioned configuration file.
///
/// This is deliberately separate from `clevo-proto` so the protocol crate
/// remains dependency-free and the TOML schema can evolve independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanPointWire {
    /// Temperature in degrees Celsius.
    pub temp: u8,
    /// Fan duty as a percentage (`0..=100`).
    pub duty_pct: u8,
}

/// A persisted four-point fan curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanCurveWire {
    /// CPU fan curve.
    pub cpu: [FanPointWire; CURVE_POINTS],
    /// GPU1 fan curve.
    pub gpu1: [FanPointWire; CURVE_POINTS],
    /// GPU2 fan curve.
    pub gpu2: [FanPointWire; CURVE_POINTS],
}

/// An RGB color in the daemon's TOML schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardColorWire {
    /// Red component.
    pub r: u8,
    /// Green component.
    pub g: u8,
    /// Blue component.
    pub b: u8,
}

impl From<Color> for KeyboardColorWire {
    fn from(color: Color) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
        }
    }
}

impl From<KeyboardColorWire> for Color {
    fn from(color: KeyboardColorWire) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
        }
    }
}

/// One persisted non-black keyboard key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardKeyWire {
    /// Row in the verified 6x20 layout.
    pub row: u8,
    /// Column in the verified 6x20 layout.
    pub col: u8,
    /// Color last applied to this key.
    pub color: KeyboardColorWire,
}

/// Persisted keyboard RGB preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardConfig {
    /// Last selected controller mode (`off`, `static`, or `wave`).
    #[serde(default = "default_keyboard_mode")]
    pub mode: String,
    /// Last brightness as a percentage in `0..=100`.
    #[serde(default = "default_keyboard_brightness")]
    pub brightness: u8,
    /// Non-black keys written by the user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<KeyboardKeyWire>,
}

impl KeyboardConfig {
    /// Convert the successful in-memory writes into a compact TOML value.
    pub fn from_snapshot(snapshot: &KeyboardSnapshot) -> Self {
        let mut keys = Vec::new();
        for row in 0..KEYBOARD_ROWS {
            for col in 0..KEYBOARD_COLS {
                let color = snapshot.keys[row][col];
                if color != Color::default() {
                    keys.push(KeyboardKeyWire {
                        row: row as u8,
                        col: col as u8,
                        color: color.into(),
                    });
                }
            }
        }
        Self {
            mode: snapshot.mode.as_str().to_string(),
            brightness: snapshot.brightness,
            keys,
        }
    }
}

impl FanCurveWire {
    /// Convert a protocol curve into its persisted representation.
    pub fn from_curve(curve: &FanCurve) -> Self {
        fn points(points: &[FanPoint; CURVE_POINTS]) -> [FanPointWire; CURVE_POINTS] {
            points.map(|point| FanPointWire {
                temp: point.temp,
                duty_pct: point.duty_pct,
            })
        }

        Self {
            cpu: points(&curve.cpu),
            gpu1: points(&curve.gpu1),
            gpu2: points(&curve.gpu2),
        }
    }

    /// Convert the persisted representation back into a protocol curve.
    pub fn to_curve(self) -> FanCurve {
        fn points(points: [FanPointWire; CURVE_POINTS]) -> [FanPoint; CURVE_POINTS] {
            points.map(|point| FanPoint {
                temp: point.temp,
                duty_pct: point.duty_pct,
            })
        }

        FanCurve {
            cpu: points(self.cpu),
            gpu1: points(self.gpu1),
            gpu2: points(self.gpu2),
        }
    }
}

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
    /// Last user-saved custom fan curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan_curve: Option<FanCurveWire>,
    /// Snapshot of the EC curve taken before the daemon ever wrote one.
    ///
    /// This is what "restore default" loads. It has to be captured on first
    /// sighting: once the daemon writes a curve, the EC no longer holds the
    /// factory table, and there is no command that restores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_curve: Option<FanCurveWire>,
    /// Last keyboard RGB preferences, when a compatible HID controller exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard: Option<KeyboardConfig>,
    /// TDP class override for the raw CPU temperature byte.
    ///
    /// One of `35W`, `47W`, `65W`, `84W` or `91W`, matching the vendor's
    /// `cpu.ini` sections. Absent (the default) means **no conversion**, which
    /// is both the vendor's behaviour for an unmatched CPU and the verified
    /// behaviour on the reference machine - there the raw byte already tracks
    /// `sensors` within 1-2 °C. Only set this if your CPU's entry in the
    /// vendor `cpu.ini` matches one of those classes, otherwise it will make
    /// the reading worse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_tdp_class: Option<String>,
    /// Whether to re-apply the saved modes on daemon startup.
    #[serde(default = "default_true")]
    pub apply_on_start: bool,
}

/// Parse a configured TDP class string.
///
/// Absent or unrecognised values yield `TdpClass::Raw` (no conversion), which
/// is the safe default: an unnecessary curve makes the reading worse, whereas
/// no curve is correct on machines the vendor's `cpu.ini` does not list.
pub fn parse_tdp_class(text: Option<&str>) -> clevo_proto::TdpClass {
    use clevo_proto::TdpClass;
    match text.map(str::trim).map(str::to_ascii_uppercase).as_deref() {
        Some("35W") => TdpClass::W35,
        Some("47W") | Some("45W") => TdpClass::W47,
        Some("65W") => TdpClass::W65,
        Some("84W") | Some("88W") => TdpClass::W84,
        Some("91W") => TdpClass::W91,
        _ => TdpClass::Raw,
    }
}

#[cfg(test)]
mod tdp_tests {
    use super::parse_tdp_class;
    use clevo_proto::TdpClass;

    #[test]
    fn absent_or_unknown_is_raw_not_a_guess() {
        assert_eq!(parse_tdp_class(None), TdpClass::Raw);
        assert_eq!(parse_tdp_class(Some("")), TdpClass::Raw);
        assert_eq!(parse_tdp_class(Some("bogus")), TdpClass::Raw);
    }

    #[test]
    fn recognises_the_vendor_classes() {
        assert_eq!(parse_tdp_class(Some("35W")), TdpClass::W35);
        assert_eq!(parse_tdp_class(Some("47w")), TdpClass::W47);
        assert_eq!(parse_tdp_class(Some("65W")), TdpClass::W65);
        assert_eq!(parse_tdp_class(Some("84W")), TdpClass::W84);
        assert_eq!(parse_tdp_class(Some("91W")), TdpClass::W91);
    }
}

fn default_true() -> bool {
    true
}

fn default_keyboard_mode() -> String {
    KeyboardMode::Static.as_str().to_string()
}

fn default_keyboard_brightness() -> u8 {
    100
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
            fan_curve: None,
            factory_curve: None,
            keyboard: None,
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
    let mut config: Config = toml::from_str(text).map_err(ConfigError::Parse)?;
    if config.schema_version > SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedVersion(config.schema_version));
    }
    // Schema 4 stored keyboard brightness on the vendor's 0..4 scale; v5 uses a
    // 0..100 percentage. The two are indistinguishable by range once written
    // (a v5 "4" is 4%), so the version is the only reliable signal: scale a v4
    // value up before the version is normalised.
    if config.schema_version < 5 {
        if let Some(keyboard) = config.keyboard.as_mut() {
            keyboard.brightness = keyboard.brightness.min(4) * 25;
        }
    }
    // Missing fields already deserialize to `None`; normalizing the version
    // makes a subsequent save an explicit migration without changing the
    // user's existing modes or curve.
    if config.schema_version < SCHEMA_VERSION {
        config.schema_version = SCHEMA_VERSION;
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
            fan_curve: Some(FanCurveWire {
                cpu: [FanPointWire {
                    temp: 40,
                    duty_pct: 25,
                }; CURVE_POINTS],
                gpu1: [FanPointWire {
                    temp: 45,
                    duty_pct: 30,
                }; CURVE_POINTS],
                gpu2: [FanPointWire {
                    temp: 0,
                    duty_pct: 0,
                }; CURVE_POINTS],
            }),
            factory_curve: Some(FanCurveWire {
                cpu: [FanPointWire {
                    temp: 40,
                    duty_pct: 25,
                }; CURVE_POINTS],
                gpu1: [FanPointWire {
                    temp: 45,
                    duty_pct: 30,
                }; CURVE_POINTS],
                gpu2: [FanPointWire {
                    temp: 0,
                    duty_pct: 0,
                }; CURVE_POINTS],
            }),
            keyboard: Some(KeyboardConfig {
                mode: "static".into(),
                brightness: 75,
                keys: vec![KeyboardKeyWire {
                    row: 1,
                    col: 2,
                    color: KeyboardColorWire { r: 1, g: 2, b: 3 },
                }],
            }),
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
        assert!(parsed.fan_curve.is_none());
    }

    #[test]
    fn v1_file_is_read_and_upgraded() {
        let parsed =
            parse("schema_version = 1\nfan_mode = 8\nperf_mode = 2\napply_on_start = true\n")
                .unwrap();
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.fan_mode, Some(8));
        assert_eq!(parsed.perf_mode, Some(2));
        assert!(parsed.fan_curve.is_none());
        assert!(parsed.factory_curve.is_none());
        assert!(parsed.keyboard.is_none());
    }

    #[test]
    fn v2_file_is_read_and_upgraded() {
        // v2 is what shipped before the factory-curve snapshot. Its saved curve
        // must survive the upgrade; the new field is simply absent (and the
        // daemon will not guess it, see `capture_factory_curve`).
        let text = "\
schema_version = 2
fan_mode = 6
apply_on_start = true

[[fan_curve.cpu]]
temp = 40
duty_pct = 20

[[fan_curve.cpu]]
temp = 60
duty_pct = 40

[[fan_curve.cpu]]
temp = 80
duty_pct = 70

[[fan_curve.cpu]]
temp = 100
duty_pct = 100

[[fan_curve.gpu1]]
temp = 40
duty_pct = 20

[[fan_curve.gpu1]]
temp = 60
duty_pct = 40

[[fan_curve.gpu1]]
temp = 80
duty_pct = 70

[[fan_curve.gpu1]]
temp = 99
duty_pct = 100

[[fan_curve.gpu2]]
temp = 0
duty_pct = 0

[[fan_curve.gpu2]]
temp = 0
duty_pct = 0

[[fan_curve.gpu2]]
temp = 0
duty_pct = 0

[[fan_curve.gpu2]]
temp = 0
duty_pct = 0
";
        let parsed = parse(text).unwrap();
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.fan_mode, Some(6));
        let curve = parsed.fan_curve.expect("v2 curve kept");
        assert_eq!(curve.cpu[1].temp, 60);
        assert_eq!(curve.cpu[1].duty_pct, 40);
        assert!(parsed.factory_curve.is_none());
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
    fn v4_brightness_is_migrated_to_percent() {
        // v4 stored the vendor's 0..4 level; v5 uses 0..100. A v4 file must be
        // scaled, because "4" would otherwise be read as 4%.
        let text = "\
schema_version = 4
[keyboard]
mode = \"wave\"
brightness = 4
";
        let parsed = parse(text).unwrap();
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.keyboard.expect("keyboard kept").brightness, 100);

        // A v4 file with a mid level maps proportionally.
        let text = "\
schema_version = 4
[keyboard]
mode = \"static\"
brightness = 2
";
        let parsed = parse(text).unwrap();
        assert_eq!(parsed.keyboard.unwrap().brightness, 50);
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
