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

/// Commands granted to the notes window and to no other window.
const NOTES_WINDOW_ONLY: &[&str] = &["notes_to_main", "set_notes_on_top"];

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
        // Commands only the notes window calls are checked in notes_window_capability_is_narrow.
        if NOTES_WINDOW_ONLY.contains(&command.as_str()) {
            assert!(
                !permissions.contains(&permission.as_str()),
                "{permission} belongs to the notes window only"
            );
        } else {
            assert!(
                permissions.contains(&permission.as_str()),
                "{permission} missing from capabilities/default.json"
            );
        }
        assert!(
            lib.contains(&format!("commands::{command},")),
            "{command} missing from generate_handler (lib.rs)"
        );
    }
    // Local claims: the main window turns them on and reads them; the notes window never does
    // (notes_window_capability_is_narrow allows note commands only).
    for command in ["configure_claims", "claims_snapshot"] {
        assert!(commands.iter().any(|c| c == command), "{command} missing");
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
    // Listening only: emitting would let the notes window fake events of the main window, such
    // as `tray-quit`. Messages go through `notes_to_main`.
    let allowed_core = [
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        // Close asks the window's own handler first; destroy ends it once input is saved.
        "core:window:allow-close",
        "core:window:allow-destroy",
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
            (command.contains("note") || command == "list_trash") && command != "open_notes_window",
            "{permission} is not a note command"
        );
    }
    for needed in [
        "allow-search-notes",
        "allow-list-trash",
        "allow-restore-note",
        "allow-export-note",
        "allow-reveal-note",
        "allow-notes-to-main",
        "allow-set-notes-on-top",
        "allow-read-note",
        "allow-write-note",
    ] {
        assert!(permissions.contains(&needed), "{needed} missing");
    }
    // On top only for itself, through its own command, never for any window it can name.
    for forbidden in [
        "core:window:allow-set-always-on-top",
        "core:event:default",
        "core:event:allow-emit",
        "core:event:allow-emit-to",
    ] {
        assert!(
            !permissions.contains(&forbidden),
            "{forbidden} in notes.json"
        );
    }
    // The main window opens the notes window only through the backend command, never by
    // creating webviews itself.
    let main: serde_json::Value = serde_json::from_str(&read("capabilities/default.json")).unwrap();
    let main: Vec<&str> = main["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    assert!(main.contains(&"allow-open-notes-window"));
    for forbidden in [
        "core:webview:allow-create-webview-window",
        "core:webview:allow-create-webview",
        "core:window:allow-create",
    ] {
        assert!(!main.contains(&forbidden), "{forbidden} in default.json");
    }
}
