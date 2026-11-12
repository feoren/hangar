//! Session manager: owns all child processes and their PTYs.
//!
//! Each session runs four threads:
//! - Read: read from the PTY (blocking) and hand off to the batcher (bounded channel = backpressure)
//! - Batch: collect output, send it to the frontend as UTF-8, wait for acknowledgements
//! - Wait: wait for the process to exit, then close the pseudo console
//! - Input: write keyboard input in order without blocking the caller
//!
//! Resume: with `resume`, a profile starts with `resume_args` instead of `args`. If that fails
//! within [`RESUME_WINDOW`], the batch thread of the failed process (without holding the
//! session lock) restarts it once normally and hands its event receiver over.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, sync_channel, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::agent_catalog::{merge_all, Agent, CatalogAgent, ProfileView, Source};
use crate::batch;
use crate::cli_session::{self, Plan};
use crate::install_source::{self, PlanInput, SystemProbe, UpdatePlan};
use crate::lock;
use crate::output::{Batcher, Flow, Utf8Carry};
use crate::proctree::ProcTree;
use crate::profile::Profile;
use crate::secrets::{self, KeyringStore, SecretStore};
use crate::session::{GateStatus, SessionId, SessionInfo, SessionKind, Status, TransitionError};
use crate::subagents;
use crate::updates::{self, CommandError};
use crate::wsl;

/// Batching window per the architecture: 8 to 16 ms.
const BATCH_DELAY: Duration = Duration::from_millis(12);
const BATCH_MAX_BYTES: usize = 256 * 1024;
const READ_BUFFER: usize = 64 * 1024;
/// At most this many read buffers sit between PTY and batcher; beyond that reading blocks
/// and the child process is slowed down when it writes.
const CHANNEL_DEPTH: usize = 64;
/// At most this many unacknowledged bytes are in flight to the frontend.
const HIGH_WATER: usize = 2 * 1024 * 1024;
const FLOW_STALL: Duration = Duration::from_secs(5);
/// How long to keep waiting for remaining output after the process exits.
const EXIT_GRACE: Duration = Duration::from_secs(2);
const IDLE_WAIT: Duration = Duration::from_secs(3600);
/// If a resume fails this quickly, there was probably nothing to resume (or the
/// CLI does not know the option). The session then starts once normally.
pub const RESUME_WINDOW: Duration = Duration::from_secs(5);

/// Identifiers of a calling agent session that launched Hangar (such as `pnpm tauri dev` in
/// Claude Code). They belong to the calling session, not to the sessions in Hangar: Claude Code
/// would otherwise take itself for a sub-session (no transcript), and tokens such as
/// `CLAUDE_CODE_MESSAGING_TOKEN` would be passed on to every child. Real settings such as
/// `CLAUDE_CODE_GIT_BASH_PATH` are kept on purpose. A profile can set any of them again
/// through `env`.
pub const INHERITED_SESSION_MARKERS: &[&str] = &[
    "AI_AGENT",
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
];

/// Windows: while the variable is set, the program search (`NeedCurrentDirectoryForExePath`,
/// used by cmd.exe among others) no longer looks in the current folder. Set for update sessions
/// and for every session whose program is a `.cmd`/`.bat` ([`batch::is_batch`]); an empty value
/// in the profile `env` removes it.
pub const NO_CWD_IN_EXE_PATH: &str = "NoDefaultCurrentDirectoryInExePath";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionEvent {
    /// `bytes` is the UTF-8 length of `data`; the frontend acknowledges exactly this number.
    Output {
        data: String,
        bytes: usize,
    },
    Exit {
        code: Option<u32>,
    },
    /// The resume (`resume_args`) ended within [`RESUME_WINDOW`] with a nonzero `code`. The
    /// session immediately restarts once normally, on the same channel and with the same id; its
    /// `Output` and `Exit` events follow. There is no `Exit` for the resume
    /// attempt. `cli_session_id` is the conversation id of the normal start (the same id after a
    /// failed `resume_id_args`, a new one after a failed `resume_args`, `null` without
    /// `session_id_args`); the frontend stores it like the one from `SessionInfo`. `sub_agents`
    /// is the new `SessionInfo.subAgents`: a failed `resume_args` had no id, the normal start
    /// has one.
    #[serde(rename_all = "camelCase")]
    ResumeFallback {
        code: Option<u32>,
        cli_session_id: Option<String>,
        sub_agents: bool,
    },
}

/// What the agent gate (`gate_service.rs`) adds to one process start of an agent session.
pub struct GateLaunch {
    /// Appended to the start arguments, before a working folder option.
    pub args: Vec<String>,
    /// Set in the child's environment only, after the profile's own variables.
    pub env: Vec<(String, Zeroizing<String>)>,
}

/// What the agent gate decided for one process start.
pub enum GatePlan {
    /// Not for this program, or the gate is off: the session shows nothing about it.
    Skip,
    Launch(GateLaunch),
    /// The program could take the hooks but does not get them; the session shows why.
    Unavailable(GateStatus),
}

impl GatePlan {
    fn status(&self) -> GateStatus {
        match self {
            GatePlan::Skip => GateStatus::None,
            GatePlan::Launch(_) => GateStatus::Active,
            GatePlan::Unavailable(status) => *status,
        }
    }
}

/// Installs the agent gate per process. `prepare` is called without the session lock for every
/// start of an agent session (first start, restart, resume and its fallback); `ended` is called
/// under the session lock when that process is gone or the session closed, so a restart that
/// follows can never lose its new registration to the old process's end.
pub trait LaunchGate: Send + Sync {
    /// Whether a start of `agent` would get the gate now (program and setting), without
    /// registering anything or starting the endpoint.
    fn applies(&self, agent: &Agent) -> bool;
    fn prepare(&self, session: SessionId, agent: &Agent, program: &Path) -> GatePlan;
    fn ended(&self, session: SessionId);
}

