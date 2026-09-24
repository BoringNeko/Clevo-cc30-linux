//! Clevo control-center UI backend.
//!
//! The backend wraps a D-Bus client for `org.clevo.CC` and exposes fan status,
//! the fan curve, the fan/performance modes and the appearance assets. The UI
//! never reaches the hardware directly.
//!
//! It is built in two shapes from the same crate:
//!
//! * `default` (`tauri-shell`): an in-process Tauri window and tray — the
//!   original desktop app.
//! * `--no-default-features`: a headless binary with a localhost JSON bridge
//!   (`serve`), spawned and proxied by the Electron shell (`ui/electron/`).
//!   This drops the whole Tauri/GTK/Wry stack, which is the point: Electron
//!   brings its own Chromium, so the Tauri WebKitGTK (and its NVIDIA GBM/EGL
//!   problems, see `docs/hardware-notes.md` §15) is not involved at all.

pub mod assets;
pub mod commands;
pub mod dbus;
pub mod launch_env;
pub mod prefs;
pub mod serve;
pub mod usage;

#[cfg(feature = "tauri-shell")]
pub mod tray;

/// Serialises tests that mutate the process environment.
///
/// `prefs::tests` and `launch_env::tests` both set and clear the same
/// variables, and the process environment is global, so they must not run
/// concurrently. Cargo runs test functions on multiple threads, so a shared
/// lock is the only thing that keeps them from clobbering each other.
#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run the Tauri application.
#[cfg(feature = "tauri-shell")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::tauri_commands::get_fan_snapshot,
            commands::tauri_commands::poll_fan,
            commands::tauri_commands::get_hardware_usage,
            commands::tauri_commands::get_fan_curve,
            commands::tauri_commands::set_fan_curve,
            commands::tauri_commands::set_fan_mode,
            commands::tauri_commands::set_perf_mode,
            commands::tauri_commands::get_launch_prefs,
            commands::tauri_commands::set_launch_prefs,
            commands::tauri_commands::save_wallpaper,
            commands::tauri_commands::load_wallpaper,
            commands::tauri_commands::clear_wallpaper,
            commands::tauri_commands::save_logo,
            commands::tauri_commands::load_logo,
            commands::tauri_commands::clear_logo,
            commands::tauri_commands::read_image_path,
            commands::tauri_commands::show_main_window,
            commands::tauri_commands::hide_main_window,
            commands::tauri_commands::quit_app,
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
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                api.prevent_close();
                if let Some(window) = tauri::Manager::get_webview_window(app, &label) {
                    let _ = window.hide();
                }
            }
            _ => {}
        });
}
