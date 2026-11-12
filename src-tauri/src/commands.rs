//! Thin Tauri commands. They only forward to the [`SessionManager`].

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Emitter as _, Manager as _, State};

use crate::account::{AccountError, AccountPage, AccountView, Project};
use crate::account_service::AccountService;
use crate::agent_catalog::ProfileView;
use crate::claims::ClaimsSnapshot;
use crate::claims_service::ClaimsService;
use crate::clipboard_image;
use crate::desktop;
use crate::export::{self, ExportError, Format};
use crate::file_tree::{self, FileTree, FileTreeError, FolderListing, PathIndex};
use crate::file_viewer::{
    self, Claim, FileContent, FileStamp, FileViewerError, HeadDiff, Target, Viewers,
};
use crate::gate::{Answer, GateSnapshot};
use crate::gate_service::GateService;
use crate::git::{self, Git, GitDiff, GitError};
use crate::git_activity::{self, ActivityError, GitActivity};
use crate::git_history::{self, GitCommitDiff, GitLog};
use crate::icon::{self, IconCache};
use crate::install_source::{self, SystemProbe};
use crate::manager::{EventSink, ManagerError, SessionEvent, SessionManager, TermSize};
use crate::notes::{
    FolderInfo, NoteFolder, NoteInfo, NotesError, NotesStore, SearchHit, TrashInfo,
};
use crate::notify::{self, Notifier};
use crate::panel_window::{self, PanelMessage, PanelWindowError};
use crate::paste_image::{self, PasteGate, PasteImageError, PasteOutcome};
use crate::persist::{PersistError, Store, Workspace};
use crate::profile::{self, ProfileProblem};
use crate::profile_edit::{self, EditError, ProfileInput};
use crate::project_notes::{self, ProjectNotes, ProjectNotesError, SharedStatus};
use crate::secrets::{self, SecretError, SecretStatus};
use crate::session::{SessionId, SessionInfo, SessionKind, SessionMode};
use crate::subagents::{self, SubAgentError, SubAgentEvent, Watchers};
use crate::team::{PendingConfirmation, TeamError, TeamStatus};
use crate::team_service::TeamService;
use crate::team_share::SessionState;
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
                query_versions(&app, &[(agent.clone(), program.clone())], false)
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
            let _ = app.emit_to(panel_window::MAIN_LABEL, UPDATE_RESULT_EVENT, outcome);
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

/// The version query, run in Hangar's own probe folder in the app data folder
/// ([`version::probe_folder`]). Without that folder nothing is started and every version is
/// unknown. `refresh` asks the programs again even when a version is remembered, except those an
/// update session is replacing right now.
fn query_versions(
    app: &tauri::AppHandle,
    installed: &[(crate::agent_catalog::Agent, PathBuf)],
    refresh: bool,
) -> HashMap<String, Option<String>> {
    let cache = &app.state::<ProbeCaches>().versions;
    match version::probe_folder(app.state::<Store>().dir()) {
        Ok(cwd) if refresh => {
            let updating = app.state::<SessionManager>().updating_profiles();
            version::redetect(installed, cache, &cwd, &updating)
        }
        Ok(cwd) => version::detect(installed, cache, &cwd),
        Err(error) => {
            eprintln!("hangar: no folder for the version query: {error}");
            installed
                .iter()
                .map(|(agent, _)| (agent.id().to_owned(), None))
                .collect()
        }
    }
}

/// The version query (with its cache) for the agent gate's minimum versions
/// ([`crate::manager::VersionSource`]).
pub struct AppVersions(pub tauri::AppHandle);