/// Whether the start arguments already carry Claude Code's `--settings`. Hangar's own would be a
/// second one, and whether Claude Code merges both or keeps one is not measured, so such a start
/// goes without the gate instead of risking the user's settings or a silently missing hook.
pub fn has_own_settings(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--settings" || arg.starts_with("--settings="))
}

/// Receiver of a session's events (a Tauri channel in production, a closure in tests).
/// `Sync`, because after a fallback the batch thread of the new process uses the same receiver.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: SessionEvent);
}

impl<F> EventSink for F
where
    F: Fn(SessionEvent) + Send + Sync + 'static,
{
    fn emit(&self, event: SessionEvent) {
        self(event);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl TermSize {
    fn to_pty(self) -> PtySize {
        PtySize {
            rows: self.rows.max(1),
            cols: self.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("Unknown profile '{0}'")]
    UnknownProfile(String),
    #[error("Unknown session {0}")]
    UnknownSession(SessionId),
    #[error("Session {0} is not running")]
    NotRunning(SessionId),
    #[error("Session {0} is still running")]
    StillRunning(SessionId),
    #[error("Command '{0}' was not found. Is it installed and on the PATH?")]
    CommandNotFound(String),
    #[error("Working folder '{0}' does not exist")]
    BadCwd(PathBuf),
    #[error("The title must not be empty")]
    EmptyTitle,
    #[error(transparent)]
    CliSessionId(#[from] cli_session::InvalidId),
    #[error("Session {0} has no sub-agents Hangar can show")]
    NoSubAgents(SessionId),
    #[error("The update of {0} is already running")]
    UpdateRunning(String),
    #[error("The update of {0} changed since it was shown; check for updates again")]
    PlanChanged(String),
    #[error("{profile} cannot be updated from Hangar: {reason}")]
    NoUpdateCommand {
        profile: String,
        reason: CommandError,
    },
    #[error("'{program}' is a cmd.exe script and will not start: {reason}")]
    UnsafeBatch {
        program: String,
        reason: batch::UnsafeBatch,
    },
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error(
        "The process tree of the session cannot be watched, so the program was not started: {0}"
    )]
    Watch(#[source] io::Error),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Transition(#[from] TransitionError),
}

/// Errors reach the frontend as readable text.
impl Serialize for ManagerError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

fn pty_error(error: impl std::fmt::Display) -> ManagerError {
    ManagerError::Pty(error.to_string())
}

enum Message {
    Data(Vec<u8>),
    /// Exit code and time of the process exit.
    Exited(Option<u32>, Instant),
}

type SharedMaster = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

/// Everything that exists only while a process is running. Drop ends the input and
/// (on Windows through the Job Object) every descendant still alive.
struct Runtime {
    master: SharedMaster,
    input: Sender<Vec<u8>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    tree: Option<ProcTree>,
    flow: Arc<Flow>,
    /// The agent gate for this process, copied into the session's info when it attaches.
    gate: GateStatus,
}

impl Runtime {
    fn kill(&self) -> io::Result<()> {
        if let Some(tree) = &self.tree {
            if tree.terminate().is_ok() {
                return Ok(());
            }
        }
        lock(&self.killer).kill()
    }
}

struct Entry {
    info: SessionInfo,
    /// The entry the session was started from; a restart looks up the program again.
    agent: Agent,
    /// Incremented on every restart so threads of an old process do not touch the new one.
    generation: u64,
    runtime: Option<Runtime>,
    /// Last known size; a fallback after a failed resume starts with it.
    size: TermSize,
    /// The user ended the running process. Its exit code is then no sign of a
    /// failed resume.
    kill_requested: bool,
}

struct Shared {
    sessions: Mutex<HashMap<SessionId, Entry>>,
    /// Values of the profiles' `secret_env`, read at every start (also restart and fallback).
    secrets: Arc<dyn SecretStore>,
    /// The agent gate, once the app set it ([`SessionManager::set_launch_gate`]).
    gate: Mutex<Option<Arc<dyn LaunchGate>>>,
}

impl Shared {
    fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            sessions: Mutex::default(),
            secrets,
            gate: Mutex::new(None),
        }
    }

    fn launch_gate(&self) -> Option<Arc<dyn LaunchGate>> {
        lock(&self.gate).clone()
    }

    /// The gate lets go of the session's process. Under the session lock (see [`LaunchGate`]).
    fn gate_ended(&self, id: SessionId) {
        if let Some(gate) = self.launch_gate() {
            gate.ended(id);
        }
    }

    fn finish(&self, id: SessionId, generation: u64, code: Option<u32>) {
        let mut sessions = lock(&self.sessions);
        if let Some(entry) = sessions.get_mut(&id) {
            if entry.generation == generation {
                entry.runtime = None;
                // Already exited means another path was faster. Nothing to do.
                let _ = entry.info.mark_exited(code);
                self.gate_ended(id);
            }
        }
    }

    fn attach(&self, id: SessionId, runtime: Runtime) -> Result<SessionInfo, ManagerError> {
        let mut sessions = lock(&self.sessions);
        let Some(entry) = sessions.get_mut(&id) else {
            // Closed during startup. On Unix, Drop does not end the process, hence
            // explicitly; the signal does not block.
            runtime.flow.close();
            let _ = runtime.kill();
            return Err(ManagerError::UnknownSession(id));
        };
        entry.info.gate = runtime.gate;
        // If the process has already exited again, the status stays `exited` and `runtime` is dropped.
        if entry.info.status == Status::Starting {
            entry.info.mark_running()?;
            entry.runtime = Some(runtime);
        }
        Ok(entry.info.clone())
    }

    /// Replaces a failed resume attempt with a normal start. `false` means
    /// no fallback (session closed, new process, ended by the user); the caller then reports
    /// the exit as usual.
    fn resume_fallback(
        self: &Arc<Self>,
        id: SessionId,
        generation: u64,
        code: Option<u32>,
        sink: &Arc<dyn EventSink>,
    ) -> bool {
        let (agent, cwd, size, next, plan, sub_agents) = {
            let mut sessions = lock(&self.sessions);
            let Some(entry) = sessions.get_mut(&id) else {
                return false;
            };
            if entry.generation != generation
                || entry.kill_requested
                || entry.info.status == Status::Exited
            {
                return false;
            }
            entry.runtime = None;
            // Running or starting to exited is always allowed, exited to starting as well.
            if entry.info.mark_exited(code).is_err() || entry.info.mark_restarting().is_err() {
                return false;
            }
            // The failed process is gone: its token, held edits and what it was told go with it,
            // so the fresh conversation starts with a clean slate like a restart.
            self.gate_ended(id);
            entry.info.resumed = false;
            entry.generation += 1;
            let plan = Plan::fallback(&entry.agent.profile, entry.info.cli_session_id.take());
            set_cli_session(&mut entry.info, &entry.agent, plan.cli_session_id.clone());
            (
                entry.agent.clone(),
                entry.info.cwd.clone(),
                entry.size,
                entry.generation,
                plan,
                entry.info.sub_agents,
            )
        };
        sink.emit(SessionEvent::ResumeFallback {
            code,
            cli_session_id: plan.cli_session_id.clone(),
            sub_agents,
        });
        let started = spawn(
            self,
            id,
            next,
            &agent,
            &Launch::normal(&plan.args, SessionKind::Agent),
            &cwd,
            size,
            Arc::clone(sink),
        );
        match started {
            Ok(runtime) => {
                // If the session is gone by now, `attach` has already cleaned up; the batch thread
                // of the new process reports its exit itself.
                let _ = self.attach(id, runtime);
            }
            Err(error) => {
                eprintln!("hangar: session {id}: normal start after resume failed: {error}");
                self.finish(id, next, None);
                sink.emit(SessionEvent::Exit { code });
            }
        }
        true
    }
}

/// How a process starts.
struct Launch<'a> {
    args: &'a [String],
    /// Started with `resume_args`: a quick failure triggers the fallback.
    resume: bool,
    kind: SessionKind,
}

impl<'a> Launch<'a> {
    fn normal(args: &'a [String], kind: SessionKind) -> Self {
        Self {
            args,
            resume: false,
            kind,
        }
    }
}

/// Is an update already running for this profile (other than in session `except`)? Two concurrent
/// updates of the same program write to the same files.
fn update_running(
    sessions: &HashMap<SessionId, Entry>,
    profile_id: &str,
    except: Option<SessionId>,
) -> bool {
    sessions.values().any(|e| {
        e.info.kind == SessionKind::Update
            && e.info.profile_id == profile_id
            && e.info.status != Status::Exited
            && Some(e.info.id) != except
    })
}

/// Inputs of the entry list and the merged result.
struct Entries {
    user: Vec<Profile>,
    /// WSL distributions; `None` until detected once ([`SessionManager::refresh_wsl`]).
    wsl: Option<Vec<Agent>>,
    merged: Arc<Vec<Agent>>,
}

pub struct SessionManager {
    /// Current state for new starts. Running sessions keep their own `Agent` in their
    /// entry; reloading the profiles ([`Self::set_profiles`]) therefore does not touch them.
    entries: Mutex<Entries>,
    /// Held while [`Self::ensure_wsl`] detects, so a second caller waits for that result
    /// instead of starting wsl.exe again. Never taken under `entries`.
    wsl_detection: Mutex<()>,
    catalog: Vec<CatalogAgent>,
    home: PathBuf,
    next_id: AtomicU64,
    shared: Arc<Shared>,
}

impl SessionManager {
    /// `profiles` from the user file take precedence; catalog agents with the same `id` are dropped.
    /// Secrets come from the OS keychain ([`KeyringStore`]); see [`Self::with_secret_store`].
    pub fn new(profiles: Vec<Profile>, catalog: Vec<CatalogAgent>, home: PathBuf) -> Self {
        let merged = Arc::new(merge_all(profiles.clone(), catalog.clone(), Vec::new()));
        Self {
            entries: Mutex::new(Entries {
                user: profiles,
                wsl: None,
                merged,
            }),
            wsl_detection: Mutex::new(()),
            catalog,
            home,
            next_id: AtomicU64::new(1),
            shared: Arc::new(Shared::new(Arc::new(KeyringStore::default()))),
        }
    }

    /// Another place for secret values, for tests. Only before the first session starts.
    #[must_use]
    pub fn with_secret_store(mut self, store: Arc<dyn SecretStore>) -> Self {
        self.shared = Arc::new(Shared::new(store));
        self
    }

    /// Hands every later agent start to the agent gate ([`LaunchGate`]).
    pub fn set_launch_gate(&self, gate: Arc<dyn LaunchGate>) {
        *lock(&self.shared.gate) = Some(gate);
    }

    pub fn secret_store(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&self.shared.secrets)
    }

    /// All startable entries including detection. The PATH search runs on every call so that
    /// a freshly installed agent shows up without restarting Hangar.
    pub fn profiles(&self) -> Vec<ProfileView> {
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        self.agents()
            .iter()
            .map(|agent| agent.view(path.as_deref(), pathext.as_deref()))
            .collect()
    }

    /// Installed entries with their resolved program path, for version and icon queries.
    /// The search runs on every call, as in [`Self::profiles`]. Entries without a version source
    /// of their own get the built-in one for their program ([`updates::enrich`], Git Bash).
    pub fn installed(&self) -> Vec<(Agent, PathBuf)> {
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        self.agents()
            .iter()
            .filter_map(|agent| installed_entry(agent, path.as_deref(), pathext.as_deref()))
            .collect()
    }

    /// [`Self::installed`] for one entry; `None` if unknown or not installed.
    pub fn installed_one(&self, profile_id: &str) -> Option<(Agent, PathBuf)> {
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        let agent = self.agent(profile_id).ok()?;
        installed_entry(&agent, path.as_deref(), pathext.as_deref())
    }

    /// Profiles that run in an agent session right now (not exited), for the warning that an
    /// update may fail on locked files.
    pub fn running_profiles(&self) -> HashSet<String> {
        lock(&self.shared.sessions)
            .values()
            .filter(|e| e.info.kind == SessionKind::Agent && e.info.status != Status::Exited)
            .map(|e| e.info.profile_id.clone())
            .collect()
    }

    /// Profiles with an update session that has not ended. Their programs are being replaced; the
    /// periodic version query leaves them alone until the update reports its result.
    pub fn updating_profiles(&self) -> HashSet<String> {
        lock(&self.shared.sessions)
            .values()
            .filter(|e| e.info.kind == SessionKind::Update && e.info.status != Status::Exited)
            .map(|e| e.info.profile_id.clone())
            .collect()
    }

    /// How the update of the entry would run on this machine ([`install_source::plan`]).
    pub fn update_plan(&self, profile_id: &str) -> Result<(Agent, UpdatePlan), ManagerError> {
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        let mut agent = self.agent(profile_id)?;
        let program = agent
            .resolve(path.as_deref(), pathext.as_deref())
            .map(|r| r.program);
        if let Some(program) = &program {
            updates::enrich(&mut agent, program, &SystemProbe);
        }
        let plan = install_source::plan(
            &PlanInput {
                profile: &agent.profile,
                install_hint: agent.install_hint.as_deref(),
                program: program.as_deref(),
                candidates: &agent.candidates,
            },
            &SystemProbe,
        );
        Ok((agent, plan))
    }

    /// Replaces the user profiles (freshly read profiles.toml) and merges in the catalog as at
    /// startup. Affects new starts only.
    pub fn set_profiles(&self, profiles: Vec<Profile>) {
        let mut entries = lock(&self.entries);
        entries.user = profiles;
        self.remerge(&mut entries);
    }

    /// Replaces the detected WSL distributions ([`wsl::detect`]).
    pub fn set_wsl(&self, distributions: Vec<Agent>) {
        let mut entries = lock(&self.entries);
        entries.wsl = Some(distributions);
        self.remerge(&mut entries);
    }

    /// Detects the WSL distributions again (a few hundred milliseconds, up to the time limit);
    /// not under the lock.
    pub fn refresh_wsl(&self) {
        self.set_wsl(wsl::detect());
    }

    /// Detects once if that has not happened yet; a caller that comes while a detection runs
    /// waits for it.
    pub fn ensure_wsl(&self) {
        let _detecting = lock(&self.wsl_detection);
        if lock(&self.entries).wsl.is_none() {
            self.refresh_wsl();
        }
    }

    fn remerge(&self, entries: &mut Entries) {
        entries.merged = Arc::new(merge_all(
            entries.user.clone(),
            self.catalog.clone(),
            entries.wsl.clone().unwrap_or_default(),
        ));
    }

    /// Snapshot, so the PATH search does not run under the lock.
    fn agents(&self) -> Arc<Vec<Agent>> {
        Arc::clone(&lock(&self.entries).merged)
    }

    /// The profile `id` from profiles.toml, `None` for catalog and WSL entries and unknown ids.
    pub fn user_profile(&self, id: &str) -> Option<Profile> {
        self.agents()
            .iter()
            .find(|agent| agent.id() == id && agent.source == Source::User)
            .map(|agent| agent.profile.clone())
    }

    /// Ids of all entries (profiles, catalog, WSL).
    pub fn ids(&self) -> Vec<String> {
        self.agents()
            .iter()
            .map(|agent| agent.id().to_owned())
            .collect()
    }

    /// `secret_env` of any entry; `None` for an unknown id.
    pub fn secret_names(&self, id: &str) -> Option<Vec<String>> {
        self.agents()
            .iter()
            .find(|agent| agent.id() == id)
            .map(|agent| agent.profile.secret_env.clone())
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let mut list: Vec<SessionInfo> = lock(&self.shared.sessions)
            .values()
            .map(|e| e.info.clone())
            .collect();
        list.sort_by_key(|info| info.id);
        list
    }

    /// Kind of the session, `None` for unknown ones.
    pub fn kind(&self, id: SessionId) -> Option<SessionKind> {
        lock(&self.shared.sessions).get(&id).map(|e| e.info.kind)
    }

    pub fn create(
        &self,
        profile_id: &str,
        cwd: Option<PathBuf>,
        size: TermSize,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        self.create_with(profile_id, cwd, size, false, sink)
    }

    /// Like [`Self::create_with`], with the conversation id stored for this session. With
    /// `resume` and `resume_id_args`, exactly that conversation is resumed; without an id the
    /// profile's `resume_args` apply as before ([`Plan::first`]). An id that is not a UUID is an
    /// error.
    pub fn create_resuming(
        &self,
        profile_id: &str,
        cwd: Option<PathBuf>,
        size: TermSize,
        resume: bool,
        cli_session_id: Option<&str>,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        let stored = cli_session_id.map(cli_session::validate).transpose()?;
        let agent = self.agent(profile_id)?;
        let title = agent.profile.name.clone();
        let plan = Plan::first(&agent.profile, resume, stored.as_deref());
        self.start(agent, cwd, size, plan, SessionKind::Agent, title, sink)
    }

    /// Like [`Self::create`]; with `resume` the program starts with `command` + `resume_args`
    /// instead of `args`, if the profile has any (otherwise normally, `resumed` stays `false`).
    /// If this attempt ends within [`RESUME_WINDOW`] with a nonzero exit code, one
    /// normal start follows ([`SessionEvent::ResumeFallback`]).
    pub fn create_with(
        &self,
        profile_id: &str,
        cwd: Option<PathBuf>,
        size: TermSize,
        resume: bool,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        self.create_resuming(profile_id, cwd, size, resume, None, sink)
    }

    /// Starts a program update as a normal session (`kind: update`, title
    /// "Update: <name>"). Only the plan's command runs ([`Self::update_plan`]: the profile's
    /// `update_hint`, the program's self-updater or its installer's update), as argv without a
    /// shell ([`updates::update_agent`]); the command text never comes from the frontend.
    /// Restarting the session repeats the update. If an update is already running for the
    /// profile, the result is [`ManagerError::UpdateRunning`].
    ///
    /// The working folder is the profile's fixed folder, otherwise the user's home folder, never one
    /// chosen by the frontend: on Windows `npm.cmd` looks for `node` in the current folder first, and
    /// a foreign project could plant a `node.cmd` there.
    pub fn create_update(
        &self,
        profile_id: &str,
        size: TermSize,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        self.create_update_checked(profile_id, None, size, sink)
    }

    /// [`Self::create_update`] that first compares the plan with the one the user saw
    /// (`expected`, [`UpdatePlan::fingerprint`]); a different plan is refused with
    /// [`ManagerError::PlanChanged`] instead of running something that was not shown.
    pub fn create_update_checked(
        &self,
        profile_id: &str,
        expected: Option<&str>,
        size: TermSize,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        let (agent, plan) = self.update_plan(profile_id)?;
        if expected.is_some_and(|expected| expected != plan.fingerprint()) {
            return Err(ManagerError::PlanChanged(agent.profile.name.clone()));
        }
        let updater = updates::update_agent(&agent, &plan).map_err(|reason| {
            ManagerError::NoUpdateCommand {
                profile: agent.profile.name.clone(),
                reason,
            }
        })?;
        let title = format!("Update: {}", agent.profile.name);
        let plan = Plan::normal(&updater.profile, None);
        self.start(updater, None, size, plan, SessionKind::Update, title, sink)
    }

    fn agent(&self, profile_id: &str) -> Result<Agent, ManagerError> {
        let find = || self.agents().iter().find(|a| a.id() == profile_id).cloned();
        // A restored WSL session may start before the first detection is done.
        let found = match find() {
            None if profile_id.starts_with(wsl::ID_PREFIX) => {
                self.ensure_wsl();
                find()
            }
            found => found,
        };
        found.ok_or_else(|| ManagerError::UnknownProfile(profile_id.to_owned()))
    }

    #[allow(clippy::too_many_arguments)] // All values belong to exactly this one start.
    fn start(
        &self,
        agent: Agent,
        cwd: Option<PathBuf>,
        size: TermSize,
        plan: Plan,
        kind: SessionKind,
        title: String,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        let profile = &agent.profile;
        // A folder fixed in the profile wins over the one chosen in the sidebar.
        let cwd = profile
            .cwd
            .clone()
            .or(cwd)
            .unwrap_or_else(|| self.home.clone());
        // A relative folder would be resolved against Hangar's own working folder, which is
        // nothing the user chose.
        if !cwd.is_absolute() || !cwd.is_dir() {
            return Err(ManagerError::BadCwd(cwd));
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // Register first, then start: if the process exits at once, the threads find their session.
        // Check and register under the same lock so two quick clicks do not both
        // start an update, and two restores of one id do not both resume that conversation.
        let plan = {
            let mut sessions = lock(&self.shared.sessions);
            if kind == SessionKind::Update && update_running(&sessions, &profile.id, None) {
                return Err(ManagerError::UpdateRunning(agent.profile.name.clone()));
            }
            // Two processes writing one conversation would corrupt it: the second starts fresh.
            let taken = plan
                .cli_session_id
                .as_deref()
                .is_some_and(|cli| cli_session_in_use(&sessions, cli));
            let plan = if taken {
                Plan::normal(profile, None)
            } else {
                plan
            };
            let mut info =
                SessionInfo::new(id, profile.id.clone(), title, cwd.clone(), now_millis());
            info.resumed = plan.resume;
            info.kind = kind;
            set_cli_session(&mut info, &agent, plan.cli_session_id.clone());
            sessions.insert(
                id,
                Entry {
                    info,
                    agent: agent.clone(),
                    generation: 0,
                    runtime: None,
                    size,
                    kill_requested: false,
                },
            );
            plan
        };
        let launch = Launch {
            args: &plan.args,
            resume: plan.resume,
            kind,
        };

        match spawn(
            &self.shared,
            id,
            0,
            &agent,
            &launch,
            &cwd,
            size,
            Arc::new(sink),
        ) {
            Ok(runtime) => self.shared.attach(id, runtime),
            Err(error) => {
                let mut sessions = lock(&self.shared.sessions);
                sessions.remove(&id);
                self.shared.gate_ended(id);
                Err(error)
            }
        }
    }

    pub fn restart(
        &self,
        id: SessionId,
        size: TermSize,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        let (agent, cwd, generation, kind, plan) = {
            let mut sessions = lock(&self.shared.sessions);
            let (kind, profile_id) = sessions
                .get(&id)
                .map(|e| (e.info.kind, e.info.profile_id.clone()))
                .ok_or(ManagerError::UnknownSession(id))?;
            if kind == SessionKind::Update && update_running(&sessions, &profile_id, Some(id)) {
                let name = sessions
                    .get(&id)
                    .map_or(profile_id.clone(), |e| e.agent.profile.name.clone());
                return Err(ManagerError::UpdateRunning(name));
            }
            let entry = sessions
                .get_mut(&id)
                .ok_or(ManagerError::UnknownSession(id))?;
            if entry.info.status != Status::Exited {
                return Err(ManagerError::StillRunning(id));
            }
            entry.info.mark_restarting()?;
            entry.info.resumed = false;
            entry.generation += 1;
            entry.size = size;
            entry.kill_requested = false;
            // A fresh conversation under a new id: the old id may already name one.
            let plan = Plan::normal(&entry.agent.profile, None);
            set_cli_session(&mut entry.info, &entry.agent, plan.cli_session_id.clone());
            (
                entry.agent.clone(),
                entry.info.cwd.clone(),
                entry.generation,
                kind,
                plan,
            )
        };
        match spawn(
            &self.shared,
            id,
            generation,
            &agent,
            &Launch::normal(&plan.args, kind),
            &cwd,
            size,
            Arc::new(sink),
        ) {
            Ok(runtime) => self.shared.attach(id, runtime),
            Err(error) => {
                if let Some(entry) = lock(&self.shared.sessions).get_mut(&id) {
                    let _ = entry.info.mark_exited(None);
                    self.shared.gate_ended(id);
                }
                Err(error)
            }
        }
    }

    pub fn write(&self, id: SessionId, data: &str) -> Result<(), ManagerError> {
        let sessions = lock(&self.shared.sessions);
        let runtime = running(&sessions, id)?;
        runtime
            .input
            .send(data.as_bytes().to_vec())
            .map_err(|_| ManagerError::NotRunning(id))
    }

    pub fn resize(&self, id: SessionId, size: TermSize) -> Result<(), ManagerError> {
        let master = {
            let mut sessions = lock(&self.shared.sessions);
            let master = Arc::clone(&running(&sessions, id)?.master);
            if let Some(entry) = sessions.get_mut(&id) {
                entry.size = size;
            }
            master
        };
        let master = lock(&master);
        if let Some(master) = master.as_ref() {
            master.resize(size.to_pty()).map_err(pty_error)?;
        }
        Ok(())
    }

    /// Acknowledgements for exited sessions are harmless and get ignored.
    pub fn ack(&self, id: SessionId, bytes: usize) -> Result<(), ManagerError> {
        let sessions = lock(&self.shared.sessions);
        let entry = sessions.get(&id).ok_or(ManagerError::UnknownSession(id))?;
        if let Some(runtime) = &entry.runtime {
            runtime.flow.ack(bytes);
        }
        Ok(())
    }

    pub fn kill(&self, id: SessionId) -> Result<(), ManagerError> {
        let mut sessions = lock(&self.shared.sessions);
        running(&sessions, id)?.kill()?;
        if let Some(entry) = sessions.get_mut(&id) {
            entry.kill_requested = true;
        }
        Ok(())
    }

    pub fn rename(&self, id: SessionId, title: &str) -> Result<SessionInfo, ManagerError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(ManagerError::EmptyTitle);
        }
        let mut sessions = lock(&self.shared.sessions);
        let entry = sessions
            .get_mut(&id)
            .ok_or(ManagerError::UnknownSession(id))?;
        title.clone_into(&mut entry.info.title);
        Ok(entry.info.clone())
    }

    /// What the sub-agent view needs to find the session's transcripts: the program's source,
    /// the conversation id and working folder, and only the source's path variables from the
    /// profile environment.
    pub fn sub_agent_target(&self, id: SessionId) -> Result<subagents::Target, ManagerError> {
        let sessions = lock(&self.shared.sessions);
        let entry = sessions.get(&id).ok_or(ManagerError::UnknownSession(id))?;
        let (Some(kind), Some(session)) = (
            entry.agent.profile.sub_agents,
            entry.info.cli_session_id.clone(),
        ) else {
            return Err(ManagerError::NoSubAgents(id));
        };
        let env = kind
            .env_names()
            .iter()
            .filter_map(|name| {
                let value = entry.agent.profile.env.get(*name)?;
                Some(((*name).to_owned(), value.clone()))
            })
            .collect();
        Ok(subagents::Target {
            kind,
            session,
            cwd: entry.info.cwd.clone(),
            env,
            home: self.home.clone(),
        })
    }

    /// Every session whose program keeps transcripts Hangar can read (the same ones as the
    /// sub-agent view), for the context meter (`context_usage`).
    pub fn context_targets(&self) -> Vec<(SessionId, subagents::Target)> {
        let ids: Vec<SessionId> = lock(&self.shared.sessions).keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| Some((id, self.sub_agent_target(id).ok()?)))
            .collect()
    }

    /// Ends the process (if it is running) and removes the session.
    pub fn close(&self, id: SessionId) -> Result<(), ManagerError> {
        let entry = {
            let mut sessions = lock(&self.shared.sessions);
            let entry = sessions
                .remove(&id)
                .ok_or(ManagerError::UnknownSession(id))?;
            self.shared.gate_ended(id);
            entry
        };
        if let Some(runtime) = entry.runtime {
            runtime.flow.close();
            // Failure usually means the process was already gone. Dropping `runtime` cleans up the rest.
            let _ = runtime.kill();
        }
        Ok(())
    }

    /// Ends all sessions, for example when the app closes.
    pub fn shutdown(&self) {
        let entries: Vec<Entry> = {
            let mut sessions = lock(&self.shared.sessions);
            let entries: Vec<Entry> = sessions.drain().map(|(_, e)| e).collect();
            for entry in &entries {
                self.shared.gate_ended(entry.info.id);
            }
            entries
        };
        for runtime in entries.into_iter().filter_map(|e| e.runtime) {
            runtime.flow.close();
            let _ = runtime.kill();
        }
    }
}

