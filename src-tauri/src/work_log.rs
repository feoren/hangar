//! Work log for the daily review (ADR 0011): which sessions ran, where and when they worked.
//! Sessions end and are closed, so the review cannot be built from the live state alone.
//!
//! One JSON Lines file per UTC day under `<local data>/activity/YYYY-MM-DD.jsonl`, append only.
//! UTC file names keep time zones out of the backend: the window asks for its local day as a
//! time window and the reader opens the few files that can overlap it. Every record carries the
//! session's details, so a session that runs past midnight needs no record from another file.
//!
//! Stored: a session key, title, program name, working folder, repository root, worktree flag
//! and times; for a cross-project message (ADR 0012) also the target session's key and folder,
//! for a message inside a link group (ADR 0014) also the sender's member name, never the text. Never terminal output, prompts, environment variables or secrets. Files older than
//! [`RETENTION_DAYS`] are deleted on startup; a day file stops growing at [`MAX_FILE_BYTES`].

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::lock;
use crate::session::{SessionId, SessionInfo, SessionKind};

/// Folder below the local data folder.
pub const DIR: &str = "activity";
/// Days kept, today included.
pub const RETENTION_DAYS: i64 = 90;
/// A day file stops growing here; a normal day writes a few kilobytes.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Longer lines are skipped when reading; a record is well below this.
const MAX_LINE_BYTES: usize = 16 * 1024;
/// Titles are cut to this many characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Paths longer than this are not logged at all: a cut path would name another folder.
const MAX_PATH_BYTES: usize = 4096;
/// A reported phase may not be longer than this; anything longer is a broken clock.
const MAX_PHASE_MS: i64 = 48 * HOUR_MS;
/// A phase may end this far in the future (clock steps between window and backend).
const FUTURE_SLACK_MS: i64 = 60_000;
/// Format version of a record.
const VERSION: u8 = 1;

pub const DAY_MS: i64 = 24 * HOUR_MS;
const HOUR_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    /// The process of a session started (also after a restart).
    Start,
    /// A working phase ended; `from` is its start, `at` its end.
    Phase,
    /// The process exited or the session was closed.
    End,
    /// The session sent a cross-project message (ADR 0012) to `to_key` in `to_cwd`; no text.
    Message,
    /// The session, a member of a link group (ADR 0014) named `member`, sent a message that was
    /// delivered to `to_key` in `to_cwd`; no text.
    LinkMessage,
}

/// One line of a day file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub v: u8,
    pub event: EventKind,
    /// Unique per session and app run: `<run>-<session id>`.
    pub key: String,
    /// Unix time in milliseconds; for a phase its end.
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<i64>,
    pub title: String,
    /// The program's display name.
    pub program: String,
    pub cwd: String,
    /// Root of the main work tree of the repository the folder belongs to; for a linked
    /// worktree the repository it was made from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default)]
    pub worktree: bool,
    /// The receiving session of a [`EventKind::Message`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_cwd: Option<String>,
    /// The sender's member name of a [`EventKind::LinkMessage`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkLogError {
    #[error("Unknown session {0}")]
    UnknownSession(SessionId),
    #[error("Invalid working phase")]
    InvalidPhase,
    #[error("Invalid link group message")]
    InvalidLinkMessage,
    #[error("Task failed: {0}")]
    Task(String),
}

impl Serialize for WorkLogError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// What the log keeps about a running session, so later records need no lookup.
#[derive(Debug, Clone)]
struct Meta {
    key: String,
    title: String,
    program: String,
    cwd: String,
    repo: Option<String>,
    worktree: bool,
    /// Started and not ended. An ended session's details stay a while: the window reports the
    /// phase that the exit ended only after the exit.
    live: bool,
}

/// Ended sessions whose details are kept; beyond this they are dropped.
const MAX_SESSIONS: usize = 1024;

