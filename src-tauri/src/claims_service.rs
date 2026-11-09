//! Runs the local claims (domain in `claims.rs`): one worker thread that resolves the worktree of
//! every running session, keeps one file watcher per distinct worktree, rescans with git and
//! publishes a [`ClaimsSnapshot`] whenever it changes.
//!
//! Git is the truth, the watcher only a trigger: events are coalesced, a slow poll repairs
//! missed ones (notify 8.2 on Windows unwatches a folder silently after a buffer overflow, see
//! docs/wiki/pages/sources/claims-rescan-measurement.md), and a watcher that cannot start leaves
//! the worktree on the poll alone. Events inside the git folder count only for `HEAD`, `index`
//! and `logs/HEAD`, which catch commits and checkouts; events in top-level folders git ignores
//! (`target`, `node_modules`) do not count at all. One worker means at most one git call of the
//! claims at a time, and a budget across all worktrees keeps git from running most of the time.
//! Before a worktree is removed, [`ClaimsService::release`] closes its watcher and waits for a
//! git call in it, because Windows keeps a watched folder open.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher as _};

use crate::claims::{
    self, folder_key, key_within, ClaimState, ClaimsSnapshot, Coalescer, RootView, ScanBudget,
    ScanReason, ScanResult, SessionClaims, WorktreeClaims, MAX_LISTED_CLAIMS, MAX_LISTED_OVERLAPS,
    POLL_ONLY, POLL_WATCHED,
};
use crate::git::{self, Git, GitError};
use crate::lock;
use crate::manager::SessionManager;
use crate::session::{SessionId, SessionKind, Status};

/// Event to the main window with the newest [`ClaimsSnapshot`].
pub const EVENT: &str = "claims-changed";
/// How often the worker looks at the session list (in memory, no process).
const SESSION_CHECK: Duration = Duration::from_secs(1);
/// A folder that was no repository or failed is resolved again after this long.
const RESOLVE_AGAIN: Duration = POLL_WATCHED;
/// Limit of the `-U0` diff; files past it count as whole files.
const MAX_DIFF_OUTPUT: usize = git::MAX_LIST_BYTES;
/// Names of top-level folders asked about in one `check-ignore`, in bytes (the Windows command
/// line holds 32767 characters); folders past it keep their events.
const MAX_IGNORE_ARGS_BYTES: usize = 8 * 1024;

const STATUS_ARGS: &[&str] = &[
    "status",
    "--porcelain=v2",
    "-z",
    "--branch",
    "--no-renames",
    "--untracked-files=normal",
    "--ignore-submodules=all",
];

const DIFF_ARGS: &[&str] = &[
    "diff",
    "-U0",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
    "--ignore-submodules=all",
    "HEAD",
    "--",
];

/// A session whose changes are observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedSession {
    pub id: SessionId,
    pub cwd: PathBuf,
}

/// Running agent sessions of the manager (update sessions and exited ones are left out).
pub fn watched_sessions(manager: &SessionManager) -> Vec<WatchedSession> {
    manager
        .list()
        .into_iter()
        .filter(|info| info.kind == SessionKind::Agent && info.status != Status::Exited)
        .map(|info| WatchedSession {
            id: info.id,
            cwd: info.cwd,
        })
        .collect()
}

/// The worktree a folder belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootInfo {
    pub top: PathBuf,
    /// The worktree's own git folder (`.git`, or `.git/worktrees/<name>` of a linked worktree).
    pub git_dir: PathBuf,
    /// Repository key: the git common folder, [`folder_key`].
    pub repo: String,
    /// Worktree key, [`folder_key`] of `top`.
    pub key: String,
}

