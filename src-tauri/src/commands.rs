//! Thin Tauri commands. They only forward to the [`SessionManager`].

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Emitter as _, Manager as _, State};

use crate::agent_catalog::ProfileView;
use crate::desktop;
use crate::export::{self, ExportError, Format};
use crate::git::{self, Git, GitDiff, GitError};
use crate::icon::{self, IconCache};
use crate::install_source::{self, SystemProbe};
use crate::manager::{EventSink, ManagerError, SessionEvent, SessionManager, TermSize};
use crate::notes::{
    FolderInfo, NoteFolder, NoteInfo, NotesError, NotesStore, SearchHit, TrashInfo,
};
use crate::notes_window::{self, NotesMessage, NotesWindowError};
use crate::notify::{self, Notifier};
use crate::persist::{PersistError, Store, Workspace};
use crate::profile::{self, ProfileProblem};
use crate::profile_edit::{self, EditError, ProfileInput};
use crate::secrets::{self, SecretError, SecretStatus};
use crate::session::{SessionId, SessionInfo, SessionKind};
use crate::subagents::{self, SubAgentError, SubAgentEvent, Watchers};
use crate::tray::{self, TrayError, TrayLabels};
use crate::updates::{self, UpdateCache, UpdateInfo};
use crate::version::{self, VersionCache};
use crate::worktree::{self, Removal, Worktree, WorktreeError};

/// Result of loading the profiles at startup. A broken profile file must not keep the app from
/// starting; it shows up as an error in the window instead. A successful reload
/// ([`reload_profiles`]) clears the error.
pub struct ProfileLoad {
    pub error: Mutex<Option<String>>,
    /// Config folder that holds `profiles.toml`.
    pub dir: PathBuf,
    pub path: PathBuf,
    /// Held while Settings reads, changes and writes the file, so two saves do not lose each
    /// other's change. Never held together with another lock.
    pub edit: Mutex<()>,
}

/// Cache for versions and icons, for the lifetime of the app.
#[derive(Default)]
pub struct ProbeCaches {
    versions: VersionCache,
    icons: IconCache,
}

type Manager<'a> = State<'a, SessionManager>;

fn channel_sink(channel: Channel<SessionEvent>) -> impl EventSink {
    move |event| {
        // A send error means the window is gone. Then there is nobody left to tell.
        let _ = channel.send(event);
    }
}

/// Event with the result of an update session ([`updates::UpdateOutcome`]).
pub const UPDATE_RESULT_EVENT: &str = "program-update-result";

/// What an update session needs to report its result: the program, the version before the
/// update (if already known) and the session id once the start returned.
#[derive(Clone)]
struct UpdateWatch {
    profile_id: String,
    before: Option<String>,
    session: Arc<OnceLock<SessionId>>,
}

impl UpdateWatch {
    /// The remembered version only; nothing is started on the calling (main) thread.
    fn new(app: &tauri::AppHandle, profile_id: &str) -> Self {
        let before = app
            .state::<SessionManager>()
            .installed_one(profile_id)
            .and_then(|(agent, program)| {
                version::remembered(&agent, &program, &app.state::<ProbeCaches>().versions)
            })
            .flatten();
        Self {
            profile_id: profile_id.to_owned(),
            before,
            session: Arc::default(),
        }
    }
}

/// Reads the version again after an update session and emits [`UPDATE_RESULT_EVENT`]. On a
/// thread of its own: the version query may take seconds and must not hold up the session's
/// batch thread.
fn report_update(app: tauri::AppHandle, watch: UpdateWatch, code: Option<u32>) {
    let spawned = std::thread::Builder::new()
        .name("update-result".to_owned())
        .spawn(move || {
            let manager = app.state::<SessionManager>();
            let found = manager.installed_one(&watch.profile_id);
            let after = found.as_ref().and_then(|(agent, program)| {
                version::detect(
                    &[(agent.clone(), program.clone())],
                    &app.state::<ProbeCaches>().versions,
                )
                .remove(agent.id())
                .flatten()
            });
            let copies = found.map_or_else(Vec::new, |(agent, _)| {
                install_source::copies(&agent.candidates, &SystemProbe)
            });
            let running = manager.running_profiles().contains(&watch.profile_id);
            let outcome = updates::outcome(
                &watch.profile_id,
                watch.session.get().copied(),
                code,
                watch.before,
                after,
                running,
                copies,
            );
            // Only the main window listens for this.
            let _ = app.emit_to(notes_window::MAIN_LABEL, UPDATE_RESULT_EVENT, outcome);
        });
    if let Err(error) = spawned {
        eprintln!("hangar: update result not checked: {error}");
    }
}

