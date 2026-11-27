//! How full each agent session's context window is, for the meter in its sidebar row.
//!
//! Claude Code so far: the newest `assistant` record of the session's main transcript carries the
//! response's `usage`; input, cache and output tokens together are the context size (measured
//! with 2.1.322, docs/wiki/pages/sources/subagent-activity-measurement.md). The window size is
//! not stored anywhere, so it comes from the model id ([`context_window`]).
//!
//! The same records give the session's output rate ([`OutputRate`]): the output tokens of the
//! responses that ended in the last [`RATE_WINDOW_MS`], divided by the time those responses took,
//! each from the prompt or tool result it answers (or the end of the response before, if later)
//! to its last record. Other records (attachments, hook notes) do not start a span: their times
//! can lie at or after the response's first record. Every record of a response carries its final
//! `usage`, so the records land once the response is complete and the rate is known per finished
//! response, never during one (measured with 2.1.319 to 2.1.332,
//! docs/wiki/pages/sources/output-rate-measurement.md).
//!
//! One thread polls every session's transcript every [`POLL_INTERVAL`] (a transcript that has not
//! grown for [`RATE_WINDOW_MS`] only every [`IDLE_EVERY`] polls) and emits a full
//! [`ContextSnapshot`] only when a number changed, so the window gets at most one event per poll
//! and none while nobody works. Only numbers leave this module, never text of the transcript.
//! Codex keeps `token_count` events with `model_context_window` in its rollout files, but Hangar
//! cannot tell which rollout belongs to a session (no thread id is fixed at launch); see the wiki.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::lock;
use crate::session::SessionId;
use crate::subagents::claude::{stamp_of, ClaudeSource, Stamp, Usage};
use crate::subagents::claude_log::skipped_cheaply;
use crate::subagents::tail::Tail;
use crate::subagents::{SubAgentKind, Target};
use crate::token_usage::parse_utc_ms;

/// Event with the newest [`ContextSnapshot`].
pub const EVENT: &str = "context-usage-changed";
/// A response lands in the transcript at once when it is complete; a poll every second shows it
/// within a second. A transcript that has been quiet for [`RATE_WINDOW_MS`] is looked at only
/// every [`IDLE_EVERY`] polls, so an idle session costs one `stat` per two seconds as before.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Every how many polls a quiet transcript is looked at.
pub const IDLE_EVERY: u64 = 2;
/// Responses that ended this long ago or less make up the output rate. 95 % of the measured
/// responses took 26 s or less, so a shorter span would let the number blink out between two of
/// them; with no response in it the rate is gone.
pub const RATE_WINDOW_MS: i64 = 30_000;
/// A response counts as at least this long, so a clock step cannot make a huge rate.
const MIN_SPAN_MS: i64 = 500;
/// Finished responses kept for the rate; more never fit in [`RATE_WINDOW_MS`].
const MAX_KEPT: usize = 64;
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
    /// Output tokens per second of the latest responses ([`OutputRate`]); `None` once none ended
    /// within [`RATE_WINDOW_MS`].
    pub output_rate: Option<u32>,
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
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct Record {
    message: Option<Message>,
}

/// `content` is never read.
#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

/// A response's numbers as one of its records carries them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseLine {
    /// `message.id`, shared by every record of the response.
    pub id: Option<String>,
    /// The context size after the response.
    pub used: u64,
    pub output: u64,
    pub model: Option<String>,
}

/// One record of the main conversation: its time and, for a response, its numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainLine {
    /// Milliseconds since the epoch; `None` without a readable `timestamp`.
    pub at: Option<i64>,
    /// A `user` record: a prompt or a tool result, what the next response answers.
    pub user: bool,
    pub response: Option<ResponseLine>,
}

/// The record a transcript line carries; `None` for a sub-agent's record, one of the large kinds
/// skipped unparsed and a broken line. Synthetic responses (Claude Code's own error notes) carry
/// zero usage and count as no response, like one without usage.
pub fn main_line(line: &[u8]) -> Option<MainLine> {
    if skipped_cheaply(line) {
        return None;
    }
    let head = serde_json::from_slice::<Head>(line).ok()?;
    if head.sidechain == Some(true) {
        return None;
    }
    let at = head.timestamp.as_deref().and_then(parse_utc_ms);
    if head.kind.as_deref() != Some("assistant") {
        let user = head.kind.as_deref() == Some("user");
        return Some(MainLine {
            at,
            user,
            response: None,
        });
    }
    let message = serde_json::from_slice::<Record>(line)
        .ok()
        .and_then(|record| record.message)
        .filter(|message| message.model.as_deref() != Some("<synthetic>"));
    let response = message.and_then(|message| {
        let usage = message.usage?;
        let used = usage.total();
        (used > 0).then(|| ResponseLine {
            id: message.id.filter(|id| !id.is_empty() && id.len() <= 200),
            used,
            output: usage.output(),
            model: message.model,
        })
    });
    Some(MainLine {
        at,
        user: false,
        response,
    })
}

