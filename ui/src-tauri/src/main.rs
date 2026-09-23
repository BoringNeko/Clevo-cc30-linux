// Prevent a console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--serve` turns this into the headless JSON bridge the Electron shell
    // spawns (see `src/serve.rs`). It must be checked before anything
    // toolkit-related runs.
    #[cfg(not(feature = "tauri-shell"))]
    {
        if std::env::args().any(|a| a == "--serve") {
            clevo_cc_ui::launch_env::apply(clevo_cc_ui::launch_env::Shell::Electron);
            if let Err(e) = clevo_cc_ui::serve::run() {
                eprintln!("clevo-cc-ui backend: {e}");
                std::process::exit(1);
            }
            return;
        }
        eprintln!("clevo-cc-ui was built without the Tauri shell; pass --serve");
        std::process::exit(2);
    }

    #[cfg(feature = "tauri-shell")]
    {
        // A stray `--serve` on the Tauri build would otherwise be ignored.
        if std::env::args().any(|a| a == "--serve") {
            eprintln!("clevo-cc-ui: --serve needs a build without the Tauri shell");
            std::process::exit(2);
        }
        // Apply the stored compatibility preferences (display backend, software
        // rendering) before the toolkit initialises. An already-set environment
        // variable still takes precedence.
        clevo_cc_ui::launch_env::apply(clevo_cc_ui::launch_env::Shell::Tauri);
        clevo_cc_ui::run();
    }
}
