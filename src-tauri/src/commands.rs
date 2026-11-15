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
use crate::arena::{self, ArenaError};
use crate::claims::ClaimsSnapshot;
use crate::claims_service::ClaimsService;
use crate::clipboard_image;
use crate::daily_review::{self, DailyReview, ReviewError};
use crate::desktop;
use crate::dictation::model::ModelId;
use crate::dictation::whisper::Language;
use crate::dictation::{self, DictationError, DictationService, DictationStatus, DictationText};
use crate::export::{self, ExportError, Format};
use crate::file_tree::{self, FileTree, FileTreeError, FolderListing, PathIndex};
use crate::file_viewer::{
    self, Claim, FileContent, FileStamp, FileViewerError, HeadDiff, Target, Viewers,
};
use crate::gate::{Answer, GateSnapshot};
use crate::gate_rules::GateRule;
use crate::gate_service::{GateService, GuardInfo};
use crate::git::{self, Git, GitDiff, GitError};
use crate::git_actions::{self, ActionError};
use crate::git_activity::{self, ActivityError, GitActivity};
use crate::git_history::{self, GitCommitDiff, GitLog};
use crate::git_snapshots::{
    self, Neighbours, RestoreOutcome, SnapshotEnv, SnapshotError, SnapshotList, Snapshots,
    TakeOutcome,
};
use crate::gpu::GpuMonitor;
use crate::icon::{self, IconCache};
use crate::install_source::{self, SystemProbe};
use crate::instructions::{self, StartPrompt};
use crate::main_store::{
    self, MainPrefs, MainStore, MainStoreError, ProtectedSettings, QueuedMessages,
    SecuritySwitches, WorkspaceTemplate,
};
use crate::manager::{
    EventSink, ManagerError, SessionEvent, SessionManager, StartRequest, TermSize,
};
use crate::notes::{
    FolderInfo, NoteFolder, NoteInfo, NotesError, NotesStore, SearchHit, TrashInfo,
};
use crate::notify::{self, Notifier};
use crate::panel_window::{self, PanelMessage, PanelScope, PanelWindowError};
use crate::paste_image::{self, PasteGate, PasteImageError, PasteOutcome};
use crate::path_links::{self, Opened, PathKind, PathLinkError};
use crate::persist::{PersistError, Store, Workspace};
use crate::profile::{self, ProfileProblem};
use crate::profile_edit::{self, EditError, ProfileInput};
use crate::project_notes::{self, ProjectNotes, ProjectNotesError, SharedStatus};
use crate::push::{self, PushError, PushEvent, PushInput, PushService, PushView};
use crate::report::{self, ReportError};
use crate::resources::{ResourceMonitor, SessionResources};
use crate::secrets::{self, SecretError, SecretStatus};
use crate::session::{SessionId, SessionInfo, SessionKind, SessionMode, Status};
use crate::subagents::{self, SubAgentError, SubAgentEvent, Watchers};
use crate::system_usage::{SystemMonitor, SystemUsage};
use crate::team::{PendingConfirmation, TeamError, TeamStatus};
use crate::team_service::TeamService;
use crate::team_share::SessionState;
use crate::tray::{self, TrayError, TrayLabels};
use crate::updates::{self, UpdateCache, UpdateInfo};
use crate::version::{self, VersionCache};
use crate::work_log::{self, WorkLog, WorkLogError};
use crate::worktree::{self, Removal, Worktree, WorktreeError};
use crate::worktree_cleanup;

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

/// Passes every event on and writes the work log's end record when the process exits.
/// `session` is set once the start returned the id.
fn work_log_sink(
    log: WorkLog,
    session: Arc<OnceLock<SessionId>>,
    inner: impl EventSink,
) -> impl EventSink {
    move |event: SessionEvent| {
        if matches!(event, SessionEvent::Exit { .. }) {
            if let Some(id) = session.get() {
                log.ended(*id, work_log::now_ms());
            }
        }
        inner.emit(event);
    }
}

/// The work log's start record of a session that just started (ADR 0011).
fn log_started(log: &WorkLog, manager: &SessionManager, info: &SessionInfo) {
    let names = manager.profile_names();
    let program = names
        .get(&info.profile_id)
        .map_or(info.profile_id.as_str(), String::as_str);
    let now = work_log::now_ms();
    log.started(info, program, now);
    // A process that exited before its id was known reported the exit to nobody.
    let exited = manager
        .list()
        .iter()
        .any(|session| session.id == info.id && session.status == Status::Exited);
    if exited {
        log.ended(info.id, now);
    }
}

/// Signal without payload: an update session's result ([`updates::UpdateOutcome`]) waits in
/// [`UpdateResults`] for the main window's `take_update_results`.
pub const UPDATE_RESULT_EVENT: &str = "program-update-result";

/// Update results not yet taken by the main window, oldest first. Bounded: a window that never
/// asks keeps only the newest ones.
#[derive(Default)]
pub struct UpdateResults(Mutex<std::collections::VecDeque<updates::UpdateOutcome>>);

impl UpdateResults {
    /// More results than programs updated at once is no realistic number.
    const MAX: usize = 32;

    pub fn push(&self, outcome: updates::UpdateOutcome) {
        let mut queue = crate::lock(&self.0);
        if queue.len() == Self::MAX {
            queue.pop_front();
        }
        queue.push_back(outcome);
    }

    /// Every waiting result, each handed out once.
    pub fn take(&self) -> Vec<updates::UpdateOutcome> {
        crate::lock(&self.0).drain(..).collect()
    }
}

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
            // A signal only: listeners of other windows receive events emitted to the main one,
            // and the outcome names program paths. The main window takes it through
            // `take_update_results`.
            app.state::<UpdateResults>().push(outcome);
            let _ = app.emit_to(panel_window::MAIN_LABEL, UPDATE_RESULT_EVENT, ());
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
        desktop::open_text_file(&path)
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
/// gets the agent gate and never reaches the team. `instructions`: hand the agent `HANGAR.md`
/// (Settings); `start_prompt`: a first prompt by name, whose text comes from the catalog, never
/// from the window. Both apply only where the program takes them ([`crate::instructions`]).
///
/// Async on a blocking thread: the start searches the PATH, may wait for the WSL detection and
/// the version query of the gate, and spawns a process; on the main thread that would freeze
/// the window.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // One Tauri command, one argument per field of the call.
pub async fn create_session(
    app: tauri::AppHandle,
    profile_id: String,
    cwd: Option<PathBuf>,
    size: TermSize,
    on_event: Channel<SessionEvent>,
    resume: Option<bool>,
    cli_session_id: Option<String>,
    mode: Option<SessionMode>,
    instructions: Option<bool>,
    start_prompt: Option<StartPrompt>,
) -> Result<SessionInfo, ManagerError> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = app.state::<SessionManager>();
        let work_log = app.state::<WorkLog>();
        let session = Arc::new(OnceLock::new());
        let sink = work_log_sink(
            work_log.inner().clone(),
            Arc::clone(&session),
            channel_sink(on_event),
        );
        // Optional, so existing calls without the later arguments stay valid.
        let info = manager.create_requested(
            &profile_id,
            cwd,
            size,
            resume.unwrap_or(false),
            cli_session_id.as_deref(),
            mode.unwrap_or_default(),
            StartRequest {
                instructions: instructions.unwrap_or(false),
                prompt: start_prompt,
            },
            sink,
        )?;
        let _ = session.set(info.id);
        log_started(&work_log, &manager, &info);
        Ok(info)
    })
    .await
    .map_err(|error| ManagerError::Task(error.to_string()))?
}

