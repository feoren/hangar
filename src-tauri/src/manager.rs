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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::agent_catalog::{merge_all, Agent, CatalogAgent, ProfileView, Source, TeamBlock};
use crate::agent_models::{self, DefProgram, ModelChoice};
use crate::batch;
use crate::cli_session::{self, Plan};
use crate::install_source::{self, PlanInput, SystemProbe, UpdatePlan};
use crate::instructions::{self, StartPrompt, StartPrompts};
use crate::lock;
use crate::no_links::{self, FolderPlace};
use crate::output::{Batcher, Flow, Utf8Carry};
use crate::proctree::{self, ProcTree, TreeUsage};
use crate::profile::Profile;
use crate::secrets::{self, KeyringStore, SecretStore};
use crate::session::{
    GateStatus, SessionId, SessionInfo, SessionKind, SessionMode, Status, TransitionError,
};
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
/// Input messages waiting for the PTY at most; beyond that a write is answered with
/// [`ManagerError::Busy`] and the window sends it again.
const INPUT_DEPTH: usize = 16;
/// Largest input of one write; the window splits a larger paste.
pub const MAX_WRITE: usize = 1024 * 1024;
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
    // The IDE connection of the calling Claude Code: a child would talk to that editor.
    "CLAUDE_CODE_SSE_PORT",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    // VS Code's handle for its `code` command inside its terminal; the git extension's own
    // variables are found by prefix (`child_env::ide_variables`).
    "VSCODE_IPC_HOOK_CLI",
];

/// Windows: while the variable is set, the program search (`NeedCurrentDirectoryForExePath`,
/// used by cmd.exe among others) no longer looks in the current folder. Set as
/// [`no_cwd_lookup`] says; an empty value in the profile `env` removes it.
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
    /// Placed before the start arguments: global options of programs whose own arguments may
    /// begin with a subcommand (`codex resume --last`).
    pub prepend: Vec<String>,
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
    /// `args`: the start arguments the gate's own are added to. `version`: the installed
    /// version of the program as the version query found it, for a hook format whose settings
    /// depend on it (Claude Code's `onFailure`); `None` when unknown or not asked.
    fn prepare(
        &self,
        session: SessionId,
        agent: &Agent,
        program: &Path,
        args: &[String],
        version: Option<&str>,
    ) -> GatePlan;
    fn ended(&self, session: SessionId);
}

/// What sending with `hangar-send` (partner projects, ADR 0012; link groups, ADR 0014) adds to one
/// process start of an agent session; empty for a shell or without the client.
#[derive(Default)]
pub struct SendLaunch {
    /// Set in the child's environment only, last of all.
    pub env: Vec<(String, Zeroizing<String>)>,
    /// Put last on the child's `PATH`: a folder that holds only `hangar-send`.
    pub path_dir: Option<PathBuf>,
    /// A line for the agent's instructions ([`StartExtras::hint`]).
    pub hint: Option<String>,
}

/// Hands sessions the send client, called like [`LaunchGate`]: `prepare` without the session lock
/// for every process start of an agent session (private and team alike), `ended` under it when
/// that process is gone.
pub trait LaunchSend: Send + Sync {
    /// `program`: the resolved program of the start; a shell gets nothing.
    fn prepare(&self, session: SessionId, cwd: &Path, program: &Path) -> SendLaunch;
    fn ended(&self, session: SessionId);
}

/// `path` (the child's `PATH` as it stands) with `dir` at its end, separated as the platform
/// does. `None` when the result cannot be a `PATH` (a separator inside `dir`): then the child
/// keeps its own.
pub fn path_with(dir: &Path, path: Option<&std::ffi::OsStr>) -> Option<std::ffi::OsString> {
    let before = path.map(std::env::split_paths).into_iter().flatten();
    std::env::join_paths(before.chain(std::iter::once(dir.to_path_buf()))).ok()
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

/// Largest terminal Hangar hands to a PTY. A window never gets near it; a larger request would
/// only make ConPTY and the programs allocate screen buffers for nothing.
pub const MAX_COLS: u16 = 4096;
pub const MAX_ROWS: u16 = 2048;

impl TermSize {
    /// Within 1..=[`MAX_COLS`] and 1..=[`MAX_ROWS`].
    pub fn clamped(self) -> Self {
        Self {
            cols: self.cols.clamp(1, MAX_COLS),
            rows: self.rows.clamp(1, MAX_ROWS),
        }
    }

    fn to_pty(self) -> PtySize {
        let size = self.clamped();
        PtySize {
            rows: size.rows,
            cols: size.cols,
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
    #[error(
        "Working folder '{0}' is a network share or device path; sessions start in local folders only"
    )]
    RemoteCwd(PathBuf),
    #[error("Session {0} is not reading its input; try again in a moment")]
    Busy(SessionId),
    #[error("At most {max} bytes can be sent to a session at once, not {len}")]
    InputTooLarge { len: usize, max: usize },
    #[error("The title must not be empty")]
    EmptyTitle,
    #[error("{0} cannot be a team session yet: Hangar cannot pause its edits")]
    NoTeamSessions(String),
    #[error(
        "{0} cannot be a team session: its profile sets a working folder of its own, which would \
         replace the session's worktree"
    )]
    TeamFolderFixed(String),
    #[error(transparent)]
    CliSessionId(#[from] cli_session::InvalidId),
    #[error("Session {0} has no sub-agents Hangar can show")]
    NoSubAgents(SessionId),
    #[error("'{0}' is no valid sub-agent name")]
    InvalidSubAgent(String),
    #[error("{0} cannot start as a sub-agent")]
    NoSubAgentStart(String),
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

impl ManagerError {
    /// A stable code for errors the window shows in its own words (`backendError.ts`); the
    /// English text after it is only the fallback, so its wording may change freely.
    fn code(&self) -> Option<&'static str> {
        match self {
            Self::BadCwd(_) => Some("bad_cwd"),
            Self::RemoteCwd(_) => Some("remote_cwd"),
            _ => None,
        }
    }
}

/// Errors reach the frontend as readable text, prefixed with `[code] ` when they have a code.
impl Serialize for ManagerError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.code() {
            Some(code) => serializer.serialize_str(&format!("[{code}] {self}")),
            None => serializer.serialize_str(&self.to_string()),
        }
    }
}

fn pty_error(error: impl std::fmt::Display) -> ManagerError {
    ManagerError::Pty(error.to_string())
}

