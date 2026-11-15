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