impl crate::manager::VersionSource for AppVersions {
    fn versions(
        &self,
        installed: &[(crate::agent_catalog::Agent, PathBuf)],
    ) -> HashMap<String, Option<String>> {
        query_versions(&self.0, installed, false)
    }
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
        query_versions(&app, &installed, false)
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
/// version comes from the `detect_versions` cache, the newest from the disk cache (25 min) or
/// the network; `force` bypasses the disk cache (not a rate limit). `redetect` asks these
/// programs for their version again, so an update made outside Hangar (an agent updating
/// itself) is seen; the periodic check sets it. Errors per entry yield `latest: null`.
#[tauri::command]
pub async fn check_program_updates(
    app: tauri::AppHandle,
    force: Option<bool>,
    redetect: Option<bool>,
) -> Result<HashMap<String, UpdateInfo>, String> {
    let handle = app.clone();
    let (items, mut cache, dir) = tauri::async_runtime::spawn_blocking(move || {
        let manager = handle.state::<SessionManager>();
        let installed: Vec<_> = manager
            .installed()
            .into_iter()
            .filter(|(agent, _)| agent.profile.latest.is_some())
            .collect();
        let versions = query_versions(&handle, &installed, redetect.unwrap_or(false));
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

/// `mode`: private or team session (ADR 0008); without one, a private session, which never
/// gets the agent gate and never reaches the team.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // One Tauri command, one argument per field of the call.
pub fn create_session(
    manager: Manager<'_>,
    profile_id: String,
    cwd: Option<PathBuf>,
    size: TermSize,
    on_event: Channel<SessionEvent>,
    resume: Option<bool>,
    cli_session_id: Option<String>,
    mode: Option<SessionMode>,
) -> Result<SessionInfo, ManagerError> {
    // Optional, so existing calls without `resume`, `cliSessionId` or `mode` stay valid.
    manager.create_resuming(
        &profile_id,
        cwd,
        size,
        resume.unwrap_or(false),
        cli_session_id.as_deref(),
        mode.unwrap_or_default(),
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
/// missing means `true`. `focus` names the one sub-agent whose activity log is streamed too
/// (missing: none); later changes go through [`focus_sub_agent`].
#[tauri::command]
pub fn watch_sub_agents(
    manager: Manager<'_>,
    watchers: State<'_, Watchers>,
    session_id: SessionId,
    on_event: Channel<SubAgentEvent>,
    excerpts: Option<bool>,
    focus: Option<String>,
) -> Result<(), SubAgentError> {
    let focus = subagents::checked_focus(focus)?;
    let target = manager.sub_agent_target(session_id)?;
    let mut source = subagents::source_for(&target, excerpts.unwrap_or(true));
    source.focus(focus);
    watchers.start(session_id, source, move |event| {
        on_event.send(event).is_ok()
    })
}

/// Switches the running watch's activity log to `agentId` (`null`: no log). `false` if no
/// watch runs for the session; an invalid id is refused.
#[tauri::command]
pub fn focus_sub_agent(
    watchers: State<'_, Watchers>,
    session_id: SessionId,
    agent_id: Option<String>,
) -> Result<bool, SubAgentError> {
    let agent = subagents::checked_focus(agent_id)?;
    Ok(watchers.focus(session_id, agent))
}

/// `false` if no watch was running for the session.
#[tauri::command]
pub fn unwatch_sub_agents(watchers: State<'_, Watchers>, session_id: SessionId) -> bool {
    watchers.stop(session_id)
}

/// The newest context use of the sessions (context_usage.rs); later changes arrive as
/// `context-usage-changed` events with the same shape.
#[tauri::command]
pub fn context_usage(
    usage: State<'_, crate::context_usage::ContextUsage>,
) -> crate::context_usage::ContextSnapshot {
    usage.snapshot()
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

// Notes per working folder (notes.rs, ADR 0004) and shared project notes (project_notes.rs,
// ADR 0006). `folder.shared` picks the store; both run on a blocking thread.

/// Both note stores, for one blocking call.
#[derive(Clone)]
pub struct Notes {
    pub private: NotesStore,
    pub shared: ProjectNotes,
}

impl Notes {
    fn dispatch<T>(
        &self,
        folder: &NoteFolder,
        private: impl FnOnce(&NotesStore) -> Result<T, NotesError>,
        shared: impl FnOnce(&ProjectNotes) -> Result<T, ProjectNotesError>,
    ) -> Result<T, NotesError> {
        if folder.shared {
            Ok(shared(&self.shared)?)
        } else {
            private(&self.private)
        }
    }
}

async fn notes_blocking<T: Send + 'static>(
    private: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    work: impl FnOnce(&Notes) -> Result<T, NotesError> + Send + 'static,
) -> Result<T, NotesError> {
    let notes = Notes {
        private: private.inner().clone(),
        shared: shared.inner().clone(),
    };
    tauri::async_runtime::spawn_blocking(move || work(&notes))
        .await
        .map_err(|error| NotesError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn list_note_folders(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
) -> Result<Vec<FolderInfo>, NotesError> {
    notes_blocking(store, shared, |n| n.private.list_folders()).await
}

#[tauri::command]
pub async fn list_notes(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<Vec<NoteInfo>, NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.list_notes(&folder),
            |p| p.list_notes(&folder),
        )
    })
    .await
}

#[tauri::command]
pub async fn read_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<String, NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.read_note(&folder, &id),
            |p| p.read_note(&folder, &id),
        )
    })
    .await
}

#[tauri::command]
pub async fn write_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
    content: String,
) -> Result<(), NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.write_note(&folder, &id, &content),
            |p| p.write_note(&folder, &id, &content),
        )
    })
    .await
}

#[tauri::command]
pub async fn create_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    title: String,
) -> Result<String, NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.create_note(&folder, &title),
            |p| p.create_note(&folder, &title),
        )
    })
    .await
}

#[tauri::command]
pub async fn rename_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
    title: String,
) -> Result<(), NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.rename_note(&folder, &id, &title),
            |p| p.rename_note(&folder, &id, &title),
        )
    })
    .await
}

#[tauri::command]
pub async fn delete_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<(), NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.delete_note(&folder, &id),
            |p| p.delete_note(&folder, &id),
        )
    })
    .await
}

/// Shared notes sort by their last change and have no manual order: a no-op for them.
#[tauri::command]
pub async fn reorder_notes(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    ids: Vec<String>,
) -> Result<(), NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(&folder, |s| s.reorder_notes(&folder, &ids), |_| Ok(()))
    })
    .await
}

