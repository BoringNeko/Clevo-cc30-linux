//! Per-user XDG autostart registration, shared by the two desktop shells.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

const ENTRY_NAME: &str = "org.clevo.cc.ui.desktop";

#[derive(Debug, Deserialize)]
struct Launcher {
    executable: PathBuf,
    #[serde(default)]
    args: Vec<String>,
}

fn entry_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".config"))
        })
        .ok_or_else(|| "Cannot resolve the user configuration directory".to_string())?;
    Ok(base.join("autostart").join(ENTRY_NAME))
}

fn launcher() -> Result<Launcher, String> {
    // The Electron shell passes its own executable, never the headless backend.
    if let Some(raw) = std::env::var_os("CLEVO_CC_UI_LAUNCHER") {
        return serde_json::from_str(&raw.to_string_lossy()).map_err(|e| e.to_string());
    }
    #[cfg(not(feature = "tauri-shell"))]
    return Err("The Electron shell did not provide its launch command".into());

    #[cfg(feature = "tauri-shell")]
    Ok(Launcher {
        executable: std::env::var_os("APPIMAGE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_exe)
            .map_err(|e| e.to_string())?,
        args: Vec::new(),
    })
}

/// Desktop Exec quoting is not shell quoting; backslashes also undergo the
/// desktop-entry string unescape. Literal percent signs must escape field codes.
fn exec_arg(arg: &str) -> Result<String, String> {
    if arg.chars().any(|c| c.is_control()) {
        return Err("The launch command contains control characters".into());
    }
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        match c {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' | '$' | '`' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

fn desktop_entry(launcher: &Launcher) -> Result<String, String> {
    if !launcher.executable.is_absolute() || !launcher.executable.is_file() {
        return Err("The UI executable is missing or not an absolute file path".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if launcher
            .executable
            .metadata()
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err("The UI launch file is not executable".into());
        }
    }
    let executable = launcher
        .executable
        .to_str()
        .ok_or_else(|| "The UI executable path is not UTF-8".to_string())?;
    // GLib validates the executable before expanding %% field escapes. Keep a
    // percent-bearing path as an argument to env rather than the Exec program.
    let mut command = if executable.contains('%') {
        vec!["/usr/bin/env".into(), "--".into()]
    } else {
        Vec::new()
    };
    command.push(exec_arg(executable)?);
    for arg in &launcher.args {
        command.push(exec_arg(arg)?);
    }
    command.push("--tray".into());
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Clevo Control Center\nExec={}\nTerminal=false\nStartupNotify=false\nHidden=false\n",
        command.join(" ")
    ))
}

fn read_enabled(path: &Path) -> Result<bool, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut in_entry = false;
    let mut has_exec = false;
    let mut hidden = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "Hidden" => hidden = value == "true",
                    "Exec" => has_exec = !value.is_empty(),
                    _ => {}
                }
            }
        }
    }
    Ok(has_exec && !hidden)
}

fn write_entry(path: &Path, contents: &str) -> Result<(), String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or("Invalid autostart path")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let tmp = parent.join(format!(
        ".{ENTRY_NAME}.{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    let result = (|| {
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
}

pub fn get() -> Result<bool, String> {
    read_enabled(&entry_path()?)
}

pub fn set(enabled: bool) -> Result<bool, String> {
    let contents = if enabled {
        desktop_entry(&launcher()?)?
    } else {
        // Hidden=true also masks an entry of the same name in /etc/xdg.
        "[Desktop Entry]\nType=Application\nName=Clevo Control Center\nHidden=true\n".into()
    };
    write_entry(&entry_path()?, &contents)?;
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_quotes_reserved_characters_and_field_codes() {
        assert_eq!(exec_arg("/tmp/a b%f").unwrap(), "\"/tmp/a b%%f\"");
        assert_eq!(
            exec_arg("a\"$`\\b").unwrap(),
            "\"a\\\\\"\\\\$\\\\`\\\\\\\\b\""
        );
        assert!(exec_arg("a\nHidden=false").is_err());
    }

    #[test]
    fn writes_and_disables_a_tray_only_entry() {
        let _lock = crate::test_env_lock();
        let dir = std::env::temp_dir().join(format!("clevo-autostart-{}", std::process::id()));
        let path = dir.join("autostart").join(ENTRY_NAME);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!read_enabled(&path).unwrap());
        let executable = std::env::current_exe().unwrap();
        let entry = desktop_entry(&Launcher {
            executable,
            args: vec!["/source UI".into()],
        })
        .unwrap();
        assert!(entry.contains("\"/source UI\" --tray\n"));
        write_entry(&path, &entry).unwrap();
        assert!(read_enabled(&path).unwrap());
        write_entry(&path, "[Desktop Entry]\nHidden=true\n").unwrap();
        assert!(!read_enabled(&path).unwrap());
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_a_missing_or_relative_launcher() {
        for executable in [PathBuf::from("clevo-cc-ui"), PathBuf::from("/missing/UI")] {
            assert!(desktop_entry(&Launcher {
                executable,
                args: vec![]
            })
            .is_err());
        }
    }

    #[test]
    fn read_and_write_failures_are_reported() {
        let dir = std::env::temp_dir();
        assert!(read_enabled(&dir).is_err());
        let file = std::env::current_exe().unwrap();
        assert!(write_entry(&file.join("autostart.desktop"), "invalid").is_err());
    }
}