/// A failed process start, named by the program and the OS error code only. portable-pty's own
/// text carries the whole command line, which may hold a first prompt or the instructions, and
/// errors reach the window and its logs.
fn spawn_failed(program: &Path, error: &(dyn std::error::Error + 'static)) -> ManagerError {
    let code = std::iter::successors(Some(error), |e| e.source())
        .find_map(|e| {
            e.downcast_ref::<io::Error>()
                .and_then(io::Error::raw_os_error)
        })
        .or_else(|| os_error_code(&error.to_string()));
    let program = program.display();
    ManagerError::Pty(match code {
        Some(code) => format!("'{program}' could not be started (OS error {code})"),
        None => format!("'{program}' could not be started"),
    })
}

/// The code of the last "(os error N)" in a text, as `io::Error` writes it.
fn os_error_code(text: &str) -> Option<i32> {
    const MARK: &str = "(os error ";
    let start = text.rfind(MARK)? + MARK.len();
    let rest = &text[start..];
    rest[..rest.find(')')?].parse().ok()
}

pub use crate::no_links::is_network_or_device;

/// A session's working folder must be a local folder or one on a mapped drive the user chose:
/// not a share or device path and not reached through a link that leads to one. Checked before
/// anything else touches the folder ([`no_links::folder_place`]).
pub fn check_cwd(cwd: &Path) -> Result<(), ManagerError> {
    match no_links::folder_place(cwd) {
        FolderPlace::Local => Ok(()),
        FolderPlace::Remote => Err(ManagerError::RemoteCwd(cwd.to_owned())),
        // A relative folder would be resolved against Hangar's own working folder, which is
        // nothing the user chose.
        FolderPlace::NotAFolder => Err(ManagerError::BadCwd(cwd.to_owned())),
    }
}

/// Whether the child must not find programs in its working folder (Windows,
/// [`NO_CWD_IN_EXE_PATH`]): update sessions, every agent from the catalog, every `.cmd`/`.bat`
/// launcher and every program that is not an interactive shell. Decided on the resolved
/// `program`, not on where the profile came from: a user profile may well start an agent
/// (`command = "claude"`). Only a shell keeps the lookup, where calling a program in the folder
/// is what the user means.
pub fn no_cwd_lookup(kind: SessionKind, source: Source, batch: bool, program: &Path) -> bool {
    kind == SessionKind::Update
        || source == Source::Catalog
        || batch
        || !is_interactive_shell(program)
}

/// Shells by file name (case-insensitive, with or without `.exe`): cmd, Windows PowerShell,
/// PowerShell 7, WSL, Git Bash and the Unix shells a user may start on Windows. Anything else,
/// above all an agent CLI, counts as a program that runs commands on the user's behalf.
pub fn is_interactive_shell(program: &Path) -> bool {
    const SHELLS: &[&str] = &[
        "cmd",
        "powershell",
        "pwsh",
        "wsl",
        "bash",
        "sh",
        "zsh",
        "fish",
        "nu",
        "git-bash",
    ];
    let Some(name) = program.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    SHELLS.contains(&stem)
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
    /// Bounded ([`INPUT_DEPTH`]): a program that stops reading cannot make Hangar queue
    /// input without end.
    input: SyncSender<Vec<u8>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    /// Shared so the resources can be read outside the session lock (`tree_usages`).
    tree: Option<Arc<ProcTree>>,
    flow: Arc<Flow>,
    /// The agent gate for this process, copied into the session's info when it attaches.
    gate: GateStatus,
}

impl Runtime {
    fn kill(&self) -> io::Result<()> {
        if let Some(tree) = &self.tree {
            if tree.terminate_tree().is_ok() {
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
    /// `HANGAR.md` when the session was started with it; restarts and fallbacks read it again.
    instructions: Option<PathBuf>,
    /// The sub-agent definition the session runs as; restarts and fallbacks keep it.
    sub_agent: Option<String>,
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
    /// Versions for the gate's minimum versions and version-bound settings
    /// ([`SessionManager::set_version_source`]). Never taken together with another lock:
    /// cloned, then asked.
    versions: Mutex<Option<Arc<dyn VersionSource>>>,
    /// The default model of every sub-agent per program, read at every agent process start
    /// ([`SessionManager::set_sub_agent_models`]).
    sub_agent_models: Mutex<BTreeMap<DefProgram, ModelChoice>>,
    /// The send client for agent sessions ([`SessionManager::set_launch_send`]).
    send: Mutex<Option<Arc<dyn LaunchSend>>>,
}

impl Shared {
    fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            sessions: Mutex::default(),
            secrets,
            gate: Mutex::new(None),
            versions: Mutex::new(None),
            sub_agent_models: Mutex::default(),
            send: Mutex::new(None),
        }
    }

    fn launch_send(&self) -> Option<Arc<dyn LaunchSend>> {
        lock(&self.send).clone()
    }

    /// What a process of `agent` gets for its sub-agents' default model.
    fn model_launch(&self, agent: &Agent, batch: bool) -> agent_models::ModelLaunch {
        let Some(program) = agent.prompting.agent_program else {
            return agent_models::ModelLaunch::default();
        };
        let choice = lock(&self.sub_agent_models)
            .get(&program)
            .cloned()
            .unwrap_or_default();
        agent_models::launch(program, &choice, &agent.profile.env, batch)
    }

    fn launch_gate(&self) -> Option<Arc<dyn LaunchGate>> {
        lock(&self.gate).clone()
    }

    /// The gate and the send client let go of the session's process. Under the session lock (see
    /// [`LaunchGate`]).
    fn gate_ended(&self, id: SessionId) {
        if let Some(gate) = self.launch_gate() {
            gate.ended(id);
        }
        if let Some(send) = self.launch_send() {
            send.ended(id);
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

    fn attach(
        &self,
        id: SessionId,
        generation: u64,
        runtime: Runtime,
    ) -> Result<SessionInfo, ManagerError> {
        let mut sessions = lock(&self.sessions);
        let Some(entry) = sessions.get_mut(&id) else {
            // Closed during startup. On Unix, Drop does not end the process, hence
            // explicitly; the signal does not block.
            runtime.flow.close();
            let _ = runtime.kill();
            return Err(ManagerError::UnknownSession(id));
        };
        // A newer process took over before this one was attached: a resume attempt that failed
        // so fast that its fallback started first. This process has exited and was reaped (no
        // kill: its pid may belong to another process by now); installing its runtime would
        // replace the live one's, whose own attach would then drop it.
        if entry.generation != generation {
            let info = entry.info.clone();
            drop(sessions);
            runtime.flow.close();
            return Ok(info);
        }
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
                (
                    plan,
                    entry.info.mode,
                    (entry.instructions.clone(), entry.sub_agent.clone()),
                ),
                entry.info.sub_agents,
            )
        };
        let (plan, mode, instructions) = plan;
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
            &Launch::normal(&plan.args, SessionKind::Agent, mode, instructions),
            &cwd,
            size,
            Arc::clone(sink),
        );
        match started {
            Ok(runtime) => {
                // If the session is gone by now, `attach` has already cleaned up; the batch thread
                // of the new process reports its exit itself.
                let _ = self.attach(id, next, runtime);
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

/// The installed version of programs whose gate needs a minimum version ([`Capabilities`]'s
/// `min_version`), from the version query of ADR 0003. Set by the app; without one every such
/// version counts as unknown.
///
/// [`Capabilities`]: crate::agent_catalog::Capabilities
pub trait VersionSource: Send + Sync {
    /// profile id → version, for the given installed entries.
    fn versions(&self, installed: &[(Agent, PathBuf)]) -> HashMap<String, Option<String>>;
}

/// Why each of `agents` cannot be a team session on this machine, `None` when it can
/// ([`crate::agent_catalog::Capabilities::team_block`]): the gate's catalog data, the installed
/// version against its floor, and no administrator's system settings in the way. Asks
/// `versions` once, for the programs that need it. A fixed working folder is not judged here:
/// such a program can still be a team session that starts exactly there ([`team_allowed`]).
fn team_blocks(
    agents: &[Agent],
    versions: Option<&dyn VersionSource>,
    path: Option<&std::ffi::OsStr>,
    pathext: Option<&std::ffi::OsStr>,
) -> HashMap<String, Option<TeamBlock>> {
    let needing: Vec<(Agent, PathBuf)> = agents
        .iter()
        .filter(|agent| agent.capabilities.gate() && agent.capabilities.min_version.is_some())
        .filter_map(|agent| installed_entry(agent, path, pathext))
        .collect();
    let known = match (versions, needing.is_empty()) {
        (Some(source), false) => source.versions(&needing),
        _ => HashMap::new(),
    };
    agents
        .iter()
        .map(|agent| {
            let capabilities = agent.capabilities;
            let admin = capabilities
                .gate_format()
                .is_some_and(crate::hook_recipes::admin_settings_present);
            let version = known.get(agent.id()).and_then(Option::as_deref);
            (
                agent.id().to_owned(),
                capabilities.team_block(version, admin),
            )
        })
        .collect()
}

/// What the window shows for a program under "Team": the backend's reason it cannot be a team
/// session, or `FixedFolder` for one that could but whose profile fixes a working folder that
/// would replace the worktree.
fn shown_team_block(agent: &Agent, block: Option<TeamBlock>) -> Option<TeamBlock> {
    match block {
        Some(block) => Some(block),
        None if agent
            .profile
            .cwd
            .as_ref()
            .is_some_and(|cwd| !cwd.as_os_str().is_empty()) =>
        {
            Some(TeamBlock::FixedFolder)
        }
        None => None,
    }
}

/// A team session needs the agent gate (a hook format for its program that works on this
/// machine, ADR 0008) and must run in the folder it was given, its own worktree: a folder fixed
/// in the profile would win over it ([`SessionManager::start`]), and the session would work in a
/// shared folder unnoticed.
fn team_allowed(agent: &Agent, ready: bool, cwd: Option<&Path>) -> Result<(), ManagerError> {
    if !ready {
        return Err(ManagerError::NoTeamSessions(agent.profile.name.clone()));
    }
    if let Some(fixed) = &agent.profile.cwd {
        if cwd != Some(fixed.as_path()) {
            return Err(ManagerError::TeamFolderFixed(agent.profile.name.clone()));
        }
    }
    Ok(())
}

/// What a new session is: kind and mode, both fixed for its life.
#[derive(Debug, Clone, Copy)]
struct Start {
    kind: SessionKind,
    mode: SessionMode,
}

/// How a process starts.
struct Launch<'a> {
    args: &'a [String],
    /// Started with `resume_args`: a quick failure triggers the fallback.
    resume: bool,
    kind: SessionKind,
    /// Only team sessions get the agent gate.
    mode: SessionMode,
    extras: StartExtras,
}

impl<'a> Launch<'a> {
    /// A restart or a fallback: `HANGAR.md` and the sub-agent again, but no first prompt.
    fn normal(
        args: &'a [String],
        kind: SessionKind,
        mode: SessionMode,
        (instructions, sub_agent): (Option<PathBuf>, Option<String>),
    ) -> Self {
        Self {
            args,
            resume: false,
            kind,
            mode,
            extras: StartExtras {
                instructions,
                prompt: None,
                sub_agent,
                hint: None,
            },
        }
    }
}

/// What the window asks a new agent session to get besides its program's arguments
/// ([`crate::instructions`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartRequest {
    /// `HANGAR.md` for the agent, where its program takes instructions.
    pub instructions: bool,
    /// A first prompt, where its program takes one; ignored when the session resumes.
    pub prompt: Option<StartPrompt>,
    /// Start as this sub-agent definition (`agent_args`); only programs that take one, and only
    /// a name [`crate::agent_defs::valid_name`] accepts.
    pub sub_agent: Option<String>,
}

/// [`StartRequest`] resolved by the manager: the file's path and the prompt's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartExtras {
    /// `HANGAR.md`, read at every process start so an edit applies to the next one.
    pub instructions: Option<PathBuf>,
    /// The first user message; only the first process of a fresh session gets it.
    pub prompt: Option<String>,
    /// The sub-agent definition, every process of the session.
    pub sub_agent: Option<String>,
    /// A line of Hangar's own after `HANGAR.md` ([`instructions::with_hint`]); set per process
    /// start by [`LaunchSend`], never stored with the session.
    pub hint: Option<String>,
}

/// Arguments `extras` adds for `agent` started as `program`: the instructions go before the start
/// arguments (`codex -c ... resume --last`), the first prompt after them. What the program does not
/// take is left out; what cannot pass a cmd.exe launcher is left out with a logged warning, so the
/// session still starts. The length of the whole line is checked later ([`ArgParts::fitting`]).
pub fn extra_args(
    agent: &Agent,
    program: &Path,
    extras: &StartExtras,
) -> (Vec<String>, Vec<String>) {
    let leave_out = |what: &str, skipped: instructions::Skipped| {
        eprintln!("hangar: {}: {what} left out: {skipped}", agent.profile.name);
        Vec::new()
    };
    let before = match (&agent.prompting.instruction_args, &extras.instructions) {
        (Some(spec), Some(path)) => {
            instructions::with_hint(instructions::read(path), extras.hint.as_deref(), path, spec)
                .map_or_else(Vec::new, |text| {
                    let args = instructions::instruction_args(spec, &text);
                    instructions::check_launcher(program, spec, args)
                        .unwrap_or_else(|skipped| leave_out(instructions::FILE_NAME, skipped))
                })
        }
        _ => Vec::new(),
    };
    // The name passed `valid_name` at the start, so it needs no launcher check; it goes right
    // after the start arguments, before a positional first prompt.
    let mut after = match (&agent.prompting.agent_args, &extras.sub_agent) {
        (Some(spec), Some(name)) => instructions::agent_args(spec, name),
        _ => Vec::new(),
    };
    after.extend(match (&agent.prompting.prompt_args, &extras.prompt) {
        (Some(spec), Some(prompt)) => {
            let args = instructions::prompt_args(spec, prompt);
            instructions::check_launcher(program, spec, args)
                .unwrap_or_else(|skipped| leave_out("the first prompt", skipped))
        }
        _ => Vec::new(),
    });
    (before, after)
}

/// The arguments of one process start before Hangar's extras are placed.
struct ArgParts<'a> {
    /// `args`, `resume_args` and so on, with the conversation id.
    start: Vec<String>,
    gate: Option<&'a GateLaunch>,
    /// Option and folder of programs that take the working folder as an argument.
    cwd_flag: Option<(&'static str, String)>,
}

impl ArgParts<'_> {
    /// The whole argument list: gate options first, then the instructions (before a subcommand
    /// such as `codex resume --last`), the start arguments, the gate's arguments, the first prompt
    /// (last of all options: a positional prompt must not be taken for an option's value) and the
    /// working folder option.
    fn compose(&self, before: &[String], after: &[String]) -> Vec<String> {
        let (prepend, appended) = self.gate.map_or((&[][..], &[][..]), |gate| {
            (&gate.prepend[..], &gate.args[..])
        });
        let mut args: Vec<String> = prepend
            .iter()
            .chain(before)
            .chain(&self.start)
            .chain(appended)
            .chain(after)
            .cloned()
            .collect();
        if let Some((flag, folder)) = &self.cwd_flag {
            args.push((*flag).to_owned());
            args.push(folder.clone());
        }
        args
    }

    /// [`Self::compose`] whose fully quoted command line fits ([`instructions::check_length`]):
    /// the instructions go first, then the first prompt, each with a logged warning. Without
    /// either the line is what it always was.
    fn fitting(&self, program: &str, extra: (Vec<String>, Vec<String>), name: &str) -> Vec<String> {
        let (mut before, mut after) = extra;
        loop {
            let args = self.compose(&before, &after);
            let what = match instructions::check_length(program, &args) {
                Ok(()) => return args,
                Err(_) if before.is_empty() && after.is_empty() => return args,
                Err(skipped) if !before.is_empty() => {
                    before.clear();
                    (instructions::FILE_NAME, skipped)
                }
                Err(skipped) => {
                    after.clear();
                    ("the first prompt", skipped)
                }
            };
            eprintln!("hangar: {name}: {} left out: {}", what.0, what.1);
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
    /// `HANGAR.md` in the config folder ([`Self::with_instructions_file`]); `None`: no session
    /// gets instructions.
    instructions_file: Option<PathBuf>,
    /// Texts of the first prompts, from the embedded catalog; `None` if it had none (a test
    /// makes sure it has).
    start_prompts: Option<StartPrompts>,
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
            instructions_file: None,
            start_prompts: crate::agent_catalog::builtin_start_prompts()
                .inspect_err(|error| eprintln!("hangar: {error}"))
                .ok(),
        }
    }

    /// Where `HANGAR.md` lives; sessions started with [`StartRequest::instructions`] read it.
    #[must_use]
    pub fn with_instructions_file(mut self, path: PathBuf) -> Self {
        self.instructions_file = Some(path);
        self
    }

    /// Another place for secret values, for tests. Only before the first session starts.
    #[must_use]
    pub fn with_secret_store(mut self, store: Arc<dyn SecretStore>) -> Self {
        self.shared = Arc::new(Shared::new(store));
        self
    }

    /// The default model of every sub-agent per program for every later process start (Settings >
    /// Sub-agents); running processes keep theirs.
    pub fn set_sub_agent_models(&self, models: BTreeMap<DefProgram, ModelChoice>) {
        *lock(&self.shared.sub_agent_models) = models;
    }

    /// The installed programs that read sub-agent definitions, each once.
    pub fn installed_programs(&self) -> Vec<DefProgram> {
        let mut out: Vec<DefProgram> = self
            .installed()
            .iter()
            .filter_map(|(agent, _)| agent.prompting.agent_program)
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Hands every later agent start to the agent gate ([`LaunchGate`]).
    pub fn set_launch_gate(&self, gate: Arc<dyn LaunchGate>) {
        *lock(&self.shared.gate) = Some(gate);
    }

    /// Hands every later agent start to the send client ([`LaunchSend`]).
    pub fn set_launch_send(&self, send: Arc<dyn LaunchSend>) {
        *lock(&self.shared.send) = Some(send);
    }

    pub fn secret_store(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&self.shared.secrets)
    }

    /// All startable entries including detection. The PATH search runs on every call so that
    /// a freshly installed agent shows up without restarting Hangar.
    pub fn profiles(&self) -> Vec<ProfileView> {
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        let agents = self.agents();
        let blocks = self.team_blocks_for(&agents);
        agents
            .iter()
            .map(|agent| {
                let mut view = agent.view(path.as_deref(), pathext.as_deref());
                // Offered only where it works on this machine, not just in the catalog.
                let block = blocks
                    .get(agent.id())
                    .cloned()
                    .unwrap_or(Some(TeamBlock::NoHooks));
                view.team_capable = block.is_none();
                view.team_block = shown_team_block(agent, block);
                view
            })
            .collect()
    }

    /// Ids of the entries whose program is a shell ([`crate::partners::is_shell_command`] on the
    /// command and every candidate), without the PATH search: a cross-project message never goes
    /// there, where its lines would run as commands.
    pub fn shell_profiles(&self) -> HashSet<String> {
        self.agents()
            .iter()
            .filter(|agent| {
                std::iter::once(&agent.profile.command)
                    .chain(&agent.candidates)
                    .any(|command| crate::partners::is_shell_command(command))
            })
            .map(|agent| agent.id().to_owned())
            .collect()
    }

    /// profile id → display name of every entry, without the PATH search and the version query
    /// of [`Self::profiles`]: cheap enough for callers on the main thread (the team's session
    /// labels, refreshed on every session state change).
    pub fn profile_names(&self) -> HashMap<String, String> {
        self.agents()
            .iter()
            .map(|agent| (agent.id().to_owned(), agent.profile.name.clone()))
            .collect()
    }

    /// Where the installed versions for the gate's minimum versions come from.
    pub fn set_version_source(&self, source: Arc<dyn VersionSource>) {
        *lock(&self.shared.versions) = Some(source);
    }

    /// [`team_blocks`] for `agents` with this manager's version source.
    fn team_blocks_for(&self, agents: &[Agent]) -> HashMap<String, Option<TeamBlock>> {
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        let source = lock(&self.shared.versions).clone();
        team_blocks(
            agents,
            source.as_deref(),
            path.as_deref(),
            pathext.as_deref(),
        )
    }

    /// Whether each of `agents` can be a team session on this machine.
    fn team_ready_for(&self, agents: &[Agent]) -> HashMap<String, bool> {
        self.team_blocks_for(agents)
            .into_iter()
            .map(|(id, block)| (id, block.is_none()))
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

    /// The start arguments of each session's profile, for telling an agent that runs without
    /// asking before tools (`links::runs_without_prompts`).
    pub fn session_args(&self) -> HashMap<SessionId, Vec<String>> {
        lock(&self.shared.sessions)
            .iter()
            .map(|(id, entry)| (*id, entry.agent.profile.args.clone()))
            .collect()
    }

    /// Kind of the session, `None` for unknown ones.
    pub fn kind(&self, id: SessionId) -> Option<SessionKind> {
        lock(&self.shared.sessions).get(&id).map(|e| e.info.kind)
    }

    /// A private session ([`SessionMode::Private`]).
    pub fn create(
        &self,
        profile_id: &str,
        cwd: Option<PathBuf>,
        size: TermSize,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        self.create_with(profile_id, cwd, size, false, sink)
    }

    /// Like [`Self::create_with`], with the conversation id stored for this session and its
    /// mode. With `resume` and `resume_id_args`, exactly that conversation is resumed; without an
    /// id the profile's `resume_args` apply as before ([`Plan::first`]). An id that is not a UUID
    /// is an error. Only a team session gets the agent gate.
    #[allow(clippy::too_many_arguments)] // All values belong to exactly this one start.
    pub fn create_resuming(
        &self,
        profile_id: &str,
        cwd: Option<PathBuf>,
        size: TermSize,
        resume: bool,
        cli_session_id: Option<&str>,
        mode: SessionMode,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        self.create_requested(
            profile_id,
            cwd,
            size,
            resume,
            cli_session_id,
            mode,
            StartRequest::default(),
            sink,
        )
    }

    /// [`Self::create_resuming`] with what the window asks for besides the program's arguments:
    /// `HANGAR.md` and a first prompt ([`StartRequest`], [`extra_args`]).
    #[allow(clippy::too_many_arguments)] // All values belong to exactly this one start.
    pub fn create_requested(
        &self,
        profile_id: &str,
        cwd: Option<PathBuf>,
        size: TermSize,
        resume: bool,
        cli_session_id: Option<&str>,
        mode: SessionMode,
        request: StartRequest,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        let extras = StartExtras {
            instructions: self
                .instructions_file
                .clone()
                .filter(|_| request.instructions),
            prompt: request.prompt.and_then(|prompt| {
                self.start_prompts
                    .as_ref()
                    .map(|texts| texts.text(prompt).to_owned())
            }),
            sub_agent: request.sub_agent,
            hint: None,
        };
        let stored = cli_session_id.map(cli_session::validate).transpose()?;
        let agent = self.agent(profile_id)?;
        if let Some(name) = &extras.sub_agent {
            if !crate::agent_defs::valid_name(name) {
                return Err(ManagerError::InvalidSubAgent(name.clone()));
            }
            if agent.prompting.agent_args.is_none() {
                return Err(ManagerError::NoSubAgentStart(agent.profile.name.clone()));
            }
        }
        if mode == SessionMode::Team {
            let ready = self
                .team_ready_for(std::slice::from_ref(&agent))
                .get(agent.id())
                .copied()
                .unwrap_or(false);
            team_allowed(&agent, ready, cwd.as_deref())?;
        }
        let title = agent.profile.name.clone();
        let plan = Plan::first(&agent.profile, resume, stored.as_deref());
        let kind = Start {
            kind: SessionKind::Agent,
            mode,
        };
        self.start(agent, cwd, size, plan, kind, extras, title, sink)
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
        self.create_resuming(
            profile_id,
            cwd,
            size,
            resume,
            None,
            SessionMode::Private,
            sink,
        )
    }

    /// Starts a program update as a normal session (`kind: update`, title
    /// "Update: <name>"). Only the plan's command runs ([`Self::update_plan`]: the profile's
    /// `update_hint`, the program's self-updater or its installer's update), as argv without a
    /// shell ([`updates::update_agent`]); the command text never comes from the frontend.
    /// Restarting the session repeats the update. If an update is already running for the
    /// profile, the result is [`ManagerError::UpdateRunning`].
    ///
    /// The working folder is always the user's home folder: neither the frontend nor the profile's
    /// fixed folder chooses it ([`updates::update_agent`] drops that folder), so no foreign
    /// project's `.npmrc` or `pip.conf` steers the installer. On Windows the update also runs
    /// without the program lookup in its folder ([`no_cwd_lookup`]).
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
        let kind = Start {
            kind: SessionKind::Update,
            mode: SessionMode::Private,
        };
        // An installer, not the agent: no instructions, no prompt.
        let extras = StartExtras::default();
        self.start(updater, None, size, plan, kind, extras, title, sink)
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
        start: Start,
        mut extras: StartExtras,
        title: String,
        sink: impl EventSink,
    ) -> Result<SessionInfo, ManagerError> {
        let Start { kind, mode } = start;
        let size = size.clamped();
        let profile = &agent.profile;
        // A folder fixed in the profile wins over the one chosen in the sidebar.
        let cwd = profile
            .cwd
            .clone()
            .or(cwd)
            .unwrap_or_else(|| self.home.clone());
        check_cwd(&cwd)?;

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
            info.mode = mode;
            info.sub_agent.clone_from(&extras.sub_agent);
            set_cli_session(&mut info, &agent, plan.cli_session_id.clone());
            sessions.insert(
                id,
                Entry {
                    info,
                    agent: agent.clone(),
                    generation: 0,
                    runtime: None,
                    size,
                    instructions: extras.instructions.clone(),
                    sub_agent: extras.sub_agent.clone(),
                    kill_requested: false,
                },
            );
            plan
        };
        // A first prompt belongs to a fresh conversation, never to a resumed one.
        if plan.resume {
            extras.prompt = None;
        }
        let launch = Launch {
            args: &plan.args,
            resume: plan.resume,
            kind,
            mode,
            extras,
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
            Ok(runtime) => self.shared.attach(id, 0, runtime),
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
        let (agent, cwd, generation, kind, mode, plan) = {
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
            entry.size = size.clamped();
            entry.kill_requested = false;
            // A fresh conversation under a new id: the old id may already name one.
            let plan = Plan::normal(&entry.agent.profile, None);
            set_cli_session(&mut entry.info, &entry.agent, plan.cli_session_id.clone());
            (
                entry.agent.clone(),
                entry.info.cwd.clone(),
                entry.generation,
                kind,
                (
                    entry.info.mode,
                    (entry.instructions.clone(), entry.sub_agent.clone()),
                ),
                plan,
            )
        };
        let (mode, instructions) = mode;
        match spawn(
            &self.shared,
            id,
            generation,
            &agent,
            &Launch::normal(&plan.args, kind, mode, instructions),
            &cwd,
            size,
            Arc::new(sink),
        ) {
            Ok(runtime) => self.shared.attach(id, generation, runtime),
            Err(error) => {
                if let Some(entry) = lock(&self.shared.sessions).get_mut(&id) {
                    let _ = entry.info.mark_exited(None);
                    self.shared.gate_ended(id);
                }
                Err(error)
            }
        }
    }

    /// Queues `data` for the session's input thread without waiting. A full queue (the program
    /// does not read) is [`ManagerError::Busy`]; more than [`MAX_WRITE`] bytes are refused.
    pub fn write(&self, id: SessionId, data: &str) -> Result<(), ManagerError> {
        if data.len() > MAX_WRITE {
            return Err(ManagerError::InputTooLarge {
                len: data.len(),
                max: MAX_WRITE,
            });
        }
        let sessions = lock(&self.shared.sessions);
        let runtime = running(&sessions, id)?;
        runtime
            .input
            .try_send(data.as_bytes().to_vec())
            .map_err(|error| match error {
                TrySendError::Full(_) => ManagerError::Busy(id),
                TrySendError::Disconnected(_) => ManagerError::NotRunning(id),
            })
    }

    pub fn resize(&self, id: SessionId, size: TermSize) -> Result<(), ManagerError> {
        let size = size.clamped();
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
        let title = session_title(title)?;
        let mut sessions = lock(&self.shared.sessions);
        let entry = sessions
            .get_mut(&id)
            .ok_or(ManagerError::UnknownSession(id))?;
        entry.info.title = title;
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

    /// The transcript target of a session and its program's name, for the session report
    /// (`crate::report`); the same sessions as the sub-agent view.
    pub fn report_target(
        &self,
        id: SessionId,
    ) -> Result<(subagents::Target, String), ManagerError> {
        let target = self.sub_agent_target(id)?;
        let program = lock(&self.shared.sessions)
            .get(&id)
            .map(|entry| entry.agent.profile.name.clone())
            .ok_or(ManagerError::UnknownSession(id))?;
        Ok((target, program))
    }

    /// Every session whose program keeps transcripts Hangar can read (the same ones as the
    /// sub-agent view), for the context meter (`context_usage`).
    pub fn context_targets(&self) -> Vec<(SessionId, subagents::Target)> {
        let ids: Vec<SessionId> = lock(&self.shared.sessions).keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| Some((id, self.sub_agent_target(id).ok()?)))
            .collect()
    }

    /// [`Self::context_targets`] with each session's title and program name, for the cost
    /// overview (`cost_service`).
    pub fn cost_targets(&self) -> Vec<crate::cost_service::CostTarget> {
        let names = self.profile_names();
        let infos: HashMap<SessionId, SessionInfo> = self
            .list()
            .into_iter()
            .map(|info| (info.id, info))
            .collect();
        self.context_targets()
            .into_iter()
            .filter_map(|(id, target)| {
                let info = infos.get(&id)?;
                Some(crate::cost_service::CostTarget {
                    id,
                    target,
                    title: info.title.clone(),
                    program: names
                        .get(&info.profile_id)
                        .cloned()
                        .unwrap_or_else(|| info.profile_id.clone()),
                })
            })
            .collect()
    }

    /// What the process tree of each running session uses right now (`resources.rs`). The trees
    /// are cloned under the session lock and asked after it is released: a few system calls per
    /// session on Windows, one pass over `/proc` on Linux.
    pub fn tree_usages(&self) -> Vec<(SessionId, TreeUsage)> {
        let (ids, trees): (Vec<SessionId>, Vec<Arc<ProcTree>>) = lock(&self.shared.sessions)
            .iter()
            .filter_map(|(id, entry)| {
                Some((*id, Arc::clone(entry.runtime.as_ref()?.tree.as_ref()?)))
            })
            .unzip();
        ids.into_iter()
            .zip(proctree::usages(&trees))
            .filter_map(|(id, usage)| Some((id, usage?)))
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

    /// Ends all sessions when the app closes, and returns only once every tree got its last
    /// signal ([`proctree::end_all`]): on Unix at most [`proctree::KILL_GRACE`] for all sessions
    /// together, then SIGKILL, so no program that ignores SIGHUP outlives Hangar.
    pub fn shutdown(&self) {
        let entries: Vec<Entry> = {
            let mut sessions = lock(&self.shared.sessions);
            let entries: Vec<Entry> = sessions.drain().map(|(_, e)| e).collect();
            for entry in &entries {
                self.shared.gate_ended(entry.info.id);
            }
            entries
        };
        let runtimes: Vec<Runtime> = entries.into_iter().filter_map(|e| e.runtime).collect();
        let mut trees = Vec::new();
        for runtime in &runtimes {
            runtime.flow.close();
            match &runtime.tree {
                Some(tree) => trees.push(Arc::clone(tree)),
                // Failure usually means the process was already gone.
                None => {
                    let _ = lock(&runtime.killer).kill();
                }
            }
        }
        proctree::end_all(&trees);
        drop(runtimes);
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

/// The installed version of a program whose gate settings depend on it (Claude Code's hook
/// `onFailure`, [`crate::gate::claude_blocks_on_failure`]), from the cached version query;
/// `None` for every other program, without a version source or when it is not known.
fn gate_version(shared: &Shared, agent: &Agent, program: &Path) -> Option<String> {
    if agent.capabilities.gate_format() != Some(crate::agent_catalog::HookFormat::ClaudeSettings) {
        return None;
    }
    let source = lock(&shared.versions).clone()?;
    source
        .versions(&[(agent.clone(), program.to_path_buf())])
        .remove(agent.id())
        .flatten()
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
    let args = launch.args.to_vec();
    // Only team sessions get the gate; update sessions never: they run an installer, not the
    // agent.
    let own_hooks = agent
        .capabilities
        .gate_format()
        .is_some_and(|format| format.conflicts_with(&args, &profile.env, &profile.secret_env));
    let plan = match (launch.kind, launch.mode, shared.launch_gate()) {
        (SessionKind::Agent, SessionMode::Team, Some(gate)) if own_hooks && gate.applies(agent) => {
            GatePlan::Unavailable(GateStatus::ProfileSettings)
        }
        (SessionKind::Agent, SessionMode::Team, Some(gate)) => {
            let version = gate_version(shared, agent, &program);
            gate.prepare(id, agent, &program, &args, version.as_deref())
        }
        _ => GatePlan::Skip,
    };
    let gate_status = plan.status();
    let gate = match plan {
        GatePlan::Launch(launch) => Some(launch),
        GatePlan::Skip | GatePlan::Unavailable(_) => None,
    };
    // Update sessions run an installer, never the agent: neither instructions nor a prompt.
    let batch = batch::is_batch(&program);
    let models = if launch.kind == SessionKind::Agent {
        shared.model_launch(agent, batch)
    } else {
        agent_models::ModelLaunch::default()
    };
    // Private and team sessions alike may send (partner projects, link groups); an installer
    // never.
    let send = match (launch.kind, shared.launch_send()) {
        (SessionKind::Agent, Some(send)) => send.prepare(id, cwd, &program),
        _ => SendLaunch::default(),
    };
    let extra = if launch.kind == SessionKind::Agent {
        let extras = StartExtras {
            hint: send.hint.clone(),
            ..launch.extras.clone()
        };
        let (mut before, after) = extra_args(agent, &program, &extras);
        // Config overrides go before a subcommand such as `codex resume --last`, like HANGAR.md.
        before.splice(0..0, models.args.iter().cloned());
        (before, after)
    } else {
        (Vec::new(), Vec::new())
    };
    let parts = ArgParts {
        start: args,
        gate: gate.as_ref(),
        cwd_flag: agent
            .cwd_flag
            .map(|flag| (flag, cwd.to_string_lossy().into_owned())),
    };
    let args = parts.fitting(&program.to_string_lossy(), extra, &agent.profile.name);
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
    let no_cwd_lookup = no_cwd_lookup(launch.kind, agent.source, batch, &program);
    crate::child_env::apply(&mut command, &profile.env, no_cwd_lookup);
    for (name, value) in &models.env {
        command.env(name, value);
    }
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
    for (name, value) in &send.env {
        command.env(name, value.as_str());
    }
    // At the end, after the profile's own `PATH`: every program of the user's comes first, and
    // `hangar-send` is a name no other program has.
    if let Some(path) = send
        .path_dir
        .as_deref()
        .and_then(|dir| path_with(dir, command.get_env("PATH")))
    {
        command.env("PATH", path);
    }
    let started = Instant::now();
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| spawn_failed(&program, error.as_ref()))?;
    // Close our end of the slave side, otherwise EOF never arrives on Unix.
    drop(pair.slave);

    // Without the tree, closing the session would leave the program's children running: such a
    // session does not start at all.
    let tree = match child.process_id().map(ProcTree::attach) {
        Some(Ok(tree)) => Some(Arc::new(tree)),
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
    let (input_tx, input_rx) = sync_channel(INPUT_DEPTH);
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
    let tree_wait = tree.clone();
    spawn_thread(format!("pty-wait-{id}"), move || {
        let code = child.wait().ok().map(|status| status.exit_code());
        // From now on the child's id may name another process (Unix): no more signals to it.
        if let Some(tree) = &tree_wait {
            tree.mark_reaped();
        }
        // Measured here, not in the batcher: there backpressure can delay the report.
        let at = Instant::now();
        // ConPTY closes the output only once the pseudo console is closed. Without this
        // the read thread would never reach EOF on Windows. Closing it can block until ConPTY
        // has flushed, so it happens outside the lock that resize and kill also take.
        let master = lock(&master_wait).take();
        drop(master);
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

/// A session title is at most this many characters; longer ones are cut. The title goes into
/// every session list, the work log and the workspace file.
pub const MAX_TITLE_CHARS: usize = 256;

/// The title as stored: trimmed, not empty, cut to [`MAX_TITLE_CHARS`].
pub fn session_title(title: &str) -> Result<String, ManagerError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(ManagerError::EmptyTitle);
    }
    Ok(title
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_catalog::HookFormat;

    #[test]
    fn session_titles_are_trimmed_and_bounded() {
        assert_eq!(session_title("  Review  ").unwrap(), "Review");
        assert!(matches!(
            session_title(" \t "),
            Err(ManagerError::EmptyTitle)
        ));
        let long = "\u{e4}".repeat(MAX_TITLE_CHARS + 50);
        let cut = session_title(&long).unwrap();
        assert_eq!(cut.chars().count(), MAX_TITLE_CHARS);
        // Cut at a character, never inside one.
        assert!(cut.chars().all(|c| c == '\u{e4}'));
        let exact = "x".repeat(MAX_TITLE_CHARS);
        assert_eq!(session_title(&exact).unwrap(), exact);
    }

    fn prompted_agent(extra: &str) -> Agent {
        let source = format!("[[agent]]\nid = 'p'\nname = 'P'\ncommands = ['p']\n{extra}");
        Agent::from_catalog(crate::agent_catalog::parse(&source).unwrap().remove(0))
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn extras_reach_only_programs_that_take_them() {
        let tmp = crate::test_support::TempDir::new("manager-extras");
        let file = tmp.path().join(instructions::FILE_NAME);
        std::fs::write(&file, "<!-- hidden -->\nUse tabs.\n").unwrap();
        let program = Path::new("/bin/p");
        let extras = StartExtras {
            instructions: Some(file.clone()),
            prompt: Some("Check upstream.".into()),
            sub_agent: None,
            hint: None,
        };
        let both = prompted_agent(
            "instruction_args = ['--append-system-prompt', '{text}']\nprompt_args = ['{prompt}']\n",
        );
        assert_eq!(
            extra_args(&both, program, &extras),
            (
                strings(&["--append-system-prompt", "Use tabs."]),
                strings(&["Check upstream."])
            )
        );
        let by_file = prompted_agent("instruction_args = ['--read', '{file}']\n");
        assert_eq!(
            extra_args(&by_file, program, &extras),
            (
                vec!["--read".to_owned(), file.to_string_lossy().into_owned()],
                Vec::new()
            )
        );
        // A program without either field gets nothing, whatever the window asked for.
        let plain = prompted_agent("");
        assert_eq!(
            extra_args(&plain, program, &extras),
            (Vec::new(), Vec::new())
        );
        // Off, or an empty file: no arguments at all.
        assert_eq!(
            extra_args(&both, program, &StartExtras::default()),
            (Vec::new(), Vec::new())
        );
        std::fs::write(&file, "<!-- only the template -->\n").unwrap();
        assert_eq!(extra_args(&both, program, &extras).0, Vec::<String>::new());
    }

    #[test]
    fn the_partner_hint_rides_along_with_hangar_md_only() {
        let tmp = crate::test_support::TempDir::new("manager-hint");
        let file = tmp.path().join(instructions::FILE_NAME);
        std::fs::write(&file, "Use tabs.\n").unwrap();
        let program = Path::new("/bin/p");
        let inline = prompted_agent("instruction_args = ['--append-system-prompt', '{text}']\n");
        let with = |instructions: Option<PathBuf>| StartExtras {
            instructions,
            hint: Some("Hangar: partners.".into()),
            ..StartExtras::default()
        };
        assert_eq!(
            extra_args(&inline, program, &with(Some(file.clone()))).0,
            strings(&["--append-system-prompt", "Use tabs.\n\nHangar: partners."])
        );
        // HANGAR.md on but empty: the hint alone.
        std::fs::write(&file, "<!-- only the template -->\n").unwrap();
        assert_eq!(
            extra_args(&inline, program, &with(Some(file.clone()))).0,
            strings(&["--append-system-prompt", "Hangar: partners."])
        );
        // HANGAR.md off: nothing of Hangar's own reaches the agent's instructions.
        assert!(extra_args(&inline, program, &with(None)).0.is_empty());
        // A program that reads the file gets the file as it is.
        let by_file = prompted_agent("instruction_args = ['--read', '{file}']\n");
        std::fs::write(&file, "Use tabs.\n").unwrap();
        assert_eq!(
            extra_args(&by_file, program, &with(Some(file.clone()))).0,
            vec!["--read".to_owned(), file.to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn the_send_folder_goes_last_on_path() {
        let dir = Path::new(if cfg!(windows) {
            r"C:\data\bin"
        } else {
            "/data/bin"
        });
        let rest = std::env::join_paths([
            Path::new(if cfg!(windows) {
                r"C:\Windows"
            } else {
                "/usr/bin"
            }),
            Path::new(if cfg!(windows) { r"C:\tools" } else { "/bin" }),
        ])
        .unwrap();
        let joined = path_with(dir, Some(&rest)).unwrap();
        let parts: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(parts.len(), 3);
        // The user's own folders keep their order in front of it.
        assert_eq!(parts[2], dir);
        assert_eq!(
            parts[..2],
            std::env::split_paths(&rest).collect::<Vec<_>>()[..]
        );
        assert_eq!(path_with(dir, None).unwrap(), std::ffi::OsString::from(dir));
        // A folder with the separator in its name cannot go on PATH.
        let bad = format!(
            "{}x{}y",
            dir.display(),
            if cfg!(windows) { ';' } else { ':' }
        );
        if cfg!(windows) {
            // Windows quotes such a part instead of refusing it.
            assert!(path_with(Path::new(&bad), None).is_some());
        } else {
            assert!(path_with(Path::new(&bad), None).is_none());
        }
    }

    #[test]
    fn the_sub_agent_goes_before_the_first_prompt() {
        let program = Path::new("/bin/p");
        let agent =
            prompted_agent("prompt_args = ['{prompt}']\nagent_args = ['--agent', '{agent}']\n");
        let extras = StartExtras {
            instructions: None,
            prompt: Some("Go.".into()),
            sub_agent: Some("backend".into()),
            hint: None,
        };
        assert_eq!(
            extra_args(&agent, program, &extras),
            (Vec::new(), strings(&["--agent", "backend", "Go."]))
        );
        // A program without agent_args never gets the option.
        let plain = prompted_agent("prompt_args = ['{prompt}']\n");
        assert_eq!(
            extra_args(&plain, program, &extras),
            (Vec::new(), strings(&["Go."]))
        );
    }

    #[test]
    fn a_too_long_command_line_loses_the_instructions_first() {
        let gate = GateLaunch {
            prepend: strings(&["--hook-trust"]),
            args: strings(&["--settings", "gate.json"]),
            env: Vec::new(),
        };
        let parts = ArgParts {
            start: strings(&["resume", "--last"]),
            gate: Some(&gate),
            cwd_flag: Some(("--cd", "/work".to_owned())),
        };
        let before = strings(&["--append-system-prompt=Use tabs."]);
        let after = strings(&["Check upstream."]);
        // Everything in its place.
        assert_eq!(
            parts.fitting("p", (before.clone(), after.clone()), "P"),
            strings(&[
                "--hook-trust",
                "--append-system-prompt=Use tabs.",
                "resume",
                "--last",
                "--settings",
                "gate.json",
                "Check upstream.",
                "--cd",
                "/work",
            ])
        );
        // The gate's arguments count: instructions that fit alone but not with them go.
        let base = instructions::command_line_units("p", &parts.compose(&[], &after));
        let room = instructions::COMMAND_LINE_MAX - base - 1;
        let exactly = vec!["x".repeat(room)];
        assert_eq!(
            parts.fitting("p", (exactly.clone(), after.clone()), "P"),
            parts.compose(&exactly, &after)
        );
        let one_more = vec!["x".repeat(room + 1)];
        assert_eq!(
            parts.fitting("p", (one_more, after.clone()), "P"),
            parts.compose(&[], &after)
        );
        // Quoting counts: the same length of quotes does not fit.
        let quoted = vec!["\"".repeat(room)];
        assert_eq!(
            parts.fitting("p", (quoted, after.clone()), "P"),
            parts.compose(&[], &after)
        );
        // A prompt that alone is too long goes as well; the start itself stays.
        let huge = vec!["y".repeat(instructions::COMMAND_LINE_MAX)];
        assert_eq!(
            parts.fitting("p", (before, huge), "P"),
            parts.compose(&[], &[])
        );
    }

    #[test]
    fn a_profile_inherits_prompting_only_for_the_catalogs_program() {
        let catalog = crate::agent_catalog::parse(
            "[[agent]]\nid = 'p'\nname = 'P'\ncommands = ['p']\nprompt_args = ['{prompt}']\n",
        )
        .unwrap();
        let profile = |command: &str| {
            let text = format!("id = 'p'\nname = 'Mine'\ncommand = '{command}'\n");
            toml::from_str::<Profile>(&text).unwrap()
        };
        let same = merge_all(vec![profile("p")], catalog.clone(), Vec::new());
        assert_eq!(same[0].prompting.prompt_args, Some(strings(&["{prompt}"])));
        let wrapper = merge_all(vec![profile("my-wrapper")], catalog, Vec::new());
        assert_eq!(wrapper[0].prompting.prompt_args, None);
    }

    #[test]
    fn own_settings_are_found_in_both_spellings_only() {
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        let has_own_settings = |list: &[String]| {
            HookFormat::ClaudeSettings.conflicts_with(list, &std::collections::BTreeMap::new(), &[])
        };
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
    fn a_fixed_folder_is_shown_only_when_nothing_else_stands_in_the_way() {
        let agent = |cwd: &str| {
            let text = format!("id = 'x'\nname = 'X'\ncommand = 'x'\ncwd = '{cwd}'\n");
            Agent::from_user(toml::from_str(&text).unwrap())
        };
        let fixed = agent("/work/fixed");
        assert_eq!(shown_team_block(&fixed, None), Some(TeamBlock::FixedFolder));
        assert_eq!(
            shown_team_block(&fixed, Some(TeamBlock::UnknownVersion)),
            Some(TeamBlock::UnknownVersion)
        );
        assert_eq!(shown_team_block(&agent(""), None), None);
        let free =
            Agent::from_user(toml::from_str("id = 'y'\nname = 'Y'\ncommand = 'y'\n").unwrap());
        assert_eq!(shown_team_block(&free, None), None);
        assert_eq!(
            shown_team_block(&free, Some(TeamBlock::NoHooks)),
            Some(TeamBlock::NoHooks)
        );
    }

    #[test]
    fn only_shells_of_the_users_own_profiles_keep_the_program_lookup_in_the_folder() {
        use SessionKind::{Agent as A, Update as U};
        // The resolved program is a native path; a Windows path has no file name on Unix.
        let (shell, agent) = if cfg!(windows) {
            (
                Path::new(r"C:\Windows\System32\cmd.exe"),
                Path::new(r"C:\Users\x\.local\bin\claude.exe"),
            )
        } else {
            (
                Path::new("/bin/bash"),
                Path::new("/home/x/.local/bin/claude"),
            )
        };
        // (kind, source, batch, program) -> protected
        let table = [
            (A, Source::User, false, shell, false),
            (A, Source::User, true, shell, true),
            // A user profile that starts an agent (`command = "claude"`) is protected as well.
            (A, Source::User, false, agent, true),
            (A, Source::Catalog, false, shell, true),
            (A, Source::Catalog, false, agent, true),
            (A, Source::Catalog, true, agent, true),
            (U, Source::User, false, shell, true),
            (U, Source::Catalog, false, agent, true),
        ];
        for (kind, source, batch, program, expected) in table {
            assert_eq!(
                no_cwd_lookup(kind, source, batch, program),
                expected,
                "{kind:?} {source:?} batch={batch} {program:?}"
            );
        }
    }

    #[test]
    fn shells_are_known_by_their_file_name() {
        // Paths as the platform resolves them: `\` separates folders on Windows only.
        #[cfg(windows)]
        let (shells, programs) = (
            [
                r"C:\Windows\System32\cmd.exe",
                r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.EXE",
                r"C:\Program Files\PowerShell\7\pwsh.exe",
                r"C:\Windows\System32\wsl.exe",
                r"C:\Program Files\Git\bin\bash.exe",
                r"C:\Program Files\Git\git-bash.exe",
                r"C:\tools\nu.exe",
            ],
            [
                r"C:\Users\x\.local\bin\claude.exe",
                r"C:\tools\cmd-agent.exe",
                r"C:\tools\bash.cmd",
            ],
        );
        #[cfg(not(windows))]
        let (shells, programs) = (
            [
                "/bin/bash",
                "/usr/bin/pwsh",
                "/opt/homebrew/bin/nu",
                "/usr/local/bin/PowerShell",
            ],
            [
                "/home/x/.local/bin/claude",
                "/opt/tools/cmd-agent",
                "/opt/tools/bash.cmd",
            ],
        );
        for shell in shells
            .into_iter()
            .chain(["/bin/sh", "/usr/bin/zsh", "/usr/local/bin/fish"])
        {
            assert!(is_interactive_shell(Path::new(shell)), "{shell}");
        }
        for program in programs.into_iter().chain(["/usr/bin/codex", ""]) {
            assert!(!is_interactive_shell(Path::new(program)), "{program}");
        }
    }

    #[test]
    fn terminal_sizes_are_clamped() {
        let huge = TermSize {
            cols: u16::MAX,
            rows: u16::MAX,
        };
        assert_eq!(
            huge.clamped(),
            TermSize {
                cols: MAX_COLS,
                rows: MAX_ROWS
            }
        );
        let pty = huge.to_pty();
        assert_eq!((pty.cols, pty.rows), (4096, 2048));
        let zero = TermSize { cols: 0, rows: 0 }.to_pty();
        assert_eq!((zero.cols, zero.rows), (1, 1));
        let normal = TermSize {
            cols: 120,
            rows: 40,
        };
        assert_eq!(normal.clamped(), normal);
    }

    #[test]
    fn a_failed_start_names_the_program_and_the_code_only() {
        let program = Path::new(r"C:\tools\agent.exe");
        // The shape of portable-pty's ConPTY error: the whole command line, then the OS error.
        let text = "CreateProcessW `\"agent.exe\" HANGAR-SECRET-MARKER` in cwd `None` failed: \
                    The system cannot find the file specified. (os error 2)";
        let error: Box<dyn std::error::Error> = text.into();
        let shown = spawn_failed(program, error.as_ref()).to_string();
        assert!(!shown.contains("HANGAR-SECRET-MARKER"), "{shown}");
        assert!(shown.contains(r"C:\tools\agent.exe"), "{shown}");
        assert!(shown.contains("OS error 2"), "{shown}");
        // An io::Error in the chain gives its code directly.
        let io = io::Error::from_raw_os_error(13);
        let shown = spawn_failed(program, &io).to_string();
        assert!(shown.contains("OS error 13"), "{shown}");
        let plain: Box<dyn std::error::Error> = "HANGAR-SECRET-MARKER broke".into();
        let shown = spawn_failed(program, plain.as_ref()).to_string();
        assert!(!shown.contains("HANGAR-SECRET-MARKER"), "{shown}");
        assert_eq!(os_error_code("x (os error 5) y (os error 32)"), Some(32));
        assert_eq!(os_error_code("no code"), None);
    }

    #[cfg(windows)]
    #[test]
    fn network_and_device_folders_are_recognized() {
        for remote in [
            r"\\host\share\project",
            r"\\?\UNC\host\share\project",
            r"\\.\PhysicalDrive0",
            r"\\?\GLOBALROOT\Device",
        ] {
            assert!(is_network_or_device(Path::new(remote)), "{remote}");
        }
        for local in [r"C:\work", r"\\?\C:\work", r"D:\", r"relative\folder"] {
            assert!(!is_network_or_device(Path::new(local)), "{local}");
        }
    }

    #[test]
    fn working_folders_must_be_local_folders() {
        let here = std::env::current_dir().unwrap();
        assert!(check_cwd(&here).is_ok());
        assert!(matches!(
            check_cwd(Path::new("relative")),
            Err(ManagerError::BadCwd(_))
        ));
        assert!(matches!(
            check_cwd(&here.join("hangar-missing-folder")),
            Err(ManagerError::BadCwd(_))
        ));
        #[cfg(windows)]
        for remote in [
            r"\\hangar-test.invalid\share",
            "//hangar-test.invalid/share/x",
        ] {
            assert!(
                matches!(
                    check_cwd(Path::new(remote)),
                    Err(ManagerError::RemoteCwd(_))
                ),
                "{remote}"
            );
        }
    }

    #[test]
    fn local_unix_paths_are_never_network_paths() {
        assert!(!is_network_or_device(Path::new("/home/user/work")));
        assert!(!is_network_or_device(Path::new("work")));
    }

    #[cfg(windows)]
    #[test]
    fn a_network_folder_is_refused_before_it_is_touched() {
        let manager = SessionManager::new(
            Vec::new(),
            crate::agent_catalog::parse(
                "[[agent]]\nid = 'p'\nname = 'P'\ncommands = ['hangar-not-needed']\n",
            )
            .unwrap(),
            std::env::temp_dir(),
        );
        let size = TermSize { cols: 80, rows: 24 };
        // The host name is reserved (RFC 2606); the check comes before any lookup.
        let error = manager
            .create(
                "p",
                Some(PathBuf::from(r"\\hangar-test.invalid\share")),
                size,
                |_| {},
            )
            .unwrap_err();
        assert!(matches!(error, ManagerError::RemoteCwd(_)), "{error}");
    }

    #[test]
    fn folder_errors_carry_a_stable_code_for_the_window() {
        let text = |error: ManagerError| serde_json::to_value(error).unwrap();
        let remote = text(ManagerError::RemoteCwd(PathBuf::from(r"\\server\share")));
        assert!(
            remote.as_str().unwrap().starts_with("[remote_cwd] "),
            "{remote}"
        );
        let missing = text(ManagerError::BadCwd(PathBuf::from("/gone")));
        assert!(
            missing.as_str().unwrap().starts_with("[bad_cwd] "),
            "{missing}"
        );
        // Errors without a code stay plain text.
        assert_eq!(
            text(ManagerError::EmptyTitle),
            "The title must not be empty"
        );
    }

    #[test]
    fn argv_line_is_dimmed_and_plain() {
        let line = argv_line(
            Path::new("/usr/bin/npm"),
            &["install".into(), "a b".into(), "x\x1b[31m".into()],
        );
        assert_eq!(line, "\x1b[2m/usr/bin/npm install \"a b\" x[31m\x1b[0m\r\n");
    }

    /// Counts kills instead of signalling anything, so a test sees whether `attach` tried one.
    #[derive(Debug)]
    struct CountingKiller(Arc<AtomicU64>);

    impl ChildKiller for CountingKiller {
        fn kill(&mut self) -> io::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(Self(Arc::clone(&self.0)))
        }
    }

    /// A runtime without a process: its flow and kill counter tell what `attach` did with it.
    fn fake_runtime(gate: GateStatus) -> (Runtime, Arc<Flow>, Arc<AtomicU64>, Receiver<Vec<u8>>) {
        let flow = Arc::new(Flow::new(HIGH_WATER, FLOW_STALL));
        let kills = Arc::new(AtomicU64::new(0));
        let (input, input_rx) = sync_channel(INPUT_DEPTH);
        let runtime = Runtime {
            master: Arc::new(Mutex::new(None)),
            input,
            killer: Mutex::new(Box::new(CountingKiller(Arc::clone(&kills)))),
            tree: None,
            flow: Arc::clone(&flow),
            gate,
        };
        (runtime, flow, kills, input_rx)
    }

    #[test]
    fn a_stale_attach_neither_replaces_nor_kills_the_live_process() {
        let shared = Shared::new(Arc::new(KeyringStore::default()));
        let id: SessionId = 7;
        // The state right after `resume_fallback`: the resume attempt (generation 0) exited, the
        // fallback (generation 1) is spawned but not yet attached.
        let mut info = SessionInfo::new(id, "p".into(), "T".into(), PathBuf::from("/w"), 0);
        info.mark_exited(Some(1)).unwrap();
        info.mark_restarting().unwrap();
        lock(&shared.sessions).insert(
            id,
            Entry {
                info,
                agent: prompted_agent(""),
                generation: 1,
                runtime: None,
                size: TermSize { cols: 80, rows: 24 },
                instructions: None,
                sub_agent: None,
                kill_requested: false,
            },
        );

        let (stale, stale_flow, stale_kills, stale_input) = fake_runtime(GateStatus::Active);
        let reported = shared.attach(id, 0, stale).unwrap();
        // Left alone: not installed, not marked running, its gate not taken over.
        assert_eq!(reported.status, Status::Starting);
        assert_eq!(reported.gate, GateStatus::None);
        {
            let sessions = lock(&shared.sessions);
            let entry = &sessions[&id];
            assert!(entry.runtime.is_none());
            assert_eq!(entry.generation, 1);
        }
        // Closed (its batch thread stops waiting for acknowledgements) and dropped, never
        // killed: its pid is already released.
        assert!(!stale_flow.wait_below_high());
        assert_eq!(stale_kills.load(Ordering::SeqCst), 0);
        assert!(stale_input.recv().is_err());

        // The fallback's own attach then installs it as usual.
        let (live, live_flow, live_kills, _live_input) = fake_runtime(GateStatus::NoEndpoint);
        let reported = shared.attach(id, 1, live).unwrap();
        assert_eq!(reported.status, Status::Running);
        assert_eq!(reported.gate, GateStatus::NoEndpoint);
        {
            let sessions = lock(&shared.sessions);
            let installed = sessions[&id].runtime.as_ref().unwrap();
            assert!(Arc::ptr_eq(&installed.flow, &live_flow));
        }
        assert!(live_flow.wait_below_high());
        assert_eq!(live_kills.load(Ordering::SeqCst), 0);
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