/// Worktree of `cwd`; `None` outside a repository.
pub fn resolve(git: &Git, cwd: &Path) -> Result<Option<RootInfo>, GitError> {
    git::check_folder(cwd)?;
    let deadline = Instant::now() + git::TIMEOUT;
    let output = git.run(
        cwd,
        [
            "rev-parse",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
        deadline,
        64 * 1024,
    )?;
    if !output.success {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(|line| line.trim_end_matches('\r'));
    let (Some(top), Some(git_dir), Some(common)) = (lines.next(), lines.next(), lines.next())
    else {
        return Ok(None);
    };
    if top.is_empty() || git_dir.is_empty() || common.is_empty() {
        return Ok(None);
    }
    // git prints these relative to the folder it ran in, unless they lie elsewhere.
    let absolute = |text: &str| {
        let path = git::native_path(text);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    let top = git::native_path(top);
    let git_dir = absolute(git_dir);
    let common = absolute(common);
    // Before anything below resolves, watches or runs git there.
    git::check_work_tree(cwd, &top, &[&git_dir, &common])?;
    Ok(Some(RootInfo {
        key: folder_key(&top),
        repo: folder_key(&common),
        git_dir,
        top,
    }))
}

/// Top-level folders of the worktree that git ignores (`target`, `node_modules`, ...). A build
/// writes there for minutes, and every write would ask for a rescan that finds nothing new.
/// Names git prints quoted (control characters) do not match and keep their events; tracked
/// files below an ignored folder are still found by the poll.
pub fn ignored_dirs(git: &Git, top: &Path, deadline: Instant) -> Result<Vec<String>, GitError> {
    let entries =
        fs::read_dir(top).map_err(|_| GitError::InvalidPath("the folder cannot be read"))?;
    let mut names = Vec::new();
    let mut bytes = 0;
    for entry in entries.flatten() {
        // The watcher does not follow symlinks either.
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name == ".git" {
            continue;
        }
        bytes += name.len() + 1;
        if bytes > MAX_IGNORE_ARGS_BYTES {
            break;
        }
        names.push(name);
    }
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let args = ["check-ignore", "--"]
        .into_iter()
        .chain(names.iter().map(String::as_str));
    let output = git.run(top, args, deadline, 64 * 1024)?;
    match output.code {
        Some(0) => {}
        // None of them is ignored.
        Some(1) => return Ok(Vec::new()),
        _ => return Err(output.failure()),
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| names.iter().any(|name| name == line))
        .map(str::to_owned)
        .collect())
}

/// `path` is one of the worktree's ignored top-level folders or lies below one.
pub fn in_ignored_dir(path: &Path, top: &Path, ignored: &[String]) -> bool {
    let Ok(rest) = path.strip_prefix(top) else {
        return false;
    };
    rest.components().next().is_some_and(|first| {
        ignored
            .iter()
            .any(|name| first.as_os_str() == OsStr::new(name))
    })
}

/// Changed files of a worktree against its HEAD, with line ranges on the HEAD side: filter
/// overrides, `git status --porcelain=v2` and `git diff -U0`, all through [`Git::run`].
pub fn scan(git: &Git, root: &RootInfo) -> Result<ScanResult, GitError> {
    git::check_folder(&root.top)?;
    let deadline = Instant::now() + git::TIMEOUT;
    let neutral = git::filter_overrides(git, &root.top, deadline)?;
    let status = git.run(
        &root.top,
        neutral
            .iter()
            .map(String::as_str)
            .chain(STATUS_ARGS.iter().copied()),
        deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !status.usable() {
        return Err(status.failure());
    }
    let parsed = claims::parse_status_v2(&status.stdout);
    let (hunks, diff_cut) = if parsed.base.is_some() && !parsed.files.is_empty() {
        let diff = git.run(
            &root.top,
            neutral
                .iter()
                .map(String::as_str)
                .chain(DIFF_ARGS.iter().copied()),
            deadline,
            MAX_DIFF_OUTPUT,
        )?;
        if !diff.usable() {
            return Err(diff.failure());
        }
        (
            Some(claims::parse_diff(&diff.stdout, diff.truncated)),
            diff.truncated,
        )
    } else {
        (None, false)
    };
    let mut result = claims::build_claims(parsed, hunks.as_ref(), diff_cut);
    result.truncated |= status.truncated;
    Ok(result)
}

/// Whether a file event at `path` can change the claims. Inside a git folder only `HEAD`,
/// `index` and `logs/HEAD` count (commits, checkouts, staging); everything else there is git's
/// own bookkeeping, often written by the rescan's neighbours.
pub fn relevant(path: &Path, git_dirs: &[PathBuf]) -> bool {
    for dir in git_dirs {
        let Ok(rest) = path.strip_prefix(dir) else {
            continue;
        };
        let names: Vec<&std::ffi::OsStr> = rest.iter().collect();
        return match names.as_slice() {
            [name] => *name == "HEAD" || *name == "index",
            [folder, name] => *folder == "logs" && *name == "HEAD",
            _ => false,
        };
    }
    true
}

enum Message {
    Enabled(bool),
    Wake,
    Stop,
}

/// Set from the watcher's thread, read by the worker. Only the first event after a read wakes
/// the worker, so a flood of events costs one message.
struct Signal {
    pending: AtomicBool,
    failed: AtomicBool,
    last_ms: AtomicU64,
    epoch: Instant,
    wake: Sender<Message>,
}

impl Signal {
    fn new(wake: Sender<Message>) -> Self {
        Self {
            pending: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            last_ms: AtomicU64::new(0),
            epoch: Instant::now(),
            wake,
        }
    }

    fn hit(&self) {
        let ms = u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.last_ms.store(ms, Ordering::SeqCst);
        if !self.pending.swap(true, Ordering::SeqCst) {
            let _ = self.wake.send(Message::Wake);
        }
    }

    fn fail(&self) {
        self.failed.store(true, Ordering::SeqCst);
        let _ = self.wake.send(Message::Wake);
    }

    /// Time of the newest event since the last call, if there was one.
    fn take(&self) -> Option<Instant> {
        self.pending
            .swap(false, Ordering::SeqCst)
            .then(|| self.epoch + Duration::from_millis(self.last_ms.load(Ordering::SeqCst)))
    }
}

/// Ignored top-level folder names of one worktree, shared with its watcher's thread.
type IgnoredDirs = Arc<Mutex<Vec<String>>>;

fn start_watcher(
    root: &RootInfo,
    signal: Arc<Signal>,
    ignored: IgnoredDirs,
) -> notify::Result<notify::RecommendedWatcher> {
    let git_dirs = vec![root.git_dir.clone(), root.top.join(".git")];
    let top = root.top.clone();
    let handler = move |event: notify::Result<notify::Event>| match event {
        Ok(event) => {
            let ignored = lock(&ignored);
            let counts =
                |path: &PathBuf| relevant(path, &git_dirs) && !in_ignored_dir(path, &top, &ignored);
            if event.need_rescan() || event.paths.iter().any(counts) {
                signal.hit();
            }
        }
        // Includes the inotify watch limit (ENOSPC) reached while new folders are added: the
        // worker then drops the watcher, which frees its watches, and polls.
        Err(_) => signal.fail(),
    };
    // Symlinks are not followed: a link to `/` or to another tree would be watched in full (on
    // Linux every folder below costs an inotify watch).
    let config = notify::Config::default().with_follow_symlinks(false);
    let mut watcher = notify::RecommendedWatcher::new(handler, config)?;
    watcher.watch(&root.top, RecursiveMode::Recursive)?;
    // A linked worktree's git folder lies outside it; HEAD and index live there.
    if !root.git_dir.starts_with(&root.top) {
        watcher.watch(&root.git_dir, RecursiveMode::Recursive)?;
    }
    Ok(watcher)
}

/// State shared between the worker and the commands.
#[derive(Default)]
struct Shared {
    snapshot: Mutex<ClaimsSnapshot>,
    /// All claims per worktree for the agent gate, rebuilt with the snapshot.
    worktrees: Mutex<Arc<Vec<WorktreeClaims>>>,
    /// Watchers per worktree key; here rather than in the worker so [`ClaimsService::release`]
    /// can close one at once.
    watchers: Mutex<HashMap<String, notify::RecommendedWatcher>>,
    /// Folders about to be removed: no watcher, no scan, until no session works there.
    released: Mutex<Vec<String>>,
    /// Held during every scan.
    scan_gate: Mutex<()>,
}

/// A folder handed to [`ClaimsService::release`]. Give it back with
/// [`ClaimsService::unrelease`] when the folder stays after all.
#[must_use]
#[derive(Debug)]
pub struct Released {
    key: String,
}

/// Tauri state of the local claims.
#[derive(Default)]
pub struct ClaimsService {
    tx: Mutex<Option<Sender<Message>>>,
    shared: Arc<Shared>,
}

impl ClaimsService {
    /// Starts the worker, which stays idle until [`Self::configure`] turns it on.
    pub fn start(
        &self,
        sessions: impl Fn() -> Vec<WatchedSession> + Send + 'static,
        emit: impl Fn(&ClaimsSnapshot) + Send + 'static,
    ) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        let worker = Worker {
            shared: Arc::clone(&self.shared),
            rx,
            tx: tx.clone(),
            sessions: Box::new(sessions),
            emit: Box::new(emit),
            enabled: false,
            git: None,
            keys: HashMap::new(),
            places: HashMap::new(),
            roots: HashMap::new(),
            active: Vec::new(),
            budget: ScanBudget::new(Instant::now()),
            dirty: true,
        };
        thread::Builder::new()
            .name("claims".to_owned())
            .spawn(move || worker.run())?;
        *lock(&self.tx) = Some(tx);
        Ok(())
    }

    /// Turns claim watching on or off (the frontend setting).
    pub fn configure(&self, enabled: bool) {
        self.send(Message::Enabled(enabled));
    }

    pub fn snapshot(&self) -> ClaimsSnapshot {
        lock(&self.shared.snapshot).clone()
    }

    /// Every worktree with sessions and a finished scan, with all its claims; empty while the
    /// claims are off. Cheap: the list is shared, not copied.
    pub fn worktrees(&self) -> Arc<Vec<WorktreeClaims>> {
        Arc::clone(&lock(&self.shared.worktrees))
    }

    /// Closes the watchers of `folder` and everything below it and waits for a running git call
    /// there, before the folder is removed. No new watcher, scan or resolve starts there while a
    /// session still works in it, or until [`Self::unrelease`].
    pub fn release(&self, folder: &Path) -> Released {
        let key = folder_key(folder);
        lock(&self.shared.released).push(key.clone());
        let closed: Vec<notify::RecommendedWatcher> = {
            let mut watchers = lock(&self.shared.watchers);
            let keys: Vec<String> = watchers
                .keys()
                .filter(|root| key_within(root, &key))
                .cloned()
                .collect();
            keys.iter().filter_map(|k| watchers.remove(k)).collect()
        };
        // Dropping stops the watch; outside the lock, it may wait for the watcher's thread.
        drop(closed);
        drop(lock(&self.shared.scan_gate));
        self.send(Message::Wake);
        Released { key }
    }

    /// The folder stays after all (the removal failed or kept it): its sessions are watched and
    /// compared again.
    pub fn unrelease(&self, released: Released) {
        {
            let mut list = lock(&self.shared.released);
            if let Some(index) = list.iter().position(|key| *key == released.key) {
                list.remove(index);
            }
        }
        self.send(Message::Wake);
    }

    pub fn stop(&self) {
        self.send(Message::Stop);
        lock(&self.shared.watchers).clear();
    }

    fn send(&self, message: Message) {
        if let Some(tx) = lock(&self.tx).as_ref() {
            let _ = tx.send(message);
        }
    }
}

/// What a session's folder turned out to be.
#[derive(PartialEq, Eq)]
enum PlaceState {
    Root(String),
    NotRepo,
    Failed(String),
}

struct Place {
    state: PlaceState,
    checked: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Events,
    PollOnly,
}

struct Root {
    info: RootInfo,
    signal: Arc<Signal>,
    ignored: IgnoredDirs,
    mode: Mode,
    coalescer: Coalescer,
    result: Option<Result<ScanResult, String>>,
}

struct Worker {
    shared: Arc<Shared>,
    rx: Receiver<Message>,
    tx: Sender<Message>,
    sessions: Box<dyn Fn() -> Vec<WatchedSession> + Send>,
    emit: Box<dyn Fn(&ClaimsSnapshot) + Send>,
    enabled: bool,
    git: Option<Git>,
    /// [`folder_key`] per session folder: resolving touches the folder, which can hang on a
    /// network path, so it happens once per folder rather than every second.
    keys: HashMap<PathBuf, String>,
    /// Per session folder, as the session list names it.
    places: HashMap<PathBuf, Place>,
    /// Per worktree key.
    roots: HashMap<String, Root>,
    /// Sessions observed in the last check, in the session list's order.
    active: Vec<WatchedSession>,
    budget: ScanBudget,
    /// Something the snapshot shows changed since it was last built.
    dirty: bool,
}

impl Worker {
    fn run(mut self) {
        let mut next_sessions = Instant::now();
        loop {
            let message = if self.enabled {
                let wake_at = self.next_deadline().min(next_sessions);
                match self
                    .rx
                    .recv_timeout(wake_at.saturating_duration_since(Instant::now()))
                {
                    Ok(message) => message,
                    Err(RecvTimeoutError::Timeout) => Message::Wake,
                    Err(RecvTimeoutError::Disconnected) => Message::Stop,
                }
            } else {
                self.rx.recv().unwrap_or(Message::Stop)
            };
            match message {
                Message::Stop => break,
                Message::Enabled(enabled) => {
                    if enabled != self.enabled {
                        self.enabled = enabled;
                        self.clear();
                        next_sessions = Instant::now();
                    }
                }
                Message::Wake => {}
            }
            if self.enabled {
                let now = Instant::now();
                if now >= next_sessions {
                    self.sync(now);
                    next_sessions = now + SESSION_CHECK;
                }
                self.collect_signals();
                self.scan_due();
            }
            self.publish();
        }
        self.clear();
    }

    fn next_deadline(&self) -> Instant {
        self.roots
            .values()
            .map(|root| root.coalescer.deadline())
            .min()
            .map_or_else(
                || Instant::now() + SESSION_CHECK,
                |due| due.max(self.budget.not_before()),
            )
    }

    fn clear(&mut self) {
        self.roots.clear();
        self.keys.clear();
        self.places.clear();
        self.active.clear();
        self.dirty = true;
        lock(&self.shared.watchers).clear();
    }

    fn is_released(&self, key: &str) -> bool {
        lock(&self.shared.released)
            .iter()
            .any(|folder| key_within(key, folder))
    }

    fn git(&mut self) -> Result<Git, GitError> {
        if let Some(git) = &self.git {
            return Ok(git.clone());
        }
        let git = Git::locate()?;
        self.git = Some(git.clone());
        Ok(git)
    }

    /// Reads the session list, resolves new folders, starts and drops worktrees and watchers.
    fn sync(&mut self, now: Instant) {
        let sessions = (self.sessions)();
        self.keys
            .retain(|cwd, _| sessions.iter().any(|session| session.cwd == *cwd));
        let keys: Vec<String> = sessions
            .iter()
            .map(|session| {
                self.keys
                    .entry(session.cwd.clone())
                    .or_insert_with(|| folder_key(&session.cwd))
                    .clone()
            })
            .collect();
        let released = {
            let mut released = lock(&self.shared.released);
            released.retain(|folder| keys.iter().any(|key| key_within(key, folder)));
            released.clone()
        };
        let is_released = |key: &str| released.iter().any(|folder| key_within(key, folder));
        let active: Vec<WatchedSession> = sessions
            .into_iter()
            .zip(&keys)
            .filter(|(_, key)| !is_released(key))
            .map(|(session, _)| session)
            .collect();
        if active != self.active {
            self.active = active;
            self.dirty = true;
        }

        let cwds: Vec<PathBuf> = self.active.iter().map(|s| s.cwd.clone()).collect();
        let before = self.places.len();
        self.places.retain(|cwd, _| cwds.contains(cwd));
        self.dirty |= self.places.len() != before;
        for cwd in &cwds {
            let stale = match self.places.get(cwd) {
                None => true,
                Some(place) => {
                    !matches!(place.state, PlaceState::Root(_))
                        && now.saturating_duration_since(place.checked) >= RESOLVE_AGAIN
                }
            };
            if !stale {
                continue;
            }
            let Some(state) = self.resolve_place(cwd, now) else {
                continue;
            };
            self.dirty |= self
                .places
                .get(cwd)
                .is_none_or(|place| place.state != state);
            self.places.insert(
                cwd.clone(),
                Place {
                    state,
                    checked: now,
                },
            );
        }

        let used: Vec<String> = self
            .places
            .values()
            .filter_map(|place| match &place.state {
                PlaceState::Root(key) if !is_released(key) => Some(key.clone()),
                _ => None,
            })
            .collect();
        let before = self.roots.len();
        self.roots.retain(|key, _| used.contains(key));
        self.dirty |= self.roots.len() != before;
        lock(&self.shared.watchers).retain(|key, _| used.contains(key));
    }

    /// What `cwd` is, asked under the scan gate so a release waits for this git call too.
    /// `None` when the folder was released meanwhile.
    fn resolve_place(&mut self, cwd: &Path, now: Instant) -> Option<PlaceState> {
        let git = match self.git() {
            Ok(git) => git,
            Err(error) => return Some(PlaceState::Failed(error.to_string())),
        };
        let shared = Arc::clone(&self.shared);
        let gate = lock(&shared.scan_gate);
        let key = self
            .keys
            .get(cwd)
            .cloned()
            .unwrap_or_else(|| folder_key(cwd));
        if self.is_released(&key) {
            return None;
        }
        let resolved = resolve(&git, cwd);
        drop(gate);
        Some(match resolved {
            Ok(Some(info)) => {
                let key = info.key.clone();
                if !self.roots.contains_key(&key) && !self.is_released(&key) {
                    self.add_root(info, now);
                }
                PlaceState::Root(key)
            }
            Ok(None) => PlaceState::NotRepo,
            Err(error) => PlaceState::Failed(error.to_string()),
        })
    }

    fn add_root(&mut self, info: RootInfo, now: Instant) {
        let signal = Arc::new(Signal::new(self.tx.clone()));
        let ignored = IgnoredDirs::default();
        let mode = self.watch(&info, &signal, &ignored);
        self.roots.insert(
            info.key.clone(),
            Root {
                info,
                signal,
                ignored,
                mode,
                coalescer: Coalescer::new(now),
                result: None,
            },
        );
    }

    /// Starts a watcher for the worktree; network folders and failures stay on the poll.
    fn watch(&self, info: &RootInfo, signal: &Arc<Signal>, ignored: &IgnoredDirs) -> Mode {
        if git::is_remote(&info.top) {
            return Mode::PollOnly;
        }
        match start_watcher(info, Arc::clone(signal), Arc::clone(ignored)) {
            Ok(watcher) => {
                // Checked under the watchers lock: a release that came in while the watcher
                // started either is seen here or closes the watcher right after the insert.
                let mut watchers = lock(&self.shared.watchers);
                let released = lock(&self.shared.released)
                    .iter()
                    .any(|folder| key_within(&info.key, folder));
                if released {
                    drop(watchers);
                    drop(watcher);
                    return Mode::PollOnly;
                }
                watchers.insert(info.key.clone(), watcher);
                Mode::Events
            }
            Err(error) => {
                eprintln!(
                    "hangar: claims fall back to polling for {}: {error}",
                    info.top.display()
                );
                Mode::PollOnly
            }
        }
    }

    fn collect_signals(&mut self) {
        let mut failed = Vec::new();
        for (key, root) in &mut self.roots {
            if root.signal.failed.swap(false, Ordering::SeqCst) && root.mode == Mode::Events {
                root.mode = Mode::PollOnly;
                failed.push(key.clone());
            }
            if let Some(at) = root.signal.take() {
                root.coalescer.event(at);
            }
        }
        let mut watchers = lock(&self.shared.watchers);
        for key in failed {
            watchers.remove(&key);
        }
    }

    fn scan_due(&mut self) {
        // The most overdue first, so the budget cannot starve a worktree.
        let mut keys: Vec<(Instant, String)> = self
            .roots
            .iter()
            .map(|(key, root)| (root.coalescer.deadline(), key.clone()))
            .collect();
        keys.sort_unstable();
        for (_, key) in keys {
            let now = Instant::now();
            if !self.budget.allows(now) {
                break;
            }
            let Some(reason) = self.roots.get(&key).and_then(|r| r.coalescer.due(now)) else {
                continue;
            };
            let git = match self.git() {
                Ok(git) => git,
                Err(error) => {
                    if let Some(root) = self.roots.get_mut(&key) {
                        root.coalescer.begin();
                        root.coalescer.finished(now, now, POLL_ONLY);
                        let result = Some(Err(error.to_string()));
                        self.dirty |= root.result != result;
                        root.result = result;
                    }
                    continue;
                }
            };
            let Some(root) = self.roots.get_mut(&key) else {
                continue;
            };
            let gate = lock(&self.shared.scan_gate);
            if lock(&self.shared.released)
                .iter()
                .any(|folder| key_within(&key, folder))
            {
                continue;
            }
            root.coalescer.begin();
            let started = Instant::now();
            let result = scan(&git, &root.info).map_err(|error| error.to_string());
            if root.mode == Mode::Events {
                // Refreshed with every scan: a new `target` or a changed `.gitignore` is an event
                // that leads here.
                let deadline = Instant::now() + git::TIMEOUT;
                *lock(&root.ignored) =
                    ignored_dirs(&git, &root.info.top, deadline).unwrap_or_default();
            }
            drop(gate);
            let finished = Instant::now();
            self.budget.spent(started, finished);
            let poll = match root.mode {
                Mode::Events => POLL_WATCHED,
                Mode::PollOnly => POLL_ONLY,
            };
            root.coalescer.finished(started, finished, poll);
            let changed = root.result.as_ref() != Some(&result);
            self.dirty |= changed;
            // The poll found what no event reported: the watcher may have stopped silently
            // (overflow on Windows). Start it again.
            let missed = reason == ScanReason::Poll
                && root.mode == Mode::Events
                && root.result.is_some()
                && changed;
            root.result = Some(result);
            if missed {
                let info = root.info.clone();
                let signal = Arc::clone(&root.signal);
                let ignored = Arc::clone(&root.ignored);
                lock(&self.shared.watchers).remove(&key);
                let mode = self.watch(&info, &signal, &ignored);
                if let Some(root) = self.roots.get_mut(&key) {
                    root.mode = mode;
                }
            }
        }
    }

    fn publish(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (snapshot, worktrees) = if self.enabled {
            (self.build(), self.worktrees())
        } else {
            (ClaimsSnapshot::default(), Vec::new())
        };
        *lock(&self.shared.worktrees) = Arc::new(worktrees);
        let mut current = lock(&self.shared.snapshot);
        if *current != snapshot {
            *current = snapshot.clone();
            drop(current);
            (self.emit)(&snapshot);
        }
    }

    /// Sessions per worktree key, in the session list's order.
    fn members(&self) -> HashMap<&str, Vec<SessionId>> {
        let mut members: HashMap<&str, Vec<SessionId>> = HashMap::new();
        for session in &self.active {
            if let Some(Place {
                state: PlaceState::Root(key),
                ..
            }) = self.places.get(&session.cwd)
            {
                members.entry(key.as_str()).or_default().push(session.id);
            }
        }
        members
    }

    fn worktrees(&self) -> Vec<WorktreeClaims> {
        let mut list: Vec<WorktreeClaims> = self
            .members()
            .into_iter()
            .filter_map(|(key, sessions)| {
                let root = self.roots.get(key)?;
                let Some(Ok(result)) = &root.result else {
                    return None;
                };
                Some(WorktreeClaims {
                    repo: root.info.repo.clone(),
                    root_key: root.info.key.clone(),
                    base: result.base.clone(),
                    sessions,
                    claims: result.claims.clone(),
                })
            })
            .collect();
        list.sort_by(|a, b| a.root_key.cmp(&b.root_key));
        list
    }

    fn build(&self) -> ClaimsSnapshot {
        let members = self.members();
        let mut keys: Vec<&str> = members.keys().copied().collect();
        keys.sort_unstable();
        let views: Vec<RootView<'_>> = keys
            .iter()
            .filter_map(|key| {
                let root = self.roots.get(*key)?;
                let Some(Ok(result)) = &root.result else {
                    return None;
                };
                Some(RootView {
                    repo: &root.info.repo,
                    base: result.base.as_deref(),
                    sessions: members.get(key).map_or(&[][..], Vec::as_slice),
                    claims: &result.claims,
                })
            })
            .collect();
        let overlaps = claims::overlaps(&views);
        let levels = claims::levels(&overlaps);
        let (overlaps, counts) = claims::cap_overlaps(overlaps, MAX_LISTED_OVERLAPS);

        let sessions = self
            .active
            .iter()
            .map(|session| {
                let overlap_count = counts.get(&session.id).copied().unwrap_or(0);
                let mut entry = SessionClaims {
                    session_id: session.id,
                    state: ClaimState::Pending,
                    root: None,
                    base: None,
                    shared_with: Vec::new(),
                    claims: Vec::new(),
                    truncated: false,
                    level: levels.get(&session.id).copied().unwrap_or_default(),
                    overlap_count,
                    overlaps_truncated: overlap_count > MAX_LISTED_OVERLAPS,
                };
                match self.places.get(&session.cwd).map(|place| &place.state) {
                    None => {}
                    Some(PlaceState::NotRepo) => entry.state = ClaimState::NotRepo,
                    Some(PlaceState::Failed(message)) => {
                        entry.state = ClaimState::Failed {
                            message: message.clone(),
                        };
                    }
                    Some(PlaceState::Root(key)) => {
                        entry.shared_with = members
                            .get(key.as_str())
                            .into_iter()
                            .flatten()
                            .copied()
                            .filter(|id| *id != session.id)
                            .collect();
                        if let Some(root) = self.roots.get(key) {
                            entry.root = Some(root.info.top.display().to_string());
                            match &root.result {
                                None => {}
                                Some(Err(message)) => {
                                    entry.state = ClaimState::Failed {
                                        message: message.clone(),
                                    };
                                }
                                Some(Ok(result)) => {
                                    entry.state = ClaimState::Ready;
                                    entry.base = result.base.clone();
                                    entry.truncated =
                                        result.truncated || result.claims.len() > MAX_LISTED_CLAIMS;
                                    entry.claims = result
                                        .claims
                                        .iter()
                                        .take(MAX_LISTED_CLAIMS)
                                        .cloned()
                                        .collect();
                                }
                            }
                        }
                    }
                }
                entry
            })
            .collect();
        ClaimsSnapshot {
            enabled: true,
            sessions,
            overlaps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claims::{ChangeKind, LineRange, OverlapLevel};
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;
    use std::fs;

    #[test]
    fn events_inside_git_folders() {
        let top = PathBuf::from("repo");
        let linked = PathBuf::from("main")
            .join(".git")
            .join("worktrees")
            .join("a");
        let dirs = [linked.clone(), top.join(".git")];
        assert!(relevant(&top.join("src").join("a.rs"), &dirs));
        assert!(relevant(&top.join(".git").join("HEAD"), &dirs));
        assert!(relevant(&top.join(".git").join("index"), &dirs));
        assert!(relevant(&top.join(".git").join("logs").join("HEAD"), &dirs));
        assert!(relevant(&linked.join("HEAD"), &dirs));
        assert!(!relevant(&top.join(".git").join("index.lock"), &dirs));
        assert!(!relevant(
            &top.join(".git").join("objects").join("ab"),
            &dirs
        ));
        assert!(!relevant(
            &top.join(".git").join("logs").join("refs"),
            &dirs
        ));
        assert!(!relevant(&linked.join("ORIG_HEAD"), &dirs));
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn numbered(lines: usize, changed: &[usize], marker: &str) -> String {
        (1..=lines)
            .map(|n| {
                if changed.contains(&n) {
                    format!("{marker} {n}\n")
                } else {
                    format!("line {n}\n")
                }
            })
            .collect()
    }

    /// Repository with one commit: `src/shared.txt` (40 lines), `docs/a.md`, a binary file.
    fn repo_with_commit(git: &Git, dir: &Path) {
        init_repo(git, dir);
        write(&dir.join("src").join("shared.txt"), &numbered(40, &[], ""));
        write(&dir.join("docs").join("a.md"), "a\n");
        fs::write(dir.join("logo.bin"), [0u8, 1, 2, 0, 255]).unwrap();
        git_ok(git, dir, &["add", "-A"]);
        git_ok(git, dir, &["commit", "--quiet", "-m", "first"]);
    }

    #[test]
    fn scan_of_a_real_worktree() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("claims-scan");
        let repo = tmp.path().join("my repo");

        // Without commits: everything counts as whole files, no diff runs.
        init_repo(&git, &repo);
        write(&repo.join("loose.txt"), "x\n");
        let info = resolve(&git, &repo).unwrap().unwrap();
        let result = scan(&git, &info).unwrap();
        assert_eq!(result.base, None);
        assert_eq!(result.claims.len(), 1);
        assert!(result.claims[0].whole_file);
        assert_eq!(result.claims[0].kind, ChangeKind::Untracked);
        fs::remove_file(repo.join("loose.txt")).unwrap();

        repo_with_commit(&git, &repo);
        // Lines 5 and 6 changed, a pure insertion after line 30, a binary change, a deletion.
        let mut text = numbered(40, &[5, 6], "changed");
        text = text.replace("line 30\n", "line 30\ninserted\n");
        write(&repo.join("src").join("shared.txt"), &text);
        fs::write(repo.join("logo.bin"), [0u8, 9, 9, 0]).unwrap();
        fs::remove_file(repo.join("docs").join("a.md")).unwrap();
        // Asked from a subfolder: the worktree is the repository root.
        let info = resolve(&git, &repo.join("src")).unwrap().unwrap();
        assert_eq!(info.key, folder_key(&repo));
        let result = scan(&git, &info).unwrap();
        assert_eq!(result.base.as_ref().map(String::len), Some(40));
        let by_path: HashMap<&str, _> =
            result.claims.iter().map(|c| (c.path.as_str(), c)).collect();
        let shared = by_path["src/shared.txt"];
        assert_eq!(
            shared.ranges,
            [
                LineRange { start: 5, end: 6 },
                LineRange { start: 30, end: 31 }
            ]
        );
        assert!(!shared.whole_file);
        assert!(by_path["logo.bin"].whole_file);
        assert_eq!(by_path["docs/a.md"].kind, ChangeKind::Deleted);
        assert!(by_path["docs/a.md"].whole_file);

        // Not a repository: the git folder itself has no work tree.
        assert_eq!(resolve(&git, &repo.join(".git")).unwrap(), None);
    }

    /// A repository whose config moves the work tree elsewhere is refused before Hangar
    /// resolves, watches or scans the folder it names.
    #[test]
    fn resolve_refuses_a_moved_work_tree() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("claims-core-worktree");
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        let other = tmp.path().join("other");
        fs::create_dir_all(&other).unwrap();
        git_ok(
            &git,
            &repo,
            &["config", "core.worktree", &other.to_string_lossy()],
        );
        assert!(matches!(
            resolve(&git, &repo),
            Err(GitError::ForeignWorkTree)
        ));
        git_ok(&git, &repo, &["config", "--unset", "core.worktree"]);
        assert!(resolve(&git, &repo).unwrap().is_some());
    }

    #[test]
    fn ignored_top_level_folders() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("claims-ignored");
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        write(&repo.join(".gitignore"), "target/\nnode_modules\n*.log\n");
        for dir in ["target", "node_modules", "src", "with space"] {
            fs::create_dir_all(repo.join(dir)).unwrap();
        }
        write(&repo.join("loose.log"), "x\n");
        let mut ignored = ignored_dirs(&git, &repo, Instant::now() + git::TIMEOUT).unwrap();
        ignored.sort();
        assert_eq!(ignored, ["node_modules", "target"]);

        assert!(in_ignored_dir(&repo.join("target"), &repo, &ignored));
        assert!(in_ignored_dir(
            &repo.join("target").join("debug").join("x.o"),
            &repo,
            &ignored
        ));
        assert!(!in_ignored_dir(
            &repo.join("src").join("a.rs"),
            &repo,
            &ignored
        ));
        assert!(!in_ignored_dir(&repo.join("loose.log"), &repo, &ignored));
        assert!(!in_ignored_dir(&repo.join(".gitignore"), &repo, &ignored));
        // Outside the worktree (a linked worktree's git folder) nothing is dropped.
        assert!(!in_ignored_dir(&tmp.path().join("target"), &repo, &ignored));

        // Nothing ignored: an empty list, not an error.
        fs::remove_file(repo.join(".gitignore")).unwrap();
        assert!(ignored_dirs(&git, &repo, Instant::now() + git::TIMEOUT)
            .unwrap()
            .is_empty());
    }

    /// Two linked worktrees of one repository change the same lines of the same file; a third
    /// session shares the first worktree. The worker watches both, reports the overlap, follows a
    /// later change through the watcher and lets go of a released worktree.
    #[test]
    fn service_reports_overlaps_between_worktrees() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("claims-service");
        let repo = tmp.path().join("repo");
        repo_with_commit(&git, &repo);
        let wt_a = tmp.path().join("wt a");
        let wt_b = tmp.path().join("wt-b");
        for (path, branch) in [(&wt_a, "a"), (&wt_b, "b")] {
            git_ok(
                &git,
                &repo,
                &[
                    "worktree",
                    "add",
                    "--quiet",
                    "-b",
                    branch,
                    &path.to_string_lossy(),
                ],
            );
        }
        write(
            &wt_a.join("src").join("shared.txt"),
            &numbered(40, &[10, 11], "from a"),
        );
        write(
            &wt_b.join("src").join("shared.txt"),
            &numbered(40, &[13], "from b"),
        );

        let sessions = vec![
            WatchedSession {
                id: 1,
                cwd: wt_a.clone(),
            },
            WatchedSession {
                id: 2,
                cwd: wt_b.join("src"),
            },
            WatchedSession {
                id: 3,
                cwd: wt_a.clone(),
            },
        ];
        let (tx, rx) = mpsc::channel();
        let service = ClaimsService::default();
        service
            .start(
                move || sessions.clone(),
                move |s| {
                    let _ = tx.send(s.clone());
                },
            )
            .unwrap();
        service.configure(true);
        let wait_for = |what: &str, check: &dyn Fn(&ClaimsSnapshot) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(30);
            while let Ok(snapshot) =
                rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                if check(&snapshot) {
                    return snapshot;
                }
            }
            panic!("no snapshot with {what}: {:#?}", service.snapshot());
        };

        let snapshot = wait_for("overlapping lines", &|s| {
            s.overlaps
                .iter()
                .any(|o| o.level == OverlapLevel::OverlappingLines)
        });
        let one_two = snapshot
            .overlaps
            .iter()
            .find(|o| o.session_id == 1 && o.other_id == 2)
            .unwrap();
        assert_eq!(one_two.path.as_deref(), Some("src/shared.txt"));
        assert_eq!(one_two.lines, [LineRange { start: 10, end: 13 }]);
        // The two sessions in one folder are not compared, but both collide with session 2.
        assert!(!snapshot
            .overlaps
            .iter()
            .any(|o| (o.session_id, o.other_id) == (1, 3)));
        assert!(snapshot.overlaps.iter().any(|o| o.session_id == 3));
        let first = &snapshot.sessions[0];
        assert_eq!(first.shared_with, [3]);
        assert_eq!(first.level, OverlapLevel::OverlappingLines);
        assert_eq!(first.state, ClaimState::Ready);
        assert_eq!(lock(&service.shared.watchers).len(), 2);

        // A later change arrives through the watcher, long before the 60 s poll.
        write(&wt_b.join("docs").join("b.md"), "b\n");
        wait_for("the new file", &|s| {
            s.sessions
                .iter()
                .any(|e| e.session_id == 2 && e.claims.iter().any(|c| c.path == "docs/b.md"))
        });

        // Released, but the removal fails: given back, session 2 is watched and compared again.
        let released = service.release(&wt_b);
        assert_eq!(lock(&service.shared.watchers).len(), 1);
        wait_for("session 2 released", &|s| {
            s.sessions.iter().all(|e| e.session_id != 2)
        });
        service.unrelease(released);
        wait_for("session 2 back", &|s| {
            s.sessions
                .iter()
                .any(|e| e.session_id == 2 && e.state == ClaimState::Ready)
                && s.overlaps.iter().any(|o| o.session_id == 2)
        });
        assert_eq!(lock(&service.shared.watchers).len(), 2);

        // Released: the watcher is closed at once and the worktree can go.
        let _released = service.release(&wt_b);
        assert_eq!(lock(&service.shared.watchers).len(), 1);
        git_ok(
            &git,
            &repo,
            &["worktree", "remove", "--force", &wt_b.to_string_lossy()],
        );
        assert!(!wt_b.exists());
        wait_for("session 2 gone", &|s| {
            s.sessions.iter().all(|e| e.session_id != 2) && s.overlaps.is_empty()
        });

        service.configure(false);
        wait_for("disabled", &|s| !s.enabled && s.sessions.is_empty());
        assert!(lock(&service.shared.watchers).is_empty());
        service.stop();
    }
}