/// For update sessions: forget the remembered versions before sending `exit` to the window. The
/// frontend checks again once the session ends and should then see the new version, even if the
/// launcher script's file stamp stayed the same (npm swaps the package, not `gemini.cmd`). Then
/// the version is read once more and the result goes out as [`UPDATE_RESULT_EVENT`].
fn update_sink(
    app: tauri::AppHandle,
    channel: Channel<SessionEvent>,
    watch: UpdateWatch,
) -> impl EventSink {
    move |event| {
        let exit = match event {
            SessionEvent::Exit { code } => Some(code),
            _ => None,
        };
        if exit.is_some() {
            app.state::<ProbeCaches>().versions.clear();
        }
        let _ = channel.send(event);
        if let Some(code) = exit {
            report_update(app.clone(), watch.clone(), code);
        }
    }
}

/// Async because the PATH search runs on every call and grows with the number of entries times
/// the number of PATH folders. Synchronous commands run on the main thread and would make the
/// window stutter.
/// The first call may also wait for the WSL detection (a few hundred milliseconds), hence on a
/// blocking thread.
#[tauri::command]
pub async fn list_profiles(app: tauri::AppHandle) -> Result<Vec<ProfileView>, String> {
    let error = crate::lock(&app.state::<ProfileLoad>().error).clone();
    if let Some(error) = error {
        return Err(error);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let manager = app.state::<SessionManager>();
        manager.ensure_wsl();
        manager.profiles()
    })
    .await
    .map_err(|error| error.to_string())
}

/// Rereads profiles.toml and returns the new list like [`list_profiles`]. Running sessions
/// stay untouched; on an error the previous state remains in effect, and the error names
/// the location in the file ([`ProfileProblem`]).
#[tauri::command]
pub async fn reload_profiles(app: tauri::AppHandle) -> Result<Vec<ProfileView>, ProfileProblem> {
    tauri::async_runtime::spawn_blocking(move || {
        let load = app.state::<ProfileLoad>();
        let profiles = profile::reload(&load.dir)?;
        let manager = app.state::<SessionManager>();
        manager.set_profiles(profiles);
        // Detected again per reload: a distribution installed meanwhile shows up.
        manager.refresh_wsl();
        *crate::lock(&load.error) = None;
        Ok(manager.profiles())
    })
    .await
    .map_err(|error| ProfileProblem {
        message: error.to_string(),
        line: None,
        column: None,
    })?
}

/// Reads, changes and writes profiles.toml under the edit lock, then loads the file again like
/// [`reload_profiles`]: reading the file afterwards (instead of using the edited list) keeps the
/// app on the newest state even when two edits finish in a different order than they started.
fn edit_profiles(
    app: &tauri::AppHandle,
    change: impl FnOnce(&str) -> Result<profile_edit::Edited, EditError>,
) -> Result<profile_edit::Edited, EditError> {
    let load = app.state::<ProfileLoad>();
    let edited = {
        let _edit = crate::lock(&load.edit);
        profile_edit::edit_file(&load.dir, change)?
    };
    let profiles = profile::load_or_init(&load.dir)?;
    app.state::<SessionManager>().set_profiles(profiles);
    *crate::lock(&load.error) = None;
    Ok(edited)
}

/// Keychain entries of names a profile no longer lists; best effort.
fn drop_secrets(app: &tauri::AppHandle, profile_id: &str, names: &[String]) {
    if !names.is_empty() {
        let store = app.state::<SessionManager>().secret_store();
        secrets::delete_all(store.as_ref(), profile_id, names);
    }
}

