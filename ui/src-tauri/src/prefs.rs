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

    if prefs.software_rendering && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
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

    #[test]
    fn apply_launch_env_sets_the_backend() {
        // Isolate from the real user config and the ambient environment.
        let dir = std::env::temp_dir().join(format!("clevo-prefs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("clevo-cc")).unwrap();
        std::fs::write(
            dir.join("clevo-cc/ui-launch.json"),
            r#"{"backend":"x11","software_rendering":true}"#,
        )
        .unwrap();

        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let prev_backend = std::env::var_os("GDK_BACKEND");
        let prev_dmabuf = std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER");
        std::env::remove_var("GDK_BACKEND");
        std::env::remove_var("WEBKIT_DISABLE_DMABUF_RENDERER");
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        apply_launch_env();

        assert_eq!(std::env::var("GDK_BACKEND").as_deref(), Ok("x11"));
        assert_eq!(
            std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").as_deref(),
            Ok("1")
        );

        // Restore the environment for the other tests in this process.
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match prev_backend {
            Some(v) => std::env::set_var("GDK_BACKEND", v),
            None => std::env::remove_var("GDK_BACKEND"),
        }
        match prev_dmabuf {
            Some(v) => std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", v),
            None => std::env::remove_var("WEBKIT_DISABLE_DMABUF_RENDERER"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
