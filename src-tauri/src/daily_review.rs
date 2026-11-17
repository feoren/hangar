//! The daily review (ADR 0011): the work log of one local day merged per session and per
//! project, plus the commits of that day and what is uncommitted right now.
//!
//! Times: a session's run time is the union of its process runs within the day, its active
//! time the union of its working phases. A project's and the day's times are unions again, so
//! two agents working side by side for an hour count as one hour of the project's time, not two.
//!
//! git runs through the hardened [`Git`] runner (argv only, no prompts, no pager, no transports,
//! a time limit per call), with a total budget for the whole review. `git log` asks only for
//! names, never for content: no diff driver, textconv or filter can run. Commits are the user's
//! own (`user.email` of the repository); a repository without one shows no commits, because
//! `--remotes` would otherwise bring in everybody else's work of the day (like git_activity.rs).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::git::{self, Git};
use crate::work_log::{self, EventKind, Record, WorkLog, DAY_MS};

/// A day is 23 to 25 hours long (daylight saving time); a little margin on top.
const MAX_WINDOW_MS: i64 = 26 * 60 * 60 * 1000;
/// Projects whose repository is read; the rest show their sessions only.
pub const MAX_GIT_PROJECTS: usize = 24;
pub const MAX_COMMITS: usize = 200;
/// File names listed per commit and per project.
pub const MAX_COMMIT_FILES: usize = 100;
pub const MAX_PROJECT_FILES: usize = 300;
/// Work trees per project whose uncommitted changes are read, and files listed per tree.
const MAX_TREES: usize = 8;
pub const MAX_UNCOMMITTED: usize = 200;
const MAX_SUBJECT_CHARS: usize = 200;
const MAX_EMAIL_BYTES: usize = 4096;
/// All git calls of one review together.
const TOTAL_TIME: Duration = Duration::from_secs(20);
/// Commits are filtered by author time; git's `--until` compares the commit time, which can lie
/// later (amend, rebase). The output is filtered again, so this only has to be generous.
const UNTIL_MARGIN_S: i64 = 2 * 86_400;

#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("Invalid day: {0}")]
    InvalidDay(&'static str),
    #[error("Task failed: {0}")]
    Task(String),
}

