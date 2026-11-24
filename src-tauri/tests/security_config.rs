//! Security settings that live in configuration and source text rather than in behavior a unit
//! test reaches: the content security policy, the updater's signed version, the commands that
//! must not run on the main thread and the events that must not carry data.

use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn tauri_conf() -> serde_json::Value {
    serde_json::from_str(&read("tauri.conf.json")).unwrap()
}

/// The policy as directive name -> sources.
fn csp_directives() -> Vec<(String, String)> {
    let conf = tauri_conf();
    let csp = conf["app"]["security"]["csp"]
        .as_str()
        .expect("a content security policy");
    csp.split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (name, sources) = part.split_once(' ').unwrap_or((part, ""));
            (name.to_owned(), sources.trim().to_owned())
        })
        .collect()
}

#[test]
fn csp_closes_what_default_src_does_not_cover() {
    let directives = csp_directives();
    let get = |name: &str| {
        directives
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, sources)| sources.as_str())
    };
    // default-src does not apply to these; without them a page could post forms to any host,
    // change the base of relative URLs or be framed.
    assert_eq!(get("form-action"), Some("'none'"));
    assert_eq!(get("base-uri"), Some("'self'"));
    assert_eq!(get("object-src"), Some("'none'"));
    assert_eq!(get("frame-ancestors"), Some("'none'"));
    assert_eq!(get("default-src"), Some("'self'"));
    // No script source beyond the app's own.
    assert_eq!(get("script-src"), None);
    for (name, sources) in &directives {
        assert!(!sources.contains("'unsafe-eval'"), "{name}: {sources}");
        assert!(!sources.contains('*'), "{name}: {sources}");
    }
}

/// The update endpoint's answer is not signed: without this, an old (validly signed) installer
/// could be offered under a new version number. Key name checked against
/// tauri-plugin-updater 2.14 (`Config::require_signed_version`, camelCase in JSON).
#[test]
fn updater_requires_the_signed_version() {
    let conf = tauri_conf();
    let updater = &conf["plugins"]["updater"];
    assert_eq!(updater["requireSignedVersion"], serde_json::json!(true));
    assert!(updater.get("allowDowngrades").is_none());
    assert!(updater.get("dangerousInsecureTransportProtocol").is_none());
    for endpoint in updater["endpoints"].as_array().unwrap() {
        assert!(endpoint.as_str().unwrap().starts_with("https://"));
    }
}

/// The body of `pub async fn <name>(` up to the next command attribute.
fn command_body<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source
        .find(&format!("pub async fn {name}("))
        .unwrap_or_else(|| panic!("{name} is not an async command"));
    let rest = &source[start..];
    let end = rest[1..]
        .find("#[tauri::command]")
        .map_or(rest.len(), |i| i + 1);
    &rest[..end]
}

/// Starting a process searches the PATH, may wait for the WSL detection and spawns ConPTY; a
/// synchronous command would do that on the main thread and freeze the window.
#[test]
fn session_starts_run_off_the_main_thread() {
    let commands = read("src/commands.rs");
    for name in ["create_session", "restart_session", "create_update_session"] {
        let body = command_body(&commands, name);
        assert!(body.contains("spawn_blocking"), "{name} runs on the caller");
    }
}

/// Who is in a link group decides which agent's text reaches which session (ADR 0014): the
/// commands that form or change a group answer the main window alone, also should a capability
/// grant them elsewhere by mistake.
#[test]
fn link_group_changes_answer_the_main_window_only() {
    let commands = read("src/commands.rs");
    for name in [
        "link_state",
        "link_sessions",
        "add_to_link_group",
        "edit_link_group",
    ] {
        let body = command_body(&commands, name);
        assert!(
            body.contains("main_store::check_main(window.label())?;"),
            "{name} does not check the window"
        );
    }
}

/// A listener for any target receives events emitted to the main window, so the panel window
/// (which may listen) could read them. The gate's held edits name files and commands: the event
/// is a bare signal, the snapshot comes from `gate_snapshot`, which only the main window may call.
#[test]
fn the_gate_event_carries_no_data() {
    let lib = read("src/lib.rs");
    assert!(
        lib.contains("emit_to(panel_window::MAIN_LABEL, gate_service::EVENT, ())"),
        "gate-changed must be emitted without payload"
    );
    let panel = read("capabilities/panel.json");
    assert!(!panel.contains("allow-gate-snapshot"));
    let viewer = read("capabilities/file-viewer.json");
    assert!(!viewer.contains("core:event:allow-listen"));
}

