//! Read-only live view of the sub-agents of an agent session (feature 23).
//!
//! Sub-agents run inside the agent's process and have no terminal of their own; the CLI writes a
//! transcript per sub-agent. A [`SubAgentSource`] turns those files into a whitelisted
//! [`Snapshot`] (never prompts, tool inputs or tool results), and a watcher thread per session
//! polls it and streams changed snapshots to the window. For the one sub-agent the window shows
//! (the focus, [`Watchers::focus`]), the same thread also streams its activity log
//! ([`LogBatch`]) incrementally: the task, assistant text, tool calls with their key argument and
//! short results, only with excerpts on; without them only tool names.
//!
//! Polling instead of file system notifications: the `notify` crate is CC0-1.0 licensed (outside
//! the project's license policy) and not in the tree, and a poll of one small folder every
//! [`POLL_INTERVAL`] costs a handful of `stat` calls. It also behaves the same on every platform
//! and on network drives, where notifications are unreliable.
//!
//! Only Claude Code so far ([`claude`]). Codex (rollout files with `parent_thread_id`), OpenCode
//! (child sessions in its SQLite database or HTTP API) and Gemini CLI would each be another
//! [`SubAgentKind`] with its own source; see docs/wiki/pages/sources/subagent-transcripts.md.

pub mod claude;
pub mod claude_log;
pub mod redact;
pub mod tail;

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize, Serializer};

use crate::lock;
use crate::manager::ManagerError;
use crate::session::SessionId;

/// One poll per interval and at most one event per poll: at most 4 events per second and session.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Where a program keeps its sub-agent transcripts (`sub_agents` in the catalog and profiles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubAgentKind {
    Claude,
}

impl SubAgentKind {
    /// Profile variables the source needs to find the files (paths, never secrets).
    pub fn env_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[claude::CONFIG_DIR_VAR],
        }
    }
}

/// What a source needs to find one session's transcripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub kind: SubAgentKind,
    /// The conversation id Hangar gave the CLI, a validated lowercase UUID.
    pub session: String,
    pub cwd: PathBuf,
    /// Only the variables of [`SubAgentKind::env_names`] that the profile sets.
    pub env: BTreeMap<String, String>,
    pub home: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Running,
    Finished,
    /// Not finished, but no writes for a minute (cancelled, crashed or waiting long).
    Stale,
}

/// The whitelist that reaches the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgent {
    pub id: String,
    pub description: Option<String>,
    pub agent_type: Option<String>,
    pub spawn_depth: Option<u32>,
    pub status: AgentStatus,
    /// Unix time in milliseconds.
    pub started_at: Option<u64>,
    pub last_activity: Option<u64>,
    /// Name of the newest tool call (current or last), such as `Grep`.
    pub tool: Option<String>,
    /// Newest assistant text, at most 400 characters.
    pub excerpt: Option<String>,
}

/// Every sub-agent of the session, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// The CLI's transcript folder was found. `false` until the first prompt of a fresh session.
    pub located: bool,
    pub agents: Vec<SubAgent>,
}

/// One line of a sub-agent's activity log. Every text is already redacted, cleaned and capped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LogEntry {
    /// The task the sub-agent was given (or a later user message).
    Prompt { text: String },
    /// Assistant text.
    Text { text: String },
    /// A tool call with its key argument (file path, command line, pattern), if it has one.
    Tool {
        name: String,
        detail: Option<String>,
    },
    /// A tool result: its first non-empty line and how many lines follow.
    Result {
        text: Option<String>,
        #[serde(rename = "moreLines")]
        more_lines: u32,
        error: bool,
    },
    /// Lines were left out here: only the end of a long transcript is read, and a burst beyond
    /// [`MAX_LOG_BATCH`] keeps its newest lines.
    Gap,
}

impl LogEntry {
    pub fn prompt(text: String) -> Self {
        Self::Prompt { text }
    }

    pub fn text(text: String) -> Self {
        Self::Text { text }
    }
}

/// New log lines of the focused sub-agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogBatch {
    pub agent_id: String,
    /// Replaces everything shown before: a new focus, or a transcript that started over.
    pub reset: bool,
    pub entries: Vec<LogEntry>,
}

