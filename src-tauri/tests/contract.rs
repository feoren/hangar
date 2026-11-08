//! Every Tauri command needs three entries: in build.rs (generates the permission), in
//! capabilities/default.json (grants it) and in lib.rs (generate_handler). If one is missing, the
//! call only fails at runtime in the window; this test catches that beforehand.

use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn build_commands() -> Vec<String> {
    let build = read("build.rs");
    let start = build
        .find("const COMMANDS")
        .expect("COMMANDS missing from build.rs");
    let list = &build[start..build[start..].find("];").unwrap() + start];
    list.lines()
        .filter_map(|line| line.trim().strip_prefix('"'))
        .filter_map(|rest| rest.strip_suffix("\","))
        .map(str::to_owned)
        .collect()
}

#[test]
fn commands_are_registered_allowed_and_wired() {
    let commands = build_commands();
    assert!(
        commands.len() >= 20,
        "COMMANDS not recognized: {commands:?}"
    );
    let capabilities: serde_json::Value =
        serde_json::from_str(&read("capabilities/default.json")).unwrap();
    let permissions: Vec<&str> = capabilities["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    let lib = read("src/lib.rs");
    for command in &commands {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(
            permissions.contains(&permission.as_str()),
            "{permission} missing from capabilities/default.json"
        );
        assert!(
            lib.contains(&format!("commands::{command},")),
            "{command} missing from generate_handler (lib.rs)"
        );
    }
    for command in [
        "save_workspace",
        "load_workspace",
        "save_scrollback",
        "load_scrollback",
        "delete_scrollback",
        "clear_scrollbacks",
    ] {
        assert!(commands.iter().any(|c| c == command), "{command} missing");
    }
}

/// The detached notes window gets the note commands and a few window permissions, nothing else:
/// no sessions, no profiles, no updates, no file dialogs of its own.
#[test]
fn notes_window_capability_is_narrow() {
    let commands = build_commands();
    let notes: serde_json::Value = serde_json::from_str(&read("capabilities/notes.json")).unwrap();
    assert_eq!(notes["windows"], serde_json::json!(["notes"]));
    let permissions: Vec<&str> = notes["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    let allowed_core = [
        "core:event:default",
        "core:window:allow-set-always-on-top",
        "core:window:allow-close",
        "core:window:allow-set-focus",
        "clipboard-manager:allow-write-text",
    ];
    for permission in &permissions {
        if allowed_core.contains(permission) {
            continue;
        }
        let command = permission
            .strip_prefix("allow-")
            .unwrap_or_else(|| panic!("unexpected permission {permission}"))
            .replace('-', "_");
        assert!(
            commands.contains(&command),
            "{permission} names no registered command"
        );
        assert!(
            command.contains("note") || command == "list_trash",
            "{permission} is not a note command"
        );
    }
    for needed in [
        "allow-search-notes",
        "allow-list-trash",
        "allow-restore-note",
        "allow-export-note",
        "allow-reveal-note",
        "allow-read-note",
        "allow-write-note",
    ] {
        assert!(permissions.contains(&needed), "{needed} missing");
    }
    // The main window opens the notes window.
    let main: serde_json::Value = serde_json::from_str(&read("capabilities/default.json")).unwrap();
    assert!(main["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p == "core:webview:allow-create-webview-window"));
}
