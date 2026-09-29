//! USB HID keyboard RGB support for the ITE 829x controller.
//!
//! The wire format here is intentionally small and explicit.  The controller
//! used by the CC30 exposes a 16-byte feature report with report id `0xcc`.
//! The reverse-engineered Control Center sends command `1` for a single key,
//! command `0` with mode data for effects, and command `9` for brightness.
//! No ACPI or EC writes are involved in this module.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hidapi::{HidApi, HidDevice};

/// ITE keyboard controller vendor id.
pub const ITE_VENDOR_ID: u16 = 0x048d;
/// ITE per-key keyboard product id.
pub const ITE_PRODUCT_ID: u16 = 0x8910;
/// HID usage page used by the vendor lighting interface.
pub const ITE_USAGE_PAGE: u16 = 0xff89;
/// HID usage used by the vendor lighting interface.
pub const ITE_USAGE: u16 = 0x8910;
/// Feature report id used by the controller.
pub const REPORT_ID: u8 = 0xcc;
/// Feature report length used by the vendor utility.
pub const REPORT_LEN: usize = 16;
/// Rows in the verified static-color layout.
pub const KEYBOARD_ROWS: usize = 6;
/// Columns in the verified static-color layout.
pub const KEYBOARD_COLS: usize = 20;

/// An RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    /// Red component.
    pub r: u8,
    /// Green component.
    pub g: u8,
    /// Blue component.
    pub b: u8,
}

/// Supported controller modes verified from the vendor utility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardMode {
    /// Turn the keyboard lighting off.
    Off,
    /// Per-key static colors.
    #[default]
    Static,
    /// The controller's wave effect.
    Wave,
}

impl KeyboardMode {
    /// Parse the D-Bus/UI spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "static" => Some(Self::Static),
            "wave" => Some(Self::Wave),
            _ => None,
        }
    }

    /// Return the stable wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Static => "static",
            Self::Wave => "wave",
        }
    }
}

/// Logical zones used by the UI convenience operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardZone {
    /// All 6x20 key positions.
    All,
    /// Columns 0..=6.
    Left,
    /// Columns 7..=12.
    Middle,
    /// Columns 13..=19.
    Right,
}

impl KeyboardZone {
    /// Parse the D-Bus/UI spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "left" => Some(Self::Left),
            "middle" => Some(Self::Middle),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    fn columns(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Self::All => 0..=KEYBOARD_COLS - 1,
            Self::Left => 0..=6,
            Self::Middle => 7..=12,
            Self::Right => 13..=KEYBOARD_COLS - 1,
        }
    }
}

/// Information about a discovered keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardInfo {
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// hidraw path when available.
    pub path: String,
    /// Stable backend name used by diagnostics and the UI.
    pub backend: &'static str,
}

/// Cached keyboard state. The controller does not expose a reliable state read
/// for these settings, so this is the state successfully written this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardSnapshot {
    /// Device identity.
    pub info: KeyboardInfo,
    /// The device can accept feature reports.
    pub writable: bool,
    /// Last successfully selected mode.
    pub mode: KeyboardMode,
    /// Brightness level in the vendor's 0..=4 scale.
    pub brightness: u8,
    /// Last successfully written per-key colors.
    pub keys: [[Color; KEYBOARD_COLS]; KEYBOARD_ROWS],
}

/// Errors returned by keyboard discovery and writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyboardError {
    /// No compatible device was found.
    NotFound,
    /// The device exists but cannot be used by this process.
    PermissionDenied,
    /// A request is outside the verified controller range.
    Invalid(String),
    /// hidapi or hidraw reported an I/O failure.
    Io(String),
    /// A response did not have the expected shape.
    Protocol(String),
}

impl fmt::Display for KeyboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "no compatible ITE 048d:8910 keyboard controller found"),
            Self::PermissionDenied => {
                write!(f, "permission denied opening the keyboard hidraw device")
            }
            Self::Invalid(message) => write!(f, "invalid keyboard request: {message}"),
            Self::Io(message) => write!(f, "keyboard hidraw I/O error: {message}"),
            Self::Protocol(message) => write!(f, "keyboard HID protocol error: {message}"),
        }
    }
}