/// Inserts or replaces a profile in profiles.toml (Settings) and returns the new list.
#[tauri::command]
pub async fn save_profile(
    app: tauri::AppHandle,
    profile: ProfileInput,
) -> Result<Vec<ProfileView>, EditError> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = app.state::<SessionManager>();
        manager.ensure_wsl();
        let known = manager.ids();
        let edited = edit_profiles(&app, |source| {
            profile_edit::upsert(source, &profile, &known)
        })?;
        drop_secrets(&app, &profile.id, &edited.dropped_secrets);
        Ok(app.state::<SessionManager>().profiles())
    })
    .await
    .map_err(|error| EditError::Task(error.to_string()))?
}

/// Removes a user profile together with its keychain entries; catalog entries cannot be deleted.
#[tauri::command]
pub async fn delete_profile(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<ProfileView>, EditError> {
    tauri::async_runtime::spawn_blocking(move || {
        let edited = edit_profiles(&app, |source| profile_edit::delete(source, &id))?;
        drop_secrets(&app, &id, &edited.dropped_secrets);
        Ok(app.state::<SessionManager>().profiles())
    })
    .await
    .map_err(|error| EditError::Task(error.to_string()))?
}

/// Values of a profile's plain `env`, only for the edit form; empty for catalog entries.
#[tauri::command]
pub fn profile_env(
    manager: Manager<'_>,
    id: String,
) -> Result<BTreeMap<String, String>, ManagerError> {
    if manager.secret_names(&id).is_none() {
        return Err(ManagerError::UnknownProfile(id));
    }
    Ok(manager.user_profile(&id).map(|p| p.env).unwrap_or_default())
}

/// The keychain may take a moment or ask the user (macOS, Linux), hence on a blocking thread.
async fn secrets_blocking<T: Send + 'static>(
    manager: Manager<'_>,
    work: impl FnOnce(&dyn secrets::SecretStore) -> Result<T, SecretError> + Send + 'static,
) -> Result<T, SecretError> {
    let store = manager.secret_store();
    tauri::async_runtime::spawn_blocking(move || work(store.as_ref()))
        .await
        .map_err(|error| SecretError::Task(error.to_string()))?
}

/// Stores a secret variable of a profile in the OS keychain. The value goes nowhere else. Only
/// for a name in the profile's `secret_env`, so the profile must be saved first.
#[tauri::command]
pub async fn set_profile_secret(
    manager: Manager<'_>,
    profile_id: String,
    name: String,
    value: String,
) -> Result<(), SecretError> {
    let value = zeroize::Zeroizing::new(value);
    let listed = manager.secret_names(&profile_id);
    secrets_blocking(manager, move |store| {
        secrets::check_listed(listed.as_deref(), &profile_id, &name)?;
        secrets::set(store, &profile_id, &name, &value)
    })
    .await
}

#[tauri::command]
pub async fn delete_profile_secret(
    manager: Manager<'_>,
    profile_id: String,
    name: String,
) -> Result<(), SecretError> {
    secrets_blocking(manager, move |store| {
        secrets::delete(store, &profile_id, &name)
    })
    .await
}

/// Per `secret_env` name of the profile whether a value is stored; never the value.
#[tauri::command]
pub async fn profile_secrets(
    manager: Manager<'_>,
    profile_id: String,
) -> Result<Vec<SecretStatus>, SecretError> {
    let names = manager
        .secret_names(&profile_id)
        .ok_or_else(|| SecretError::InvalidProfile(profile_id.clone()))?;
    secrets_blocking(manager, move |store| {
        secrets::statuses(store, &profile_id, &names)
    })
    .await
}

