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

/// Commands granted to the side panel window and to no other window.
const PANEL_WINDOW_ONLY: &[&str] = &["panel_to_main", "set_panel_on_top"];
/// Commands granted to the file viewer windows and to no other window.
const FILE_VIEWER_ONLY: &[&str] = &[
    "file_viewer_content",
    "file_viewer_stamp",
    "file_viewer_diff",
    "file_viewer_open_link",
];

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
        // Commands only the panel window or a file viewer calls are checked in
        // panel_window_capability_is_narrow and file_viewer_capability_is_narrow.
        if PANEL_WINDOW_ONLY.contains(&command.as_str())
            || FILE_VIEWER_ONLY.contains(&command.as_str())
        {
            assert!(
                !permissions.contains(&permission.as_str()),
                "{permission} belongs to another window only"
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
    // Local claims: the main window turns them on and reads them; the panel window never does
    // (the main window pushes what the panel shows, panel_window_capability_is_narrow).
    // The agent gate likewise: only the main window answers held edits.
    for command in [
        "configure_claims",
        "claims_snapshot",
        "configure_gate",
        "gate_snapshot",
        "answer_gate",
    ] {
        assert!(commands.iter().any(|c| c == command), "{command} missing");
    }
    // The Hangar account: only the main window signs in, lists projects and opens account
    // pages; the panel window never holds these permissions.
    let panel: serde_json::Value = serde_json::from_str(&read("capabilities/panel.json")).unwrap();
    let panel_text = panel["permissions"].to_string();
    for command in [
        "account_status",
        "account_sign_in",
        "account_cancel_sign_in",
        "account_sign_out",
        "account_projects",
        "account_set_base_url",
        "account_open",
        // Opening a dev server address in the browser is the main window's.
        "open_local_url",
        // The team connection: sharing a project and confirming devices is the main window's.
        "team_status",
        "team_enable",
        "team_disable",
        "team_pending_confirmations",
        "team_confirm",
        "team_reject",
        // The activity calendar runs git in folders the window names: main window only.
        "git_activity",
    ] {
        assert!(commands.iter().any(|c| c == command), "{command} missing");
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(
            !panel_text.contains(&permission),
            "{permission} must not reach the panel window"
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

/// The detached side panel gets exactly what its tabs read (notes, changes, commit history, the
/// session's file tree and opening a viewer) and a few window permissions, nothing else: no
/// sessions, no claims or team commands (the main window pushes them), no profiles, no updates.
#[test]
fn panel_window_capability_is_narrow() {
    let commands = build_commands();
    let panel: serde_json::Value = serde_json::from_str(&read("capabilities/panel.json")).unwrap();
    assert_eq!(panel["windows"], serde_json::json!(["panel"]));
    let mut permissions: Vec<&str> = panel["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    permissions.sort_unstable();
    let mut expected = vec![
        // Listening only: emitting would let the panel window fake events of the main window,
        // such as `tray-quit`. Messages go through `panel_to_main`.
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        // Close asks the window's own handler first; destroy ends it once input is saved.
        "core:window:allow-close",
        "core:window:allow-destroy",
        "core:window:allow-set-focus",
        "clipboard-manager:allow-write-text",
        "allow-list-note-folders",
        "allow-list-notes",
        "allow-read-note",
        "allow-write-note",
        "allow-create-note",
        "allow-rename-note",
        "allow-delete-note",
        "allow-reorder-notes",
        "allow-reveal-notes-folder",
        "allow-search-notes",
        "allow-list-trash",
        "allow-restore-note",
        "allow-export-note",
        "allow-reveal-note",
        "allow-move-note",
        "allow-shared-notes-status",
        "allow-shared-notes-prepare",
        "allow-shared-notes-save-code",
        "allow-shared-notes-enable",
        "allow-shared-notes-unlock",
        "allow-shared-notes-copy-code",
        "allow-shared-notes-cancel",
        "allow-git-diff",
        "allow-git-log",
        "allow-git-commit-diff",
        "allow-list-session-folder",
        "allow-list-session-paths",
        "allow-reveal-session-folder",
        "allow-open-file-viewer",
        "allow-panel-to-main",
        "allow-set-panel-on-top",
    ];
    expected.sort_unstable();
    assert_eq!(permissions, expected);
    for permission in &permissions {
        if let Some(rest) = permission.strip_prefix("allow-") {
            let command = rest.replace('-', "_");
            assert!(
                commands.contains(&command),
                "{permission} names no registered command"
            );
        }
    }
    for command in PANEL_WINDOW_ONLY {
        assert!(commands.iter().any(|c| c == command), "{command} missing");
    }
    // The main window opens the panel window only through the backend command, never by
    // creating webviews itself.
    let main: serde_json::Value = serde_json::from_str(&read("capabilities/default.json")).unwrap();
    let main: Vec<&str> = main["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    assert!(main.contains(&"allow-open-panel-window"));
    for forbidden in [
        "core:webview:allow-create-webview-window",
        "core:webview:allow-create-webview",
        "core:window:allow-create",
    ] {
        assert!(!main.contains(&forbidden), "{forbidden} in default.json");
    }
    // The old notes window is gone: one detached window type only.
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("capabilities/notes.json")
        .exists());
}

/// A file viewer window reads the one file the backend bound to it and nothing else: no
/// sessions, no notes, no other files, no events of its own.
#[test]
fn file_viewer_capability_is_narrow() {
    let commands = build_commands();
    let viewer: serde_json::Value =
        serde_json::from_str(&read("capabilities/file-viewer.json")).unwrap();
    assert_eq!(viewer["windows"], serde_json::json!(["file-viewer-*"]));
    let mut permissions: Vec<&str> = viewer["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    permissions.sort_unstable();
    assert_eq!(
        permissions,
        [
            "allow-file-viewer-content",
            "allow-file-viewer-diff",
            "allow-file-viewer-open-link",
            "allow-file-viewer-stamp",
            "clipboard-manager:allow-write-text",
            "core:event:allow-listen",
            "core:event:allow-unlisten",
            "core:window:allow-close",
            "core:window:allow-set-focus",
            "core:window:allow-set-title",
        ]
    );
    for command in FILE_VIEWER_ONLY {
        assert!(commands.iter().any(|c| c == command), "{command} missing");
    }
    // The main window and the side panel open viewers (both show the Files tab); a viewer never
    // opens another one, and only a viewer reads a viewer's file.
    let main = read("capabilities/default.json");
    assert!(main.contains("\"allow-open-file-viewer\""));
    let panel = read("capabilities/panel.json");
    assert!(panel.contains("\"allow-open-file-viewer\""));
    for command in FILE_VIEWER_ONLY {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(!panel.contains(&permission), "{permission} in panel.json");
    }
}