impl Serialize for ReviewError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReview {
    pub start: i64,
    pub end: i64,
    /// Time in which at least one session worked.
    pub active_ms: i64,
    /// Time in which at least one session ran.
    pub run_ms: i64,
    pub session_count: u32,
    pub commit_count: u32,
    /// Most active first.
    pub projects: Vec<ProjectReview>,
    /// A limit was reached (log size, commits, files or git's time budget); something is missing.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReview {
    /// Repository root, or the working folder outside a repository.
    pub path: String,
    pub is_repo: bool,
    pub active_ms: i64,
    pub run_ms: i64,
    /// By first start.
    pub sessions: Vec<SessionReview>,
    /// Newest first.
    pub commits: Vec<CommitReview>,
    /// Files touched by the day's commits, sorted, at most [`MAX_PROJECT_FILES`].
    pub files: Vec<String>,
    pub file_count: u32,
    /// Only for a day that includes now: the work trees with uncommitted changes.
    pub uncommitted: Vec<TreeChanges>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReview {
    pub key: String,
    pub title: String,
    pub program: String,
    pub cwd: String,
    pub worktree: bool,
    /// First and last moment within the day (ms).
    pub first: i64,
    pub last: i64,
    pub active_ms: i64,
    pub run_ms: i64,
    /// Still running at the end of the covered time.
    pub running: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReview {
    pub hash: String,
    pub subject: String,
    /// Author time in ms.
    pub time: i64,
    pub files: Vec<String>,
    pub file_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeChanges {
    pub path: String,
    pub worktree: bool,
    pub files: Vec<String>,
    pub count: u32,
}

/// A local day as the window computed it: `[start, end)` in ms. It must be a plausible day
/// within the log's retention, so a call cannot make Hangar read years of history.
pub fn check_window(start: i64, end: i64, now: i64) -> Result<(), ReviewError> {
    // `end - start` overflows for far apart values the window could send.
    if end <= start
        || end
            .checked_sub(start)
            .is_none_or(|length| length > MAX_WINDOW_MS)
    {
        return Err(ReviewError::InvalidDay("not one day"));
    }
    if start < now - (work_log::RETENTION_DAYS + 2) * DAY_MS || start > now + DAY_MS {
        return Err(ReviewError::InvalidDay("outside the kept days"));
    }
    Ok(())
}

/// Merges the log's records into sessions and projects of `[start, end)`. `live`: keys of the
/// sessions running now; their open run lasts until `now`, an open run of any other session
/// (a crash) ends at its last record.
pub fn merge(
    records: &[Record],
    start: i64,
    end: i64,
    now: i64,
    live: &HashSet<String>,
) -> DailyReview {
    let mut by_key: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
    // A cross-project message is no work of its own; it neither opens nor extends a run.
    for record in records.iter().filter(|r| r.event != EventKind::Message) {
        by_key.entry(record.key.as_str()).or_default().push(record);
    }
    let mut projects: BTreeMap<String, ProjectBuild> = BTreeMap::new();
    for (key, mut list) in by_key {
        list.sort_by_key(|record| (record.at, event_order(record.event)));
        let Some(session) = SessionBuild::from_records(&list, now, live.contains(key)) else {
            continue;
        };
        let runs = clip_all(&session.runs, start, end);
        let phases = clip_all(&session.phases, start, end);
        let covered = union(runs.iter().chain(&phases).copied());
        if covered.is_empty() {
            continue;
        }
        let Some(latest) = list.last() else { continue };
        let (path, is_repo) = match &latest.repo {
            Some(repo) => (repo.clone(), true),
            None => (latest.cwd.clone(), false),
        };
        let first = covered.first().map_or(start, |span| span.0);
        let last = covered.last().map_or(start, |span| span.1);
        let review = SessionReview {
            key: key.to_owned(),
            title: latest.title.clone(),
            program: latest.program.clone(),
            cwd: latest.cwd.clone(),
            worktree: latest.worktree,
            first,
            last,
            active_ms: total(&union(phases.iter().copied())),
            run_ms: total(&covered),
            running: session.open && start <= now && now < end,
        };
        let project = projects
            .entry(git::path_key(Path::new(&path)))
            .or_insert_with(|| ProjectBuild::new(path, is_repo));
        project.runs.extend(covered);
        project.phases.extend(phases);
        project.sessions.push(review);
    }

    let mut review = DailyReview {
        start,
        end,
        ..DailyReview::default()
    };
    let mut all_runs = Vec::new();
    let mut all_phases = Vec::new();
    for (_, build) in projects {
        all_runs.extend(build.runs.iter().copied());
        all_phases.extend(build.phases.iter().copied());
        review.projects.push(build.finish());
    }
    review.projects.sort_by(|a, b| {
        b.active_ms
            .cmp(&a.active_ms)
            .then(b.run_ms.cmp(&a.run_ms))
            .then(a.path.cmp(&b.path))
    });
    review.active_ms = total(&union(all_phases));
    review.run_ms = total(&union(all_runs));
    review.session_count = count(review.projects.iter().map(|p| p.sessions.len()).sum());
    review
}

/// The whole review of `[start, end)`: the log merged, then git where it is installed.
pub fn review(
    log: &WorkLog,
    git: Option<&Git>,
    start: i64,
    end: i64,
    now: i64,
) -> Result<DailyReview, ReviewError> {
    check_window(start, end, now)?;
    let (records, truncated) = log.read_window(start, end);
    let mut review = merge(&records, start, end, now, &log.live_keys());
    review.truncated |= truncated;
    if let Some(git) = git {
        add_git(git, &mut review, now);
    }
    Ok(review)
}

/// Commits of the day and, for a day that includes now, the uncommitted changes, for every
/// project that is a repository (at most [`MAX_GIT_PROJECTS`], most active first).
pub fn add_git(git: &Git, review: &mut DailyReview, now: i64) {
    let total_end = Instant::now() + TOTAL_TIME;
    let today = review.start <= now && now < review.end;
    let (start, end) = (review.start, review.end);
    for project in review
        .projects
        .iter_mut()
        .filter(|p| p.is_repo && git_reachable(p))
        .take(MAX_GIT_PROJECTS)
    {
        if Instant::now() >= total_end {
            review.truncated = true;
            break;
        }
        let deadline = total_end.min(Instant::now() + git::TIMEOUT);
        let root = PathBuf::from(&project.path);
        if let Some((commits, cut)) = commits(git, &root, start, end, deadline) {
            review.truncated |= cut;
            set_commits(project, commits);
        }
        if today {
            let mut trees: Vec<(PathBuf, bool)> = Vec::new();
            for session in &project.sessions {
                let Some(location) = work_log::locate_repo(Path::new(&session.cwd)) else {
                    continue;
                };
                if !trees
                    .iter()
                    .any(|(tree, _)| git::path_key(tree) == git::path_key(&location.root))
                {
                    trees.push((location.root, location.worktree));
                }
            }
            for (tree, worktree) in trees.into_iter().take(MAX_TREES) {
                let deadline = total_end.min(Instant::now() + git::TIMEOUT);
                if let Some(changes) = uncommitted(git, &tree, worktree, deadline) {
                    if changes.count > 0 {
                        project.uncommitted.push(changes);
                    }
                }
            }
        }
    }
    review.commit_count = count(review.projects.iter().map(|p| p.commits.len()).sum());
}

/// Whether git may run for the project. The log names the repository as found when a session
/// started, possibly by an older Hangar; a repository on a network share is only asked when one
/// of the project's sessions worked on that same share, never because a local folder's `.git`
/// file pointed there (on Windows each access sends the user's NTLM credentials to that host).
fn git_reachable(project: &ProjectReview) -> bool {
    let root = Path::new(&project.path);
    !git::is_remote(root)
        || project
            .sessions
            .iter()
            .any(|session| git::gitdir_allowed(Path::new(&session.cwd), root))
}

fn set_commits(project: &mut ProjectReview, commits: Vec<CommitReview>) {
    let mut files = BTreeSet::new();
    for commit in &commits {
        files.extend(commit.files.iter().cloned());
    }
    project.file_count = count(files.len());
    project.files = files.into_iter().take(MAX_PROJECT_FILES).collect();
    project.commits = commits;
}

/// The day's commits in the repository at `root`, newest first, and whether the list was cut.
/// `None` when the folder is no usable repository, has no `user.email` or git fails.
pub fn commits(
    git: &Git,
    root: &Path,
    start: i64,
    end: i64,
    deadline: Instant,
) -> Option<(Vec<CommitReview>, bool)> {
    git::check_folder(root).ok()?;
    let top = git::toplevel(git, root, deadline).ok().flatten()?;
    let email = own_email(git, &top, deadline)?;
    let since = format!("--since={}", work_log::iso_utc(start.div_euclid(1000)));
    let until = format!(
        "--until={}",
        work_log::iso_utc(end.div_euclid(1000) + UNTIL_MARGIN_S)
    );
    // `-F -i`: the address is a fixed string, its case does not matter; the angle brackets
    // match the whole address, not a longer one that ends with it.
    let author = format!("--author=<{email}>");
    let args = [
        "-c",
        "core.quotePath=false",
        "log",
        "--branches",
        "--tags",
        "--remotes",
        "--no-merges",
        "--no-show-signature",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--name-only",
        "--format=%x1e%H%x1f%at%x1f%s",
        "--fixed-strings",
        "--regexp-ignore-case",
        author.as_str(),
        since.as_str(),
        until.as_str(),
    ];
    let output = git
        .run(&top, args, deadline, git::MAX_LIST_BYTES)
        .ok()
        .filter(git::Output::usable)?;
    let mut list: Vec<CommitReview> = parse_log(&output.stdout, output.truncated)
        .into_iter()
        .filter(|commit| commit.time >= start && commit.time < end)
        .collect();
    // The same commit on several branches is listed once by git already; worktrees share it.
    list.sort_by(|a, b| b.time.cmp(&a.time).then(a.hash.cmp(&b.hash)));
    let cut = output.truncated || list.len() > MAX_COMMITS;
    list.truncate(MAX_COMMITS);
    Some((list, cut))
}

/// `git log --name-only --format=%x1e%H%x1f%at%x1f%s`: per commit a record that starts with
/// 0x1E, the header line, an empty line and the file names. A cut output loses its last record.
pub fn parse_log(output: &[u8], truncated: bool) -> Vec<CommitReview> {
    let text = String::from_utf8_lossy(output);
    let mut records: Vec<&str> = text.split('\x1e').skip(1).collect();
    if truncated {
        records.pop();
    }
    records
        .into_iter()
        .filter_map(|record| {
            let mut lines = record.split('\n').map(|line| line.trim_end_matches('\r'));
            let mut header = lines.next()?.splitn(3, '\x1f');
            let hash = header.next()?;
            let time = header.next()?.parse::<i64>().ok()?;
            let subject: String = header
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_SUBJECT_CHARS)
                .collect();
            if hash.len() < 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let names: Vec<String> = lines
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect();
            Some(CommitReview {
                hash: hash.to_ascii_lowercase(),
                subject,
                time: time.saturating_mul(1000),
                file_count: count(names.len()),
                files: names.into_iter().take(MAX_COMMIT_FILES).collect(),
            })
        })
        .collect()
}

/// The repository's `user.email`, or `None` when none is configured or it is unusable.
fn own_email(git: &Git, root: &Path, deadline: Instant) -> Option<String> {
    let output = git
        .run(
            root,
            ["config", "--get", "user.email"],
            deadline,
            MAX_EMAIL_BYTES,
        )
        .ok()
        .filter(|output| output.success && !output.truncated)?;
    let email = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!email.is_empty() && !email.contains(['\n', '\r', '\0'])).then_some(email)
}

/// The changed files of the work tree at `tree` right now (`git status`, untracked included).
fn uncommitted(git: &Git, tree: &Path, worktree: bool, deadline: Instant) -> Option<TreeChanges> {
    git::check_folder(tree).ok()?;
    let root = git::toplevel(git, tree, deadline).ok().flatten()?;
    let neutral = git::filter_overrides(git, &root, deadline).ok()?;
    let (files, _truncated) = git::status_files(git, &root, &neutral, deadline).ok()?;
    Some(TreeChanges {
        path: root.to_string_lossy().into_owned(),
        worktree,
        count: count(files.len()),
        files: files
            .into_iter()
            .take(MAX_UNCOMMITTED)
            .map(|file| file.path)
            .collect(),
    })
}

/// Ends before starts at the same moment, so a restart joins its runs.
fn event_order(event: EventKind) -> u8 {
    match event {
        EventKind::End => 0,
        EventKind::Phase => 1,
        EventKind::Start => 2,
        EventKind::Message => 3,
    }
}

type Span = (i64, i64);

struct SessionBuild {
    runs: Vec<Span>,
    phases: Vec<Span>,
    /// The last run has not ended.
    open: bool,
}

impl SessionBuild {
    /// `list` sorted by time. A phase without a start before it (the start lies in a file that
    /// was not read) opens the run at the phase's start.
    fn from_records(list: &[&Record], now: i64, live: bool) -> Option<Self> {
        let mut runs = Vec::new();
        let mut phases = Vec::new();
        let mut open: Option<i64> = None;
        let mut last_seen = i64::MIN;
        for record in list {
            match record.event {
                EventKind::Start => {
                    open.get_or_insert(record.at);
                }
                EventKind::Phase => {
                    let from = record.from.unwrap_or(record.at).min(record.at);
                    phases.push((from, record.at));
                    open.get_or_insert(from);
                }
                EventKind::Message => continue,
                EventKind::End => {
                    // An end without a start (start older than the files read) ends a run that
                    // began before them; the clip to the day keeps only the day's part.
                    let began = open.take().unwrap_or(i64::MIN / 2);
                    if record.at > began {
                        runs.push((began, record.at));
                    }
                }
            }
            last_seen = last_seen.max(record.at);
        }
        let still_open = open.is_some();
        if let Some(began) = open {
            let until = if live { now.max(last_seen) } else { last_seen };
            if until > began {
                runs.push((began, until));
            }
        }
        (!runs.is_empty() || !phases.is_empty()).then_some(Self {
            runs,
            phases,
            open: still_open && live,
        })
    }
}

struct ProjectBuild {
    path: String,
    is_repo: bool,
    runs: Vec<Span>,
    phases: Vec<Span>,
    sessions: Vec<SessionReview>,
}

impl ProjectBuild {
    fn new(path: String, is_repo: bool) -> Self {
        Self {
            path,
            is_repo,
            runs: Vec::new(),
            phases: Vec::new(),
            sessions: Vec::new(),
        }
    }