/// Opens exactly this machine's `HANGAR.md` in the default editor; if it is missing, it is
/// created from the commented template ([`crate::instructions::DEFAULT_TEXT`]). Takes no path:
/// the window cannot open any other file with it.
#[tauri::command]
pub async fn open_hangar_md(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let load = app.state::<ProfileLoad>();
        let path = instructions::ensure_file(&load.dir)
            .map_err(|error| format!("{} cannot be created: {error}", load.dir.display()))?;
        desktop::open_text_file(&path)
            .map_err(|error| format!("{} cannot be opened: {error}", path.display()))
    })
    .await
    .map_err(|error| error.to_string())?
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

/// `false` when the session's input queue is full (its program does not read right now); the
/// window sends the same text again a moment later. At most [`crate::manager::MAX_WRITE`] bytes
/// per call: the window splits a larger paste.
#[tauri::command]
pub fn write_session(
    manager: Manager<'_>,
    id: SessionId,
    data: String,
) -> Result<bool, ManagerError> {
    match manager.write(id, &data) {
        Ok(()) => Ok(true),
        Err(ManagerError::Busy(_)) => Ok(false),
        Err(error) => Err(error),
    }
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

/// On a blocking thread: ending a tree terminates a Job Object, which may wait on the processes.
#[tauri::command]
pub async fn kill_session(app: tauri::AppHandle, id: SessionId) -> Result<(), ManagerError> {
    tauri::async_runtime::spawn_blocking(move || app.state::<SessionManager>().kill(id))
        .await
        .map_err(|error| ManagerError::Task(error.to_string()))?
}

/// Async on a blocking thread, like [`create_session`].
#[tauri::command]
pub async fn restart_session(
    app: tauri::AppHandle,
    id: SessionId,
    size: TermSize,
    on_event: Channel<SessionEvent>,
) -> Result<SessionInfo, ManagerError> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = app.state::<SessionManager>();
        let work_log = app.state::<WorkLog>();
        // Restarting an update session repeats the update; after that it behaves like a start.
        let update_profile = manager
            .list()
            .into_iter()
            .find(|info| info.id == id && info.kind == SessionKind::Update)
            .map(|info| info.profile_id);
        let info = if let Some(profile_id) = update_profile {
            let watch = UpdateWatch::new(&app, &profile_id);
            let _ = watch.session.set(id);
            manager.restart(id, size, update_sink(app.clone(), on_event, watch))
        } else {
            let session = Arc::new(OnceLock::from(id));
            let sink = work_log_sink(work_log.inner().clone(), session, channel_sink(on_event));
            manager.restart(id, size, sink)
        }?;
        log_started(&work_log, &manager, &info);
        // The restart runs under a new conversation id; a watch would keep following the old
        // one. The window starts a new watch with the returned `SessionInfo` if it wants one.
        app.state::<Watchers>().stop(id);
        Ok(info)
    })
    .await
    .map_err(|error| ManagerError::Task(error.to_string()))?
}

#[tauri::command]
pub fn rename_session(
    manager: Manager<'_>,
    work_log: State<'_, WorkLog>,
    id: SessionId,
    title: String,
) -> Result<SessionInfo, ManagerError> {
    let info = manager.rename(id, &title)?;
    work_log.renamed(id, &info.title);
    Ok(info)
}

/// Also withdraws the session's desktop notification and ends its sub-agent watch. All on a
/// blocking thread: ending the tree and closing the pseudo console may wait.
#[tauri::command]
pub async fn close_session(app: tauri::AppHandle, id: SessionId) -> Result<(), ManagerError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<SessionManager>().close(id)?;
        app.state::<WorkLog>().ended(id, work_log::now_ms());
        app.state::<Watchers>().stop(id);
        app.state::<Notifier>().forget(id);
        Ok(())
    })
    .await
    .map_err(|error| ManagerError::Task(error.to_string()))?
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

/// Tokens and estimated cost per running session and per local day (cost_service.rs); the
/// payload-free `cost-usage-changed` signal says when to read it again. Other features read one
/// session's numbers from `sessions` (or through `CostService::session` in the backend).
#[tauri::command]
pub fn cost_report(
    costs: State<'_, crate::cost_service::CostService>,
) -> crate::cost_service::CostReport {
    costs.report()
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

/// The panel window reaches the shared notes of its session's working folder only
/// ([`PanelScope::allows_folder`]); any other folder is answered as if it were no repository,
/// before anything touches it.
fn notes_in_scope(
    label: &str,
    scope: &PanelScope,
    folder: &NoteFolder,
) -> Result<(), ProjectNotesError> {
    if scope.allows_folder(label, folder) {
        Ok(())
    } else {
        Err(ProjectNotesError::NotRepository)
    }
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<Vec<NoteInfo>, NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<String, NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
    content: String,
) -> Result<(), NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    title: String,
) -> Result<String, NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
    title: String,
) -> Result<(), NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<(), NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    ids: Vec<String>,
) -> Result<(), NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
    notes_blocking(store, shared, move |n| {
        n.dispatch(&folder, |s| s.reorder_notes(&folder, &ids), |_| Ok(()))
    })
    .await
}

#[tauri::command]
pub async fn reveal_notes_folder(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<(), NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    query: String,
) -> Result<Vec<SearchHit>, NotesError> {
    let hits = notes_blocking(store, shared, move |n| {
        let private = n.private.search(&query)?;
        let shared = n.shared.search(&query)?;
        Ok(crate::notes::merge_hits(&query, private, shared))
    })
    .await?;
    // The panel sees the shared notes of its session's folder only (`notes_in_scope`).
    let label = window.label();
    Ok(hits
        .into_iter()
        .filter(|hit| scope.allows_hit(label, hit))
        .collect())
}

#[tauri::command]
pub async fn list_trash(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<Vec<TrashInfo>, NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<String, NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    from: NoteFolder,
    id: String,
    to: NoteFolder,
) -> Result<String, NotesError> {
    notes_in_scope(window.label(), &scope, &from)?;
    notes_in_scope(window.label(), &scope, &to)?;
    notes_blocking(store, shared, move |n| {
        project_notes::move_note(&n.private, &n.shared, &from, &id, &to)
    })
    .await
}

/// Saves a note as `.md`: the backend opens the save dialog, so the window never chooses a path.
/// `None` when the dialog was cancelled, otherwise the path written.
#[tauri::command]
pub async fn export_note(
    scope: State<'_, PanelScope>,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<Option<String>, NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    label: String,
    untitled: String,
) -> Result<SharedStatus, ProjectNotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
    shared_blocking(shared, move |p| p.status(&folder, &label, &untitled)).await
}

/// First step of turning shared notes on: returns the notes recovery code to show once.
#[tauri::command]
pub async fn shared_notes_prepare(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<String, ProjectNotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    scope: State<'_, PanelScope>,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    heading: String,
) -> Result<project_notes::SaveCode, ProjectNotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    app: tauri::AppHandle,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<(), ProjectNotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
) -> Result<(), ProjectNotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
    shared_blocking(shared, move |p| p.cancel(&folder)).await
}

/// Second step, after "I saved it": writes the vault and stores the key; nothing is staged.
#[tauri::command]
pub async fn shared_notes_enable(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    label: String,
    untitled: String,
) -> Result<SharedStatus, ProjectNotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
    shared_blocking(shared, move |p| p.enable(&folder, &label, &untitled)).await
}

/// Unlocks the shared notes on this computer with the notes recovery code.
#[tauri::command]
pub async fn shared_notes_unlock(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    code: String,
    label: String,
    untitled: String,
) -> Result<project_notes::Unlock, ProjectNotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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

/// Closes the panel window: the calling window itself, never another one. The close runs the
/// window's own close handler, which saves pending input and then calls
/// [`destroy_panel_window`]. Instead of `core:window:allow-close`, whose command takes any
/// window's label.
#[tauri::command]
pub fn close_panel_window(window: tauri::WebviewWindow) -> Result<(), PanelWindowError> {
    panel_window::check_sender(window.label())?;
    window.close()?;
    Ok(())
}

