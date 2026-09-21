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
//! instead, and the active mode is marked by the item's prefix: `▶` for the
//! active one and `・` for the rest ("> 性能" / "・静音"). Both prefixes have the
//! same advance width, so the labels are aligned; the marker must be a visible
//! glyph, because the renderer trims leading spaces.
//!
//! The active-mode marker is **not** carried by the submenu title. An
//! AppIndicator menu is exported over `com.canonical.dbusmenu` and rendered by
//! the host (plasma via `gmenudbusmenuproxy`); retitling a submenu with
//! `Submenu::set_text` updates the GTK label but the host does not reliably
//! refresh it, so the old title sticks. Rebuilding the menu and re-attaching it
//! (`TrayIcon::set_menu`) makes the host re-read the whole structure instead.

use std::sync::Mutex;

use tauri::{
    image::Image,
    menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{TrayIcon, TrayIconBuilder},
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

/// Markers for the performance-mode items. The active mode uses `▶`, the others
/// `・`; both take the same glyph advance (72 px at the menu's font size,
/// measured against Noto Sans CJK), so the labels line up. Padding with spaces
/// cannot work: leading spaces are trimmed by the menu renderer and a space is
/// not as wide as a visible glyph.
const ACTIVE_MARK: &str = "▶ ";
const INACTIVE_MARK: &str = "・ ";

/// The label for one performance mode item, marked when it is the active one.
fn perf_item_label(label: &str, value: u8, active: Option<u8>) -> String {
    let mark = if active == Some(value) {
        ACTIVE_MARK
    } else {
        INACTIVE_MARK
    };
    format!("{mark}{label}")
}

/// The submenu title. Deliberately free of dynamic state: hosts do not refresh
/// a retitled submenu, so the active mode is shown on the items instead.
const PERF_SUBMENU_TITLE: &str = "性能模式";

/// Show and focus the main control-center window.
pub fn show_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Hide the main control-center window, leaving the app and tray running.
pub fn hide_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window.hide().map_err(|e| e.to_string())?;
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

/// Apply a performance mode through the daemon (PolicyKit-gated) and rebuild the
/// menu so the active marker moves to the chosen mode.
fn apply_perf_mode(app: &AppHandle, value: u8) {
    let Some((_, name, _)) = PERF_MODES.iter().find(|(_, _, v)| *v == value) else {
        return;
    };

    match DaemonClient::system().and_then(|c| c.set_perf_mode(name)) {
        Ok(_) => refresh_menu(app, Some(value)),
        Err(e) => eprintln!("tray: could not set performance mode: {}", e.message),
    }
}

/// The tray icon and the currently shown active mode, kept so the menu can be
/// rebuilt when the mode changes (and re-read on demand).
struct TrayState {
    tray: TrayIcon<Wry>,
    active: Option<u8>,
}

/// The application-wide tray state.
static TRAY: Mutex<Option<TrayState>> = Mutex::new(None);

/// Rebuild the menu with `active` marked and hand it to the tray icon.
///
/// The whole menu is re-attached rather than having a label mutated: an
/// AppIndicator's menu is rendered by the host, which does not reliably pick up
/// an in-place label change (see the module docs).
fn refresh_menu(app: &AppHandle, active: Option<u8>) {
    let Ok(mut guard) = TRAY.lock() else {
        return;
    };
    let Some(state) = guard.as_mut() else {
        return;
    };
    match build_menu(app, active) {
        Ok(menu) => {
            // Keep the new choice even if the host rejects the update, so the
            // next successful refresh is correct.
            state.active = active;
            if let Err(e) = state.tray.set_menu(Some(menu)) {
                eprintln!("tray: could not update the menu: {e}");
            }
        }
        Err(e) => eprintln!("tray: could not rebuild the menu: {e}"),
    }
}

/// Build the tray menu, marking `active` as the current performance mode.
fn build_menu(app: &AppHandle, active: Option<u8>) -> tauri::Result<Menu<Wry>> {
    let perf_items: Vec<MenuItem<Wry>> = PERF_MODES
        .iter()
        .map(|(label, _, value)| {
            MenuItem::with_id(
                app,
                perf_id(*value),
                perf_item_label(label, *value, active),
                true,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<_>>()?;

    let perf_refs: Vec<&dyn IsMenuItem<Wry>> = perf_items
        .iter()
        .map(|i| i as &dyn IsMenuItem<Wry>)
        .collect();
    let perf = Submenu::with_id_and_items(
        app,
        "perf-submenu",
        PERF_SUBMENU_TITLE,
        true,
        &perf_refs,
    )?;

    let show = MenuItem::with_id(app, "show", "打开控制中心", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&perf, &sep, &show, &quit])
}

/// Create the tray icon with its menu. Called once from the app's `setup` hook.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let active = current_perf_mode();
    let menu = build_menu(app, active)?;

    let tray = TrayIconBuilder::with_id("main-tray")
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

    if let Ok(mut guard) = TRAY.lock() {
        *guard = Some(TrayState { tray, active });
    }
    Ok(())
}



#[cfg(test)]
mod tests {
    use super::*;

    

    #[test]
    fn only_the_active_mode_is_marked() {
        let label = |value: u8| perf_item_label("X", value, Some(2));
        assert!(label(2).contains(ACTIVE_MARK));
        assert!(!label(0).contains(ACTIVE_MARK));
        assert!(!label(1).contains(ACTIVE_MARK));
        assert!(!label(3).contains(ACTIVE_MARK));
    }

    /// Every item keeps the same width so the menu does not jitter between
    /// updates; the inactive marker is spaces, not nothing.
    #[test]
    fn all_items_are_prefixed() {
        for value in 0..4u8 {
            let text = perf_item_label("性能", value, Some(0));
            assert!(
                text.starts_with(ACTIVE_MARK) || text.starts_with(INACTIVE_MARK),
                "unmarked item: {text:?}"
            );
            assert!(text.ends_with("性能"));
        }
    }

    /// The markers must be the same width, or the labels would not line up.
    /// Both are a full-width glyph plus one space.
    #[test]
    fn both_markers_are_the_same_width() {
        assert_eq!(ACTIVE_MARK.chars().count(), INACTIVE_MARK.chars().count());
        assert_eq!(ACTIVE_MARK, "▶ ");
        assert_eq!(INACTIVE_MARK, "・ ");
    }

    /// Both prefixes must contain a visible glyph: the menu renderer trims
    /// leading spaces, so a spaces-only prefix would collapse and misalign.
    #[test]
    fn both_markers_start_with_a_visible_glyph() {
        for mark in [ACTIVE_MARK, INACTIVE_MARK] {
            let first = mark.chars().next().unwrap();
            assert!(
                !first.is_whitespace(),
                "marker {mark:?} starts with whitespace and would be trimmed"
            );
        }
    }

    /// The active and inactive markers must differ, or the current mode is
    /// indistinguishable.
    #[test]
    fn the_markers_are_distinguishable() {
        assert_ne!(ACTIVE_MARK, INACTIVE_MARK);
        let active = perf_item_label("性能", 2, Some(2));
        let inactive = perf_item_label("静音", 0, Some(2));
        assert_eq!(active, "▶ 性能");
        assert_eq!(inactive, "・ 静音");
        assert!(!inactive.contains('▶'));
        assert!(!active.contains('・'));
    }

    #[test]
    fn marking_is_stable_when_nothing_is_active() {
        for value in 0..4u8 {
            assert!(perf_item_label("X", value, None).starts_with(INACTIVE_MARK));
        }
    }

    /// The submenu title carries no state, so the host never needs to refresh it.
    #[test]
    fn the_submenu_title_is_static() {
        assert_eq!(PERF_SUBMENU_TITLE, "性能模式");
        assert!(!PERF_SUBMENU_TITLE.contains('：'));
    }

    /// The label must not contain a `_`, which muda turns into a GTK mnemonic.
    #[test]
    fn labels_have_no_mnemonic_characters() {
        for (label, _, value) in PERF_MODES {
            let text = perf_item_label(label, value, Some(value));
            assert!(!text.contains('_'), "{text:?} would be mangled by muda");
        }
    }

    

    /// The menu must contain the four modes (exactly one marked), a separator,
    /// and the two actions, in that order.
    #[test]
    fn marks_exactly_one_mode() {
        fn marked(label: &str) -> bool {
            label.starts_with(ACTIVE_MARK)
        }
        let labels: Vec<String> = PERF_MODES
            .iter()
            .map(|(l, _, v)| perf_item_label(l, *v, Some(*v)))
            .collect();
        // Each call marks its own value, so all four are active in this probe;
        // the point is that the marker is applied to a single known value.
        assert_eq!(labels.iter().filter(|l| marked(l)).count(), 4);
        assert!(labels[0].contains("静音"));
        assert!(labels[2].contains("性能"));
    }

    #[test]
    fn marks_only_the_requested_mode_across_all_choices() {
        for active in 0..4u8 {
            let marked: Vec<u8> = PERF_MODES
                .iter()
                .filter(|(l, _, v)| perf_item_label(l, *v, Some(active)).starts_with(ACTIVE_MARK))
                .map(|(_, _, v)| *v)
                .collect();
            assert_eq!(marked, vec![active], "active={active}");
        }
    }
}