/// Opens exactly this machine's profiles.toml in the default editor; if it is missing, it is
/// created from the commented template. Deliberately takes no path argument: the frontend cannot
/// open any other file with it.
#[tauri::command]
pub async fn open_profile_file(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let load = app.state::<ProfileLoad>();
        let path = profile::ensure_file(&load.dir).map_err(|error| error.to_string())?;
        desktop::open(&path, desktop::Target::TextFile)
            .map_err(|error| format!("{} cannot be opened: {error}", path.display()))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// profileId → version or `null`, only for installed entries (ADR 0003). Starts each
/// program at most once per file state, in parallel and with a time limit; hence on a
/// blocking thread, not on the async runtime thread.
#[tauri::command]
pub async fn detect_versions(
    app: tauri::AppHandle,
) -> Result<HashMap<String, Option<String>>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let installed = app.state::<SessionManager>().installed();
        version::detect(&installed, &app.state::<ProbeCaches>().versions)
    })
    .await
    .map_err(|error| error.to_string())
}

/// profileId → `data:image/png;base64,...` or `null`, only for installed entries.
#[tauri::command]
pub async fn program_icons(
    app: tauri::AppHandle,
) -> Result<HashMap<String, Option<String>>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let installed = app.state::<SessionManager>().installed();
        icon::icons(&installed, &app.state::<ProbeCaches>().icons)
    })
    .await
    .map_err(|error| error.to_string())
}

/// profileId → version state, only for installed entries with `latest`. The installed
/// version comes from the `detect_versions` cache, the newest from the disk cache (24 h) or
/// the network; `force` bypasses the disk cache. Errors per entry yield `latest: null`.
#[tauri::command]
pub async fn check_program_updates(
    app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<HashMap<String, UpdateInfo>, String> {
    let handle = app.clone();
    let (items, mut cache, dir) = tauri::async_runtime::spawn_blocking(move || {
        let manager = handle.state::<SessionManager>();
        let installed: Vec<_> = manager
            .installed()
            .into_iter()
            .filter(|(agent, _)| agent.profile.latest.is_some())
            .collect();
        let versions = version::detect(&installed, &handle.state::<ProbeCaches>().versions);
        let running = manager.running_profiles();
        let items = updates::items(&installed, &versions, &running, &SystemProbe);
        let dir = handle.state::<Store>().dir().to_path_buf();
        let cache = UpdateCache::load(&dir);
        (items, cache, dir)
    })
    .await
    .map_err(|error| error.to_string())?;

    let before = cache.clone();
    let result = updates::check(
        &items,
        &mut cache,
        updates::now_millis(),
        force.unwrap_or(false),
    )
    .await;
    if cache != before {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(error) = cache.save(&dir) {
                eprintln!("hangar: update cache not saved: {error}");
            }
        })
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(result)
}

/// Full path of profiles.toml, for the About dialog.
#[tauri::command]
pub fn profile_path(load: State<'_, ProfileLoad>) -> String {
    load.path.display().to_string()
}

#[tauri::command]
pub fn default_cwd(manager: Manager<'_>) -> PathBuf {
    manager.home().to_path_buf()
}

#[tauri::command]
pub fn list_sessions(manager: Manager<'_>) -> Vec<SessionInfo> {
    manager.list()
}

#[tauri::command]
pub fn create_session(
    manager: Manager<'_>,
    profile_id: String,
    cwd: Option<PathBuf>,
    size: TermSize,
    on_event: Channel<SessionEvent>,
    resume: Option<bool>,
    cli_session_id: Option<String>,
) -> Result<SessionInfo, ManagerError> {
    // Optional, so existing calls without `resume` or `cliSessionId` stay valid.
    manager.create_resuming(
        &profile_id,
        cwd,
        size,
        resume.unwrap_or(false),
        cli_session_id.as_deref(),
        channel_sink(on_event),
    )
}