impl std::error::Error for KeyboardError {}

/// A keyboard backend. Implementations are safe to share between D-Bus calls.
pub trait Keyboard: Send + Sync {
    /// Current device and cached state.
    fn snapshot(&self) -> KeyboardSnapshot;
    /// Set a verified controller mode.
    fn set_mode(&self, mode: KeyboardMode) -> Result<(), KeyboardError>;
    /// Apply a color to a logical zone.
    fn set_zone(&self, zone: KeyboardZone, color: Color) -> Result<(), KeyboardError>;
    /// Apply a color to one key in the 6x20 layout.
    fn set_per_key(&self, row: u8, col: u8, color: Color) -> Result<(), KeyboardError>;
    /// Return the cached brightness level.
    fn brightness(&self) -> Result<u8, KeyboardError> {
        Ok(self.snapshot().brightness)
    }
    /// Set brightness in the vendor's 0..=4 scale.
    fn set_brightness(&self, level: u8) -> Result<(), KeyboardError>;
}

/// Build the 16-byte `SetLEDStatus` feature report.
pub fn build_feature_report(
    command: u8,
    data0: u8,
    data1: u8,
    data2: u8,
    data3: u8,
) -> [u8; REPORT_LEN] {
    [
        REPORT_ID, command, data0, data1, data2, data3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]
}

/// Build the request report used by the vendor's `GetLedData` helper.
pub fn build_get_feature_report(command: u8, data0: u8) -> [u8; REPORT_LEN] {
    build_feature_report(command, data0, 0, 0, 0)
}

/// Map the vendor UI's brightness level to the controller byte.
pub const fn brightness_raw(level: u8) -> Option<u8> {
    match level {
        0 => Some(0),
        1 => Some(2),
        2 => Some(4),
        3 => Some(6),
        4 => Some(10),
        _ => None,
    }
}

/// HID-backed keyboard implementation.
pub struct HidKeyboard {
    device: Mutex<HidDevice>,
    state: Mutex<KeyboardSnapshot>,
}

impl HidKeyboard {
    /// Find and open the first compatible ITE controller.
    pub fn discover() -> Result<Option<Self>, KeyboardError> {
        let api = HidApi::new().map_err(|e| KeyboardError::Io(e.to_string()))?;
        let candidates = api
            .device_list()
            .filter(|info| info.vendor_id() == ITE_VENDOR_ID && info.product_id() == ITE_PRODUCT_ID)
            .collect::<Vec<_>>();
        // Prefer the vendor lighting usage when the USB HID descriptor exposes
        // it. Some Linux hidraw stacks report zero usage metadata, so retain a
        // VID/PID fallback for those devices.
        let device_info = candidates
            .iter()
            .copied()
            .find(|info| info.usage_page() == ITE_USAGE_PAGE && info.usage() == ITE_USAGE)
            .or_else(|| candidates.first().copied())
            .ok_or(KeyboardError::NotFound);
        let Some(device_info) = device_info.ok() else {
            return Ok(None);
        };
        let device = device_info.open_device(&api).map_err(map_hid_error)?;
        let path = device_info.path().to_string_lossy().into_owned();
        let info = KeyboardInfo {
            vendor_id: device_info.vendor_id(),
            product_id: device_info.product_id(),
            path,
            backend: "usb-hid",
        };
        Ok(Some(Self {
            device: Mutex::new(device),
            state: Mutex::new(KeyboardSnapshot {
                info,
                writable: true,
                mode: KeyboardMode::Static,
                brightness: 4,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            }),
        }))
    }

    fn send(&self, report: &[u8; REPORT_LEN]) -> Result<(), KeyboardError> {
        self.device
            .lock()
            .map_err(|_| KeyboardError::Io("keyboard device lock poisoned".into()))?
            .send_feature_report(report)
            .map(|_| ())
            .map_err(map_hid_error)
    }

