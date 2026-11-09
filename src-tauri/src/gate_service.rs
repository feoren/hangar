//! Runs the agent gate (domain in `gate.rs`): a hook endpoint on 127.0.0.1, started on the first
//! launch that needs it, that holds an agent's edit while the person decides in the window.
//!
//! The gate coordinates the user's own sessions; it is not a security boundary. Everything that
//! goes wrong lets the edit through (Claude Code treats a failed hook so), and an agent can change
//! files through its shell without any hook.
//!
//! Every session that gets the gate has its own random token. It reaches the agent only through
//! the PTY child's environment ([`TOKEN_ENV`]) and Claude Code's header interpolation; it is never
//! written to a file or a log and never sent to the window. A request without a known token is
//! refused before its body is read.
//!
//! Connections: one thread each. Until its head is read and its token known, a connection holds
//! one of [`MAX_UNAUTHENTICATED`] slots and has [`Limits::head`] in total for the head, however
//! slowly it sends; a known session then moves to one of [`MAX_CONNECTIONS`] slots with
//! [`Limits::body`] for its body. The accept thread never reads or writes: a connection over the
//! limit is closed at once. A held request checks every [`WAIT_TICK`] whether the agent still
//! waits, so an interrupted agent does not leave a card behind.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zeroize::Zeroizing;

use crate::agent_catalog::Agent;
use crate::claims::WorktreeClaims;
use crate::gate::{
    self, Answer, EditRequest, Gate, GateSnapshot, Opened, Outcome, DECISION_TIMEOUT,
    ROUTE_PRE_TOOL_USE, ROUTE_USER_PROMPT_SUBMIT, TOKEN_ENV,
};
use crate::hook_http::{self, Head, Status};
use crate::lock;
use crate::manager::{GateLaunch, GatePlan, LaunchGate};
use crate::session::{GateStatus, SessionId};
use crate::team_share::TeammateFile;
use crate::team_tasks::TeamTask;

/// Event to the main window with the newest [`GateSnapshot`].
pub const EVENT: &str = "gate-changed";
/// Connections of known sessions handled at the same time; one more is answered 503 (Claude
/// Code then carries on as without the hook).
pub const MAX_CONNECTIONS: usize = 64;
/// Connections whose token is not known yet. Small, so a flood of slow connections cannot hold
/// the threads; it only delays new hooks, which then fail open.
pub const MAX_UNAUTHENTICATED: usize = 8;
/// How often a held request checks whether the agent still waits.
const WAIT_TICK: Duration = Duration::from_millis(500);
/// After the answer: how much the client may still send before the close.
const LINGER_BYTES: u64 = 64 * 1024;
/// File name start of the hook settings written for cmd.exe launchers.
const SETTINGS_PREFIX: &str = "claude-hooks-";

/// Time limits of one connection; each is a total, not per read.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// Request line and headers.
    head: Duration,
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

struct Inner {
    gate: Mutex<Gate<Waiter>>,
    /// Port of the endpoint once it runs.
    port: Mutex<Option<u16>>,
    unauthenticated: Arc<AtomicUsize>,
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
                unauthenticated: Arc::new(AtomicUsize::new(0)),
                connections: Arc::new(AtomicUsize::new(0)),
                limits,
                settings_dir,
                worktrees: Box::new(worktrees),
                titles: Box::new(titles),
                emit: Box::new(emit),
                teammates: Mutex::new(None),
                team_tasks: Mutex::new(None),
            }),
        }
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

    /// The person's answer; `false` when the decision no longer waits.
    pub fn answer(&self, id: u64, answer: Answer) -> bool {
        let resolved = lock(&self.inner.gate).answer(id, answer);
        let found = resolved.is_some();
        deliver(resolved.into_iter().flatten());
        if found {
            self.inner.publish();
        }
        found
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
        let inner = Arc::clone(&self.inner);
        thread::Builder::new()
            .name("gate-http".to_owned())
            .spawn(move || accept_loop(&inner, &listener))?;
        *port = Some(bound);
        Ok(bound)
    }

    /// The hook settings as a file, for a cmd.exe launcher: Hangar refuses `"` in such a
    /// launcher's arguments (batch.rs), so the JSON cannot go inline. `None` when the file's path
    /// holds a character cmd.exe would interpret or the file cannot be written.
    fn settings_file(&self, port: u16, context: bool, settings: &str) -> Option<String> {
        let suffix = if context { "-context" } else { "" };
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

    fn prepare(&self, session: SessionId, agent: &Agent, program: &Path) -> GatePlan {
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
        let settings = gate::claude_settings(port, context);
        let argument = if crate::batch::is_batch(program) {
            match self.settings_file(port, context, &settings) {
                Some(path) => path,
                None => return GatePlan::Unavailable(GateStatus::Launcher),
            }
        } else {
            settings
        };
        let token = lock(&self.inner.gate).register(session);
        GatePlan::Launch(GateLaunch {
            args: vec!["--settings".to_owned(), argument],
            env: vec![(TOKEN_ENV.to_owned(), Zeroizing::new(token))],
        })
    }

    fn ended(&self, session: SessionId) {
        let ended = lock(&self.inner.gate).forget(session);
        if !ended.is_empty() {
            deliver(ended);
            self.inner.publish();
        }
    }
}