#[tauri::command]
pub async fn reveal_notes_folder(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<(), NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.reveal(&folder),
            |_| Err(ProjectNotesError::NoFile),
        )
    })
    .await
}

/// Case-insensitive substring search across all scopes (no regex), at most
/// [`crate::notes::MAX_SEARCH_RESULTS`] hits; shared notes of every project unlocked in this
/// run are decrypted in memory and included.
#[tauri::command]
pub async fn search_notes(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    query: String,
) -> Result<Vec<SearchHit>, NotesError> {
    notes_blocking(store, shared, move |n| {
        let private = n.private.search(&query)?;
        let shared = n.shared.search(&query)?;
        Ok(crate::notes::merge_hits(&query, private, shared))
    })
    .await
}

#[tauri::command]
pub async fn list_trash(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<Vec<TrashInfo>, NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.list_trash(&folder),
            |p| p.list_trash(&folder),
        )
    })
    .await
}

/// Returns the restored note's ID (the old one unless it is taken again).
#[tauri::command]
pub async fn restore_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<String, NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.restore_note(&folder, &id),
            |p| p.restore_note(&folder, &id),
        )
    })
    .await
}

/// Moves a note between the private notes of a folder and its shared notes. Returns the new ID.
#[tauri::command]
pub async fn move_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    from: NoteFolder,
    id: String,
    to: NoteFolder,
) -> Result<String, NotesError> {
    notes_blocking(store, shared, move |n| {
        project_notes::move_note(&n.private, &n.shared, &from, &id, &to)
    })
    .await
}

/// Saves a note as `.md`: the backend opens the save dialog, so the window never chooses a path.
/// `None` when the dialog was cancelled, otherwise the path written.
#[tauri::command]
pub async fn export_note(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<Option<String>, NotesError> {
    notes_blocking(store, shared, move |n| {
        let (title, content) = n.dispatch(
            &folder,
            |s| s.note_for_export(&folder, &id),
            |p| p.note_for_export(&folder, &id),
        )?;
        save_text(
            &app,
            &window,
            &export::note_file_name(&title),
            (export::NOTE_FILTER, export::NOTE_EXTENSION),
            &content,
        )
    })
    .await
}

/// Opens the save dialog for a file named `name` of type `filter` (label, extension) and writes
/// `content` there.
fn save_text(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    name: &str,
    filter: (&str, &str),
    content: &str,
) -> Result<Option<String>, NotesError> {
    use tauri_plugin_dialog::DialogExt as _;
    let chosen = app
        .dialog()
        .file()
        .set_parent(window)
        .set_file_name(name)
        .add_filter(filter.0, &[filter.1])
        .blocking_save_file();
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let path = chosen.into_path().map_err(|_| ExportError::NotAPath)?;
    export::write(&path, content)?;
    Ok(Some(path.display().to_string()))
}

// Shared project notes: state, setup and unlocking (project_notes.rs, ADR 0006).

async fn shared_blocking<T: Send + 'static>(
    shared: State<'_, ProjectNotes>,
    work: impl FnOnce(&ProjectNotes) -> Result<T, ProjectNotesError> + Send + 'static,
) -> Result<T, ProjectNotesError> {
    let shared = shared.inner().clone();
    tauri::async_runtime::spawn_blocking(move || work(&shared))
        .await
        .map_err(|error| ProjectNotesError::Task(error.to_string()))?
}

/// State of the Shared group for the working folder `folder`. Resolves conflicted note files
/// first; `label` names the copies ("Title (label, Name)").
#[tauri::command]
pub async fn shared_notes_status(
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    label: String,
    untitled: String,
) -> Result<SharedStatus, ProjectNotesError> {
    shared_blocking(shared, move |p| p.status(&folder, &label, &untitled)).await
}