/// Starts a program update as a session of its own. The frontend only names the profile; the
/// command comes solely from the update plan: the profile's `update_hint`, the program's
/// self-updater or its installer's update with the catalog's package name (contract:
/// docs/wiki/pages/syntheses/program-updates.md). Async so the detection (file checks and the
/// uninstall keys) does not run on the main thread.
///
/// `expected_plan`: the `planId` from `check_program_updates` the user saw; if the plan changed
/// since, the start is refused ([`ManagerError::PlanChanged`]). Optional for older frontends.
#[tauri::command]
pub async fn create_update_session(
    app: tauri::AppHandle,
    profile_id: String,
    size: TermSize,
    on_event: Channel<SessionEvent>,
    expected_plan: Option<String>,
) -> Result<SessionInfo, ManagerError> {
    tauri::async_runtime::spawn_blocking(move || {
        let watch = UpdateWatch::new(&app, &profile_id);
        let session = Arc::clone(&watch.session);
        let manager = app.state::<SessionManager>();
        let info = manager.create_update_checked(
            &profile_id,
            expected_plan.as_deref(),
            size,
            update_sink(app.clone(), on_event, watch),
        )?;
        let _ = session.set(info.id);
        Ok(info)
    })
    .await
    .map_err(|error| ManagerError::Task(error.to_string()))?
}

#[tauri::command]
pub fn write_session(
    manager: Manager<'_>,
    id: SessionId,
    data: String,
) -> Result<(), ManagerError> {
    manager.write(id, &data)
}

#[tauri::command]
pub fn resize_session(
    manager: Manager<'_>,
    id: SessionId,
    size: TermSize,
) -> Result<(), ManagerError> {
    manager.resize(id, size)
}

#[tauri::command]
pub fn ack_session(manager: Manager<'_>, id: SessionId, bytes: usize) -> Result<(), ManagerError> {
    manager.ack(id, bytes)
}

#[tauri::command]
pub fn kill_session(manager: Manager<'_>, id: SessionId) -> Result<(), ManagerError> {
    manager.kill(id)
}

#[tauri::command]
pub fn restart_session(
    app: tauri::AppHandle,
    manager: Manager<'_>,
    watchers: State<'_, Watchers>,
    id: SessionId,
    size: TermSize,
    on_event: Channel<SessionEvent>,
) -> Result<SessionInfo, ManagerError> {
    // Restarting an update session repeats the update; after that it behaves like a start.
    let update_profile = manager
        .list()
        .into_iter()
        .find(|info| info.id == id && info.kind == SessionKind::Update)
        .map(|info| info.profile_id);
    let info = if let Some(profile_id) = update_profile {
        let watch = UpdateWatch::new(&app, &profile_id);
        let _ = watch.session.set(id);
        manager.restart(id, size, update_sink(app, on_event, watch))
    } else {
        manager.restart(id, size, channel_sink(on_event))
    }?;
    // The restart runs under a new conversation id; a watch would keep following the old one.
    // The window starts a new watch with the returned `SessionInfo` if it wants one.
    watchers.stop(id);
    Ok(info)
}

#[tauri::command]
pub fn rename_session(
    manager: Manager<'_>,
    id: SessionId,
    title: String,
) -> Result<SessionInfo, ManagerError> {
    manager.rename(id, &title)
}

/// Also withdraws the session's desktop notification, off the main thread, and ends its
/// sub-agent watch.
#[tauri::command]
pub fn close_session(
    app: tauri::AppHandle,
    manager: Manager<'_>,
    watchers: State<'_, Watchers>,
    id: SessionId,
) -> Result<(), ManagerError> {
    manager.close(id)?;
    watchers.stop(id);
    tauri::async_runtime::spawn_blocking(move || app.state::<Notifier>().forget(id));
    Ok(())
}

// Sub-agent view (subagents/). Contract: docs/wiki/pages/syntheses/agent-workflow-features.md, item 23.

/// Streams snapshots of the session's sub-agents on `on_event` until [`unwatch_sub_agents`], the
/// session closes or the window drops the channel. A second call for the same session replaces
/// the first watch. `excerpts: false` (the frontend setting) sends no assistant text at all;
/// missing means `true`.
#[tauri::command]
pub fn watch_sub_agents(
    manager: Manager<'_>,
    watchers: State<'_, Watchers>,
    session_id: SessionId,
    on_event: Channel<SubAgentEvent>,
    excerpts: Option<bool>,
) -> Result<(), SubAgentError> {
    let target = manager.sub_agent_target(session_id)?;
    let source = subagents::source_for(&target, excerpts.unwrap_or(true));
    watchers.start(session_id, source, move |event| {
        on_event.send(event).is_ok()
    })
}

