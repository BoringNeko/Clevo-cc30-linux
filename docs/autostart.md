# UI Login Autostart

In Settings > Application, enable the login autostart switch. It starts the UI
in the system tray at the next desktop login, without creating the main window.
Open the control center from the tray menu when needed. Both Tauri and Electron
support `--tray`; normal manual launches still open a window.

The switch manages the current user's
`$XDG_CONFIG_HOME/autostart/org.clevo.cc.ui.desktop`, falling back to
`~/.config/autostart/org.clevo.cc.ui.desktop`. No root permission is needed.
Disabling writes `Hidden=true` and does not quit an already running UI.

The entry records the current shell's executable. Electron AppImages record the
AppImage path rather than the temporary mount or headless backend. After moving
the executable or switching shells, disable and re-enable the switch in the
desired UI to update the entry. Source-checkout registrations also depend on
the checkout and built frontend remaining available.

Save failures are shown in the settings pane and leave the previous switch
value intact. There is no browser-only/localStorage substitute for registration.

## Tests

```bash
node scripts/tests-electron-autostart.mjs
cargo test --manifest-path ui/src-tauri/Cargo.toml --no-default-features --lib autostart
cargo build --manifest-path ui/src-tauri/Cargo.toml --no-default-features
node scripts/tests-ui-autostart.mjs
```

The bridge test uses a temporary configuration directory. When available,
`desktop-file-validate` and `gio launch` also verify the generated entry and
exact launch arguments without launching the real UI or writing to hardware.