/// Hands each outcome to its waiting request; a request that gave up meanwhile is gone.
fn deliver(outcomes: impl IntoIterator<Item = (Waiter, Outcome)>) {
    for (waiter, outcome) in outcomes {
        let _ = waiter.try_send(outcome);
    }
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

/// Reads with one deadline for all reads together: each read may take only the time left, so a
/// client sending a byte now and then cannot stretch a limit meant for the whole request.
struct Deadline<'a> {
    stream: &'a TcpStream,
    until: Instant,
}

impl<'a> Deadline<'a> {
    fn new(stream: &'a TcpStream, limit: Duration) -> Self {
        Self {
            stream,
            until: Instant::now() + limit,
        }
    }

    /// A fresh total from now, for the next part of the request.
    fn restart(&mut self, limit: Duration) {
        self.until = Instant::now() + limit;
    }
}

impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        let mut stream = self.stream;
        stream.read(buf)
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
        // Over the limit: closed unanswered, without a read or a write that could block this
        // thread. Claude Code treats that as a failed hook and goes on.
        let Some(slot) = Slot::take(&inner.unauthenticated, MAX_UNAUTHENTICATED) else {
            continue;
        };
        let worker = Arc::clone(inner);
        // A failed spawn drops the closure with its slot and stream.
        let _ = thread::Builder::new()
            .name("gate-conn".to_owned())
            .spawn(move || serve(&worker, &stream, slot));
    }
}