/// Ends the panel window once its close handler saved pending input: the calling window itself,
/// never another one. Instead of `core:window:allow-destroy`.
#[tauri::command]
pub fn destroy_panel_window(window: tauri::WebviewWindow) -> Result<(), PanelWindowError> {
    panel_window::check_sender(window.label())?;
    window.destroy()?;
    Ok(())
}

/// The only way from the panel window to the main window: a validated [`PanelMessage`],
/// delivered as event `panel-message` to `main` alone. The panel window may not emit events
/// itself, so it cannot fake events such as `tray-quit`. Text goes to the session the main
/// window handed the panel only ([`PanelMessage::scoped`]).
#[tauri::command]
pub fn panel_to_main(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    scope: State<'_, PanelScope>,
    message: PanelMessage,
) -> Result<(), PanelWindowError> {
    panel_window::check_sender(window.label())?;
    message.validate()?;
    let message = message.scoped(&scope)?;
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    store: State<'_, NotesStore>,
    shared: State<'_, ProjectNotes>,
    folder: NoteFolder,
    id: String,
) -> Result<(), NotesError> {
    notes_in_scope(window.label(), &scope, &folder)?;
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

// Session resources (resources.rs) and push to the phone (push.rs).

/// CPU, memory and GPU of every running session's process tree. The window asks every few seconds
/// while it is visible and the setting is on; nothing samples in between, and an answer younger
/// than a second is repeated.
#[tauri::command]
pub async fn session_resources(app: tauri::AppHandle) -> Vec<SessionResources> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = std::time::Instant::now();
        app.state::<ResourceMonitor>().sample(
            now,
            || app.state::<SessionManager>().tree_usages(),
            || app.state::<GpuMonitor>().snapshot(now),
        )
    })
    .await
    .unwrap_or_default()
}

/// Load of the whole machine (CPU, memory, GPU) for the sidebar's system bar. Asked like
/// `session_resources`: only while the window is visible and the setting is on.
#[tauri::command]
pub async fn system_usage(app: tauri::AppHandle) -> SystemUsage {
    tauri::async_runtime::spawn_blocking(move || {
        let now = std::time::Instant::now();
        app.state::<SystemMonitor>()
            .sample(now, || app.state::<GpuMonitor>().snapshot(now))
    })
    .await
    .unwrap_or_default()
}

/// Whether the machine runs on battery, read afresh (power.rs); later changes arrive as
/// `power-changed` events with the same shape. One system call or a few small sysfs files.
#[tauri::command]
pub fn power_state(monitor: State<'_, crate::power::PowerMonitor>) -> crate::power::PowerState {
    monitor.refresh(crate::power::read())
}

/// The keychain may take a moment or ask the user, hence on a blocking thread.
async fn push_blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    work: impl FnOnce(&PushService) -> Result<T, PushError> + Send + 'static,
) -> Result<T, PushError> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<PushService>()))
        .await
        .map_err(|_| PushError::Task)?
}

/// The push settings and which secrets are stored (never their values).
#[tauri::command]
pub async fn push_settings(app: tauri::AppHandle) -> Result<PushView, PushError> {
    push_blocking(app, PushService::view).await
}

/// Saves the push settings; secrets go to the keychain only.
#[tauri::command]
pub async fn push_save(app: tauri::AppHandle, input: PushInput) -> Result<PushView, PushError> {
    push_blocking(app, move |push| push.save(input)).await
}

/// Sends `text` once with the saved settings; at most one test every few seconds.
#[tauri::command]
pub async fn push_test(app: tauri::AppHandle, text: String) -> Result<(), PushError> {
    let (client, request) = push_blocking(app, move |push| {
        let request = push.for_test(&text, std::time::Instant::now())?;
        Ok((push.client()?, request))
    })
    .await?;
    push::deliver(&client, request).await
}

/// A push for a session's new state; `false` when none was due (see
/// [`PushService::for_session`]). `template` is the window's translated sentence with one
/// `{name}`; the backend fills in the session's program and number (or its name, if allowed).
#[tauri::command]
pub async fn push_session(
    app: tauri::AppHandle,
    id: SessionId,
    event: PushEvent,
    template: String,
) -> Result<bool, PushError> {
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        let manager = app.state::<SessionManager>();
        let sessions: Vec<(SessionId, String, String)> = manager
            .list()
            .into_iter()
            .map(|info| (info.id, info.profile_id, info.title))
            .collect();
        let names = push::session_names(&sessions, &manager.profile_names(), id)
            .ok_or(PushError::UnknownSession)?;
        let push = app.state::<PushService>();
        match push.for_session(id, event, &template, &names, std::time::Instant::now())? {
            Some(request) => Ok::<_, PushError>(Some((push.client()?, request))),
            None => Ok(None),
        }
    })
    .await
    .map_err(|_| PushError::Task)??;
    let Some((client, request)) = prepared else {
        return Ok(false);
    };
    push::deliver(&client, request).await?;
    Ok(true)
}

// Git and worktrees (git.rs, worktree.rs), on a blocking thread because git may take seconds.

/// Branch per folder, read from the files under `.git`; no process.
#[tauri::command]
pub async fn git_branches(paths: Vec<String>) -> Result<HashMap<String, Option<String>>, GitError> {
    tauri::async_runtime::spawn_blocking(move || git::branches(&paths))
        .await
        .map_err(|error| GitError::Task(error.to_string()))?
}

/// Repository place per folder for the project overview (`git::RepoPlace`): the work tree and
/// the repository's main folder, read from `.git` files without a process, like `git_branches`.
#[tauri::command]
pub async fn git_repo_places(
    paths: Vec<String>,
) -> Result<HashMap<String, Option<git::RepoPlace>>, GitError> {
    tauri::async_runtime::spawn_blocking(move || git::repo_places(&paths))
        .await
        .map_err(|error| GitError::Task(error.to_string()))?
}

/// A git read of a session's folder failed: the session is unknown, or git itself failed.
#[derive(Debug, thiserror::Error)]
pub enum SessionGitError {
    #[error("Unknown session {0}")]
    UnknownSession(SessionId),
    #[error(transparent)]
    Git(#[from] GitError),
}

impl Serialize for SessionGitError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// The working folder of session `id`. The git reads name a session, never a folder: git runs
/// programs a repository's config names (fsmonitor, filters), so a window must not be able to
/// point it at any folder it likes, and the side panel cannot start sessions at all.
pub fn session_folder(manager: &SessionManager, id: SessionId) -> Result<PathBuf, SessionGitError> {
    manager
        .list()
        .into_iter()
        .find(|info| info.id == id)
        .map(|info| info.cwd)
        .ok_or(SessionGitError::UnknownSession(id))
}

/// Runs one git read in the folder of session `id` (with `root`: the main folder of its
/// repository, as `git_repo_places` names it) on a blocking thread.
async fn session_git<T: Send + 'static>(
    manager: &SessionManager,
    id: SessionId,
    root: bool,
    work: impl FnOnce(&Git, &std::path::Path) -> Result<T, GitError> + Send + 'static,
) -> Result<T, SessionGitError> {
    let cwd = session_folder(manager, id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let folder = if root {
            let key = cwd.to_string_lossy().into_owned();
            git::repo_places(std::slice::from_ref(&key))?
                .remove(&key)
                .flatten()
                .map_or(cwd, |place| PathBuf::from(place.root))
        } else {
            cwd
        };
        Ok(work(&Git::locate()?, &folder)?)
    })
    .await
    .map_err(|error| SessionGitError::Git(GitError::Task(error.to_string())))?
}

/// The panel window reaches only the session the main window handed it ([`PanelScope`]); any
/// other id is answered like an unknown session.
fn in_scope(window: &tauri::Window, scope: &PanelScope, id: SessionId) -> bool {
    scope.allows(window.label(), id)
}