struct Inner {
    dir: PathBuf,
    /// Distinguishes the session ids of one app run from those of the next.
    run: String,
    sessions: Mutex<HashMap<SessionId, Meta>>,
    /// Held while a line is appended, so two lines never interleave.
    write: Mutex<()>,
}

/// Shared handle; cloned into blocking tasks.
#[derive(Clone)]
pub struct WorkLog(Arc<Inner>);

impl WorkLog {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_run(dir, &now_ms().to_string())
    }

    /// With a fixed run id, for tests.
    pub fn with_run(dir: PathBuf, run: &str) -> Self {
        Self(Arc::new(Inner {
            dir,
            run: run.to_owned(),
            sessions: Mutex::new(HashMap::new()),
            write: Mutex::new(()),
        }))
    }

    pub fn dir(&self) -> &Path {
        &self.0.dir
    }

    /// The process of `info` started. Update sessions are no work and are not logged.
    pub fn started(&self, info: &SessionInfo, program: &str, at: i64) {
        if info.kind == SessionKind::Update {
            return;
        }
        let Some(cwd) = path_text(&info.cwd) else {
            return;
        };
        let location = locate_repo(&info.cwd);
        let repo = location
            .as_ref()
            .and_then(|location| path_text(&location.repo));
        let meta = Meta {
            key: format!("{}-{}", self.0.run, info.id),
            title: cut_title(&info.title),
            program: cut_title(program),
            cwd,
            repo,
            worktree: location.is_some_and(|location| location.worktree),
            live: true,
        };
        let mut sessions = lock(&self.0.sessions);
        if sessions.len() >= MAX_SESSIONS {
            sessions.retain(|_, meta| meta.live);
        }
        sessions.insert(info.id, meta.clone());
        drop(sessions);
        self.append(&record(&meta, EventKind::Start, at, None));
    }

    /// New title for the records that follow; nothing is written.
    pub fn renamed(&self, id: SessionId, title: &str) {
        if let Some(meta) = lock(&self.0.sessions).get_mut(&id) {
            meta.title = cut_title(title);
        }
    }

    /// A working phase of a running (or just ended) session, reported by the window when it ends.
    pub fn phase(&self, id: SessionId, from: i64, to: i64, now: i64) -> Result<(), WorkLogError> {
        // A phase is reported when it ends: one that ended long ago is no phase of this run and
        // would land in an old day file.
        if to <= from
            || to.saturating_sub(from) > MAX_PHASE_MS
            || to > now.saturating_add(FUTURE_SLACK_MS)
            || to < now.saturating_sub(MAX_PHASE_MS)
        {
            return Err(WorkLogError::InvalidPhase);
        }
        let meta = lock(&self.0.sessions)
            .get(&id)
            .cloned()
            .ok_or(WorkLogError::UnknownSession(id))?;
        self.append(&record(&meta, EventKind::Phase, to, Some(from)));
        Ok(())
    }

    /// Session `from` sent a cross-project message to session `to` at `at`: who, where and when,
    /// never the text. Both sessions must be known to the log.
    pub fn message(&self, from: SessionId, to: SessionId, at: i64) -> Result<(), WorkLogError> {
        let (sender, target) = {
            let sessions = lock(&self.0.sessions);
            let sender = sessions
                .get(&from)
                .cloned()
                .ok_or(WorkLogError::UnknownSession(from))?;
            let target = sessions
                .get(&to)
                .cloned()
                .ok_or(WorkLogError::UnknownSession(to))?;
            (sender, target)
        };
        let mut line = record(&sender, EventKind::Message, at, None);
        line.to_key = Some(target.key);
        line.to_cwd = Some(target.cwd);
        self.append(&line);
        Ok(())
    }

    /// A message from session `from`, member `member` of a link group, was delivered to session
    /// `to` at `at` (ADR 0014): who, where and when, never the text. Checked like a working
    /// phase: the name must be a member name and `at` lie within the bounds of a report made now.
    pub fn link_message(
        &self,
        from: SessionId,
        to: SessionId,
        member: &str,
        at: i64,
        now: i64,
    ) -> Result<(), WorkLogError> {
        if !crate::partners::valid_name(member)
            || at > now.saturating_add(FUTURE_SLACK_MS)
            || at < now.saturating_sub(MAX_PHASE_MS)
        {
            return Err(WorkLogError::InvalidLinkMessage);
        }
        let (sender, target) = {
            let sessions = lock(&self.0.sessions);
            let sender = sessions
                .get(&from)
                .cloned()
                .ok_or(WorkLogError::UnknownSession(from))?;
            let target = sessions
                .get(&to)
                .cloned()
                .ok_or(WorkLogError::UnknownSession(to))?;
            (sender, target)
        };
        let mut line = record(&sender, EventKind::LinkMessage, at, None);
        line.to_key = Some(target.key);
        line.to_cwd = Some(target.cwd);
        line.member = Some(member.to_owned());
        self.append(&line);
        Ok(())
    }

    /// The process exited or the session closed. A second call (exit, then close) does nothing.
    pub fn ended(&self, id: SessionId, at: i64) {
        let meta = {
            let mut sessions = lock(&self.0.sessions);
            match sessions.get_mut(&id) {
                Some(meta) if meta.live => {
                    meta.live = false;
                    meta.clone()
                }
                _ => return,
            }
        };
        self.append(&record(&meta, EventKind::End, at, None));
    }

    /// Every session still running ends now: the app quits.
    pub fn end_all(&self, at: i64) {
        let metas: Vec<Meta> = lock(&self.0.sessions)
            .values_mut()
            .filter(|meta| meta.live)
            .map(|meta| {
                meta.live = false;
                meta.clone()
            })
            .collect();
        for meta in metas {
            self.append(&record(&meta, EventKind::End, at, None));
        }
    }

    /// Keys of the sessions that started and have not ended.
    pub fn live_keys(&self) -> HashSet<String> {
        lock(&self.0.sessions)
            .values()
            .filter(|meta| meta.live)
            .map(|meta| meta.key.clone())
            .collect()
    }

    /// Deletes day files older than [`RETENTION_DAYS`]; other files in the folder stay.
    pub fn prune(&self, now: i64) {
        let Ok(entries) = fs::read_dir(&self.0.dir) else {
            return;
        };
        let oldest = day_number(now) - (RETENTION_DAYS - 1);
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(day) = name.to_str().and_then(parse_file_name) else {
                continue;
            };
            if day < oldest {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// The records of every file that can hold something of `[start, end)`: the days of the
    /// window plus [`LOOKBACK_DAYS`] before, so a session that started earlier keeps its start.
    /// `truncated`: a file was larger than [`MAX_FILE_BYTES`] and was read only that far.
    pub fn read_window(&self, start: i64, end: i64) -> (Vec<Record>, bool) {
        let mut records = Vec::new();
        let mut truncated = false;
        let first = day_number(start) - LOOKBACK_DAYS;
        let last = day_number(end.saturating_sub(1).max(start));
        for day in first..=last {
            let path = self.0.dir.join(file_name(day));
            let (mut found, cut) = read_file(&path);
            truncated |= cut;
            records.append(&mut found);
        }
        (records, truncated)
    }

    fn append(&self, record: &Record) {
        let Ok(mut line) = serde_json::to_vec(record) else {
            return;
        };
        line.push(b'\n');
        let path = self.0.dir.join(file_name(day_number(record.at)));
        let _guard = lock(&self.0.write);
        if let Err(error) = append_line(&self.0.dir, &path, &line) {
            eprintln!("hangar: work log not written: {error}");
        }
    }
}

/// Days before the window whose files are read too.
pub const LOOKBACK_DAYS: i64 = 3;

fn record(meta: &Meta, event: EventKind, at: i64, from: Option<i64>) -> Record {
    Record {
        v: VERSION,
        event,
        key: meta.key.clone(),
        at,
        from,
        title: meta.title.clone(),
        program: meta.program.clone(),
        cwd: meta.cwd.clone(),
        repo: meta.repo.clone(),
        worktree: meta.worktree,
        to_key: None,
        to_cwd: None,
        member: None,
    }
}

/// Folder and files are the user's own (0700 and 0600 on Unix), like the notes: they name this
/// machine's folders and session titles.
fn append_line(dir: &Path, path: &Path, line: &[u8]) -> std::io::Result<()> {
    crate::persist::create_private_dir(dir)?;
    let size = fs::metadata(path).map_or(0, |meta| meta.len());
    if size.saturating_add(line.len() as u64) > MAX_FILE_BYTES {
        return Err(std::io::Error::other("day file is full"));
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(line)
}

/// Records of one day file; broken, foreign or too long lines are skipped.
fn read_file(path: &Path) -> (Vec<Record>, bool) {
    let Ok(file) = fs::File::open(path) else {
        return (Vec::new(), false);
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return (Vec::new(), false);
    }
    let truncated = bytes.len() as u64 > MAX_FILE_BYTES;
    if truncated {
        let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        bytes.truncate(end);
    }
    let records = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty() && line.len() <= MAX_LINE_BYTES)
        .filter_map(|line| serde_json::from_slice::<Record>(line).ok())
        .filter(|record| record.v == VERSION)
        .collect();
    (records, truncated)
}

fn cut_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE_CHARS)
        .collect()
}