/// First step of turning shared notes on: returns the notes recovery code to show once.
#[tauri::command]
pub async fn shared_notes_prepare(
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<String, ProjectNotesError> {
    shared_blocking(shared, move |p| {
        p.prepare(&folder).map(|code| code.to_string())
    })
    .await
}

/// Saves the prepared recovery code as a text file. The backend opens the save dialog in
/// Documents (else Desktop) and refuses a place inside the repository or any other Git work tree,
/// where the code could be committed. `heading` is the first line, in the window's language.
#[tauri::command]
pub async fn shared_notes_save_code(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    heading: String,
) -> Result<project_notes::SaveCode, ProjectNotesError> {
    use tauri_plugin_dialog::DialogExt as _;
    let heading: String = heading
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    let start = app
        .path()
        .document_dir()
        .or_else(|_| app.path().desktop_dir())
        .ok();
    shared_blocking(shared, move |p| {
        let code = p.pending_code(&folder)?;
        let root = p.root(&folder)?;
        let mut dialog = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(project_notes::CODE_FILE_NAME)
            .add_filter(
                project_notes::CODE_FILE_FILTER.0,
                &[project_notes::CODE_FILE_FILTER.1],
            );
        if let Some(start) = start {
            dialog = dialog.set_directory(start);
        }
        let Some(chosen) = dialog.blocking_save_file() else {
            return Ok(project_notes::SaveCode::Cancelled);
        };
        let Ok(path) = chosen.into_path() else {
            return Ok(project_notes::SaveCode::Cancelled);
        };
        if !project_notes::code_path_allowed(&root, &path) {
            return Ok(project_notes::SaveCode::InsideRepository);
        }
        let text = zeroize::Zeroizing::new(format!("{heading}\n\n{}\n", *code));
        export::write(&path, &text).map_err(|error| ProjectNotesError::Io {
            path: path.clone(),
            source: std::io::Error::other(error.to_string()),
        })?;
        Ok(project_notes::SaveCode::Saved {
            path: path.display().to_string(),
        })
    })
    .await
}

/// Copies the prepared recovery code, marked as secret for clipboard histories, and clears the
/// clipboard after a minute if it still holds the code.
#[tauri::command]
pub async fn shared_notes_copy_code(
    app: tauri::AppHandle,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<(), ProjectNotesError> {
    use tauri_plugin_clipboard_manager::ClipboardExt as _;
    let code = shared_blocking(shared, move |p| p.pending_code(&folder)).await?;
    app.clipboard()
        .write_secret(code.as_str())
        .map_err(|error| ProjectNotesError::Task(error.to_string()))?;
    let spawned = std::thread::Builder::new()
        .name("notes-code-clipboard".to_owned())
        .spawn(move || {
            std::thread::sleep(project_notes::CODE_CLIPBOARD_TTL);
            let clipboard = app.clipboard();
            if let Ok(text) = clipboard.read_text() {
                let text = zeroize::Zeroizing::new(text);
                if text.trim() == code.as_str() {
                    let _ = clipboard.clear();
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("hangar: the clipboard will not be cleared: {error}");
    }
    Ok(())
}

/// The setup dialog closed without turning shared notes on: the prepared key and code go.
#[tauri::command]
pub async fn shared_notes_cancel(
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<(), ProjectNotesError> {
    shared_blocking(shared, move |p| p.cancel(&folder)).await
}

/// Second step, after "I saved it": writes the vault and stores the key; nothing is staged.
#[tauri::command]
pub async fn shared_notes_enable(
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    label: String,
    untitled: String,
) -> Result<SharedStatus, ProjectNotesError> {
    shared_blocking(shared, move |p| p.enable(&folder, &label, &untitled)).await
}

/// Unlocks the shared notes on this computer with the notes recovery code.
#[tauri::command]
pub async fn shared_notes_unlock(
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    code: String,
    label: String,
    untitled: String,
) -> Result<project_notes::Unlock, ProjectNotesError> {
    let code = zeroize::Zeroizing::new(code);
    shared_blocking(shared, move |p| p.unlock(&folder, &code, &label, &untitled)).await
}

// Detached side panel window (panel_window.rs, ADR 0007).

/// Opens the panel window with its fixed label and URL, at the remembered size and place when
/// that is still on a monitor, or brings it to the front if it is already open. Async: creating
/// a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_panel_window(
    app: tauri::AppHandle,
    always_on_top: bool,
    bounds: Option<panel_window::Bounds>,
) -> Result<(), PanelWindowError> {
    let focus = |window: tauri::WebviewWindow| -> Result<(), PanelWindowError> {
        let _ = window.unminimize();
        window.show()?;
        window.set_focus()?;
        Ok(())
    };
    if let Some(window) = app.get_webview_window(panel_window::LABEL) {
        return focus(window);
    }
    let monitors: Vec<panel_window::Area> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| {
            let scale = monitor.scale_factor().max(0.1);
            let position = monitor.position();
            let size = monitor.size();
            panel_window::Area {
                x: f64::from(position.x) / scale,
                y: f64::from(position.y) / scale,
                width: f64::from(size.width) / scale,
                height: f64::from(size.height) / scale,
            }
        })
        .collect();
    let placement = panel_window::placement(bounds, &monitors);
    let mut builder = tauri::WebviewWindowBuilder::new(
        &app,
        panel_window::LABEL,
        tauri::WebviewUrl::App(panel_window::URL.into()),
    )
    .title("Hangar")
    .inner_size(placement.width, placement.height)
    .min_inner_size(panel_window::MIN_WIDTH, panel_window::MIN_HEIGHT)
    .always_on_top(always_on_top);
    if let Some((x, y)) = placement.position {
        builder = builder.position(x, y);
    }
    match builder.build() {
        Ok(_) => Ok(()),
        // Two quick clicks: the other call created it meanwhile.
        Err(error) => match app.get_webview_window(panel_window::LABEL) {
            Some(window) => focus(window),
            None => Err(error.into()),
        },
    }
}

/// Keeps the panel window above other windows or not. Only the panel window calls it, for
/// itself: it holds this command instead of the general `set_always_on_top` permission.
#[tauri::command]
pub fn set_panel_on_top(
    window: tauri::WebviewWindow,
    on_top: bool,
) -> Result<(), PanelWindowError> {
    panel_window::check_sender(window.label())?;
    window.set_always_on_top(on_top)?;
    Ok(())
}

/// The only way from the panel window to the main window: a validated [`PanelMessage`],
/// delivered as event `panel-message` to `main` alone. The panel window may not emit events
/// itself, so it cannot fake events such as `tray-quit`.
#[tauri::command]
pub fn panel_to_main(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    message: PanelMessage,
) -> Result<(), PanelWindowError> {
    panel_window::check_sender(window.label())?;
    message.validate()?;
    if message.shows_main() {
        tray::show_main_window(&app);
    }
    app.emit_to(
        panel_window::MAIN_LABEL,
        panel_window::MESSAGE_EVENT,
        &message,
    )?;
    Ok(())
}

/// Shows the note's file selected in Explorer or Finder (Linux: its folder).
#[tauri::command]
pub async fn reveal_note(
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<(), NotesError> {
    notes_blocking(store, shared, move |n| {
        n.dispatch(
            &folder,
            |s| s.reveal_note(&folder, &id),
            |_| Err(ProjectNotesError::NoFile),
        )
    })
    .await
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
        panel_window::MAIN_LABEL,
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

/// The user's own commits in the repositories of `folders`, for the activity calendar
/// (git_activity.rs). Folders that are no repository are skipped, not reported.
#[tauri::command]
pub async fn git_activity(folders: Vec<PathBuf>) -> Result<GitActivity, ActivityError> {
    tauri::async_runtime::spawn_blocking(move || git_activity::activity(&Git::locate()?, &folders))
        .await
        .map_err(|error| ActivityError::Git(GitError::Task(error.to_string())))?
}

/// One page of the commits reachable from HEAD (git_history.rs); `limit` is clamped to 1..=100.
#[tauri::command]
pub async fn git_log(cwd: PathBuf, skip: u32, limit: u32) -> Result<GitLog, GitError> {
    tauri::async_runtime::spawn_blocking(move || {
        git_history::log(&Git::locate()?, &cwd, skip, limit)
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

/// The changes of one commit against its first parent, in the shape of `git_diff`'s answer plus
/// the commit's header. `sha` must be 7 to 64 hex characters.
#[tauri::command]
pub async fn git_commit_diff(cwd: PathBuf, sha: String) -> Result<GitCommitDiff, GitError> {
    tauri::async_runtime::spawn_blocking(move || {
        git_history::commit_diff(&Git::locate()?, &cwd, &sha)
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

/// One folder of a session's file tree (file_tree.rs). The window names only the session and a
/// `/` separated path below its folder; the folder itself comes from the session manager. A
/// folder that does not answer within [`file_tree::COMMAND_TIMEOUT`] is reported as an error;
/// its thread keeps the folder claimed until it ends.
#[tauri::command]
pub async fn list_session_folder(
    manager: Manager<'_>,
    tree: State<'_, FileTree>,
    id: SessionId,
    relative: String,
) -> Result<FolderListing, FileTreeError> {
    let cwd = manager
        .list()
        .into_iter()
        .find(|info| info.id == id)
        .map(|info| info.cwd)
        .ok_or(FileTreeError::UnknownSession(id))?;
    let claim = tree.in_flight.claim(id, &relative)?;
    let tree = tree.inner().clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _claim = claim;
        // Without git the tree still lists; only the ignored marks are missing.
        let git = Git::locate().ok();
        tree.list(&cwd, &relative, git.as_ref())
    });
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| FileTreeError::Task(error.to_string()))?,
        Err(_) => Err(FileTreeError::TimedOut),
    }
}

/// Opens a local dev server address a session printed in the default browser. Only loopback
/// http(s) addresses with an explicit port pass (`desktop::local_dev_url`), so the frontend cannot
/// open arbitrary pages or programs through it.
#[tauri::command]
pub async fn open_local_url(url: String) -> Result<(), String> {
    let url = crate::desktop::local_dev_url(&url)
        .ok_or_else(|| "not a local dev server address".to_string())?;
    tauri::async_runtime::spawn_blocking(move || crate::desktop::open_url(&url))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// Every path below a session's folder for the file tree's filter (file_tree.rs, `PathIndex`):
/// bounded in number, depth and time, ignored files left out. Takes only the session's ID.
#[tauri::command]
pub async fn list_session_paths(
    manager: Manager<'_>,
    tree: State<'_, FileTree>,
    id: SessionId,
) -> Result<PathIndex, FileTreeError> {
    let cwd = manager
        .list()
        .into_iter()
        .find(|info| info.id == id)
        .map(|info| info.cwd)
        .ok_or(FileTreeError::UnknownSession(id))?;
    let claim = tree.in_flight.claim(id, file_tree::INDEX_FLIGHT_KEY)?;
    let tree = tree.inner().clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _claim = claim;
        let git = Git::locate().ok();
        tree.index(&cwd, git.as_ref())
    });
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| FileTreeError::Task(error.to_string()))?,
        Err(_) => Err(FileTreeError::TimedOut),
    }
}

/// Opens the session's folder (the root of its file tree) in Explorer, Finder or the Linux file
/// manager. Takes only the session's ID: the backend looks up the folder itself, so the frontend
/// can never name a path to open.
#[tauri::command]
pub async fn reveal_session_folder(
    manager: Manager<'_>,
    id: SessionId,
) -> Result<(), FileTreeError> {
    let cwd = manager
        .list()
        .into_iter()
        .find(|info| info.id == id)
        .map(|info| info.cwd)
        .ok_or(FileTreeError::UnknownSession(id))?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        if !cwd.is_dir() {
            return Err(FileTreeError::NotAFolder);
        }
        crate::desktop::open(&cwd, crate::desktop::Target::Folder).map_err(FileTreeError::Io)
    });
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| FileTreeError::Task(error.to_string()))?,
        Err(_) => Err(FileTreeError::TimedOut),
    }
}

// Read-only file viewer windows (file_viewer.rs).

/// Opens a viewer for one file of a session's tree, or brings the open one to the front. The
/// main window names the session and the file's `/` separated path as `list_session_folder`
/// takes it; the window gets a fresh label and is bound to that file in [`Viewers`]. Async:
/// creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_file_viewer(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    manager: Manager<'_>,
    viewers: State<'_, Viewers>,
    session_id: SessionId,
    path: String,
) -> Result<(), FileViewerError> {
    file_viewer::check_opener(window.label())?;
    let cwd = manager
        .list()
        .into_iter()
        .find(|info| info.id == session_id)
        .map(|info| info.cwd)
        .ok_or(FileViewerError::UnknownSession(session_id))?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        file_viewer::resolve_file(&cwd, &path).map(|(root, absolute)| Target {
            session_id,
            root,
            path: absolute,
            relative: path,
        })
    });
    let target = match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| FileViewerError::Task(error.to_string()))??,
        Err(_) => return Err(FileViewerError::TimedOut),
    };
    let title = target.name();
    let label = match viewers.claim(target) {
        Claim::Existing(label) => {
            // Missing only while the other call is still creating it.
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.unminimize();
                window.show()?;
                window.set_focus()?;
            }
            return Ok(());
        }
        Claim::New(label) => label,
    };
    let built = tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App(file_viewer::URL.into()),
    )
    .title(title)
    .inner_size(file_viewer::WIDTH, file_viewer::HEIGHT)
    .min_inner_size(file_viewer::MIN_WIDTH, file_viewer::MIN_HEIGHT)
    .resizable(true)
    .build();
    match built {
        Ok(window) => {
            let viewers = viewers.inner().clone();
            let closed = label.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Destroyed = event {
                    viewers.remove(&closed);
                }
            });
            Ok(())
        }
        Err(error) => {
            viewers.remove(&label);
            Err(error.into())
        }
    }
}

