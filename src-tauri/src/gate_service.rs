//! Runs the agent gate (domain in `gate.rs`): a hook endpoint on 127.0.0.1, started on the first
//! launch that needs it, that holds an agent's edit while the person decides in the window.
//!
//! The gate coordinates the user's own sessions; it is not a security boundary. Everything that
//! goes wrong lets the edit through (Claude Code treats a failed hook so), and an agent can change
//! files through its shell without any hook.
//!
//! Every session that gets the gate has its own random token. It reaches the agent only through
//! the PTY child's environment ([`TOKEN_ENV`] for the agents' HTTP hooks, which put it into a
//! header, [`HELPER_ENV`] for the helper command `hangar gate-hook` and the in-process extensions
//! of Pi, Oh My Pi and OpenCode); it is never written to a
//! file, an argument or a log and never sent to the window. A request without a known token is
//! refused before its body is read.
//!
//! The same endpoint serves the send route of `hangar-send` (cross-project messages, ADR 0012,
//! and link groups, ADR 0014) for every agent session, private ones included, so it also starts
//! without the gate ([`GateService::send_token`]). Each token carries one scope
//! ([`TokenScope`]): a send token opens no hook route and a hook token sends no message.
//!
//! Notice routes (ADR 0016): Claude Code sessions, private ones included, report through
//! report-only hooks when a permission or input dialog appears and when it is gone, with a token
//! of scope [`TokenScope::Notify`] in [`NOTIFY_TOKEN_ENV`]. The routes never decide: they answer
//! an empty 200, read at most [`gate::MAX_NOTIFY_BODY_BYTES`] of the body and look only at the
//! event's and the notification's name. The window hears [`PERMISSION_EVENT`] (no payload) and
//! reads the set with [`GateService::permission_waiting`].
//!
//! How the hooks reach one session depends on the program's [`HookFormat`] (recipes in
//! `hook_recipes.rs`): start arguments, and for some formats files in a folder of the session's
//! own under `<data>/gate/sessions/`, removed when the session's process ends.
//!
//! Shell commands: Claude Code's hook (matcher `*`, every tool) and the extensions' shell tool
//! reach the same route with the command line instead of a path. A known shell tool, or any
//! other tool with a string `tool_input.command` that is no edit tool, goes to the command guard
//! (`command_guard.rs`, rules in `command-guard.toml`), which decides whether the person is
//! asked; a held command that gets no answer in time is denied (fail safe). Other tools that are
//! no edits pass at once.
//!
//! Connections: one thread each. Until its head is read and its token known, a connection holds
//! one of [`MAX_UNAUTHENTICATED`] slots and has [`Limits::head`] in total for the head, however
//! slowly it sends; when all slots are taken, the oldest of them is closed for the newcomer
//! once it waited [`Limits::grace`] for its head (a hook sends its head at once, so a burst of
//! hooks is never cut), and past [`MAX_HEAD_THREADS`] busy head threads with none of them
//! silent that long a new connection is closed at once. A known session then moves to one of [`MAX_CONNECTIONS`]
//! slots, at most [`MAX_PER_SESSION`] of them its own, with [`Limits::body`] for its body. The
//! accept thread never reads or blocks. A held request checks every [`WAIT_TICK`] whether the
//! agent still waits, so an interrupted agent does not leave a card behind.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zeroize::Zeroizing;

use crate::agent_catalog::{Agent, HookFormat};
use crate::claims::WorktreeClaims;
use crate::command_guard::{
    self, Hit, Place, RuleProblem, RuleSet, RuleSummary, Style, RULE_HANGAR_CONFIG, RULE_TOO_LONG,
};
use crate::gate::{
    self, Answer, AskRule, CommandConflict, CommandText, EditRequest, Gate, GateSnapshot, Opened,
    Outcome, TokenScope, Waiting, DECISION_TIMEOUT, NOTIFY_TOKEN_ENV, ROUTE_NOTIFY_ASKS,
    ROUTE_NOTIFY_CLEAR, ROUTE_PRE_TOOL_USE, ROUTE_PRE_TOOL_USE_JSON, ROUTE_USER_PROMPT_SUBMIT,
    ROUTE_USER_PROMPT_SUBMIT_JSON, TOKEN_ENV,
};
use crate::gate_rules::{self, GateRule, RuleAnswer, RuleBook, RuleEffect, RuleScope};
use crate::hook_http::{self, Head, Status};
use crate::hook_recipes::{
    self as recipes, Shell, AGENT_CODEX, AGENT_DROID, AGENT_GEMINI, EVENT_BEFORE_TOOL,
    EVENT_PRE_TOOL_USE, HELPER_ENV,
};
use crate::lock;
use crate::manager::{GateLaunch, GatePlan, LaunchGate};
use crate::partners::{read_send_request, SendRequest, MAX_SEND_BODY_BYTES, ROUTE_SEND};
use crate::permission_wait::PermissionWaits;
use crate::session::{GateStatus, SessionId};
use crate::team_share::TeammateFile;
use crate::team_tasks::TeamTask;

/// Event to the main window with the newest [`GateSnapshot`].
pub const EVENT: &str = "gate-changed";
/// Signal to the main window that the sessions waiting for a dialog changed (ADR 0016); no
/// payload, the window reads them with `permission_waiting`.
pub const PERMISSION_EVENT: &str = "permission-waits-changed";
/// How often old dialog signals are dropped ([`crate::permission_wait::SIGNAL_TTL`]).
const SIGNAL_EXPIRY_TICK: Duration = Duration::from_secs(30);
/// Connections of known sessions handled at the same time. One more is denied while the gate
/// is on (`Inner::overloaded`): an unanswered hook would let its call go ahead unchecked.
pub const MAX_CONNECTIONS: usize = 128;
/// Connections whose token is not known yet that may stay silent. With more open, a new one
/// closes the oldest once that has waited [`HEAD_GRACE`] for its head: a hook sends its head at
/// once, so silent clients cannot keep the hooks out, and a burst of parallel hooks is not cut.
pub const MAX_UNAUTHENTICATED: usize = 8;
/// Threads reading a head at once, closed connections that have not noticed yet included; over
/// it a new connection is closed unanswered unless a silent one makes room. Bounds what a flood
/// of connections costs; well above the hooks an agent sends at once.
pub const MAX_HEAD_THREADS: usize = 128;
/// How long a connection may wait for its head before a newer one may take its place. A hook's
/// head arrives with the connection, so only a client that holds it silently is closed.
const HEAD_GRACE: Duration = Duration::from_secs(1);
/// Connections one known session holds at once, so one session cannot take every slot. Well
/// above what an agent opens: Claude Code hooks every tool and runs several tools and
/// sub-agents at once, and a quick hook holds its connection for milliseconds only.
pub const MAX_PER_SESSION: usize = 32;
/// How often a connection reading its head looks whether a newer one took its slot.
const HEAD_POLL: Duration = Duration::from_millis(100);
/// Stack of each connection thread (see `accept_loop`).
const CONNECTION_STACK: usize = 8 * 1024 * 1024;
/// How often a held request checks whether the agent still waits.
const WAIT_TICK: Duration = Duration::from_millis(500);
/// After the answer: how much the client may still send before the close.
const LINGER_BYTES: u64 = 64 * 1024;
/// File name start of the hook settings written for cmd.exe launchers.
const SETTINGS_PREFIX: &str = "claude-hooks-";
/// Folder (in the settings folder) with one folder per gated process start that needs files,
/// named `<port>-<session>-<n>`.
const SESSIONS_DIR: &str = "sessions";

/// Time limits of one connection; each is a total, not per read.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// Request line and headers.
    head: Duration,
    /// How long a connection may wait silently for its head before a newer one may close it.
    grace: Duration,
    /// The body of a known session.
    body: Duration,
    /// Reading what the client still sends after the answer.
    linger: Duration,
    /// Each write of the answer.
    write: Duration,
    /// How long a decision waits for the person.
    decision: Duration,
}

const LIMITS: Limits = Limits {
    head: Duration::from_secs(5),
    grace: HEAD_GRACE,
    body: Duration::from_secs(30),
    linger: Duration::from_secs(2),
    write: Duration::from_secs(5),
    decision: DECISION_TIMEOUT,
};

type Waiter = SyncSender<Outcome>;
type Worktrees = Box<dyn Fn() -> Arc<Vec<WorktreeClaims>> + Send + Sync>;
type Titles = Box<dyn Fn() -> HashMap<SessionId, String> + Send + Sync>;
type Emit = Box<dyn Fn(&GateSnapshot) + Send + Sync>;
/// The files teammates' sessions are changing in the repository with a root commit (first) for
/// the clone at a git common folder (second): only teams that clone is linked to count.
type Teammates = Box<dyn Fn(&str, &str) -> Vec<TeammateFile> + Send + Sync>;
/// The team's reservations for the same clone (`TeamService::team_tasks`).
type TeamTasks = Box<dyn Fn(&str, &str) -> Vec<TeamTask> + Send + Sync>;
/// Hands a message an agent sent with `hangar-send` to the partner service: the sending session
/// and the request; the answer is the HTTP status and the JSON body for the client.
type Messages = Arc<dyn Fn(SessionId, SendRequest) -> (Status, String) + Send + Sync>;
/// Tells the window that the dialog signals changed.
type Signal = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    gate: Mutex<Gate<Waiter>>,
    /// Port of the endpoint once it runs.
    port: Mutex<Option<u16>>,
    connections: Arc<AtomicUsize>,
    limits: Limits,
    /// Where the hook settings for cmd.exe launchers go (no secret in them).
    settings_dir: PathBuf,
    worktrees: Worktrees,
    titles: Titles,
    emit: Emit,
    /// The teammates' live claims, once a team service exists (step F part 2b).
    teammates: Mutex<Option<Teammates>>,
    /// The team's reservations, once a team service exists.
    team_tasks: Mutex<Option<TeamTasks>>,
    /// The Hangar executable that hook commands start as `gate-hook` helper.
    helper: Mutex<Option<PathBuf>>,
    /// Hook file folders per session, removed when its process ends.
    session_dirs: Mutex<HashMap<SessionId, Vec<PathBuf>>>,
    /// Makes each start's folder name unique, so removing a finished start's folder never
    /// touches the next start's.
    starts: AtomicUsize,
    /// The person's gate rules (`gate_rules.rs`) and the file they live in.
    rules: Mutex<Rules>,
    /// The command guard's rules.
    guard: Mutex<Guard>,
    /// The command guard's setting (on by default). Not under a lock, so `ask_command` can read
    /// it while it holds the gate's.
    guard_enabled: AtomicBool,
    /// Connections whose head is still being read, oldest first.
    heads: Arc<Mutex<Heads>>,
    /// Connections each known session holds.
    per_session: Arc<Mutex<HashMap<SessionId, usize>>>,
    /// Where the send route hands messages, once the app set it ([`GateService::set_messages`]).
    messages: Mutex<Option<Messages>>,
    /// Sessions whose agent shows a permission or input dialog (ADR 0016).
    permission: Mutex<PermissionWaits>,
    /// Announces a change of [`Inner::permission`], once the app set it.
    permission_signal: Mutex<Option<Signal>>,
}

/// The command guard: on by default; its rules come from the file, read again when it changed.
struct Guard {
    /// `command-guard.toml` in Hangar's config folder; without one (tests) the default rules.
    file: Option<PathBuf>,
    /// Modification time and length of the file the rules were read from.
    stamp: Option<(Option<SystemTime>, u64)>,
    /// Whether Hangar already tried to write the default file in this run.
    wrote_default: bool,
    rules: Arc<RuleSet>,
}

impl Default for Guard {
    fn default() -> Self {
        Self {
            file: None,
            stamp: None,
            wrote_default: false,
            rules: Arc::new(command_guard::default_rules()),
        }
    }
}

/// What Settings shows of the command guard (command `command_guard_info`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardInfo {
    pub enabled: bool,
    /// Where the rules live, for the person to edit; empty without a file.
    pub file: String,
    pub rules: Vec<RuleSummary>,
    pub problems: Vec<RuleProblem>,
    /// The file could not be read, so the default rules apply.
    pub defaults: bool,
    /// How many rules Hangar ships, so Settings can say when the file holds fewer.
    pub default_count: usize,
}

/// The rule book and where it is saved; without a file (tests, before `load_rules`) rules live
/// in memory only.
#[derive(Default)]
struct Rules {
    file: Option<PathBuf>,
    book: RuleBook,
}

impl Rules {
    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        if let Err(error) = crate::persist::write_atomic(file, &self.book.to_json()) {
            eprintln!(
                "hangar: gate rules not saved to {}: {error}",
                file.display()
            );
        }
    }
}

/// Tauri state of the agent gate; clones share one gate.
#[derive(Clone)]
pub struct GateService {
    inner: Arc<Inner>,
}

impl GateService {
    /// `settings_dir`: Hangar's own data folder for the launcher settings files; `worktrees`: the
    /// claims of step A; `titles`: session titles for the reasons the agent reads; `emit`: sends
    /// a snapshot to the window. Nothing starts before the first launch.
    pub fn new(
        settings_dir: PathBuf,
        worktrees: impl Fn() -> Arc<Vec<WorktreeClaims>> + Send + Sync + 'static,
        titles: impl Fn() -> HashMap<SessionId, String> + Send + Sync + 'static,
        emit: impl Fn(&GateSnapshot) + Send + Sync + 'static,
    ) -> Self {
        Self::with_limits(settings_dir, worktrees, titles, emit, LIMITS)
    }