#[tauri::command]
pub async fn git_diff(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
) -> Result<GitDiff, SessionGitError> {
    if !in_scope(&window, &scope, id) {
        return Err(SessionGitError::UnknownSession(id));
    }
    session_git(&manager, id, false, git::diff).await
}

/// How many files `git_diff` would list for the session's folder (`None`: no repository), for
/// the notice when a session is done; costs one `git status` instead of the whole diff.
#[tauri::command]
pub async fn git_changed_count(
    manager: Manager<'_>,
    id: SessionId,
) -> Result<Option<u32>, SessionGitError> {
    session_git(&manager, id, false, git::changed_count).await
}

/// The user's own commits in the repositories of `folders`, for the activity calendar
/// (git_activity.rs). Folders that are no repository are skipped, not reported.
#[tauri::command]
pub async fn git_activity(folders: Vec<PathBuf>) -> Result<GitActivity, ActivityError> {
    tauri::async_runtime::spawn_blocking(move || git_activity::activity(&Git::locate()?, &folders))
        .await
        .map_err(|error| ActivityError::Git(GitError::Task(error.to_string())))?
}

/// A working phase of a session ended (`from`, `to` in Unix ms); the window reports one event
/// per phase, never per output chunk. Only times: the backend adds the session's details.
#[tauri::command]
pub async fn log_work_phase(
    work_log: State<'_, WorkLog>,
    id: SessionId,
    from: i64,
    to: i64,
) -> Result<(), WorkLogError> {
    let log = work_log.inner().clone();
    tauri::async_runtime::spawn_blocking(move || log.phase(id, from, to, work_log::now_ms()))
        .await
        .map_err(|error| WorkLogError::Task(error.to_string()))?
}

/// The daily review of one local day, `[start, end)` in Unix ms as the window computed it
/// (daily_review.rs, ADR 0011).
#[tauri::command]
pub async fn daily_review(
    work_log: State<'_, WorkLog>,
    start: i64,
    end: i64,
) -> Result<DailyReview, ReviewError> {
    let log = work_log.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let git = Git::locate().ok();
        daily_review::review(&log, git.as_ref(), start, end, work_log::now_ms())
    })
    .await
    .map_err(|error| ReviewError::Task(error.to_string()))?
}

/// One page of the commits reachable from HEAD of the session's folder (git_history.rs); with
/// `root`, of its repository's main folder (the project overview). `limit` is clamped to 1..=100.
#[tauri::command]
pub async fn git_log(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
    skip: u32,
    limit: u32,
    root: Option<bool>,
) -> Result<GitLog, SessionGitError> {
    if !in_scope(&window, &scope, id) {
        return Err(SessionGitError::UnknownSession(id));
    }
    session_git(&manager, id, root.unwrap_or(false), move |git, cwd| {
        git_history::log(git, cwd, skip, limit)
    })
    .await
}

/// The changes of one commit against its first parent, in the shape of `git_diff`'s answer plus
/// the commit's header. `sha` must be 7 to 64 hex characters.
#[tauri::command]
pub async fn git_commit_diff(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
    sha: String,
) -> Result<GitCommitDiff, SessionGitError> {
    if !in_scope(&window, &scope, id) {
        return Err(SessionGitError::UnknownSession(id));
    }
    session_git(&manager, id, false, move |git, cwd| {
        git_history::commit_diff(git, cwd, &sha)
    })
    .await
}

/// The working folder of a session, for the git actions: the window names only the session.
fn action_folder(manager: &SessionManager, id: SessionId) -> Result<PathBuf, ActionError> {
    manager
        .list()
        .into_iter()
        .find(|info| info.id == id)
        .map(|info| info.cwd)
        .ok_or(ActionError::UnknownSession(id))
}

/// Runs one git action (git_actions.rs) on a blocking thread, in the session's folder.
async fn git_action(
    window: &tauri::Window,
    scope: &PanelScope,
    manager: &SessionManager,
    id: SessionId,
    work: impl FnOnce(&Git, &std::path::Path) -> Result<(), ActionError> + Send + 'static,
) -> Result<(), ActionError> {
    if !in_scope(window, scope, id) {
        return Err(ActionError::UnknownSession(id));
    }
    let cwd = action_folder(manager, id)?;
    tauri::async_runtime::spawn_blocking(move || work(&Git::locate()?, &cwd))
        .await
        .map_err(|error| ActionError::Task(error.to_string()))?
}

/// Stages files of the session's repository; `paths` relative to its root, as `git_diff` lists
/// them. The repository's hooks do not run.
#[tauri::command]
pub async fn git_stage(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
    paths: Vec<String>,
) -> Result<(), ActionError> {
    git_action(&window, &scope, &manager, id, move |git, cwd| {
        git_actions::stage(git, cwd, &paths)
    })
    .await
}

/// Takes files of the session's repository out of the index again; the work tree stays. The
/// repository's hooks do not run.
#[tauri::command]
pub async fn git_unstage(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
    paths: Vec<String>,
) -> Result<(), ActionError> {
    git_action(&window, &scope, &manager, id, move |git, cwd| {
        git_actions::unstage(git, cwd, &paths)
    })
    .await
}

/// Throws away the changes of files (untracked ones are deleted); the window asks first. Paths
/// through links are refused, the repository's hooks do not run.
#[tauri::command]
pub async fn git_discard(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
    paths: Vec<String>,
) -> Result<(), ActionError> {
    git_action(&window, &scope, &manager, id, move |git, cwd| {
        git_actions::discard(git, cwd, &paths)
    })
    .await
}

/// Commits what is staged in the session's repository; hooks run as configured.
#[tauri::command]
pub async fn git_commit(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
    message: String,
) -> Result<(), ActionError> {
    git_action(&window, &scope, &manager, id, move |git, cwd| {
        git_actions::commit(git, cwd, &message)
    })
    .await
}

// Timeline of a session's work tree (git_snapshots.rs). Windows name the session by ID only: its
// folder comes from the session manager and its workspace key from the binding the main window
// sets with `snapshot_bind`, so the panel window can reach no other session's snapshots.

/// Emitted to every window with the session ID whenever its snapshots change.
pub const SNAPSHOTS_CHANGED: &str = "snapshots-changed";

/// Runs one snapshot call on a blocking thread, with the side repositories below Hangar's local
/// data folder.
async fn snapshot_call<T: Send + 'static>(
    app: tauri::AppHandle,
    work: impl FnOnce(&Snapshots, &SnapshotEnv<'_>) -> Result<T, SnapshotError> + Send + 'static,
) -> Result<T, SnapshotError> {
    tauri::async_runtime::spawn_blocking(move || {
        let folder = app
            .path()
            .app_local_data_dir()
            .map_err(|error| SnapshotError::DataFolder(error.to_string()))?
            .join(git_snapshots::FOLDER);
        let git = Git::locate()?;
        work(
            app.state::<Snapshots>().inner(),
            &SnapshotEnv::new(&git, &folder),
        )
    })
    .await
    .map_err(|error| SnapshotError::Task(error.to_string()))?
}

/// The running sessions with their folders, and which of them the window reports as working.
fn snapshot_neighbours(manager: &SessionManager, team: &TeamService) -> Neighbours {
    Neighbours {
        sessions: manager
            .list()
            .into_iter()
            .map(|info| (info.id, info.cwd))
            .collect(),
        working: team.working_sessions(),
    }
}

fn snapshots_changed(app: &tauri::AppHandle, id: SessionId) {
    // A window that misses it reads the timeline again on its next refresh.
    let _ = app.emit(SNAPSHOTS_CHANGED, id);
}