/// Context size and model of a main-conversation response, `None` for any other line.
pub fn response_usage(line: &[u8]) -> Option<(u64, Option<String>)> {
    let response = main_line(line)?.response?;
    Some((response.used, response.model))
}

/// A response for the rate: its output and when it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Timed {
    id: Option<String>,
    /// The prompt or tool result it answers, or the end of the response before if that is later.
    /// `None` when the read began with the response: its span is not known, it does not count.
    start: Option<i64>,
    /// Its last record.
    end: i64,
    output: u64,
}

/// Output tokens per second of a conversation, from its main transcript's records in file order.
/// Pure: times come with the records and "now" with the question, so tests need no clock.
#[derive(Debug, Default)]
pub struct OutputRate {
    /// Where the next response's time starts: the newest prompt or tool result, or the newest
    /// response record if that is later.
    last: Option<i64>,
    /// Newest last.
    responses: VecDeque<Timed>,
}

impl OutputRate {
    /// Takes the next prompt or tool result (`response` is `None`) or response record (its
    /// `message.id` and output tokens), `at` its time. The records of one response (one per
    /// content block, same id and usage) count once, as one response up to its last record.
    pub fn record(&mut self, at: Option<i64>, response: Option<(Option<&str>, u64)>) {
        let Some(at) = at else {
            // A record without a time cannot start a response's span; the next one starts fresh.
            self.last = None;
            return;
        };
        if let Some((id, output)) = response {
            let seen = id.and_then(|id| {
                self.responses
                    .iter_mut()
                    .rev()
                    .find(|known| known.id.as_deref() == Some(id))
            });
            if let Some(known) = seen {
                known.end = known.end.max(at);
                known.output = known.output.max(output);
            } else {
                self.responses.push_back(Timed {
                    id: id.map(str::to_owned),
                    start: self.last.filter(|start| *start <= at),
                    end: at,
                    output,
                });
                if self.responses.len() > MAX_KEPT {
                    self.responses.pop_front();
                }
            }
            // Responses never overlap: the next one starts after this one at the earliest.
            self.last = Some(self.last.map_or(at, |last| last.max(at)));
        } else {
            self.last = Some(at);
        }
    }

    /// Output tokens per second, rounded, of the responses that ended within [`RATE_WINDOW_MS`]
    /// before `now`: their output over the time they took together, so a long response weighs
    /// more than a short one. `None` without such a response or without output.
    pub fn per_second(&self, now: i64) -> Option<u32> {
        let since = now.saturating_sub(RATE_WINDOW_MS);
        let (output, span) = self
            .responses
            .iter()
            .filter(|response| (since..=now).contains(&response.end))
            .filter_map(|response| Some((response.output, response.end - response.start?)))
            .fold((0u64, 0i64), |(output, span), (tokens, took)| {
                let took = took.max(MIN_SPAN_MS);
                (output.saturating_add(tokens), span.saturating_add(took))
            });
        if output == 0 {
            return None;
        }
        // Exact in f64 for any count a transcript can hold; `span` is at least MIN_SPAN_MS.
        let rate = (output as f64 * 1000.0 / span as f64).round();
        (rate >= 1.0).then(|| rate.min(f64::from(u32::MAX)) as u32)
    }