/// The waiting cross-project messages carry an agent's text: their event is a bare signal, the
/// texts come from `waiting_messages`, which only the main window may call.
#[test]
fn the_waiting_messages_event_carries_no_data() {
    let lib = read("src/lib.rs");
    assert!(
        lib.contains("emit_to(panel_window::MAIN_LABEL, partner_service::EVENT, ())"),
        "partner-messages-changed must be emitted without payload"
    );
    for window in ["panel.json", "file-viewer.json"] {
        let capability = read(&format!("capabilities/{window}"));
        assert!(!capability.contains("allow-waiting-message"), "{window}");
        assert!(
            !capability.contains("allow-close-waiting-message"),
            "{window}"
        );
    }
}

/// The git reads name a session, never a folder (the backend finds it): a window cannot point
/// git, which runs programs a repository's config names, at a folder of its choice.
#[test]
fn git_reads_take_a_session_not_a_folder() {
    let commands = read("src/commands.rs");
    for name in [
        "git_diff",
        "git_changed_count",
        "git_log",
        "git_commit_diff",
    ] {
        let body = command_body(&commands, name);
        let signature = &body[..body.find('{').unwrap()];
        assert!(signature.contains("id: SessionId"), "{name}: {signature}");
        assert!(!signature.contains("cwd"), "{name}: {signature}");
    }
}

/// The main window gets exactly the updater commands the frontend calls (src/useUpdater.ts),
/// derived from the calls themselves: 0.14.0 split the install into `download` and `install`
/// while the capability still granted only `download-and-install`, and every update from it
/// failed with "not allowed by ACL". No other window may update.
#[test]
fn the_updater_grants_exactly_the_commands_the_frontend_calls() {
    let frontend = read("../src/useUpdater.ts");
    let mut needed = vec!["updater:allow-check"];
    for (call, permission) in [
        (".downloadAndInstall(", "updater:allow-download-and-install"),
        (".download(", "updater:allow-download"),
        (".install(", "updater:allow-install"),
    ] {
        if frontend.contains(call) {
            needed.push(permission);
        }
    }
    assert!(
        needed.len() > 1,
        "src/useUpdater.ts installs no update any more"
    );
    let capability: serde_json::Value =
        serde_json::from_str(&read("capabilities/default.json")).unwrap();
    let mut granted: Vec<&str> = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|permission| permission.starts_with("updater:"))
        .collect();
    granted.sort_unstable();
    needed.sort_unstable();
    assert_eq!(granted, needed);
    for other in ["panel.json", "file-viewer.json"] {
        let text = read(&format!("capabilities/{other}"));
        assert!(!text.contains("updater:"), "{other} grants the updater");
    }
}

/// Ending a process kills a Job Object or waits on ConPTY, removing a gate rule writes a file,
/// opening a page starts the browser, and linking a repository writes the team settings: none of
/// that may run on the main thread, where synchronous commands run.
#[test]
fn blocking_commands_run_off_the_main_thread() {
    let commands = read("src/commands.rs");
    for name in [
        "close_session",
        "kill_session",
        "account_open",
        "delete_gate_rule",
        "team_link_repo",
        // An "always" answer writes the gate rules; the guard's info reads its rules file.
        "answer_gate",
        "command_guard_info",
        "protected_settings",
        "save_protected_settings",
        "main_prefs",
        "save_main_prefs",
        "load_templates",
        "save_templates",
        "load_recently_closed",
        "save_recently_closed",
        "partner_projects",
        "link_partner",
        "unlink_partner",
        "partner_targets",
        "deliver_partner_message",
        "waiting_messages",
        "waiting_message_text",
        "close_waiting_message",
        // The usage statistics write their state file.
        "telemetry_set_consent",
        "telemetry_settings",
        "telemetry_count",
        "telemetry_open_info",
    ] {
        let body = command_body(&commands, name);
        assert!(body.contains("spawn_blocking"), "{name} runs on the caller");
    }
}