    fn with_limits(
        settings_dir: PathBuf,
        worktrees: impl Fn() -> Arc<Vec<WorktreeClaims>> + Send + Sync + 'static,
        titles: impl Fn() -> HashMap<SessionId, String> + Send + Sync + 'static,
        emit: impl Fn(&GateSnapshot) + Send + Sync + 'static,
        limits: Limits,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                gate: Mutex::new(Gate::default()),
                port: Mutex::new(None),
                connections: Arc::new(AtomicUsize::new(0)),
                limits,
                settings_dir,
                worktrees: Box::new(worktrees),
                titles: Box::new(titles),
                emit: Box::new(emit),
                teammates: Mutex::new(None),
                team_tasks: Mutex::new(None),
                helper: Mutex::new(std::env::current_exe().ok()),
                session_dirs: Mutex::new(HashMap::new()),
                starts: AtomicUsize::new(0),
                rules: Mutex::new(Rules::default()),
                guard: Mutex::new(Guard::default()),
                guard_enabled: AtomicBool::new(true),
                heads: Arc::new(Mutex::new(Heads::default())),
                per_session: Arc::new(Mutex::new(HashMap::new())),
                messages: Mutex::new(None),
                permission: Mutex::new(PermissionWaits::default()),
                permission_signal: Mutex::new(None),
            }),
        }
    }

    /// The program hook commands start as `hangar gate-hook`; by default this process's own
    /// executable.
    pub fn set_helper(&self, exe: PathBuf) {
        *lock(&self.inner.helper) = Some(exe);
    }

    /// Where the teammates' live claims come from: an edit of a file a teammate's session is
    /// changing pauses like one of the user's own sessions, and the agent's context names it.
    pub fn set_teammates(
        &self,
        teammates: impl Fn(&str, &str) -> Vec<TeammateFile> + Send + Sync + 'static,
    ) {
        *lock(&self.inner.teammates) = Some(Box::new(teammates));
    }

    /// Where the team's reservations come from: an edit inside the area of a teammate's
    /// reservation pauses like a teammate's file, and the agent's context lists them.
    pub fn set_team_tasks(
        &self,
        tasks: impl Fn(&str, &str) -> Vec<TeamTask> + Send + Sync + 'static,
    ) {
        *lock(&self.inner.team_tasks) = Some(Box::new(tasks));
    }

    /// Where the send route ([`ROUTE_SEND`]) hands the messages agents send with `hangar-send`.
    pub fn set_messages(
        &self,
        messages: impl Fn(SessionId, SendRequest) -> (Status, String) + Send + Sync + 'static,
    ) {
        *lock(&self.inner.messages) = Some(Arc::new(messages));
    }

    /// A send token for the session's next process and the endpoint's port, which starts now if
    /// it does not run yet: sending works in private sessions too, without the gate (ADR 0012,
    /// ADR 0014).
    /// The token opens the send route only ([`TokenScope::Send`]).
    pub fn send_token(&self, session: SessionId) -> io::Result<(u16, Zeroizing<String>)> {
        let port = self.ensure_server()?;
        let token = lock(&self.inner.gate).register(session, TokenScope::Send);
        Ok((port, Zeroizing::new(token)))
    }

    /// How the window hears that the sessions waiting for a dialog changed ([`PERMISSION_EVENT`]).
    pub fn set_permission_signal(&self, signal: impl Fn() + Send + Sync + 'static) {
        *lock(&self.inner.permission_signal) = Some(Arc::new(signal));
    }

    /// The sessions whose agent shows a permission or input dialog right now (ADR 0016).
    pub fn permission_waiting(&self) -> Vec<SessionId> {
        lock(&self.inner.permission).sessions(Instant::now())
    }

    /// The person pressed a key that answers a dialog (Enter, Esc, a digit) in `session`: its
    /// signal goes. Claude Code sends no hook after a "No" in the dialog (measured), so without
    /// this the signal would hold until the next prompt or the time limit. A key that did not
    /// answer leaves the screen check as the fallback; the next dialog sets the signal again.
    pub fn permission_answered(&self, session: SessionId) {
        if lock(&self.inner.permission).clear(session) {
            self.inner.permission_changed();
        }
    }

    /// The session's send token stops working (its process ended).
    pub fn revoke_send(&self, session: SessionId) {
        lock(&self.inner.gate).revoke(session, TokenScope::Send);
    }

    /// The window's setting. Turning it off lets every waiting edit through.
    pub fn configure(&self, enabled: bool) {
        let changed = {
            let mut gate = lock(&self.inner.gate);
            let changed = gate.enabled() != enabled;
            deliver(gate.set_enabled(enabled));
            changed
        };
        if changed {
            self.inner.publish();
        }
    }

    pub fn snapshot(&self) -> GateSnapshot {
        lock(&self.inner.gate).snapshot()
    }

    /// The person's answer; `false` when the decision no longer waits. An answer that sets a
    /// gate rule ([`Answer::rule`]) saves it for the decision's project and answers the
    /// other waiting decisions it covers at once.
    pub fn answer(&self, id: u64, answer: Answer) -> bool {
        // Checked before any rule is saved: an answer the decision does not take sets nothing.
        if !lock(&self.inner.gate).accepts(id, answer) {
            return false;
        }
        let rules = answer
            .rule()
            .map(|(effect, scope)| self.add_rules_for(id, effect, scope))
            .unwrap_or_default();
        let resolved = lock(&self.inner.gate).answer(id, answer);
        let found = resolved.is_some();
        deliver(resolved.into_iter().flatten());
        let mut settled = false;
        for rule in rules.iter().filter(|rule| rule.scope != RuleScope::Command) {
            settled |= self.inner.settle_waiting(rule);
        }
        if rules.iter().any(|rule| rule.scope == RuleScope::Command) {
            settled |= self.inner.settle_commands();
        }
        if found || settled {
            self.inner.publish();
        }
        found
    }

    /// Reads the rule book from `file` (Hangar's local data folder); later changes are saved
    /// there. A missing, unreadable or broken file is an empty book, said on stderr.
    pub fn load_rules(&self, file: PathBuf) {
        let book = match crate::persist::read_limited(&file, gate_rules::MAX_FILE_BYTES) {
            Ok(Some(bytes)) => RuleBook::parse(&bytes),
            Ok(None) => RuleBook::default(),
            Err(reason) => {
                eprintln!("hangar: ignoring {}: {reason}", file.display());
                RuleBook::default()
            }
        };
        *lock(&self.inner.rules) = Rules {
            file: Some(file),
            book,
        };
    }

    /// Where the command guard's rules live; read on first use, written with the commented
    /// defaults when missing.
    pub fn load_command_guard(&self, file: PathBuf) {
        let mut guard = lock(&self.inner.guard);
        guard.file = Some(file);
        guard.stamp = None;
    }

    /// The window's setting. Turning it off lets every held shell command go on.
    pub fn configure_command_guard(&self, enabled: bool) {
        let changed = self.inner.guard_enabled.swap(enabled, Ordering::SeqCst) != enabled;
        if !changed || enabled {
            return;
        }
        let released = lock(&self.inner.gate).release_commands();
        if !released.is_empty() {
            deliver(released);
            self.inner.publish();
        }
    }

    /// The guard's setting, file, rules and problems, the file read again if it changed.
    pub fn command_guard_info(&self) -> GuardInfo {
        let set = self.inner.guard_rules();
        let guard = lock(&self.inner.guard);
        GuardInfo {
            enabled: self.inner.guard_enabled.load(Ordering::SeqCst),
            file: guard
                .file
                .as_ref()
                .map(|file| file.to_string_lossy().into_owned())
                .unwrap_or_default(),
            rules: set.summaries(),
            problems: set.problems.clone(),
            defaults: set.defaults,
            default_count: command_guard::default_rules().rules.len(),
        }
    }

    /// Every gate rule, in the order they were set.
    pub fn rules(&self) -> Vec<GateRule> {
        lock(&self.inner.rules).book.rules().to_vec()
    }

    /// `false` when no rule has that id.
    pub fn delete_rule(&self, id: u64) -> bool {
        let mut rules = lock(&self.inner.rules);
        let removed = rules.book.remove(id);
        if removed {
            rules.save();
        }
        removed
    }

    /// The rules an answer sets for decision `id`: its file, or the file's folder, or each guard
    /// rule a held command matched (except those [`command_guard::rememberable`] refuses), in the
    /// project of the waiting session. Empty when the decision no longer waits, the session is in
    /// no known repository, or the path cannot be a rule (a folder rule for a file at the top).
    fn add_rules_for(&self, id: u64, effect: RuleEffect, scope: RuleScope) -> Vec<GateRule> {
        let Some(waiting) = lock(&self.inner.gate).waiting(id) else {
            return Vec::new();
        };
        let worktrees = (self.inner.worktrees)();
        let Some(project) = worktrees
            .iter()
            .find(|w| w.sessions.contains(&waiting.session))
            .map(|w| w.repo.clone())
        else {
            return Vec::new();
        };
        let paths: Vec<String> = match (scope, &waiting.command_rules) {
            (RuleScope::Command, Some(ids)) => ids
                .iter()
                .filter(|id| command_guard::rememberable(id))
                .cloned()
                .collect(),
            (RuleScope::File, None) => vec![waiting.path.clone()],
            (RuleScope::Folder, None) => gate_rules::folder_of(&waiting.path).into_iter().collect(),
            _ => Vec::new(),
        };
        let mut rules = lock(&self.inner.rules);
        let added: Vec<GateRule> = paths
            .iter()
            .filter_map(|path| rules.book.add(&project, path, scope, effect, now_millis()))
            .collect();
        if !added.is_empty() {
            rules.save();
        }
        added
    }

    /// Hangar quits: waiting edits are denied, no token works any more.
    pub fn stop(&self) {
        deliver(lock(&self.inner.gate).shutdown());
    }

    /// Port of the endpoint, once started.
    pub fn port(&self) -> Option<u16> {
        *lock(&self.inner.port)
    }

    fn ensure_server(&self) -> io::Result<u16> {
        let mut port = lock(&self.inner.port);
        if let Some(port) = *port {
            return Ok(port);
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let bound = listener.local_addr()?.port();
        // Settings files of earlier runs name ports that are gone.
        remove_old_settings(&self.inner.settings_dir);
        remove_old_session_dirs(&self.inner.settings_dir.join(SESSIONS_DIR));
        let inner = Arc::clone(&self.inner);
        thread::Builder::new()
            .name("gate-http".to_owned())
            .spawn(move || accept_loop(&inner, &listener))?;
        // Old dialog signals go on their own; the thread ends with the service.
        let weak = Arc::downgrade(&self.inner);
        let _ = thread::Builder::new()
            .name("gate-notice-expiry".to_owned())
            .spawn(move || loop {
                thread::sleep(SIGNAL_EXPIRY_TICK);
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let expired = lock(&inner.permission).expire(Instant::now());
                if expired {
                    inner.permission_changed();
                }
            });
        *port = Some(bound);
        Ok(bound)
    }

    /// Claude Code's `--settings` with the hooks inline, or as a file for a cmd.exe launcher.
    fn claude_args(
        &self,
        port: u16,
        context: bool,
        program: &Path,
        version: Option<&str>,
    ) -> Result<Vec<String>, GateStatus> {
        let block = gate::claude_blocks_on_failure(version);
        let notify = gate::claude_notifies(version);
        let settings = gate::claude_settings(port, context, block, notify);
        // One file per content, so sessions of Claude Code versions on both sides of
        // `CLAUDE_ON_FAILURE_SINCE` or `CLAUDE_NOTIFY_SINCE` never rewrite each other's file.
        let suffix = format!(
            "{}{}{}",
            if context { "-context" } else { "" },
            if block { "-block" } else { "" },
            if notify { "-notify" } else { "" }
        );
        let argument = if crate::batch::is_batch(program) {
            self.settings_file(port, &suffix, &settings)
                .ok_or(GateStatus::Launcher)?
        } else {
            settings
        };
        Ok(vec!["--settings".to_owned(), argument])
    }

    /// The hook settings as a file, for a cmd.exe launcher: Hangar refuses `"` in such a
    /// launcher's arguments (batch.rs), so the JSON cannot go inline. `None` when the file's path
    /// holds a character cmd.exe would interpret or the file cannot be written.
    fn settings_file(&self, port: u16, suffix: &str, settings: &str) -> Option<String> {
        let path = self
            .inner
            .settings_dir
            .join(format!("{SETTINGS_PREFIX}{port}{suffix}.json"));
        let text = path.to_str()?.to_owned();
        if !crate::batch::argument_ok(&text) {
            return None;
        }
        if std::fs::read_to_string(&path).ok().as_deref() == Some(settings) {
            return Some(text);
        }
        let written = std::fs::create_dir_all(&self.inner.settings_dir).and_then(|()| {
            // Written whole under another name first, so a starting agent never reads half.
            let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&temporary, settings)?;
            std::fs::rename(&temporary, &path)
        });
        match written {
            Ok(()) => Some(text),
            Err(error) => {
                eprintln!("hangar: agent gate settings not written: {error}");
                None
            }
        }
    }

    /// The program hook commands run as `hangar gate-hook`.
    fn helper(&self) -> Result<PathBuf, GateStatus> {
        lock(&self.inner.helper)
            .clone()
            .ok_or(GateStatus::InstallPath)
    }

    /// A new, empty folder for one start's hook files, remembered for [`LaunchGate::ended`].
    fn session_dir(&self, port: u16, session: SessionId) -> Result<PathBuf, GateStatus> {
        let failed = |error: io::Error| {
            eprintln!("hangar: agent gate folder not created: {error}");
            GateStatus::HookFiles
        };
        let parent = self.inner.settings_dir.join(SESSIONS_DIR);
        if !parent.is_dir() {
            std::fs::create_dir_all(&self.inner.settings_dir).map_err(failed)?;
            match create_private_dir(&parent) {
                Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
                    return Err(failed(error))
                }
                _ => {}
            }
        }
        // Created fresh, never reused: a folder that exists already (a crashed run's leftover
        // with the same port, or one planted there) is skipped, not trusted.
        for _ in 0..16 {
            let start = self.inner.starts.fetch_add(1, Ordering::SeqCst);
            let dir = parent.join(format!("{port}-{session}-{start}"));
            match create_private_dir(&dir) {
                Ok(()) => {
                    lock(&self.inner.session_dirs)
                        .entry(session)
                        .or_default()
                        .push(dir.clone());
                    return Ok(dir);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(failed(error)),
            }
        }
        Err(failed(io::ErrorKind::AlreadyExists.into()))
    }

    /// What one start of a program with `format` gets, without the token: arguments before and
    /// after the user's, environment, and whether the token goes to the helper
    /// ([`HELPER_ENV`]) instead of an HTTP hook header ([`TOKEN_ENV`]).
    #[allow(clippy::too_many_arguments)] // All values belong to exactly this one start.
    fn install(
        &self,
        format: HookFormat,
        session: SessionId,
        port: u16,
        context: bool,
        program: &Path,
        user_args: &[String],
        version: Option<&str>,
    ) -> Result<Install, GateStatus> {
        let batch = crate::batch::is_batch(program);
        let path_text = |path: &Path| path.to_string_lossy().into_owned();
        let install = match format {
            HookFormat::ClaudeSettings => Install {
                args: self.claude_args(port, context, program, version)?,
                ..Install::default()
            },
            HookFormat::QwenSystemSettings => {
                let dir = self.session_dir(port, session)?;
                let file = write_file(
                    &dir,
                    recipes::QWEN_FILE,
                    &recipes::qwen_settings(port, context),
                )?;
                Install {
                    env: vec![(recipes::QWEN_SETTINGS_ENV.to_owned(), path_text(&file))],
                    ..Install::default()
                }
            }
            HookFormat::GeminiSystemSettings => {
                let command = recipes::helper_command(
                    &self.helper()?,
                    AGENT_GEMINI,
                    EVENT_BEFORE_TOOL,
                    Shell::for_agent(true),
                    false,
                )
                .map_err(|_| GateStatus::InstallPath)?;
                let dir = self.session_dir(port, session)?;
                let file = write_file(
                    &dir,
                    recipes::GEMINI_FILE,
                    &recipes::gemini_settings(&command),
                )?;
                Install {
                    env: vec![
                        (recipes::GEMINI_SETTINGS_ENV.to_owned(), path_text(&file)),
                        (recipes::GEMINI_TRUST_ENV.to_owned(), "true".to_owned()),
                    ],
                    helper: true,
                    ..Install::default()
                }
            }
            HookFormat::CopilotPlugin => {
                let exe = self.helper()?;
                let dir = self.session_dir(port, session)?;
                let plugin = dir.join(recipes::COPILOT_DIR);
                let plugin_text = path_text(&plugin);
                // A cmd.exe launcher (npm's copilot.cmd) gets the folder as an argument.
                if batch && !crate::batch::argument_ok(&plugin_text) {
                    return Err(GateStatus::Launcher);
                }
                create_private_dir(&plugin).map_err(|_| GateStatus::HookFiles)?;
                write_file(
                    &plugin,
                    recipes::COPILOT_MANIFEST,
                    &recipes::copilot_manifest(),
                )?;
                write_file(
                    &plugin,
                    recipes::COPILOT_HOOKS,
                    &recipes::copilot_hooks(&exe),
                )?;
                Install {
                    prepend: vec![recipes::COPILOT_PLUGIN_FLAG.to_owned(), plugin_text],
                    helper: true,
                    ..Install::default()
                }
            }
            HookFormat::CodexConfigFlags => {
                // Through a cmd.exe launcher (npm's codex.cmd) the value must carry no `"`, so
                // the path has to stand bare.
                let command = recipes::helper_command(
                    &self.helper()?,
                    AGENT_CODEX,
                    EVENT_PRE_TOOL_USE,
                    Shell::for_agent(false),
                    batch,
                )
                .map_err(|_| GateStatus::InstallPath)?;
                let prepend = recipes::codex_args(&command, user_args);
                if batch && !prepend.iter().all(|arg| crate::batch::argument_ok(arg)) {
                    return Err(GateStatus::InstallPath);
                }
                Install {
                    prepend,
                    helper: true,
                    ..Install::default()
                }
            }
            // The extension asks the endpoint itself; it reads port and token from HELPER_ENV.
            HookFormat::PiExtension | HookFormat::OmpExtension => {
                let dir = self.session_dir(port, session)?;
                let file_text = path_text(&dir.join(recipes::PI_FILE));
                // A cmd.exe launcher (npm's pi.cmd) gets the file as an argument.
                if batch && !crate::batch::argument_ok(&file_text) {
                    return Err(GateStatus::Launcher);
                }
                write_file(&dir, recipes::PI_FILE, &recipes::pi_extension())?;
                Install {
                    prepend: vec![recipes::PI_EXTENSION_FLAG.to_owned(), file_text],
                    helper: true,
                    ..Install::default()
                }
            }
            HookFormat::OpencodePlugin => {
                // Named inline rather than through a configuration folder: OpenCode runs an npm
                // install in every configuration folder it starts with (measured).
                let dir = self.session_dir(port, session)?;
                let plugin = write_file(&dir, recipes::OPENCODE_FILE, &recipes::opencode_plugin())?;
                let config = recipes::opencode_config(&plugin).ok_or(GateStatus::HookFiles)?;
                Install {
                    env: vec![(recipes::OPENCODE_CONFIG_CONTENT_ENV.to_owned(), config)],
                    helper: true,
                    ..Install::default()
                }
            }
            HookFormat::DroidSettings => {
                // Which shell Droid runs a hook command in is not documented; a double quoted
                // path reads the same in cmd.exe and a POSIX shell.
                let command = recipes::helper_command(
                    &self.helper()?,
                    AGENT_DROID,
                    EVENT_PRE_TOOL_USE,
                    Shell::for_agent(false),
                    false,
                )
                .map_err(|_| GateStatus::InstallPath)?;
                let dir = self.session_dir(port, session)?;
                let file_text = path_text(&dir.join(recipes::DROID_FILE));
                if batch && !crate::batch::argument_ok(&file_text) {
                    return Err(GateStatus::Launcher);
                }
                write_file(
                    &dir,
                    recipes::DROID_FILE,
                    &recipes::droid_settings(&command),
                )?;
                Install {
                    prepend: vec![recipes::DROID_SETTINGS_FLAG.to_owned(), file_text],
                    helper: true,
                    ..Install::default()
                }
            }
        };
        Ok(install)
    }
}

/// [`GateService::install`]'s result.
#[derive(Default)]
struct Install {
    prepend: Vec<String>,
    args: Vec<String>,
    env: Vec<(String, String)>,
    helper: bool,
}

/// Writes one hook file of a fresh session folder; no agent reads it before the start.
fn write_file(dir: &Path, name: &str, text: &str) -> Result<PathBuf, GateStatus> {
    let path = dir.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|error| {
            eprintln!("hangar: agent gate file not written: {error}");
            GateStatus::HookFiles
        })?;
    Ok(path)
}

/// Creates exactly `dir` (its parent must exist), failing when it exists already; only the user
/// may enter it on Unix.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::DirBuilder::new().create(dir)
    }
}

