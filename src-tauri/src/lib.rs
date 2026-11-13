//! Hangar backend: owns all processes, the frontend only renders.

pub mod account;
pub mod account_api;
pub mod account_service;
pub mod agent_catalog;
pub mod avatar;
pub mod batch;
pub mod child_env;
pub mod claims;
pub mod claims_service;
pub mod cli_session;
pub mod clipboard_image;
mod commands;
pub mod context_usage;
pub mod desktop;
pub mod exe_image;
pub mod exec_alias;
pub mod export;
pub mod file_tree;
pub mod file_viewer;
pub mod filecache;
pub mod gate;
pub mod gate_hook;
pub mod gate_rules;
pub mod gate_service;
pub mod git;
pub mod git_actions;
pub mod git_activity;
pub mod git_history;
pub mod gpu;
pub mod hook_http;
pub mod hook_recipes;
pub mod icon;
pub mod install_source;
pub mod instructions;
pub mod manager;
pub mod notes;
pub mod notify;
pub mod output;
pub mod panel_window;
pub mod paste_image;
pub mod persist;
pub mod proctree;
pub mod profile;
pub mod profile_edit;
pub mod project_notes;
pub mod push;
pub mod resolve;
pub mod resources;
pub mod secrets;
pub mod session;
pub mod single_instance;
pub mod subagents;
pub mod system_usage;
pub mod team;
pub mod team_engine;
pub mod team_relay;
pub mod team_service;
pub mod team_share;
pub mod team_tasks;
#[cfg(test)]
mod test_support;
pub mod text;
pub mod tray;
pub mod updates;
pub mod version;
pub mod worktree;
pub mod wsl;

use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::{Emitter as _, Manager, RunEvent};

use crate::commands::{ProbeCaches, ProfileLoad};
use crate::manager::SessionManager;

