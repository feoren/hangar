//! Tokens and estimated cost per session and per local day (contract:
//! docs/wiki/pages/syntheses/costs-and-handover.md).
//!
//! One thread polls the transcripts of every session whose program keeps Claude Code
//! transcripts ([`token_usage`]), prices each response with the [`PriceTable`] and keeps the
//! numbers per conversation and local day in a small ledger (`cost-ledger.json` in the local data
//! folder), so the last days still count sessions that were closed since. The window gets a full
//! signal only when a number changed and then reads the [`CostReport`]. Only numbers, titles and
//! program names leave this module, never text of a transcript.
//!
//! Other agents: Codex CLI keeps token counts in rollout files Hangar cannot tie to a session,
//! and the others document no usage log; their sessions show nothing (see the wiki).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::lock;
use crate::model_prices::PriceTable;
use crate::session::SessionId;
use crate::subagents::claude::{is_agent_id, is_real_dir, ClaudeSource, MAX_AGENTS};
use crate::subagents::{SubAgentKind, Target};
use crate::token_usage::{system_local_day, Bucket, Day, LocalDay, SessionTally, Tokens};

/// Signal without payload that the [`CostReport`] changed; the main window reads it with
/// `cost_report` (any window may listen to an event, and the report carries session titles).
pub const EVENT: &str = "cost-usage-changed";
/// Costs need no second-by-second updates; an idle session costs one `stat` per file and poll.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// The ledger in the local data folder.
pub const LEDGER_FILE: &str = "cost-ledger.json";
/// Days the ledger keeps; the report shows [`REPORT_DAYS`] of them.
pub const KEEP_DAYS: i64 = 31;
pub const REPORT_DAYS: i64 = 7;
/// The ledger is written at most this often while sessions work, and on exit.
const SAVE_INTERVAL: Duration = Duration::from_secs(30);
/// Bounds the ledger: the conversations with the newest days win.
const MAX_LEDGER_SESSIONS: usize = 2000;
const MAX_LEDGER_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 120;
const LEDGER_VERSION: u32 = 1;

/// What the poll needs to know about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostTarget {
    pub id: SessionId,
    pub target: Target,
    pub title: String,
    /// The program's name ("Claude Code").
    pub program: String,
}

/// Tokens and cost of one conversation, as the window shows them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostAmount {
    pub tokens: Tokens,
    /// All tokens together.
    pub total: u64,
    /// Estimated US dollars of the priced responses; `None` when none has a price.
    pub cost_usd: Option<f64>,
    /// Some responses have no price, so `cost_usd` is less than the whole.
    pub partial: bool,
}

impl From<&Bucket> for CostAmount {
    fn from(bucket: &Bucket) -> Self {
        let total = bucket.tokens.total();
        let priced = bucket.unpriced < total;
        Self {
            tokens: bucket.tokens,
            total,
            cost_usd: priced.then_some(bucket.cost),
            partial: priced && bucket.unpriced > 0,
        }
    }
}

/// One running session's conversation so far (all days). Reused by other features (the arena
/// compares sessions by it) through [`CostService::session`] and the `cost_report` command.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCost {
    pub session_id: SessionId,
    #[serde(flatten)]
    pub amount: CostAmount,
}