fn installed_entry(
    agent: &Agent,
    path: Option<&std::ffi::OsStr>,
    pathext: Option<&std::ffi::OsStr>,
) -> Option<(Agent, PathBuf)> {
    let program = agent.resolve(path, pathext)?.program;
    let mut agent = agent.clone();
    updates::enrich(&mut agent, &program, &SystemProbe);
    Some((agent, program))
}

/// The conversation id of the process that starts now, and whether sub-agents can be shown.
fn set_cli_session(info: &mut SessionInfo, agent: &Agent, cli_session_id: Option<String>) {
    info.sub_agents = agent.profile.sub_agents.is_some() && cli_session_id.is_some();
    info.cli_session_id = cli_session_id;
}

/// A session that has not exited runs (or is about to run) under this conversation id.
fn cli_session_in_use(sessions: &HashMap<SessionId, Entry>, cli_session_id: &str) -> bool {
    sessions.values().any(|e| {
        e.info.status != Status::Exited && e.info.cli_session_id.as_deref() == Some(cli_session_id)
    })
}

fn running(sessions: &HashMap<SessionId, Entry>, id: SessionId) -> Result<&Runtime, ManagerError> {
    let entry = sessions.get(&id).ok_or(ManagerError::UnknownSession(id))?;
    entry.runtime.as_ref().ok_or(ManagerError::NotRunning(id))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn spawn_thread(name: String, body: impl FnOnce() + Send + 'static) -> Result<(), ManagerError> {
    thread::Builder::new().name(name).spawn(body)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)] // All values belong to exactly this one start.
