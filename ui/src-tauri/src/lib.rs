//! Clevo control-center UI backend.
//!
//! Read-only in this phase: the Tauri commands wrap a D-Bus client for
//! `org.clevo.CC` and expose fan status and the fan curve to the React
//! frontend. The UI never reaches the hardware directly.

pub mod assets;
pub mod commands;
pub mod dbus;
pub mod prefs;

/// Run the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_fan_snapshot,
            commands::poll_fan,
            commands::get_fan_curve,
            commands::set_fan_mode,
            commands::set_perf_mode,
            commands::get_launch_prefs,
            commands::set_launch_prefs,
            commands::save_wallpaper,
            commands::load_wallpaper,
            commands::clear_wallpaper,
            commands::save_logo,
            commands::load_logo,
            commands::clear_logo,
            commands::read_image_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
