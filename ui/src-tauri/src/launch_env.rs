//! Shared launch-environment setup.
//!
//! Both shells call [`apply`] as the very first thing in `main`, before any
//! toolkit initialises:
//!
//! * Tauri reads [`crate::prefs`] and translates the compatibility choices into
//!   GTK/WebKit environment variables (see [`crate::prefs::apply_launch_env`]).
//! * Electron cannot use the WebKit variables (it runs Chromium), so it gets a
//!   single hint telling it whether the user asked for software rendering; the
//!   Chromium switches live in `ui/electron/main.cjs`.
//!
//! Keeping this in one place means an explicit environment variable still wins
//! over the stored preference in both shells, which is what the tests pin.

use crate::prefs;

/// Environment variable the Electron shell reads to decide whether to disable
/// Chromium's GPU path. It mirrors `LaunchPrefs::software_rendering`.
pub const SOFTWARE_RENDERING_ENV: &str = "CLEVO_CC_SOFTWARE_RENDERING";

/// Apply the stored launch preferences to the process environment.
///
/// `shell` selects which variables are relevant; an explicit value already in
/// the environment is never overwritten.
pub fn apply(shell: Shell) {
    let prefs = prefs::load_launch_prefs();

    match shell {
        Shell::Tauri => prefs::apply_launch_env_with(&prefs),
        Shell::Electron => {
            if std::env::var_os(SOFTWARE_RENDERING_ENV).is_none() {
                std::env::set_var(
                    SOFTWARE_RENDERING_ENV,
                    if prefs.software_rendering { "1" } else { "0" },
                );
            }
        }
    }
}

/// Which shell is hosting the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// The in-process Tauri window/tray shell.
    Tauri,
    /// The headless HTTP backend the Electron main process spawns.
    Electron,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANAGED: [&str; 5] = [
        "GDK_BACKEND",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
        "WEBKIT_SKIA_ENABLE_CPU_RENDERING",
        SOFTWARE_RENDERING_ENV,
    ];

    struct EnvGuard {
        dir: std::path::PathBuf,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
        saved_xdg: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.saved {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
            match &self.saved_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn isolate(prefs_json: &str) -> EnvGuard {
        // Share the crate-wide lock with `prefs::tests`: both mutate the same
        // process environment, so they must not run concurrently.
        let lock = crate::test_env_lock();
        let dir = std::env::temp_dir().join(format!(
            "clevo-launch-env-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(dir.join("clevo-cc")).unwrap();
        std::fs::write(dir.join("clevo-cc/ui-launch.json"), prefs_json).unwrap();

        let saved = MANAGED.iter().map(|k| (*k, std::env::var_os(k))).collect();
        let saved_xdg = std::env::var_os("XDG_CONFIG_HOME");
        for key in MANAGED {
            std::env::remove_var(key);
        }
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        EnvGuard {
            dir,
            saved,
            saved_xdg,
            _lock: lock,
        }
    }

    #[test]
    fn electron_uses_its_own_hint_and_leaves_webkit_alone() {
        let _guard = isolate(r#"{"backend":"x11","software_rendering":true}"#);

        apply(Shell::Electron);

        assert_eq!(std::env::var(SOFTWARE_RENDERING_ENV).as_deref(), Ok("1"));
        assert!(std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none());
        assert!(std::env::var_os("GDK_BACKEND").is_none());
    }

    #[test]
    fn electron_defaults_to_hardware_rendering() {
        let _guard = isolate(r#"{"backend":"auto","software_rendering":false}"#);
        apply(Shell::Electron);
        assert_eq!(std::env::var(SOFTWARE_RENDERING_ENV).as_deref(), Ok("0"));
    }

    #[test]
    fn an_explicit_hint_wins_in_electron() {
        let _guard = isolate(r#"{"backend":"auto","software_rendering":false}"#);
        std::env::set_var(SOFTWARE_RENDERING_ENV, "1");
        apply(Shell::Electron);
        assert_eq!(std::env::var(SOFTWARE_RENDERING_ENV).as_deref(), Ok("1"));
    }

    #[test]
    fn tauri_still_sets_the_webkit_variables() {
        let _guard = isolate(r#"{"backend":"x11","software_rendering":false}"#);
        apply(Shell::Tauri);
        assert_eq!(std::env::var("GDK_BACKEND").as_deref(), Ok("x11"));
        assert_eq!(
            std::env::var("WEBKIT_DMABUF_RENDERER_FORCE_SHM").as_deref(),
            Ok("1")
        );
        assert!(std::env::var_os(SOFTWARE_RENDERING_ENV).is_none());
    }
}