    /// The file was read again from its start: what was read before is gone.
    fn clear(&mut self) {
        *self = Self::default();
    }
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
    rate: OutputRate,
    /// The tail's start count at the last read: another one means the file was read anew.
    generation: u32,
    /// When the transcript last grew ("now" of that poll): a quiet one is looked at less often.
    grew: Option<i64>,
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
            rate: OutputRate::default(),
            generation: 0,
            grew: None,
            #[cfg(test)]
            reads: 0,
        }
    }

    /// Whether this poll looks at the transcript: always while it grows, else every
    /// [`IDLE_EVERY`] polls.
    fn due(&self, now: i64, tick: u64) -> bool {
        let growing = self
            .grew
            .is_some_and(|grew| now.saturating_sub(grew) <= RATE_WINDOW_MS);
        growing || tick.is_multiple_of(IDLE_EVERY)
    }

    fn poll(&mut self, now: i64) -> Option<(u64, u64)> {
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
                if self.tail.generation() != self.generation {
                    self.generation = self.tail.generation();
                    self.rate.clear();
                }
                if !lines.is_empty() {
                    self.grew = Some(now);
                }
                let mut newest = None;
                for record in lines.iter().filter_map(|line| main_line(line)) {
                    let response = record.response.as_ref();
                    if record.user || response.is_some() {
                        self.rate
                            .record(record.at, response.map(|r| (r.id.as_deref(), r.output)));
                    }
                    if record.response.is_some() {
                        newest = record.response;
                    }
                }
                if let Some(ResponseLine { used, model, .. }) = newest {
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
    /// Counts the polls, for [`IDLE_EVERY`].
    tick: u64,
}

impl Poller {
    /// The context use and output rate at `now` (milliseconds since the epoch) of every session
    /// in `targets` whose transcript has a response yet. Sessions no longer listed are forgotten.
    pub fn poll(&mut self, targets: &[(SessionId, Target)], now: i64) -> ContextSnapshot {
        let tick = self.tick;
        self.tick = self.tick.wrapping_add(1);
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
            let latest = if tracker.due(now, tick) {
                tracker.poll(now)
            } else {
                tracker.latest
            };
            if let Some((used, window)) = latest {
                sessions.push(SessionContext {
                    session_id: *id,
                    used,
                    window,
                    output_rate: tracker.rate.per_second(now),
                });
            }
        }
        sessions.sort_by_key(|s| s.session_id);
        ContextSnapshot { sessions }
    }
}

