//! Clevo control-center UI backend.
//!
//! Read-only in this phase: the Tauri commands wrap a D-Bus client for
//! `org.clevo.CC` and expose fan status and the fan curve to the React
//! frontend. The UI never reaches the hardware directly.

use tauri::{Manager, WindowEvent};

pub mod assets;
pub mod commands;
pub mod dbus;
pub mod prefs;
pub mod tray;

/// Run the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_fan_snapshot,
            commands::poll_fan,
            commands::get_fan_curve,
            commands::set_fan_curve,
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
            commands::show_main_window,
            commands::hide_main_window,
            commands::quit_app,
        ])
        .setup(|app| {
            // The tray icon lives for the whole process; its menu carries the
            // performance switcher, "open control center" and "quit".
            tray::build_tray(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Tauri application")
        .run(|app, event| match event {
            // Closing the last window must not quit: the control center keeps
            // running for its tray icon. Quitting is explicit (the tray's "退出"
            // item or the frontend's quit command).
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                api.prevent_close();
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.hide();
                }
            }
            _ => {}
        });
}