/// One conversation's share of a day.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySession {
    /// The running session, `None` once it was closed.
    pub session_id: Option<SessionId>,
    pub title: String,
    pub program: String,
    #[serde(flatten)]
    pub amount: CostAmount,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCost {
    /// Local day, "YYYY-MM-DD".
    pub day: String,
    #[serde(flatten)]
    pub amount: CostAmount,
    /// Most expensive first.
    pub sessions: Vec<DaySession>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostReport {
    /// Local day of the report, "YYYY-MM-DD".
    pub today: String,
    /// Running sessions with known usage, by id.
    pub sessions: Vec<SessionCost>,
    /// Today and the days before it, newest first, [`REPORT_DAYS`] entries.
    pub days: Vec<DayCost>,
    /// Where the prices come from and when they were taken.
    pub price_source: String,
    pub price_date: String,
}

impl CostReport {
    pub fn session(&self, id: SessionId) -> Option<&SessionCost> {
        self.sessions.iter().find(|entry| entry.session_id == id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct LedgerSession {
    #[serde(default)]
    title: String,
    #[serde(default)]
    program: String,
    /// Local day ("YYYY-MM-DD") to its numbers.
    #[serde(default)]
    days: BTreeMap<String, Bucket>,
}

/// Per conversation id, per local day: what was used. Replaced as a whole from the transcript on
/// every change, so reading the same conversation twice never counts it twice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Ledger {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    sessions: BTreeMap<String, LedgerSession>,
}

fn short(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT_CHARS)
        .collect()
}

fn valid_bucket(bucket: &Bucket) -> bool {
    bucket.cost.is_finite() && bucket.cost >= 0.0
}

impl Ledger {
    /// Reads the ledger; a missing, oversized or broken file starts an empty one, and entries
    /// that do not check out are dropped.
    fn load(path: &Path) -> Self {
        let fits = fs::metadata(path).is_ok_and(|meta| meta.len() <= MAX_LEDGER_BYTES);
        let mut ledger: Self = fits
            .then(|| fs::read(path).ok())
            .flatten()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        ledger.sessions.retain(|id, session| {
            session.title = short(&session.title);
            session.program = short(&session.program);
            session
                .days
                .retain(|day, bucket| Day::parse(day).is_some() && valid_bucket(bucket));
            crate::cli_session::validate(id).is_ok_and(|valid| &valid == id)
                && !session.days.is_empty()
        });
        ledger.version = LEDGER_VERSION;
        ledger
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        let data = serde_json::to_vec(self).map_err(io::Error::other)?;
        crate::persist::write_atomic(path, &data)
    }

    /// Drops days before `oldest` and, beyond [`MAX_LEDGER_SESSIONS`], the conversations with
    /// the oldest days. `true` when something went.
    fn prune(&mut self, oldest: Day) -> bool {
        let oldest = oldest.to_string();
        let before: usize = self.sessions.values().map(|s| s.days.len()).sum();
        for session in self.sessions.values_mut() {
            session.days.retain(|day, _| *day >= oldest);
        }
        self.sessions.retain(|_, session| !session.days.is_empty());
        if self.sessions.len() > MAX_LEDGER_SESSIONS {
            let mut newest: Vec<(String, String)> = self
                .sessions
                .iter()
                .map(|(id, s)| {
                    (
                        s.days.keys().next_back().cloned().unwrap_or_default(),
                        id.clone(),
                    )
                })
                .collect();
            newest.sort_by(|a, b| b.cmp(a));
            for (_, id) in newest.into_iter().skip(MAX_LEDGER_SESSIONS) {
                self.sessions.remove(&id);
            }
        }
        let after: usize = self.sessions.values().map(|s| s.days.len()).sum();
        after != before
    }
}

/// The report from the ledger and the running sessions' totals. `live` names each running
/// session's conversation id; `today` is the local day the report ends with.
fn build_report(
    ledger: &Ledger,
    live: &[(SessionId, String)],
    totals: &BTreeMap<SessionId, Bucket>,
    prices: &PriceTable,
    today: Day,
) -> CostReport {
    let sessions = totals
        .iter()
        .filter(|(_, bucket)| bucket.tokens.total() > 0)
        .map(|(id, bucket)| SessionCost {
            session_id: *id,
            amount: CostAmount::from(bucket),
        })
        .collect();
    let running: HashMap<&str, SessionId> =
        live.iter().map(|(id, cli)| (cli.as_str(), *id)).collect();
    let days = (0..REPORT_DAYS)
        .map(|back| {
            let day = today.minus(back).to_string();
            let mut whole = Bucket::default();
            let mut entries: Vec<DaySession> = Vec::new();
            for (cli, session) in &ledger.sessions {
                let Some(bucket) = session.days.get(&day) else {
                    continue;
                };
                if bucket.tokens.total() == 0 {
                    continue;
                }
                whole.add(bucket);
                entries.push(DaySession {
                    session_id: running.get(cli.as_str()).copied(),
                    title: session.title.clone(),
                    program: session.program.clone(),
                    amount: CostAmount::from(bucket),
                });
            }
            entries.sort_by(|a, b| {
                let cost = |s: &DaySession| s.amount.cost_usd.unwrap_or(0.0);
                cost(b)
                    .total_cmp(&cost(a))
                    .then(b.amount.total.cmp(&a.amount.total))
                    .then(a.title.cmp(&b.title))
            });
            DayCost {
                day,
                amount: CostAmount::from(&whole),
                sessions: entries,
            }
        })
        .collect();
    CostReport {
        today: today.to_string(),
        sessions,
        days,
        price_source: prices.source.clone(),
        price_date: prices.retrieved.clone(),
    }
}

/// The main transcript and the transcripts of the conversation's sub-agents, newest
/// [`MAX_AGENTS`]. Only real files in real folders count, never links.
fn transcript_files(main: &Path, session: &str) -> Vec<PathBuf> {
    let mut files = vec![main.to_path_buf()];
    let Some(project) = main.parent() else {
        return files;
    };
    let folder = project.join(session);
    let dir = folder.join("subagents");
    if !is_real_dir(&folder) || !is_real_dir(&dir) {
        return files;
    }
    let Ok(entries) = fs::read_dir(&dir) else {
        return files;
    };
    let mut agents: Vec<(Option<SystemTime>, PathBuf)> = entries
        .take(4 * MAX_AGENTS)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix("agent-")?.strip_suffix(".jsonl"))
                .is_some_and(is_agent_id)
        })
        .map(|entry| {
            let modified = entry.metadata().and_then(|m| m.modified()).ok();
            (modified, entry.path())
        })
        .collect();
    agents.sort_by(|a, b| b.cmp(a));
    files.extend(agents.into_iter().take(MAX_AGENTS).map(|(_, path)| path));
    files
}