/// Binds the session's workspace key, which names its snapshots across restarts; the main window
/// calls it once it knows both. A session keeps its key, and a key of another running session is
/// refused.
#[tauri::command]
pub fn snapshot_bind(
    snapshots: State<'_, Snapshots>,
    manager: Manager<'_>,
    id: SessionId,
    key: String,
) -> Result<(), SnapshotError> {
    let sessions = manager.list();
    let cwd = sessions
        .iter()
        .find(|info| info.id == id)
        .map(|info| info.cwd.clone())
        .ok_or(SnapshotError::UnknownSession(id))?;
    snapshots.bind(id, &key, &cwd, &|other| {
        sessions.iter().any(|info| info.id == other)
    })
}

/// Takes a snapshot of the session's work tree after an agent step (the window calls it when the
/// session stops working). `prompt`: the agent's last prompt, kept cut with the snapshot.
#[tauri::command]
pub async fn snapshot_take(
    app: tauri::AppHandle,
    id: SessionId,
    prompt: Option<String>,
) -> Result<TakeOutcome, SnapshotError> {
    let notify = app.clone();
    let outcome = snapshot_call(app, move |snapshots, env| {
        snapshots.take(env, id, prompt.as_deref().unwrap_or_default())
    })
    .await;
    // A skipped or failed step changes the timeline's note too.
    if !matches!(
        outcome,
        Ok(TakeOutcome::Unchanged | TakeOutcome::NotARepository)
    ) {
        snapshots_changed(&notify, id);
    }
    outcome
}

/// The session's snapshots, newest first, why its newest step was not saved, and the other
/// sessions in its work tree.
#[tauri::command]
pub async fn snapshot_list(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    app: tauri::AppHandle,
    manager: Manager<'_>,
    team: State<'_, TeamService>,
    id: SessionId,
) -> Result<SnapshotList, SnapshotError> {
    if !in_scope(&window, &scope, id) {
        return Err(SnapshotError::UnknownSession(id));
    }
    let neighbours = snapshot_neighbours(&manager, &team);
    snapshot_call(app, move |snapshots, env| {
        snapshots.list(env, id, &neighbours)
    })
    .await
}

/// What one step changed, in the shape of `git_diff`'s answer.
#[tauri::command]
pub async fn snapshot_diff(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    app: tauri::AppHandle,
    id: SessionId,
    number: u64,
) -> Result<GitDiff, SnapshotError> {
    if !in_scope(&window, &scope, id) {
        return Err(SnapshotError::UnknownSession(id));
    }
    snapshot_call(app, move |snapshots, env| snapshots.diff(env, id, number)).await
}

/// Makes the session's work tree match a snapshot after saving the current state; the window
/// asks first. Refused while a session in the same work tree is working; paths through links,
/// files that need a filter and folders in the way are refused, hooks and filters do not run.
#[tauri::command]
pub async fn snapshot_restore(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    app: tauri::AppHandle,
    manager: Manager<'_>,
    team: State<'_, TeamService>,
    id: SessionId,
    number: u64,
) -> Result<RestoreOutcome, SnapshotError> {
    if !in_scope(&window, &scope, id) {
        return Err(SnapshotError::UnknownSession(id));
    }
    let neighbours = snapshot_neighbours(&manager, &team);
    let notify = app.clone();
    let outcome = snapshot_call(app, move |snapshots, env| {
        snapshots.restore(env, id, number, &neighbours)
    })
    .await;
    // Even a failed jump may have saved the current state as its undo point.
    snapshots_changed(&notify, id);
    outcome
}

/// Deletes the session's snapshots and its key; the window calls it once the session is closed,
/// or on quit when sessions are not restored. Works after the session ended: the binding keeps
/// its folder.
#[tauri::command]
pub async fn snapshot_forget(app: tauri::AppHandle, id: SessionId) -> Result<(), SnapshotError> {
    let notify = app.clone();
    snapshot_call(app, move |snapshots, env| snapshots.forget(env, id)).await?;
    snapshots_changed(&notify, id);
    Ok(())
}

/// One folder of a session's file tree (file_tree.rs). The window names only the session and a
/// `/` separated path below its folder; the folder itself comes from the session manager. A
/// folder that does not answer within [`file_tree::COMMAND_TIMEOUT`] is reported as an error;
/// its thread keeps the folder claimed until it ends.
#[tauri::command]
pub async fn list_session_folder(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    tree: State<'_, FileTree>,
    id: SessionId,
    relative: String,
) -> Result<FolderListing, FileTreeError> {
    if !in_scope(&window, &scope, id) {
        return Err(FileTreeError::UnknownSession(id));
    }
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

// Paths and addresses clicked in the terminal (path_links.rs).

/// File, folder or nothing for each of at most 64 local absolute paths a session printed, so the
/// terminal underlines only paths that exist. Never reads a file.
#[tauri::command]
pub async fn stat_paths(paths: Vec<String>) -> Result<Vec<Option<PathKind>>, PathLinkError> {
    if paths.len() > path_links::MAX_PATHS {
        return Err(PathLinkError::TooMany);
    }
    let task = tauri::async_runtime::spawn_blocking(move || path_links::kinds(&paths));
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| PathLinkError::Task(error.to_string()))?,
        Err(_) => Err(PathLinkError::TimedOut),
    }
}

/// Opens a path clicked in the terminal: a folder in the file manager, a file in its default
/// program when it is plainly harmless (path_links.rs), anything else selected in the file
/// manager. Only from the focused window, since it follows a click.
#[tauri::command]
pub async fn open_path(
    window: tauri::WebviewWindow,
    path: String,
) -> Result<Opened, PathLinkError> {
    if !window.is_focused().unwrap_or(false) {
        return Err(PathLinkError::NotFocused);
    }
    let task = tauri::async_runtime::spawn_blocking(move || path_links::open(&path));
    match tokio::time::timeout(file_tree::COMMAND_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|error| PathLinkError::Task(error.to_string()))?,
        Err(_) => Err(PathLinkError::TimedOut),
    }
}

/// Opens a plain http(s) address clicked in the terminal in the default browser; only from the
/// focused window.
#[tauri::command]
pub async fn open_web_url(window: tauri::WebviewWindow, url: String) -> Result<(), PathLinkError> {
    if !window.is_focused().unwrap_or(false) {
        return Err(PathLinkError::NotFocused);
    }
    let url = path_links::web_url(&url).ok_or(PathLinkError::NotWeb)?;
    tauri::async_runtime::spawn_blocking(move || desktop::open_url(&url))
        .await
        .map_err(|error| PathLinkError::Task(error.to_string()))?
        .map_err(PathLinkError::Open)
}

