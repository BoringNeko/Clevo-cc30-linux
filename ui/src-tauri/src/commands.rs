//! Backend commands exposed to the frontend.
//!
//! Read commands wrap [`DaemonClient`] directly. Write commands go through the
//! daemon too, which is where PolicyKit authorization happens: the UI never
//! decides whether a write is allowed, and a denial comes back as an error the
//! UI must display rather than swallow.
//!
//! These functions are shell-agnostic: the Tauri build registers them as
//! `#[tauri::command]`s, while the Electron build dispatches them by name over
//! the localhost bridge (see [`crate::serve`]). Only the window/tray lifecycle
//! at the bottom is Tauri-specific and lives behind the `tauri-shell` feature.

use crate::dbus::{DaemonClient, FanCurve, FanSnapshot};

fn client() -> Result<DaemonClient, String> {
    DaemonClient::system().map_err(|e| e.message)
}

/// Return the daemon's cached snapshot without polling.
pub fn get_fan_snapshot() -> Result<FanSnapshot, String> {
    client()?.snapshot().map_err(|e| e.message)
}

/// Ask the daemon to poll the hardware once, then return the new snapshot.
pub fn poll_fan() -> Result<FanSnapshot, String> {
    let client = client()?;
    client.poll().map_err(|e| e.message)?;
    client.snapshot().map_err(|e| e.message)
}

/// Read host CPU/GPU utilisation and the latest daemon temperatures.
pub fn get_hardware_usage() -> Result<crate::usage::HardwareUsage, String> {
    crate::usage::read()
}

/// Return the parsed fan curve.
pub fn get_fan_curve() -> Result<FanCurve, String> {
    client()?.curve().map_err(|e| e.message)
}

/// Set the fan mode (`auto`/`max`/`maxq`/`quiet`); returns the applied value.
pub fn set_fan_mode(mode: String) -> Result<u8, String> {
    client()?.set_fan_mode(&mode).map_err(|e| e.message)
}

/// Set the performance mode (`quiet`/`pwrsaving`/`performance`/`entertainment`).
pub fn set_perf_mode(mode: String) -> Result<u8, String> {
    client()?.set_perf_mode(&mode).map_err(|e| e.message)
}

/// Write a custom fan curve and select the `custom` fan mode.
///
/// `curve` is the same shape [`get_fan_curve`] returns, so the UI can read the
/// current curve, let the user drag points, and send it straight back.
pub fn set_fan_curve(curve: crate::dbus::FanCurve) -> Result<(), String> {
    let json = curve_to_json(&curve);
    client()?.set_curve(&json).map_err(|e| e.message)
}

/// Serialize a curve back into the daemon's JSON wire shape.
///
/// Duty goes out as the percentage the daemon expects; `clevo-proto` converts
/// it to the EC's raw value on the way down.
///
/// `pub` so the round-trip (read a curve, write it back unchanged) can be
/// tested against a real daemon: `SetCurve` rejects anything above 100%, which
/// is exactly what a second conversion here would produce.
pub fn curve_to_json(curve: &crate::dbus::FanCurve) -> String {
    let points = |points: &[crate::dbus::CurvePoint]| {
        points
            .iter()
            .map(|p| format!("[{},{}]", p.temp, p.duty_pct))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{{\"cpu\":[{}],\"gpu1\":[{}],\"gpu2\":[{}]}}",
        points(&curve.cpu),
        points(&curve.gpu1),
        points(&curve.gpu2)
    )
}

/// Read the launch-time compatibility preferences.
pub fn get_launch_prefs() -> crate::prefs::LaunchPrefs {
    crate::prefs::load_launch_prefs()
}

/// Persist the launch-time compatibility preferences.
///
/// Under Tauri they take effect the next time the app is started through
/// `scripts/run-ui.sh`. Under Electron the shell reads the same file before it
/// creates the window, so a restart applies them there too.
pub fn set_launch_prefs(prefs: crate::prefs::LaunchPrefs) -> Result<(), String> {
    crate::prefs::save_launch_prefs(&prefs)
}

// --- Wallpaper and logo persistence -----------------------------------------

use base64::Engine;

/// Save the wallpaper from a base64 payload; returns its data URL.
pub fn save_wallpaper(
    data_base64: String,
    ext: String,
) -> Result<crate::assets::SavedImage, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| format!("invalid base64: {e}"))?;
    crate::assets::save_wallpaper(&bytes, &ext)
}

/// Load the saved wallpaper as a data URL, if any.
pub fn load_wallpaper() -> Option<String> {
    crate::assets::load_wallpaper()
}

