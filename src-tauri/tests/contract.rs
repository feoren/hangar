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
const PANEL_WINDOW_ONLY: &[&str] = &[
    "panel_to_main",
    "set_panel_on_top",
    "close_panel_window",
    "destroy_panel_window",
];
/// Commands granted to the file viewer windows and to no other window.
const FILE_VIEWER_ONLY: &[&str] = &[
    "file_viewer_content",
    "file_viewer_stamp",
    "file_viewer_diff",
    "file_viewer_open_link",
];

/// Tauri hands an event sent with `emit_to` to one window also to listeners of other webviews
/// that listen to any target (the panel may listen). The account and team signals therefore
/// carry no payload; the main window fetches the state with commands only it may call.
#[test]
fn account_and_team_signals_carry_no_payload() {
    let lib = read("src/lib.rs");
    for event in ["account::EVENT", "team::EVENT"] {
        let emits: Vec<&str> = lib
            .match_indices(event)
            .map(|(at, _)| {
                let end = lib[at..].find(')').map_or(lib.len(), |e| at + e);
                &lib[at..end]
            })
            .collect();
        assert!(!emits.is_empty(), "{event} is never emitted");
        for emit in emits {
            assert_eq!(
                emit,
                format!("{event}, ("),
                "{event} carries a payload: {emit}"
            );
        }
    }
    for path in ["capabilities/panel.json", "capabilities/file-viewer.json"] {
        let capability = read(path);
        for command in ["allow-team-snapshot", "allow-account-status"] {
            assert!(!capability.contains(command), "{path} grants {command}");
        }
    }
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
        "gate_rules",
        "delete_gate_rule",
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
        // Paths and addresses clicked in the terminal name any local path: main window only.
        "stat_paths",
        "open_path",
        "open_web_url",
        // The team connection: sharing a project and confirming devices is the main window's.
        "team_status",
        "team_enable",
        "team_disable",
        "team_pending_confirmations",
        "team_confirm",
        "team_reject",
        // The activity calendar runs git in folders the window names: main window only.
        "git_activity",
        // Session resources and push to the phone: the push settings hold keychain secrets
        // and send to the network, so only the main window reaches them.
        "session_resources",
        "push_settings",
        "push_save",
        "push_test",
        "push_session",
        // The panel and file viewer windows share the main window's localStorage; what they
        // must not change or read lives in the backend for the main window alone.
        "security_switches",
        "save_security_switches",
        "load_message_queue",
        "save_message_queue",
        "protected_settings",
        "save_protected_settings",
        "main_prefs",
        "save_main_prefs",
        "load_templates",
        "save_templates",
        // Partner projects decide which agent a message reaches (ADR 0012).
        "partner_projects",
        "link_partner",
        "unlink_partner",
        "partner_targets",
        "deliver_partner_message",
        "set_panel_session",
        "take_update_results",
        "claims_snapshot",
        // Dictation opens the microphone and downloads models: main window only.
        "dictation_status",
        "dictation_start",
        "dictation_stop",
        "dictation_take_text",
        "dictation_install_model",
        "dictation_remove_model",
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
        // The Changes tab works in the panel window too, so it may change the active session's
        // repository: by session ID only (the backend finds the folder), paths checked, discard
        // confirmed in the window. It still cannot start, write to or end a session.
        "allow-git-stage",
        "allow-git-unstage",
        "allow-git-discard",
        "allow-git-commit",
        // The timeline view: list a session's snapshots, one step's changes, and go back to one
        // after the window asked (the backend saves the current state first). Taking a snapshot
        // and deleting them stay with the main window, which sees the attention changes and
        // closes sessions.
        "allow-snapshot-list",
        "allow-snapshot-diff",
        "allow-snapshot-restore",
        "allow-list-session-folder",
        "allow-list-session-paths",
        "allow-reveal-session-folder",
        "allow-open-file-viewer",
        "allow-panel-to-main",
        "allow-set-panel-on-top",
        // Close asks the window's own handler first; destroy ends it once input is saved. Both
        // act on the calling window only, unlike core:window:allow-close and allow-destroy.
        "allow-close-panel-window",
        "allow-destroy-panel-window",
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

/// The window plugin's commands take a window label and act on whichever window it names: a
/// panel or viewer holding `core:window:allow-close` could close the main window, one holding
/// `allow-set-focus` could pull it to the front at will. Only the trusted main window holds such
/// permissions; the panel closes itself through its own commands.
#[test]
fn no_window_permissions_outside_the_main_window() {
    for path in ["capabilities/panel.json", "capabilities/file-viewer.json"] {
        let capability: serde_json::Value = serde_json::from_str(&read(path)).unwrap();
        for permission in capability["permissions"].as_array().unwrap() {
            let permission = permission.as_str().unwrap_or_default();
            for forbidden in ["core:window:", "core:webview:", "core:app:", "core:default"] {
                assert!(
                    !permission.starts_with(forbidden),
                    "{path} grants {permission}"
                );
            }
        }
    }
}

/// The body of `pub fn <name>(` (sync or async) up to the next command attribute.
fn command_body<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source
        .find(&format!("pub fn {name}("))
        .or_else(|| source.find(&format!("pub async fn {name}(")))
        .unwrap_or_else(|| panic!("{name} is not a command"));
    let rest = &source[start..];
    let end = rest[1..]
        .find("#[tauri::command]")
        .map_or(rest.len(), |i| i + 1);
    &rest[..end]
}