    fn finish(mut self) -> ProjectReview {
        self.sessions
            .sort_by(|a, b| a.first.cmp(&b.first).then(a.key.cmp(&b.key)));
        ProjectReview {
            path: self.path,
            is_repo: self.is_repo,
            active_ms: total(&union(self.phases)),
            run_ms: total(&union(self.runs)),
            sessions: self.sessions,
            ..ProjectReview::default()
        }
    }
}

fn clip_all(spans: &[Span], start: i64, end: i64) -> Vec<Span> {
    spans
        .iter()
        .map(|&(from, to)| (from.max(start), to.min(end)))
        .filter(|(from, to)| to > from)
        .collect()
}

/// Sorted, non-overlapping spans covering the same time; touching spans join.
fn union(spans: impl IntoIterator<Item = Span>) -> Vec<Span> {
    let mut spans: Vec<Span> = spans.into_iter().filter(|(a, b)| b > a).collect();
    spans.sort_unstable();
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for (from, to) in spans {
        match merged.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    merged
}

fn total(spans: &[Span]) -> i64 {
    spans.iter().map(|(from, to)| to - from).sum()
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{commit_dated, git, git_ok, init_repo};
    use crate::test_support::TempDir;
    use std::collections::HashMap;

    /// 2026-11-14T00:00:00Z; the "local day" in these tests is UTC.
    const DAY: i64 = 1_794_614_400_000;
    const MIN: i64 = 60_000;
    const HOUR: i64 = 60 * MIN;

    fn rec(event: EventKind, key: &str, at: i64, from: Option<i64>, repo: &str) -> Record {
        Record {
            v: 1,
            event,
            key: key.into(),
            at,
            from,
            title: format!("title {key}"),
            program: "Claude Code".into(),
            cwd: format!("{repo}/src"),
            repo: Some(repo.into()),
            worktree: false,
            to_key: None,
            to_cwd: None,
        }
    }

    fn start(key: &str, at: i64, repo: &str) -> Record {
        rec(EventKind::Start, key, at, None, repo)
    }
    fn phase(key: &str, from: i64, to: i64, repo: &str) -> Record {
        rec(EventKind::Phase, key, to, Some(from), repo)
    }
    fn end(key: &str, at: i64, repo: &str) -> Record {
        rec(EventKind::End, key, at, None, repo)
    }

    #[test]
    fn messages_add_no_time() {
        let a = "/work/alpha";
        let base = vec![
            start("r-1", DAY + 9 * HOUR, a),
            end("r-1", DAY + 10 * HOUR, a),
        ];
        let mut with = base.clone();
        // Sent after the end, by a session the window never saw start: still no run.
        with.push(rec(EventKind::Message, "r-1", DAY + 11 * HOUR, None, a));
        with.push(rec(EventKind::Message, "r-9", DAY + 12 * HOUR, None, a));
        let review = |records: &[Record]| {
            merge(records, DAY, DAY + DAY_MS, DAY + 20 * HOUR, &HashSet::new())
        };
        let (plain, sent) = (review(&base), review(&with));
        assert_eq!(sent.session_count, 1);
        assert_eq!(sent.projects[0].run_ms, plain.projects[0].run_ms);
        assert_eq!(sent.projects[0].run_ms, HOUR);
    }

    #[test]
    fn union_joins_overlaps_and_touching_spans() {
        assert_eq!(
            union([(5, 9), (1, 3), (3, 4), (8, 12), (20, 20)]),
            [(1, 4), (5, 12)]
        );
        assert_eq!(total(&union([(0, 10), (2, 5)])), 10);
        assert!(union([]).is_empty());
    }

    #[test]
    fn sessions_and_projects_take_unions_of_their_time() {
        let a = "/work/alpha";
        let b = "/work/beta";
        let records = vec![
            // Two agents side by side in alpha: 9:00 to 11:00 and 10:00 to 12:00.
            start("r-1", DAY + 9 * HOUR, a),
            phase("r-1", DAY + 9 * HOUR, DAY + 9 * HOUR + 30 * MIN, a),
            phase("r-1", DAY + 10 * HOUR, DAY + 10 * HOUR + 30 * MIN, a),
            end("r-1", DAY + 11 * HOUR, a),
            start("r-2", DAY + 10 * HOUR, a),
            phase(
                "r-2",
                DAY + 10 * HOUR + 15 * MIN,
                DAY + 10 * HOUR + 45 * MIN,
                a,
            ),
            end("r-2", DAY + 12 * HOUR, a),
            // Beta: one shell, never working.
            start("r-3", DAY + 14 * HOUR, b),
            end("r-3", DAY + 14 * HOUR + 20 * MIN, b),
        ];
        let review = merge(
            &records,
            DAY,
            DAY + DAY_MS,
            DAY + 20 * HOUR,
            &HashSet::new(),
        );
        assert_eq!(review.session_count, 3);
        assert_eq!(review.projects.len(), 2);
        let alpha = &review.projects[0];
        assert_eq!(alpha.path, a);
        assert!(alpha.is_repo);
        assert_eq!(alpha.run_ms, 3 * HOUR);
        // 9:00-9:30 plus 10:00-10:45 (the phases of both overlap).
        assert_eq!(alpha.active_ms, 75 * MIN);
        assert_eq!(
            alpha
                .sessions
                .iter()
                .map(|s| (s.active_ms, s.run_ms))
                .collect::<Vec<_>>(),
            [(60 * MIN, 2 * HOUR), (30 * MIN, 2 * HOUR)]
        );
        assert_eq!(alpha.sessions[0].first, DAY + 9 * HOUR);
        assert_eq!(alpha.sessions[0].last, DAY + 11 * HOUR);
        assert_eq!(alpha.sessions[0].title, "title r-1");
        assert!(!alpha.sessions[0].running);
        let beta = &review.projects[1];
        assert_eq!((beta.active_ms, beta.run_ms), (0, 20 * MIN));
        assert_eq!(review.active_ms, 75 * MIN);
        assert_eq!(review.run_ms, 3 * HOUR + 20 * MIN);
    }

    #[test]
    fn midnight_restarts_crashes_and_live_sessions() {
        let a = "/work/alpha";
        let records = vec![
            // Started the evening before, phase across midnight, ended in the morning.
            start("r-1", DAY - 2 * HOUR, a),
            phase("r-1", DAY - 30 * MIN, DAY + 30 * MIN, a),
            end("r-1", DAY + HOUR, a),
            // Restarted: two runs with a pause between.
            start("r-2", DAY + 2 * HOUR, a),
            end("r-2", DAY + 3 * HOUR, a),
            start("r-2", DAY + 4 * HOUR, a),
            end("r-2", DAY + 5 * HOUR, a),
            // Crashed: no end, not live; the run ends at its last record.
            start("old-3", DAY + 6 * HOUR, a),
            phase(
                "old-3",
                DAY + 6 * HOUR + 10 * MIN,
                DAY + 6 * HOUR + 40 * MIN,
                a,
            ),
            // Live: runs until now.
            start("r-4", DAY + 8 * HOUR, a),
            // Only a phase: its start lies in a file that was not read.
            phase("r-5", DAY + 7 * HOUR, DAY + 7 * HOUR + 5 * MIN, a),
            // The day after: nothing of it counts.
            start("r-6", DAY + DAY_MS + HOUR, a),
        ];
        let now = DAY + 9 * HOUR;
        let live = HashSet::from(["r-4".to_owned()]);
        let review = merge(&records, DAY, DAY + DAY_MS, now, &live);
        let sessions = &review.projects[0].sessions;
        let by_key: HashMap<&str, &SessionReview> =
            sessions.iter().map(|s| (s.key.as_str(), s)).collect();
        assert_eq!(sessions.len(), 5);
        let first = by_key["r-1"];
        assert_eq!(
            (first.first, first.run_ms, first.active_ms),
            (DAY, HOUR, 30 * MIN)
        );
        assert_eq!(by_key["r-2"].run_ms, 2 * HOUR);
        assert_eq!(by_key["old-3"].run_ms, 40 * MIN);
        assert!(!by_key["old-3"].running);
        assert_eq!(by_key["r-4"].run_ms, HOUR);
        assert!(by_key["r-4"].running);
        assert_eq!(by_key["r-5"].run_ms, 5 * MIN);
        // A past day: the live session's open run ends at that day's end.
        let yesterday = merge(&records, DAY - DAY_MS, DAY, now, &live);
        assert_eq!(yesterday.projects[0].sessions.len(), 1);
        assert_eq!(yesterday.projects[0].sessions[0].run_ms, 2 * HOUR);
    }

    #[test]
    fn folders_outside_a_repository_are_their_own_project() {
        let mut shell = start("r-1", DAY + HOUR, "/x");
        shell.repo = None;
        shell.cwd = "/home/me/scratch".into();
        let review = merge(
            &[shell],
            DAY,
            DAY + DAY_MS,
            DAY + 2 * HOUR,
            &HashSet::from(["r-1".into()]),
        );
        assert_eq!(review.projects[0].path, "/home/me/scratch");
        assert!(!review.projects[0].is_repo);
        assert_eq!(review.projects[0].run_ms, HOUR);
    }

    #[test]
    fn windows_are_checked() {
        let now = DAY + 10 * HOUR;
        assert!(check_window(DAY, DAY + DAY_MS, now).is_ok());
        assert!(check_window(DAY, DAY + 25 * HOUR, now).is_ok());
        assert!(check_window(DAY, DAY, now).is_err());
        assert!(check_window(DAY, DAY + 27 * HOUR, now).is_err());
        assert!(check_window(DAY - 200 * DAY_MS, DAY - 199 * DAY_MS, now).is_err());
        assert!(check_window(DAY + 3 * DAY_MS, DAY + 4 * DAY_MS, now).is_err());
        // Far apart values neither overflow nor pass.
        assert!(check_window(i64::MIN, i64::MAX, now).is_err());
        assert!(check_window(i64::MIN, now, now).is_err());
        assert!(check_window(now, i64::MAX, now).is_err());
    }

    #[test]
    fn log_records() {
        let a = "0123456789abcdef0123456789abcdef01234567";
        let b = "89ABCDEF0123456789abcdef0123456789abcdef";
        let output = format!(
            "\x1e{a}\x1f1794650400\x1ffeat: add\x1fpart\n\nsrc/a.rs\nsrc/b c.rs\n\
             \x1e{b}\x1f1794646800\x1fempty\n\x1ebad\x1f1\x1fx\n\x1e{a}\x1f17946"
        );
        let commits = parse_log(output.as_bytes(), true);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "feat: add\x1fpart".replace('\x1f', ""));
        assert_eq!(commits[0].files, ["src/a.rs", "src/b c.rs"]);
        assert_eq!(commits[0].time, 1_794_650_400_000);
        assert_eq!(commits[1].hash, b.to_ascii_lowercase());
        assert!(commits[1].files.is_empty());
    }

    fn commit_file(git: &Git, repo: &Path, file: &str, author: &str, date: &str) {
        std::fs::write(repo.join(file), date).unwrap();
        git_ok(git, repo, &["add", "--", file]);
        commit_dated(
            git,
            repo,
            &[
                "-m",
                &format!("change {file}"),
                &format!("--author={author}"),
            ],
            date,
        );
    }

    #[test]
    fn commits_of_the_day_with_their_files_and_uncommitted_changes() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("daily-review-git");
        let repo = tmp.path().join("repo with space");
        init_repo(&git, &repo);
        git_ok(
            &git,
            &repo,
            &["config", "user.email", "test@example.invalid"],
        );
        let me = "Hangar Test <test@example.invalid>";
        commit_file(&git, &repo, "before.txt", me, "2026-11-13T23:00:00Z");
        commit_file(&git, &repo, "a.txt", me, "2026-11-14T09:00:00Z");
        commit_file(
            &git,
            &repo,
            "b.txt",
            "Other <other@example.invalid>",
            "2026-11-14T10:00:00Z",
        );
        commit_file(&git, &repo, "c.txt", me, "2026-11-14T11:00:00Z");
        commit_file(&git, &repo, "after.txt", me, "2026-11-15T01:00:00Z");
        std::fs::write(repo.join("open.txt"), "not yet").unwrap();

        let deadline = Instant::now() + git::TIMEOUT;
        let (list, cut) = commits(&git, &repo, DAY, DAY + DAY_MS, deadline).unwrap();
        assert!(!cut);
        assert_eq!(
            list.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
            ["change c.txt", "change a.txt"]
        );
        assert_eq!(list[0].files, ["c.txt"]);

        let records = vec![
            start("r-1", DAY + 8 * HOUR, &repo.to_string_lossy()),
            end("r-1", DAY + 12 * HOUR, &repo.to_string_lossy()),
        ];
        let mut review = merge(
            &records,
            DAY,
            DAY + DAY_MS,
            DAY + 13 * HOUR,
            &HashSet::new(),
        );
        // The session ran in `<repo>/src`; its work tree is the repository.
        std::fs::create_dir_all(repo.join("src")).unwrap();
        add_git(&git, &mut review, DAY + 13 * HOUR);
        assert_eq!(review.commit_count, 2);
        let project = &review.projects[0];
        assert_eq!(project.files, ["a.txt", "c.txt"]);
        assert_eq!(project.file_count, 2);
        assert_eq!(project.uncommitted.len(), 1);
        assert_eq!(project.uncommitted[0].files, ["open.txt"]);
        assert!(!project.uncommitted[0].worktree);

        // A past day shows no uncommitted changes: they are of now, not of then.
        let mut past = merge(
            &records,
            DAY,
            DAY + DAY_MS,
            DAY + 3 * DAY_MS,
            &HashSet::new(),
        );
        add_git(&git, &mut past, DAY + 3 * DAY_MS);
        assert!(past.projects[0].uncommitted.is_empty());
        assert_eq!(past.commit_count, 2);
    }