/// `false` if no watch was running for the session.
#[tauri::command]
pub fn unwatch_sub_agents(watchers: State<'_, Watchers>, session_id: SessionId) -> bool {
    watchers.stop(session_id)
}

// Workspace and scrollback (persist.rs). File access runs on a blocking thread because
// synchronous commands run on the main thread and 2 MiB of scrollback would make the window stutter.

async fn blocking<T: Send + 'static>(
    store: State<'_, Store>,
    work: impl FnOnce(&Store) -> Result<T, PersistError> + Send + 'static,
) -> Result<T, PersistError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || work(&store))
        .await
        .map_err(|error| PersistError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn save_workspace(store: State<'_, Store>, state: Workspace) -> Result<(), PersistError> {
    blocking(store, move |s| s.save_workspace(&state)).await
}

/// `null` if nothing is stored or the file is unusable (reason in the log).
#[tauri::command]
pub async fn load_workspace(store: State<'_, Store>) -> Result<Option<Workspace>, PersistError> {
    blocking(store, |s| Ok(s.load_workspace())).await
}

#[tauri::command]
pub async fn save_scrollback(
    store: State<'_, Store>,
    key: String,
    data: String,
) -> Result<(), PersistError> {
    blocking(store, move |s| s.save_scrollback(&key, &data)).await
}

#[tauri::command]
pub async fn load_scrollback(
    store: State<'_, Store>,
    key: String,
) -> Result<Option<String>, PersistError> {
    blocking(store, move |s| s.load_scrollback(&key)).await
}

#[tauri::command]
pub async fn delete_scrollback(store: State<'_, Store>, key: String) -> Result<(), PersistError> {
    blocking(store, move |s| s.delete_scrollback(&key)).await
}

#[tauri::command]
pub async fn clear_scrollbacks(store: State<'_, Store>) -> Result<(), PersistError> {
    blocking(store, Store::clear_scrollbacks).await
}

// Notes per working folder (notes.rs, ADR 0004). Also on a blocking thread.

async fn notes_blocking<T: Send + 'static>(
    store: State<'_, NotesStore>,
    work: impl FnOnce(&NotesStore) -> Result<T, NotesError> + Send + 'static,
) -> Result<T, NotesError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || work(&store))
        .await
        .map_err(|error| NotesError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn list_note_folders(
    store: State<'_, NotesStore>,
) -> Result<Vec<FolderInfo>, NotesError> {
    notes_blocking(store, NotesStore::list_folders).await
}

#[tauri::command]
pub async fn list_notes(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
) -> Result<Vec<NoteInfo>, NotesError> {
    notes_blocking(store, move |s| s.list_notes(&folder)).await
}

#[tauri::command]
pub async fn read_note(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    id: String,
) -> Result<String, NotesError> {
    notes_blocking(store, move |s| s.read_note(&folder, &id)).await
}

#[tauri::command]
pub async fn write_note(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    id: String,
    content: String,
) -> Result<(), NotesError> {
    notes_blocking(store, move |s| s.write_note(&folder, &id, &content)).await
}

#[tauri::command]
pub async fn create_note(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    title: String,
) -> Result<String, NotesError> {
    notes_blocking(store, move |s| s.create_note(&folder, &title)).await
}

#[tauri::command]
pub async fn rename_note(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    id: String,
    title: String,
) -> Result<(), NotesError> {
    notes_blocking(store, move |s| s.rename_note(&folder, &id, &title)).await
}

#[tauri::command]
pub async fn delete_note(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    id: String,
) -> Result<(), NotesError> {
    notes_blocking(store, move |s| s.delete_note(&folder, &id)).await
}

#[tauri::command]
pub async fn reorder_notes(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    ids: Vec<String>,
) -> Result<(), NotesError> {
    notes_blocking(store, move |s| s.reorder_notes(&folder, &ids)).await
}

#[tauri::command]
pub async fn reveal_notes_folder(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
) -> Result<(), NotesError> {
    notes_blocking(store, move |s| s.reveal(&folder)).await
}

/// Case-insensitive substring search across all scopes (no regex), at most
/// [`crate::notes::MAX_SEARCH_RESULTS`] hits.
#[tauri::command]
pub async fn search_notes(
    store: State<'_, NotesStore>,
    query: String,
) -> Result<Vec<SearchHit>, NotesError> {
    notes_blocking(store, move |s| s.search(&query)).await
}

#[tauri::command]
pub async fn list_trash(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
) -> Result<Vec<TrashInfo>, NotesError> {
    notes_blocking(store, move |s| s.list_trash(&folder)).await
}

/// Returns the restored note's ID (the old one unless it is taken again).
#[tauri::command]
pub async fn restore_note(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    id: String,
) -> Result<String, NotesError> {
    notes_blocking(store, move |s| s.restore_note(&folder, &id)).await
}

/// Saves a note as `.md`: the backend opens the save dialog, so the window never chooses a path.
/// `None` when the dialog was cancelled, otherwise the path written.
#[tauri::command]
pub async fn export_note(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    id: String,
) -> Result<Option<String>, NotesError> {
    use tauri_plugin_dialog::DialogExt as _;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (title, content) = store.note_for_export(&folder, &id)?;
        let chosen = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(export::note_file_name(&title))
            .add_filter(export::NOTE_FILTER, &[export::NOTE_EXTENSION])
            .blocking_save_file();
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen.into_path().map_err(|_| ExportError::NotAPath)?;
        export::write(&path, &content)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|error| NotesError::Task(error.to_string()))?
}

