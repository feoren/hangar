//! How full each agent session's context window is, for the meter in its sidebar row.
//!
//! Claude Code so far: the newest `assistant` record of the session's main transcript carries the
//! response's `usage`; input, cache and output tokens together are the context size (measured
//! with 2.1.322, docs/wiki/pages/sources/subagent-activity-measurement.md). The window size is
//! not stored anywhere, so it comes from the model id ([`context_window`]).
//!
//! One thread polls every session's transcript every [`POLL_INTERVAL`] and emits a full
//! [`ContextSnapshot`] only when a number changed, so the window gets at most one event per poll
//! and none while nobody works. Only numbers leave this module, never text of the transcript.
//! Codex keeps `token_count` events with `model_context_window` in its rollout files, but Hangar
//! cannot tell which rollout belongs to a session (no thread id is fixed at launch); see the wiki.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::lock;
use crate::session::SessionId;
use crate::subagents::claude::{stamp_of, ClaudeSource, Stamp, Usage};
use crate::subagents::claude_log::skipped_cheaply;
use crate::subagents::tail::Tail;
use crate::subagents::{SubAgentKind, Target};

/// Event with the newest [`ContextSnapshot`].
pub const EVENT: &str = "context-usage-changed";
/// A response takes seconds; a slower poll would lag behind it, a faster one only costs `stat`s.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Window of a model this table does not know.
pub const DEFAULT_WINDOW: u64 = 200_000;
/// Window of the current models (and of older ones with the 1M option).
pub const LARGE_WINDOW: u64 = 1_000_000;
/// The first read of a transcript takes only its end: the newest response is all that counts.
/// Transcripts reach 12 MB and single records exceed 1 MB, so the end is generous.
const INITIAL_TAIL: u64 = 4 * 1024 * 1024;

/// Context use of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContext {
    pub session_id: SessionId,
    /// Tokens in the context after the newest response.
    pub used: u64,
    /// Size of the model's context window.
    pub window: u64,
}

/// Every session with a known context use, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ContextSnapshot {
    pub sessions: Vec<SessionContext>,
}

/// Model id prefixes with a 1M token window. Source: the model table of the Claude API docs
/// (cached 2026-11-03): Fable, Mythos, Opus 5.5, Opus 5, Opus 4.8, 4.7, 4.6, Sonnet 5.5, Sonnet 5,
/// Sonnet 4.6 and Haiku 5.5 have 1M; Haiku 4.5 and older models 200K.
const LARGE_MODELS: &[&str] = &[
    "claude-fable-",
    "claude-mythos-",
    "claude-opus-5",
    "claude-sonnet-5",
    "claude-haiku-5",
    "claude-opus-4-6",
    "claude-opus-4-7",
    "claude-opus-4-8",
    "claude-sonnet-4-6",
];

/// The context window of `model` with `used` tokens in it: [`LARGE_WINDOW`] for the models above
/// and an id with the `[1m]` suffix, else [`DEFAULT_WINDOW`] (Haiku 4.5, Sonnet and Opus up to
/// 4.5, Claude 3, unknown ids). A context already larger than that (a 1M option the id does not
/// show, a newer model) counts as [`LARGE_WINDOW`].
pub fn context_window(model: Option<&str>, used: u64) -> u64 {
    let id = model.unwrap_or_default().trim().to_ascii_lowercase();
    let known = if id.ends_with("[1m]") || LARGE_MODELS.iter().any(|p| id.starts_with(p)) {
        LARGE_WINDOW
    } else {
        DEFAULT_WINDOW
    };
    if used > known {
        known.max(LARGE_WINDOW)
    } else {
        known
    }
}

#[derive(Deserialize)]
struct Head {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "isSidechain")]
    sidechain: Option<bool>,
}

#[derive(Deserialize)]
struct Record {
    message: Option<Message>,
}

/// `content` is never read.
#[derive(Deserialize)]
struct Message {
    model: Option<String>,
    usage: Option<Usage>,
}

/// Context size and model of a main-conversation response, `None` for any other line. Synthetic
/// responses (Claude Code's own error notes) carry zero usage and are skipped.
pub fn response_usage(line: &[u8]) -> Option<(u64, Option<String>)> {
    if skipped_cheaply(line) {
        return None;
    }
    let head = serde_json::from_slice::<Head>(line).ok()?;
    if head.kind.as_deref() != Some("assistant") || head.sidechain == Some(true) {
        return None;
    }
    let message = serde_json::from_slice::<Record>(line).ok()?.message?;
    if message.model.as_deref() == Some("<synthetic>") {
        return None;
    }
    let used = message.usage?.total();
    (used > 0).then_some((used, message.model))
}