    fn update_key(&self, row: usize, col: usize, color: Color) {
        if let Ok(mut state) = self.state.lock() {
            state.keys[row][col] = color;
        }
    }
}

impl Keyboard for HidKeyboard {
    fn snapshot(&self) -> KeyboardSnapshot {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| KeyboardSnapshot {
                info: KeyboardInfo {
                    vendor_id: ITE_VENDOR_ID,
                    product_id: ITE_PRODUCT_ID,
                    path: String::new(),
                    backend: "usb-hid",
                },
                writable: false,
                mode: KeyboardMode::Static,
                brightness: 0,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            })
    }

    fn set_mode(&self, mode: KeyboardMode) -> Result<(), KeyboardError> {
        let report = match mode {
            KeyboardMode::Off => build_feature_report(9, 0, 0, 0, 0),
            KeyboardMode::Static => build_feature_report(0, 12, 0, 0, 0),
            KeyboardMode::Wave => build_feature_report(0, 4, 0, 0, 0),
        };
        self.send(&report)?;
        if let Ok(mut state) = self.state.lock() {
            state.mode = mode;
        }
        Ok(())
    }

    fn set_zone(&self, zone: KeyboardZone, color: Color) -> Result<(), KeyboardError> {
        for row in 0..KEYBOARD_ROWS {
            for col in zone.columns() {
                self.set_per_key(row as u8, col as u8, color)?;
            }
        }
        Ok(())
    }

    fn set_per_key(&self, row: u8, col: u8, color: Color) -> Result<(), KeyboardError> {
        if usize::from(row) >= KEYBOARD_ROWS || usize::from(col) >= KEYBOARD_COLS {
            return Err(KeyboardError::Invalid(format!(
                "key position ({row},{col}) is outside 6x20"
            )));
        }
        let key = (row << 5) | col;
        let report = build_feature_report(1, key, color.r, color.g, color.b);
        self.send(&report)?;
        self.update_key(usize::from(row), usize::from(col), color);
        Ok(())
    }

    fn set_brightness(&self, level: u8) -> Result<(), KeyboardError> {
        let raw = brightness_raw(level).ok_or_else(|| {
            KeyboardError::Invalid(format!("brightness {level} is outside 0..=4"))
        })?;
        self.send(&build_feature_report(9, raw, 0, 0, 0))?;
        if let Ok(mut state) = self.state.lock() {
            state.brightness = level;
        }
        Ok(())
    }
}

/// Default sysfs node exposed by the ACPI/DCHU RGB15 kernel backend.
pub const DEFAULT_ACPI_KEYBOARD_PATH: &str = "/sys/devices/platform/CLV0001:00/keyboard_rgb";

/// ACPI/DCHU RGB15 backend used by machines whose built-in keyboard is not a
/// USB HID controller. The kernel driver owns the `_DSM` call and exposes only
/// named RGB15 operations through this file.
pub struct AcpiKeyboard {
    path: PathBuf,
    state: Mutex<KeyboardSnapshot>,
}

impl AcpiKeyboard {
    /// Discover the kernel driver's RGB15 sysfs interface.
    pub fn discover() -> Result<Option<Self>, KeyboardError> {
        let path = Path::new(DEFAULT_ACPI_KEYBOARD_PATH);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(Self::with_path(path)))
    }

    /// Use an explicit sysfs path, primarily for tests and non-standard
    /// platform-device layouts.
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            state: Mutex::new(KeyboardSnapshot {
                info: KeyboardInfo {
                    vendor_id: 0,
                    product_id: 0,
                    path: path.display().to_string(),
                    backend: "acpi-dchu",
                },
                writable: true,
                mode: KeyboardMode::Static,
                brightness: 4,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            }),
            path,
        }
    }

    fn write_operation(&self, operation: &str) -> Result<(), KeyboardError> {
        std::fs::write(&self.path, format!("{operation}\n")).map_err(|error| {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                KeyboardError::PermissionDenied
            } else {
                KeyboardError::Io(format!("{}: {error}", self.path.display()))
            }
        })
    }

    fn write_zone(&self, _zone: KeyboardZone, color: Color) -> Result<(), KeyboardError> {
        let value = format!("{:02x}{:02x}{:02x}", color.r, color.g, color.b);
        // COLORFUL P15 23 reports kb_type=6 but has one physical RGB15
        // channel. The firmware accepts the vendor's other zone selectors,
        // yet they do not address LEDs. Keep the logical API compatible while
        // routing every request to the single channel.
        self.write_operation(&format!("all {value}"))
    }

    fn update_zone(&self, _zone: KeyboardZone, color: Color) {
        if let Ok(mut state) = self.state.lock() {
            for row in 0..KEYBOARD_ROWS {
                for col in KeyboardZone::All.columns() {
                    state.keys[row][col] = color;
                }
            }
        }
    }
}