/// Remove the saved wallpaper.
pub fn clear_wallpaper() -> bool {
    crate::assets::clear_wallpaper()
}

/// Save a custom logo from a base64 payload; returns its data URL.
pub fn save_logo(data_base64: String, ext: String) -> Result<crate::assets::SavedImage, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| format!("invalid base64: {e}"))?;
    crate::assets::save_logo(&bytes, &ext)
}

/// Load the saved custom logo as a data URL, if any.
pub fn load_logo() -> Option<String> {
    crate::assets::load_logo()
}

/// Remove the saved custom logo.
pub fn clear_logo() -> bool {
    crate::assets::clear_logo()
}

/// Read an arbitrary image path as a data URL (used for the bundled default logo).
pub fn read_image_path(path: String) -> Option<String> {
    crate::assets::read_path_as_data_url(&path)
}

// --- Window / app lifecycle -------------------------------------------------
//
// Tauri only. In the Electron build the shell owns the window and tray, and the
// renderer calls the shell directly for these (see `ui/electron/main.cjs`), so
// the backend has nothing to expose.

/// Show and focus the main control-center window.
#[cfg(feature = "tauri-shell")]
pub fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::tray::show_main_window(&app)
}

/// Hide the main window to the tray, keeping the process alive.
#[cfg(feature = "tauri-shell")]
pub fn hide_main_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::tray::hide_main_window(&app)
}

/// Quit the whole application (tray and main window).
#[cfg(feature = "tauri-shell")]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// --- Tauri command wrappers -------------------------------------------------
//
// The `#[tauri::command]` macro generates an extra layer that captures
// `AppHandle`s; keeping it in its own block leaves the plain functions above
// callable from the Electron bridge unchanged.

/// The Tauri command surface, re-exporting the shell-agnostic functions.
#[cfg(feature = "tauri-shell")]
pub mod tauri_commands {
    use super::*;

    #[tauri::command]
    pub fn get_fan_snapshot() -> Result<FanSnapshot, String> {
        super::get_fan_snapshot()
    }

    #[tauri::command]
    pub fn poll_fan() -> Result<FanSnapshot, String> {
        super::poll_fan()
    }

    #[tauri::command]
    pub fn get_hardware_usage() -> Result<crate::usage::HardwareUsage, String> {
        super::get_hardware_usage()
    }

    #[tauri::command]
    pub fn get_fan_curve() -> Result<FanCurve, String> {
        super::get_fan_curve()
    }

    #[tauri::command]
    pub fn set_fan_mode(mode: String) -> Result<u8, String> {
        super::set_fan_mode(mode)
    }

    #[tauri::command]
    pub fn set_perf_mode(mode: String) -> Result<u8, String> {
        super::set_perf_mode(mode)
    }

    #[tauri::command]
    pub fn set_fan_curve(curve: FanCurve) -> Result<(), String> {
        super::set_fan_curve(curve)
    }

    #[tauri::command]
    pub fn get_launch_prefs() -> crate::prefs::LaunchPrefs {
        super::get_launch_prefs()
    }

    #[tauri::command]
    pub fn set_launch_prefs(prefs: crate::prefs::LaunchPrefs) -> Result<(), String> {
        super::set_launch_prefs(prefs)
    }

    #[tauri::command]
    pub fn save_wallpaper(
        data_base64: String,
        ext: String,
    ) -> Result<crate::assets::SavedImage, String> {
        super::save_wallpaper(data_base64, ext)
    }

    #[tauri::command]
    pub fn load_wallpaper() -> Option<String> {
        super::load_wallpaper()
    }

    #[tauri::command]
    pub fn clear_wallpaper() -> bool {
        super::clear_wallpaper()
    }

    #[tauri::command]
    pub fn save_logo(
        data_base64: String,
        ext: String,
    ) -> Result<crate::assets::SavedImage, String> {
        super::save_logo(data_base64, ext)
    }

    #[tauri::command]
    pub fn load_logo() -> Option<String> {
        super::load_logo()
    }

    #[tauri::command]
    pub fn clear_logo() -> bool {
        super::clear_logo()
    }

    #[tauri::command]
    pub fn read_image_path(path: String) -> Option<String> {
        super::read_image_path(path)
    }

    #[tauri::command]
    pub fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
        super::show_main_window(app)
    }

    #[tauri::command]
    pub fn hide_main_window(app: tauri::AppHandle) -> Result<(), String> {
        super::hide_main_window(app)
    }

    #[tauri::command]
    pub fn quit_app(app: tauri::AppHandle) {
        super::quit_app(app)
    }
}