fn path_text(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    (text.len() <= MAX_PATH_BYTES).then(|| text.into_owned())
}

/// Where a folder lies in git terms, found from the `.git` entries alone, without running git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoLocation {
    /// The work tree that contains the folder.
    pub root: PathBuf,
    /// The main work tree of the repository; `root` itself unless it is a linked worktree.
    pub repo: PathBuf,
    pub worktree: bool,
}

/// Folders walked up at most; deeper trees are not a repository Hangar looks at.
const MAX_DEPTH: usize = 64;
/// A `.git` file holds one `gitdir:` line; anything larger is not one.
const MAX_DOT_GIT_BYTES: usize = 4096;

/// The nearest `.git` above `folder`: a folder marks a main work tree, a file a linked worktree
/// (its git folder lies in `<repo>/.git/worktrees/<name>`) or a submodule (then the submodule
/// counts as its own repository).
pub fn locate_repo(folder: &Path) -> Option<RepoLocation> {
    for dir in folder.ancestors().take(MAX_DEPTH) {
        let dot_git = dir.join(".git");
        let Ok(meta) = fs::symlink_metadata(&dot_git) else {
            continue;
        };
        if meta.is_dir() {
            return Some(RepoLocation {
                root: dir.to_path_buf(),
                repo: dir.to_path_buf(),
                worktree: false,
            });
        }
        // Bounded and without following links: this runs on every session start. A git folder on
        // another share than the folder itself is not followed (each access there would send
        // the user's NTLM credentials); the folder then counts as its own repository.
        let main = crate::persist::read_limited(&dot_git, MAX_DOT_GIT_BYTES)
            .ok()
            .flatten()
            .and_then(|content| crate::git::parse_gitdir_file(&String::from_utf8_lossy(&content)))
            .map(|git_dir| dir.join(git_dir))
            .filter(|git_dir| crate::git::gitdir_allowed(folder, git_dir))
            .and_then(|git_dir| main_of_worktree(&git_dir));
        return Some(RepoLocation {
            root: dir.to_path_buf(),
            repo: main.clone().unwrap_or_else(|| dir.to_path_buf()),
            worktree: main.is_some(),
        });
    }
    None
}