/// One session's transcripts.
struct Tracker {
    conversation: String,
    locator: ClaudeSource,
    tally: SessionTally,
    title: String,
    program: String,
}

/// The poll's state between rounds.
struct Poller {
    trackers: HashMap<SessionId, Tracker>,
    prices: PriceTable,
    local_day: LocalDay,
}

impl Poller {
    fn new(prices: PriceTable, local_day: LocalDay) -> Self {
        Self {
            trackers: HashMap::new(),
            prices,
            local_day,
        }
    }

    /// Reads the targets' transcripts and writes what changed into `ledger`. Returns the running
    /// sessions' totals and whether the ledger changed.
    fn poll(
        &mut self,
        targets: &[CostTarget],
        ledger: &mut Ledger,
    ) -> (BTreeMap<SessionId, Bucket>, bool) {
        self.trackers
            .retain(|id, _| targets.iter().any(|target| target.id == *id));
        let mut totals = BTreeMap::new();
        let mut changed = false;
        for target in targets {
            if target.target.kind != SubAgentKind::Claude {
                continue;
            }
            let tracker = self
                .trackers
                .entry(target.id)
                .and_modify(|tracker| {
                    if tracker.conversation != target.target.session {
                        *tracker = Tracker::new(target);
                    }
                })
                .or_insert_with(|| Tracker::new(target));
            let Some(main) = tracker.locator.main_transcript() else {
                continue;
            };
            let files = transcript_files(&main, &tracker.conversation);
            let read = tracker.tally.poll(&files, &self.prices, self.local_day);
            let renamed = tracker.title != target.title || tracker.program != target.program;
            if renamed {
                tracker.title.clone_from(&target.title);
                tracker.program.clone_from(&target.program);
            }
            if read || renamed {
                let days: BTreeMap<String, Bucket> = tracker
                    .tally
                    .days()
                    .into_iter()
                    .map(|(day, bucket)| (day.to_string(), bucket))
                    .collect();
                let entry = LedgerSession {
                    title: short(&tracker.title),
                    program: short(&tracker.program),
                    days,
                };
                let slot = ledger.sessions.entry(tracker.conversation.clone());
                let slot = slot.or_default();
                if *slot != entry && !entry.days.is_empty() {
                    *slot = entry;
                    changed = true;
                }
            }
            totals.insert(target.id, tracker.tally.total());
        }
        ledger
            .sessions
            .retain(|_, session| !session.days.is_empty());
        (totals, changed)
    }
}

