//! Every `#[tauri::command]` must be registered with the invoke handler.
//!
//! A command that exists but is missing from `generate_handler!` compiles
//! cleanly and is silently unreachable: the frontend's `invoke("name")` fails
//! at runtime with "Command name not found", which looks like a frontend bug.
//! That is exactly how `set_fan_curve` shipped - defined in commands.rs,
//! documented in the UI, and never listed.
//!
//! Checked by reading the sources, because the failure mode is an omission in a
//! macro invocation, which nothing else can observe.

const COMMANDS: &str = include_str!("../src/commands.rs");
const LIB: &str = include_str!("../src/lib.rs");

/// Command names declared with `#[tauri::command]` in commands.rs.
fn declared() -> Vec<String> {
    COMMANDS
        .lines()
        .collect::<Vec<_>>()
        .windows(3)
        .filter(|w| w[0].trim() == "#[tauri::command]")
        .filter_map(|w| {
            // The signature follows, e.g. `pub fn set_fan_curve(`, possibly
            // after an attribute on the middle line.
            w.iter().find_map(|line| {
                let rest = line.trim().strip_prefix("pub fn ")?;
                Some(rest.split('(').next()?.to_string())
            })
        })
        .collect()
}

/// Command names passed to `generate_handler!` in lib.rs.
fn registered() -> Vec<String> {
    let start = LIB
        .find("generate_handler!")
        .expect("lib.rs registers an invoke handler");
    let body = &LIB[start..];
    let end = body.find("])").expect("the handler list is closed");
    body[..end]
        .split("commands::")
        .skip(1)
        .filter_map(|chunk| {
            let name: String = chunk
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

#[test]
fn every_command_is_registered() {
    let declared = declared();
    let registered = registered();

    assert!(
        !declared.is_empty(),
        "no commands were found; the parser is broken, not the code"
    );

    let missing: Vec<_> = declared
        .iter()
        .filter(|name| !registered.contains(name))
        .collect();

    assert!(
        missing.is_empty(),
        "these commands are declared but never registered, so invoke() cannot \
         reach them: {missing:?}"
    );
}

#[test]
fn every_registered_command_exists() {
    let declared = declared();
    let registered = registered();

    let unknown: Vec<_> = registered
        .iter()
        .filter(|name| !declared.contains(name))
        .collect();

    assert!(
        unknown.is_empty(),
        "these registered commands have no `#[tauri::command]` definition: {unknown:?}"
    );
}
