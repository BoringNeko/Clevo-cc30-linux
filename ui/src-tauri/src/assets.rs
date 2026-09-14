//! Appearance assets that must live on disk: the custom wallpaper and logo.
//!
//! Wallpapers are copied into the app's data directory so the choice survives a
//! restart (a blob URL would not). Images are returned to the frontend as data
//! URLs, which avoids granting the webview broad filesystem access.

use std::path::PathBuf;

use serde::Serialize;

/// Directory for app data: `$XDG_DATA_HOME/clevo-cc` or `~/.local/share/clevo-cc`.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("clevo-cc")
}

fn wallpaper_path() -> PathBuf {
    data_dir().join("wallpaper")
}

fn logo_path() -> PathBuf {
    data_dir().join("logo")
}

/// MIME type for a file extension.
fn mime_for(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// Read `path` and return a data URL, or `None` if it cannot be read.
pub fn read_as_data_url(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mime = mime_for(ext);
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

/// Read an arbitrary image path as a data URL (used for the default logo).
pub fn read_path_as_data_url(path: &str) -> Option<String> {
    read_as_data_url(std::path::Path::new(path))
}

/// Serialisable result for the frontend.
#[derive(Debug, Serialize)]
pub struct SavedImage {
    /// Data URL of the saved image.
    pub data_url: String,
    /// MIME type.
    pub mime: String,
}

fn save_image(kind: &str, base: &PathBuf, bytes: &[u8], ext: &str) -> Result<SavedImage, String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Remove any previous file of this kind (extension may differ).
    for old in ["jpg", "jpeg", "png", "webp", "gif", "svg", "bmp"] {
        let _ = std::fs::remove_file(dir.join(format!("{kind}.{old}")));
    }
    let ext = if ext.is_empty() { "png" } else { ext };
    let path = dir.join(format!("{kind}.{ext}"));
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mime = mime_for(ext).to_string();
    let data_url = read_as_data_url(&path).ok_or("saved but could not be read back")?;
    let _ = base;
    Ok(SavedImage { data_url, mime })
}

/// Remove any saved image of `kind` and return whether one existed.
fn clear_image(kind: &str) -> bool {
    let dir = data_dir();
    let mut removed = false;
    for ext in ["jpg", "jpeg", "png", "webp", "gif", "svg", "bmp"] {
        if std::fs::remove_file(dir.join(format!("{kind}.{ext}"))).is_ok() {
            removed = true;
        }
    }
    removed
}

/// Load a previously saved image of `kind` as a data URL.
fn load_image(kind: &str) -> Option<String> {
    let dir = data_dir();
    for ext in ["jpg", "jpeg", "png", "webp", "gif", "svg", "bmp"] {
        let path = dir.join(format!("{kind}.{ext}"));
        if path.exists() {
            return read_as_data_url(&path);
        }
    }
    None
}

/// Persist the wallpaper bytes; extension is taken from the caller.
pub fn save_wallpaper(bytes: &[u8], ext: &str) -> Result<SavedImage, String> {
    save_image("wallpaper", &wallpaper_path(), bytes, ext)
}

/// Load the saved wallpaper as a data URL.
pub fn load_wallpaper() -> Option<String> {
    load_image("wallpaper")
}

/// Delete the saved wallpaper.
pub fn clear_wallpaper() -> bool {
    clear_image("wallpaper")
}

/// Persist the custom logo bytes.
pub fn save_logo(bytes: &[u8], ext: &str) -> Result<SavedImage, String> {
    save_image("logo", &logo_path(), bytes, ext)
}

/// Load the saved custom logo as a data URL.
pub fn load_logo() -> Option<String> {
    load_image("logo")
}

/// Delete the saved custom logo.
pub fn clear_logo() -> bool {
    clear_image("logo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_types() {
        assert_eq!(mime_for("jpg"), "image/jpeg");
        assert_eq!(mime_for("PNG"), "image/png");
        assert_eq!(mime_for("webp"), "image/webp");
        assert_eq!(mime_for("xyz"), "application/octet-stream");
    }

    #[test]
    fn data_url_round_trip() {
        let dir = std::env::temp_dir().join(format!("clevo-assets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.png");
        std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47]).unwrap();
        let url = read_as_data_url(&path).expect("data url");
        assert!(url.starts_with("data:image/png;base64,"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_is_none() {
        assert!(read_as_data_url(std::path::Path::new("/nonexistent/nope.png")).is_none());
    }
}
