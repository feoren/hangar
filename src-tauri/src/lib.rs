//! Hangar backend: owns all processes, the frontend only renders.

pub mod account;
pub mod account_api;
pub mod account_service;
pub mod agent_catalog;
pub mod agent_commands;
pub mod agent_defs;
pub mod agent_figures;
pub mod agent_models;
pub mod agent_provision;
pub mod app_menu;
pub mod arena;
pub mod avatar;
pub mod batch;
pub mod bounded_output;
pub mod bundled_agents;
pub mod child_env;
pub mod civil;
pub mod claims;
pub mod claims_service;
pub mod cli_session;
pub mod clipboard_image;
pub mod codex_agents;
pub mod codex_shell_env;
pub mod command_guard;
pub mod command_split;
mod commands;
pub mod context_usage;
pub mod cost_service;
pub mod daily_review;
pub mod desktop;
pub mod dictation;
pub mod exe_image;
pub mod exec_alias;
pub mod exit_marker;
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
pub mod git_snapshots;
pub mod gpu;
pub mod hook_http;
pub mod hook_recipes;
pub mod icon;
pub mod install_source;
pub mod instructions;
pub mod launcher_badge;
pub mod link_memory;
pub mod link_service;
pub mod links;
pub mod login_path;
pub mod main_store;
pub mod main_window;
pub mod manager;
pub mod md_agents;
pub mod model_prices;
pub mod no_links;
pub mod notes;
pub mod notify;
pub mod output;
pub mod panel_window;
pub mod partner_inbox;
pub mod partner_service;
pub mod partners;
pub mod paste_image;
pub mod path_links;
pub mod permission_wait;
pub mod persist;
pub mod power;
pub mod proctree;
pub mod profile;
pub mod profile_edit;
pub mod project_notes;
pub mod push;
#[cfg(unix)]
pub mod quit_signal;
pub mod repo_hooks;
pub mod report;
pub mod resolve;
pub mod resources;
pub mod secret_scan;
pub mod secrets;
pub mod send_bin;
pub mod server_check;
pub mod session;
pub mod single_instance;
pub mod subagent_counts;
pub mod subagents;
pub mod system_usage;
pub mod team;
pub mod team_decisions;
pub mod team_engine;
pub mod team_relay;
pub mod team_service;
pub mod team_share;
pub mod team_tasks;
pub mod telemetry;
pub mod telemetry_catalog;
pub mod telemetry_service;
#[cfg(test)]
mod test_support;
pub mod text;
pub mod token_usage;
pub mod tray;
pub mod unattended;
pub mod updates;
pub mod version;
pub mod work_log;
pub mod worktree;
pub mod worktree_cleanup;
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
    // First, while the process has a single thread (`child_env::scrub_own_environment`).
    child_env::scrub_own_environment();
    // Still before any other thread: a desktop start lacks the PATH of the user's dotfiles.
    #[cfg(unix)]
    login_path::adopt();
    let mut builder = tauri::Builder::default();
    // First of all plugins, so a second start exits before anything else runs; debug builds are
    // exempt (see `single_instance::ENFORCED`).
    if single_instance::ENFORCED {
        builder = builder.plugin(single_instance::plugin());
    }
    // Cmd+Q quits through the window, like closing it (app_menu.rs).
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .menu(app_menu::build)
            .on_menu_event(app_menu::on_menu_event);
    }
    let app = builder
        // Places the hidden main window where it was and shows it (main_window.rs).
        .plugin(main_window::plugin())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .on_window_event(tray::on_window_event)
        .setup(|app| {
            // First, before anything that could crash: the marker tells the next start whether
            // this run quit cleanly (exit_marker.rs). A second release instance never gets here,
            // the single instance plugin ends it before `setup`.
            let marker_dir = app.path().app_local_data_dir();
            if let Err(error) = &marker_dir {
                eprintln!("hangar: exit marker not used: {error}");
            }
            let marker =
                std::sync::Arc::new(exit_marker::ExitMarker::arm(marker_dir.ok().as_deref()));
            // Quit paths without an `Exit` event still clear the resource table.
            app.resources_table()
                .add(exit_marker::DisarmOnCleanup(marker.clone()));
            app.manage(marker);
            // Shutdown, logoff and service managers send SIGTERM or SIGHUP: quit as after Quit, so
            // the `Exit` handler removes the marker and ends the sessions in order.
            #[cfg(unix)]
            {
                let quitting = app.handle().clone();
                quit_signal::watch(move |_| quitting.exit(0));
            }
            let home = app.path().home_dir()?;
            let config_dir = app.path().app_config_dir()?;
            let profile_path = config_dir.join(profile::FILE_NAME);
            // The command guard's rules sit next to the profiles: the user edits both.
            let guard_file = config_dir.join(command_guard::FILE_NAME);
            let config_dir_for_figures = config_dir.clone();
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
                // A signal without payload: Tauri also hands events sent to one window to
                // listeners of other webviews that listen to any target, so the account view
                // stays behind `account_status`, which only the main window may call.
                std::sync::Arc::new(move |_: &account::AccountView| {
                    let _ = events.emit_to(panel_window::MAIN_LABEL, account::EVENT, ());
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
                // A signal without payload, as for the account: the snapshot (teammates, safety
                // numbers, tasks) comes through `team_snapshot`, a main window command.
                std::sync::Arc::new(move |snapshot: &team::TeamSnapshot| {
                    tray::set_connected(&events, snapshot.any_connected());
                    let _ = events.emit_to(panel_window::MAIN_LABEL, team::EVENT, ());
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
            // Figures of the sub-agent definitions: the choice next to the profiles, own images in
            // the app's data folder (never in the user's .claude folder).
            app.manage(std::sync::Arc::new(agent_figures::FigureStore::new(
                &config_dir_for_figures,
                &app.path().app_data_dir()?,
            )));
            app.manage(git_snapshots::Snapshots::default());
            app.manage(file_viewer::Viewers::default());
            app.manage(notify::Notifier::default());
            app.manage(launcher_badge::LauncherBadge::default());
            app.manage(resources::ResourceMonitor::default());
            app.manage(system_usage::SystemMonitor::default());
            // Battery or AC for the power saving mode: one cheap reading every 30 s, events only
            // on a change.
            let power = power::PowerMonitor::default();
            let events = app.handle().clone();
            let started = power.start(power::read, move |state| {
                let _ = events.emit_to(panel_window::MAIN_LABEL, power::EVENT, state);
            });
            if let Err(error) = started {
                eprintln!("hangar: power state not watched: {error}");
            }
            app.manage(power);
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
            // Running sub-agents on every session's row: one thread for all sessions, counts only.
            let counts = subagent_counts::SubAgentCounts::default();
            let sessions = app.handle().clone();
            let events = app.handle().clone();
            let started = counts.start(
                move || sessions.state::<SessionManager>().context_targets(),
                move |snapshot| {
                    let _ =
                        events.emit_to(panel_window::MAIN_LABEL, subagent_counts::EVENT, snapshot);
                },
            );
            if let Err(error) = started {
                eprintln!("hangar: sub-agent counts not started: {error}");
            }
            app.manage(counts);
            // Tokens and estimated cost per session and local day: one thread for all sessions,
            // numbers, titles and program names only. Prices: built in, plus the user's
            // model-prices.toml next to profiles.toml.
            let costs = cost_service::CostService::default();
            let (prices, price_error) =
                model_prices::PriceTable::load(&app.state::<ProfileLoad>().dir);
            if let Some(error) = price_error {
                eprintln!("hangar: {error}");
            }
            let ledger = app
                .path()
                .app_local_data_dir()?
                .join(cost_service::LEDGER_FILE);
            let sessions = app.handle().clone();
            let events = app.handle().clone();
            let started = costs.start(
                ledger,
                prices,
                move || sessions.state::<SessionManager>().cost_targets(),
                // A signal without payload: any window may listen, and the report carries session
                // titles; the main window reads it with `cost_report`.
                move |_report| {
                    let _ = events.emit_to(panel_window::MAIN_LABEL, cost_service::EVENT, ());
                },
            );
            if let Err(error) = started {
                eprintln!("hangar: cost overview not started: {error}");
            }
            app.manage(costs);
            // Local rather than roaming: scrollback is large, may contain secrets and belongs to
            // this machine (Windows: %LOCALAPPDATA%\<identifier>).
            let local_dir = app.path().app_local_data_dir()?;
            // Local dictation: idle until the window starts a recording; the speech models are
            // large and belong to this machine, hence the local data folder.
            let events = app.handle().clone();
            app.manage(dictation::DictationService::new(
                local_dir.join("models"),
                std::sync::Arc::new(dictation::audio::CpalSource),
                std::sync::Arc::new(dictation::whisper::Whisper),
                // Signals only, like the gate's: a listener in any window receives events emitted
                // to the main one. The main window reads the state and the dictated text through
                // `dictation_status` and `dictation_take_text`; the level is a bare number.
                std::sync::Arc::new(move |notice| {
                    let main = panel_window::MAIN_LABEL;
                    let _ = match notice {
                        dictation::Notice::Changed => {
                            events.emit_to(main, dictation::STATUS_EVENT, ())
                        }
                        dictation::Notice::Level(level) => {
                            events.emit_to(main, dictation::LEVEL_EVENT, level)
                        }
                        dictation::Notice::TextReady => {
                            events.emit_to(main, dictation::TEXT_EVENT, ())
                        }
                    };
                }),
            ));
            let notes = notes::NotesStore::new(&local_dir);
            notes.migrate_legacy_layout();
            app.manage(notes);
            // Shared project notes (ADR 0006): key rings in the keychain, trash next to the
            // private notes.
            let shared_notes = project_notes::ProjectNotes::new(
                &local_dir,
                std::sync::Arc::new(secrets::KeyringStore::with_service(
                    project_notes::KEYCHAIN_SERVICE,
                )),
            );
            // Team vaults take their key ring from the team (ADR 0006, phase 2).
            shared_notes.set_team(std::sync::Arc::new(
                app.state::<team_service::TeamService>().inner().clone(),
            ));
            app.manage(shared_notes);
            // Decisions (ADR 0021): a team's first notes key ring is made only where this
            // computer kept no keys of it.
            let probe = app.handle().clone();
            app.state::<team_service::TeamService>()
                .set_notes_cache_probe(move |key| {
                    probe
                        .state::<project_notes::ProjectNotes>()
                        .has_team_cache(key)
                });
            // Hook settings for cmd.exe launchers of gated agents; they hold no secret.
            let gate_dir = local_dir.join("gate");
            // The person's gate rules: local, they name this machine's
            // repositories.
            let gate_rules_file = local_dir.join(gate_rules::FILE_NAME);
            // Work log for the daily review (ADR 0011): local, it names this machine's
            // folders. Old days are deleted off the main thread.
            let work_log = work_log::WorkLog::new(local_dir.join(work_log::DIR));
            let pruned = work_log.clone();
            std::thread::spawn(move || pruned.prune(work_log::now_ms()));
            app.manage(work_log);
            // The security switches and queued messages: only the main window reads and writes
            // them, unlike `localStorage`, which the panel and viewer windows share.
            let main_store = main_store::MainStore::new(local_dir.clone());
            // The default model of every sub-agent per program, for every session started from
            // now on; then the copies of the user's definitions in the other installed programs
            // are brought up to date (agent_provision.rs), off the main thread.
            let sub_agents = main_store.load_sub_agents();
            app.state::<SessionManager>()
                .set_sub_agent_models(sub_agents.models.clone());
            let provision = app.handle().clone();
            // No session runs yet: the stored start places are the projects known so far.
            let prefs = main_store.load_prefs().unwrap_or_default();
            let start_places: Vec<std::path::PathBuf> = prefs
                .favorites
                .iter()
                .chain(&prefs.recent)
                .map(std::path::PathBuf::from)
                .collect();
            let bundled_store = main_store.clone();
            std::thread::spawn(move || {
                let manager = provision.state::<SessionManager>();
                let installed = manager.installed_programs();
                // Hangar's own sub-agents first: the copies below take them along.
                bundled_agents::provide_for(manager.home(), &installed, &bundled_store);
                let bundled =
                    bundled_agents::unchanged(manager.home(), &bundled_store.bundled_manifest());
                agent_provision::sync_installed(
                    manager.home(),
                    &start_places,
                    &installed,
                    &sub_agents.provide_off,
                    &bundled,
                );
            });
            app.manage(main_store);
            // Snapshots of sessions that will not come back, side repositories of repositories
            // that are gone, leftover temporary indexes (git_snapshots.rs). Other snapshot calls
            // wait until this is done.
            let keep = persist::Store::new(local_dir.clone()).workspace_keys();
            // The sessions the window restores; only they can bring a remembered link group back.
            let link_keep = keep.clone().unwrap_or_default();
            let folder = local_dir.join(git_snapshots::FOLDER);
            let upkeep = app.handle().clone();
            std::thread::spawn(move || {
                let Ok(git) = git::Git::locate() else {
                    return;
                };
                let env = git_snapshots::SnapshotEnv::new(&git, &folder);
                upkeep
                    .state::<git_snapshots::Snapshots>()
                    .clean_up(&env, keep.as_ref());
            });
            app.manage(persist::Store::new(local_dir));
            app.manage(panel_window::PanelScope::default());
            app.manage(commands::UpdateResults::default());
            app.manage(worktree_cleanup::PendingWorktrees::default());
            // Local claims: the worker stays idle until the window turns the setting on.
            let claims = claims_service::ClaimsService::default();
            let sessions = app.handle().clone();
            let events = app.handle().clone();
            let started = claims.start(
                move || claims_service::watched_sessions(&sessions.state::<SessionManager>()),
                // Only a signal, as for the gate: the overlaps name files and sessions, and a
                // listener in any window receives events emitted to the main one. The main
                // window reads `claims_snapshot`, which only it may call.
                move |_snapshot| {
                    let _ = events.emit_to(panel_window::MAIN_LABEL, claims_service::EVENT, ());
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
                // Only a signal: a listener in any window receives events emitted to the main
                // one, and the held edits name files and commands. The main window reads the
                // snapshot through `gate_snapshot`, which only it may call.
                move |_snapshot| {
                    let _ = events.emit_to(panel_window::MAIN_LABEL, gate_service::EVENT, ());
                },
            );
            // Dialog notices (ADR 0016): a signal only, like the gate's; the main window reads the
            // waiting sessions through `permission_waiting`.
            let events = app.handle().clone();
            gate.set_permission_signal(move || {
                let _ =
                    events.emit_to(panel_window::MAIN_LABEL, gate_service::PERMISSION_EVENT, ());
            });
            gate.load_rules(gate_rules_file);
            gate.load_command_guard(guard_file);
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
            // Decisions of the team (ADR 0021): the agent's context lists them first.
            let decided = app.handle().clone();
            gate.set_team_decisions(move |repo, folder| {
                decided
                    .state::<team_service::TeamService>()
                    .team_decisions(repo, folder)
            });
            // Cross-project messages (ADR 0012): agents of projects with partners send with
            // `hangar-send` through the same endpoint, private sessions included; each message
            // waits for the person in the partner project's session row. Only a signal, like the
            // gate's: the texts come from `waiting_messages`, a main window command.
            let partner_store = app.state::<main_store::MainStore>().inner().clone();
            let events = app.handle().clone();
            let inbox = partner_service::PartnerInbox::load(partner_store.clone(), move || {
                let _ = events.emit_to(panel_window::MAIN_LABEL, partner_service::EVENT, ());
            });
            // Link groups (ADR 0014): linked agent sessions message each other through the same
            // route, the target looked up among the group's members first. Only a signal again:
            // the texts come from `link_state`, a main window command.
            let events = app.handle().clone();
            let links = link_service::LinkService::new(move || {
                let _ = events.emit_to(panel_window::MAIN_LABEL, link_service::EVENT, ());
            });
            // The groups of the last run come back as offers only; the person confirms each
            // (`link_offers`, `apply_link_offer`).
            links.load_offers(link_memory::prune(
                partner_store.load_link_groups(),
                &link_keep,
            ));
            let asked = links.clone();
            gate.set_members(move |from| link_service::answer_members(&asked, from));
            let receiver = inbox.clone();
            let linked = links.clone();
            let sessions = app.handle().clone();
            let book_store = partner_store.clone();
            gate.set_messages(move |from, request| {
                let manager = sessions.state::<SessionManager>();
                let shells = manager.shell_profiles();
                link_service::answer_send(
                    &linked,
                    &receiver,
                    &book_store.load_partners(),
                    &manager.list(),
                    from,
                    &request,
                    &partner_service::Probe {
                        root: &partner_service::project_root,
                        is_shell: &|profile| shells.contains(profile),
                    },
                    work_log::now_ms(),
                )
            });
            app.manage(inbox);
            // The client goes into a folder of its own for the sessions' PATH (send_bin.rs).
            let bin_dir = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
                .and_then(|exe_dir| {
                    send_bin::install(&exe_dir, &app.path().app_local_data_dir().ok()?)
                        .unwrap_or_else(|error| {
                            eprintln!("hangar: hangar-send not set up: {error}");
                            None
                        })
                });
            app.state::<SessionManager>()
                .set_launch_send(std::sync::Arc::new(
                    partner_service::PartnerLaunch::new(gate.clone(), partner_store, bin_dir)
                        .with_links(links.clone()),
                ));
            app.manage(links);
            app.manage(gate);
            // Anonymous usage statistics (ADR 0019): off until the person answers the window's
            // one question; the thread then counts session minutes and sends yesterday's report.
            let transport: std::sync::Arc<dyn telemetry_service::Transport> =
                match telemetry_service::HttpTransport::new() {
                    Ok(transport) => std::sync::Arc::new(transport),
                    Err(error) => {
                        eprintln!("hangar: usage statistics cannot send: {error}");
                        std::sync::Arc::new(telemetry_service::NoTransport)
                    }
                };
            let telemetry = telemetry_service::TelemetryService::new(
                telemetry_service::Options::system(app.path().app_local_data_dir()?),
                transport,
            );
            let sessions = app.handle().clone();
            let accounts = app.handle().clone();
            let started = telemetry.start(
                move || sessions.state::<SessionManager>().running_programs(),
                move || {
                    let signed_in = matches!(
                        accounts
                            .state::<account_service::AccountService>()
                            .view()
                            .state,
                        account::AccountState::SignedIn { .. }
                    );
                    let shared = accounts
                        .state::<team_service::TeamService>()
                        .snapshot()
                        .teams
                        .len();
                    telemetry::Team {
                        signed_in,
                        shared_projects: u32::try_from(shared).unwrap_or(u32::MAX),
                    }
                },
            );
            if let Err(error) = started {
                eprintln!("hangar: usage statistics not started: {error}");
            }
            app.manage(telemetry);
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
            commands::last_exit_was_clean,
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
            commands::shared_notes_team,
            commands::shared_notes_team_enable,
            commands::shared_notes_to_team,
            commands::shared_notes_link,
            commands::shared_notes_copy_code,
            commands::shared_notes_cancel,
            commands::open_panel_window,
            commands::set_panel_on_top,
            commands::close_panel_window,
            commands::destroy_panel_window,
            commands::panel_to_main,
            commands::check_program_updates,
            commands::create_update_session,
            commands::notify_session,
            commands::session_resources,
            commands::system_usage,
            commands::power_state,
            commands::set_launcher_count,
            commands::push_settings,
            commands::push_save,
            commands::push_test,
            commands::push_session,
            commands::git_branches,
            commands::git_repo_places,
            commands::git_diff,
            commands::git_changed_count,
            commands::git_activity,
            commands::log_work_phase,
            commands::daily_review,
            commands::git_log,
            commands::git_commit_diff,
            commands::git_stage,
            commands::git_unstage,
            commands::git_discard,
            commands::git_commit,
            commands::snapshot_bind,
            commands::snapshot_take,
            commands::snapshot_list,
            commands::snapshot_diff,
            commands::snapshot_restore,
            commands::snapshot_forget,
            commands::list_session_folder,
            commands::list_session_paths,
            commands::reveal_session_folder,
            commands::open_local_url,
            commands::stat_paths,
            commands::open_path,
            commands::open_web_url,
            commands::open_file_viewer,
            commands::file_viewer_content,
            commands::file_viewer_stamp,
            commands::file_viewer_diff,
            commands::file_viewer_open_link,
            commands::create_worktree,
            commands::remove_worktree,
            commands::list_hangar_worktrees,
            commands::hangar_worktree_size,
            commands::remove_hangar_worktree,
            commands::prune_hangar_worktree,
            commands::arena_create,
            commands::arena_diff_stat,
            commands::arena_apply,
            commands::arena_discard,
            commands::arena_run_tests,
            commands::configure_claims,
            commands::claims_snapshot,
            commands::configure_gate,
            commands::gate_snapshot,
            commands::permission_waiting,
            commands::permission_answered,
            commands::answer_gate,
            commands::gate_rules,
            commands::delete_gate_rule,
            commands::configure_command_guard,
            commands::command_guard_info,
            commands::save_profile,
            commands::delete_profile,
            commands::profile_env,
            commands::set_profile_secret,
            commands::delete_profile_secret,
            commands::profile_secrets,
            commands::export_session,
            commands::export_report,
            commands::paste_clipboard_image,
            commands::configure_tray,
            commands::watch_sub_agents,
            commands::focus_sub_agent,
            commands::unwatch_sub_agents,
            commands::context_usage,
            commands::sub_agent_counts,
            commands::cost_report,
            commands::account_status,
            commands::account_sign_in,
            commands::account_cancel_sign_in,
            commands::account_sign_out,
            commands::account_projects,
            commands::account_avatar,
            commands::account_set_base_url,
            commands::account_open,
            commands::account_check_server,
            commands::team_status,
            commands::team_enable,
            commands::team_disable,
            commands::team_pending_confirmations,
            commands::team_confirm,
            commands::team_reject,
            commands::team_snapshot,
            commands::team_confirm_admission,
            commands::team_remove_device,
            commands::team_session_states,
            commands::team_link_repo,
            commands::team_reserve_task,
            commands::team_release_task,
            commands::team_ask_release,
            commands::team_answer_release,
            commands::team_record_decision,
            commands::team_withdraw_decision,
            commands::security_switches,
            commands::save_security_switches,
            commands::load_message_queue,
            commands::save_message_queue,
            commands::protected_settings,
            commands::save_protected_settings,
            commands::main_prefs,
            commands::save_main_prefs,
            commands::load_templates,
            commands::save_templates,
            commands::load_recently_closed,
            commands::save_recently_closed,
            commands::partner_projects,
            commands::link_partner,
            commands::unlink_partner,
            commands::partner_targets,
            commands::deliver_partner_message,
            commands::waiting_messages,
            commands::waiting_message_text,
            commands::close_waiting_message,
            commands::link_state,
            commands::link_sessions,
            commands::add_to_link_group,
            commands::unlink_group,
            commands::edit_link_group,
            commands::link_message_text,
            commands::hold_link_message,
            commands::link_person_typed,
            commands::close_link_message,
            commands::link_history,
            commands::resume_link_group,
            commands::merge_link_groups,
            commands::link_offers,
            commands::apply_link_offer,
            commands::discard_link_offer,
            commands::set_panel_session,
            commands::take_update_results,
            commands::dictation_status,
            commands::dictation_start,
            commands::dictation_stop,
            commands::dictation_take_text,
            commands::dictation_install_model,
            commands::dictation_remove_model,
            commands::sub_agent_library,
            commands::create_sub_agent,
            commands::sync_sub_agents,
            commands::set_sub_agent_model,
            commands::sub_agent_settings,
            commands::save_sub_agent_settings,
            commands::delete_sub_agent,
            commands::open_sub_agent,
            commands::sub_agent_figures,
            commands::set_sub_agent_figure,
            commands::paste_sub_agent_figure,
            commands::telemetry_status,
            commands::telemetry_set_consent,
            commands::telemetry_settings,
            commands::telemetry_count,
            commands::telemetry_open_info,
        ])
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
        // A click on the Dock icon while the window is hidden (notification area icon on)
        // brings it back, as a click on that icon does.
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = event
        {
            tray::show_main_window(handle);
        }
        if let RunEvent::Exit = event {
            // First: reaching `Exit` is a clean quit. The steps below may take seconds (team
            // close frames, process trees); Windows ends the process during a shutdown or logoff
            // after a few seconds, which must not leave the marker for a false crash notice.
            handle
                .state::<std::sync::Arc<exit_marker::ExitMarker>>()
                .disarm();
            // The microphone closes with the app.
            handle.state::<dictation::DictationService>().stop();
            // Do not let any agent outlive the app.
            handle.state::<subagents::Watchers>().stop_all();
            handle.state::<context_usage::ContextUsage>().stop();
            handle.state::<subagent_counts::SubAgentCounts>().stop();
            handle.state::<cost_service::CostService>().stop();
            handle.state::<power::PowerMonitor>().stop();
            handle.state::<telemetry_service::TelemetryService>().stop();
            // Team connections close with a close frame.
            handle.state::<team_service::TeamService>().shutdown();
            // Waiting edits are denied before the agents are ended.
            handle.state::<gate_service::GateService>().stop();
            handle.state::<claims_service::ClaimsService>().stop();
            // The sessions end with the app; their runs end now in the work log.
            handle
                .state::<work_log::WorkLog>()
                .end_all(work_log::now_ms());
            handle.state::<SessionManager>().shutdown();
        }
    });
    Ok(())
}