/// Removes session folders of earlier runs whose port nobody listens on any more; another
/// Hangar running at the same time keeps its own.
fn remove_old_session_dirs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if settings_port(&name.to_string_lossy()).is_some_and(port_in_use) {
            continue;
        }
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// Deletes the launcher settings files of earlier runs; a failure only leaves a stale file.
/// A file whose port is still listened on stays: another Hangar running at the same time (an
/// installed one next to a development build) shares the folder, and its agents may still start
/// with that file.
fn remove_old_settings(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix(SETTINGS_PREFIX) else {
            continue;
        };
        if settings_port(rest).is_some_and(port_in_use) {
            continue;
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

/// The port a settings file name names after [`SETTINGS_PREFIX`] (`8123.json`,
/// `8123-context.json`, `8123-context.json.<pid>.tmp`).
fn settings_port(rest: &str) -> Option<u16> {
    let digits = rest.split(|c: char| !c.is_ascii_digit()).next()?;
    digits.parse().ok().filter(|port| *port != 0)
}

/// Something listens on the loopback port: binding it fails.
fn port_in_use(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_err()
}

impl LaunchGate for GateService {
    fn applies(&self, agent: &Agent) -> bool {
        agent.capabilities.gate() && lock(&self.inner.gate).enabled()
    }

    fn prepare(
        &self,
        session: SessionId,
        agent: &Agent,
        program: &Path,
        args: &[String],
        version: Option<&str>,
    ) -> GatePlan {
        let Some(format) = agent.capabilities.gate_format() else {
            return GatePlan::Skip;
        };
        if !self.applies(agent) {
            return GatePlan::Skip;
        }
        let port = match self.ensure_server() {
            Ok(port) => port,
            Err(error) => {
                eprintln!("hangar: agent gate not started: {error}");
                return GatePlan::Unavailable(GateStatus::NoEndpoint);
            }
        };
        let context = agent.capabilities.context_inject;
        // Each program takes its hooks in its own format; the endpoint and token are the same.
        let install = match self.install(format, session, port, context, program, args, version) {
            Ok(install) => install,
            Err(status) => {
                self.remove_dirs(session);
                return GatePlan::Unavailable(status);
            }
        };
        let token = Zeroizing::new(lock(&self.inner.gate).register(session, TokenScope::Hooks));
        let token_var = if install.helper {
            (
                HELPER_ENV.to_owned(),
                Zeroizing::new(format!("{port}:{}", token.as_str())),
            )
        } else {
            (TOKEN_ENV.to_owned(), token)
        };
        // Claude Code's settings carry the notice hooks too (`claude_args`), with a token of
        // their own scope.
        let notice_var = (format == HookFormat::ClaudeSettings && gate::claude_notifies(version))
            .then(|| {
                let token = lock(&self.inner.gate).register(session, TokenScope::Notify);
                (NOTIFY_TOKEN_ENV.to_owned(), Zeroizing::new(token))
            });
        let env = install
            .env
            .into_iter()
            .map(|(name, value)| (name, Zeroizing::new(value)))
            .chain(std::iter::once(token_var))
            .chain(notice_var)
            .collect();
        GatePlan::Launch(GateLaunch {
            prepend: install.prepend,
            args: install.args,
            env,
        })
    }

    fn notify(
        &self,
        session: SessionId,
        agent: &Agent,
        program: &Path,
        version: Option<&str>,
    ) -> Option<GateLaunch> {
        if agent.capabilities.gate_format() != Some(HookFormat::ClaudeSettings)
            || !gate::claude_notifies(version)
        {
            return None;
        }
        let port = match self.ensure_server() {
            Ok(port) => port,
            Err(error) => {
                eprintln!("hangar: notice hooks not started: {error}");
                return None;
            }
        };
        let settings = gate::claude_notify_settings(port);
        let argument = if crate::batch::is_batch(program) {
            self.settings_file(port, "-notify-only", &settings)?
        } else {
            settings
        };
        let token = lock(&self.inner.gate).register(session, TokenScope::Notify);
        Some(GateLaunch {
            prepend: Vec::new(),
            args: vec!["--settings".to_owned(), argument],
            env: vec![(NOTIFY_TOKEN_ENV.to_owned(), Zeroizing::new(token))],
        })
    }

    fn ended(&self, session: SessionId) {
        let ended = lock(&self.inner.gate).forget(session);
        self.remove_dirs(session);
        if !ended.is_empty() {
            deliver(ended);
            self.inner.publish();
        }
        // Under the session lock: the window is told on a signal only, nothing waits here.
        if lock(&self.inner.permission).clear(session) {
            self.inner.permission_changed();
        }
    }
}

impl GateService {
    /// Removes the session's hook folders on a thread of its own: `ended` runs under the
    /// session lock, which must not wait for the disk.
    fn remove_dirs(&self, session: SessionId) {
        let Some(dirs) = lock(&self.inner.session_dirs).remove(&session) else {
            return;
        };
        let removal = thread::Builder::new()
            .name("gate-cleanup".to_owned())
            .spawn(move || {
                for dir in dirs {
                    let _ = std::fs::remove_dir_all(dir);
                }
            });
        if let Err(error) = removal {
            eprintln!("hangar: agent gate folders not removed: {error}");
        }
    }
}

/// The log line for an edit `rule` answered.
fn log_entry(
    rule: &GateRule,
    session: SessionId,
    titles: &HashMap<SessionId, String>,
    path: &str,
) -> RuleAnswer {
    RuleAnswer {
        id: 0,
        at: now_millis(),
        session_id: session,
        session: titles
            .get(&session)
            .cloned()
            .unwrap_or_else(|| format!("#{session}")),
        project_name: rule.project_name.clone(),
        path: path.to_owned(),
        rule_id: rule.id,
        rule_path: rule.path.clone(),
        scope: rule.scope,
        effect: rule.effect,
    }
}

/// Hands each outcome to its waiting request; a request that gave up meanwhile is gone.
fn deliver(outcomes: impl IntoIterator<Item = (Waiter, Outcome)>) {
    for (waiter, outcome) in outcomes {
        let _ = waiter.try_send(outcome);
    }
}

/// The default rules with a problem that names why the file could not be read.
fn file_problem(reason: &str) -> RuleSet {
    let mut set = command_guard::default_rules();
    set.defaults = true;
    set.problems.insert(
        0,
        RuleProblem {
            rule: None,
            id: None,
            line: None,
            message: reason.to_owned(),
        },
    );
    set
}

/// The user's home folder, where `~` and `$HOME` in a command lead.
fn home_folder() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Keys of `path` as named and with its existing part resolved (a mapped drive or a symlink in
/// the agent's spelling), for [`gate::assess`]. Relative paths count against the hook's `cwd`;
/// without one they cannot be placed and are left alone. `own_root` is the session's worktree
/// ([`WorktreeClaims::root_key`] as a path); see [`may_resolve`].
pub fn path_candidates(path: &str, cwd: Option<&str>, own_root: &Path) -> Vec<String> {
    candidates_with(path, cwd, own_root, resolve_existing)
}

fn candidates_with(
    path: &str,
    cwd: Option<&str>,
    own_root: &Path,
    resolve: impl Fn(&Path) -> Option<PathBuf>,
) -> Vec<String> {
    let named = Path::new(path);
    let absolute = if named.is_absolute() {
        named.to_path_buf()
    } else {
        match cwd.map(Path::new).filter(|cwd| cwd.is_absolute()) {
            Some(cwd) => cwd.join(named),
            None => return Vec::new(),
        }
    };
    let mut keys = vec![crate::git::path_key(&absolute)];
    if may_resolve(&absolute, own_root) {
        if let Some(resolved) = resolve(&absolute) {
            let key = crate::git::path_key(&resolved);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// Resolving touches the file system, and the path comes from the agent, which a prompt can talk
/// into any path. On Windows, touching a UNC path connects to that host and sends the user's
/// NTLM credentials, and a device path opens a pipe or a port, all before Claude Code's own
/// permission prompt. So only paths on a local drive, or on the share the session's own worktree
/// lives on ([`crate::git::gitdir_allowed`]), and without a DOS device name are resolved; any
/// other path is compared as named.
fn may_resolve(path: &Path, own_root: &Path) -> bool {
    crate::git::gitdir_allowed(own_root, path) && !names_device(path)
}

/// A part of the path is a DOS device name (`CON`, `NUL`, `COM1`, `LPT1.txt` …), which Windows
/// may open as the device instead of a file.
fn names_device(path: &Path) -> bool {
    cfg!(windows)
        && path.components().any(|part| match part {
            Component::Normal(name) => is_device_name(name),
            _ => false,
        })
}

fn is_device_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    let stem = name.split('.').next().unwrap_or_default();
    let stem = stem.trim_end_matches([' ', ':']).to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str()) {
        return true;
    }
    ["COM", "LPT"].iter().any(|port| {
        stem.strip_prefix(port).is_some_and(|number| {
            matches!(
                number,
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

/// The path with its longest existing ancestor resolved; the file itself may not exist yet.
fn resolve_existing(path: &Path) -> Option<PathBuf> {
    let mut rest = Vec::new();
    let mut current = path;
    loop {
        if let Ok(resolved) = std::fs::canonicalize(current) {
            return Some(rest.iter().rev().fold(resolved, |acc, part| acc.join(part)));
        }
        rest.push(current.file_name()?.to_owned());
        current = current.parent()?;
    }
}

/// One counted connection slot, given back when dropped.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(counter: &Arc<AtomicUsize>, limit: usize) -> Option<Slot> {
        if counter.fetch_add(1, Ordering::SeqCst) >= limit {
            counter.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot(Arc::clone(counter)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Connections whose head is still being read.
#[derive(Default)]
struct Heads {
    next: u64,
    /// The ones holding a slot, oldest first: id, a handle to close it, its cancel flag, when
    /// it was accepted.
    open: VecDeque<(u64, TcpStream, Arc<AtomicBool>, Instant)>,
    /// Threads still reading a head, the closed ones that have not noticed yet included.
    alive: usize,
}

/// A connection reading its head: one of [`MAX_UNAUTHENTICATED`] slots until the head is read
/// or a newer connection takes the slot.
struct HeadSlot {
    heads: Arc<Mutex<Heads>>,
    id: u64,
    cancel: Arc<AtomicBool>,
}

impl HeadSlot {
    /// A slot for `stream`. With [`MAX_UNAUTHENTICATED`] or more open, or every head thread
    /// busy, the oldest connection is closed for this one once it waited `grace` for its head: a hook sends its head at once, a client that holds a slot silently does not,
    /// so silent clients cannot keep the hooks out, while a burst of hooks arriving together
    /// is never cut. `None` (closed unanswered) only when [`MAX_HEAD_THREADS`] threads are busy
    /// and none of them has been silent that long. Each thread over that limit closed a silent
    /// one, which ends within [`HEAD_POLL`], so the threads stay bounded.
    fn take(heads: &Arc<Mutex<Heads>>, stream: &TcpStream, grace: Duration) -> Option<HeadSlot> {
        let handle = stream.try_clone().ok()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let now = Instant::now();
        let (id, evicted) = {
            let mut list = lock(heads);
            let crowded = list.open.len() >= MAX_UNAUTHENTICATED || list.alive >= MAX_HEAD_THREADS;
            let silent = list
                .open
                .front()
                .is_some_and(|(_, _, _, since)| now.saturating_duration_since(*since) >= grace);
            let evicted = if crowded && silent {
                list.open.pop_front()
            } else {
                None
            };
            if evicted.is_none() && list.alive >= MAX_HEAD_THREADS {
                return None;
            }
            let id = list.next;
            list.next = list.next.wrapping_add(1);
            list.open.push_back((id, handle, Arc::clone(&cancel), now));
            list.alive += 1;
            (id, evicted)
        };
        if let Some((_, oldest, flag, _)) = evicted {
            flag.store(true, Ordering::SeqCst);
            // Wakes its reader where the platform does; the flag ends it within HEAD_POLL.
            let _ = oldest.shutdown(Shutdown::Both);
        }
        Some(HeadSlot {
            heads: Arc::clone(heads),
            id,
            cancel,
        })
    }
}

impl HeadSlot {
    /// The head is read: this connection leaves the slots a newer one may take, under the lock,
    /// before its request is looked at, so it is never closed while it is answered. `false`
    /// when a newer one took its slot first (its stream is closed then).
    fn head_read(&self) -> bool {
        let mut list = lock(&self.heads);
        let before = list.open.len();
        list.open.retain(|(id, _, _, _)| *id != self.id);
        before != list.open.len() && !self.cancel.load(Ordering::SeqCst)
    }
}

impl Drop for HeadSlot {
    fn drop(&mut self) {
        let mut list = lock(&self.heads);
        list.open.retain(|(id, _, _, _)| *id != self.id);
        list.alive = list.alive.saturating_sub(1);
    }
}

/// One of [`MAX_PER_SESSION`] connections of a known session.
struct SessionSlot {
    sessions: Arc<Mutex<HashMap<SessionId, usize>>>,
    session: SessionId,
}

impl SessionSlot {
    fn take(sessions: &Arc<Mutex<HashMap<SessionId, usize>>>, session: SessionId) -> Option<Self> {
        let mut counts = lock(sessions);
        let count = counts.entry(session).or_insert(0);
        if *count >= MAX_PER_SESSION {
            return None;
        }
        *count += 1;
        Some(SessionSlot {
            sessions: Arc::clone(sessions),
            session,
        })
    }
}

impl Drop for SessionSlot {
    fn drop(&mut self) {
        let mut counts = lock(&self.sessions);
        if let Some(count) = counts.get_mut(&self.session) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&self.session);
            }
        }
    }
}

/// Reads with one deadline for all reads together: each read may take only the time left, so a
/// client sending a byte now and then cannot stretch a limit meant for the whole request.
struct Deadline<'a> {
    stream: &'a TcpStream,
    until: Instant,
    /// Set when a newer connection took this one's slot: reading stops within [`HEAD_POLL`].
    cancel: Option<Arc<AtomicBool>>,
}

impl<'a> Deadline<'a> {
    fn new(stream: &'a TcpStream, limit: Duration) -> Self {
        Self {
            stream,
            until: Instant::now() + limit,
            cancel: None,
        }
    }

    /// A fresh total from now, for the next part of the request.
    fn restart(&mut self, limit: Duration) {
        self.until = Instant::now() + limit;
        self.cancel = None;
    }
}

impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self
                .cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::SeqCst))
            {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            let left = self.until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let slice = match self.cancel {
                Some(_) => left.min(HEAD_POLL),
                None => left,
            };
            self.stream.set_read_timeout(Some(slice))?;
            let mut stream = self.stream;
            match stream.read(buf) {
                Err(error)
                    if self.cancel.is_some()
                        && matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) => {}
                other => return other,
            }
        }
    }
}

fn accept_loop(inner: &Arc<Inner>, listener: &TcpListener) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                // Out of handles and similar: do not spin.
                eprintln!("hangar: agent gate accept failed: {error}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let loopback = stream
            .peer_addr()
            .is_ok_and(|peer: SocketAddr| peer.ip().is_loopback());
        if !loopback {
            continue;
        }
        // No thread left for heads: closed unanswered, without a read or a write that could
        // block this thread. Claude Code treats that as a failed hook and goes on.
        let Some(slot) = HeadSlot::take(&inner.heads, &stream, inner.limits.grace) else {
            continue;
        };
        let worker = Arc::clone(inner);
        // A failed spawn drops the closure with its slot and stream. The stack is set rather
        // than left to the platform default: the command guard splits text the agent wrote, and
        // a stack overflow here would abort the whole app with every session in it. Its
        // recursion is bounded (`command_split::MAX_DEPTH`, `command_guard::MAX_BRACE_GROUPS`);
        // this is the margin on top.
        let _ = thread::Builder::new()
            .name("gate-conn".to_owned())
            .stack_size(CONNECTION_STACK)
            .spawn(move || serve(&worker, &stream, slot));
    }
}

fn serve(inner: &Inner, stream: &TcpStream, unauthenticated: HeadSlot) {
    let _ = stream.set_write_timeout(Some(inner.limits.write));
    let mut deadline = Deadline::new(stream, inner.limits.head);
    deadline.cancel = Some(Arc::clone(&unauthenticated.cancel));
    let mut reader = BufReader::new(deadline);
    // Held until the connection closes, once the session is known.
    let mut _authenticated = None;
    let (status, body) = match hook_http::read_head(&mut reader) {
        Err(error) => match error.status() {
            Some(status) if !unauthenticated.cancel.load(Ordering::SeqCst) => {
                (status, String::new())
            }
            _ => return,
        },
        Ok(_) if !unauthenticated.head_read() => return,
        Ok(head) => match inner.admit(&head) {
            Err(status) => (status, String::new()),
            Ok((session, scope)) => {
                drop(unauthenticated);
                let slots = Slot::take(&inner.connections, MAX_CONNECTIONS)
                    .zip(SessionSlot::take(&inner.per_session, session));
                match slots {
                    None => inner.overloaded(&head),
                    Some(slots) => {
                        _authenticated = Some(slots);
                        reader.get_mut().restart(inner.limits.body);
                        inner.route(session, scope, &head, &mut reader, stream)
                    }
                }
            }
        },
    };
    answer_and_close(
        stream,
        &hook_http::response(status, &body),
        inner.limits.linger,
    );
}

/// Writes the answer and closes gracefully: the write side first, then whatever the client still
/// sends is read and dropped until it closes too or `linger` is over. Windows otherwise may reset
/// the connection on close, and a client can lose the end of an answer it already received
/// (measured in the tests).
fn answer_and_close(stream: &TcpStream, response: &[u8], linger: Duration) {
    let mut writer = stream;
    if writer
        .write_all(response)
        .and_then(|()| writer.flush())
        .is_err()
    {
        return;
    }
    let _ = stream.shutdown(Shutdown::Write);
    let mut rest = Deadline::new(stream, linger).take(LINGER_BYTES);
    let _ = io::copy(&mut rest, &mut io::sink());
}

/// The agent stopped waiting: it closed the connection (interrupted by the person, hook
/// cancelled). Data or nothing to read means it still waits.
fn client_gone(stream: &TcpStream) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return false;
    }
    let mut probe = [0u8; 1];
    let gone = match stream.peek(&mut probe) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => !matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
        ),
    };
    let _ = stream.set_nonblocking(false);
    gone
}

impl Inner {
    /// Answers every waiting decision the new `rule` covers and logs each; `true` when any.
    fn settle_waiting(&self, rule: &GateRule) -> bool {
        let waiting = lock(&self.gate).all_waiting();
        if waiting.is_empty() {
            return false;
        }
        let worktrees = (self.worktrees)();
        let titles = (self.titles)();
        let covered: Vec<Waiting> = waiting
            .into_iter()
            .filter(|w| {
                worktrees
                    .iter()
                    .find(|tree| tree.sessions.contains(&w.session))
                    .is_some_and(|tree| {
                        w.command_rules.is_none()
                            && gate_rules::rule_matches(
                                rule,
                                &tree.repo,
                                &w.key,
                                gate_rules::CASE_INSENSITIVE,
                            )
                    })
            })
            .collect();
        let mut gate = lock(&self.gate);
        let mut any = false;
        for w in covered {
            let Some(outcomes) = gate.settle(w.id, rule.effect) else {
                continue;
            };
            deliver(outcomes);
            gate.record(log_entry(rule, w.session, &titles, &w.path));
            any = true;
        }
        any
    }

    /// Answers the held shell commands the remembered answers now cover: any deny for one of a
    /// command's rules blocks it, an allow for every one of them lets it go on. `true` when any.
    fn settle_commands(&self) -> bool {
        let waiting: Vec<Waiting> = lock(&self.gate)
            .all_waiting()
            .into_iter()
            .filter(|w| w.command_rules.is_some())
            .collect();
        if waiting.is_empty() {
            return false;
        }
        let worktrees = (self.worktrees)();
        let titles = (self.titles)();
        let mut settled: Vec<(Waiting, RuleEffect, Vec<GateRule>)> = Vec::new();
        {
            let rules = lock(&self.rules);
            for w in waiting {
                let Some(tree) = worktrees.iter().find(|t| t.sessions.contains(&w.session)) else {
                    continue;
                };
                let ids = w.command_rules.clone().unwrap_or_default();
                let found: Vec<Option<GateRule>> = ids
                    .iter()
                    .map(|id| {
                        rules
                            .book
                            .find_command(&tree.repo, id)
                            .filter(|_| command_guard::rememberable(id))
                            .cloned()
                    })
                    .collect();
                if let Some(deny) = found
                    .iter()
                    .flatten()
                    .find(|rule| rule.effect == RuleEffect::Deny)
                {
                    let deny = deny.clone();
                    settled.push((w, RuleEffect::Deny, vec![deny]));
                } else if found.iter().all(Option::is_some) {
                    let all = found.into_iter().flatten().collect();
                    settled.push((w, RuleEffect::Allow, all));
                }
            }
        }
        let mut gate = lock(&self.gate);
        let mut any = false;
        for (w, effect, used) in settled {
            let Some(outcomes) = gate.settle(w.id, effect) else {
                continue;
            };
            deliver(outcomes);
            for rule in &used {
                gate.record(log_entry(rule, w.session, &titles, &rule.path));
            }
            any = true;
        }
        any
    }

    fn publish(&self) {
        let snapshot = lock(&self.gate).snapshot();
        (self.emit)(&snapshot);
    }

    fn permission_changed(&self) {
        let signal = lock(&self.permission_signal).clone();
        if let Some(signal) = signal {
            signal();
        }
    }

    /// A notice hook of `session`: on the asks route a dialog appeared, on the clear route it is
    /// gone. Answers an empty 200 always, so no notice ever decides or blocks anything.
    fn notice(&self, session: SessionId, head: &Head, body: &mut impl Read) -> (Status, String) {
        let asks = head.path == ROUTE_NOTIFY_ASKS;
        let notice = gate::parse_notice(body, head.content_length);
        if gate::notice_counts(&notice, asks) {
            let changed = {
                // The process may have ended while the body was read: its `ended` cleared the
                // signal and dropped the token, and a late notice must not set it again. The
                // gate's lock is held while the signal changes, and `ended` takes the two locks
                // one after the other, so either the token is gone here or `ended` clears after.
                let gate = lock(&self.gate);
                if !gate.holds(session, TokenScope::Notify) {
                    return (Status::Ok, String::new());
                }
                let mut waits = lock(&self.permission);
                if asks {
                    waits.set(session, Instant::now())
                } else {
                    waits.clear(session)
                }
            };
            if changed {
                self.permission_changed();
            }
        }
        (Status::Ok, String::new())
    }