/// At most this many log lines per event; a burst keeps its newest ones.
pub const MAX_LOG_BATCH: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SubAgentEvent {
    /// The full current state; replaces the previous one. Sent once at the start and then only
    /// on a change.
    Snapshot(Snapshot),
    /// Log lines of the focused sub-agent, sent only when there are new ones (or on a reset).
    Log(LogBatch),
}

/// Agent ids as they appear in file names (measured for Claude: `a` plus 16 hex digits).
/// Anything that could leave the folder or look like another file is rejected before a path is
/// built from it.
pub fn is_agent_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Turns a CLI's files into snapshots. Implementations keep their read positions between
/// polls; `now` decides the stale status.
pub trait SubAgentSource: Send {
    fn poll(&mut self, now: SystemTime) -> Snapshot;

    /// The sub-agent whose log [`Self::poll_log`] follows from now on (`None`: none). Called
    /// with a validated id ([`is_agent_id`]); the next batch resets the log.
    fn focus(&mut self, _agent: Option<String>) {}

    /// New log lines of the focused sub-agent since the last call, `None` when there are none.
    /// Called right after [`Self::poll`], whose scan it may rely on.
    fn poll_log(&mut self) -> Option<LogBatch> {
        None
    }
}

/// Messages to a running watch.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Control {
    Focus(Option<String>),
}

/// `excerpts: false` reads no assistant text at all; every `excerpt` is then `null`.
pub fn source_for(target: &Target, excerpts: bool) -> Box<dyn SubAgentSource> {
    match target.kind {
        SubAgentKind::Claude => Box::new(claude::ClaudeSource::new(target, excerpts)),
    }
}

/// Receiver of the events; `false` means it is gone and the watch ends.
pub trait SubAgentSink: Send + 'static {
    fn emit(&self, event: SubAgentEvent) -> bool;
}