/// The content of the file the calling viewer window was opened for, read again on every call.
/// The window names no path: its label alone selects the file.
#[tauri::command]
pub async fn file_viewer_content(
    window: tauri::WebviewWindow,
    viewers: State<'_, Viewers>,
) -> Result<FileContent, FileViewerError> {
    file_viewer::check_sender(window.label())?;
    let target = viewers
        .target(window.label())
        .ok_or(FileViewerError::UnknownWindow)?;
    let task = tauri::async_runtime::spawn_blocking(move || file_viewer::read_target(&target));
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| FileViewerError::Task(error.to_string()))?,
        Err(_) => Err(FileViewerError::TimedOut),
    }
}

/// The calling viewer's target, looked up by its label only.
fn viewer_target(
    window: &tauri::WebviewWindow,
    viewers: &Viewers,
) -> Result<Target, FileViewerError> {
    file_viewer::check_sender(window.label())?;
    viewers
        .target(window.label())
        .ok_or(FileViewerError::UnknownWindow)
}

/// Size and time of the last change of the calling viewer's file; the window asks every second
/// while it is visible and reads the file again when the stamp changed (live refresh).
#[tauri::command]
pub async fn file_viewer_stamp(
    window: tauri::WebviewWindow,
    viewers: State<'_, Viewers>,
) -> Result<FileStamp, FileViewerError> {
    let target = viewer_target(&window, &viewers)?;
    let task = tauri::async_runtime::spawn_blocking(move || file_viewer::stamp_target(&target));
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| FileViewerError::Task(error.to_string()))?,
        Err(_) => Err(FileViewerError::TimedOut),
    }
}