// Detached notes window (notes_window.rs).

/// Opens the notes window with its fixed label, URL and size, or brings it to the front if it
/// is already open. Async: creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_notes_window(
    app: tauri::AppHandle,
    always_on_top: bool,
) -> Result<(), NotesWindowError> {
    let focus = |window: tauri::WebviewWindow| -> Result<(), NotesWindowError> {
        let _ = window.unminimize();
        window.show()?;
        window.set_focus()?;
        Ok(())
    };
    if let Some(window) = app.get_webview_window(notes_window::LABEL) {
        return focus(window);
    }
    let built = tauri::WebviewWindowBuilder::new(
        &app,
        notes_window::LABEL,
        tauri::WebviewUrl::App(notes_window::URL.into()),
    )
    .title("Hangar")
    .inner_size(notes_window::WIDTH, notes_window::HEIGHT)
    .min_inner_size(notes_window::MIN_WIDTH, notes_window::MIN_HEIGHT)
    .always_on_top(always_on_top)
    .build();
    match built {
        Ok(_) => Ok(()),
        // Two quick clicks: the other call created it meanwhile.
        Err(error) => match app.get_webview_window(notes_window::LABEL) {
            Some(window) => focus(window),
            None => Err(error.into()),
        },
    }
}

/// Keeps the notes window above other windows or not. Only the notes window calls it, for
/// itself: it holds this command instead of the general `set_always_on_top` permission.
#[tauri::command]
pub fn set_notes_on_top(
    window: tauri::WebviewWindow,
    on_top: bool,
) -> Result<(), NotesWindowError> {
    notes_window::check_sender(window.label())?;
    window.set_always_on_top(on_top)?;
    Ok(())
}

/// The only way from the notes window to the main window: a validated [`NotesMessage`],
/// delivered as event `notes-message` to `main` alone. The notes window may not emit events
/// itself, so it cannot fake events such as `tray-quit`.
#[tauri::command]
pub fn notes_to_main(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    message: NotesMessage,
) -> Result<(), NotesWindowError> {
    notes_window::check_sender(window.label())?;
    message.validate()?;
    if message == (NotesMessage::ShowMain {}) {
        tray::show_main_window(&app);
    }
    app.emit_to(
        notes_window::MAIN_LABEL,
        notes_window::MESSAGE_EVENT,
        &message,
    )?;
    Ok(())
}