/// One session's transcript: where it is and the newest numbers.
struct Tracker {
    /// The conversation id; another one (restart, resume fallback) starts a new tracker.
    conversation: String,
    locator: ClaudeSource,
    path: Option<PathBuf>,
    tail: Tail,
    /// Size and time of the transcript at the last successful read: an unchanged file is not
    /// opened again, so an idle session costs one `stat` per poll.
    stamp: Option<Stamp>,
    latest: Option<(u64, u64)>,
    /// Transcript reads, to prove the skip.
    #[cfg(test)]
    reads: usize,
}

impl Tracker {
    fn new(target: &Target) -> Self {
        Self {
            conversation: target.session.clone(),
            locator: ClaudeSource::new(target, false),
            path: None,
            tail: Tail::with_initial(INITIAL_TAIL),
            stamp: None,
            latest: None,
            #[cfg(test)]
            reads: 0,
        }
    }

    fn poll(&mut self) -> Option<(u64, u64)> {
        if self.path.is_none() {
            self.path = self.locator.main_transcript();
        }
        if let Some(path) = &self.path {
            // The transcript only grows while the session works; a file replaced by one of the
            // same size changes its time.
            let stamp = stamp_of(path);
            if stamp.is_some() && stamp == self.stamp {
                return self.latest;
            }
            #[cfg(test)]
            {
                self.reads += 1;
            }
            // A read error (file replaced, locked for a moment) is retried on the next poll.
            if let Ok(lines) = self.tail.read_new(path) {
                // A read that stopped before the end is continued on the next poll.
                self.stamp = stamp.filter(|_| !self.tail.behind());
                if let Some((used, model)) = lines.iter().rev().find_map(|l| response_usage(l)) {
                    self.latest = Some((used, context_window(model.as_deref(), used)));
                }
            }
        }
        self.latest
    }
}

/// Follows the transcripts of the given sessions; kept between polls.
#[derive(Default)]
pub struct Poller {
    trackers: HashMap<SessionId, Tracker>,
}

impl Poller {
    /// The context use of every session in `targets` whose transcript has a response yet.
    /// Sessions no longer listed are forgotten.
    pub fn poll(&mut self, targets: &[(SessionId, Target)]) -> ContextSnapshot {
        self.trackers
            .retain(|id, _| targets.iter().any(|(target, _)| target == id));
        let mut sessions = Vec::new();
        for (id, target) in targets {
            if target.kind != SubAgentKind::Claude {
                continue;
            }
            let tracker = self
                .trackers
                .entry(*id)
                .and_modify(|tracker| {
                    if tracker.conversation != target.session {
                        *tracker = Tracker::new(target);
                    }
                })
                .or_insert_with(|| Tracker::new(target));
            if let Some((used, window)) = tracker.poll() {
                sessions.push(SessionContext {
                    session_id: *id,
                    used,
                    window,
                });
            }
        }
        sessions.sort_by_key(|s| s.session_id);
        ContextSnapshot { sessions }
    }
}

/// The running poll and its newest snapshot.
#[derive(Default)]
pub struct ContextUsage {
    latest: Arc<Mutex<ContextSnapshot>>,
    stop: Arc<AtomicBool>,
}

