//! System-tray icon.
//!
//! On Linux the tray is an AppIndicator, which only supports a **native menu**
//! and never delivers click events to the app (unlike Windows/macOS). The menu
//! therefore carries everything: a "性能模式" submenu plus open/quit. The
//! performance submenu talks to the daemon directly, so switching works without
//! opening any window.
//!
//! `muda`'s GTK `CheckMenuItem` shares its checked state across every check item
//! in the same menu, so it cannot model a radio group. Plain items are used
//! instead, and the currently applied mode is shown in the submenu label
//! ("性能模式：性能").

use tauri::{
    image::Image,
    menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

use crate::dbus::DaemonClient;

pub const MAIN_WINDOW: &str = "main";

/// Performance modes, in display order: `(label, daemon name, value)`.
const PERF_MODES: [(&str, &str, u8); 4] = [
    ("静音", "quiet", 0),
    ("节能", "pwrsaving", 1),
    ("性能", "performance", 2),
    ("娱乐", "entertainment", 3),
];

/// The menu item id for a performance mode value, e.g. `perf:2`.
fn perf_id(value: u8) -> String {
    format!("perf:{value}")
}

/// The submenu label, including the active mode when known.
fn perf_submenu_label(active: Option<u8>) -> String {
    match active.and_then(|v| PERF_MODES.iter().find(|(_, _, m)| *m == v)) {
        Some((label, _, _)) => format!("性能模式：{label}"),
        None => "性能模式".to_string(),
    }
}

/// Show and focus the main control-center window.
pub fn show_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Read the daemon's current performance mode (255 means "unset").
fn current_perf_mode() -> Option<u8> {
    match DaemonClient::system().and_then(|c| c.snapshot()) {
        Ok(s) if s.perf_mode < PERF_MODES.len() as u8 => Some(s.perf_mode),
        _ => None,
    }
}

/// Apply a performance mode through the daemon (PolicyKit-gated) and refresh the
/// submenu label on success.
fn apply_perf_mode(app: &AppHandle, value: u8) {
    let Some((_, name, _)) = PERF_MODES.iter().find(|(_, _, v)| *v == value) else {
        return;
    };

    match DaemonClient::system().and_then(|c| c.set_perf_mode(name)) {
        Ok(_) => {
            if let Some(submenu) = app.try_state::<PerfSubmenu>() {
                let _ = submenu.0.set_text(perf_submenu_label(Some(value)));
            }
        }
        Err(e) => eprintln!("tray: could not set performance mode: {}", e.message),
    }
}

/// The performance submenu, kept so its label can be updated after a switch.
struct PerfSubmenu(Submenu<Wry>);

/// Create the tray icon with its menu. Called once from the app's `setup` hook.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let active = current_perf_mode();

    let perf_items: Vec<MenuItem<Wry>> = PERF_MODES
        .iter()
        .map(|(label, _, value)| {
            MenuItem::with_id(app, perf_id(*value), *label, true, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;

    let perf_refs: Vec<&dyn IsMenuItem<Wry>> = perf_items
        .iter()
        .map(|i| i as &dyn IsMenuItem<Wry>)
        .collect();
    let perf = Submenu::with_id_and_items(
        app,
        "perf-submenu",
        perf_submenu_label(active),
        true,
        &perf_refs,
    )?;

    let show = MenuItem::with_id(app, "show", "打开控制中心", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&perf, &sep, &show, &quit])?;

    app.manage(PerfSubmenu(perf));

    TrayIconBuilder::with_id("main-tray")
        .icon(Image::from_bytes(include_bytes!("../icons/32x32.png"))?)
        .tooltip("Clevo Control Center")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            if let Some(value) = id.strip_prefix("perf:") {
                if let Ok(value) = value.parse::<u8>() {
                    apply_perf_mode(app, value);
                }
                return;
            }
            match id {
                "show" => {
                    let _ = show_main_window(app);
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}
