// The app commands are registered here explicitly. Tauri then generates one permission per
// command (allow-<command>), and the frontend may only call what is granted in
// capabilities/default.json.
const COMMANDS: &[&str] = &[
    "list_profiles",
    "default_cwd",
    "list_sessions",
    "create_session",
    "write_session",
    "resize_session",
    "ack_session",
    "kill_session",
    "restart_session",
    "rename_session",
    "close_session",
    "detect_versions",
    "program_icons",
    "profile_path",
    "reload_profiles",
    "open_profile_file",
    "save_workspace",
    "load_workspace",
    "save_scrollback",
    "load_scrollback",
    "delete_scrollback",
    "clear_scrollbacks",
    "list_note_folders",
    "list_notes",
    "read_note",
    "write_note",
    "create_note",
    "rename_note",
    "delete_note",
    "reorder_notes",
    "reveal_notes_folder",
    "check_program_updates",
    "create_update_session",
    "notify_session",
    "git_branches",
    "git_diff",
    "create_worktree",
    "remove_worktree",
    "save_profile",
    "delete_profile",
    "profile_env",
    "set_profile_secret",
    "delete_profile_secret",
    "profile_secrets",
    "export_session",
    "configure_tray",
    "watch_sub_agents",
    "unwatch_sub_agents",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    // A build script reports errors to Cargo by panicking; that is the intended interface.
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("Tauri build failed: {error:#}");
    }
}