/// The panel's own window commands check the caller and act on the calling window only: they
/// take no label and use the window Tauri hands the command.
#[test]
fn panel_window_commands_act_on_the_caller_only() {
    let commands = read("src/commands.rs");
    for name in [
        "close_panel_window",
        "destroy_panel_window",
        "set_panel_on_top",
        "panel_to_main",
    ] {
        let body = command_body(&commands, name);
        assert!(
            body.contains("panel_window::check_sender(window.label())?;"),
            "{name} does not check the caller"
        );
        let signature = &body[..body.find('{').unwrap()];
        assert!(!signature.contains("label"), "{name}: {signature}");
    }
    assert!(command_body(&commands, "close_panel_window").contains("window.close()"));
    assert!(command_body(&commands, "destroy_panel_window").contains("window.destroy()"));
}

/// A file viewer window reads the one file the backend bound to it and nothing else: no
/// sessions, no notes, no other files, no events at all (listening would let it read every event
/// the app emits, since a listener for any target receives events meant for the main window).
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

/// Like the gate's and the claims': a listener in any window receives events emitted to the main
/// one, and the panel may listen. The dictation state and the dictated text therefore go out as
/// bare signals; the main window reads them through `dictation_status` and `dictation_take_text`.
/// Only the level carries data, a number from 0 to 1 (`Notice::Level(f32)`). Every dictation
/// command checks that the main window called it.
#[test]
fn dictation_signals_carry_no_text_and_commands_check_the_window() {
    let lib = read("src/lib.rs");
    for event in ["dictation::STATUS_EVENT", "dictation::TEXT_EVENT"] {
        assert!(
            lib.contains(&format!("emit_to(main, {event}, ())")),
            "{event} must be emitted without payload"
        );
        assert_eq!(lib.matches(event).count(), 1, "{event} emitted elsewhere");
    }
    assert!(lib.contains("emit_to(main, dictation::LEVEL_EVENT, level)"));
    let service = read("src/dictation/mod.rs");
    assert!(
        service.contains("    Level(f32),"),
        "the level is a number only"
    );
    let commands = read("src/commands.rs");
    for name in [
        "dictation_status",
        "dictation_start",
        "dictation_stop",
        "dictation_take_text",
        "dictation_install_model",
        "dictation_remove_model",
    ] {
        assert!(
            command_body(&commands, name).contains("dictation_main(&window)?;"),
            "{name} does not check the caller"
        );
    }
    assert!(commands.contains("panel_window::check_main(window.label()).map_err("));
    for path in ["capabilities/panel.json", "capabilities/file-viewer.json"] {
        let capability = read(path);
        assert!(
            !capability.contains("allow-dictation-"),
            "{path} grants dictation"
        );
    }
}

/// macOS asks for two things before an app with the hardened runtime may open the microphone: the
/// usage text in Info.plist (merged by the Tauri bundler from the file next to tauri.conf.json)
/// and the audio-input entitlement in the signature.
#[test]
fn macos_bundle_may_open_the_microphone() {
    let config: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(
        config["bundle"]["macOS"]["entitlements"],
        serde_json::json!("Entitlements.plist")
    );
    let entitlements = read("Entitlements.plist");
    assert!(entitlements.contains("<key>com.apple.security.device.audio-input</key>\n  <true/>"));
    assert!(read("Info.plist").contains("<key>NSMicrophoneUsageDescription</key>"));
}