/// The note commands the panel may call with a folder check it first: the shared notes live in
/// a repository (and run git there), so the panel reaches only those of its session's folder.
#[test]
fn panel_note_commands_check_the_handed_folder() {
    let commands = read("src/commands.rs");
    for name in [
        "list_notes",
        "read_note",
        "write_note",
        "create_note",
        "rename_note",
        "delete_note",
        "reorder_notes",
        "reveal_notes_folder",
        "list_trash",
        "restore_note",
        "export_note",
        "reveal_note",
        "shared_notes_status",
        "shared_notes_prepare",
        "shared_notes_save_code",
        "shared_notes_copy_code",
        "shared_notes_cancel",
        "shared_notes_enable",
        "shared_notes_unlock",
    ] {
        let body = command_body(&commands, name);
        assert!(
            body.contains("notes_in_scope(window.label(), &scope, &folder)?;"),
            "{name} does not check the panel's folder"
        );
    }
    let moved = command_body(&commands, "move_note");
    for folder in ["&from", "&to"] {
        assert!(
            moved.contains(&format!(
                "notes_in_scope(window.label(), &scope, {folder})?;"
            )),
            "move_note does not check {folder}"
        );
    }
    // Every command that takes a note folder is in the lists above.
    let mut named = 0;
    for (at, _) in commands.match_indices("    folder: NoteFolder,\n") {
        let head = commands[..at].rfind("pub async fn ").unwrap();
        let body = command_body(&commands, {
            let rest = &commands[head + "pub async fn ".len()..];
            &rest[..rest.find('(').unwrap()]
        });
        assert!(body.contains("notes_in_scope("), "{}", &body[..80]);
        named += 1;
    }
    assert!(named >= 19, "{named}");
    // The search spans every unlocked repository: its hits pass the same rule.
    assert!(command_body(&commands, "search_notes").contains("scope.allows_hit(label, hit)"));
    // Text from the panel goes to the handed session only (panel_to_main is synchronous).
    let start = commands.find("pub fn panel_to_main(").unwrap();
    let end = commands[start..].find("\n}\n").unwrap() + start;
    assert!(commands[start..end].contains("message.scoped(&scope)?"));
}

/// explorer.exe parses its own command line and splits a path at commas (`C:\Users\Doe, Jane`),
/// so it may open another folder or file than the one meant. Folders and files open through
/// `desktop::open_folder` and `desktop::open_text_file` (ShellExecute on Windows) instead.
#[test]
fn folders_and_files_never_open_through_explorer_arguments() {
    for file in ["src/commands.rs", "src/notes.rs"] {
        let source = read(file);
        assert!(
            !source.contains("desktop::open("),
            "{file} opens a path through explorer.exe's command line"
        );
    }
}

/// Like the gate's: the claims and the update results name files, sessions and program paths, and
/// the panel window may listen. Both events are bare signals; the main window reads
/// `claims_snapshot` and `take_update_results`, which the panel's capability does not grant.
#[test]
fn claims_and_update_result_events_carry_no_data() {
    let lib = read("src/lib.rs");
    assert!(
        lib.contains("emit_to(panel_window::MAIN_LABEL, claims_service::EVENT, ())"),
        "claims-changed must be emitted without payload"
    );
    let commands = read("src/commands.rs");
    assert!(commands.contains("emit_to(panel_window::MAIN_LABEL, UPDATE_RESULT_EVENT, ())"));
    assert_eq!(commands.matches("UPDATE_RESULT_EVENT,").count(), 1);
    let panel = read("capabilities/panel.json");
    for permission in ["allow-claims-snapshot", "allow-take-update-results"] {
        assert!(!panel.contains(permission), "{permission}");
    }
}

/// Every command the panel window may call with a session id checks that it is the session the
/// main window handed the panel (`PanelScope`).
#[test]
fn panel_session_commands_check_the_handed_session() {
    let commands = read("src/commands.rs");
    for name in [
        "git_diff",
        "git_log",
        "git_commit_diff",
        "list_session_folder",
        "list_session_paths",
        "reveal_session_folder",
        "open_file_viewer",
    ] {
        let body = command_body(&commands, name);
        assert!(
            body.contains("in_scope(&window, &scope,") || body.contains("scope.allows("),
            "{name} does not check the panel's session"
        );
    }
    for name in ["git_stage", "git_unstage", "git_discard", "git_commit"] {
        let body = command_body(&commands, name);
        assert!(
            body.contains("git_action(&window, &scope,"),
            "{name} does not check the panel's session"
        );
    }
}

/// Stage, unstage and discard switch hooks off through git itself (git.rs, `core.hooksPath` below
/// the git program); they create no folder of their own, so nothing left in Hangar's data folder
/// can block them.
#[test]
fn git_actions_create_no_hooks_folder() {
    let commands = read("src/commands.rs");
    for name in ["git_stage", "git_unstage", "git_discard"] {
        let body = command_body(&commands, name);
        assert!(!body.contains("AppHandle"), "{name} takes the app handle");
        assert!(
            !body.contains("app_local_data_dir"),
            "{name} touches the data folder"
        );
    }
    assert!(!read("src/git_actions.rs").contains("git-no-hooks"));
}