impl Tracker {
    fn new(target: &CostTarget) -> Self {
        Self {
            conversation: target.target.session.clone(),
            locator: ClaudeSource::new(&target.target, false),
            tally: SessionTally::default(),
            title: String::new(),
            program: String::new(),
        }
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The running poll, its newest report and the ledger.
#[derive(Default)]
pub struct CostService {
    latest: Arc<Mutex<CostReport>>,
    ledger: Arc<Mutex<Option<(PathBuf, Ledger, bool)>>>,
    stop: Arc<AtomicBool>,
}

impl CostService {
    /// Starts the poll thread. `ledger` is the ledger file, `targets` lists the sessions to
    /// follow, `emit` gets every changed report.
    pub fn start(
        &self,
        ledger: PathBuf,
        prices: PriceTable,
        targets: impl Fn() -> Vec<CostTarget> + Send + 'static,
        emit: impl Fn(&CostReport) + Send + 'static,
    ) -> io::Result<()> {
        let latest = Arc::clone(&self.latest);
        let shared = Arc::clone(&self.ledger);
        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("cost-usage".into())
            .spawn(move || {
                *lock(&shared) = Some((ledger.clone(), Ledger::load(&ledger), false));
                let mut poller = Poller::new(prices, system_local_day);
                let mut saved = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    let targets = targets();
                    let today = system_local_day(now_secs()).unwrap_or(Day::from_index(0));
                    let report = {
                        let mut guard = lock(&shared);
                        let Some((path, ledger, dirty)) = guard.as_mut() else {
                            break;
                        };
                        let (totals, changed) = poller.poll(&targets, ledger);
                        let pruned = ledger.prune(today.minus(KEEP_DAYS - 1));
                        *dirty |= changed || pruned;
                        if *dirty && saved.elapsed() >= SAVE_INTERVAL {
                            match ledger.save(path) {
                                Ok(()) => *dirty = false,
                                Err(error) => eprintln!("hangar: cost ledger not saved: {error}"),
                            }
                            saved = Instant::now();
                        }
                        let live: Vec<(SessionId, String)> = targets
                            .iter()
                            .map(|t| (t.id, t.target.session.clone()))
                            .collect();
                        build_report(ledger, &live, &totals, &poller.prices, today)
                    };
                    let changed = {
                        let mut current = lock(&latest);
                        let changed = *current != report;
                        if changed {
                            current.clone_from(&report);
                        }
                        changed
                    };
                    if changed {
                        emit(&report);
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            })
            .map(drop)
    }

    /// The newest report (empty before the first poll).
    pub fn report(&self) -> CostReport {
        lock(&self.latest).clone()
    }

    /// One running session's tokens and cost so far, `None` without known usage.
    pub fn session(&self, id: SessionId) -> Option<SessionCost> {
        lock(&self.latest).session(id).cloned()
    }

    /// Ends the thread after its current poll and writes the ledger if it changed.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some((path, ledger, dirty)) = lock(&self.ledger).as_mut() {
            if *dirty && ledger.save(path).is_ok() {
                *dirty = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::subagents::claude::CONFIG_DIR_VAR;
    use crate::test_support::TempDir;
    use crate::token_usage::tests::{berlin, FIXTURE};

    const SESSION: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const AGENT: &str = "a1b2c3d4e5f6a7b8c";

    fn target(dir: &TempDir, id: SessionId, title: &str) -> CostTarget {
        CostTarget {
            id,
            target: Target {
                kind: SubAgentKind::Claude,
                session: SESSION.into(),
                cwd: dir.path().join("work space"),
                env: [(
                    CONFIG_DIR_VAR.to_owned(),
                    dir.path().join("config").display().to_string(),
                )]
                .into(),
                home: dir.path().join("home"),
            },
            title: title.into(),
            program: "Claude Code".into(),
        }
    }

    fn append(path: &Path, data: &str) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(data.as_bytes()).unwrap();
    }

    fn day(text: &str) -> Day {
        Day::parse(text).unwrap()
    }

    #[test]
    fn the_report_shows_sessions_and_days_with_honest_costs() {
        let dir = TempDir::new("cost-service");
        let project = dir.path().join("config").join("projects").join("p");
        let agents = project.join(SESSION).join("subagents");
        fs::create_dir_all(&agents).unwrap();
        append(&project.join(format!("{SESSION}.jsonl")), FIXTURE);
        append(
            &agents.join(format!("agent-{AGENT}.jsonl")),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-03-30T08:00:00Z\",\"message\":{\"id\":\"msg_sub\",\"model\":\"claude-haiku-4-5\",\"usage\":{\"input_tokens\":2000000,\"output_tokens\":0}}}\n",
        );
        // Not a sub-agent transcript: never read.
        append(
            &agents.join("notes.jsonl"),
            &FIXTURE.replace("msg_", "other_"),
        );
        let mut poller = Poller::new(PriceTable::built_in(), berlin);
        let mut ledger = Ledger::default();
        let targets = [target(&dir, 3, "Claude Code (2)")];
        let (totals, changed) = poller.poll(&targets, &mut ledger);
        assert!(changed);
        let report = build_report(
            &ledger,
            &[(3, SESSION.into())],
            &totals,
            &poller.prices,
            day("2026-03-30"),
        );
        assert_eq!(report.today, "2026-03-30");
        assert_eq!(report.days.len(), REPORT_DAYS as usize);
        assert_eq!(report.days[0].day, "2026-03-30");
        assert_eq!(report.days[6].day, "2026-03-24");
        // The 30th: msg_s (1M at $1) and the sub-agent's 2M at $1.
        let today = &report.days[0];
        assert_eq!(today.amount.tokens.input, 3_000_000);
        assert!((today.amount.cost_usd.unwrap() - 3.0).abs() < 1e-9);
        assert!(!today.amount.partial);
        assert_eq!(today.sessions.len(), 1);
        assert_eq!(today.sessions[0].session_id, Some(3));
        assert_eq!(today.sessions[0].title, "Claude Code (2)");
        assert_eq!(today.sessions[0].program, "Claude Code");
        // A day without use is listed empty.
        assert_eq!(report.days[3].amount.total, 0);
        assert_eq!(report.days[3].amount.cost_usd, None);
        assert!(report.days[3].sessions.is_empty());
        // The session's total covers every day and the unknown model, so it is partial.
        let session = report.session(3).unwrap();
        assert!(session.amount.partial);
        assert!(session.amount.cost_usd.unwrap() > 3.0);
        assert!(!serde_json::to_string(&report).unwrap().contains("MARKER"));
        assert_eq!(
            serde_json::to_value(&report.days[0].sessions[0]).unwrap()["sessionId"],
            serde_json::json!(3)
        );
        // Nothing new: nothing changes.
        assert!(!poller.poll(&targets, &mut ledger).1);
        // A renamed session renames its ledger entry.
        let renamed = [target(&dir, 3, "Review")];
        assert!(poller.poll(&renamed, &mut ledger).1);
        assert_eq!(ledger.sessions[SESSION].title, "Review");
        // A closed session stays in the days, without a session id.
        let (totals, _) = poller.poll(&[], &mut ledger);
        assert!(totals.is_empty());
        let report = build_report(&ledger, &[], &totals, &poller.prices, day("2026-03-30"));
        assert!(report.sessions.is_empty());
        assert_eq!(report.days[0].sessions[0].session_id, None);
        assert_eq!(report.days[0].sessions[0].title, "Review");
    }

    #[test]
    fn a_session_with_only_unknown_models_shows_tokens_only() {
        let mut ledger = Ledger::default();
        let bucket = Bucket {
            tokens: Tokens {
                input: 10,
                ..Tokens::default()
            },
            cost: 0.0,
            unpriced: 10,
        };
        ledger.sessions.insert(
            SESSION.into(),
            LedgerSession {
                title: "Local".into(),
                program: "Claude Code".into(),
                days: [("2026-11-13".to_owned(), bucket)].into(),
            },
        );
        let totals: BTreeMap<SessionId, Bucket> = [(1, bucket)].into();
        let report = build_report(
            &ledger,
            &[(1, SESSION.into())],
            &totals,
            &PriceTable::built_in(),
            day("2026-11-13"),
        );
        assert_eq!(report.sessions[0].amount.cost_usd, None);
        assert!(!report.sessions[0].amount.partial);
        assert_eq!(report.sessions[0].amount.total, 10);
        assert_eq!(report.days[0].amount.cost_usd, None);
    }

    #[test]
    fn the_ledger_survives_a_restart_and_forgets_old_days() {
        let dir = TempDir::new("cost-ledger");
        let path = dir.path().join(LEDGER_FILE);
        let bucket = Bucket {
            tokens: Tokens {
                output: 5,
                ..Tokens::default()
            },
            cost: 0.5,
            unpriced: 0,
        };
        let mut ledger = Ledger::default();
        ledger.sessions.insert(
            SESSION.into(),
            LedgerSession {
                title: "A".into(),
                program: "Claude Code".into(),
                days: [
                    ("2026-10-01".to_owned(), bucket),
                    ("2026-11-13".to_owned(), bucket),
                ]
                .into(),
            },
        );
        ledger.save(&path).unwrap();
        let mut loaded = Ledger::load(&path);
        assert_eq!(loaded.sessions, ledger.sessions);
        assert!(loaded.prune(day("2026-11-13").minus(KEEP_DAYS - 1)));
        assert_eq!(loaded.sessions[SESSION].days.len(), 1);
        assert!(!loaded.prune(day("2026-11-13").minus(KEEP_DAYS - 1)));
        // Broken entries are dropped, a broken file starts empty.
        fs::write(
            &path,
            format!(
                "{{\"sessions\":{{\"not-an-id\":{{\"days\":{{\"2026-11-13\":{{\"tokens\":{{\"input\":1,\"output\":0,\"cacheWrite5m\":0,\"cacheWrite1h\":0,\"cacheRead\":0}},\"cost\":1.0,\"unpriced\":0}}}}}},\"{SESSION}\":{{\"days\":{{\"2026-13-01\":{{\"tokens\":{{\"input\":1,\"output\":0,\"cacheWrite5m\":0,\"cacheWrite1h\":0,\"cacheRead\":0}},\"cost\":1.0,\"unpriced\":0}}}}}}}}}}"
            ),
        )
        .unwrap();
        assert!(Ledger::load(&path).sessions.is_empty());
        fs::write(&path, "{").unwrap();
        assert!(Ledger::load(&path).sessions.is_empty());
        assert!(Ledger::load(&dir.path().join("missing.json"))
            .sessions
            .is_empty());
    }
}