/// The calling viewer's file against the last commit (`git diff HEAD -- <file>`).
#[tauri::command]
pub async fn file_viewer_diff(
    window: tauri::WebviewWindow,
    viewers: State<'_, Viewers>,
) -> Result<HeadDiff, FileViewerError> {
    let target = viewer_target(&window, &viewers)?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        file_viewer::head_diff(&Git::locate()?, &target)
    });
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| FileViewerError::Task(error.to_string()))?,
        Err(_) => Err(FileViewerError::TimedOut),
    }
}

/// Opens a link of the Markdown preview in the default browser: only from a viewer window that
/// still has its file, and only plain http(s) addresses ([`file_viewer::checked_link`]).
#[tauri::command]
pub async fn file_viewer_open_link(
    window: tauri::WebviewWindow,
    viewers: State<'_, Viewers>,
    url: String,
) -> Result<(), FileViewerError> {
    viewer_target(&window, &viewers)?;
    let url = file_viewer::checked_link(&url)?;
    // A link follows a click in the viewer, so the viewer has the focus; a page that opens
    // links on its own (in the background, in a loop) gets nothing.
    if !window.is_focused()? || !viewers.allow_link(window.label(), std::time::Instant::now()) {
        return Err(FileViewerError::LinkRefused);
    }
    tauri::async_runtime::spawn_blocking(move || desktop::open_url(&url))
        .await
        .map_err(|error| FileViewerError::Task(error.to_string()))?
        .map_err(FileViewerError::Open)
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

/// Never forces: what stays is reported in `reason`. The claims let go of the worktree first
/// (watcher closed, running git call finished), since Windows keeps a watched folder open, and
/// take it back when it stays.
#[tauri::command]
pub async fn remove_worktree(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    worktree: Worktree,
) -> Result<Removal, WorktreeError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        worktree::check_shape(&worktree)?;
        worktree::check_managed(&base, &worktree.path, &worktree.repo_root)?;
        let claims = app.state::<ClaimsService>();
        let released = claims.release(&worktree.path);
        let removal = Git::locate()
            .map_err(WorktreeError::from)
            .and_then(|git| worktree::remove(&git, &base, &worktree));
        // The worktree stays: its sessions keep their overlap warnings.
        if !removal.as_ref().is_ok_and(|removal| removal.removed) {
            claims.unrelease(released);
        }
        removal
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

// Local claims (claims.rs, claims_service.rs). Contract: team-service-v1.md, step A.

/// Turns the observation of the sessions' changed files on or off. While on, the backend emits
/// `claims-changed` to the main window with every new [`ClaimsSnapshot`].
#[tauri::command]
pub fn configure_claims(claims: State<'_, ClaimsService>, enabled: bool) {
    claims.configure(enabled);
}

/// The newest snapshot, for a window that starts listening late.
#[tauri::command]
pub fn claims_snapshot(claims: State<'_, ClaimsService>) -> ClaimsSnapshot {
    claims.snapshot()
}

// Agent gate (gate.rs, gate_service.rs). Contract: team-service-v1.md, step B.

/// Turns the agent gate on or off (the window's setting). Only agents started while it is on get
/// the hook; turning it off lets every waiting edit through. Emits `gate-changed` with a
/// [`GateSnapshot`] whenever the waiting edits change.
#[tauri::command]
pub fn configure_gate(gate: State<'_, GateService>, enabled: bool) {
    gate.configure(enabled);
}

#[tauri::command]
pub fn gate_snapshot(gate: State<'_, GateService>) -> GateSnapshot {
    gate.snapshot()
}

/// The person's answer to a waiting edit; `false` when it no longer waits (answered, timed out,
/// session ended).
#[tauri::command]
pub fn answer_gate(gate: State<'_, GateService>, id: u64, answer: Answer) -> bool {
    gate.answer(id, answer)
}

// Hangar account (account.rs, account_service.rs). Contract: hangar-account-api.md. Every change
// of the state is also emitted as `account-changed` with an `AccountView`.

/// The sign-in state; reads a stored sign-in from the keychain on the first call.
#[tauri::command]
pub async fn account_status(
    account: State<'_, AccountService>,
) -> Result<AccountView, AccountError> {
    Ok(account.status().await)
}

/// Starts the device flow: shows a code and opens the confirmation page in the browser.
#[tauri::command]
pub async fn account_sign_in(
    account: State<'_, AccountService>,
) -> Result<AccountView, AccountError> {
    account.sign_in().await
}

#[tauri::command]
pub fn account_cancel_sign_in(account: State<'_, AccountService>) -> AccountView {
    account.cancel_sign_in()
}

/// Drops the tokens on this device only (the server has no revoke endpoint).
#[tauri::command]
pub async fn account_sign_out(
    account: State<'_, AccountService>,
) -> Result<AccountView, AccountError> {
    account.sign_out().await
}

#[tauri::command]
pub async fn account_projects(
    account: State<'_, AccountService>,
) -> Result<Vec<Project>, AccountError> {
    account.projects().await
}

/// Changes the server for self-hosting; `null` or empty: the official service.
#[tauri::command]
pub async fn account_set_base_url(
    account: State<'_, AccountService>,
    base_url: Option<String>,
) -> Result<AccountView, AccountError> {
    account.set_base_url(base_url).await
}

/// Opens one of the account's pages; the window names the page, never a URL.
#[tauri::command]
pub fn account_open(
    account: State<'_, AccountService>,
    page: AccountPage,
) -> Result<(), AccountError> {
    account.open(page)
}

// Team connection (team.rs, team_engine.rs, team_service.rs; step F part 2a). Every change is
// also emitted as `team-changed` with a `TeamSnapshot`.

/// The connection state of one project ("Share with team" off when never turned on).
#[tauri::command]
pub fn team_status(team: State<'_, TeamService>, project_id: String) -> TeamStatus {
    team.status(&project_id)
}

/// Turns "Share with team" on for a project of the signed-in account.
#[tauri::command]
pub async fn team_enable(
    team: State<'_, TeamService>,
    project_id: String,
) -> Result<TeamStatus, TeamError> {
    team.enable(&project_id).await
}

#[tauri::command]
pub async fn team_disable(
    team: State<'_, TeamService>,
    project_id: String,
) -> Result<TeamStatus, TeamError> {
    team.disable(&project_id).await
}

/// Devices that wait for this device's confirmation, in every shared project.
#[tauri::command]
pub fn team_pending_confirmations(team: State<'_, TeamService>) -> Vec<PendingConfirmation> {
    team.pending()
}

/// Confirms a waiting device after the person compared its safety number.
#[tauri::command]
pub async fn team_confirm(team: State<'_, TeamService>, request: String) -> Result<(), TeamError> {
    team.confirm(&request).await
}

#[tauri::command]
pub async fn team_reject(team: State<'_, TeamService>, request: String) -> Result<(), TeamError> {
    team.reject(&request).await
}

/// What a session is doing, as the window's attention logic says.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateEntry {
    session_id: SessionId,
    state: SessionState,
}