fn serve(inner: &Inner, stream: &TcpStream, unauthenticated: Slot) {
    let _ = stream.set_write_timeout(Some(inner.limits.write));
    let mut reader = BufReader::new(Deadline::new(stream, inner.limits.head));
    // Held until the connection closes, once the session is known.
    let mut _authenticated = None;
    let (status, body) = match hook_http::read_head(&mut reader) {
        Err(error) => match error.status() {
            Some(status) => (status, String::new()),
            None => return,
        },
        Ok(head) => match inner.admit(&head) {
            Err(status) => (status, String::new()),
            Ok(session) => {
                drop(unauthenticated);
                match Slot::take(&inner.connections, MAX_CONNECTIONS) {
                    None => (Status::Unavailable, String::new()),
                    Some(slot) => {
                        _authenticated = Some(slot);
                        reader.get_mut().restart(inner.limits.body);
                        inner.route(session, &head, &mut reader, stream)
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
    fn publish(&self) {
        let snapshot = lock(&self.gate).snapshot();
        (self.emit)(&snapshot);
    }

    /// Who sent the request. A browser page (`Origin`, `Sec-Fetch-*`) or another `Host` than the
    /// one in the hook URL is refused before the token is looked at.
    fn admit(&self, head: &Head) -> Result<SessionId, Status> {
        let port = *lock(&self.port);
        let expected = port.map(|port| format!("127.0.0.1:{port}"));
        if head.from_browser || head.host.is_none() || head.host != expected {
            return Err(Status::Forbidden);
        }
        head.token
            .as_deref()
            .and_then(|token| lock(&self.gate).session_for(token))
            .ok_or(Status::Unauthorized)
    }

    fn route(
        &self,
        session: SessionId,
        head: &Head,
        reader: &mut impl Read,
        stream: &TcpStream,
    ) -> (Status, String) {
        let mut body = reader.take(head.content_length);
        match head.path.as_str() {
            ROUTE_PRE_TOOL_USE => match gate::parse_pre_tool_use(&mut body) {
                Ok(request) => (Status::Ok, self.decide(session, request, stream).body()),
                Err(_) => (Status::BadRequest, String::new()),
            },
            ROUTE_USER_PROMPT_SUBMIT => match gate::parse_prompt_submit(&mut body) {
                Ok(conversation) => (Status::Ok, self.context(session, conversation.as_deref())),
                Err(_) => (Status::BadRequest, String::new()),
            },
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
        let Some(path) = request.path.as_deref() else {
            return Outcome::Allow;
        };
        let worktrees = (self.worktrees)();
        // A session outside every known worktree cannot collide; its path is not even looked at.
        let Some(own) = worktrees.iter().find(|w| w.sessions.contains(&session)) else {
            return Outcome::Allow;
        };
        let candidates = path_candidates(path, request.cwd.as_deref(), Path::new(&own.root_key));
        let teammates = self.teammates_of(session, &worktrees);
        let tasks = self.tasks_of(session, &worktrees);
        let Some(conflict) =
            gate::assess_with_tasks(session, &candidates, &worktrees, &teammates, &tasks)
        else {
            return Outcome::Allow;
        };
        let titles = (self.titles)();
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
            body: Duration::from_secs(5),
            linger: Duration::from_millis(500),
            write: Duration::from_secs(5),
            decision,
        }
    }

    /// Session 1 in `root`, session 2 in another worktree of the same repository that changes
    /// `src/shared.rs` lines 10 to 12.
    fn fixture(decision: Duration) -> Fixture {
        let root = std::env::current_dir().unwrap().join("repo-under-test");
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
        let launch = launched(
            fixture
                .service
                .prepare(session, &claude(), Path::new("claude.exe")),
        );
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
            fixture.service.prepare(1, &claude(), program),
            GatePlan::Skip
        ));
        assert_eq!(fixture.service.port(), None);
        fixture.service.configure(true);
        assert!(matches!(
            fixture.service.prepare(1, &plain_agent(), program),
            GatePlan::Skip
        ));
        let launch = launched(fixture.service.prepare(1, &claude(), program));
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
        let launch = launched(fixture.service.prepare(2, &no_context, program));
        assert!(!launch.args[1].contains("UserPromptSubmit"));
    }

    #[test]
    fn a_cmd_launcher_gets_the_settings_as_a_file_without_the_token() {
        if !cfg!(windows) {
            return;
        }
        let fixture = fixture(DECISION_TIMEOUT);
        fixture.service.configure(true);
        let program = Path::new(r"C:\npm\claude.cmd");
        let launch = launched(fixture.service.prepare(1, &claude(), program));
        let path = PathBuf::from(&launch.args[1]);
        assert!(path.starts_with(fixture.settings.path()), "{path:?}");
        // What cmd.exe passes on unchanged, as the manager checks before the start.
        assert_eq!(crate::batch::check(program, &launch.args), Ok(()));
        let text = std::fs::read_to_string(&path).unwrap();
        let port = fixture.service.port().unwrap();
        assert_eq!(text, gate::claude_settings(port, true));
        assert!(!text.contains(launch.env[0].1.as_str()));
        // A second start reuses the file.
        let again = launched(fixture.service.prepare(2, &claude(), program));
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
            service.prepare(1, &claude(), Path::new(r"C:\npm\claude.cmd")),
            GatePlan::Unavailable(GateStatus::Launcher)
        ));
        // A real program file still gets the gate inline.
        assert!(matches!(
            service.prepare(1, &claude(), Path::new("claude.exe")),
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
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some(&token), "{").0, 400);
        // A session's token stops working when the session ends.
        fixture.service.ended(1);
        assert_eq!(post(port, ROUTE_PRE_TOOL_USE, Some(&token), &body).0, 401);
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
    fn idle_connections_cannot_lock_out_the_endpoint() {
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
        // Silent connections fill every slot for unknown clients and many more wait beyond.
        let idle: Vec<TcpStream> = (0..MAX_UNAUTHENTICATED + MAX_CONNECTIONS)
            .map(|_| connect(port))
            .collect();
        // The accept thread is not blocked by them: one over the limit is closed at once.
        let started = Instant::now();
        let extra = connect(port);
        let took = closed_after(extra, started);
        assert!(took < Duration::from_secs(1), "{took:?}");
        assert!(fixture.service.answer(id, Answer::AllowOnce));
        assert_eq!(waiting.join().unwrap(), (200, String::new()));
        // Once the head limit closed them, a new request is served again.
        thread::sleep(limits(DECISION_TIMEOUT).head + Duration::from_millis(500));
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
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(idle);
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
}