/// Every path below a session's folder for the file tree's filter (file_tree.rs, `PathIndex`):
/// bounded in number, depth and time, ignored files left out. Takes only the session's ID.
#[tauri::command]
pub async fn list_session_paths(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    tree: State<'_, FileTree>,
    id: SessionId,
) -> Result<PathIndex, FileTreeError> {
    if !in_scope(&window, &scope, id) {
        return Err(FileTreeError::UnknownSession(id));
    }
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
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: SessionId,
) -> Result<(), FileTreeError> {
    if !in_scope(&window, &scope, id) {
        return Err(FileTreeError::UnknownSession(id));
    }
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
        crate::desktop::open_folder(&cwd).map_err(FileTreeError::Io)
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
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    viewers: State<'_, Viewers>,
    session_id: SessionId,
    path: String,
) -> Result<(), FileViewerError> {
    file_viewer::check_opener(window.label())?;
    if !scope.allows(window.label(), session_id) {
        return Err(FileViewerError::UnknownSession(session_id));
    }
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

/// The new worktree counts as in use for the cleanup until a session works in it.
#[tauri::command]
pub async fn create_worktree(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    cwd: PathBuf,
    label: String,
) -> Result<Worktree, WorktreeError> {
    // No session exists yet, so the folder comes from the window; at least never a share, a
    // device or a link to one, which git would contact before any other check
    // (no_links::folder_place).
    if crate::no_links::folder_place(&cwd) == crate::no_links::FolderPlace::Remote {
        return Err(GitError::InvalidPath("a network share or device path").into());
    }
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        let created = worktree::create(&Git::locate()?, &base, &cwd, &label)?;
        app.state::<worktree_cleanup::PendingWorktrees>()
            .add(&created.path, std::time::Instant::now());
        Ok(created)
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
        app.state::<worktree_cleanup::PendingWorktrees>()
            .remove(&worktree.path);
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

// Worktree cleanup (worktree_cleanup.rs). Contract: agent-workflow-features.md, "Worktree
// cleanup".

/// Folders that occupy a worktree: every session's folder (running or ended, still in the
/// list), every saved session's folder and worktree, which a restart brings back, and the
/// worktrees created for a session that has not started yet ([`worktree_cleanup::PendingWorktrees`]).
fn occupied_folders(app: &tauri::AppHandle, store: &Store) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut occupied: Vec<PathBuf> = app
        .state::<SessionManager>()
        .list()
        .into_iter()
        .map(|info| info.cwd)
        .collect();
    let mut roots = Vec::new();
    if let Some(workspace) = store.load_workspace() {
        for session in workspace.sessions {
            occupied.push(session.cwd);
            if let Some(worktree) = session.worktree {
                occupied.push(worktree.path);
                roots.push(worktree.repo_root);
            }
        }
    }
    let occupied = app
        .state::<worktree_cleanup::PendingWorktrees>()
        .with_pending(
            occupied,
            std::time::Instant::now(),
            worktree_cleanup::PENDING_TIMEOUT,
        );
    (occupied, roots)
}

/// The worktrees Hangar created, each with its state. `folders` are the window's folders whose
/// repositories count too (recent folders, favorites, sessions' repositories), at most
/// [`worktree_cleanup::MAX_FOLDERS`]; a missing worktree folder is only found through them.
#[tauri::command]
pub async fn list_hangar_worktrees(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    folders: Vec<PathBuf>,
) -> Result<Vec<worktree_cleanup::CleanupEntry>, WorktreeError> {
    if folders.len() > worktree_cleanup::MAX_FOLDERS {
        return Err(GitError::TooManyPaths.into());
    }
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let git = Git::locate()?;
        let base = worktrees_dir(&store);
        let (occupied, roots) = occupied_folders(&app, &store);
        let cx = worktree_cleanup::Context {
            git: &git,
            base: &base,
            occupied: &occupied,
        };
        Ok(worktree_cleanup::list(&cx, &folders, &roots))
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

/// Size of one worktree folder below Hangar's worktrees folder, bounded in files and time.
#[tauri::command]
pub async fn hangar_worktree_size(
    store: State<'_, Store>,
    path: PathBuf,
) -> Result<worktree_cleanup::WorktreeSize, WorktreeError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        worktree_cleanup::check_measurable(&base, &path)?;
        Ok(worktree_cleanup::measure(
            &path,
            worktree_cleanup::SIZE_MAX_FILES,
            worktree_cleanup::SIZE_BUDGET,
        ))
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

/// Checks the state again and removes the worktree only when it is still merged or clean.
#[tauri::command]
pub async fn remove_hangar_worktree(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    path: PathBuf,
    repo_root: PathBuf,
) -> Result<worktree_cleanup::CleanupOutcome, WorktreeError> {
    clean_up_worktree(
        app,
        &store,
        path,
        repo_root,
        worktree_cleanup::Action::Remove,
    )
    .await
}

/// Checks the state again and drops git's record only when the folder is still missing.
#[tauri::command]
pub async fn prune_hangar_worktree(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    path: PathBuf,
    repo_root: PathBuf,
) -> Result<worktree_cleanup::CleanupOutcome, WorktreeError> {
    clean_up_worktree(
        app,
        &store,
        path,
        repo_root,
        worktree_cleanup::Action::Prune,
    )
    .await
}

/// Like [`remove_worktree`], the claims let go of the folder first and take it back when it
/// stays.
async fn clean_up_worktree(
    app: tauri::AppHandle,
    store: &Store,
    path: PathBuf,
    repo_root: PathBuf,
    action: worktree_cleanup::Action,
) -> Result<worktree_cleanup::CleanupOutcome, WorktreeError> {
    let store = store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let git = Git::locate()?;
        let base = worktrees_dir(&store);
        worktree::check_managed(&base, &path, &repo_root)?;
        let (occupied, _roots) = occupied_folders(&app, &store);
        let cx = worktree_cleanup::Context {
            git: &git,
            base: &base,
            occupied: &occupied,
        };
        let claims = app.state::<ClaimsService>();
        let released = claims.release(&path);
        let outcome = worktree_cleanup::clean_up(&cx, &path, &repo_root, action);
        if !outcome.as_ref().is_ok_and(|outcome| outcome.done) {
            claims.unrelease(released);
        }
        outcome
    })
    .await
    .map_err(|error| GitError::Task(error.to_string()))?
}

// Agent arena (arena.rs), on a blocking thread because git and the test command take time.

/// One worktree per agent label from the current commit of the repository around `cwd`. Like
/// `create_worktree`, never on a share or a device, and each new worktree counts as in use for
/// the cleanup until its contestant's session works in it.
#[tauri::command]
pub async fn arena_create(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    cwd: PathBuf,
    labels: Vec<String>,
) -> Result<arena::Arena, ArenaError> {
    if crate::no_links::folder_place(&cwd) == crate::no_links::FolderPlace::Remote {
        return Err(GitError::InvalidPath("a network share or device path").into());
    }
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        let created = arena::create(&Git::locate()?, &base, &cwd, &labels)?;
        let pending = app.state::<worktree_cleanup::PendingWorktrees>();
        let now = std::time::Instant::now();
        for worktree in &created.worktrees {
            pending.add(&worktree.path, now);
        }
        Ok(created)
    })
    .await
    .map_err(|error| ArenaError::from(GitError::Task(error.to_string())))?
}

/// A contestant's changed files and lines against the arena's base commit.
#[tauri::command]
pub async fn arena_diff_stat(
    store: State<'_, Store>,
    worktree: Worktree,
    base_commit: String,
) -> Result<arena::DiffStat, ArenaError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        arena::diff_stat(&Git::locate()?, &base, &worktree, &base_commit)
    })
    .await
    .map_err(|error| ArenaError::from(GitError::Task(error.to_string())))?
}

/// Takes the winner's changes over into the project folder, uncommitted; refuses shared files.
/// `files` is the list the user confirmed; changes that differ from it by now are refused.
#[tauri::command]
pub async fn arena_apply(
    store: State<'_, Store>,
    worktree: Worktree,
    base_commit: String,
    files: Vec<String>,
) -> Result<arena::ApplyOutcome, ArenaError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        arena::apply(&Git::locate()?, &base, &worktree, &base_commit, &files)
    })
    .await
    .map_err(|error| ArenaError::from(GitError::Task(error.to_string())))?
}

/// Removes a contestant's worktree and branch with its changes, after the user confirmed it.
/// The claims let go of the folder first, as for `remove_worktree`.
#[tauri::command]
pub async fn arena_discard(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    worktree: Worktree,
) -> Result<(), ArenaError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        worktree::check_shape(&worktree)?;
        worktree::check_managed(&base, &worktree.path, &worktree.repo_root)?;
        app.state::<worktree_cleanup::PendingWorktrees>()
            .remove(&worktree.path);
        let claims = app.state::<ClaimsService>();
        let released = claims.release(&worktree.path);
        let result = Git::locate()
            .map_err(ArenaError::from)
            .and_then(|git| arena::discard(&git, &base, &worktree));
        if result.is_err() {
            claims.unrelease(released);
        }
        result
    })
    .await
    .map_err(|error| ArenaError::from(GitError::Task(error.to_string())))?
}