/// The window's states of its sessions, for the team's presence (only sessions in repositories
/// linked to a shared project leave the device).
#[tauri::command]
pub fn team_session_states(team: State<'_, TeamService>, states: Vec<SessionStateEntry>) {
    team.set_session_states(
        states
            .into_iter()
            .map(|entry| (entry.session_id, entry.state))
            .collect(),
    );
}

/// Links or unlinks a repository folder (its root commit and clone folder, from `team-changed`'s
/// `repos`) to a shared project: its sessions' presence and changed files go to the team from
/// now on. `folder: null` names a link from before folders were bound, which can only be
/// removed (`on: false`).
#[tauri::command]
pub fn team_link_repo(
    team: State<'_, TeamService>,
    project_id: String,
    root: String,
    folder: Option<String>,
    on: bool,
) -> Result<(), TeamError> {
    match (folder, on) {
        (Some(folder), on) => team.link_repo(&project_id, &root, &folder, on),
        (None, false) => team.unlink_legacy_repo(&project_id, &root),
        (None, true) => Err(TeamError::UnknownRepository),
    }
}

/// Reserves a task in a shared project ("I'm working on ..."): a short title, optionally linked
/// to a running session in a shared repository and a folder of it. The relay decides who holds
/// the task (first wins); `team-changed` shows the outcome.
#[tauri::command]
pub async fn team_reserve_task(
    team: State<'_, TeamService>,
    project_id: String,
    title: String,
    session_id: Option<SessionId>,
    area: Option<String>,
) -> Result<(), TeamError> {
    let task = crate::team_service::Task {
        title,
        session: session_id,
        area,
    };
    team.reserve_task(&project_id, task).await
}

