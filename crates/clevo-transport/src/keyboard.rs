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
/// Highest brightness the UI, daemon and D-Bus accept, as a percentage.
///
/// A single 0..=100 scale is used across every backend; it is converted to the
/// backend's native representation at the hardware boundary. The analog RGB15
/// channel is driven with the raw 0..=191 byte, so 100% means the EC maximum
/// rather than the vendor's calibrated step 4.
pub const BRIGHTNESS_PERCENT_MAX: u8 = 100;
/// Raw RGB15 brightness byte accepted by the EC (the hardware maximum).
pub const BRIGHTNESS_RAW_MAX: u16 = 191;

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

/// Keyboard lighting effects.
///
/// `Off`, `Static` and `Wave` are common to every backend. The remaining
/// variants are the firmware's native RGB15 effects (the words come from the
/// vendor's RGBKB.SetMode for kb_type 6/22); a backend that cannot drive them
/// simply does not list them in its [`KeyboardInfo::modes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardMode {
    /// Turn the keyboard lighting off.
    Off,
    /// Per-key static colors.
    #[default]
    Static,
    /// Single-color breathing pulse.
    Breath,
    /// Sequential color cycling.
    Cycle,
    /// Flowing wave.
    Wave,
    /// Rhythmic dance pattern.
    Dance,
    /// Tempo-synced pulse.
    Tempo,
    /// Strobe / flash.
    Flash,
    /// Random color sparkle.
    Random,
}

/// Every mode, in the display order the UI should use.
pub const ALL_KEYBOARD_MODES: &[KeyboardMode] = &[
    KeyboardMode::Off,
    KeyboardMode::Static,
    KeyboardMode::Breath,
    KeyboardMode::Cycle,
    KeyboardMode::Wave,
    KeyboardMode::Dance,
    KeyboardMode::Tempo,
    KeyboardMode::Flash,
    KeyboardMode::Random,
];

/// Modes the ITE USB HID backend can drive (its own command-0 effect set is
/// only partially reverse-engineered, so only the verified ones are offered).
pub const USB_HID_KEYBOARD_MODES: &[KeyboardMode] =
    &[KeyboardMode::Off, KeyboardMode::Static, KeyboardMode::Wave];

/// Effects the ACPI-DCHU RGB15 backend offers.
///
/// Measured on the COLORFUL P15 23 (single-zone RGB15, `kb_type=6`): the DSDT's
/// command-103 handler *does* implement the vendor effect words, but the EC
/// never animates for them — not even for a bare word sent with no status,
/// brightness or colour write around it (see the kernel driver's `raw-effect`
/// diagnostic). The vendor utility agrees: it only shows its effect panel for
/// multi-zone models and gives `kb_type 6` static colour plus brightness.
///
/// So this backend offers `off`/`static` only. The wider [`KeyboardMode`] set
/// stays defined because the kernel interface still speaks those words and a
/// multi-zone RGB15 board could report them without any caller changing.
pub const ACPI_KEYBOARD_MODES: &[KeyboardMode] = &[KeyboardMode::Off, KeyboardMode::Static];