/// The user's test command in a contestant's worktree (argv without a shell, with a time limit).
#[tauri::command]
pub async fn arena_run_tests(
    store: State<'_, Store>,
    worktree: Worktree,
    command: String,
) -> Result<arena::TestRun, ArenaError> {
    let base = worktrees_dir(&store);
    tauri::async_runtime::spawn_blocking(move || {
        arena::run_tests(&base, &worktree, &command, arena::TEST_TIMEOUT)
    })
    .await
    .map_err(|error| ArenaError::from(GitError::Task(error.to_string())))?
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
/// the hook; turning it off lets every waiting edit through. Emits the payload-free `gate-changed`
/// signal whenever the waiting edits change; the window then reads `gate_snapshot`.
#[tauri::command]
pub fn configure_gate(gate: State<'_, GateService>, enabled: bool) {
    gate.configure(enabled);
}

#[tauri::command]
pub fn gate_snapshot(gate: State<'_, GateService>) -> GateSnapshot {
    gate.snapshot()
}

/// The person's answer to a waiting edit; `false` when it no longer waits (answered, timed out,
/// session ended). On a blocking thread: an "always" answer writes the rules file.
#[tauri::command]
pub async fn answer_gate(
    gate: State<'_, GateService>,
    id: u64,
    answer: Answer,
) -> Result<bool, ()> {
    let gate = gate.inner().clone();
    Ok(
        tauri::async_runtime::spawn_blocking(move || gate.answer(id, answer))
            .await
            .unwrap_or(false),
    )
}

/// The person's gate rules, per project (`gate_rules.rs`).
#[tauri::command]
pub fn gate_rules(gate: State<'_, GateService>) -> Vec<GateRule> {
    gate.rules()
}

/// Deletes a gate rule; `false` when no rule has that id. On a blocking thread: it rewrites the
/// rules file.
#[tauri::command]
pub async fn delete_gate_rule(gate: State<'_, GateService>, id: u64) -> Result<bool, ()> {
    let gate = gate.inner().clone();
    Ok(
        tauri::async_runtime::spawn_blocking(move || gate.delete_rule(id))
            .await
            .unwrap_or(false),
    )
}

/// Turns the command guard on or off (the window's setting; on by default). It applies at once,
/// also to running team sessions; turning it off lets every held shell command go on.
#[tauri::command]
pub fn configure_command_guard(gate: State<'_, GateService>, enabled: bool) {
    gate.configure_command_guard(enabled);
}

/// The command guard's rules file, its rules and what could not be read in it. On a blocking
/// thread: it reads the rules file.
#[tauri::command]
pub async fn command_guard_info(gate: State<'_, GateService>) -> Result<GuardInfo, String> {
    let gate = gate.inner().clone();
    tauri::async_runtime::spawn_blocking(move || gate.command_guard_info())
        .await
        .map_err(|error| error.to_string())
}

// Hangar account (account.rs, account_service.rs). Contract: hangar-account-api.md. Every change
// of the state is signalled as the payload-free `account-changed`; the window then reads
// `account_status`.

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

/// The signed-in person's profile picture as a `data:` URL, `null` without one. Separate from
/// the view, which carries only `user.avatarVersion`, so every status and event stays small.
#[tauri::command]
pub fn account_avatar(account: State<'_, AccountService>) -> Option<String> {
    account.avatar()
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

/// Opens one of the account's pages; the window names the page, never a URL. On a blocking
/// thread: starting the browser may take a while.
#[tauri::command]
pub async fn account_open(
    account: State<'_, AccountService>,
    page: AccountPage,
) -> Result<(), AccountError> {
    let account = account.inner().clone();
    tauri::async_runtime::spawn_blocking(move || account.open(page))
        .await
        .map_err(|error| AccountError::Open(error.to_string()))?
}

// Team connection (team.rs, team_engine.rs, team_service.rs; step F part 2a). Every change is
// signalled as the payload-free `team-changed`; the main window reads `team_snapshot`.

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

/// Every team's connection, the pending confirmations and the repositories to link: what the
/// payload-free `team-changed` signal asks the main window to fetch.
#[tauri::command]
pub fn team_snapshot(team: State<'_, TeamService>) -> crate::team::TeamSnapshot {
    team.snapshot()
}

/// This device joined the team the person expected: they compared the admitting device's and
/// the log's safety numbers. Only then does the device share anything. `log_index` and
/// `safety_number` are the numbers the window showed; a team that moved on refuses them.
#[tauri::command]
pub async fn team_confirm_admission(
    team: State<'_, TeamService>,
    project_id: String,
    log_index: u64,
    safety_number: Vec<String>,
) -> Result<(), TeamError> {
    team.confirm_admission(&project_id, log_index, safety_number)
        .await
}

/// Removes another device of the signed-in person from the team (a lost one, for example).
#[tauri::command]
pub async fn team_remove_device(
    team: State<'_, TeamService>,
    project_id: String,
    device: String,
) -> Result<(), TeamError> {
    team.remove_device(&project_id, &device).await
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

/// Links or unlinks a repository folder (its root commit and clone folder, from `team_snapshot`'s
/// `repos`) to a shared project: its sessions' presence and changed files go to the team from
/// now on. `folder: null` names a link from before folders were bound, which can only be
/// removed (`on: false`).
/// On a blocking thread: it writes the team settings.
#[tauri::command]
pub async fn team_link_repo(
    team: State<'_, TeamService>,
    project_id: String,
    root: String,
    folder: Option<String>,
    on: bool,
) -> Result<(), TeamError> {
    let team = team.inner().clone();
    tauri::async_runtime::spawn_blocking(move || match (folder, on) {
        (Some(folder), on) => team.link_repo(&project_id, &root, &folder, on),
        (None, false) => team.unlink_legacy_repo(&project_id, &root),
        (None, true) => Err(TeamError::UnknownRepository),
    })
    .await
    // A task that could not finish: nothing was linked, the window may try again.
    .map_err(|_| TeamError::Busy)?
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
/// is the task's keyed hash from `team_snapshot`.
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

/// Saves the conversation of a session as a Markdown report read from the agent's own
/// transcript (report/). The backend renders it, then opens the save dialog itself. `None` when
/// the dialog was cancelled. `utcOffsetMinutes`: the window's offset from UTC for the times shown;
/// `labels`: the report's headings in the window's language.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export_report(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    manager: Manager<'_>,
    session_id: SessionId,
    title: String,
    suggested_name: String,
    utc_offset_minutes: i32,
    labels: report::Labels,
) -> Result<Option<String>, ReportError> {
    use tauri_plugin_dialog::DialogExt as _;
    let (target, program) = manager.report_target(session_id)?;
    let source = report::Source {
        target,
        program,
        title,
    };
    let labels = labels.cleaned();
    let name = export::report_file_name(&suggested_name);
    tauri::async_runtime::spawn_blocking(move || {
        let content = report::build(&source, &labels, utc_offset_minutes)?;
        let chosen = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(name)
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
    .map_err(|error| ReportError::Task(error.to_string()))?
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

// Main window only (main_store.rs, panel_window.rs): state the panel and file viewer windows,
// which share the main window's origin and storage, must not be able to write or read.

/// The security switches as stored; `null` when nothing was stored yet (the window then migrates
/// its old values once).
#[tauri::command]
pub async fn security_switches(
    window: tauri::Window,
    store: State<'_, MainStore>,
) -> Result<Option<SecuritySwitches>, MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.load_switches())
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))
}

#[tauri::command]
pub async fn save_security_switches(
    window: tauri::Window,
    store: State<'_, MainStore>,
    switches: SecuritySwitches,
) -> Result<(), MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.save_switches(switches))
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))?
}

