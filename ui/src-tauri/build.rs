// The Tauri build script only runs for the Tauri shell. In the Electron build
// (`--no-default-features`) the crate is a headless backend and Tauri is not a
// dependency, so there is nothing to generate.
fn main() {
    #[cfg(feature = "tauri-shell")]
    tauri_build::build();
}
