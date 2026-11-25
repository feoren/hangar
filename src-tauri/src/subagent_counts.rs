//! How many sub-agents each agent session runs right now, for the count on its sidebar row.
//!
//! The full sub-agent watch (`subagents::Watchers`) follows one session, the one whose tree or
//! view is open; the other rows only need a number. One thread lists every session's sub-agent
//! folder every [`POLL_INTERVAL`] through [`subagents::counter_for`] (no transcript is read,
//! only the small meta files and the unfinished transcripts' times) and emits a full [`CountSnapshot`] only when a count
//! changed. The status is the very one the watch's snapshot gives ([`AgentStatus::Running`]),
//! so a row and the tree never disagree on what runs. Only numbers leave this module.
//!
//! [`AgentStatus::Running`]: crate::subagents::AgentStatus::Running

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::lock;
use crate::session::SessionId;
use crate::subagents::{self, SubAgentSource, Target};

/// Event with the newest [`CountSnapshot`].
pub const EVENT: &str = "sub-agent-counts-changed";
/// A sub-agent runs for seconds to minutes: a second is quick enough for a row's count and costs
/// one directory listing per session.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Running sub-agents of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCount {
    pub session_id: SessionId,
    pub running: u32,
}

/// Every session with at least one running sub-agent, by id; a session not listed runs none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CountSnapshot {
    pub sessions: Vec<SessionCount>,
}

/// One session's counting source.
struct Tracker {
    /// The conversation id; another one (restart, resume fallback) starts a new tracker.
    conversation: String,
    source: Box<dyn SubAgentSource>,
}

impl Tracker {
    fn new(target: &Target) -> Self {
        Self {
            conversation: target.session.clone(),
            source: subagents::counter_for(target),
        }
    }
}

/// Follows the sub-agent folders of the given sessions; kept between polls.
#[derive(Default)]
pub struct Poller {
    trackers: HashMap<SessionId, Tracker>,
}

impl Poller {
    /// The running sub-agents of every session in `targets` as of `now`. Sessions no longer
    /// listed are forgotten.
    pub fn poll(&mut self, targets: &[(SessionId, Target)], now: SystemTime) -> CountSnapshot {
        self.trackers
            .retain(|id, _| targets.iter().any(|(target, _)| target == id));
        let mut sessions = Vec::new();
        for (id, target) in targets {
            let tracker = self
                .trackers
                .entry(*id)
                .and_modify(|tracker| {
                    if tracker.conversation != target.session {
                        *tracker = Tracker::new(target);
                    }
                })
                .or_insert_with(|| Tracker::new(target));
            let running = u32::try_from(tracker.source.running(now)).unwrap_or(u32::MAX);
            if running > 0 {
                sessions.push(SessionCount {
                    session_id: *id,
                    running,
                });
            }
        }
        sessions.sort_by_key(|s| s.session_id);
        CountSnapshot { sessions }
    }
}

/// The running poll and its newest snapshot.
#[derive(Default)]
pub struct SubAgentCounts {
    latest: Arc<Mutex<CountSnapshot>>,
    stop: Arc<AtomicBool>,
}

impl SubAgentCounts {
    /// Starts the poll thread. `targets` lists the sessions whose program has sub-agents Hangar
    /// can see; `emit` gets every changed snapshot.
    pub fn start(
        &self,
        targets: impl Fn() -> Vec<(SessionId, Target)> + Send + 'static,
        emit: impl Fn(&CountSnapshot) + Send + 'static,
    ) -> io::Result<()> {
        let latest = Arc::clone(&self.latest);
        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("sub-agent-counts".into())
            .spawn(move || {
                let mut poller = Poller::default();
                while !stop.load(Ordering::Relaxed) {
                    let snapshot = poller.poll(&targets(), SystemTime::now());
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

    pub fn snapshot(&self) -> CountSnapshot {
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
    use std::path::Path;

    use super::*;
    use crate::subagents::claude::{encode_project, CONFIG_DIR_VAR, STALE_AFTER};
    use crate::subagents::SubAgentKind;
    use crate::test_support::TempDir;

    const FIRST: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const SECOND: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

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

    /// The sub-agent folder of `session`, under the project folder Claude would use.
    fn subagents(dir: &TempDir, session: &str) -> std::path::PathBuf {
        let project = dir
            .path()
            .join("config")
            .join("projects")
            .join(encode_project(&dir.path().join("work space")));
        let sub = project.join(session).join("subagents");
        fs::create_dir_all(&sub).unwrap();
        fs::write(project.join(format!("{session}.jsonl")), "").unwrap();
        sub
    }

    fn agent(sub: &Path, id: &str, finished: bool) {
        fs::write(
            sub.join(format!("agent-{id}.jsonl")),
            "{\"type\":\"user\"}\n",
        )
        .unwrap();
        if finished {
            fs::write(
                sub.join(format!("agent-{id}.meta.json")),
                "{\"finished\":true}",
            )
            .unwrap();
        }
    }

    #[test]
    fn counts_running_sub_agents_per_session() {
        let dir = TempDir::new("sub-agent-counts");
        let first = subagents(&dir, FIRST);
        let second = subagents(&dir, SECOND);
        agent(&first, "a1", false);
        agent(&first, "a2", false);
        agent(&first, "a3", true);
        agent(&second, "b1", true);
        let targets = vec![(9, target(&dir, SECOND)), (4, target(&dir, FIRST))];
        let mut poller = Poller::default();
        let now = SystemTime::now();
        let snapshot = poller.poll(&targets, now);
        // Only sessions with a running sub-agent, sorted by id; finished ones never count.
        assert_eq!(
            snapshot.sessions,
            [SessionCount {
                session_id: 4,
                running: 2
            }]
        );
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            serde_json::json!({ "sessions": [{ "sessionId": 4, "running": 2 }] })
        );
        // Quiet for a minute: stale, as in the tree, so no longer running.
        let later = now + STALE_AFTER + Duration::from_secs(2);
        assert_eq!(poller.poll(&targets, later), CountSnapshot::default());
        agent(&second, "b2", false);
        assert_eq!(
            poller.poll(&targets, now).sessions,
            [
                SessionCount {
                    session_id: 4,
                    running: 2
                },
                SessionCount {
                    session_id: 9,
                    running: 1
                }
            ]
        );
        // A closed session is forgotten.
        assert_eq!(poller.poll(&[], now), CountSnapshot::default());
        assert!(poller.trackers.is_empty());
    }

    #[test]
    fn a_new_conversation_starts_a_new_count() {
        let dir = TempDir::new("sub-agent-counts-restart");
        let first = subagents(&dir, FIRST);
        agent(&first, "a1", false);
        subagents(&dir, SECOND);
        let mut poller = Poller::default();
        let now = SystemTime::now();
        assert_eq!(
            poller.poll(&[(1, target(&dir, FIRST))], now).sessions[0].running,
            1
        );
        // The same session restarted under a new conversation id without sub-agents yet.
        assert_eq!(
            poller.poll(&[(1, target(&dir, SECOND))], now),
            CountSnapshot::default()
        );
        assert_eq!(poller.trackers[&1].conversation, SECOND);
    }
}