/// Shows the note's file selected in Explorer or Finder (Linux: its folder).
#[tauri::command]
pub async fn reveal_note(
    store: State<'_, NotesStore>,
    folder: NoteFolder,
    id: String,
) -> Result<(), NotesError> {
    notes_blocking(store, move |s| s.reveal_note(&folder, &id)).await
}

// Desktop notifications (notify.rs).

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotificationActivated {
    session_id: SessionId,
}

/// Mirrors tauri-plugin-notification: see [`notify::app_id`].
fn notification_app_id(app: &tauri::AppHandle) -> String {
    let identifier = &app.config().identifier;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_default();
    notify::app_id(identifier, &exe_dir).to_owned()
}

/// Brings the main window to the front and tells the frontend which session was meant.
fn activate_session(app: &tauri::AppHandle, id: SessionId) {
    tray::show_main_window(app);
    let _ = app.emit_to(
        notes_window::MAIN_LABEL,
        "notification-activated",
        NotificationActivated { session_id: id },
    );
}

/// `false` when the platform has no support or the toast failed; the reason goes to the log,
/// the texts do not.
#[tauri::command]
pub async fn notify_session(
    app: tauri::AppHandle,
    id: SessionId,
    title: String,
    body: String,
) -> bool {
    tauri::async_runtime::spawn_blocking(move || {
        let app_id = notification_app_id(&app);
        let handle = app.clone();
        let shown = app
            .state::<Notifier>()
            .show(&app_id, id, &title, &body, move || {
                activate_session(&handle, id);
            });
        shown.unwrap_or_else(|error| {
            eprintln!("hangar: notification not shown: {error}");
            false
        })
    })
    .await
    .unwrap_or(false)
}

// Git and worktrees (git.rs, worktree.rs), on a blocking thread because git may take seconds.

/// Branch per folder, read from the files under `.git`; no process.
#[tauri::command]
pub async fn git_branches(paths: Vec<String>) -> Result<HashMap<String, Option<String>>, GitError> {
    tauri::async_runtime::spawn_blocking(move || git::branches(&paths))
        .await
        .map_err(|error| GitError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn git_diff(cwd: PathBuf) -> Result<GitDiff, GitError> {
    tauri::async_runtime::spawn_blocking(move || git::diff(&Git::locate()?, &cwd))
        .await
        .map_err(|error| GitError::Task(error.to_string()))?
}

fn worktrees_dir(store: &Store) -> PathBuf {
    store.dir().join(worktree::DIR_NAME)
}

#[tauri::command]
pub async fn create_worktree(
    store: State<'_, Store>,
    cwd: PathBuf,
    label: String,
) -> Result<Worktree, WorktreeError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        worktree::create(&Git::locate()?, &base, &cwd, &label)
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

/// Never forces: what stays is reported in `reason`.
#[tauri::command]
pub async fn remove_worktree(
    store: State<'_, Store>,
    worktree: Worktree,
) -> Result<Removal, WorktreeError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        worktree::remove(&Git::locate()?, &base, &worktree)
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

// Export (export.rs) and notification area icon (tray.rs).

/// Saves a session's content: the backend opens the save dialog itself, so the window never
/// chooses a path. `None` when the dialog was cancelled.
#[tauri::command]
pub async fn export_session(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    format: Format,
    content: String,
    suggested_name: String,
) -> Result<Option<String>, ExportError> {
    use tauri_plugin_dialog::DialogExt as _;
    export::check_size(&content)?;
    let name = export::file_name(&suggested_name, format);
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(name)
            .add_filter(format.filter_name(), &[format.extension()])
            .blocking_save_file();
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen.into_path().map_err(|_| ExportError::NotAPath)?;
        export::write(&path, &content)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|error| ExportError::Task(error.to_string()))?
}

/// Adds, updates or removes the notification area icon. Async so the menu is built while the
/// main thread is free (Tauri hands that work to it).
#[tauri::command]
pub async fn configure_tray(
    app: tauri::AppHandle,
    enabled: bool,
    labels: TrayLabels,
) -> Result<(), TrayError> {
    tray::configure(&app, enabled, &labels)
}