/// The queued messages per session key; `null` when nothing was stored yet.
#[tauri::command]
pub async fn load_message_queue(
    window: tauri::Window,
    store: State<'_, MainStore>,
) -> Result<Option<QueuedMessages>, MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.load_queue())
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))
}

/// Replaces the stored queued messages; an empty map removes them all.
#[tauri::command]
pub async fn save_message_queue(
    window: tauri::Window,
    store: State<'_, MainStore>,
    entries: QueuedMessages,
) -> Result<(), MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.save_queue(entries))
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))?
}

/// The settings that decide what reaches an agent or the machine (paste warning, automatic
/// updates, HANGAR.md, snippets); `null` when nothing was stored yet.
#[tauri::command]
pub async fn protected_settings(
    window: tauri::Window,
    store: State<'_, MainStore>,
) -> Result<Option<ProtectedSettings>, MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.load_settings())
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))
}

#[tauri::command]
pub async fn save_protected_settings(
    window: tauri::Window,
    store: State<'_, MainStore>,
    settings: ProtectedSettings,
) -> Result<(), MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.save_settings(settings))
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))?
}

/// Where and how the next session starts and whether a waiting session shows itself (start
/// mode, last program, folders, session restore, notices); `null` when nothing was stored yet.
#[tauri::command]
pub async fn main_prefs(
    window: tauri::Window,
    store: State<'_, MainStore>,
) -> Result<Option<MainPrefs>, MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.load_prefs())
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))
}

#[tauri::command]
pub async fn save_main_prefs(
    window: tauri::Window,
    store: State<'_, MainStore>,
    prefs: MainPrefs,
) -> Result<(), MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.save_prefs(prefs))
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))?
}

/// The workspace templates; `null` when nothing was stored yet.
#[tauri::command]
pub async fn load_templates(
    window: tauri::Window,
    store: State<'_, MainStore>,
) -> Result<Option<Vec<WorkspaceTemplate>>, MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.load_templates())
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))
}

/// Replaces the stored workspace templates; an empty list removes them all.
#[tauri::command]
pub async fn save_templates(
    window: tauri::Window,
    store: State<'_, MainStore>,
    templates: Vec<WorkspaceTemplate>,
) -> Result<(), MainStoreError> {
    main_store::check_main(window.label())?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.save_templates(templates))
        .await
        .map_err(|error| MainStoreError::Task(error.to_string()))?
}

/// The session the panel window shows from now on (`null`: none). The main window calls it
/// before it sends the panel its context; the panel's session commands answer for this session
/// only, its shared notes for this session's working folder only ([`PanelScope`]). The folder
/// comes from the session manager; an id it does not know hands the panel nothing.
#[tauri::command]
pub fn set_panel_session(
    window: tauri::Window,
    scope: State<'_, PanelScope>,
    manager: Manager<'_>,
    id: Option<SessionId>,
) -> Result<(), PanelWindowError> {
    panel_window::check_main(window.label())?;
    let handed = id.and_then(|id| session_folder(&manager, id).ok().map(|cwd| (id, cwd)));
    scope.set(handed.as_ref().map(|(id, cwd)| (*id, cwd.as_path())));
    Ok(())
}

/// The results of finished update sessions not yet taken, each handed out once. The
/// `program-update-result` signal says that one is waiting.
#[tauri::command]
pub fn take_update_results(
    window: tauri::Window,
    results: State<'_, UpdateResults>,
) -> Result<Vec<updates::UpdateOutcome>, PanelWindowError> {
    panel_window::check_main(window.label())?;
    Ok(results.take())
}

// ------------------------------------------------------------------------------------------
// Local dictation (dictation/)
// ------------------------------------------------------------------------------------------

/// The capabilities grant dictation to the main window alone; this holds even if a capability
/// file grew by mistake.
fn dictation_main(window: &tauri::Window) -> Result<(), DictationError> {
    panel_window::check_main(window.label()).map_err(|_| DictationError::WrongWindow)
}

/// State of dictation for the model the window chose, which models are installed and whether
/// a microphone is there. Off the main thread: asking the audio system for its default input
/// device may take a moment.
#[tauri::command]
pub async fn dictation_status(
    window: tauri::Window,
    app: tauri::AppHandle,
    model: ModelId,
) -> Result<DictationStatus, DictationError> {
    dictation_main(&window)?;
    tauri::async_runtime::spawn_blocking(move || app.state::<DictationService>().status(model))
        .await
        .map_err(|_| DictationError::Microphone)
}

/// Opens the microphone; the text later waits for session `id` (`dictation_take_text`).
/// Async because opening the device takes a moment.
#[tauri::command]
pub async fn dictation_start(
    window: tauri::Window,
    app: tauri::AppHandle,
    id: SessionId,
    model: ModelId,
    language: Language,
) -> Result<(), DictationError> {
    dictation_main(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let known = app
            .state::<SessionManager>()
            .list()
            .iter()
            .any(|info| info.id == id);
        if !known {
            return Err(DictationError::UnknownSession);
        }
        app.state::<DictationService>().start(id, model, language)
    })
    .await
    .map_err(|_| DictationError::Microphone)?
}

/// Closes the microphone and transcribes; nothing happens when no recording runs.
#[tauri::command]
pub fn dictation_stop(
    window: tauri::Window,
    dictation: State<'_, DictationService>,
) -> Result<(), DictationError> {
    dictation_main(&window)?;
    dictation.stop();
    Ok(())
}

/// The transcripts not yet taken, oldest first, each handed out once. The `dictation-text`
/// signal says that one is waiting.
#[tauri::command]
pub fn dictation_take_text(
    window: tauri::Window,
    dictation: State<'_, DictationService>,
) -> Result<Vec<DictationText>, DictationError> {
    dictation_main(&window)?;
    Ok(dictation.take_texts())
}

/// Downloads a speech model (pinned file and SHA-256). The window asks for consent first.
#[tauri::command]
pub async fn dictation_install_model(
    window: tauri::Window,
    app: tauri::AppHandle,
    model: ModelId,
) -> Result<(), DictationError> {
    dictation_main(&window)?;
    let service = app.state::<DictationService>().inner().clone();
    dictation::install(&service, model).await
}

/// Deletes a speech model's file.
#[tauri::command]
pub fn dictation_remove_model(
    window: tauri::Window,
    dictation: State<'_, DictationService>,
    model: ModelId,
) -> Result<(), DictationError> {
    dictation_main(&window)?;
    dictation.remove(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The git reads find the folder through the session; an id the manager does not know is
    /// refused before git runs anywhere.
    #[test]
    fn git_reads_refuse_an_unknown_session() {
        let manager = SessionManager::new(Vec::new(), Vec::new(), std::env::temp_dir());
        let error = session_folder(&manager, 42).unwrap_err();
        assert!(matches!(error, SessionGitError::UnknownSession(42)));
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!("Unknown session 42")
        );
    }

    #[test]
    fn update_results_are_handed_out_once_and_bounded() {
        let results = UpdateResults::default();
        let outcome =
            |id: &str| updates::outcome(id, Some(1), Some(0), None, None, false, Vec::new());
        results.push(outcome("a"));
        results.push(outcome("b"));
        let taken: Vec<String> = results.take().into_iter().map(|o| o.profile_id).collect();
        assert_eq!(taken, ["a", "b"]);
        assert!(results.take().is_empty());
        for i in 0..=UpdateResults::MAX {
            results.push(outcome(&i.to_string()));
        }
        let taken = results.take();
        assert_eq!(taken.len(), UpdateResults::MAX);
        assert_eq!(taken[0].profile_id, "1", "the oldest went first");
    }
}