impl KeyboardMode {
    /// Parse the D-Bus/UI spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "static" => Some(Self::Static),
            "breath" => Some(Self::Breath),
            "cycle" => Some(Self::Cycle),
            "wave" => Some(Self::Wave),
            "dance" => Some(Self::Dance),
            "tempo" => Some(Self::Tempo),
            "flash" => Some(Self::Flash),
            "random" => Some(Self::Random),
            _ => None,
        }
    }

    /// Return the stable wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Static => "static",
            Self::Breath => "breath",
            Self::Cycle => "cycle",
            Self::Wave => "wave",
            Self::Dance => "dance",
            Self::Tempo => "tempo",
            Self::Flash => "flash",
            Self::Random => "random",
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
    /// Effects this backend can actually drive, in display order.
    pub modes: &'static [KeyboardMode],
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
    /// Brightness as a percentage in `0..=100`.
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
///
/// Brightness is a percentage in `0..=100` on every backend; each converts it to
/// its native representation (the RGB15 raw byte, the HID controller's step).
pub trait Keyboard: Send + Sync {
    /// Current device and cached state.
    fn snapshot(&self) -> KeyboardSnapshot;
    /// Set a verified controller mode.
    fn set_mode(&self, mode: KeyboardMode) -> Result<(), KeyboardError>;
    /// Apply a color to a logical zone.
    fn set_zone(&self, zone: KeyboardZone, color: Color) -> Result<(), KeyboardError>;
    /// Apply a color to one key in the 6x20 layout.
    fn set_per_key(&self, row: u8, col: u8, color: Color) -> Result<(), KeyboardError>;
    /// Return the cached brightness percentage.
    fn brightness(&self) -> Result<u8, KeyboardError> {
        Ok(self.snapshot().brightness)
    }
    /// Set brightness as a percentage in `0..=100`.
    fn set_brightness(&self, percent: u8) -> Result<(), KeyboardError>;
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

/// Map a brightness percentage to the ITE controller's byte.
///
/// The vendor utility only ever sent five calibrated bytes (0, 2, 4, 6, 10), so
/// a percentage is snapped to the nearest verified value rather than being driven
/// beyond what the vendor used. Returns `None` above 100%.
pub const fn brightness_raw(percent: u8) -> Option<u8> {
    if percent > BRIGHTNESS_PERCENT_MAX {
        return None;
    }
    // Midpoints between the vendor's five levels (0, 25, 50, 75, 100%).
    Some(if percent < 13 {
        0
    } else if percent < 38 {
        2
    } else if percent < 63 {
        4
    } else if percent < 88 {
        6
    } else {
        10
    })
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
            modes: USB_HID_KEYBOARD_MODES,
        };
        Ok(Some(Self {
            device: Mutex::new(device),
            state: Mutex::new(KeyboardSnapshot {
                info,
                writable: true,
                mode: KeyboardMode::Static,
                brightness: 100,
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
                    modes: USB_HID_KEYBOARD_MODES,
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
            other => {
                return Err(KeyboardError::Invalid(format!(
                    "effect {} is not supported by the USB HID backend",
                    other.as_str()
                )))
            }
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

    fn set_brightness(&self, percent: u8) -> Result<(), KeyboardError> {
        let raw = brightness_raw(percent).ok_or_else(|| {
            KeyboardError::Invalid(format!("brightness {percent} is outside 0..=100"))
        })?;
        self.send(&build_feature_report(9, raw, 0, 0, 0))?;
        if let Ok(mut state) = self.state.lock() {
            state.brightness = percent;
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
                    modes: ACPI_KEYBOARD_MODES,
                },
                writable: true,
                mode: KeyboardMode::Static,
                brightness: 100,
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
                    modes: ACPI_KEYBOARD_MODES,
                },
                writable: false,
                mode: KeyboardMode::Static,
                brightness: 0,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            })
    }

    fn set_mode(&self, mode: KeyboardMode) -> Result<(), KeyboardError> {
        // Keep the write path in step with the advertised capability: a mode
        // this backend does not list must not reach the hardware (a bare
        // command-103 word is accepted by the EC yet does nothing).
        if !ACPI_KEYBOARD_MODES.contains(&mode) {
            return Err(KeyboardError::Invalid(format!(
                "effect {} is not supported by the ACPI-DCHU backend",
                mode.as_str()
            )));
        }
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

    fn set_brightness(&self, percent: u8) -> Result<(), KeyboardError> {
        if percent > BRIGHTNESS_PERCENT_MAX {
            return Err(KeyboardError::Invalid(format!(
                "brightness {percent} is outside 0..=100"
            )));
        }
        self.write_operation(&format!("brightness {percent}"))?;
        if let Ok(mut state) = self.state.lock() {
            state.brightness = percent;
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
                    modes: ACPI_KEYBOARD_MODES,
                },
                writable: true,
                mode: KeyboardMode::Static,
                brightness: 100,
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
                    modes: ACPI_KEYBOARD_MODES,
                },
                writable: false,
                mode: KeyboardMode::Static,
                brightness: 0,
                keys: [[Color::default(); KEYBOARD_COLS]; KEYBOARD_ROWS],
            })
    }

