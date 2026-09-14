// Prevent a console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Apply the stored compatibility preferences (display backend, software
    // rendering) before GTK/WebKit initialise. An already-set environment
    // variable still takes precedence.
    clevo_cc_ui::prefs::apply_launch_env();
    clevo_cc_ui::run();
}
