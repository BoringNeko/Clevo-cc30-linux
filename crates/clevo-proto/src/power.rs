//! Shared, I/O-free power information exchanged over the daemon API.

/// A charge window. Some drivers provide only an end threshold.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChargeThresholds {
    /// FlexiCharger enable state; absent for standard threshold attributes.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub enabled: Option<bool>,
    /// Start charging below this percentage; absent on end-only drivers.
    pub start: Option<u8>,
    /// Stop charging at this percentage.
    pub end: u8,
}

/// Native firmware charge control, with its supported discrete percentages.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ChargeControl {
    /// Named control provider.
    pub backend: String,
    /// Allowed start percentages reported by firmware.
    pub start_values: Vec<u8>,
    /// Allowed end percentages reported by firmware.
    pub end_values: Vec<u8>,
}

/// One battery, with missing measurements represented explicitly.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Battery {
    /// Kernel power_supply name.
    pub name: String,
    /// Present battery, false when a removable battery is absent.
    pub present: bool,
    /// Current state of charge, in percent.
    pub capacity: Option<u8>,
    /// Kernel charging state.
    pub status: Option<String>,
    /// Kernel-reported health label.
    pub health: Option<String>,
    /// Full capacity divided by design capacity, using matching units.
    pub health_percent: Option<f64>,
    /// Full charge cycle count, if reported.
    pub cycle_count: Option<u64>,
    /// Current charging thresholds, if readable.
    pub thresholds: Option<ChargeThresholds>,
    /// Native provider metadata; absent for standard sysfs threshold controls.
    #[cfg_attr(feature = "serde", serde(default))]
    pub charge_control: Option<ChargeControl>,
    /// Whether the selected provider supports a start threshold.
    pub start_supported: bool,
    /// Whether the selected provider supports an end threshold.
    pub end_supported: bool,
    /// Per-attribute read errors; unsupported optional fields are omitted.
    pub errors: Vec<String>,
}

/// Desktop platform-profile interface, independent of battery support.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub struct PlatformProfile {
    /// Kernel profile provider name, if discoverable.
    pub driver: Option<String>,
    /// The last confirmed mode; unknown until a mode was actually applied.
    pub current: Option<String>,
    /// Profiles advertised by the kernel.
    pub choices: Vec<String>,
    /// Missing interface or an unreadable current profile.
    pub reason: Option<String>,
}

/// A fresh read of Linux power information.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub struct PowerSnapshot {
    /// Whether a real system backend is enabled.
    pub available: bool,
    /// Connected external supply, unknown when not reported.
    pub ac_online: Option<bool>,
    /// All battery power supplies, sorted by name.
    pub batteries: Vec<Battery>,
    /// Desktop power profile interface.
    pub platform_profile: PlatformProfile,
    /// Directory/supply errors that could not be assigned to one battery.
    pub errors: Vec<String>,
}