    #[test]
    fn a_repository_without_an_address_shows_no_commits() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("daily-review-no-email");
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        git_ok(
            &git,
            &repo,
            &["config", "user.email", "test@example.invalid"],
        );
        commit_file(
            &git,
            &repo,
            "theirs.txt",
            "Other <other@example.invalid>",
            "2026-11-14T10:00:00Z",
        );
        // An empty local value hides any global one: no address, so nobody's commits are the
        // user's, and fetched work of others must not show up as the user's day.
        git_ok(&git, &repo, &["config", "user.email", ""]);
        let deadline = Instant::now() + git::TIMEOUT;
        assert!(commits(&git, &repo, DAY, DAY + DAY_MS, deadline).is_none());
    }

    #[test]
    fn a_folder_that_is_no_repository_has_no_commits() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("daily-review-none");
        let deadline = Instant::now() + git::TIMEOUT;
        assert!(commits(
            &git,
            &tmp.path().join("missing"),
            DAY,
            DAY + DAY_MS,
            deadline
        )
        .is_none());
        assert!(commits(&git, Path::new("relative"), DAY, DAY + DAY_MS, deadline).is_none());
    }

    fn project(path: &str, cwds: &[&str]) -> ProjectReview {
        ProjectReview {
            path: path.into(),
            is_repo: true,
            sessions: cwds
                .iter()
                .map(|cwd| SessionReview {
                    cwd: (*cwd).into(),
                    ..SessionReview::default()
                })
                .collect(),
            ..ProjectReview::default()
        }
    }

    /// A logged repository on a network share is only asked when a session worked on that
    /// share; a local session never makes Hangar reach out to it (NTLM).
    #[cfg(windows)]
    #[test]
    fn a_share_is_only_asked_for_sessions_on_it() {
        let share = r"\\example.invalid\share";
        assert!(!git_reachable(&project(share, &[r"C:\work\linked"])));
        assert!(!git_reachable(&project(share, &[])));
        assert!(git_reachable(&project(
            share,
            &[r"\\example.invalid\share\src"]
        )));
        assert!(git_reachable(&project(
            share,
            &[r"C:\work", r"\\EXAMPLE.invalid\share\src"]
        )));
        assert!(!git_reachable(&project(share, &[r"\\other.invalid\share"])));
        assert!(git_reachable(&project(r"C:\work\repo", &[r"C:\work\repo"])));
    }

    #[test]
    fn a_local_repository_is_always_asked() {
        assert!(git_reachable(&project(
            "/home/me/repo",
            &["/home/me/repo/src"]
        )));
        assert!(git_reachable(&project("/home/me/repo", &[])));
    }
}