impl ContextUsage {
    /// Starts the poll thread. `targets` lists the sessions to follow (the ones whose program
    /// keeps Claude transcripts); `emit` gets every changed snapshot.
    pub fn start(
        &self,
        targets: impl Fn() -> Vec<(SessionId, Target)> + Send + 'static,
        emit: impl Fn(&ContextSnapshot) + Send + 'static,
    ) -> io::Result<()> {
        let latest = Arc::clone(&self.latest);
        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("context-usage".into())
            .spawn(move || {
                let mut poller = Poller::default();
                while !stop.load(Ordering::Relaxed) {
                    let snapshot = poller.poll(&targets());
                    let changed = {
                        let mut current = lock(&latest);
                        let changed = *current != snapshot;
                        if changed {
                            current.clone_from(&snapshot);
                        }
                        changed
                    };
                    if changed {
                        emit(&snapshot);
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            })
            .map(drop)
    }

    pub fn snapshot(&self) -> ContextSnapshot {
        lock(&self.latest).clone()
    }

    /// Ends the thread after its current poll.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;
    use std::path::Path;

    use super::*;
    use crate::subagents::claude::CONFIG_DIR_VAR;
    use crate::test_support::TempDir;

    const SESSION: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    fn response(model: &str, input: u64, read: u64, output: u64) -> String {
        format!(
            "{{\"parentUuid\":\"p\",\"isSidechain\":false,\"type\":\"assistant\",\"message\":{{\"model\":\"{model}\",\"content\":[{{\"type\":\"text\",\"text\":\"TEXT-MARKER\"}}],\"usage\":{{\"input_tokens\":{input},\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":{read},\"output_tokens\":{output}}}}}}}\n"
        )
    }

    fn append(path: &Path, data: &str) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(data.as_bytes()).unwrap();
    }

    fn target(dir: &TempDir, session: &str) -> Target {
        Target {
            kind: SubAgentKind::Claude,
            session: session.into(),
            cwd: dir.path().join("work space"),
            env: [(
                CONFIG_DIR_VAR.to_owned(),
                dir.path().join("config").display().to_string(),
            )]
            .into(),
            home: dir.path().join("home"),
        }
    }

    #[test]
    fn windows_follow_the_model_table_and_the_use() {
        assert_eq!(context_window(Some("claude-opus-5-5"), 10), LARGE_WINDOW);
        assert_eq!(context_window(Some("claude-sonnet-4-6"), 10), LARGE_WINDOW);
        assert_eq!(context_window(Some("claude-haiku-5-5"), 10), LARGE_WINDOW);
        assert_eq!(context_window(Some("claude-fable-5-2"), 10), LARGE_WINDOW);
        assert_eq!(context_window(Some("claude-haiku-4-5"), 10), DEFAULT_WINDOW);
        assert_eq!(
            context_window(Some("claude-sonnet-4-5"), 10),
            DEFAULT_WINDOW
        );
        assert_eq!(
            context_window(Some("claude-sonnet-4-5[1m]"), 10),
            LARGE_WINDOW
        );
        assert_eq!(context_window(Some("some-new-model"), 10), DEFAULT_WINDOW);
        assert_eq!(context_window(None, 10), DEFAULT_WINDOW);
        // More in use than the table allows: the 1M option is on.
        assert_eq!(
            context_window(Some("claude-sonnet-4-5"), 250_000),
            LARGE_WINDOW
        );
        assert_eq!(context_window(None, 250_000), LARGE_WINDOW);
    }

    #[test]
    fn only_main_responses_with_usage_count() {
        let main = response("claude-opus-5-5", 3, 95_000, 297);
        assert_eq!(
            response_usage(main.as_bytes()),
            Some((95_300, Some("claude-opus-5-5".into())))
        );
        for line in [
            response("<synthetic>", 0, 0, 0),
            response("claude-opus-5-5", 0, 0, 0),
            main.replace("\"isSidechain\":false", "\"isSidechain\":true"),
            "{\"type\":\"user\",\"message\":{\"usage\":{\"input_tokens\":5}}}".into(),
            "{\"type\":\"api-request-blob\",\"message\":{\"usage\":{\"input_tokens\":5".into(),
            "not json".into(),
        ] {
            assert_eq!(response_usage(line.as_bytes()), None, "{line}");
        }
    }

    #[test]
    fn poller_follows_the_newest_response_per_session() {
        let dir = TempDir::new("context-usage");
        let project = dir.path().join("config").join("projects").join("p");
        fs::create_dir_all(&project).unwrap();
        let transcript = project.join(format!("{SESSION}.jsonl"));
        let targets = vec![(4, target(&dir, SESSION))];
        let mut poller = Poller::default();
        // No transcript yet: nothing to show.
        assert_eq!(poller.poll(&targets), ContextSnapshot::default());
        append(
            &transcript,
            "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n",
        );
        append(&transcript, &response("claude-opus-5-5", 3, 100_000, 997));
        // The search for the project folder backs off; a fresh poller finds it at once.
        let mut poller = Poller::default();
        let first = poller.poll(&targets);
        assert_eq!(
            first.sessions,
            [SessionContext {
                session_id: 4,
                used: 101_000,
                window: LARGE_WINDOW
            }]
        );
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains("MARKER"), "{json}");
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::json!({ "sessions": [{ "sessionId": 4, "used": 101_000, "window": 1_000_000 }] })
        );
        // Lines without a response keep the last numbers; a new response replaces them.
        append(
            &transcript,
            "{\"type\":\"user\",\"message\":{\"content\":\"next\"}}\n",
        );
        assert_eq!(poller.poll(&targets), first);
        append(&transcript, &response("claude-haiku-4-5", 10, 20_000, 10));
        assert_eq!(
            poller.poll(&targets).sessions[0],
            SessionContext {
                session_id: 4,
                used: 20_020,
                window: DEFAULT_WINDOW
            }
        );
        // An unchanged transcript is not opened again.
        let reads = poller.trackers[&4].reads;
        assert_eq!(poller.poll(&targets).sessions.len(), 1);
        assert_eq!(poller.trackers[&4].reads, reads);
        append(&transcript, &response("claude-haiku-4-5", 10, 30_000, 10));
        assert_eq!(poller.poll(&targets).sessions[0].used, 30_020);
        assert_eq!(poller.trackers[&4].reads, reads + 1);
        // A closed session is forgotten.
        assert_eq!(poller.poll(&[]), ContextSnapshot::default());
        assert!(poller.trackers.is_empty());
    }
}