/// Locks a mutex even if another thread panicked while holding it. The protected data stays
/// consistent because no write is ever left half done across a panic.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn run() -> tauri::Result<()> {
    let mut builder = tauri::Builder::default();
    // First of all plugins, so a second start exits before anything else runs; debug builds are
    // exempt (see `single_instance::ENFORCED`).
    if single_instance::ENFORCED {
        builder = builder.plugin(single_instance::plugin());
    }
    let app = builder
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
            app.manage(
                SessionManager::new(profiles, catalog, home)
                    .with_instructions_file(config_dir.join(instructions::FILE_NAME)),
            );
            // Hangar account: the chosen server comes from the config folder; the sign-in itself
            // is read from the keychain on the window's first status call.
            let events = app.handle().clone();
            app.manage(account_service::AccountService::new(
                account::load_base(&config_dir),
                std::sync::Arc::new(secrets::KeyringStore::with_service(
                    account::KEYCHAIN_SERVICE,
                )),
                std::sync::Arc::new(|url: &reqwest::Url| desktop::open_url(url)),
                std::sync::Arc::new(move |view: &account::AccountView| {
                    let _ = events.emit_to(panel_window::MAIN_LABEL, account::EVENT, view);
                }),
                account_service::Options {
                    config_dir: Some(config_dir.clone()),
                    avatar_dir: Some(app.path().app_local_data_dir()?.join(avatar::CACHE_DIR)),
                    ..account_service::Options::default()
                },
            ));
            // Team connections: one worker per project with "Share with team" on; started
            // here so shared projects reconnect without the window.
            let events = app.handle().clone();
            let team = team_service::TeamService::new(
                app.state::<account_service::AccountService>()
                    .inner()
                    .clone(),
                std::sync::Arc::new(secrets::KeyringStore::with_service(team::KEYCHAIN_SERVICE)),
                std::sync::Arc::new(move |snapshot: &team::TeamSnapshot| {
                    let _ = events.emit_to(panel_window::MAIN_LABEL, team::EVENT, snapshot);
                }),
                team_service::Options {
                    config_dir: Some(config_dir.clone()),
                    state_dir: Some(app.path().app_local_data_dir()?),
                    ..team_service::Options::default()
                },
            );
            team.start_enabled();
            app.manage(team);
            // Push to the phone: off until the user sets up a provider; secrets in the keychain.
            app.manage(push::PushService::new(
                &config_dir,
                std::sync::Arc::new(secrets::KeyringStore::with_service(push::KEYCHAIN_SERVICE)),
            ));
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
            // Pasted screenshots older than a week go; the next paste would do that as well,
            // but without one they would stay. Housekeeping only: an unknown cache folder must
            // not stop the app from starting.
            match app.path().app_cache_dir() {
                Ok(cache) => {
                    let pastes = cache.join(paste_image::FOLDER);
                    let cleanup = std::thread::Builder::new()
                        .name("paste-cleanup".to_owned())
                        .spawn(move || {
                            paste_image::cleanup(
                                &pastes,
                                std::time::SystemTime::now(),
                                paste_image::MAX_AGE,
                            )
                        });
                    if let Err(error) = cleanup {
                        eprintln!("hangar: paste cleanup not started: {error}");
                    }
                }
                Err(error) => eprintln!("hangar: paste cleanup skipped: {error}"),
            }
            app.manage(paste_image::PasteGate::default());
            app.manage(ProbeCaches::default());
            // Team sessions need a program version at or above the gate's floor (ADR 0003 query).
            app.state::<SessionManager>()
                .set_version_source(std::sync::Arc::new(commands::AppVersions(
                    app.handle().clone(),
                )));
            app.manage(file_tree::FileTree::default());
            app.manage(file_viewer::Viewers::default());
            app.manage(notify::Notifier::default());
            app.manage(resources::ResourceMonitor::default());
            app.manage(system_usage::SystemMonitor::default());
            // One GPU reading for the system bar and the session rows alike.
            app.manage(gpu::GpuMonitor::default());
            app.manage(subagents::Watchers::default());
            // Context meter of the sessions' rows: one thread for all sessions, numbers only.
            let context = context_usage::ContextUsage::default();
            let sessions = app.handle().clone();
            let events = app.handle().clone();
            let started = context.start(
                move || sessions.state::<SessionManager>().context_targets(),
                move |snapshot| {
                    let _ =
                        events.emit_to(panel_window::MAIN_LABEL, context_usage::EVENT, snapshot);
                },
            );
            if let Err(error) = started {
                eprintln!("hangar: context meter not started: {error}");
            }
            app.manage(context);
            // Local rather than roaming: scrollback is large, may contain secrets and belongs to
            // this machine (Windows: %LOCALAPPDATA%\<identifier>).
            let local_dir = app.path().app_local_data_dir()?;
            let notes = notes::NotesStore::new(&local_dir);
            notes.migrate_legacy_layout();
            app.manage(notes);
            // Shared project notes (ADR 0006): key rings in the keychain, trash next to the
            // private notes.
            app.manage(project_notes::ProjectNotes::new(
                &local_dir,
                std::sync::Arc::new(secrets::KeyringStore::with_service(
                    project_notes::KEYCHAIN_SERVICE,
                )),
            ));
            // Hook settings for cmd.exe launchers of gated agents; they hold no secret.
            let gate_dir = local_dir.join("gate");
            // The person's gate rules: local, they name this machine's
            // repositories.
            let gate_rules_file = local_dir.join(gate_rules::FILE_NAME);
            app.manage(persist::Store::new(local_dir));
            // Local claims: the worker stays idle until the window turns the setting on.
            let claims = claims_service::ClaimsService::default();
            let sessions = app.handle().clone();
            let events = app.handle().clone();
            let started = claims.start(
                move || claims_service::watched_sessions(&sessions.state::<SessionManager>()),
                move |snapshot| {
                    let _ =
                        events.emit_to(panel_window::MAIN_LABEL, claims_service::EVENT, snapshot);
                },
            );
            if let Err(error) = started {
                eprintln!("hangar: claims not started: {error}");
            }
            app.manage(claims);
            // Agent gate: holds an agent's edit of a file another session is changing until the
            // person decides; idle until the window turns it on and a gated agent starts.
            let worktrees = app.handle().clone();
            let titles = app.handle().clone();
            let events = app.handle().clone();
            let gate = gate_service::GateService::new(
                gate_dir,
                move || {
                    worktrees
                        .state::<claims_service::ClaimsService>()
                        .worktrees()
                },
                move || {
                    titles
                        .state::<SessionManager>()
                        .list()
                        .into_iter()
                        .map(|info| (info.id, info.title))
                        .collect()
                },
                move |snapshot| {
                    let _ = events.emit_to(panel_window::MAIN_LABEL, gate_service::EVENT, snapshot);
                },
            );
            gate.load_rules(gate_rules_file);
            app.state::<SessionManager>()
                .set_launch_gate(std::sync::Arc::new(gate.clone()));
            // The team shares the sessions of repositories linked to a shared project; the gate
            // pauses an edit of a file a teammate's session is changing (step F part 2b).
            let teammates = app.handle().clone();
            gate.set_teammates(move |repo, folder| {
                teammates
                    .state::<team_service::TeamService>()
                    .teammate_files(repo, folder)
            });
            // Reservations of the team: their areas pause edits like a teammate's file, and the
            // agent's context names them.
            let reserved = app.handle().clone();
            gate.set_team_tasks(move |repo, folder| {
                reserved
                    .state::<team_service::TeamService>()
                    .team_tasks(repo, folder)
            });
            app.manage(gate);
            let source = app.handle().clone();
            app.state::<team_service::TeamService>()
                .set_local_source(move || {
                    let worktrees = source.state::<claims_service::ClaimsService>().worktrees();
                    let manager = source.state::<SessionManager>();
                    // Names only: `profiles()` would run the PATH search and the gate's version
                    // query on every session state change, here on the main thread.
                    let names = manager.profile_names();
                    // Private sessions stay off the team (ADR 0008).
                    let labels = team_share::team_labels(manager.list(), &names);
                    (worktrees, labels)
                });
            let refresh = app.handle().clone();
            app.state::<claims_service::ClaimsService>()
                .on_worktrees(move || {
                    refresh.state::<team_service::TeamService>().refresh_local();
                });
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
            commands::open_hangar_md,
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
            commands::move_note,
            commands::shared_notes_status,
            commands::shared_notes_prepare,
            commands::shared_notes_save_code,
            commands::shared_notes_enable,
            commands::shared_notes_unlock,
            commands::shared_notes_copy_code,
            commands::shared_notes_cancel,
            commands::open_panel_window,
            commands::set_panel_on_top,
            commands::panel_to_main,
            commands::check_program_updates,
            commands::create_update_session,
            commands::notify_session,
            commands::session_resources,
            commands::system_usage,
            commands::push_settings,
            commands::push_save,
            commands::push_test,
            commands::push_session,
            commands::git_branches,
            commands::git_diff,
            commands::git_activity,
            commands::git_log,
            commands::git_commit_diff,
            commands::git_stage,
            commands::git_unstage,
            commands::git_discard,
            commands::git_commit,
            commands::list_session_folder,
            commands::list_session_paths,
            commands::reveal_session_folder,
            commands::open_local_url,
            commands::open_file_viewer,
            commands::file_viewer_content,
            commands::file_viewer_stamp,
            commands::file_viewer_diff,
            commands::file_viewer_open_link,
            commands::create_worktree,
            commands::remove_worktree,
            commands::configure_claims,
            commands::claims_snapshot,
            commands::configure_gate,
            commands::gate_snapshot,
            commands::answer_gate,
            commands::gate_rules,
            commands::delete_gate_rule,
            commands::save_profile,
            commands::delete_profile,
            commands::profile_env,
            commands::set_profile_secret,
            commands::delete_profile_secret,
            commands::profile_secrets,
            commands::export_session,
            commands::paste_clipboard_image,
            commands::configure_tray,
            commands::watch_sub_agents,
            commands::focus_sub_agent,
            commands::unwatch_sub_agents,
            commands::context_usage,
            commands::account_status,
            commands::account_sign_in,
            commands::account_cancel_sign_in,
            commands::account_sign_out,
            commands::account_projects,
            commands::account_avatar,
            commands::account_set_base_url,
            commands::account_open,
            commands::team_status,
            commands::team_enable,
            commands::team_disable,
            commands::team_pending_confirmations,
            commands::team_confirm,
            commands::team_reject,
            commands::team_session_states,
            commands::team_link_repo,
            commands::team_reserve_task,
            commands::team_release_task,
            commands::team_ask_release,
            commands::team_answer_release,
        ])
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Do not let any agent outlive the app.
            handle.state::<subagents::Watchers>().stop_all();
            handle.state::<context_usage::ContextUsage>().stop();
            // Team connections close with a close frame.
            handle.state::<team_service::TeamService>().shutdown();
            // Waiting edits are denied before the agents are ended.
            handle.state::<gate_service::GateService>().stop();
            handle.state::<claims_service::ClaimsService>().stop();
            handle.state::<SessionManager>().shutdown();
        }
    });
    Ok(())
}
