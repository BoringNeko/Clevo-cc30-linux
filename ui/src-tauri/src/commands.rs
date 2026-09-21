//! Tauri commands exposed to the frontend.
//!
//! Read commands wrap [`DaemonClient`] directly. Write commands go through the
//! daemon too, which is where PolicyKit authorization happens: the UI never
//! decides whether a write is allowed, and a denial comes back as an error the
//! UI must display rather than swallow.

use crate::dbus::{DaemonClient, FanCurve, FanSnapshot};

fn client() -> Result<DaemonClient, String> {
    DaemonClient::system().map_err(|e| e.message)
}

/// Return the daemon's cached snapshot without polling.
#[tauri::command]
pub fn get_fan_snapshot() -> Result<FanSnapshot, String> {
    client()?.snapshot().map_err(|e| e.message)
}

/// Ask the daemon to poll the hardware once, then return the new snapshot.
#[tauri::command]
pub fn poll_fan() -> Result<FanSnapshot, String> {
    let client = client()?;
    client.poll().map_err(|e| e.message)?;
    client.snapshot().map_err(|e| e.message)
}

/// Return the parsed fan curve.
#[tauri::command]
pub fn get_fan_curve() -> Result<FanCurve, String> {
    client()?.curve().map_err(|e| e.message)
}

/// Set the fan mode (`auto`/`max`/`maxq`/`quiet`); returns the applied value.
#[tauri::command]
pub fn set_fan_mode(mode: String) -> Result<u8, String> {
    client()?.set_fan_mode(&mode).map_err(|e| e.message)
}

/// Set the performance mode (`quiet`/`pwrsaving`/`performance`/`entertainment`).
#[tauri::command]
pub fn set_perf_mode(mode: String) -> Result<u8, String> {
    client()?.set_perf_mode(&mode).map_err(|e| e.message)
}

/// Read the launch-time compatibility preferences.
#[tauri::command]
pub fn get_launch_prefs() -> crate::prefs::LaunchPrefs {
    crate::prefs::load_launch_prefs()
}

/// Persist the launch-time compatibility preferences. They are applied the next
/// time the app is started via `scripts/run-ui.sh`.
#[tauri::command]
pub fn set_launch_prefs(prefs: crate::prefs::LaunchPrefs) -> Result<(), String> {
    crate::prefs::save_launch_prefs(&prefs)
}

// --- Wallpaper and logo persistence -----------------------------------------

use base64::Engine;

/// Save the wallpaper from a base64 payload; returns its data URL.
#[tauri::command]
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
#[tauri::command]
pub fn load_wallpaper() -> Option<String> {
    crate::assets::load_wallpaper()
}

/// Remove the saved wallpaper.
#[tauri::command]
pub fn clear_wallpaper() -> bool {
    crate::assets::clear_wallpaper()
}

/// Save a custom logo from a base64 payload; returns its data URL.
#[tauri::command]
pub fn save_logo(data_base64: String, ext: String) -> Result<crate::assets::SavedImage, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| format!("invalid base64: {e}"))?;
    crate::assets::save_logo(&bytes, &ext)
}

/// Load the saved custom logo as a data URL, if any.
#[tauri::command]
pub fn load_logo() -> Option<String> {
    crate::assets::load_logo()
}

/// Remove the saved custom logo.
#[tauri::command]
pub fn clear_logo() -> bool {
    crate::assets::clear_logo()
}

/// Read an arbitrary image path as a data URL (used for the bundled default logo).
#[tauri::command]
pub fn read_image_path(path: String) -> Option<String> {
    crate::assets::read_path_as_data_url(&path)
}

// --- Window / app lifecycle -------------------------------------------------

/// Show and focus the main control-center window.
#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::tray::show_main_window(&app)
}

/// Hide the main window to the tray, keeping the process alive.
#[tauri::command]
pub fn hide_main_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::tray::hide_main_window(&app)
}

/// Quit the whole application (tray and main window).
#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}