fn spawn(
    shared: &Arc<Shared>,
    id: SessionId,
    generation: u64,
    agent: &Agent,
    launch: &Launch<'_>,
    cwd: &Path,
    size: TermSize,
    sink: Arc<dyn EventSink>,
) -> Result<Runtime, ManagerError> {
    let path = std::env::var_os("PATH");
    let pathext = std::env::var_os("PATHEXT");
    let program = agent
        .resolve(path.as_deref(), pathext.as_deref())
        .ok_or_else(|| ManagerError::CommandNotFound(agent.command_label()))?
        .program;
    let profile = &agent.profile;
    let mut args = launch.args.to_vec();
    // Update sessions never get the gate: they run an installer, not the agent.
    let plan = match (launch.kind, shared.launch_gate()) {
        (SessionKind::Agent, Some(gate)) if has_own_settings(&args) && gate.applies(agent) => {
            GatePlan::Unavailable(GateStatus::ProfileSettings)
        }
        (SessionKind::Agent, Some(gate)) => gate.prepare(id, agent, &program),
        _ => GatePlan::Skip,
    };
    let gate_status = plan.status();
    let gate = match plan {
        GatePlan::Launch(launch) => Some(launch),
        GatePlan::Skip | GatePlan::Unavailable(_) => None,
    };
    if let Some(gate) = &gate {
        args.extend(gate.args.iter().cloned());
    }
    if let Some(flag) = agent.cwd_flag {
        args.push(flag.to_owned());
        args.push(cwd.to_string_lossy().into_owned());
    }
    let batch = batch::is_batch(&program);
    if batch {
        batch::check(&program, &args).map_err(|reason| ManagerError::UnsafeBatch {
            program: program.display().to_string(),
            reason,
        })?;
    }

    let pair = native_pty_system()
        .openpty(size.to_pty())
        .map_err(pty_error)?;
    // argv array, never an assembled shell string.
    let mut command = CommandBuilder::new(&program);
    command.args(&args);
    command.cwd(cwd);
    // No program lookup in the working folder for update sessions and `.cmd`/`.bat` launchers.
    // Shells (.exe) do not get it: there the user calls programs in the folder on purpose.
    let no_cwd_lookup = launch.kind == SessionKind::Update || batch;
    crate::child_env::apply(&mut command, &profile.env, no_cwd_lookup);
    // Straight into the child's environment; the values are not kept anywhere else.
    for (name, value) in
        secrets::for_launch(shared.secrets.as_ref(), &profile.id, &profile.secret_env)
    {
        command.env(name, value.as_str());
    }
    // Last, so no profile variable can replace the gate's token.
    for (name, value) in gate.iter().flat_map(|gate| &gate.env) {
        command.env(name, value.as_str());
    }
    let started = Instant::now();
    let mut child = pair.slave.spawn_command(command).map_err(pty_error)?;
    // Close our end of the slave side, otherwise EOF never arrives on Unix.
    drop(pair.slave);

    // Without the tree, closing the session would leave the program's children running: such a
    // session does not start at all.
    let tree = match child.process_id().map(ProcTree::attach) {
        Some(Ok(tree)) => Some(tree),
        // A program that ended before it could join the job has no tree left to watch; the
        // wait thread reports its exit like any other.
        Some(Err(_)) if matches!(child.try_wait(), Ok(Some(_))) => None,
        Some(Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ManagerError::Watch(error));
        }
        // Already gone again; the wait thread reports the exit.
        None => None,
    };
    let killer = child.clone_killer();
    let reader = pair.master.try_clone_reader().map_err(pty_error)?;
    let writer = pair.master.take_writer().map_err(pty_error)?;
    let master: SharedMaster = Arc::new(Mutex::new(Some(pair.master)));
    let flow = Arc::new(Flow::new(HIGH_WATER, FLOW_STALL));
    let (tx, rx) = sync_channel(CHANNEL_DEPTH);
    let (input_tx, input_rx) = mpsc::channel();
    if launch.kind == SessionKind::Update {
        // The exact argv, dimmed, before any output of the update: the user sees what runs.
        // Through the batcher like program output, so flow control counts it too.
        let _ = tx.send(Message::Data(argv_line(&program, &args).into_bytes()));
    }

    let tx_read = tx.clone();
    spawn_thread(format!("pty-read-{id}"), move || {
        read_loop(reader, &tx_read)
    })?;
    spawn_thread(format!("pty-input-{id}"), move || {
        input_loop(writer, &input_rx)
    })?;

    let master_wait = Arc::clone(&master);
    spawn_thread(format!("pty-wait-{id}"), move || {
        let code = child.wait().ok().map(|status| status.exit_code());
        // Measured here, not in the batcher: there backpressure can delay the report.
        let at = Instant::now();
        // ConPTY closes the output only once the pseudo console is closed. Without this
        // the read thread would never reach EOF on Windows.
        lock(&master_wait).take();
        let _ = tx.send(Message::Exited(code, at));
    })?;

    let shared_batch = Arc::clone(shared);
    let flow_batch = Arc::clone(&flow);
    let resume = launch.resume;
    spawn_thread(format!("pty-batch-{id}"), move || {
        let (code, exited_at) = batch_loop(&rx, &flow_batch, sink.as_ref());
        let failed_fast = exited_at.is_some_and(|at| at.duration_since(started) < RESUME_WINDOW)
            && code != Some(0);
        if resume && failed_fast && shared_batch.resume_fallback(id, generation, code, &sink) {
            return;
        }
        shared_batch.finish(id, generation, code);
        sink.emit(SessionEvent::Exit { code });
    })?;

    Ok(Runtime {
        master,
        input: input_tx,
        killer: Mutex::new(killer),
        tree,
        flow,
        gate: gate_status,
    })
}