impl<F> SubAgentSink for F
where
    F: Fn(SubAgentEvent) -> bool + Send + 'static,
{
    fn emit(&self, event: SubAgentEvent) -> bool {
        self(event)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubAgentError {
    #[error(transparent)]
    Session(#[from] ManagerError),
    #[error("The sub-agent watch could not start: {0}")]
    Thread(#[source] io::Error),
    #[error("Not a sub-agent id: {0:?}")]
    AgentId(String),
}

/// The refused id in an error is cut to this many characters: it comes from the window and ends
/// up in an error text and maybe a log line.
const MAX_ERROR_ID_CHARS: usize = 64;

/// `None` stays `None`; an id that fails [`is_agent_id`] is refused.
pub fn checked_focus(agent: Option<String>) -> Result<Option<String>, SubAgentError> {
    match agent {
        Some(id) if !is_agent_id(&id) => Err(SubAgentError::AgentId(
            id.chars().take(MAX_ERROR_ID_CHARS).collect(),
        )),
        other => Ok(other),
    }
}

impl Serialize for SubAgentError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// The running watches, one per session. Dropping a watch's sender ends its thread at the next
/// wake-up, without waiting for it here.
#[derive(Default)]
pub struct Watchers {
    watches: Mutex<HashMap<SessionId, Sender<Control>>>,
}

impl Watchers {
    /// Starts watching; a watch already running for the session is replaced.
    pub fn start(
        &self,
        session: SessionId,
        source: Box<dyn SubAgentSource>,
        sink: impl SubAgentSink,
    ) -> Result<(), SubAgentError> {
        let (stop, stopped) = mpsc::channel();
        thread::Builder::new()
            .name(format!("subagents-{session}"))
            .spawn(move || run(source, &sink, &stopped))
            .map_err(SubAgentError::Thread)?;
        let previous = lock(&self.watches).insert(session, stop);
        drop(previous);
        Ok(())
    }

    /// `false` if no watch was running for the session.
    pub fn stop(&self, session: SessionId) -> bool {
        let removed = lock(&self.watches).remove(&session);
        removed.is_some()
    }

    /// Switches the running watch's log to `agent` (validated by the caller); `false` if no
    /// watch runs for the session. The watch polls at once, so the log follows without delay.
    pub fn focus(&self, session: SessionId, agent: Option<String>) -> bool {
        lock(&self.watches)
            .get(&session)
            .is_some_and(|watch| watch.send(Control::Focus(agent)).is_ok())
    }

    pub fn stop_all(&self) {
        let all: Vec<Sender<Control>> = lock(&self.watches).drain().map(|(_, s)| s).collect();
        drop(all);
    }
}

fn run(mut source: Box<dyn SubAgentSource>, sink: &dyn SubAgentSink, control: &Receiver<Control>) {
    let mut last: Option<Snapshot> = None;
    loop {
        let snapshot = source.poll(SystemTime::now());
        if last.as_ref() != Some(&snapshot) {
            if !sink.emit(SubAgentEvent::Snapshot(snapshot.clone())) {
                return;
            }
            last = Some(snapshot);
        }
        if let Some(batch) = source.poll_log() {
            if !sink.emit(SubAgentEvent::Log(batch)) {
                return;
            }
        }
        match control.recv_timeout(POLL_INTERVAL) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(first) => {
                // Switches that queued up while the poll ran: only the newest counts, so a burst
                // of clicks costs one reset and one read instead of one per click.
                let Control::Focus(agent) = control.try_iter().fold(first, |_, newer| newer);
                source.focus(agent);
            }
            // Stopped: the sender was dropped.
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Instant;

    use super::*;

    /// Yields the scripted snapshots, then repeats the last one.
    struct Scripted(Arc<StdMutex<Vec<Snapshot>>>);

    impl SubAgentSource for Scripted {
        fn poll(&mut self, _now: SystemTime) -> Snapshot {
            let mut list = self.0.lock().unwrap();
            if list.len() > 1 {
                list.remove(0)
            } else {
                list[0].clone()
            }
        }
    }

    fn snapshot(located: bool) -> Snapshot {
        Snapshot {
            located,
            agents: Vec::new(),
        }
    }

    #[test]
    fn events_serialize_as_documented() {
        let agent = SubAgent {
            id: "a1".into(),
            description: Some("d".into()),
            agent_type: Some("Explore".into()),
            spawn_depth: Some(1),
            status: AgentStatus::Stale,
            started_at: Some(1),
            last_activity: Some(2),
            tool: Some("Grep".into()),
            excerpt: None,
        };
        let json = serde_json::to_value(SubAgentEvent::Snapshot(Snapshot {
            located: true,
            agents: vec![agent],
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "snapshot", "located": true, "agents": [{
                "id": "a1", "description": "d", "agentType": "Explore", "spawnDepth": 1,
                "status": "stale", "startedAt": 1, "lastActivity": 2, "tool": "Grep",
                "excerpt": null }] })
        );
        assert_eq!(
            serde_json::from_str::<SubAgentKind>("\"claude\"").unwrap(),
            SubAgentKind::Claude
        );
    }

    #[test]
    fn only_changes_are_sent_and_stop_ends_the_thread() {
        let script = Arc::new(StdMutex::new(vec![
            snapshot(false),
            snapshot(false),
            snapshot(true),
        ]));
        let watchers = Watchers::default();
        let (tx, rx) = channel();
        watchers
            .start(7, Box::new(Scripted(Arc::clone(&script))), move |e| {
                tx.send(e).is_ok()
            })
            .unwrap();
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first, SubAgentEvent::Snapshot(snapshot(false)));
        assert_eq!(second, SubAgentEvent::Snapshot(snapshot(true)));
        // Nothing changes any more, so nothing is sent.
        assert!(rx.recv_timeout(POLL_INTERVAL * 3).is_err());
        assert!(watchers.stop(7));
        assert!(!watchers.stop(7));
        // The thread drops the sink (and its sender) once it ends.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Err(RecvTimeoutError::Disconnected) => break,
                _ if Instant::now() > deadline => panic!("watch thread did not end"),
                _ => {}
            }
        }
    }

    #[test]
    fn log_events_serialize_as_documented() {
        let json = serde_json::to_value(SubAgentEvent::Log(LogBatch {
            agent_id: "a1".into(),
            reset: true,
            entries: vec![
                LogEntry::Gap,
                LogEntry::prompt("task".into()),
                LogEntry::text("note".into()),
                LogEntry::Tool {
                    name: "Read".into(),
                    detail: Some("a.rs".into()),
                },
                LogEntry::Result {
                    text: None,
                    more_lines: 3,
                    error: true,
                },
            ],
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "log", "agentId": "a1", "reset": true,
                "entries": [
                    { "kind": "gap" },
                    { "kind": "prompt", "text": "task" },
                    { "kind": "text", "text": "note" },
                    { "kind": "tool", "name": "Read", "detail": "a.rs" },
                    { "kind": "result", "text": null, "moreLines": 3, "error": true },
                ] })
        );
    }

    #[test]
    fn focus_ids_are_checked() {
        assert_eq!(checked_focus(None).unwrap(), None);
        assert_eq!(
            checked_focus(Some("a0123456789abcdef".into())).unwrap(),
            Some("a0123456789abcdef".into())
        );
        for bad in ["", "..", "a/b", "a\\b", "a.b", "a b"] {
            assert!(checked_focus(Some(bad.into())).is_err(), "{bad:?}");
        }
        assert!(checked_focus(Some("x".repeat(65))).is_err());
        let Err(SubAgentError::AgentId(shown)) = checked_focus(Some("/".repeat(5000))) else {
            panic!("refused id expected");
        };
        assert_eq!(shown.chars().count(), MAX_ERROR_ID_CHARS);
    }

    /// Records every focus; the first poll waits until the test has queued its switches.
    struct Recording {
        focuses: Arc<StdMutex<Vec<Option<String>>>>,
        gate: Option<std::sync::mpsc::Receiver<()>>,
    }

    impl SubAgentSource for Recording {
        fn poll(&mut self, _now: SystemTime) -> Snapshot {
            if let Some(gate) = self.gate.take() {
                let _ = gate.recv_timeout(Duration::from_secs(5));
            }
            snapshot(true)
        }

        fn focus(&mut self, agent: Option<String>) {
            self.focuses.lock().unwrap().push(agent);
        }
    }

    #[test]
    fn queued_focus_switches_collapse_to_the_newest() {
        let focuses = Arc::new(StdMutex::new(Vec::new()));
        let (open, gate) = channel();
        let watchers = Watchers::default();
        let source = Recording {
            focuses: Arc::clone(&focuses),
            gate: Some(gate),
        };
        watchers.start(4, Box::new(source), |_| true).unwrap();
        // The watch is stuck in its first poll: all three wait in the channel.
        for id in ["a1", "a2", "a3"] {
            assert!(watchers.focus(4, Some(id.into())));
        }
        open.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while focuses.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "no focus arrived");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(POLL_INTERVAL);
        assert_eq!(*focuses.lock().unwrap(), [Some("a3".to_owned())]);
        watchers.stop_all();
    }

    /// Reports every focus it gets as a log batch with that id.
    struct Focusing(Option<Option<String>>);

    impl SubAgentSource for Focusing {
        fn poll(&mut self, _now: SystemTime) -> Snapshot {
            snapshot(true)
        }

        fn focus(&mut self, agent: Option<String>) {
            self.0 = Some(agent);
        }

        fn poll_log(&mut self) -> Option<LogBatch> {
            let agent = self.0.take()?;
            Some(LogBatch {
                agent_id: agent.unwrap_or_default(),
                reset: true,
                entries: Vec::new(),
            })
        }
    }

    #[test]
    fn focus_reaches_the_running_watch_at_once() {
        let watchers = Watchers::default();
        assert!(!watchers.focus(3, Some("a1".into())));
        let (tx, rx) = channel();
        watchers
            .start(3, Box::new(Focusing(None)), move |e| tx.send(e).is_ok())
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            SubAgentEvent::Snapshot(snapshot(true))
        );
        assert!(watchers.focus(3, Some("a1".into())));
        let Ok(SubAgentEvent::Log(batch)) = rx.recv_timeout(Duration::from_secs(5)) else {
            panic!("no log batch");
        };
        assert_eq!(batch.agent_id, "a1");
        watchers.stop_all();
    }

    #[test]
    fn a_gone_receiver_ends_the_watch() {
        let watchers = Watchers::default();
        let calls = Arc::new(StdMutex::new(0));
        let counted = Arc::clone(&calls);
        let changing = Arc::new(StdMutex::new(vec![
            snapshot(false),
            snapshot(true),
            snapshot(false),
        ]));
        watchers
            .start(1, Box::new(Scripted(changing)), move |_| {
                *counted.lock().unwrap() += 1;
                false
            })
            .unwrap();
        std::thread::sleep(POLL_INTERVAL * 4);
        assert_eq!(*calls.lock().unwrap(), 1);
        watchers.stop_all();
    }
}