impl Keyboard for AcpiKeyboard {
    fn snapshot(&self) -> KeyboardSnapshot {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| KeyboardSnapshot {
                info: KeyboardInfo {
                    vendor_id: 0,
                    product_id: 0,
                    path: self.path.display().to_string(),
                    backend: "acpi-dchu",
                },
                writable: false,
                mode: KeyboardMode::Static,
                brightness: 0,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            })
    }

    fn set_mode(&self, mode: KeyboardMode) -> Result<(), KeyboardError> {
        self.write_operation(&format!("mode {}", mode.as_str()))?;
        if let Ok(mut state) = self.state.lock() {
            state.mode = mode;
        }
        Ok(())
    }

    fn set_zone(&self, zone: KeyboardZone, color: Color) -> Result<(), KeyboardError> {
        self.write_zone(zone, color)?;
        self.update_zone(zone, color);
        Ok(())
    }

    fn set_per_key(&self, row: u8, col: u8, color: Color) -> Result<(), KeyboardError> {
        if usize::from(row) >= KEYBOARD_ROWS || usize::from(col) >= KEYBOARD_COLS {
            return Err(KeyboardError::Invalid(format!(
                "key position ({row},{col}) is outside 6x20"
            )));
        }
        // RGB15 on this machine has one physical channel rather than
        // independent keys or three addressable zones.
        self.set_zone(KeyboardZone::All, color)
    }

    fn set_brightness(&self, level: u8) -> Result<(), KeyboardError> {
        if level > 4 {
            return Err(KeyboardError::Invalid(format!(
                "brightness {level} is outside 0..=4"
            )));
        }
        self.write_operation(&format!("brightness {level}"))?;
        if let Ok(mut state) = self.state.lock() {
            state.brightness = level;
        }
        Ok(())
    }
}

/// In-memory backend used by tests and offline development.
pub struct MockKeyboard {
    state: Mutex<KeyboardSnapshot>,
}

impl Default for MockKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl MockKeyboard {
    /// Create a fully writable mock keyboard.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(KeyboardSnapshot {
                info: KeyboardInfo {
                    vendor_id: ITE_VENDOR_ID,
                    product_id: ITE_PRODUCT_ID,
                    path: "mock://keyboard".into(),
                    backend: "mock",
                },
                writable: true,
                mode: KeyboardMode::Static,
                brightness: 4,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            }),
        }
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, KeyboardSnapshot>, KeyboardError> {
        self.state
            .lock()
            .map_err(|_| KeyboardError::Io("mock keyboard lock poisoned".into()))
    }
}