/// Milliseconds since the epoch on the wall clock, the clock of Claude Code's `timestamp`s.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
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
                    let snapshot = poller.poll(&targets(), now_ms());
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
    /// 2026-11-27T10:00:00.000Z, "now" of the polls.
    const NOW: i64 = 1_795_773_600_000;

    /// `2026-11-27T10:00:00.000Z` plus `ms`.
    fn stamp(ms: i64) -> String {
        let at = NOW + ms;
        let (day, hour, minute, second) = crate::token_usage::utc_parts(at.div_euclid(1000));
        format!(
            "{day}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
            at.rem_euclid(1000)
        )
    }

    fn user_at(ms: i64) -> String {
        format!(
            "{{\"type\":\"user\",\"isSidechain\":false,\"timestamp\":\"{}\",\"message\":{{\"content\":\"TEXT-MARKER\"}}}}\n",
            stamp(ms)
        )
    }

    /// Attachments start with `parentUuid` like prompts, so they are parsed, not skipped.
    fn attachment_at(ms: i64) -> String {
        format!(
            "{{\"parentUuid\":\"p\",\"isSidechain\":false,\"type\":\"attachment\",\"timestamp\":\"{}\",\"attachment\":{{\"type\":\"queued_command\"}}}}\n",
            stamp(ms)
        )
    }

    fn response_at(ms: i64, id: &str, output: u64) -> String {
        format!(
            "{{\"isSidechain\":false,\"type\":\"assistant\",\"timestamp\":\"{}\",\"message\":{{\"id\":\"{id}\",\"model\":\"claude-opus-5-5\",\"content\":[{{\"type\":\"text\",\"text\":\"TEXT-MARKER\"}}],\"usage\":{{\"input_tokens\":3,\"cache_read_input_tokens\":50000,\"output_tokens\":{output}}}}}}}\n",
            stamp(ms)
        )
    }

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
        assert_eq!(poller.poll(&targets, NOW), ContextSnapshot::default());
        append(
            &transcript,
            "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n",
        );
        append(&transcript, &response("claude-opus-5-5", 3, 100_000, 997));
        // The search for the project folder backs off; a fresh poller finds it at once.
        let mut poller = Poller::default();
        let first = poller.poll(&targets, NOW);
        assert_eq!(
            first.sessions,
            [SessionContext {
                session_id: 4,
                used: 101_000,
                window: LARGE_WINDOW,
                output_rate: None,
            }]
        );
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains("MARKER"), "{json}");
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::json!({ "sessions": [{
                "sessionId": 4, "used": 101_000, "window": 1_000_000, "outputRate": null
            }] })
        );
        // Lines without a response keep the last numbers; a new response replaces them.
        append(
            &transcript,
            "{\"type\":\"user\",\"message\":{\"content\":\"next\"}}\n",
        );
        assert_eq!(poller.poll(&targets, NOW), first);
        append(&transcript, &response("claude-haiku-4-5", 10, 20_000, 10));
        assert_eq!(
            poller.poll(&targets, NOW).sessions[0],
            SessionContext {
                session_id: 4,
                used: 20_020,
                window: DEFAULT_WINDOW,
                output_rate: None,
            }
        );
        // An unchanged transcript is not opened again.
        let reads = poller.trackers[&4].reads;
        assert_eq!(poller.poll(&targets, NOW).sessions.len(), 1);
        assert_eq!(poller.trackers[&4].reads, reads);
        append(&transcript, &response("claude-haiku-4-5", 10, 30_000, 10));
        assert_eq!(poller.poll(&targets, NOW).sessions[0].used, 30_020);
        assert_eq!(poller.trackers[&4].reads, reads + 1);
        // A closed session is forgotten.
        assert_eq!(poller.poll(&[], NOW), ContextSnapshot::default());
        assert!(poller.trackers.is_empty());
    }

    #[test]
    fn the_test_clock_writes_claude_times() {
        assert_eq!(stamp(1_234), "2026-11-27T10:00:01.234Z");
        assert_eq!(parse_utc_ms(&stamp(-1)), Some(NOW - 1));
    }

    #[test]
    fn main_lines_carry_times_ids_and_output() {
        let line = main_line(response_at(2_500, "msg_a", 300).as_bytes()).unwrap();
        assert_eq!(line.at, Some(NOW + 2_500));
        assert_eq!(
            line.response,
            Some(ResponseLine {
                id: Some("msg_a".into()),
                used: 50_303,
                output: 300,
                model: Some("claude-opus-5-5".into()),
            })
        );
        assert!(!line.user);
        let user = main_line(user_at(1).as_bytes()).unwrap();
        assert_eq!(
            (user.at, user.user, user.response),
            (Some(NOW + 1), true, None)
        );
        // A synthetic note is a record with a time but no response, and no prompt either.
        let note = response_at(5, "msg_n", 0).replace("claude-opus-5-5", "<synthetic>");
        let note = main_line(note.as_bytes()).unwrap();
        assert_eq!(
            (note.at, note.user, note.response),
            (Some(NOW + 5), false, None)
        );
        let attachment = main_line(attachment_at(7).as_bytes()).unwrap();
        assert_eq!((attachment.at, attachment.user), (Some(NOW + 7), false));
        let side =
            response_at(5, "msg_s", 9).replace("\"isSidechain\":false", "\"isSidechain\":true");
        assert_eq!(main_line(side.as_bytes()), None);
        assert_eq!(
            main_line(b"{\"type\":\"api-request-blob\",\"timestamp\":\"x\""),
            None
        );
    }

    #[test]
    fn a_rate_counts_each_response_once_over_the_time_it_took() {
        let mut rate = OutputRate::default();
        assert_eq!(rate.per_second(NOW), None);
        rate.record(Some(NOW), None);
        // Three records of one response (one per content block), the same usage on each.
        rate.record(Some(NOW + 2_000), Some((Some("msg_a"), 300)));
        rate.record(Some(NOW + 2_500), Some((Some("msg_a"), 300)));
        rate.record(Some(NOW + 3_000), Some((Some("msg_a"), 300)));
        assert_eq!(rate.per_second(NOW + 3_000), Some(100));
        // The tool result, then the next response: 400 tokens in 3 s plus 2 s.
        rate.record(Some(NOW + 3_100), None);
        rate.record(Some(NOW + 5_100), Some((Some("msg_b"), 100)));
        assert_eq!(rate.per_second(NOW + 5_100), Some(80));
        // A repeated record of an earlier response still counts once.
        rate.record(Some(NOW + 5_200), Some((Some("msg_a"), 300)));
        assert_eq!(rate.per_second(NOW + 5_200), Some(56));
    }

    #[test]
    fn a_rate_fades_when_no_response_ends_for_the_window() {
        let mut rate = OutputRate::default();
        rate.record(Some(NOW), None);
        rate.record(Some(NOW + 4_000), Some((Some("msg_a"), 200)));
        rate.record(Some(NOW + 4_100), None);
        rate.record(Some(NOW + 14_100), Some((Some("msg_b"), 600)));
        assert_eq!(rate.per_second(NOW + 14_100), Some(57));
        // msg_a leaves the window first, then msg_b.
        assert_eq!(rate.per_second(NOW + 4_000 + RATE_WINDOW_MS), Some(57));
        assert_eq!(rate.per_second(NOW + 4_001 + RATE_WINDOW_MS), Some(60));
        assert_eq!(rate.per_second(NOW + 14_101 + RATE_WINDOW_MS), None);
        // A response "from the future" (the clock stepped back) does not count yet.
        assert_eq!(rate.per_second(NOW + 14_000), Some(50));
    }

    #[test]
    fn a_rate_needs_the_record_before_a_response() {
        let mut rate = OutputRate::default();
        // A read that begins with a response cannot tell how long it took, not even from its
        // later records.
        rate.record(Some(NOW), Some((Some("msg_a"), 500)));
        rate.record(Some(NOW + 1_000), Some((Some("msg_a"), 500)));
        assert_eq!(rate.per_second(NOW + 1_000), None);
        // A record without a time does not start one either.
        rate.record(None, None);
        rate.record(Some(NOW + 2_000), Some((Some("msg_b"), 500)));
        assert_eq!(rate.per_second(NOW + 2_000), None);
        // Two responses in a row without a prompt between: the second starts where the first ended.
        rate.record(Some(NOW + 2_500), None);
        rate.record(Some(NOW + 2_600), Some((Some("msg_d"), 100)));
        rate.record(Some(NOW + 2_700), Some((Some("msg_e"), 100)));
        assert_eq!(rate.per_second(NOW + 2_700), Some(200));
        // A very short response counts as half a second.
        rate.record(Some(NOW + 3_000), None);
        rate.record(Some(NOW + 3_010), Some((Some("msg_c"), 40)));
        assert_eq!(rate.per_second(NOW + 3_010), Some(240 * 1000 / 1500));
        // No output, no rate.
        let mut quiet = OutputRate::default();
        quiet.record(Some(NOW), None);
        quiet.record(Some(NOW + 1_000), Some((None, 0)));
        assert_eq!(quiet.per_second(NOW + 1_000), None);
    }

    #[test]
    fn the_poller_reports_the_rate_live_and_looks_at_quiet_files_less_often() {
        let dir = TempDir::new("context-rate");
        let project = dir.path().join("config").join("projects").join("p");
        fs::create_dir_all(&project).unwrap();
        let transcript = project.join(format!("{SESSION}.jsonl"));
        append(&transcript, &user_at(0));
        // An attachment written along with the response does not start its span.
        append(&transcript, &attachment_at(1_990));
        append(&transcript, &response_at(2_000, "msg_a", 150));
        append(&transcript, &response_at(3_000, "msg_a", 150));
        let targets = vec![(7, target(&dir, SESSION))];
        let mut poller = Poller::default();
        let first = poller.poll(&targets, NOW + 3_200);
        assert_eq!(first.sessions[0].output_rate, Some(50));
        assert_eq!(
            serde_json::to_value(&first).unwrap()["sessions"][0]["outputRate"],
            serde_json::json!(50)
        );
        assert!(!serde_json::to_string(&first).unwrap().contains("MARKER"));
        // The next response raises it: 450 tokens in 3 s plus 2 s.
        append(&transcript, &user_at(3_100));
        append(&transcript, &response_at(5_100, "msg_b", 300));
        assert_eq!(
            poller.poll(&targets, NOW + 5_300).sessions[0].output_rate,
            Some(90)
        );
        // Without new responses it fades, while the context use stays.
        let faded = poller.poll(&targets, NOW + 5_101 + RATE_WINDOW_MS);
        assert_eq!(faded.sessions[0].output_rate, None);
        assert_eq!(faded.sessions[0].used, 50_303);
        // Quiet for longer than the window: looked at only every second poll.
        let later = NOW + 5_300 + RATE_WINDOW_MS + 1;
        append(&transcript, &user_at(40_000));
        let reads = poller.trackers[&7].reads;
        poller.poll(&targets, later);
        poller.poll(&targets, later);
        assert_eq!(poller.trackers[&7].reads, reads + 1);
        // Grown again: looked at on every poll.
        append(&transcript, &response_at(41_000, "msg_c", 100));
        let reads = poller.trackers[&7].reads;
        for _ in 0..4 {
            poller.poll(&targets, later);
            append(&transcript, &user_at(42_000));
        }
        assert_eq!(poller.trackers[&7].reads, reads + 4);
    }
}
