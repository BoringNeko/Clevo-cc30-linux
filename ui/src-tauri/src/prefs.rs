//! Compatibility preferences that must be applied before the WebView starts.
//!
//! Display backend and software rendering are process-wide environment
//! variables, so they cannot be changed while the app runs. The UI stores them
//! here and `scripts/run-ui.sh` reads the same file to launch the app with the
//! right environment.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Persisted launch preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchPrefs {
    /// Display backend: `auto`, `wayland` or `x11`.
    pub backend: String,
    /// Whether to disable the DMA-BUF renderer (software rendering).
    pub software_rendering: bool,
}

impl Default for LaunchPrefs {
    fn default() -> Self {
        Self {
            backend: "auto".into(),
            software_rendering: false,
        }
    }
}

/// Path of the launch preferences file: `$XDG_CONFIG_HOME/clevo-cc/ui-launch.json`
/// (falling back to `~/.config/clevo-cc/ui-launch.json`).
pub fn launch_prefs_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("clevo-cc").join("ui-launch.json")
}

/// Load the launch preferences, returning defaults when absent or malformed.
pub fn load_launch_prefs() -> LaunchPrefs {
    let path = launch_prefs_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => LaunchPrefs::default(),
    }
}

/// Persist the launch preferences, creating the directory as needed.
pub fn save_launch_prefs(prefs: &LaunchPrefs) -> Result<(), String> {
    let path = launch_prefs_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Apply the launch preferences as environment variables.
///
/// Display backend and software rendering are read by GTK/WebKit when the
/// toolkit initialises, so this must run at the very start of `main`, before
/// Tauri (and thus GTK) spins up. Variables already set in the environment win,
/// so an explicit `GDK_BACKEND=... clevo-cc-ui` still overrides the stored
/// preference.
///
/// Regardless of the stored preference, the DMA-BUF *transport* is forced onto
/// shared memory (`WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`). On the NVIDIA
/// proprietary driver WebKitGTK 2.52 fails to allocate a GBM buffer and dies
/// with `Gdk Error 71` before the window is mapped, which reproduces on every
/// compositor (Wayland and X11, KDE/GNOME/wlroots alike). Forcing the shared
/// memory transport keeps the GL compositor itself enabled, so hardware
/// acceleration and the frosted `backdrop-filter` still work — unlike the
/// `WEBKIT_DISABLE_DMABUF_RENDERER=1` sledgehammer, which tears down the whole
/// accelerated compositor. The user-facing "software rendering" switch keeps
/// the sledgehammer as a last resort for other broken drivers.
pub fn apply_launch_env() {
    let prefs = load_launch_prefs();

    match prefs.backend.as_str() {
        "wayland" if std::env::var_os("GDK_BACKEND").is_none() => {
            std::env::set_var("GDK_BACKEND", "wayland");
        }
        "x11" if std::env::var_os("GDK_BACKEND").is_none() => {
            std::env::set_var("GDK_BACKEND", "x11");
        }
        _ => {}
    }

    if prefs.software_rendering {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        return;
    }

    if std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_none() {
        std::env::set_var("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_auto_and_hardware() {
        let prefs = LaunchPrefs::default();
        assert_eq!(prefs.backend, "auto");
        assert!(!prefs.software_rendering);
    }

    #[test]
    fn round_trips_through_json() {
        let prefs = LaunchPrefs {
            backend: "x11".into(),
            software_rendering: true,
        };
        let text = serde_json::to_string(&prefs).unwrap();
        let back: LaunchPrefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back, prefs);
    }

    /// The env vars this module writes, isolated and restored per test.
    const MANAGED: [&str; 3] = [
        "GDK_BACKEND",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
    ];

    /// The process environment is global, so the tests that mutate it must not
    /// run concurrently. This lock serialises them.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Point `XDG_CONFIG_HOME` at a fresh dir holding `prefs_json`, and clear
    /// the managed env vars so `apply_launch_env` starts from a known state.
    /// Returns a guard that restores everything on drop.
    struct EnvGuard {
        dir: PathBuf,
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
        // Poisoning is irrelevant here: a previous assertion failure already
        // fails its own test, and we only need mutual exclusion.
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let dir = std::env::temp_dir().join(format!(
            "clevo-prefs-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(dir.join("clevo-cc")).unwrap();
        std::fs::write(dir.join("clevo-cc/ui-launch.json"), prefs_json).unwrap();

        let saved = MANAGED
            .iter()
            .map(|k| (*k, std::env::var_os(k)))
            .collect();
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
    fn hardware_path_forces_the_shared_memory_transport() {
        let _guard = isolate(r#"{"backend":"x11","software_rendering":false}"#);

        apply_launch_env();

        assert_eq!(std::env::var("GDK_BACKEND").as_deref(), Ok("x11"));
        // The GBM allocation workaround, not the compositor-disabling hammer.
        assert_eq!(
            std::env::var("WEBKIT_DMABUF_RENDERER_FORCE_SHM").as_deref(),
            Ok("1")
        );
        assert!(std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none());
    }

    #[test]
    fn software_rendering_disables_the_accelerated_compositor() {
        let _guard = isolate(r#"{"backend":"auto","software_rendering":true}"#);

        apply_launch_env();

        assert_eq!(
            std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").as_deref(),
            Ok("1")
        );
        // The sledgehammer already covers it; do not also force SHM.
        assert!(std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_none());
    }

    #[test]
    fn an_explicit_environment_value_wins() {
        let _guard = isolate(r#"{"backend":"auto","software_rendering":false}"#);
        std::env::set_var("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "0");

        apply_launch_env();

        assert_eq!(
            std::env::var("WEBKIT_DMABUF_RENDERER_FORCE_SHM").as_deref(),
            Ok("0")
        );
    }
}