    /// The guard's rules, read again when the file changed (one look at its metadata per call,
    /// taken from the opened file so the check and the read see the same file). A missing file
    /// is written once per run with the commented defaults. The disk is never touched under the
    /// guard's lock.
    fn guard_rules(&self) -> Arc<RuleSet> {
        let (file, stamp, wrote_default, cached) = {
            let guard = lock(&self.guard);
            (
                guard.file.clone(),
                guard.stamp,
                guard.wrote_default,
                Arc::clone(&guard.rules),
            )
        };
        let Some(file) = file else {
            return cached;
        };
        let mut opened = std::fs::File::open(&file);
        if matches!(&opened, Err(error) if error.kind() == io::ErrorKind::NotFound) {
            if !wrote_default {
                lock(&self.guard).wrote_default = true;
                let written = file
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| {
                        crate::persist::write_atomic(&file, command_guard::DEFAULT_RULES.as_bytes())
                    });
                if let Err(error) = written {
                    eprintln!("hangar: {} not written: {error}", file.display());
                }
            }
            opened = std::fs::File::open(&file);
        }
        let mut handle = match opened {
            Ok(handle) => handle,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Arc::new(command_guard::default_rules());
            }
            Err(error) => return Arc::new(file_problem(&format!("not readable: {error}"))),
        };
        let Ok(meta) = handle.metadata() else {
            return Arc::new(file_problem("its size could not be read"));
        };
        let now = (meta.modified().ok(), meta.len());
        if stamp == Some(now) {
            return cached;
        }
        let limit = command_guard::MAX_FILE_BYTES as u64;
        let mut bytes = Vec::new();
        let set = if meta.len() > limit {
            file_problem("the file is larger than 1 MiB")
        } else {
            match (&mut handle).take(limit + 1).read_to_end(&mut bytes) {
                Err(error) => file_problem(&format!("not readable: {error}")),
                Ok(_) if bytes.len() as u64 > limit => {
                    file_problem("the file is larger than 1 MiB")
                }
                Ok(_) => match String::from_utf8(bytes) {
                    Ok(text) => command_guard::parse_rules(&text),
                    Err(_) => file_problem("the file is not UTF-8 text"),
                },
            }
        };
        let set = Arc::new(set);
        let mut guard = lock(&self.guard);
        guard.stamp = Some(now);
        guard.rules = Arc::clone(&set);
        set
    }

    /// Hangar's own folders: the config folder (the guard's file) and the local data folder (the
    /// remembered answers, the hook folders).
    /// Only the folders that hold settings or gate state: the config folder (the guard's rules,
    /// the profiles) and the gate's own folder (hook settings, the extensions' session folders).
    /// Not the local data folder as a whole, where the team sessions' worktrees live.
    fn protected_folders(&self) -> Vec<PathBuf> {
        let config = lock(&self.guard)
            .file
            .as_ref()
            .and_then(|file| file.parent().map(Path::to_path_buf));
        config
            .into_iter()
            .chain(std::iter::once(self.settings_dir.clone()))
            .collect()
    }

    /// The settings files themselves, wherever they lie: the guard's rules and the remembered
    /// answers (in the local data folder).
    fn protected_files(&self) -> Vec<PathBuf> {
        let guard = lock(&self.guard).file.clone();
        let rules = lock(&self.rules).file.clone();
        [guard, rules].into_iter().flatten().collect()
    }

    /// Where a shell command or an edit of `session` runs, for the guard's checks. Without a
    /// usable working folder from the hook, relative paths stay unplaceable (asked about).
    fn place_for(&self, cwd: Option<&str>, own: Option<&WorktreeClaims>) -> Place {
        let key = |path: &Path| crate::git::path_key(path);
        let protected: Vec<String> = self.protected_folders().iter().map(|p| key(p)).collect();
        // The identifier of the config folder; the data folder has the same name, which is why
        // names only count for paths outside the session's project.
        let names: Vec<&str> = protected.first().map(String::as_str).into_iter().collect();
        Place {
            cwd: cwd.map(Path::new).filter(|cwd| cwd.is_absolute()).map(key),
            project: own.map(|own| own.root_key.clone()),
            home: home_folder().as_deref().map(key),
            protected_names: command_guard::protected_names(&names),
            protected_files: self.protected_files().iter().map(|p| key(p)).collect(),
            protected,
        }
    }

    /// Who sent the request. A browser page (`Origin`, `Sec-Fetch-*`) or another `Host` than the
    /// one in the hook URL is refused before the token is looked at.
    fn admit(&self, head: &Head) -> Result<(SessionId, TokenScope), Status> {
        let port = *lock(&self.port);
        let expected = port.map(|port| format!("127.0.0.1:{port}"));
        if head.from_browser || head.host.is_none() || head.host != expected {
            return Err(Status::Forbidden);
        }
        head.token
            .as_deref()
            .and_then(|token| lock(&self.gate).holder(token))
            .ok_or(Status::Unauthorized)
    }

    /// A message `hangar-send` posted for `session`: the body is bounded before it is read, the
    /// partner service checks and keeps it.
    fn send(&self, session: SessionId, head: &Head, body: &mut impl Read) -> (Status, String) {
        if head.content_length > MAX_SEND_BODY_BYTES {
            return (Status::PayloadTooLarge, String::new());
        }
        let Some(request) = read_send_request(body, head.content_length) else {
            return (Status::BadRequest, String::new());
        };
        // Not under the lock: the service reads files and the session list.
        let messages = lock(&self.messages).clone();
        match messages {
            Some(messages) => messages(session, request),
            None => (Status::Unavailable, String::new()),
        }
    }

    /// The answer to a known session's request that finds no free slot, without reading its
    /// body. A tool hook is denied while the gate is on: an unanswered one (503, closed) lets
    /// the call go ahead unchecked, so a session flooding the endpoint could wave a command
    /// past the guard. With the gate off it gets "no decision": Claude Code's hook blocks the
    /// call on an error (`onFailure: "block"`). The prompt hook only adds context and may go
    /// without.
    fn overloaded(&self, head: &Head) -> (Status, String) {
        let tool_hook = matches!(
            head.path.as_str(),
            ROUTE_PRE_TOOL_USE | ROUTE_PRE_TOOL_USE_JSON
        );
        if !tool_hook {
            return (Status::Unavailable, String::new());
        }
        if lock(&self.gate).enabled() {
            (Status::Ok, Outcome::Deny(gate::busy_reason()).body())
        } else if head.path == ROUTE_PRE_TOOL_USE_JSON {
            (Status::Ok, gate::EMPTY_JSON.to_owned())
        } else {
            (Status::Ok, String::new())
        }
    }

    /// Each route answers only a token of its own scope: a send token opens no hook, a hook token
    /// sends no message.
    fn route(
        &self,
        session: SessionId,
        scope: TokenScope,
        head: &Head,
        reader: &mut impl Read,
        stream: &TcpStream,
    ) -> (Status, String) {
        let path = head.path.as_str();
        let wanted = match path {
            ROUTE_SEND => TokenScope::Send,
            ROUTE_PRE_TOOL_USE
            | ROUTE_PRE_TOOL_USE_JSON
            | ROUTE_USER_PROMPT_SUBMIT
            | ROUTE_USER_PROMPT_SUBMIT_JSON => TokenScope::Hooks,
            ROUTE_NOTIFY_ASKS | ROUTE_NOTIFY_CLEAR => TokenScope::Notify,
            _ => return (Status::NotFound, String::new()),
        };
        if scope != wanted {
            return (Status::Forbidden, String::new());
        }
        let mut body = reader.take(head.content_length);
        if path == ROUTE_SEND {
            return self.send(session, head, &mut body);
        }
        if wanted == TokenScope::Notify {
            return self.notice(session, head, &mut body);
        }
        // The JSON routes answer "no decision" with `{}` instead of an empty body.
        let json = path == ROUTE_PRE_TOOL_USE_JSON || path == ROUTE_USER_PROMPT_SUBMIT_JSON;
        let filled = |body: String| {
            if json && body.is_empty() {
                gate::EMPTY_JSON.to_owned()
            } else {
                body
            }
        };
        match path {
            ROUTE_PRE_TOOL_USE | ROUTE_PRE_TOOL_USE_JSON => {
                match gate::parse_pre_tool_use(&mut body) {
                    Ok(request) => (
                        Status::Ok,
                        filled(self.decide(session, request, stream).body()),
                    ),
                    // A known session's call Hangar cannot read is blocked while the gate is on,
                    // with a reason (a refused request lets Claude Code's hooks older than
                    // `onFailure` go on). With the gate off it goes ahead: "no decision", since
                    // an error now blocks the call (`onFailure: "block"`).
                    Err(_) if lock(&self.gate).enabled() => (
                        Status::Ok,
                        Outcome::Deny(gate::unreadable_call_reason()).body(),
                    ),
                    Err(_) => (Status::Ok, filled(String::new())),
                }
            }
            ROUTE_USER_PROMPT_SUBMIT | ROUTE_USER_PROMPT_SUBMIT_JSON => {
                match gate::parse_prompt_submit(&mut body) {
                    Ok(conversation) => (
                        Status::Ok,
                        filled(self.context(session, conversation.as_deref())),
                    ),
                    Err(_) => (Status::BadRequest, String::new()),
                }
            }
            _ => (Status::NotFound, String::new()),
        }
    }

    /// The teammates' files in the repository of `session`'s worktree.
    fn teammates_of(&self, session: SessionId, worktrees: &[WorktreeClaims]) -> Vec<TeammateFile> {
        let Some((repo, folder)) = worktrees
            .iter()
            .find(|w| w.sessions.contains(&session))
            .and_then(|w| Some((w.root_commit.as_deref()?, w.repo.as_str())))
        else {
            return Vec::new();
        };
        lock(&self.teammates)
            .as_ref()
            .map(|teammates| teammates(repo, folder))
            .unwrap_or_default()
    }

    /// The team's reservations for the clone of `session`'s worktree.
    fn tasks_of(&self, session: SessionId, worktrees: &[WorktreeClaims]) -> Vec<TeamTask> {
        let Some((repo, folder)) = worktrees
            .iter()
            .find(|w| w.sessions.contains(&session))
            .and_then(|w| Some((w.root_commit.as_deref()?, w.repo.as_str())))
        else {
            return Vec::new();
        };
        lock(&self.team_tasks)
            .as_ref()
            .map(|tasks| tasks(repo, folder))
            .unwrap_or_default()
    }

    fn context(&self, session: SessionId, conversation: Option<&str>) -> String {
        if !lock(&self.gate).enabled() {
            return String::new();
        }
        let worktrees = (self.worktrees)();
        let titles = (self.titles)();
        let teammates = self.teammates_of(session, &worktrees);
        let tasks = self.tasks_of(session, &worktrees);
        // Built before the gate's lock: it may look into the worktree for up to 50 ms, and
        // other sessions' edits wait on that lock.
        let Some(text) = gate::context_facts_full(session, &worktrees, &titles, &teammates, &tasks)
        else {
            return String::new();
        };
        lock(&self.gate)
            .tell(session, conversation, text)
            .map(|text| gate::context_body(&text))
            .unwrap_or_default()
    }

    /// Answers one edit hook: at once when nothing collides, otherwise once the person decided,
    /// the time ran out or the agent stopped waiting.
    fn decide(&self, session: SessionId, request: EditRequest, stream: &TcpStream) -> Outcome {
        if !lock(&self.gate).enabled() {
            return Outcome::Allow;
        }
        if let Some(command) = request.command.clone() {
            return self.decide_command(session, &request, command, stream);
        }
        // Claude Code sends every tool (matcher `*`); only edits are looked at here.
        if !gate::is_edit_tool(&request.tool) {
            return Outcome::Allow;
        }
        let worktrees = (self.worktrees)();
        let own = worktrees.iter().find(|w| w.sessions.contains(&session));
        // A path the gate cannot read is asked about: refusing the request would let it through.
        if let Some(start) = &request.unreadable_path {
            let text = format!("{}: {start}", request.tool);
            let hit = Hit::builtin(RULE_TOO_LONG, String::new());
            return self.ask_command(session, &request.tool, own, text, vec![hit], stream, false);
        }
        let Some(path) = request.path.as_deref() else {
            return Outcome::Allow;
        };
        // An edit of Hangar's own settings is held like a risky command, whatever collides; also
        // through a link inside the project that leads there.
        let place = self.place_for(request.cwd.as_deref(), own);
        let candidates = own
            .map(|own| path_candidates(path, request.cwd.as_deref(), Path::new(&own.root_key)))
            .unwrap_or_default();
        if command_guard::edit_touches_hangar(path, &place, Style::HOST)
            || candidates
                .iter()
                .any(|key| command_guard::edit_touches_hangar(key, &place, Style::HOST))
        {
            let text = format!("{}: {path}", request.tool);
            let hit = Hit::builtin(RULE_HANGAR_CONFIG, text.clone());
            return self.ask_command(session, &request.tool, own, text, vec![hit], stream, false);
        }
        // A session outside every known worktree cannot collide; its path is not even looked at.
        let Some(own) = own else {
            return Outcome::Allow;
        };
        let teammates = self.teammates_of(session, &worktrees);
        let tasks = self.tasks_of(session, &worktrees);
        let Some(conflict) =
            gate::assess_with_tasks(session, &candidates, &worktrees, &teammates, &tasks)
        else {
            return Outcome::Allow;
        };
        let titles = (self.titles)();
        // A gate rule answers before the person is asked; deny rules win.
        let rule = lock(&self.rules)
            .book
            .find(&own.repo, &conflict.key)
            .cloned();
        let named: Vec<String> = conflict
            .others
            .iter()
            .map(|id| titles.get(id).cloned().unwrap_or_else(|| format!("#{id}")))
            .chain(conflict.teammates.iter().cloned())
            .chain(
                conflict
                    .reserved
                    .iter()
                    .map(|r| format!("{}: {}", r.who, r.title)),
            )
            .collect();
        let shown_path = conflict.path.clone();
        if let Some(rule) = rule {
            let outcome = match rule.effect {
                RuleEffect::Allow => Outcome::Allow,
                RuleEffect::Deny => Outcome::Deny(gate::rule_denied_reason(&shown_path, &named)),
            };
            {
                let mut gate = lock(&self.gate);
                if !gate.enabled() {
                    return Outcome::Allow;
                }
                gate.record(log_entry(&rule, session, &titles, &shown_path));
            }
            self.publish();
            return outcome;
        }
        let (tx, rx) = sync_channel(1);
        let opened = {
            let mut gate = lock(&self.gate);
            if !gate.enabled() {
                return Outcome::Allow;
            }
            gate.open(
                session,
                &request.tool,
                conflict,
                named.clone(),
                now_millis(),
                tx,
            )
        };
        let (id, ticket) = match opened {
            Opened::Waiting { id, ticket } => (id, ticket),
            Opened::Allowed => return Outcome::Allow,
            Opened::Full => return Outcome::Ask(gate::full_reason(&shown_path, &named)),
        };
        self.publish();
        self.hold(id, ticket, &rx, stream)
    }

    /// Answers one shell command: at once when no guard rule matches it, by the person's
    /// remembered answers for the rules in this project, or once the person decided. With the
    /// guard switched off only Hangar's own settings are still asked about.
    fn decide_command(
        &self,
        session: SessionId,
        request: &EditRequest,
        command: CommandText,
        stream: &TcpStream,
    ) -> Outcome {
        let guard_on = self.guard_enabled.load(Ordering::SeqCst);
        let worktrees = (self.worktrees)();
        let own = worktrees.iter().find(|w| w.sessions.contains(&session));
        let place = self.place_for(request.cwd.as_deref(), own);
        let (text, hits) = match command {
            CommandText::TooLong(start) => {
                (start, vec![Hit::builtin(RULE_TOO_LONG, String::new())])
            }
            CommandText::Text(text) => {
                let rules = self.guard_rules();
                let dialects = command_guard::dialects_of(&request.tool);
                let hits = command_guard::check(&rules.rules, &text, dialects, &place, Style::HOST);
                (text, hits)
            }
        };
        let hits = hits
            .into_iter()
            .filter(|hit| guard_on || hit.rule_id == RULE_HANGAR_CONFIG)
            .collect();
        self.ask_command(session, &request.tool, own, text, hits, stream, true)
    }

    /// The person's decision about `hits` of one command (or an edit of Hangar's settings): a
    /// remembered deny for any rule blocks it, rules with a remembered allow drop out, and the
    /// rest are asked about on one card. Allowed at once when nothing is left. `guarded`: the
    /// hits come from the command guard, so switching it off before the card opens drops all
    /// but Hangar's own settings.
    #[allow(clippy::too_many_arguments)]
    fn ask_command(
        &self,
        session: SessionId,
        tool: &str,
        own: Option<&WorktreeClaims>,
        text: String,
        hits: Vec<Hit>,
        stream: &TcpStream,
        guarded: bool,
    ) -> Outcome {
        if hits.is_empty() {
            return Outcome::Allow;
        }
        let mut remaining = Vec::new();
        let mut remembered = Vec::new();
        {
            let rules = lock(&self.rules);
            for hit in hits {
                let rule = own.and_then(|own| {
                    rules
                        .book
                        .find_command(&own.repo, &hit.rule_id)
                        .filter(|_| command_guard::rememberable(&hit.rule_id))
                        .cloned()
                });
                match rule {
                    Some(rule) => remembered.push((rule, hit)),
                    None => remaining.push(hit),
                }
            }
        }
        if !remembered.is_empty() {
            let titles = (self.titles)();
            let shown = command_guard::shown(&text, gate_rules::LOG_COMMAND_CHARS);
            {
                let mut gate = lock(&self.gate);
                if !gate.enabled() {
                    return Outcome::Allow;
                }
                for (rule, _) in &remembered {
                    gate.record(log_entry(rule, session, &titles, &shown));
                }
            }
            self.publish();
        }
        if let Some((_, hit)) = remembered
            .iter()
            .find(|(rule, _)| rule.effect == RuleEffect::Deny)
        {
            return Outcome::Deny(gate::command_rule_denied_reason(&[AskRule::new(
                &hit.rule_id,
                &hit.description,
            )]));
        }
        if remaining.is_empty() {
            return Outcome::Allow;
        }
        let (tx, rx) = sync_channel(1);
        let (opened, rules) = {
            let mut gate = lock(&self.gate);
            if !gate.enabled() {
                return Outcome::Allow;
            }
            // Read under the gate's lock: `configure_command_guard` clears the flag before it
            // takes this lock to release held commands, so a card opened here after that sees
            // it, and one opened before is released there.
            if guarded && !self.guard_enabled.load(Ordering::SeqCst) {
                remaining.retain(|hit| hit.rule_id == RULE_HANGAR_CONFIG);
                if remaining.is_empty() {
                    return Outcome::Allow;
                }
            }
            let call = remaining
                .iter()
                .find(|hit| !hit.call.is_empty())
                .map(|hit| hit.call.clone())
                .unwrap_or_default();
            let rules: Vec<AskRule> = remaining
                .iter()
                .map(|hit| AskRule::new(&hit.rule_id, &hit.description))
                .collect();
            let conflict = CommandConflict {
                rules: rules.clone(),
                call,
                text,
            };
            (
                gate.open_command(session, tool, conflict, now_millis(), tx),
                rules,
            )
        };
        let (id, ticket) = match opened {
            Opened::Waiting { id, ticket } => (id, ticket),
            Opened::Allowed => return Outcome::Allow,
            // Not the agent's own prompt: in an unattended mode it may answer itself.
            Opened::Full => return Outcome::Deny(gate::command_full_reason(&rules)),
        };
        self.publish();
        self.hold(id, ticket, &rx, stream)
    }

    /// Waits for the outcome of decision `id`: the person's answer, a rule set meanwhile, the
    /// expiry after the decision time, or the agent closing the connection.
    fn hold(
        &self,
        id: u64,
        ticket: u64,
        rx: &std::sync::mpsc::Receiver<Outcome>,
        stream: &TcpStream,
    ) -> Outcome {
        let deadline = Instant::now() + self.limits.decision;
        loop {
            match rx.recv_timeout(WAIT_TICK.min(self.limits.decision)) {
                Ok(outcome) => return outcome,
                // Dropped without an answer: only when withdrawn, which this thread does.
                Err(RecvTimeoutError::Disconnected) => return Outcome::Allow,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if Instant::now() >= deadline {
                let expired = lock(&self.gate).expire(id);
                if let Some(outcomes) = expired {
                    deliver(outcomes);
                    self.publish();
                }
                // This request's own outcome is in the channel now, or was put there by the
                // answer or expiry that came first.
                continue;
            }
            if client_gone(stream) {
                let removed = lock(&self.gate).withdraw(id, ticket);
                if removed {
                    self.publish();
                }
                return Outcome::Allow;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_catalog::{Capabilities, CatalogAgent};
    use crate::claims::{ChangeKind, Claim, LineRange};
    use crate::test_support::TempDir;
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc::{channel, Receiver};

    fn claude() -> Agent {
        let mut catalog = crate::agent_catalog::builtin()
            .unwrap()
            .into_iter()
            .find(|a| a.id == "claude")
            .unwrap();
        catalog.capabilities.context_inject = true;
        Agent::from_catalog(catalog)
    }

    fn plain_agent() -> Agent {
        let catalog: CatalogAgent =
            crate::agent_catalog::parse("[[agent]]\nid='x'\nname='X'\ncommands=['x']\n")
                .unwrap()
                .remove(0);
        Agent::from_catalog(catalog)
    }

    struct Fixture {
        service: GateService,
        events: Receiver<GateSnapshot>,
        root: PathBuf,
        settings: TempDir,
    }

    /// Short limits so the slow-client tests finish quickly.
    fn limits(decision: Duration) -> Limits {
        Limits {
            head: Duration::from_millis(1500),
            grace: Duration::from_millis(300),
            body: Duration::from_secs(5),
            linger: Duration::from_millis(500),
            write: Duration::from_secs(5),
            decision,
        }
    }

    /// Session 1 in `root`, session 2 in another worktree of the same repository that changes
    /// `src/shared.rs` lines 10 to 12.
    fn fixture(decision: Duration) -> Fixture {
        fixture_at(
            std::env::current_dir().unwrap().join("repo-under-test"),
            decision,
        )
    }

    /// [`fixture`] with session 1's worktree at `root`.
    fn fixture_at(root: PathBuf, decision: Duration) -> Fixture {
        let root_key = crate::git::path_key(&root);
        let worktrees = Arc::new(vec![
            WorktreeClaims {
                repo: "repo".into(),
                root_key,
                name: "repo".into(),
                root_commit: Some("r".repeat(40)),
                base: Some("b".into()),
                sessions: vec![1],
                claims: Vec::new(),
                truncated: false,
            },
            WorktreeClaims {
                repo: "repo".into(),
                root_key: "elsewhere/feature".into(),
                name: "feature".into(),
                root_commit: None,
                truncated: false,
                base: Some("b".into()),
                sessions: vec![2],
                claims: vec![Claim {
                    path: "src/shared.rs".into(),
                    kind: ChangeKind::Modified,
                    whole_file: false,
                    ranges: vec![LineRange { start: 10, end: 12 }],
                    area: Some("src".into()),
                }],
            },
        ]);
        let (tx, events) = channel();
        let tx = Mutex::new(tx);
        // One folder per fixture: tests run in parallel in one process.
        static FIXTURES: AtomicUsize = AtomicUsize::new(0);
        let n = FIXTURES.fetch_add(1, Ordering::SeqCst);
        let settings = TempDir::new(&format!("gate-settings-{n}"));
        let service = GateService::with_limits(
            settings.path().to_path_buf(),
            move || Arc::clone(&worktrees),
            || [(2, "Codex · feature".to_owned())].into_iter().collect(),
            move |snapshot: &GateSnapshot| {
                let _ = lock(&tx).send(snapshot.clone());
            },
            limits(decision),
        );
        Fixture {
            service,
            events,
            root,
            settings,
        }
    }

    fn launched(plan: GatePlan) -> GateLaunch {
        match plan {
            GatePlan::Launch(launch) => launch,
            GatePlan::Skip => panic!("skipped"),
            GatePlan::Unavailable(status) => panic!("unavailable: {status:?}"),
        }
    }

    fn launch(fixture: &Fixture, session: SessionId) -> (u16, String) {
        let launch = launched(fixture.service.prepare(
            session,
            &claude(),
            Path::new("claude.exe"),
            &[],
            None,
        ));
        let token = launch.env[0].1.to_string();
        (fixture.service.port().unwrap(), token)
    }

    fn connect(port: u16) -> TcpStream {
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        stream
    }

    /// A raw HTTP/1.1 request as Claude Code sends it; the answer's status and body.
    fn post(port: u16, path: &str, token: Option<&str>, body: &str) -> (u16, String) {
        post_with(
            port,
            path,
            token,
            &format!("Host: 127.0.0.1:{port}\r\n"),
            body,
        )
    }

    /// [`post`] with other headers than `Host` and the token.
    fn post_with(
        port: u16,
        path: &str,
        token: Option<&str>,
        headers: &str,
        body: &str,
    ) -> (u16, String) {
        let mut stream = connect(port);
        let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
        let request = format!(
            "POST {path} HTTP/1.1\r\n{headers}Content-Type: application/json\r\n\
             {auth}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        read_response(stream)
    }

    fn read_response(stream: TcpStream) -> (u16, String) {
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader.read_line(&mut status).unwrap();
        let code = status
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let mut rest = String::new();
        if let Err(error) = reader.read_to_string(&mut rest) {
            panic!("reading the answer: {error:?}, so far {rest:?}, status {status:?}");
        }
        let body = rest.split_once("\r\n\r\n").map(|(_, b)| b.to_owned());
        (code, body.unwrap_or_default())
    }

    fn edit(root: &Path, file: &str) -> String {
        serde_json::json!({
            "session_id": "x", "cwd": root, "hook_event_name": "PreToolUse",
            "tool_name": "Edit",
            "tool_input": { "file_path": root.join(file), "old_string": "a", "new_string": "b" },
        })
        .to_string()
    }

    fn next_pending(events: &Receiver<GateSnapshot>) -> GateSnapshot {
        loop {
            let snapshot = events.recv_timeout(Duration::from_secs(20)).unwrap();
            if !snapshot.pending.is_empty() {
                return snapshot;
            }
        }
    }

    /// Waits until the connection is closed by the server; how long that took.
    fn closed_after(mut stream: TcpStream, started: Instant) -> Duration {
        let mut buf = [0u8; 256];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return started.elapsed(),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    panic!("still open after {:?}", started.elapsed())
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    panic!("still open after {:?}", started.elapsed())
                }
                // Reset: closed as well.
                Err(_) => return started.elapsed(),
            }
        }
    }

    #[test]
    fn an_edit_of_a_file_a_teammate_changes_waits_for_the_person() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.set_teammates(|repo, _folder| {
            if repo != "r".repeat(40) {
                return Vec::new();
            }
            vec![TeammateFile {
                repo: repo.to_owned(),
                path: "src/team.rs".into(),
                key: "src/team.rs".into(),
                who: "Anna (Claude Code)".into(),
                lines: vec![LineRange { start: 4, end: 9 }],
            }]
        });
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/team.rs");
        let waiting = thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body));
        let snapshot = next_pending(&fixture.events);
        let decision = &snapshot.pending[0];
        assert_eq!(decision.teammates, ["Anna (Claude Code)"]);
        assert!(decision.other_sessions.is_empty());
        assert_eq!(decision.lines, [LineRange { start: 4, end: 9 }]);
        assert!(fixture.service.answer(decision.id, Answer::Deny));
        let (code, body) = waiting.join().unwrap();
        assert_eq!(code, 200);
        assert!(body.contains("\"deny\""), "{body}");
        assert!(body.contains("Anna (Claude Code)"), "{body}");
        // A file no teammate changes goes through at once.
        let (_, token) = launch(&fixture, 1);
        let (code, body) = post(
            port,
            ROUTE_PRE_TOOL_USE,
            Some(&token),
            &edit(&fixture.root, "src/other.rs"),
        );
        assert_eq!((code, body.as_str()), (200, ""));
    }

    #[test]
    fn launch_gets_settings_only_when_on_and_capable() {
        let fixture = fixture(DECISION_TIMEOUT);
        let program = Path::new("claude.exe");
        // Off by default: nothing added, no endpoint.
        assert!(matches!(
            fixture.service.prepare(1, &claude(), program, &[], None),
            GatePlan::Skip
        ));
        assert_eq!(fixture.service.port(), None);
        fixture.service.configure(true);
        assert!(matches!(
            fixture
                .service
                .prepare(1, &plain_agent(), program, &[], None),
            GatePlan::Skip
        ));
        let launch = launched(fixture.service.prepare(1, &claude(), program, &[], None));
        assert_eq!(launch.args[0], "--settings");
        assert!(launch.args[1].contains(&format!("127.0.0.1:{}", fixture.service.port().unwrap())));
        assert_eq!(launch.env[0].0, TOKEN_ENV);
        // The token is in the environment only, never in the arguments.
        assert!(!launch.args[1].contains(launch.env[0].1.as_str()));
        let mut no_context = claude();
        no_context.capabilities = Capabilities {
            context_inject: false,
            ..no_context.capabilities
        };
        let launch = launched(fixture.service.prepare(2, &no_context, program, &[], None));
        assert!(!launch.args[1].contains(ROUTE_USER_PROMPT_SUBMIT));
    }

    #[test]
    fn only_a_claude_code_that_knows_on_failure_gets_it() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let program = Path::new("claude.exe");
        let hook_of = |version: Option<&str>| -> serde_json::Value {
            let launch = launched(fixture.service.prepare(1, &claude(), program, &[], version));
            let json: serde_json::Value = serde_json::from_str(&launch.args[1]).unwrap();
            json["hooks"]["PreToolUse"][0]["hooks"][0].clone()
        };
        for version in [None, Some("2.1.294"), Some("unknown")] {
            assert!(hook_of(version).get("onFailure").is_none(), "{version:?}");
        }
        for version in ["2.1.295", "2.1.325"] {
            assert_eq!(hook_of(Some(version))["onFailure"], "block", "{version}");
        }
    }

    #[test]
    fn a_cmd_launcher_gets_the_settings_as_a_file_without_the_token() {
        if !cfg!(windows) {
            return;
        }
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let program = Path::new(r"C:\npm\claude.cmd");
        let launch = launched(fixture.service.prepare(1, &claude(), program, &[], None));
        let path = PathBuf::from(&launch.args[1]);
        assert!(path.starts_with(fixture.settings.path()), "{path:?}");
        // What cmd.exe passes on unchanged, as the manager checks before the start.
        assert_eq!(crate::batch::check(program, &launch.args), Ok(()));
        let text = std::fs::read_to_string(&path).unwrap();
        let port = fixture.service.port().unwrap();
        assert_eq!(text, gate::claude_settings(port, true, false, false));
        assert!(!text.contains(launch.env[0].1.as_str()));
        // A second start reuses the file.
        let again = launched(fixture.service.prepare(2, &claude(), program, &[], None));
        assert_eq!(again.args[1], launch.args[1]);
    }

    #[test]
    fn old_settings_files_go_unless_their_port_still_listens() {
        let dir = TempDir::new("gate-settings-old");
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let held_port = held.local_addr().unwrap().port();
        let free_port = {
            let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            probe.local_addr().unwrap().port()
        };
        let names = [
            format!("{SETTINGS_PREFIX}{held_port}-context.json"),
            format!("{SETTINGS_PREFIX}{free_port}.json"),
            format!("{SETTINGS_PREFIX}{free_port}-context.json.77.tmp"),
            format!("{SETTINGS_PREFIX}junk.json"),
            "other.json".to_owned(),
        ];
        for name in &names {
            std::fs::write(dir.path().join(name), "{}").unwrap();
        }
        remove_old_settings(dir.path());
        let left: Vec<bool> = names.iter().map(|n| dir.path().join(n).exists()).collect();
        // Another running Hangar's file stays; stale ones, leftovers and junk go; others stay.
        assert_eq!(left, [true, false, false, false, true]);
        assert_eq!(settings_port("8123-context.json.5.tmp"), Some(8123));
        assert_eq!(settings_port("0.json"), None);
        assert_eq!(settings_port("99999.json"), None);
        drop(held);
    }

    #[test]
    fn a_settings_path_cmd_would_interpret_leaves_the_session_without_gate() {
        if !cfg!(windows) {
            return;
        }
        let dir = TempDir::new("gate-settings-odd");
        let odd = dir.path().join("a&b");
        let service = GateService::with_limits(
            odd,
            || Arc::new(Vec::new()),
            HashMap::new,
            |_: &GateSnapshot| {},
            limits(DECISION_TIMEOUT),
        );
        service.configure(true);
        assert!(matches!(
            service.prepare(1, &claude(), Path::new(r"C:\npm\claude.cmd"), &[], None),
            GatePlan::Unavailable(GateStatus::Launcher)
        ));
        // A real program file still gets the gate inline.
        assert!(matches!(
            service.prepare(1, &claude(), Path::new("claude.exe"), &[], None),
            GatePlan::Launch(_)
        ));
    }

    #[test]
    fn endpoint_refuses_unknown_tokens_and_routes() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, None, &body).0, 401);
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some("wrong"), &body).0, 401);
        let mut near = token.clone();
        near.pop();
        near.push('x');
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some(&near), &body).0, 401);
        assert_eq!(post(port, "/v1/other", Some(&token), &body).0, 404);
        // A body Hangar cannot read is blocked, not refused: a refused hook lets the call go on.
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some(&token), "{").0, 200);
        // A session's token stops working when the session ends.
        fixture.service.ended(1);
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body).0, 401);
    }

    /// The private session's notice launch and the token in it.
    fn notice_launch(fixture: &Fixture, session: SessionId) -> (u16, String, GateLaunch) {
        let launch = fixture
            .service
            .notify(session, &claude(), Path::new("claude.exe"), Some("2.1.329"))
            .expect("notice hooks");
        assert_eq!(launch.env.len(), 1);
        assert_eq!(launch.env[0].0, NOTIFY_TOKEN_ENV);
        let token = launch.env[0].1.to_string();
        (fixture.service.port().unwrap(), token, launch)
    }

    #[test]
    fn a_private_session_gets_report_only_hooks_and_no_gate() {
        let fixture = fixture(DECISION_TIMEOUT);
        // The gate setting is off: the notices do not depend on it.
        let (_, token, launch) = notice_launch(&fixture, 4);
        assert_eq!(launch.args[0], "--settings");
        assert!(launch.prepend.is_empty());
        let json: serde_json::Value = serde_json::from_str(&launch.args[1]).unwrap();
        assert!(json["hooks"].get("PreToolUse").is_none());
        assert!(json["hooks"].get("PermissionRequest").is_some());
        assert!(!launch.args[1].contains(&token));
        // Nothing for another program, an older or unknown Claude Code.
        let program = Path::new("claude.exe");
        assert!(fixture
            .service
            .notify(5, &plain_agent(), program, Some("2.1.329"))
            .is_none());
        for version in [None, Some("2.1.200")] {
            assert!(fixture
                .service
                .notify(5, &claude(), program, version)
                .is_none());
        }
        // Nothing waits for a decision of a private session.
        assert!(fixture.service.snapshot().pending.is_empty());
    }

    #[test]
    fn a_notice_token_opens_the_notice_routes_only_and_decides_nothing() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, notice, _) = notice_launch(&fixture, 4);
        let (_, hooks) = launch(&fixture, 1);
        let (_, send) = fixture.service.send_token(4).unwrap();
        let body = r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash"}"#;
        // A notice is answered with an empty 200: no decision, the person's dialog stays.
        assert_eq!(
            post(port, ROUTE_NOTIFY_ASKS, Some(&notice), body),
            (200, String::new())
        );
        assert_eq!(fixture.service.permission_waiting(), [4]);
        // The notice token opens no gate route and sends no message.
        let edit_body = edit(&fixture.root, "src/shared.rs");
        for route in [
            ROUTE_PRE_TOOL_USE,
            ROUTE_PRE_TOOL_USE_JSON,
            ROUTE_USER_PROMPT_SUBMIT,
        ] {
            assert_eq!(
                post(port, route, Some(&notice), &edit_body).0,
                403,
                "{route}"
            );
        }
        assert_eq!(
            post(
                port,
                ROUTE_SEND,
                Some(&notice),
                r#"{"partner":"web","text":"hi"}"#
            )
            .0,
            403
        );
        // Neither a hook token nor a send token reaches the notice routes.
        assert_eq!(post(port, ROUTE_NOTIFY_ASKS, Some(&hooks), body).0, 403);
        assert_eq!(
            post(port, ROUTE_NOTIFY_CLEAR, Some(send.as_str()), body).0,
            403
        );
        assert_eq!(post(port, ROUTE_NOTIFY_ASKS, None, body).0, 401);
        assert_eq!(post(port, "/v1/notification", Some(&notice), body).0, 404);
        assert!(fixture.service.snapshot().pending.is_empty());
    }

    #[test]
    fn notices_set_and_clear_the_signal_and_tell_the_window() {
        let fixture = fixture(DECISION_TIMEOUT);
        let signals = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&signals);
        fixture.service.set_permission_signal(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        let (port, token, _) = notice_launch(&fixture, 4);
        let (_, other, _) = notice_launch(&fixture, 6);
        let ask = |token: &str, body: &str| post(port, ROUTE_NOTIFY_ASKS, Some(token), body).0;
        let clear = |token: &str, body: &str| post(port, ROUTE_NOTIFY_CLEAR, Some(token), body).0;
        // An idle notice is no dialog, even when it reaches the asks route.
        let idle = r#"{"hook_event_name":"Notification","notification_type":"idle_prompt"}"#;
        assert_eq!(ask(&token, idle), 200);
        assert!(fixture.service.permission_waiting().is_empty());
        let prompt = r#"{"hook_event_name":"Notification","message":"Claude needs your permission","notification_type":"permission_prompt"}"#;
        assert_eq!(ask(&token, prompt), 200);
        assert_eq!(
            ask(&other, r#"{"hook_event_name":"PermissionRequest"}"#),
            200
        );
        assert_eq!(fixture.service.permission_waiting(), [4, 6]);
        assert_eq!(signals.load(Ordering::SeqCst), 2);
        // The same dialog again changes nothing to show.
        assert_eq!(ask(&token, prompt), 200);
        assert_eq!(signals.load(Ordering::SeqCst), 2);
        // The tool ran: the dialog is gone; a session's notice never clears another's.
        assert_eq!(clear(&token, r#"{"hook_event_name":"PostToolUse"}"#), 200);
        assert_eq!(fixture.service.permission_waiting(), [6]);
        assert_eq!(signals.load(Ordering::SeqCst), 3);
        // A PermissionRequest with a whole file in it: not kept, the route still counts.
        let large = format!(
            r#"{{"hook_event_name":"PermissionRequest","tool_input":{{"content":"{}"}}}}"#,
            "m".repeat(200 * 1024)
        );
        assert_eq!(ask(&token, &large), 200);
        assert_eq!(fixture.service.permission_waiting(), [4, 6]);
        // A body that is no JSON: the route decides.
        assert_eq!(clear(&other, "not json"), 200);
        assert_eq!(fixture.service.permission_waiting(), [4]);
        // The process ends: its signal goes with it.
        fixture.service.ended(4);
        assert!(fixture.service.permission_waiting().is_empty());
        assert_eq!(signals.load(Ordering::SeqCst), 6);
        // And its token is gone.
        assert_eq!(ask(&token, prompt), 401);
    }

    #[test]
    fn the_persons_answer_clears_the_signal_of_that_session_only() {
        let fixture = fixture(DECISION_TIMEOUT);
        let signals = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&signals);
        fixture.service.set_permission_signal(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        let (port, four, _) = notice_launch(&fixture, 4);
        let (_, six, _) = notice_launch(&fixture, 6);
        let body = r#"{"hook_event_name":"PermissionRequest"}"#;
        assert_eq!(post(port, ROUTE_NOTIFY_ASKS, Some(&four), body).0, 200);
        assert_eq!(post(port, ROUTE_NOTIFY_ASKS, Some(&six), body).0, 200);
        fixture.service.permission_answered(4);
        assert_eq!(fixture.service.permission_waiting(), [6]);
        assert_eq!(signals.load(Ordering::SeqCst), 3);
        // Nothing to clear: no signal.
        fixture.service.permission_answered(4);
        assert_eq!(signals.load(Ordering::SeqCst), 3);
        // The next dialog sets it again.
        assert_eq!(post(port, ROUTE_NOTIFY_ASKS, Some(&four), body).0, 200);
        assert_eq!(fixture.service.permission_waiting(), [4, 6]);
    }

    #[test]
    fn a_notice_that_arrives_after_its_session_ended_sets_nothing() {
        let fixture = fixture(DECISION_TIMEOUT);
        let (port, token, _) = notice_launch(&fixture, 4);
        // The request is admitted, then the process ends while its body is still on the way.
        let mut stream = connect(port);
        let body = r#"{"hook_event_name":"PermissionRequest"}"#;
        let head = format!(
            "POST {ROUTE_NOTIFY_ASKS} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             Authorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(200));
        fixture.service.ended(4);
        stream.write_all(body.as_bytes()).unwrap();
        let (status, _) = read_response(stream);
        assert_eq!(status, 200);
        assert!(fixture.service.permission_waiting().is_empty());
    }

    #[test]
    fn a_team_session_gets_the_notices_next_to_the_gate() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let program = Path::new("claude.exe");
        let launch = launched(
            fixture
                .service
                .prepare(1, &claude(), program, &[], Some("2.1.329")),
        );
        assert_eq!(launch.env[0].0, TOKEN_ENV);
        assert_eq!(launch.env[1].0, NOTIFY_TOKEN_ENV);
        assert_ne!(launch.env[0].1.as_str(), launch.env[1].1.as_str());
        let json: serde_json::Value = serde_json::from_str(&launch.args[1]).unwrap();
        assert!(json["hooks"].get("PreToolUse").is_some());
        assert!(json["hooks"].get("PermissionRequest").is_some());
        let port = fixture.service.port().unwrap();
        let notice = launch.env[1].1.to_string();
        assert_eq!(
            post(port, ROUTE_NOTIFY_ASKS, Some(&notice), "{}"),
            (200, String::new())
        );
        assert_eq!(fixture.service.permission_waiting(), [1]);
        // Without a known version: the gate as before, no notices.
        let older = launched(fixture.service.prepare(2, &claude(), program, &[], None));
        assert_eq!(older.env.len(), 1);
        assert!(!older.args[1].contains("PermissionRequest"));
    }

    #[test]
    fn the_send_route_takes_only_a_send_token() {
        let fixture = fixture(DECISION_TIMEOUT);
        // No gate needed: private sessions send too.
        let (port, send) = fixture.service.send_token(7).unwrap();
        let send = send.to_string();
        let body = r#"{"partner":"web","text":"hi"}"#;
        // Before the app hands the route a service, nothing is taken.
        assert_eq!(post(port, ROUTE_SEND, Some(&send), body).0, 503);
        fixture.service.set_messages(|session, request| {
            (
                Status::Ok,
                format!("{session}:{}:{}", request.partner, request.text),
            )
        });
        assert_eq!(
            post(port, ROUTE_SEND, Some(&send), body),
            (200, "7:web:hi".to_owned())
        );
        assert_eq!(post(port, ROUTE_SEND, None, body).0, 401);
        assert_eq!(post(port, ROUTE_SEND, Some("wrong"), body).0, 401);
        assert_eq!(post(port, ROUTE_SEND, Some(&send), "{").0, 400);
        assert_eq!(
            post(port, ROUTE_SEND, Some(&send), r#"{"partner":"web"}"#).0,
            400
        );
        // A send token opens no hook route, a hook token sends nothing.
        let hook_body = edit(&fixture.root, "src/shared.rs");
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&send), &hook_body).0,
            403
        );
        assert_eq!(
            post(port, ROUTE_USER_PROMPT_SUBMIT, Some(&send), "{}").0,
            403
        );
        fixture.service.configure(true);
        let (_, hooks) = launch(&fixture, 1);
        assert_eq!(post(port, ROUTE_SEND, Some(&hooks), body).0, 403);
        // A browser page is refused even with the token.
        assert_eq!(
            post_with(
                port,
                ROUTE_SEND,
                Some(&send),
                &format!("Host: 127.0.0.1:{port}\r\nOrigin: null\r\n"),
                body
            )
            .0,
            403
        );
        // The size is checked before the body is read.
        let mut stream = connect(port);
        let head = format!(
            "POST {ROUTE_SEND} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             Authorization: Bearer {send}\r\nContent-Length: {}\r\n\r\n",
            MAX_SEND_BODY_BYTES + 1
        );
        stream.write_all(head.as_bytes()).unwrap();
        assert_eq!(read_response(stream).0, 413);
        // The token ends with the session's process.
        fixture.service.revoke_send(7);
        assert_eq!(post(port, ROUTE_SEND, Some(&send), body).0, 401);
        let (_, again) = fixture.service.send_token(7).unwrap();
        fixture.service.ended(7);
        assert_eq!(post(port, ROUTE_SEND, Some(&again), body).0, 401);
    }

    #[test]
    fn browser_requests_and_other_hosts_are_refused_even_with_a_token() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/own.rs");
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        for headers in [
            format!("{host}Origin: http://example.invalid\r\n"),
            format!("{host}Sec-Fetch-Site: cross-site\r\n"),
            format!("Host: localhost:{port}\r\n"),
            "Host: 127.0.0.1\r\n".to_owned(),
            format!("Host: attacker.invalid:{port}\r\n"),
            String::new(),
        ] {
            assert_eq!(
                post_with(port, ROUTE_PRE_TOOL_USE, Some(&token), &headers, &body).0,
                403,
                "{headers}"
            );
        }
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body).0, 200);
    }

    #[test]
    fn raw_garbage_and_oversized_requests_get_errors() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let send = |raw: &[u8]| {
            let mut stream = connect(port);
            stream.write_all(raw).unwrap();
            read_response(stream).0
        };
        assert_eq!(send(b"GET / HTTP/1.1\r\nContent-Length: 0\r\n\r\n"), 405);
        assert_eq!(
            send(b"\x16\x03\x01\x02\x00\x01\x00\x01\xfc\x03\x03\r\n\r\n"),
            400
        );
        let huge = format!(
            "POST {ROUTE_PRE_TOOL_USE} HTTP/1.1\r\nAuthorization: Bearer {token}\r\n\
             Content-Length: 999999999\r\n\r\n"
        );
        assert_eq!(send(huge.as_bytes()), 413);
        let chunked =
            format!("POST {ROUTE_PRE_TOOL_USE} HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert_eq!(send(chunked.as_bytes()), 501);
        let long_head = format!("POST / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(20_000));
        assert_eq!(send(long_head.as_bytes()), 431);
    }

    #[test]
    fn a_head_sent_byte_by_byte_is_cut_off_at_the_total_limit() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, _) = launch(&fixture, 1);
        let stream = connect(port);
        let mut writer = stream.try_clone().unwrap();
        let started = Instant::now();
        // One byte every 100 ms: each read is quick, the whole head never ends.
        let dripping = thread::spawn(move || {
            for byte in b"POST /v1/pre-tool-use HTTP/1.1\r\nX-Slow: "
                .iter()
                .cycle()
                .take(400)
            {
                if writer.write_all(&[*byte]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        });
        let took = closed_after(stream, started);
        let head = limits(DECISION_TIMEOUT).head;
        assert!(took >= head - Duration::from_millis(200), "{took:?}");
        assert!(took < head + Duration::from_secs(3), "{took:?}");
        dripping.join().unwrap();
    }

    #[test]
    fn a_body_sent_byte_by_byte_is_cut_off_at_the_total_limit() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let mut stream = connect(port);
        let head = format!(
            "POST {ROUTE_PRE_TOOL_USE} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             Authorization: Bearer {token}\r\nContent-Length: 100000\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).unwrap();
        let mut writer = stream.try_clone().unwrap();
        let started = Instant::now();
        let dripping = thread::spawn(move || {
            for _ in 0..1000 {
                if writer.write_all(b" ").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        });
        let took = closed_after(stream, started);
        let body = limits(DECISION_TIMEOUT).body;
        assert!(took >= body - Duration::from_millis(300), "{took:?}");
        assert!(took < body + Duration::from_secs(3), "{took:?}");
        dripping.join().unwrap();
    }

    #[test]
    fn idle_connections_do_not_block_a_new_hook() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");
        // A known session's held request keeps its slot whatever unknown clients do.
        let waiting = {
            let token = token.clone();
            let body = body.clone();
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let id = next_pending(&fixture.events).pending[0].id;
        // Silent connections take every slot for unknown clients, and more come after them.
        let mut idle: Vec<TcpStream> = (0..MAX_UNAUTHENTICATED).map(|_| connect(port)).collect();
        let grace = limits(DECISION_TIMEOUT).grace;
        thread::sleep(grace + Duration::from_millis(200));
        let oldest = idle.remove(0);
        let started = Instant::now();
        idle.push(connect(port));
        // The newcomer takes the slot of the oldest, silent past its grace: that one is closed
        // at once, long before the head limit (another second away).
        let took = closed_after(oldest, started);
        assert!(took < Duration::from_millis(600), "{took:?}");
        // A hook sent while silent clients hold every slot is served right away.
        let started = Instant::now();
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&token),
                &edit(&fixture.root, "src/own.rs")
            ),
            (200, String::new())
        );
        assert!(
            started.elapsed() < limits(DECISION_TIMEOUT).head,
            "{:?}",
            started.elapsed()
        );
        // A burst of many more does not lock the endpoint out either.
        let burst: Vec<TcpStream> = (0..3 * MAX_HEAD_THREADS).map(|_| connect(port)).collect();
        thread::sleep(grace + Duration::from_millis(200));
        let started = Instant::now();
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&token),
                &edit(&fixture.root, "src/own.rs")
            ),
            (200, String::new())
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(fixture.service.answer(id, Answer::AllowOnce));
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        drop(idle);
        drop(burst);
    }

    #[test]
    fn one_session_cannot_take_every_slot() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let (_, other) = launch(&fixture, 2);
        let head = format!(
            "POST {ROUTE_PRE_TOOL_USE} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             Authorization: Bearer {token}\r\nContent-Length: 100\r\n\r\n"
        );
        // Session 1 holds as many connections as it may, each waiting for its body.
        let hold = || {
            let mut stream = connect(port);
            stream.write_all(head.as_bytes()).unwrap();
            stream
        };
        // Session 1 has all but one of its connections open, each still sending its body.
        let mut held: Vec<TcpStream> = (0..MAX_PER_SESSION - 1).map(|_| hold()).collect();
        thread::sleep(Duration::from_millis(300));
        // Its shell command in the last free slot is still checked by the guard.
        let risky = bash(&fixture.root, "git push --force");
        let waiting = {
            let (token, risky) = (token.clone(), risky.clone());
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &risky))
        };
        let snapshot = next_pending(&fixture.events);
        assert!(fixture.service.answer(snapshot.pending[0].id, Answer::Deny));
        assert_eq!(decision_of(&waiting.join().unwrap().1).0, "deny");
        // With every slot taken, one more of session 1 is denied unread, never left
        // unanswered: an unanswered hook would let the command run unchecked.
        held.push(hold());
        thread::sleep(Duration::from_millis(300));
        let (code, answer) = post(port, ROUTE_PRE_TOOL_USE, Some(&token), &risky);
        assert_eq!(code, 200);
        assert_eq!(
            decision_of(&answer),
            ("deny".to_owned(), gate::busy_reason())
        );
        // The prompt hook only adds context; it may go without.
        assert_eq!(
            post(port, ROUTE_USER_PROMPT_SUBMIT, Some(&token), "{}").0,
            503
        );
        // Another session is still served.
        let started = Instant::now();
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&other),
                &edit(&fixture.root, "src/own.rs")
            ),
            (200, String::new())
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        // With the gate off the overflow gets "no decision", not an error: Claude Code's hook
        // blocks a call whose hook fails (`onFailure: "block"`).
        fixture.service.configure(false);
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &risky),
            (200, String::new())
        );
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE_JSON, Some(&token), &risky),
            (200, gate::EMPTY_JSON.to_owned())
        );
        fixture.service.configure(true);
        // Once the held connections close, session 1 is checked as before.
        drop(held);
        thread::sleep(Duration::from_millis(300));
        let waiting = thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &risky));
        let snapshot = next_pending(&fixture.events);
        assert!(fixture.service.answer(snapshot.pending[0].id, Answer::Deny));
        assert_eq!(decision_of(&waiting.join().unwrap().1).0, "deny");
    }

    #[test]
    fn a_risky_command_among_a_storm_of_parallel_reads_is_never_waved_through() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let read = serde_json::json!({
            "cwd": fixture.root, "tool_name": "Read",
            "tool_input": { "file_path": fixture.root.join("src").join("shared.rs") },
        })
        .to_string();
        // Far more parallel reads than one session may hold, and a force push among them.
        let reads: Vec<_> = (0..3 * MAX_PER_SESSION)
            .map(|_| {
                let (token, read) = (token.clone(), read.clone());
                thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &read))
            })
            .collect();
        let risky = bash(&fixture.root, "git push --force");
        let waiting = thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &risky));
        // Either the guard holds it for the person, or the session was too busy and it is
        // denied unread; it never comes back as an empty "go ahead".
        let started = Instant::now();
        while !waiting.is_finished() {
            if let Some(pending) = fixture.service.snapshot().pending.first() {
                fixture.service.answer(pending.id, Answer::Deny);
            }
            assert!(started.elapsed() < Duration::from_secs(20));
            thread::sleep(Duration::from_millis(20));
        }
        let (code, answer) = waiting.join().unwrap();
        assert_eq!(code, 200);
        assert_eq!(decision_of(&answer).0, "deny");
        // Every read is answered: let through, or denied while the session was too busy.
        for read in reads {
            let (code, answer) = read.join().unwrap();
            assert_eq!(code, 200);
            if !answer.is_empty() {
                assert_eq!(
                    decision_of(&answer),
                    ("deny".to_owned(), gate::busy_reason())
                );
            }
        }
    }

    #[test]
    fn a_hundred_hooks_of_tools_that_change_nothing_are_answered_quickly() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let tools = [
            (
                "Read",
                serde_json::json!({ "file_path": fixture.root.join("src/shared.rs") }),
            ),
            ("Glob", serde_json::json!({ "pattern": "**/*.rs" })),
            (
                "Grep",
                serde_json::json!({ "pattern": "fn main", "path": fixture.root }),
            ),
            (
                "WebFetch",
                serde_json::json!({ "url": "https://example.com/", "prompt": "x" }),
            ),
        ];
        let started = Instant::now();
        for round in 0..100 {
            let (tool, input) = &tools[round % tools.len()];
            let body = serde_json::json!({
                "cwd": fixture.root, "tool_name": tool, "tool_input": input,
            })
            .to_string();
            assert_eq!(
                post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body),
                (200, String::new()),
                "{tool}"
            );
        }
        // A local round trip each, no disk work and no lock held for long: well under 10 ms
        // per hook even in a debug build on a loaded machine.
        let took = started.elapsed();
        assert!(took < Duration::from_secs(2), "{took:?}");
        assert!(fixture.service.snapshot().pending.is_empty());
    }

    #[test]
    fn an_edit_path_hangar_cannot_read_is_asked_about() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let long = fixture.root.join("a".repeat(gate::MAX_FIELD_BYTES + 1));
        let body = serde_json::json!({
            "cwd": fixture.root, "tool_name": "Write",
            "tool_input": { "file_path": long, "content": "x" },
        })
        .to_string();
        let held = thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body));
        let snapshot = next_pending(&fixture.events);
        let rules = snapshot.pending[0].command.as_ref().unwrap().rules.clone();
        assert_eq!(rules[0].id, RULE_TOO_LONG);
        assert!(fixture.service.answer(snapshot.pending[0].id, Answer::Deny));
        let (code, answer) = held.join().unwrap();
        assert_eq!(code, 200);
        assert_eq!(decision_of(&answer).0, "deny");
    }

    #[test]
    fn every_tool_reaches_the_endpoint_but_only_edits_and_commands_are_checked() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        // A read of a file another session changes is no edit: it goes ahead at once.
        let read = serde_json::json!({
            "cwd": fixture.root, "tool_name": "Read",
            "tool_input": { "file_path": fixture.root.join("src").join("shared.rs") },
        })
        .to_string();
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &read),
            (200, String::new())
        );
        // A tool Hangar does not know by name but with a command is read as a shell command.
        let run = serde_json::json!({
            "cwd": fixture.root, "tool_name": "mcp__box__run",
            "tool_input": { "command": "git push --force" },
        })
        .to_string();
        let held = thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &run));
        let snapshot = next_pending(&fixture.events);
        let rules = snapshot.pending[0].command.as_ref().unwrap().rules.clone();
        assert!(
            rules.iter().any(|rule| rule.id == "git-push-force"),
            "{rules:?}"
        );
        assert!(fixture.service.answer(snapshot.pending[0].id, Answer::Deny));
        assert_eq!(decision_of(&held.join().unwrap().1).0, "deny");
    }

    #[test]
    fn a_command_found_before_the_guard_was_switched_off_opens_no_card() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let _ = launch(&fixture, 1);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let hit = |id: &str| Hit {
            rule_id: id.to_owned(),
            description: "d".into(),
            call: "git push --force".into(),
        };
        // The hits were found while the guard was on; it is switched off before the card opens.
        fixture.service.configure_command_guard(false);
        let outcome = fixture.service.inner.ask_command(
            1,
            "Bash",
            None,
            "git push --force".into(),
            vec![hit("git-push-force")],
            &server,
            true,
        );
        assert_eq!(outcome, Outcome::Allow);
        assert!(fixture.service.snapshot().pending.is_empty());
        drop(client);
    }

    #[test]
    fn an_edit_through_a_link_into_hangars_folder_is_asked_about() {
        let project = TempDir::new("gate-link-project");
        let fixture = fixture_at(project.path().to_path_buf(), DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        // A link inside the project that leads into the gate's own folder.
        let target = fixture.settings.path().join("linked");
        std::fs::create_dir_all(&target).unwrap();
        let link = project.path().join("innocent");
        if !make_dir_link(&link, &target) {
            eprintln!("no directory link could be made; the test is skipped");
            return;
        }
        let body = serde_json::json!({
            "cwd": project.path(), "tool_name": "Write",
            "tool_input": { "file_path": link.join("hooks.json"), "content": "x" },
        })
        .to_string();
        let held = thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body));
        let snapshot = next_pending(&fixture.events);
        let rules = snapshot.pending[0].command.as_ref().unwrap().rules.clone();
        assert_eq!(rules[0].id, RULE_HANGAR_CONFIG);
        assert!(fixture.service.answer(snapshot.pending[0].id, Answer::Deny));
        assert_eq!(decision_of(&held.join().unwrap().1).0, "deny");
        // The same file name in an ordinary folder of the project goes ahead.
        let (_, token) = launch(&fixture, 1);
        std::fs::create_dir_all(project.path().join("plain")).unwrap();
        let plain = serde_json::json!({
            "cwd": project.path(), "tool_name": "Write",
            "tool_input": { "file_path": project.path().join("plain").join("hooks.json") },
        })
        .to_string();
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &plain),
            (200, String::new())
        );
        let _ = std::fs::remove_dir(&link);
    }

    /// A directory junction (Windows, no privilege needed) or symlink at `link` to `target`.
    fn make_dir_link(link: &Path, target: &Path) -> bool {
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|output| output.status.success())
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
    }

    #[test]
    fn edits_without_overlap_pass_at_once() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let (code, body) = post(
            port,
            ROUTE_PRE_TOOL_USE,
            Some(&token),
            &edit(&fixture.root, "src/own.rs"),
        );
        assert_eq!((code, body.as_str()), (200, ""));
        // Outside every worktree.
        let outside = edit(Path::new("/nowhere"), "x.rs");
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &outside),
            (200, String::new())
        );
    }

    #[test]
    fn an_overlapping_edit_waits_for_the_answer() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");

        // Deny: the reason names the other session and the file.
        let waiting = {
            let token = token.clone();
            let body = body.clone();
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let snapshot = next_pending(&fixture.events);
        let decision = &snapshot.pending[0];
        assert_eq!(decision.session_id, 1);
        assert_eq!(decision.path, "src/shared.rs");
        assert_eq!(decision.other_sessions, [2]);
        assert_eq!(decision.lines, [LineRange { start: 10, end: 12 }]);
        assert!(fixture.service.answer(decision.id, Answer::Deny));
        assert!(!fixture.service.answer(decision.id, Answer::Deny));
        let (code, answer) = waiting.join().unwrap();
        assert_eq!(code, 200);
        let json: serde_json::Value = serde_json::from_str(&answer).unwrap();
        let output = &json["hookSpecificOutput"];
        assert_eq!(output["permissionDecision"], "deny");
        let reason = output["permissionDecisionReason"].as_str().unwrap();
        assert!(
            reason.contains("[\"Codex · feature\"]") && reason.contains("\"src/shared.rs\""),
            "{reason}"
        );

        // Allow for this file: this time and every later time without asking.
        let waiting = {
            let token = token.clone();
            let body = body.clone();
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let id = next_pending(&fixture.events).pending[0].id;
        assert!(fixture.service.answer(id, Answer::AllowFile));
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body),
            (200, String::new())
        );
    }

    #[test]
    fn gate_rules_answer_before_the_person_is_asked() {
        let fixture = fixture(DECISION_TIMEOUT);
        let file = fixture
            .settings
            .path()
            .join("rules")
            .join("gate-rules.json");
        fixture.service.load_rules(file.clone());
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");

        // "Always for this folder": this edit goes ahead and the rule is saved for the project.
        let waiting = {
            let token = token.clone();
            let body = body.clone();
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let id = next_pending(&fixture.events).pending[0].id;
        assert!(fixture.service.answer(id, Answer::AlwaysFolder));
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        let rules = fixture.service.rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            (rules[0].project.as_str(), rules[0].path.as_str()),
            ("repo", "src")
        );
        assert_eq!(
            (rules[0].scope, rules[0].effect),
            (RuleScope::Folder, RuleEffect::Allow)
        );
        let saved = RuleBook::parse(&std::fs::read(&file).unwrap());
        assert_eq!(saved.rules(), rules.as_slice());

        // The next ask is answered by the rule, without a card, and logged.
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body),
            (200, String::new())
        );
        let snapshot = fixture.service.snapshot();
        assert!(snapshot.pending.is_empty());
        assert_eq!(snapshot.log.len(), 1);
        let entry = &snapshot.log[0];
        assert_eq!(
            (entry.session_id, entry.path.as_str(), entry.rule_id),
            (1, "src/shared.rs", rules[0].id)
        );
        assert_eq!(entry.effect, RuleEffect::Allow);

        // A deny rule for the file wins over the folder's allow rule.
        let deny = {
            let mut book = lock(&fixture.service.inner.rules);
            book.book
                .add(
                    "repo",
                    "src/shared.rs",
                    RuleScope::File,
                    RuleEffect::Deny,
                    1,
                )
                .unwrap()
        };
        let (code, answer) = post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body);
        assert_eq!(code, 200);
        let json: serde_json::Value = serde_json::from_str(&answer).unwrap();
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        let reason = json["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap();
        assert!(reason.contains("standing rule"), "{reason}");
        let log = fixture.service.snapshot().log;
        assert_eq!((log.len(), log[0].rule_id), (2, deny.id));
        assert!(log[0].id > log[1].id, "newest first");

        // Deleted, the folder's allow rule answers again.
        assert!(fixture.service.delete_rule(deny.id));
        assert!(!fixture.service.delete_rule(deny.id));
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body),
            (200, String::new())
        );
        assert_eq!(
            RuleBook::parse(&std::fs::read(&file).unwrap())
                .rules()
                .len(),
            1
        );
    }

    fn bash(root: &Path, command: &str) -> String {
        serde_json::json!({
            "session_id": "x", "cwd": root, "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": command, "description": "run it" },
        })
        .to_string()
    }

    fn decision_of(answer: &str) -> (String, String) {
        let json: serde_json::Value = serde_json::from_str(answer).unwrap();
        let output = &json["hookSpecificOutput"];
        (
            output["permissionDecision"].as_str().unwrap().to_owned(),
            output["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    }

    #[test]
    fn a_risky_command_waits_for_the_person_and_harmless_ones_pass() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        // Harmless commands, and ones the rules allow inside the project, go ahead at once.
        for command in [
            "git push",
            "rm -rf ./node_modules",
            "git reset --soft HEAD~1",
            "ls",
        ] {
            assert_eq!(
                post(
                    port,
                    ROUTE_PRE_TOOL_USE,
                    Some(&token),
                    &bash(&fixture.root, command)
                ),
                (200, String::new()),
                "{command}"
            );
        }
        // A force push waits; the card names the rule and shows the command as sent.
        let command = "git push --force origin main\u{1b}[2J";
        let waiting = {
            let (token, body) = (token.clone(), bash(&fixture.root, command));
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let decision = next_pending(&fixture.events).pending.remove(0);
        let ask = decision.command.clone().unwrap();
        assert_eq!(ask.rules.len(), 1);
        assert_eq!(ask.rules[0].id, "git-push-force");
        assert!(ask.rules[0].builtin);
        assert_eq!(ask.call, "git push --force origin main\u{1b}[2J");
        assert_eq!(ask.text, command);
        assert!(decision.path.is_empty() && decision.other_sessions.is_empty());
        // A file answer does not fit a command; the card stays.
        assert!(!fixture.service.answer(decision.id, Answer::AllowFile));
        assert_eq!(fixture.service.snapshot().pending.len(), 1);
        assert!(fixture.service.answer(decision.id, Answer::Deny));
        let (code, answer) = waiting.join().unwrap();
        assert_eq!(code, 200);
        let (verdict, reason) = decision_of(&answer);
        assert_eq!(verdict, "deny");
        assert!(reason.contains("\"git-push-force\""), "{reason}");
        assert!(reason.contains("did not allow"), "{reason}");
        // Switched off in Settings, the same command goes ahead without a card.
        fixture.service.configure_command_guard(false);
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&token),
                &bash(&fixture.root, command)
            ),
            (200, String::new())
        );
        assert!(!fixture.service.command_guard_info().enabled);
    }

    #[test]
    fn a_remembered_answer_covers_the_rule_in_the_project() {
        let fixture = fixture(DECISION_TIMEOUT);
        let file = fixture.settings.path().join("gate-rules.json");
        fixture.service.load_rules(file.clone());
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let first = bash(&fixture.root, "git reset --hard");
        let waiting = {
            let (token, body) = (token.clone(), first.clone());
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let id = next_pending(&fixture.events).pending[0].id;
        assert!(fixture.service.answer(id, Answer::AlwaysCommand));
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        let rules = fixture.service.rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            (
                rules[0].project.as_str(),
                rules[0].path.as_str(),
                rules[0].scope
            ),
            ("repo", "git-reset-hard", RuleScope::Command)
        );
        assert_eq!(
            RuleBook::parse(&std::fs::read(&file).unwrap()).rules(),
            rules.as_slice()
        );
        // Another command matching the same rule: allowed by the rule, logged, no card.
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&token),
                &bash(&fixture.root, "git -C . reset --hard HEAD~2")
            ),
            (200, String::new())
        );
        let snapshot = fixture.service.snapshot();
        assert!(snapshot.pending.is_empty());
        assert_eq!(snapshot.log[0].path, "git -C . reset --hard HEAD~2");
        assert_eq!(snapshot.log[0].rule_path, "git-reset-hard");
        // A command rule never answers an edit, and an edit rule never a command.
        assert!(lock(&fixture.service.inner.rules)
            .book
            .find("repo", "git-reset-hard")
            .is_none());
        // "Always block" turns the same rule into a deny.
        {
            let mut rules = lock(&fixture.service.inner.rules);
            rules
                .book
                .add(
                    "repo",
                    "git-reset-hard",
                    RuleScope::Command,
                    RuleEffect::Deny,
                    1,
                )
                .unwrap();
        }
        let (code, answer) = post(port, ROUTE_PRE_TOOL_USE, Some(&token), &first);
        assert_eq!(code, 200);
        let (verdict, reason) = decision_of(&answer);
        assert_eq!(verdict, "deny");
        assert!(reason.contains("standing answer"), "{reason}");
    }

    #[test]
    fn a_command_nobody_answers_is_blocked() {
        let fixture = fixture(Duration::from_millis(300));
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let (code, answer) = post(
            port,
            ROUTE_PRE_TOOL_USE,
            Some(&token),
            &bash(&fixture.root, "rm -rf ~"),
        );
        assert_eq!(code, 200);
        let (verdict, reason) = decision_of(&answer);
        assert_eq!(verdict, "deny");
        assert!(reason.contains("no answer"), "{reason}");
        assert!(reason.contains("rm-outside-project"), "{reason}");
    }

    #[test]
    fn switching_the_guard_off_lets_a_held_command_go_on() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let waiting = {
            let (token, body) = (token.clone(), bash(&fixture.root, "diskpart"));
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        next_pending(&fixture.events);
        fixture.service.configure_command_guard(false);
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        assert!(fixture.service.snapshot().pending.is_empty());
    }

    #[test]
    fn an_overlong_command_is_asked_about_not_handed_on() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let long = "x".repeat(command_guard::MAX_COMMAND_BYTES + 1);
        let waiting = {
            let (token, body) = (token.clone(), bash(&fixture.root, &long));
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let decision = next_pending(&fixture.events).pending.remove(0);
        let ask = decision.command.unwrap();
        assert_eq!(ask.rules[0].id, command_guard::RULE_TOO_LONG);
        assert_eq!(ask.text.chars().count(), gate::MAX_SHOWN_COMMAND);
        assert!(fixture.service.answer(decision.id, Answer::Deny));
        let (code, answer) = waiting.join().unwrap();
        assert_eq!((code, decision_of(&answer).0.as_str()), (200, "deny"));
        // An edit tool keeps its `command` field to itself (Codex's patch text); any other tool
        // with a command is read as a shell command
        // (`every_tool_reaches_the_endpoint_but_only_edits_and_commands_are_checked`).
        let other = serde_json::json!({
            "tool_name": "apply_patch", "cwd": fixture.root,
            "tool_input": { "command": "git push --force" },
        })
        .to_string();
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &other),
            (200, String::new())
        );
    }

    #[test]
    fn a_working_folder_hangar_cannot_use_still_asks() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        for cwd in [
            "a".repeat(gate::MAX_FIELD_BYTES + 1),
            "C:/x\u{0}y".to_owned(),
        ] {
            let body = serde_json::json!({
                "tool_name": "Bash", "cwd": cwd,
                "tool_input": { "command": "rm -rf build" },
            })
            .to_string();
            let waiting = {
                let token = token.clone();
                thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
            };
            let decision = next_pending(&fixture.events).pending.remove(0);
            assert!(fixture.service.answer(decision.id, Answer::AllowOnce));
            assert_eq!(waiting.join().unwrap(), (200, String::new()));
        }
    }

    #[test]
    fn an_edit_with_a_working_folder_hangar_cannot_use_is_still_checked() {
        let fixture = fixture(DECISION_TIMEOUT);
        let file = fixture
            .settings
            .path()
            .join("com.example.hangar")
            .join(command_guard::FILE_NAME);
        fixture.service.load_command_guard(file.clone());
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let long = "a".repeat(gate::MAX_FIELD_BYTES + 1);
        // An ordinary edit goes ahead instead of being refused.
        let body = serde_json::json!({
            "tool_name": "Edit", "cwd": long,
            "tool_input": { "file_path": fixture.root.join("src").join("own.rs") },
        })
        .to_string();
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body),
            (200, String::new())
        );
        // An edit of the rules file is still held.
        let body = serde_json::json!({
            "tool_name": "Write", "cwd": long,
            "tool_input": { "file_path": file, "content": "" },
        })
        .to_string();
        let waiting = {
            let token = token.clone();
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let decision = next_pending(&fixture.events).pending.remove(0);
        assert_eq!(
            decision.command.unwrap().rules[0].id,
            command_guard::RULE_HANGAR_CONFIG
        );
        assert!(fixture.service.answer(decision.id, Answer::Deny));
        assert_eq!(decision_of(&waiting.join().unwrap().1).0, "deny");
    }

    #[test]
    fn hangars_worktrees_in_its_data_folder_are_not_its_settings() {
        let fixture = fixture(DECISION_TIMEOUT);
        // Next to the gate's folder, as the app has it (the fixture's gate folder is its root).
        let data_dir = TempDir::new("gate-data-folder");
        let data = data_dir.path().to_path_buf();
        fixture.service.load_command_guard(
            fixture
                .settings
                .path()
                .join("config")
                .join(command_guard::FILE_NAME),
        );
        fixture.service.load_rules(data.join(gate_rules::FILE_NAME));
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        // The session's own folder holds nothing of Hangar's; an edit and a command pass.
        let edit = serde_json::json!({
            "tool_name": "Edit", "cwd": fixture.root,
            "tool_input": { "file_path": fixture.root.join("src").join("main.rs") },
        })
        .to_string();
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &edit),
            (200, String::new())
        );
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&token),
                &bash(&fixture.root, "git status")
            ),
            (200, String::new())
        );
        // The remembered answers' file is protected, the data folder around it is not.
        let slashed = |path: &Path| path.display().to_string().replace('\\', "/");
        let protected = format!("rm {}", slashed(&data.join(gate_rules::FILE_NAME)));
        let waiting = {
            let (token, body) = (token.clone(), bash(&fixture.root, &protected));
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let decision = next_pending(&fixture.events).pending.remove(0);
        assert_eq!(
            decision.command.unwrap().rules[0].id,
            command_guard::RULE_HANGAR_CONFIG
        );
        assert!(fixture.service.answer(decision.id, Answer::AllowOnce));
        waiting.join().unwrap();
        let elsewhere = format!("ls {}", slashed(&data.join("notes")));
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&token),
                &bash(&fixture.root, &elsewhere)
            ),
            (200, String::new())
        );
    }

    #[test]
    fn every_rule_of_a_command_counts_and_only_the_unanswered_ones_are_shown() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        lock(&fixture.service.inner.rules)
            .book
            .add(
                "repo",
                "git-reset-hard",
                RuleScope::Command,
                RuleEffect::Allow,
                1,
            )
            .unwrap();
        // The remembered allow for the reset does not carry the delete of the home folder.
        let chained = bash(&fixture.root, "git reset --hard && rm -rf ~");
        let waiting = {
            let (token, body) = (token.clone(), chained.clone());
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let decision = next_pending(&fixture.events).pending.remove(0);
        let ask = decision.command.unwrap();
        let ids: Vec<&str> = ask.rules.iter().map(|rule| rule.id.as_str()).collect();
        assert_eq!(ids, ["rm-outside-project"]);
        assert_eq!(ask.call, "rm -rf ~");
        // "Always allow" remembers the rules shown.
        assert!(fixture.service.answer(decision.id, Answer::AlwaysCommand));
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        let remembered: Vec<String> = fixture
            .service
            .rules()
            .into_iter()
            .map(|rule| rule.path)
            .collect();
        assert_eq!(remembered, ["git-reset-hard", "rm-outside-project"]);
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), &chained),
            (200, String::new())
        );
        // A remembered deny for any rule blocks the whole command.
        lock(&fixture.service.inner.rules)
            .book
            .add(
                "repo",
                "rm-outside-project",
                RuleScope::Command,
                RuleEffect::Deny,
                2,
            )
            .unwrap();
        let (code, answer) = post(port, ROUTE_PRE_TOOL_USE, Some(&token), &chained);
        assert_eq!((code, decision_of(&answer).0.as_str()), (200, "deny"));
    }

    #[test]
    fn a_full_queue_blocks_a_command_instead_of_handing_it_on() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let mut held = Vec::new();
        for n in 0..gate::MAX_PENDING_PER_SESSION {
            let body = bash(&fixture.root, &format!("rm -rf /tmp/x{n}"));
            let token = token.clone();
            held.push(thread::spawn(move || {
                post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body)
            }));
            loop {
                if next_pending(&fixture.events).pending.len() == n + 1 {
                    break;
                }
            }
        }
        let (code, answer) = post(
            port,
            ROUTE_PRE_TOOL_USE,
            Some(&token),
            &bash(&fixture.root, "rm -rf /tmp/more"),
        );
        let (verdict, reason) = decision_of(&answer);
        assert_eq!((code, verdict.as_str()), (200, "deny"));
        assert!(reason.contains("too many decisions"), "{reason}");
        fixture.service.ended(1);
        for waiting in held {
            assert_eq!(decision_of(&waiting.join().unwrap().1).0, "deny");
        }
    }

    #[test]
    fn hangars_own_settings_are_asked_about_even_with_the_guard_off() {
        let fixture = fixture(DECISION_TIMEOUT);
        let config = fixture.settings.path().join("com.example.hangar");
        let file = config.join(command_guard::FILE_NAME);
        fixture.service.load_command_guard(file.clone());
        fixture.service.configure(true);
        fixture.service.configure_command_guard(false);
        let (port, token) = launch(&fixture, 1);
        // An edit of the rules file.
        let body = serde_json::json!({
            "tool_name": "Write", "cwd": fixture.root,
            "tool_input": { "file_path": file, "content": "" },
        })
        .to_string();
        let waiting = {
            let token = token.clone();
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let decision = next_pending(&fixture.events).pending.remove(0);
        let ask = decision.command.unwrap();
        assert_eq!(ask.rules[0].id, command_guard::RULE_HANGAR_CONFIG);
        assert!(!ask.rememberable);
        // It cannot be allowed for good, and switching the guard off does not release it.
        assert!(!fixture.service.answer(decision.id, Answer::AlwaysCommand));
        fixture.service.configure_command_guard(true);
        fixture.service.configure_command_guard(false);
        assert_eq!(fixture.service.snapshot().pending.len(), 1);
        assert!(fixture.service.answer(decision.id, Answer::Deny));
        assert_eq!(decision_of(&waiting.join().unwrap().1).0, "deny");
        assert!(fixture.service.rules().is_empty());
        // A command that names the folder.
        // Forward slashes: in a POSIX command a backslash escapes.
        let command = format!(
            "cp x {}",
            config
                .join("profiles.toml")
                .display()
                .to_string()
                .replace('\\', "/")
        );
        let waiting = {
            let (token, body) = (token.clone(), bash(&fixture.root, &command));
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let decision = next_pending(&fixture.events).pending.remove(0);
        assert_eq!(
            decision.command.unwrap().rules[0].id,
            command_guard::RULE_HANGAR_CONFIG
        );
        assert!(fixture.service.answer(decision.id, Answer::AllowOnce));
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        // Other commands pass while the guard is off.
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE,
                Some(&token),
                &bash(&fixture.root, "git push -f")
            ),
            (200, String::new())
        );
    }

    #[test]
    fn the_rules_file_is_written_once_and_read_again_when_it_changes() {
        let fixture = fixture(DECISION_TIMEOUT);
        let file = fixture
            .settings
            .path()
            .join("config")
            .join(command_guard::FILE_NAME);
        fixture.service.load_command_guard(file.clone());
        let info = fixture.service.command_guard_info();
        assert!(info.enabled);
        assert_eq!(info.file, file.to_string_lossy());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            command_guard::DEFAULT_RULES
        );
        assert_eq!(info.rules.len(), command_guard::default_rules().rules.len());
        assert!(info.problems.is_empty());
        // The person's edit applies to the next command, with its problems reported.
        std::fs::write(
            &file,
            "[[rule]]\nid = 'only'\ndescription = 'any ls'\nprogram = ['ls']\n\n\
             [[rule]]\nid = 'broken'\ndescription = 'x'\nprogram = ['x']\nanny = ['-f']\n",
        )
        .unwrap();
        let info = fixture.service.command_guard_info();
        let ids: Vec<&str> = info.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["only"]);
        assert_eq!(info.problems.len(), 1);
        assert_eq!(info.problems[0].id.as_deref(), Some("broken"));
        // A file that is no TOML keeps the defaults and says why.
        std::fs::write(&file, "[[rule]\n").unwrap();
        let info = fixture.service.command_guard_info();
        assert!(info.defaults);
        assert_eq!(info.rules.len(), command_guard::default_rules().rules.len());
        // An oversized file is reported, and the defaults apply.
        std::fs::write(&file, "#".repeat(command_guard::MAX_FILE_BYTES + 1)).unwrap();
        let info = fixture.service.command_guard_info();
        assert!(info.defaults);
        assert!(
            info.problems[0].message.contains("1 MiB"),
            "{:?}",
            info.problems
        );
        assert_eq!(info.default_count, info.rules.len());
        // Deleted, it is not written again in the same run; the defaults apply.
        std::fs::remove_file(&file).unwrap();
        let info = fixture.service.command_guard_info();
        assert!(!file.exists());
        assert_eq!(info.rules.len(), command_guard::default_rules().rules.len());
    }

    #[test]
    fn never_for_this_file_denies_now_and_later() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");
        let waiting = {
            let token = token.clone();
            let body = body.clone();
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };
        let id = next_pending(&fixture.events).pending[0].id;
        assert!(fixture.service.answer(id, Answer::NeverFile));
        let denied = |answer: &str| {
            let json: serde_json::Value = serde_json::from_str(answer).unwrap();
            json["hookSpecificOutput"]["permissionDecision"] == "deny"
        };
        assert!(denied(&waiting.join().unwrap().1));
        assert!(denied(
            &post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body).1
        ));
        let rules = fixture.service.rules();
        assert_eq!(
            (rules[0].path.as_str(), rules[0].scope),
            ("src/shared.rs", RuleScope::File)
        );
    }

    #[test]
    fn a_full_queue_hands_the_question_to_the_agents_own_prompt() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");
        // The same file asked again joins the first card, up to a few requests.
        let held: Vec<_> = (0..gate::MAX_WAITERS)
            .map(|_| {
                let token = token.clone();
                let body = body.clone();
                let request =
                    thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body));
                next_pending(&fixture.events);
                request
            })
            .collect();
        assert_eq!(fixture.service.snapshot().pending.len(), 1);
        let (code, answer) = post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body);
        assert_eq!(code, 200);
        let json: serde_json::Value = serde_json::from_str(&answer).unwrap();
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "ask");
        let id = fixture.service.snapshot().pending[0].id;
        assert!(fixture.service.answer(id, Answer::AllowOnce));
        for request in held {
            assert_eq!(request.join().unwrap(), (200, String::new()));
        }
    }

    #[test]
    fn no_answer_in_time_denies() {
        let fixture = fixture(Duration::from_millis(300));
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let (code, answer) = post(
            port,
            ROUTE_PRE_TOOL_USE,
            Some(&token),
            &edit(&fixture.root, "src/shared.rs"),
        );
        assert_eq!(code, 200);
        assert!(answer.contains("no answer"), "{answer}");
        assert!(fixture.service.snapshot().pending.is_empty());
    }

    #[test]
    fn session_end_quit_and_switching_off_release_waiting_edits() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");
        let start = |token: String, body: String| {
            thread::spawn(move || post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body))
        };

        let waiting = start(token.clone(), body.clone());
        next_pending(&fixture.events);
        fixture.service.configure(false);
        assert_eq!(waiting.join().unwrap(), (200, String::new()));

        fixture.service.configure(true);
        let waiting = start(token.clone(), body.clone());
        next_pending(&fixture.events);
        fixture.service.ended(1);
        let (_, answer) = waiting.join().unwrap();
        assert!(
            answer.contains("\"deny\"") && answer.contains("ended"),
            "{answer}"
        );

        let (_, token) = launch(&fixture, 1);
        let waiting = start(token, body);
        next_pending(&fixture.events);
        fixture.service.stop();
        let (_, answer) = waiting.join().unwrap();
        assert!(answer.contains("\"deny\""), "{answer}");
    }

    #[test]
    fn an_agent_that_stops_waiting_takes_its_card_along() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let body = edit(&fixture.root, "src/shared.rs");
        let mut stream = connect(port);
        let request = format!(
            "POST {ROUTE_PRE_TOOL_USE} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             Authorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        next_pending(&fixture.events);
        drop(stream);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !fixture.service.snapshot().pending.is_empty() {
            assert!(Instant::now() < deadline, "card stayed");
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn prompts_get_the_facts_once_per_conversation() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let prompt = r#"{"session_id":"c1","hook_event_name":"UserPromptSubmit","prompt":"hi"}"#;
        let (code, body) = post(port, ROUTE_USER_PROMPT_SUBMIT, Some(&token), prompt);
        assert_eq!(code, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let context = json["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(
            context.contains("\"src/shared.rs\"") && context.contains("\"Codex · feature\""),
            "{context}"
        );
        assert_eq!(
            post(port, ROUTE_USER_PROMPT_SUBMIT, Some(&token), prompt),
            (200, String::new())
        );
        // A new conversation in the same session (such as after /clear) hears them again.
        let cleared = prompt.replace("c1", "c2");
        let (_, again) = post(port, ROUTE_USER_PROMPT_SUBMIT, Some(&token), &cleared);
        assert!(again.contains("additionalContext"), "{again}");
    }

    #[test]
    fn relative_and_resolved_paths() {
        let cwd = std::env::current_dir().unwrap();
        assert!(path_candidates("a.rs", None, &cwd).is_empty());
        let keys = path_candidates("src/gate.rs", cwd.to_str(), &cwd);
        assert_eq!(
            keys[0],
            crate::git::path_key(&cwd.join("src").join("gate.rs"))
        );
        // A file that does not exist yet resolves through its folder.
        let missing = cwd.join("src").join("not-there-yet.rs");
        let keys = path_candidates(missing.to_str().unwrap(), None, &cwd);
        assert!(
            keys.iter().any(|k| k.ends_with("/src/not-there-yet.rs")),
            "{keys:?}"
        );
    }

    #[test]
    fn network_and_device_paths_are_never_touched() {
        if !cfg!(windows) {
            return;
        }
        let touched = Mutex::new(Vec::<PathBuf>::new());
        let resolve = |path: &Path| {
            lock(&touched).push(path.to_path_buf());
            None
        };
        let local_root = Path::new(r"C:\repo");
        for path in [
            r"\\attacker.invalid\share\a.rs",
            "//attacker.invalid/share/a.rs",
            r"\\?\UNC\attacker.invalid\share\a.rs",
            r"\\.\pipe\hangar-test",
            r"\\?\GLOBALROOT\Device\Mup\attacker.invalid\share\a.rs",
            r"C:\repo\CON",
            r"C:\repo\nul.txt",
            r"C:\repo\src\COM1.rs",
            r"C:\repo\LPT¹",
        ] {
            let keys = candidates_with(path, None, local_root, resolve);
            assert_eq!(keys.len(), 1, "{path}: {keys:?}");
        }
        // A relative path against a share as the working folder stays untouched too.
        candidates_with(
            "a.rs",
            Some(r"\\attacker.invalid\share"),
            local_root,
            resolve,
        );
        assert!(lock(&touched).is_empty(), "{:?}", lock(&touched));

        // A local path, or the share the session's own worktree lives on, is resolved.
        candidates_with(r"C:\repo\src\a.rs", None, local_root, resolve);
        let share_root = Path::new("//fileserver/projects/repo");
        candidates_with(
            r"\\FileServer\Projects\repo\a.rs",
            None,
            share_root,
            resolve,
        );
        candidates_with(r"\\other\projects\a.rs", None, share_root, resolve);
        assert_eq!(lock(&touched).len(), 2, "{:?}", lock(&touched));
    }

    #[test]
    fn device_names_are_recognized() {
        for name in [
            "CON", "con.txt", "Nul", "COM1", "lpt9.log", "AUX ", "CONIN$", "COM²",
        ] {
            assert!(is_device_name(OsStr::new(name)), "{name}");
        }
        for name in ["console", "COM10", "LPT", "nullable.rs", "a.con", "src"] {
            assert!(!is_device_name(OsStr::new(name)), "{name}");
        }
    }

    /// A catalog program with the gate in `format`.
    fn agent_in(format: &str, context: bool) -> Agent {
        let toml = format!(
            "[[agent]]\nid='x'\nname='X'\ncommands=['x']\ncapabilities = {{ pre_edit_block = true, \
             per_session_config = true, context_inject = {context}, hook_format = '{format}', measured = true }}\n"
        );
        Agent::from_catalog(crate::agent_catalog::parse(&toml).unwrap().remove(0))
    }

    fn helper_exe(spaced: bool) -> PathBuf {
        PathBuf::from(match (cfg!(windows), spaced) {
            (true, true) => r"C:\Program Files\Hangar\hangar.exe",
            (true, false) => r"C:\Users\Ann\AppData\Local\Hangar\hangar.exe",
            (false, true) => "/opt/Hangar App/hangar",
            (false, false) => "/opt/hangar/hangar",
        })
    }

    fn env_of<'a>(launch: &'a GateLaunch, name: &str) -> Option<&'a str> {
        launch
            .env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The session's token, from whichever variable carries it.
    fn token_of(launch: &GateLaunch) -> String {
        env_of(launch, TOKEN_ENV)
            .map(str::to_owned)
            .or_else(|| {
                env_of(launch, HELPER_ENV)
                    .and_then(|v| v.split_once(':'))
                    .map(|(_, token)| token.to_owned())
            })
            .unwrap()
    }

    /// Every file below `dir`, read as text.
    fn files_below(dir: &Path) -> Vec<(PathBuf, String)> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(files_below(&path));
            } else {
                found.push((path.clone(), std::fs::read_to_string(&path).unwrap()));
            }
        }
        found
    }

    /// Neither a file nor an argument holds the token.
    fn assert_token_only_in_env(fixture: &Fixture, launch: &GateLaunch) {
        let token = token_of(launch);
        assert_eq!(token.len(), 64);
        for arg in launch.prepend.iter().chain(&launch.args) {
            assert!(!arg.contains(&token), "token in argument {arg}");
        }
        for (path, text) in files_below(fixture.settings.path()) {
            assert!(!text.contains(&token), "token in {path:?}");
        }
    }

    fn wait_until_gone(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while path.exists() {
            assert!(Instant::now() < deadline, "{path:?} stayed");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn qwen_gets_a_settings_file_and_the_token_for_its_header() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let launch = launched(fixture.service.prepare(
            7,
            &agent_in("qwen_system_settings", true),
            Path::new("qwen.exe"),
            &[],
            None,
        ));
        let port = fixture.service.port().unwrap();
        assert!(launch.prepend.is_empty() && launch.args.is_empty());
        let file = PathBuf::from(env_of(&launch, recipes::QWEN_SETTINGS_ENV).unwrap());
        assert!(
            file.starts_with(fixture.settings.path().join(SESSIONS_DIR)),
            "{file:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            recipes::qwen_settings(port, true)
        );
        assert!(env_of(&launch, HELPER_ENV).is_none());
        assert_token_only_in_env(&fixture, &launch);
        // The process ends: its folder goes, its token stops working.
        let token = token_of(&launch);
        fixture.service.ended(7);
        wait_until_gone(file.parent().unwrap());
        let body = edit(&fixture.root, "src/own.rs");
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE_JSON, Some(&token), &body).0,
            401
        );
    }

    #[test]
    fn gemini_gets_a_settings_file_that_runs_the_helper() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        fixture.service.set_helper(helper_exe(true));
        let launch = launched(fixture.service.prepare(
            3,
            &agent_in("gemini_system_settings", false),
            Path::new("gemini.exe"),
            &[],
            None,
        ));
        let file = PathBuf::from(env_of(&launch, recipes::GEMINI_SETTINGS_ENV).unwrap());
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let command = settings["hooks"]["BeforeTool"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            command.contains(helper_exe(true).to_str().unwrap()),
            "{command}"
        );
        assert!(
            command.ends_with("gate-hook gemini before-tool"),
            "{command}"
        );
        assert_eq!(env_of(&launch, recipes::GEMINI_TRUST_ENV), Some("true"));
        // The helper gets port and token under the neutral name only.
        let port = fixture.service.port().unwrap();
        let gate = env_of(&launch, HELPER_ENV).unwrap();
        assert!(gate.starts_with(&format!("{port}:")), "{gate}");
        assert!(env_of(&launch, TOKEN_ENV).is_none());
        assert_token_only_in_env(&fixture, &launch);
        fixture.service.ended(3);
        wait_until_gone(file.parent().unwrap());
    }

    #[test]
    fn copilot_gets_a_plugin_folder_before_the_users_arguments() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        fixture.service.set_helper(helper_exe(true));
        let launch = launched(fixture.service.prepare(
            4,
            &agent_in("copilot_plugin", false),
            Path::new("copilot.exe"),
            &["--continue".to_owned()],
            None,
        ));
        assert_eq!(launch.prepend[0], "--plugin-dir");
        let plugin = PathBuf::from(&launch.prepend[1]);
        assert_eq!(launch.prepend.len(), 2);
        assert!(launch.args.is_empty());
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(plugin.join("plugin.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["hooks"], "hooks.json");
        let hooks: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(plugin.join("hooks.json")).unwrap())
                .unwrap();
        assert_eq!(
            hooks["hooks"]["preToolUse"][0]["exec"],
            helper_exe(true).to_str().unwrap()
        );
        assert!(env_of(&launch, HELPER_ENV).is_some());
        assert_token_only_in_env(&fixture, &launch);
        fixture.service.ended(4);
        wait_until_gone(&plugin);
    }

    #[test]
    fn codex_gets_its_hook_layer_as_arguments_only() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        fixture.service.set_helper(helper_exe(true));
        let codex = agent_in("codex_config_flags", false);
        let launch = launched(fixture.service.prepare(
            5,
            &codex,
            Path::new("codex.exe"),
            &["resume".to_owned(), "--last".to_owned()],
            None,
        ));
        assert_eq!(launch.prepend.len(), 3);
        assert_eq!(launch.prepend[0], recipes::CODEX_TRUST_FLAG);
        assert_eq!(launch.prepend[1], "-c");
        assert!(launch.prepend[2].starts_with("hooks.PreToolUse=["));
        assert!(
            launch.prepend[2].contains(helper_exe(true).to_str().unwrap()),
            "{}",
            launch.prepend[2]
        );
        assert!(launch.args.is_empty());
        assert_token_only_in_env(&fixture, &launch);
        // No files: nothing to clean up.
        assert!(
            !fixture.settings.path().join(SESSIONS_DIR).exists()
                || files_below(&fixture.settings.path().join(SESSIONS_DIR)).is_empty()
        );
        // The user's own bypass flag is not repeated.
        let own = launched(fixture.service.prepare(
            6,
            &codex,
            Path::new("codex.exe"),
            &[recipes::CODEX_TRUST_FLAG.to_owned()],
            None,
        ));
        assert_eq!(own.prepend[0], "-c");
    }

    #[test]
    fn a_cmd_launcher_takes_codexs_layer_only_with_a_bare_path() {
        if !cfg!(windows) {
            return;
        }
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let codex = agent_in("codex_config_flags", false);
        let shim = Path::new(r"C:\npm\codex.cmd");
        fixture.service.set_helper(helper_exe(true));
        assert!(matches!(
            fixture.service.prepare(1, &codex, shim, &[], None),
            GatePlan::Unavailable(GateStatus::InstallPath)
        ));
        fixture.service.set_helper(helper_exe(false));
        let launch = launched(fixture.service.prepare(1, &codex, shim, &[], None));
        // What the manager checks before starting a cmd.exe launcher.
        assert_eq!(crate::batch::check(shim, &launch.prepend), Ok(()));
    }

    #[test]
    fn a_helper_path_a_shell_would_read_leaves_the_session_without_gate() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let odd = if cfg!(windows) {
            r"C:\Users\O'Neil\hangar.exe"
        } else {
            "/home/o'neil/hangar"
        };
        fixture.service.set_helper(PathBuf::from(odd));
        for format in [
            "gemini_system_settings",
            "codex_config_flags",
            "droid_settings",
        ] {
            assert!(
                matches!(
                    fixture.service.prepare(
                        1,
                        &agent_in(format, false),
                        Path::new("x.exe"),
                        &[],
                        None
                    ),
                    GatePlan::Unavailable(GateStatus::InstallPath)
                ),
                "{format}"
            );
        }
        // No token was handed out and no folder stays behind.
        let body = edit(&fixture.root, "src/own.rs");
        let port = fixture.service.port().unwrap();
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some("x"), &body).0, 401);
        assert!(lock(&fixture.service.inner.session_dirs).is_empty());
        // Copilot runs the helper without a shell, and the in-process extensions run no helper,
        // so the same path is fine there.
        for (session, format) in [
            (2, "copilot_plugin"),
            (3, "pi_extension"),
            (4, "opencode_plugin"),
        ] {
            launched(fixture.service.prepare(
                session,
                &agent_in(format, false),
                Path::new("x.exe"),
                &[],
                None,
            ));
        }
    }

    #[test]
    fn pi_and_omp_get_the_extension_file_before_the_users_arguments() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        for (session, format) in [(11, "pi_extension"), (12, "omp_extension")] {
            let launch = launched(fixture.service.prepare(
                session,
                &agent_in(format, false),
                Path::new("pi.exe"),
                &["--continue".to_owned()],
                None,
            ));
            assert_eq!(launch.prepend.len(), 2, "{format}");
            assert_eq!(launch.prepend[0], "-e");
            let file = PathBuf::from(&launch.prepend[1]);
            assert!(file.is_absolute(), "{file:?}");
            assert!(
                file.starts_with(fixture.settings.path().join(SESSIONS_DIR)),
                "{file:?}"
            );
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                recipes::pi_extension()
            );
            assert!(launch.args.is_empty());
            // The extension reads port and token from the helper's variable.
            let port = fixture.service.port().unwrap();
            assert!(env_of(&launch, HELPER_ENV)
                .unwrap()
                .starts_with(&format!("{port}:")));
            assert!(env_of(&launch, TOKEN_ENV).is_none());
            assert_token_only_in_env(&fixture, &launch);
            fixture.service.ended(session);
            wait_until_gone(file.parent().unwrap());
        }
    }

    #[test]
    fn opencode_gets_its_plugin_named_in_an_inline_configuration() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let launch = launched(fixture.service.prepare(
            13,
            &agent_in("opencode_plugin", false),
            Path::new("opencode.exe"),
            &["--continue".to_owned()],
            None,
        ));
        assert!(launch.prepend.is_empty() && launch.args.is_empty());
        let config: serde_json::Value =
            serde_json::from_str(env_of(&launch, recipes::OPENCODE_CONFIG_CONTENT_ENV).unwrap())
                .unwrap();
        let url = url::Url::parse(config["plugin"][0].as_str().unwrap()).unwrap();
        let plugin = url.to_file_path().unwrap();
        let dir = plugin.parent().unwrap().to_path_buf();
        assert!(
            dir.starts_with(fixture.settings.path().join(SESSIONS_DIR)),
            "{dir:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&plugin).unwrap(),
            recipes::opencode_plugin()
        );
        // The plugin file only: no configuration folder for OpenCode to install into.
        assert_eq!(files_below(&dir).len(), 1);
        assert!(env_of(&launch, "OPENCODE_CONFIG_DIR").is_none());
        assert!(env_of(&launch, HELPER_ENV).is_some());
        assert_token_only_in_env(&fixture, &launch);
        fixture.service.ended(13);
        wait_until_gone(&dir);
    }

    #[test]
    fn droid_gets_a_settings_file_that_runs_the_helper() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        fixture.service.set_helper(helper_exe(true));
        let launch = launched(fixture.service.prepare(
            14,
            &agent_in("droid_settings", false),
            Path::new("droid.exe"),
            &["--resume".to_owned()],
            None,
        ));
        assert_eq!(launch.prepend[0], "--settings");
        assert_eq!(launch.prepend.len(), 2);
        let file = PathBuf::from(&launch.prepend[1]);
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let command = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            command.contains(helper_exe(true).to_str().unwrap()),
            "{command}"
        );
        assert!(
            command.ends_with("gate-hook droid pre-tool-use"),
            "{command}"
        );
        assert!(env_of(&launch, HELPER_ENV).is_some());
        assert_token_only_in_env(&fixture, &launch);
        fixture.service.ended(14);
        wait_until_gone(file.parent().unwrap());
    }

    #[test]
    fn a_cmd_launcher_takes_the_extension_file_only_where_cmd_passes_it_on() {
        if !cfg!(windows) {
            return;
        }
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let shim = Path::new(r"C:\npm\pi.cmd");
        let launch = launched(fixture.service.prepare(
            15,
            &agent_in("pi_extension", false),
            shim,
            &[],
            None,
        ));
        // What the manager checks before starting a cmd.exe launcher.
        assert_eq!(crate::batch::check(shim, &launch.prepend), Ok(()));
    }

    #[test]
    fn json_routes_answer_no_decision_as_an_empty_object() {
        let fixture = fixture(Duration::from_millis(300));
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        assert_eq!(
            post(
                port,
                ROUTE_PRE_TOOL_USE_JSON,
                Some(&token),
                &edit(&fixture.root, "src/own.rs")
            ),
            (200, "{}".to_owned())
        );
        // A decision carries the event name, as Qwen Code expects.
        let (code, body) = post(
            port,
            ROUTE_PRE_TOOL_USE_JSON,
            Some(&token),
            &edit(&fixture.root, "src/shared.rs"),
        );
        assert_eq!(code, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        let prompt = r#"{"session_id":"q1","hook_event_name":"UserPromptSubmit","prompt":"hi"}"#;
        let (_, first) = post(port, ROUTE_USER_PROMPT_SUBMIT_JSON, Some(&token), prompt);
        assert!(first.contains("additionalContext"), "{first}");
        assert_eq!(
            post(port, ROUTE_USER_PROMPT_SUBMIT_JSON, Some(&token), prompt),
            (200, "{}".to_owned())
        );
    }

    #[test]
    fn every_variable_a_format_sets_is_one_a_child_hangar_strips() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        fixture.service.set_helper(helper_exe(false));
        let no_profile_env = std::collections::BTreeMap::new();
        let inside = crate::child_env::changes_with(&no_profile_env, false, true);
        for (session, format) in [
            "claude_settings",
            "qwen_system_settings",
            "gemini_system_settings",
            "copilot_plugin",
            "codex_config_flags",
            "pi_extension",
            "omp_extension",
            "opencode_plugin",
            "droid_settings",
        ]
        .into_iter()
        .enumerate()
        {
            let launch = launched(fixture.service.prepare(
                session as SessionId + 1,
                &agent_in(format, true),
                Path::new("x.exe"),
                &[],
                None,
            ));
            for (name, _) in &launch.env {
                assert!(
                    recipes::GATE_ENV_NAMES.contains(&name.as_str()),
                    "{format}: {name} missing from GATE_ENV_NAMES"
                );
                assert!(
                    inside.contains(&crate::child_env::Change::Remove(name)),
                    "{format}: {name} not stripped"
                );
            }
        }
    }

    #[test]
    fn a_session_folder_that_exists_already_is_never_reused() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let port = fixture.service.ensure_server().unwrap();
        // Planted where the next start's folder would go, with a file in it.
        let next = fixture.service.inner.starts.load(Ordering::SeqCst);
        let planted = fixture
            .settings
            .path()
            .join(SESSIONS_DIR)
            .join(format!("{port}-9-{next}"));
        std::fs::create_dir_all(&planted).unwrap();
        std::fs::write(planted.join(recipes::QWEN_FILE), "planted").unwrap();
        let launch = launched(fixture.service.prepare(
            9,
            &agent_in("qwen_system_settings", false),
            Path::new("qwen.exe"),
            &[],
            None,
        ));
        let file = PathBuf::from(env_of(&launch, recipes::QWEN_SETTINGS_ENV).unwrap());
        assert!(!file.starts_with(&planted), "{file:?}");
        assert_eq!(
            std::fs::read_to_string(planted.join(recipes::QWEN_FILE)).unwrap(),
            "planted"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(file.parent().unwrap()), 0o700);
            assert_eq!(mode(&file), 0o600);
        }
    }

    #[test]
    fn old_session_folders_go_unless_their_port_still_listens() {
        let dir = TempDir::new("gate-sessions-old");
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let held_port = held.local_addr().unwrap().port();
        let free_port = {
            let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            probe.local_addr().unwrap().port()
        };
        let names = [format!("{held_port}-1-0"), format!("{free_port}-2-3")];
        for name in &names {
            std::fs::create_dir_all(dir.path().join(name).join("copilot-plugin")).unwrap();
        }
        remove_old_session_dirs(dir.path());
        let left: Vec<bool> = names.iter().map(|n| dir.path().join(n).exists()).collect();
        assert_eq!(left, [true, false]);
        drop(held);
    }

    #[test]
    fn a_connection_whose_head_is_read_is_never_closed_for_a_newcomer() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let heads = Arc::new(Mutex::new(Heads::default()));
        let mut clients = Vec::new();
        let mut accepted = || {
            clients.push(connect(port));
            listener.accept().unwrap().0
        };
        let first_stream = accepted();
        let first = HeadSlot::take(&heads, &first_stream, Duration::ZERO).unwrap();
        assert!(first.head_read());
        // Every slot taken and then some, with no grace: each newcomer closes the oldest one
        // still reading its head, never the one being answered.
        let mut others = Vec::new();
        for _ in 0..2 * MAX_UNAUTHENTICATED {
            let stream = accepted();
            others.push(HeadSlot::take(&heads, &stream, Duration::ZERO).unwrap());
        }
        assert!(!first.cancel.load(Ordering::SeqCst));
        assert!(others.iter().any(|slot| slot.cancel.load(Ordering::SeqCst)));
        // One that lost its slot before its head was read says so.
        let evicted = others
            .iter()
            .find(|slot| slot.cancel.load(Ordering::SeqCst))
            .unwrap();
        assert!(!evicted.head_read());
    }

    #[test]
    fn a_tool_call_hangar_cannot_read_is_blocked_not_refused() {
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let (port, token) = launch(&fixture, 1);
        let root = fixture.root.to_string_lossy().replace('\\', "/");
        for body in [
            "{".to_owned(),
            r#"{"tool_name":"Bash","tool_input":{"command":"echo \udc00; git push --force"}}"#
                .to_owned(),
            format!(
                r#"{{"tool_name":"Bash","cwd":"{root}/a\udc00","tool_input":{{"command":"git push --force"}}}}"#
            ),
            format!(
                r#"{{"tool_name":"Edit","tool_input":{{"file_path":"{root}/src/a\udc00.rs"}}}}"#
            ),
            r#"{"tool_name":"Ba\udc00sh","tool_input":{"command":"git push --force"}}"#.to_owned(),
        ] {
            for route in [ROUTE_PRE_TOOL_USE, ROUTE_PRE_TOOL_USE_JSON] {
                let (status, answer) = post(port, route, Some(&token), &body);
                assert_eq!(status, 200, "{body}");
                assert!(answer.contains("\"deny\""), "{body}: {answer}");
                assert!(answer.contains("could not read"), "{answer}");
            }
        }
        // With the gate off nothing is held: "no decision", not an error, since Claude Code's
        // hook blocks the call on any error (`onFailure`).
        fixture.service.configure(false);
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE, Some(&token), "{"),
            (200, String::new())
        );
        assert_eq!(
            post(port, ROUTE_PRE_TOOL_USE_JSON, Some(&token), "{"),
            (200, gate::EMPTY_JSON.to_owned())
        );
    }
}