    fn set_mode(&self, mode: KeyboardMode) -> Result<(), KeyboardError> {
        if !ACPI_KEYBOARD_MODES.contains(&mode) {
            return Err(KeyboardError::Invalid(format!(
                "effect {} is not supported by this backend",
                mode.as_str()
            )));
        }
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

    fn set_brightness(&self, percent: u8) -> Result<(), KeyboardError> {
        brightness_raw(percent).ok_or_else(|| {
            KeyboardError::Invalid(format!("brightness {percent} is outside 0..=100"))
        })?;
        self.state()?.brightness = percent;
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
    fn brightness_mapping_snaps_to_vendor_levels() {
        // The five bytes the vendor utility used, at their percentage points.
        assert_eq!(brightness_raw(0), Some(0));
        assert_eq!(brightness_raw(25), Some(2));
        assert_eq!(brightness_raw(50), Some(4));
        assert_eq!(brightness_raw(75), Some(6));
        assert_eq!(brightness_raw(100), Some(10));
        // Snapped to the nearest verified byte, never beyond them.
        assert_eq!(brightness_raw(13), Some(2));
        assert_eq!(brightness_raw(80), Some(6));
        assert_eq!(brightness_raw(101), None);
        assert_eq!(brightness_raw(255), None);
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
        keyboard.set_mode(KeyboardMode::Static).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mode static\n");
        keyboard.set_brightness(60).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "brightness 60\n");

        let state = keyboard.snapshot();
        assert_eq!(state.info.backend, "acpi-dchu");
        assert_eq!(state.info.modes, ACPI_KEYBOARD_MODES);
        assert_eq!(state.keys[0][19], blue);
        assert_eq!(state.keys[0][12], blue);
        assert_eq!(state.mode, KeyboardMode::Static);
        assert_eq!(state.brightness, 60);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn acpi_backend_refuses_effects_it_does_not_advertise() {
        // The single-zone RGB15 EC accepts the vendor effect words yet never
        // animates for them, so this backend must not offer them and must not
        // forward them to the hardware either.
        let path = std::env::temp_dir().join(format!(
            "clevo-acpi-keyboard-{}-{}",
            std::process::id(),
            "effects"
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "").unwrap();
        let keyboard = AcpiKeyboard::with_path(&path);

        assert_eq!(
            keyboard.snapshot().info.modes,
            [KeyboardMode::Off, KeyboardMode::Static]
        );
        for effect in [
            KeyboardMode::Breath,
            KeyboardMode::Cycle,
            KeyboardMode::Wave,
            KeyboardMode::Dance,
            KeyboardMode::Tempo,
            KeyboardMode::Flash,
            KeyboardMode::Random,
        ] {
            assert!(
                keyboard.set_mode(effect).is_err(),
                "{} must be rejected before touching the hardware",
                effect.as_str()
            );
        }
        // Nothing was written, so the sysfs node is still empty.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn hid_backend_offers_and_rejects_the_verified_set() {
        // The USB HID backend only implements the verified command-0 set.
        assert!(USB_HID_KEYBOARD_MODES.contains(&KeyboardMode::Wave));
        assert!(!USB_HID_KEYBOARD_MODES.contains(&KeyboardMode::Breath));
        // Every backend must be able to turn the lights off and go static.
        for modes in [USB_HID_KEYBOARD_MODES, ACPI_KEYBOARD_MODES] {
            assert!(modes.contains(&KeyboardMode::Off));
            assert!(modes.contains(&KeyboardMode::Static));
        }
    }

    #[test]
    fn mode_spellings_round_trip() {
        for mode in ALL_KEYBOARD_MODES {
            assert_eq!(KeyboardMode::parse(mode.as_str()), Some(*mode));
        }
        assert_eq!(KeyboardMode::parse("spectrum"), None);
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
