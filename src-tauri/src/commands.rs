//! Thin Tauri commands. They only forward to the [`SessionManager`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Emitter as _, Manager as _, State};

use crate::agent_catalog::ProfileView;
use crate::desktop;
use crate::git::{self, Git, GitDiff, GitError};
use crate::icon::{self, IconCache};
use crate::manager::{EventSink, ManagerError, SessionEvent, SessionManager, TermSize};
use crate::notes::{FolderInfo, NoteFolder, NoteInfo, NotesError, NotesStore};
use crate::notify::{self, Notifier};
use crate::persist::{PersistError, Store, Workspace};
use crate::profile::{self, ProfileProblem};
use crate::session::{SessionId, SessionInfo, SessionKind};
use crate::updates::{self, Item, UpdateCache, UpdateInfo};
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

/// For update sessions: forget the remembered versions before sending `exit` to the window. The
/// frontend checks again once the session ends and should then see the new version, even if the
/// launcher script's file stamp stayed the same (npm swaps the package, not `gemini.cmd`).
fn update_sink(app: tauri::AppHandle, channel: Channel<SessionEvent>) -> impl EventSink {
    move |event| {
        if matches!(event, SessionEvent::Exit { .. }) {
            app.state::<ProbeCaches>().versions.clear();
        }
        let _ = channel.send(event);
    }
}

/// Async because the PATH search runs on every call and grows with the number of entries times
/// the number of PATH folders. Synchronous commands run on the main thread and would make the
/// window stutter.
#[tauri::command]
pub async fn list_profiles(
    load: State<'_, ProfileLoad>,
    manager: Manager<'_>,
) -> Result<Vec<ProfileView>, String> {
    let error = crate::lock(&load.error).clone();
    match error {
        Some(error) => Err(error),
        None => Ok(manager.profiles()),
    }
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
        let installed: Vec<_> = handle
            .state::<SessionManager>()
            .installed()
            .into_iter()
            .filter(|(agent, _)| agent.profile.latest.is_some())
            .collect();
        let versions = version::detect(&installed, &handle.state::<ProbeCaches>().versions);
        let items: Vec<Item> = installed
            .iter()
            .filter_map(|(agent, _)| Item::of(agent, versions.get(agent.id()).cloned().flatten()))
            .collect();
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
) -> Result<SessionInfo, ManagerError> {
    // Optional, so existing calls without `resume` stay valid.
    manager.create_with(
        &profile_id,
        cwd,
        size,
        resume.unwrap_or(false),
        channel_sink(on_event),
    )
}

/// Starts a program update as a session of its own. The frontend only names the profile; the
/// command comes solely from its `update_hint` (contract:
/// docs/wiki/pages/syntheses/program-updates.md).
#[tauri::command]
pub fn create_update_session(
    app: tauri::AppHandle,
    manager: Manager<'_>,
    profile_id: String,
    size: TermSize,
    on_event: Channel<SessionEvent>,
) -> Result<SessionInfo, ManagerError> {
    manager.create_update(&profile_id, size, update_sink(app, on_event))
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
    id: SessionId,
    size: TermSize,
    on_event: Channel<SessionEvent>,
) -> Result<SessionInfo, ManagerError> {
    // Restarting an update session repeats the update; after that it behaves like a start.
    if manager.kind(id) == Some(SessionKind::Update) {
        manager.restart(id, size, update_sink(app, on_event))
    } else {
        manager.restart(id, size, channel_sink(on_event))
    }
}

#[tauri::command]
pub fn rename_session(
    manager: Manager<'_>,
    id: SessionId,
    title: String,
) -> Result<SessionInfo, ManagerError> {
    manager.rename(id, &title)
}

#[tauri::command]
pub fn close_session(manager: Manager<'_>, id: SessionId) -> Result<(), ManagerError> {
    manager.close(id)
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
    if let Some(window) = app.get_webview_window("main") {
        // Each step may fail harmlessly (already visible, not minimized).
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    let _ = app.emit(
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
