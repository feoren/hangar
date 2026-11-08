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
