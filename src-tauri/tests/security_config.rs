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

/// The frontend only checks for an update and installs it in one step (src/useUpdater.ts), so the
/// main window gets exactly those two commands, not the separate download and install that
/// `updater:default` adds.
#[test]
fn the_updater_grants_only_check_and_download_and_install() {
    let capability: serde_json::Value =
        serde_json::from_str(&read("capabilities/default.json")).unwrap();
    let updater: Vec<&str> = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|permission| permission.starts_with("updater:"))
        .collect();
    assert_eq!(
        updater,
        ["updater:allow-check", "updater:allow-download-and-install"]
    );
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
    ] {
        let body = command_body(&commands, name);
        assert!(body.contains("spawn_blocking"), "{name} runs on the caller");
    }
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
