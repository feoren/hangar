//! Hangar backend: owns all processes, the frontend only renders.

pub mod agent_catalog;
pub mod batch;
pub mod cli_session;
mod commands;
pub mod desktop;
pub mod export;
pub mod filecache;
pub mod git;
pub mod icon;
pub mod install_source;
pub mod manager;
pub mod notes;
pub mod notify;
pub mod output;
pub mod persist;
pub mod proctree;
pub mod profile;
pub mod profile_edit;
pub mod resolve;
pub mod secrets;
pub mod session;
pub mod subagents;
#[cfg(test)]
mod test_support;
pub mod text;
pub mod tray;
pub mod updates;
pub mod version;
pub mod worktree;
pub mod wsl;

use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::{Manager, RunEvent};

use crate::commands::{ProbeCaches, ProfileLoad};
use crate::manager::SessionManager;

/// Locks a mutex even if another thread panicked while holding it. The protected data stays
/// consistent because no write is ever left half done across a panic.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .on_window_event(tray::on_window_event)
        .setup(|app| {
            let home = app.path().home_dir()?;
            let config_dir = app.path().app_config_dir()?;
            let profile_path = config_dir.join(profile::FILE_NAME);
            let (profiles, error) = match profile::load_or_init(&config_dir) {
                Ok(profiles) => (profiles, None),
                Err(error) => (
                    Vec::new(),
                    Some(format!("{error} ({})", profile_path.display())),
                ),
            };
            // The catalog is embedded and checked by a test; should it fail anyway, the user
            // profiles at least keep working.
            let catalog = agent_catalog::builtin().unwrap_or_else(|error| {
                eprintln!("hangar: {error}");
                Vec::new()
            });
            app.manage(SessionManager::new(profiles, catalog, home));
            app.manage(ProfileLoad {
                error: Mutex::new(error),
                dir: config_dir,
                path: profile_path,
                edit: Mutex::new(()),
            });
            // WSL detection takes a few hundred milliseconds; started now, it is usually done
            // before the window asks for the list (which otherwise detects itself).
            let handle = app.handle().clone();
            let detection = std::thread::Builder::new()
                .name("wsl-detect".to_owned())
                .spawn(move || handle.state::<SessionManager>().ensure_wsl());
            if let Err(error) = detection {
                eprintln!("hangar: WSL detection not started: {error}");
            }
            app.manage(ProbeCaches::default());
            app.manage(notify::Notifier::default());
            app.manage(subagents::Watchers::default());
            // Local rather than roaming: scrollback is large, may contain secrets and belongs to
            // this machine (Windows: %LOCALAPPDATA%\<identifier>).
            let local_dir = app.path().app_local_data_dir()?;
            let notes = notes::NotesStore::new(&local_dir);
            notes.migrate_legacy_layout();
            app.manage(notes);
            app.manage(persist::Store::new(local_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::default_cwd,
            commands::list_sessions,
            commands::create_session,
            commands::write_session,
            commands::resize_session,
            commands::ack_session,
            commands::kill_session,
            commands::restart_session,
            commands::rename_session,
            commands::close_session,
            commands::detect_versions,
            commands::program_icons,
            commands::profile_path,
            commands::reload_profiles,
            commands::open_profile_file,
            commands::save_workspace,
            commands::load_workspace,
            commands::save_scrollback,
            commands::load_scrollback,
            commands::delete_scrollback,
            commands::clear_scrollbacks,
            commands::list_note_folders,
            commands::list_notes,
            commands::read_note,
            commands::write_note,
            commands::create_note,
            commands::rename_note,
            commands::delete_note,
            commands::reorder_notes,
            commands::reveal_notes_folder,
            commands::search_notes,
            commands::list_trash,
            commands::restore_note,
            commands::export_note,
            commands::reveal_note,
            commands::check_program_updates,
            commands::create_update_session,
            commands::notify_session,
            commands::git_branches,
            commands::git_diff,
            commands::create_worktree,
            commands::remove_worktree,
            commands::save_profile,
            commands::delete_profile,
            commands::profile_env,
            commands::set_profile_secret,
            commands::delete_profile_secret,
            commands::profile_secrets,
            commands::export_session,
            commands::configure_tray,
            commands::watch_sub_agents,
            commands::unwatch_sub_agents,
        ])
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Do not let any agent outlive the app.
            handle.state::<subagents::Watchers>().stop_all();
            handle.state::<SessionManager>().shutdown();
        }
    });
    Ok(())
}