/// Releases a task this device holds, or gives up an attempt someone else's hold blocks. `task`
/// is the task's keyed hash from `team-changed`.
#[tauri::command]
pub async fn team_release_task(
    team: State<'_, TeamService>,
    project_id: String,
    task: String,
) -> Result<(), TeamError> {
    team.release_task(&project_id, &task).await
}

/// Asks the holder of a task to hand it over.
#[tauri::command]
pub async fn team_ask_release(
    team: State<'_, TeamService>,
    project_id: String,
    task: String,
) -> Result<(), TeamError> {
    team.ask_release(&project_id, &task).await
}

/// The holder's answer to a release request: `requester` is the member the card showed (a
/// request of anyone else is refused with `unknownTask`), `accept` hands the task over.
#[tauri::command]
pub async fn team_answer_release(
    team: State<'_, TeamService>,
    project_id: String,
    task: String,
    requester: String,
    accept: bool,
) -> Result<(), TeamError> {
    team.answer_release(&project_id, &task, &requester, accept)
        .await
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

/// Saves the clipboard's image as a PNG in the app's cache folder and returns its path, for the
/// window to paste like a dropped file. One paste at a time: while one runs, another answers
/// `Busy`. Off the main thread: reading the clipboard there may deadlock on Linux.
#[tauri::command]
pub async fn paste_clipboard_image(
    app: tauri::AppHandle,
    gate: State<'_, PasteGate>,
) -> Result<PasteOutcome, PasteImageError> {
    let Some(ticket) = gate.try_start() else {
        return Ok(PasteOutcome::Busy);
    };
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| PasteImageError::CacheDir(error.to_string()))?
        .join(paste_image::FOLDER);
    tauri::async_runtime::spawn_blocking(move || {
        let _ticket = ticket;
        let Some(image) = clipboard_image::read()? else {
            return Ok(PasteOutcome::NoImage);
        };
        let path = paste_image::save(&dir, image, std::time::SystemTime::now())?;
        Ok(PasteOutcome::Saved {
            path: path.display().to_string(),
        })
    })
    .await
    .map_err(|error| PasteImageError::Task(error.to_string()))?
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