/// `<repo>/.git/worktrees/<name>` gives `<repo>`.
fn main_of_worktree(git_dir: &Path) -> Option<PathBuf> {
    let worktrees = git_dir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    let dot_git = worktrees.parent()?;
    if dot_git.file_name()? != ".git" {
        return None;
    }
    dot_git.parent().map(Path::to_path_buf)
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Days since 1970-01-01 (UTC) of a time in milliseconds.
pub fn day_number(ms: i64) -> i64 {
    ms.div_euclid(DAY_MS)
}

/// `YYYY-MM-DD.jsonl` of a day number.
pub fn file_name(day: i64) -> String {
    let (year, month, day) = civil_from_days(day);
    format!("{year:04}-{month:02}-{day:02}.jsonl")
}

/// The day number of a file name from [`file_name`]; `None` for anything else.
pub fn parse_file_name(name: &str) -> Option<i64> {
    let stem = name.strip_suffix(".jsonl")?;
    let bytes = stem.as_bytes();
    let shape = bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    if !shape {
        return None;
    }
    let year: i64 = stem.get(0..4)?.parse().ok()?;
    let month: u32 = stem.get(5..7)?.parse().ok()?;
    let day: u32 = stem.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let number = days_from_civil(year, month, day);
    // 2026-02-31 and the like do not round-trip.
    (civil_from_days(number) == (year, month, day)).then_some(number)
}

/// UTC time as `YYYY-MM-DDTHH:MM:SSZ`, the form git reads for `--since`.
pub fn iso_utc(seconds: i64) -> String {
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let rest = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

// Calendar arithmetic after Howard Hinnant's `days_from_civil` and `civil_from_days`
// (proleptic Gregorian calendar).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Both are in range by construction: day 1..=31, month 1..=12.
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month = i64::from(month);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    /// 2026-11-14T00:00:00Z.
    const DAY: i64 = 1_794_614_400_000;

    fn info(id: SessionId, cwd: &Path) -> SessionInfo {
        SessionInfo::new(
            id,
            "claude".into(),
            "Fix login".into(),
            cwd.to_path_buf(),
            0,
        )
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(file_name(0), "1970-01-01.jsonl");
        assert_eq!(file_name(day_number(DAY)), "2026-11-14.jsonl");
        assert_eq!(file_name(day_number(DAY - 1)), "2026-11-13.jsonl");
        assert_eq!(file_name(day_number(-1)), "1969-12-31.jsonl");
        for day in [-1, 0, 59, 60, 11_016, 20_406, 2_932_896] {
            assert_eq!(parse_file_name(&file_name(day)), Some(day), "{day}");
        }
        assert_eq!(parse_file_name("2024-02-29.jsonl"), Some(19_782));
        for bad in [
            "2026-02-31.jsonl",
            "2026-13-01.jsonl",
            "2026-1-01.jsonl",
            "2026-11-14.json",
            "notes.jsonl",
            "+026-11-14.jsonl",
        ] {
            assert_eq!(parse_file_name(bad), None, "{bad}");
        }
        assert_eq!(iso_utc(DAY / 1000 + 3723), "2026-11-14T01:02:03Z");
        assert_eq!(iso_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn a_link_message_names_the_member_target_and_time_but_no_text() {
        let tmp = TempDir::new("work-log-link-message");
        let log = WorkLog::with_run(tmp.path().join(DIR), "r1");
        let app = tmp.path().join("app");
        let web = tmp.path().join("web");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&web).unwrap();
        log.started(&info(1, &app), "Claude Code", DAY + 1000);
        log.started(&info(2, &web), "Codex CLI", DAY + 1000);
        log.link_message(1, 2, "app", DAY + 2000, DAY + 2000)
            .unwrap();
        for (member, at) in [
            ("App", DAY + 2000),
            ("a\nb", DAY + 2000),
            ("app", DAY + 2000 + FUTURE_SLACK_MS + 1),
            ("app", DAY + 2000 - MAX_PHASE_MS - 1),
        ] {
            assert!(
                matches!(
                    log.link_message(1, 2, member, at, DAY + 2000),
                    Err(WorkLogError::InvalidLinkMessage)
                ),
                "{member:?} {at}"
            );
        }
        assert!(matches!(
            log.link_message(1, 9, "app", DAY + 2000, DAY + 2000),
            Err(WorkLogError::UnknownSession(9))
        ));
        let (records, _) = log.read_window(DAY, DAY + DAY_MS);
        let line: Vec<&Record> = records
            .iter()
            .filter(|r| r.event == EventKind::LinkMessage)
            .collect();
        assert_eq!(line.len(), 1);
        assert_eq!(line[0].key, "r1-1");
        assert_eq!(line[0].member.as_deref(), Some("app"));
        assert_eq!(line[0].to_key.as_deref(), Some("r1-2"));
        let text = fs::read_to_string(log.dir().join("2026-11-14.jsonl")).unwrap();
        assert!(text.contains("\"event\":\"linkMessage\""), "{text}");
        assert!(text.contains("\"member\":\"app\""), "{text}");
    }

    #[test]
    fn a_message_names_sender_target_and_time_but_no_text() {
        let tmp = TempDir::new("work-log-message");
        let log = WorkLog::with_run(tmp.path().join(DIR), "r1");
        let app = tmp.path().join("app");
        let web = tmp.path().join("web");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&web).unwrap();
        log.started(&info(1, &app), "Claude Code", DAY + 1000);
        log.started(&info(2, &web), "Codex CLI", DAY + 1000);
        log.message(1, 2, DAY + 2000).unwrap();
        assert!(matches!(
            log.message(1, 9, DAY + 3000),
            Err(WorkLogError::UnknownSession(9))
        ));
        let (records, _) = log.read_window(DAY, DAY + DAY_MS);
        let message = records
            .iter()
            .find(|r| r.event == EventKind::Message)
            .unwrap();
        assert_eq!(message.key, "r1-1");
        assert_eq!(message.at, DAY + 2000);
        assert_eq!(message.to_key.as_deref(), Some("r1-2"));
        assert_eq!(
            message.to_cwd.as_deref(),
            Some(web.to_string_lossy().as_ref())
        );
        let text = fs::read_to_string(log.dir().join("2026-11-14.jsonl")).unwrap();
        let line = text.lines().find(|l| l.contains("\"message\"")).unwrap();
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "at", "cwd", "event", "key", "program", "repo", "title", "toCwd", "toKey", "v",
                "worktree"
            ]
        );
    }

    #[test]
    fn records_carry_the_session_and_never_more() {
        let tmp = TempDir::new("work-log-write");
        let cwd = tmp.path().join("project with space");
        fs::create_dir_all(cwd.join(".git")).unwrap();
        let log = WorkLog::with_run(tmp.path().join(DIR), "r1");
        let mut info = info(7, &cwd.join("src"));
        fs::create_dir_all(&info.cwd).unwrap();
        info.title = format!("Line\u{1b}[31m{}", "x".repeat(300));
        log.started(&info, "Claude Code", DAY + 1000);
        log.renamed(7, "Renamed");
        log.phase(7, DAY + 2000, DAY + 5000, DAY + 6000).unwrap();
        log.ended(7, DAY + 9000);
        // Exit, then close: the second end writes nothing.
        log.ended(7, DAY + 9500);
        // The window reports the phase the exit ended after the exit.
        log.phase(7, DAY + 8000, DAY + 9000, DAY + 9600).unwrap();

        let text = fs::read_to_string(log.dir().join("2026-11-14.jsonl")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        let (records, truncated) = log.read_window(DAY, DAY + DAY_MS);
        assert!(!truncated);
        assert_eq!(records.len(), 4);
        assert_eq!(records[3].event, EventKind::Phase);
        let start = &records[0];
        assert_eq!(start.event, EventKind::Start);
        assert_eq!(start.key, "r1-7");
        assert_eq!(start.program, "Claude Code");
        assert_eq!(start.title.chars().count(), MAX_TITLE_CHARS);
        assert!(!start.title.contains('\u{1b}'));
        assert_eq!(start.repo.as_deref(), Some(cwd.to_string_lossy().as_ref()));
        assert!(!start.worktree);
        assert_eq!(records[1].event, EventKind::Phase);
        assert_eq!(
            (records[1].from, records[1].at),
            (Some(DAY + 2000), DAY + 5000)
        );
        assert_eq!(records[1].title, "Renamed");
        assert_eq!(records[2].event, EventKind::End);
        // Only these fields exist; nothing else can slip in.
        let value: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        let mut keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["at", "cwd", "event", "from", "key", "program", "repo", "title", "v", "worktree"]
        );
        assert!(log.live_keys().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(log.dir()), 0o700);
            assert_eq!(mode(&log.dir().join("2026-11-14.jsonl")), 0o600);
        }
    }

    #[test]
    fn phases_are_checked() {
        let tmp = TempDir::new("work-log-phase");
        let log = WorkLog::with_run(tmp.path().join(DIR), "r");
        assert!(matches!(
            log.phase(1, DAY, DAY + 10, DAY + 20),
            Err(WorkLogError::UnknownSession(1))
        ));
        log.started(&info(1, tmp.path()), "Shell", DAY);
        for (from, to) in [
            (DAY + 10, DAY + 10),
            (DAY + 10, DAY + 5),
            (DAY, DAY + MAX_PHASE_MS + 1),
            (DAY, DAY + 10 * 60_000),
            // Ended long before now: it would land in an old day file.
            (DAY - 3 * DAY_MS, DAY - 3 * DAY_MS + 10),
            (i64::MIN, DAY),
        ] {
            assert!(
                matches!(
                    log.phase(1, from, to, DAY + 60_000),
                    Err(WorkLogError::InvalidPhase)
                ),
                "{from} {to}"
            );
        }
        assert_eq!(log.live_keys(), HashSet::from(["r-1".to_owned()]));
        log.end_all(DAY + 100);
        assert!(log.live_keys().is_empty());
        let (records, _) = log.read_window(DAY, DAY + DAY_MS);
        assert_eq!(
            records.iter().map(|r| r.event).collect::<Vec<_>>(),
            [EventKind::Start, EventKind::End]
        );
    }

    #[test]
    fn update_sessions_are_not_logged() {
        let tmp = TempDir::new("work-log-update");
        let log = WorkLog::with_run(tmp.path().join(DIR), "r");
        let mut update = info(2, tmp.path());
        update.kind = SessionKind::Update;
        log.started(&update, "npm", DAY);
        assert!(log.live_keys().is_empty());
        assert!(!log.dir().exists());
    }

    #[test]
    fn days_split_by_utc_and_the_window_reads_back() {
        let tmp = TempDir::new("work-log-days");
        let log = WorkLog::with_run(tmp.path().join(DIR), "r");
        log.started(&info(3, tmp.path()), "Shell", DAY - 5 * DAY_MS);
        log.phase(3, DAY - 100, DAY + 100, DAY + 200).unwrap();
        log.ended(3, DAY + DAY_MS + 5);
        assert!(log.dir().join("2026-11-09.jsonl").exists());
        assert!(log.dir().join("2026-11-14.jsonl").exists());
        assert!(log.dir().join("2026-11-15.jsonl").exists());
        // The window of the 14th sees its own file and three days back, not five.
        let (records, _) = log.read_window(DAY, DAY + DAY_MS);
        assert_eq!(
            records.iter().map(|r| r.event).collect::<Vec<_>>(),
            [EventKind::Phase]
        );
        let (records, _) = log.read_window(DAY + DAY_MS, DAY + 2 * DAY_MS);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn broken_lines_and_foreign_versions_are_skipped() {
        let tmp = TempDir::new("work-log-broken");
        let dir = tmp.path().join(DIR);
        fs::create_dir_all(&dir).unwrap();
        let good = r#"{"v":1,"event":"start","key":"k","at":1794614400001,"title":"t","program":"p","cwd":"/x","worktree":false}"#;
        let newer = good.replace(r#""v":1"#, r#""v":2"#);
        let long = format!(
            r#"{{"v":1,"event":"start","key":"{}","at":1794614400001,"title":"t","program":"p","cwd":"/x"}}"#,
            "k".repeat(MAX_LINE_BYTES)
        );
        let content = format!("{good}\nnot json\n{newer}\n{long}\n{{\"v\":1,\"event\":\"start\"");
        fs::write(dir.join("2026-11-14.jsonl"), content).unwrap();
        let log = WorkLog::with_run(dir, "r");
        let (records, truncated) = log.read_window(DAY, DAY + DAY_MS);
        assert!(!truncated);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].key, "k");
        assert_eq!(records[0].repo, None);
    }

    #[test]
    fn a_full_day_file_stops_growing() {
        let tmp = TempDir::new("work-log-full");
        let dir = tmp.path().join(DIR);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2026-11-14.jsonl");
        let filler = vec![b'\n'; usize::try_from(MAX_FILE_BYTES).unwrap() - 10];
        fs::write(&path, &filler).unwrap();
        let log = WorkLog::with_run(dir, "r");
        log.started(&info(4, tmp.path()), "Shell", DAY);
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_FILE_BYTES - 10);
    }

    #[test]
    fn prune_keeps_ninety_days_and_foreign_files() {
        let tmp = TempDir::new("work-log-prune");
        let dir = tmp.path().join(DIR);
        fs::create_dir_all(&dir).unwrap();
        let today = day_number(DAY);
        let kept = today - (RETENTION_DAYS - 1);
        for day in [today, kept, kept - 1, today - 400] {
            fs::write(dir.join(file_name(day)), "").unwrap();
        }
        fs::write(dir.join("readme.txt"), "").unwrap();
        WorkLog::with_run(dir.clone(), "r").prune(DAY + 1000);
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [file_name(kept), file_name(today), "readme.txt".into()]
        );
    }

    #[test]
    fn worktrees_belong_to_their_repository() {
        let tmp = TempDir::new("work-log-locate");
        let repo = tmp.path().join("main repo");
        let git_dir = repo.join(".git").join("worktrees").join("feature");
        fs::create_dir_all(&git_dir).unwrap();
        let linked = tmp.path().join("linked");
        fs::create_dir_all(linked.join("deep")).unwrap();
        fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", git_dir.to_string_lossy()),
        )
        .unwrap();
        assert_eq!(
            locate_repo(&linked.join("deep")),
            Some(RepoLocation {
                root: linked.clone(),
                repo: repo.clone(),
                worktree: true
            })
        );
        assert_eq!(
            locate_repo(&repo),
            Some(RepoLocation {
                root: repo.clone(),
                repo: repo.clone(),
                worktree: false
            })
        );
        // A submodule's git folder is no worktree: the submodule is its own repository.
        let sub = repo.join("vendor");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".git"), "gitdir: ../.git/modules/vendor\n").unwrap();
        assert_eq!(
            locate_repo(&sub),
            Some(RepoLocation {
                root: sub.clone(),
                repo: sub,
                worktree: false
            })
        );
    }

    /// A `.git` file in a local folder that names a git folder on a network share: the share is
    /// never taken as the repository (each access there would send the user's NTLM credentials).
    #[cfg(windows)]
    #[test]
    fn a_worktree_never_leads_to_a_network_share() {
        let tmp = TempDir::new("work-log-unc");
        let linked = tmp.path().join("linked");
        fs::create_dir_all(&linked).unwrap();
        fs::write(
            linked.join(".git"),
            r"gitdir: \\example.invalid\share\.git\worktrees\feature",
        )
        .unwrap();
        assert_eq!(
            locate_repo(&linked),
            Some(RepoLocation {
                root: linked.clone(),
                repo: linked,
                worktree: false
            })
        );
    }
}