impl Keyboard for MockKeyboard {
    fn snapshot(&self) -> KeyboardSnapshot {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| KeyboardSnapshot {
                info: KeyboardInfo {
                    vendor_id: ITE_VENDOR_ID,
                    product_id: ITE_PRODUCT_ID,
                    path: "mock://keyboard".into(),
                    backend: "mock",
                },
                writable: false,
                mode: KeyboardMode::Static,
                brightness: 0,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            })
    }

    fn set_mode(&self, mode: KeyboardMode) -> Result<(), KeyboardError> {
        self.state()?.mode = mode;
        Ok(())
    }

    fn set_zone(&self, zone: KeyboardZone, color: Color) -> Result<(), KeyboardError> {
        let mut state = self.state()?;
        for row in 0..KEYBOARD_ROWS {
            for col in zone.columns() {
                state.keys[row][col] = color;
            }
        }
        Ok(())
    }

    fn set_per_key(&self, row: u8, col: u8, color: Color) -> Result<(), KeyboardError> {
        if usize::from(row) >= KEYBOARD_ROWS || usize::from(col) >= KEYBOARD_COLS {
            return Err(KeyboardError::Invalid(format!(
                "key position ({row},{col}) is outside 6x20"
            )));
        }
        self.state()?.keys[usize::from(row)][usize::from(col)] = color;
        Ok(())
    }

    fn set_brightness(&self, level: u8) -> Result<(), KeyboardError> {
        brightness_raw(level).ok_or_else(|| {
            KeyboardError::Invalid(format!("brightness {level} is outside 0..=4"))
        })?;
        self.state()?.brightness = level;
        Ok(())
    }
}

fn map_hid_error(error: hidapi::HidError) -> KeyboardError {
    let message = error.to_string();
    if message.to_ascii_lowercase().contains("permission") {
        KeyboardError::PermissionDenied
    } else {
        KeyboardError::Io(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_report_matches_vendor_layout() {
        assert_eq!(
            build_feature_report(1, 0x25, 0x10, 0x20, 0x30),
            [0xcc, 1, 0x25, 0x10, 0x20, 0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn get_report_starts_with_report_id_and_command_type() {
        assert_eq!(build_get_feature_report(17, 1)[..3], [0xcc, 17, 1]);
    }

    #[test]
    fn brightness_mapping_is_explicit() {
        assert_eq!(brightness_raw(0), Some(0));
        assert_eq!(brightness_raw(1), Some(2));
        assert_eq!(brightness_raw(2), Some(4));
        assert_eq!(brightness_raw(3), Some(6));
        assert_eq!(brightness_raw(4), Some(10));
        assert_eq!(brightness_raw(5), None);
    }

    #[test]
    fn mock_updates_per_key_and_zone() {
        let keyboard = MockKeyboard::new();
        let red = Color { r: 255, g: 0, b: 0 };
        keyboard.set_per_key(2, 3, red).unwrap();
        keyboard.set_zone(KeyboardZone::Right, red).unwrap();
        let state = keyboard.snapshot();
        assert_eq!(state.keys[2][3], red);
        assert_eq!(state.keys[0][19], red);
        assert_eq!(state.keys[0][12], Color::default());
    }

    #[test]
    fn acpi_backend_writes_named_rgb15_operations() {
        let path = std::env::temp_dir().join(format!(
            "clevo-acpi-keyboard-{}-{}",
            std::process::id(),
            "operations"
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "").unwrap();
        let keyboard = AcpiKeyboard::with_path(&path);
        let blue = Color { r: 0, g: 0, b: 255 };

        keyboard.set_zone(KeyboardZone::Right, blue).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "all 0000ff\n");
        keyboard.set_mode(KeyboardMode::Wave).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mode wave\n");
        keyboard.set_brightness(3).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "brightness 3\n");

        let state = keyboard.snapshot();
        assert_eq!(state.info.backend, "acpi-dchu");
        assert_eq!(state.keys[0][19], blue);
        assert_eq!(state.keys[0][12], blue);
        assert_eq!(state.mode, KeyboardMode::Wave);
        assert_eq!(state.brightness, 3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn acpi_per_key_requests_target_the_physical_zone() {
        let path = std::env::temp_dir().join(format!(
            "clevo-acpi-keyboard-{}-{}",
            std::process::id(),
            "per-key"
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "").unwrap();
        let keyboard = AcpiKeyboard::with_path(&path);

        keyboard
            .set_per_key(2, 8, Color { r: 1, g: 2, b: 3 })
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "all 010203\n");
        let _ = std::fs::remove_file(path);
    }
}