/// `program args…` dimmed, with a line break; control characters are left out so the line can
/// never carry a terminal sequence of its own.
fn argv_line(program: &Path, args: &[String]) -> String {
    let text = install_source::display_command(&program.to_string_lossy(), args);
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    format!("\x1b[2m{text}\x1b[0m\r\n")
}

fn read_loop(mut reader: Box<dyn Read + Send>, tx: &SyncSender<Message>) {
    let mut buffer = vec![0u8; READ_BUFFER];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(Message::Data(buffer[..n].to_vec())).is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // On Unix the PTY reports EIO instead of EOF after the process exits.
            Err(_) => break,
        }
    }
}

fn input_loop(mut writer: Box<dyn Write + Send>, rx: &Receiver<Vec<u8>>) {
    while let Ok(data) = rx.recv() {
        if writer
            .write_all(&data)
            .and_then(|()| writer.flush())
            .is_err()
        {
            break;
        }
    }
}

/// Runs until the read and wait threads are done and returns exit code and time of the
/// process exit (`None` if the exit was never reported).
fn batch_loop(
    rx: &Receiver<Message>,
    flow: &Flow,
    sink: &dyn EventSink,
) -> (Option<u32>, Option<Instant>) {
    let mut batcher = Batcher::new(BATCH_DELAY, BATCH_MAX_BYTES);
    let mut utf8 = Utf8Carry::default();
    // Exit code, process exit, arrival of the report (the grace period starts from there).
    let mut exited: Option<(Option<u32>, Instant, Instant)> = None;

    loop {
        let grace_end = exited.map(|(_, _, received)| received + EXIT_GRACE);
        let wake = [batcher.deadline(), grace_end].into_iter().flatten().min();
        let timeout = wake.map_or(IDLE_WAIT, |at| at.saturating_duration_since(Instant::now()));
        match rx.recv_timeout(timeout) {
            Ok(Message::Data(data)) => batcher.push(&data, Instant::now()),
            Ok(Message::Exited(code, at)) => exited = Some((code, at, Instant::now())),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        if batcher.ready(now) {
            send_text(utf8.decode(&batcher.take()), flow, sink);
        }
        // Does not close the PTY by itself after the process exits; do not wait forever.
        if grace_end.is_some_and(|end| now >= end) {
            break;
        }
    }
    send_text(utf8.decode(&batcher.take()), flow, sink);
    send_text(utf8.finish(), flow, sink);
    match exited {
        Some((code, at, _)) => (code, Some(at)),
        None => (None, None),
    }
}

fn send_text(data: String, flow: &Flow, sink: &dyn EventSink) {
    if data.is_empty() || !flow.wait_below_high() {
        return;
    }
    let bytes = data.len();
    flow.add(bytes);
    sink.emit(SessionEvent::Output { data, bytes });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_settings_are_found_in_both_spellings_only() {
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert!(has_own_settings(&args(&[
            "--model",
            "x",
            "--settings",
            "a.json"
        ])));
        assert!(has_own_settings(&args(&["--settings={}"])));
        assert!(!has_own_settings(&args(&["--setting-sources", "user"])));
        assert!(!has_own_settings(&args(&["--settingsx", "-settings"])));
        assert!(!has_own_settings(&args(&[])));
    }

    #[test]
    fn argv_line_is_dimmed_and_plain() {
        let line = argv_line(
            Path::new("/usr/bin/npm"),
            &["install".into(), "a b".into(), "x\x1b[31m".into()],
        );
        assert_eq!(line, "\x1b[2m/usr/bin/npm install \"a b\" x[31m\x1b[0m\r\n");
    }

    #[test]
    fn events_serialize_as_in_contract() {
        let json = |event| serde_json::to_value(event).unwrap();
        assert_eq!(
            json(SessionEvent::ResumeFallback {
                code: Some(2),
                cli_session_id: Some("0f8fad5b-d9cb-469f-a165-70867728950e".into()),
                sub_agents: true,
            }),
            serde_json::json!({ "type": "resumeFallback", "code": 2,
                "cliSessionId": "0f8fad5b-d9cb-469f-a165-70867728950e", "subAgents": true })
        );
        assert_eq!(
            json(SessionEvent::ResumeFallback {
                code: None,
                cli_session_id: None,
                sub_agents: false,
            }),
            serde_json::json!({ "type": "resumeFallback", "code": null, "cliSessionId": null,
                "subAgents": false })
        );
        assert_eq!(
            json(SessionEvent::Exit { code: Some(0) }),
            serde_json::json!({ "type": "exit", "code": 0 })
        );
    }
}
