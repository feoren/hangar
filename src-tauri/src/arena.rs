//! Agent arena: one task for 2 to 4 agents, each in its own worktree from the same commit
//! (contract: docs/wiki/pages/syntheses/agent-arena.md).
//!
//! The backend owns everything that touches git and processes: it creates the worktrees (branch
//! `hangar/arena-<id>-<agent>`), measures each contestant's changes against the shared base
//! commit, runs the user's test command in a contestant's worktree, takes the winner's changes
//! over into the project folder as uncommitted changes, and discards a losing worktree on request.
//! Nothing here commits, and taking over refuses files the project folder has changed itself.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};

use crate::batch::{self, UnsafeBatch};
use crate::git::{self, Git, GitError};
use crate::proctree::ProcTree;
use crate::worktree::{self, Worktree, WorktreeError, BRANCH_PREFIX};

pub const MIN_CONTESTANTS: usize = 2;
pub const MAX_CONTESTANTS: usize = 4;
/// Slug part before the arena id: `arena-1a2b3c-claude-code`.
pub const SLUG_PREFIX: &str = "arena-";
const ID_CHARS: usize = 6;
/// Agent part of the slug; long program names are cut so the slug stays readable.
const MAX_AGENT_CHARS: usize = 24;
/// Creating several worktrees, measuring and taking over can take a while on large trees.
const GIT_TIMEOUT: Duration = Duration::from_secs(120);
/// The patch of a winner; anything larger is refused instead of taken over half.
pub const MAX_PATCH_BYTES: usize = 64 * 1024 * 1024;
/// New files whose lines are counted; beyond this the list is marked as cut.
const MAX_COUNTED_FILES: usize = 2000;
/// New files larger than this count as binary (no lines).
const MAX_COUNTED_BYTES: usize = 8 * 1024 * 1024;
/// A test run that takes longer is stopped together with everything it started.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Output kept per stream, from the end, for the comparison view.
const TAIL_BYTES: usize = 4000;
/// How long to wait for the remaining output after the test process tree was ended.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, thiserror::Error)]
pub enum ArenaError {
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("An arena needs {MIN_CONTESTANTS} to {MAX_CONTESTANTS} agents")]
    Count,
    #[error("The repository has no commit yet; an arena starts from the current commit")]
    NoCommit,
    #[error("This worktree does not belong to an arena")]
    NotArena,
    #[error("The contestant's worktree no longer exists")]
    Missing,
    #[error("The contestant changed nothing")]
    NothingToApply,
    #[error("The changes are too large to take over")]
    TooLarge,
    #[error("git could not apply the changes: {0}")]
    Apply(String),
    #[error("{path} cannot be taken over: {reason}")]
    Copy { path: String, reason: String },
    #[error("The test command is not usable: {0}")]
    Command(String),
    #[error("The program {0} was not found")]
    ProgramNotFound(String),
    #[error(transparent)]
    Batch(#[from] UnsafeBatch),
    #[error("The test command could not be started: {0}")]
    Spawn(String),
}

impl ArenaError {
    /// Stable code the frontend translates (`arenaErrorText` in arena.ts). Errors passed through
    /// from git and worktree keep their own text and have none.
    fn code(&self) -> Option<&'static str> {
        match self {
            Self::Worktree(_) | Self::Git(_) => None,
            Self::Count => Some("arena_count"),
            Self::NoCommit => Some("arena_no_commit"),
            Self::NotArena => Some("arena_not_arena"),
            Self::Missing => Some("arena_missing"),
            Self::NothingToApply => Some("arena_nothing_to_apply"),
            Self::TooLarge => Some("arena_too_large"),
            Self::Apply(_) => Some("arena_apply"),
            Self::Copy { .. } => Some("arena_copy"),
            Self::Command(_) => Some("arena_command"),
            Self::ProgramNotFound(_) => Some("arena_program_not_found"),
            Self::Batch(_) => Some("arena_unsafe_batch"),
            Self::Spawn(_) => Some("arena_spawn"),
        }
    }
}

/// Errors reach the frontend as readable text, prefixed with `[code] ` when they have a code
/// (the same form as `ManagerError`).
impl Serialize for ArenaError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.code() {
            Some(code) => serializer.serialize_str(&format!("[{code}] {self}")),
            None => serializer.serialize_str(&self.to_string()),
        }
    }
}

/// What `arena_create` answers: the shared base commit and one worktree per agent, in the order
/// of the labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Arena {
    pub id: String,
    pub base_commit: String,
    pub repo_root: PathBuf,
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStat {
    /// Relative to the worktree, with `/`.
    pub path: String,
    pub added: u64,
    pub removed: u64,
}

/// A contestant's changes against the base commit: committed, staged, unstaged and new files
/// (ignored files left out).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffStat {
    pub files: Vec<FileStat>,
    pub added: u64,
    pub removed: u64,
    /// The file list was cut; the sums cover what was counted.
    pub truncated: bool,
}

/// Result of taking a winner over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ApplyOutcome {
    /// The files now changed in the project folder, not staged and not committed.
    Applied { files: Vec<String> },
    /// Nothing was touched: the project folder has its own changes in these files.
    Conflict { files: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRun {
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    /// The end of the output (stdout, then stderr), without color codes.
    pub output: String,
}

// ---------------------------------------------------------------------------------------------
// Names

/// Six lowercase hex characters, new per arena.
pub fn new_id() -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    random.get(..ID_CHARS).unwrap_or("000000").to_owned()
}

fn is_id(text: &str) -> bool {
    text.len() == ID_CHARS
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `arena-<id>-<agent>` per label, in order. The same agent twice gets `-2`, `-3`, so every
/// contestant has a branch of its own.
pub fn contestant_slugs(id: &str, labels: &[String]) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    labels
        .iter()
        .map(|label| {
            let mut agent = worktree::slug_label(label);
            // ASCII only, so byte and char counts agree.
            agent.truncate(MAX_AGENT_CHARS);
            let agent = agent.trim_end_matches('-').to_owned();
            let count = seen.entry(agent.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                format!("{SLUG_PREFIX}{id}-{agent}")
            } else {
                format!("{SLUG_PREFIX}{id}-{agent}-{count}")
            }
        })
        .collect()
}

/// Branch of an arena worktree: `hangar/arena-<6 hex>-<agent>`.
pub fn is_arena_branch(branch: &str) -> bool {
    if worktree::check_branch(branch).is_err() {
        return false;
    }
    let Some(rest) = branch
        .strip_prefix(BRANCH_PREFIX)
        .and_then(|slug| slug.strip_prefix(SLUG_PREFIX))
    else {
        return false;
    };
    match rest.split_once('-') {
        Some((id, agent)) => is_id(id) && !agent.is_empty(),
        None => false,
    }
}

/// The checks every call on a contestant's worktree passes first: Hangar's own worktree record,
/// inside its worktrees folder, an arena branch, git folders on the local disk, still there.
fn check_contestant(base: &Path, worktree: &Worktree) -> Result<(), ArenaError> {
    worktree::check_shape(worktree)?;
    worktree::check_managed(base, &worktree.path, &worktree.repo_root)?;
    if !is_arena_branch(&worktree.branch) {
        return Err(ArenaError::NotArena);
    }
    if !worktree.path.is_dir() {
        return Err(ArenaError::Missing);
    }
    git::check_folder(&worktree.path)?;
    Ok(())
}

fn check_commit(commit: &str) -> Result<(), ArenaError> {
    if worktree::is_full_commit_id(commit) {
        Ok(())
    } else {
        Err(GitError::InvalidCommit.into())
    }
}

// ---------------------------------------------------------------------------------------------
// Creating

/// One worktree per label from the current commit of the repository that contains `cwd`. HEAD is
/// read once, so all contestants start from the same commit. If one worktree fails, the ones
/// already made are removed again (they are clean, so never by force).
pub fn create(git: &Git, base: &Path, cwd: &Path, labels: &[String]) -> Result<Arena, ArenaError> {
    if !(MIN_CONTESTANTS..=MAX_CONTESTANTS).contains(&labels.len()) {
        return Err(ArenaError::Count);
    }
    git::check_folder(cwd)?;
    let deadline = Instant::now() + GIT_TIMEOUT;
    let root = git::toplevel(git, cwd, deadline)?.ok_or(WorktreeError::NotARepo)?;
    let head = git.run(
        &root,
        ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        deadline,
        4096,
    )?;
    let base_commit = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    if !head.success || !worktree::is_full_commit_id(&base_commit) {
        return Err(ArenaError::NoCommit);
    }
    let id = new_id();
    let mut made: Vec<Worktree> = Vec::new();
    for slug in contestant_slugs(&id, labels) {
        match worktree::create_at(git, base, &root, &slug, &base_commit) {
            Ok(created) => made.push(created),
            Err(error) => {
                for created in &made {
                    let _ = worktree::remove(git, base, created);
                }
                return Err(error.into());
            }
        }
    }
    Ok(Arena {
        id,
        base_commit,
        repo_root: root,
        worktrees: made,
    })
}

// ---------------------------------------------------------------------------------------------
// Measuring

fn neutral_args<'a>(neutral: &'a [String], rest: &[&'a str]) -> Vec<&'a str> {
    neutral
        .iter()
        .map(String::as_str)
        .chain(rest.iter().copied())
        .collect()
}

/// Tracked changes against `base_commit` (committed, staged and in the work tree), per file.
fn tracked_stats(
    git: &Git,
    path: &Path,
    neutral: &[String],
    base_commit: &str,
    deadline: Instant,
) -> Result<(Vec<FileStat>, bool), ArenaError> {
    let mut args = neutral_args(neutral, git::DIFF_ARGS);
    args.extend(["--numstat", "-z", "--no-renames", base_commit, "--"]);
    let output = git.run(path, args, deadline, git::MAX_LIST_BYTES)?;
    if !output.usable() {
        return Err(output.failure().into());
    }
    let files = git::parse_numstat(&output.stdout)
        .into_iter()
        .map(|(path, added, removed)| FileStat {
            path,
            added,
            removed,
        })
        .collect();
    Ok((files, output.truncated))
}

/// New files that are neither tracked nor ignored (`ls-files --others --exclude-standard`).
fn untracked_files(
    git: &Git,
    path: &Path,
    neutral: &[String],
    deadline: Instant,
) -> Result<(Vec<String>, bool), ArenaError> {
    let args = neutral_args(
        neutral,
        &["ls-files", "--others", "--exclude-standard", "-z", "--"],
    );
    let output = git.run(path, args, deadline, git::MAX_LIST_BYTES)?;
    if !output.usable() {
        return Err(output.failure().into());
    }
    let mut files: Vec<String> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect();
    if output.truncated {
        // The last name may be cut in the middle.
        files.pop();
    }
    Ok((files, output.truncated))
}

/// Lines of a new file; binary files (a NUL byte) and very large ones count as none.
pub fn count_lines(content: &[u8]) -> u64 {
    if content.contains(&0) {
        return 0;
    }
    let newlines = content.iter().filter(|b| **b == b'\n').count();
    let open_last = !content.is_empty() && !content.ends_with(b"\n");
    u64::try_from(newlines + usize::from(open_last)).unwrap_or(u64::MAX)
}

/// A relative path from git (`a/b.txt`) as a path below a folder: plain names only, so it can
/// never leave the folder or name a drive or an alternate data stream.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains(['\\', ':', '\0'])
            || Path::new(part).is_absolute()
        {
            return None;
        }
        out.push(part);
    }
    let plain = out
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    (plain && !out.as_os_str().is_empty()).then_some(out)
}

/// The contestant's changes against the base commit, per file and summed up.
pub fn diff_stat(
    git: &Git,
    base: &Path,
    worktree: &Worktree,
    base_commit: &str,
) -> Result<DiffStat, ArenaError> {
    check_contestant(base, worktree)?;
    check_commit(base_commit)?;
    let deadline = Instant::now() + GIT_TIMEOUT;
    let neutral = git::filter_overrides(git, &worktree.path, deadline)?;
    let (mut files, mut truncated) =
        tracked_stats(git, &worktree.path, &neutral, base_commit, deadline)?;
    let (untracked, cut) = untracked_files(git, &worktree.path, &neutral, deadline)?;
    truncated |= cut || untracked.len() > MAX_COUNTED_FILES;
    for name in untracked.into_iter().take(MAX_COUNTED_FILES) {
        let lines = safe_relative(&name)
            .map(|relative| worktree.path.join(relative))
            .and_then(|file| crate::persist::read_limited(&file, MAX_COUNTED_BYTES).ok())
            .flatten()
            .map_or(0, |content| count_lines(&content));
        files.push(FileStat {
            path: name,
            added: lines,
            removed: 0,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let added = files.iter().map(|file| file.added).sum();
    let removed = files.iter().map(|file| file.removed).sum();
    Ok(DiffStat {
        files,
        added,
        removed,
        truncated,
    })
}

// ---------------------------------------------------------------------------------------------
// Taking the winner over

/// Paths compared as the file system does: without case on Windows and macOS.
fn fold(path: &str) -> String {
    if cfg!(any(windows, target_os = "macos")) {
        path.to_lowercase()
    } else {
        path.to_owned()
    }
}

/// The winner's files that the project folder has changed itself, sorted.
pub fn conflicts(changed: &[String], dirty: &[String]) -> Vec<String> {
    let dirty: BTreeSet<String> = dirty.iter().map(|path| fold(path)).collect();
    let mut found: Vec<String> = changed
        .iter()
        .filter(|path| dirty.contains(&fold(path)))
        .cloned()
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Files with uncommitted changes in the project folder, untracked ones included.
fn dirty_files(
    git: &Git,
    root: &Path,
    neutral: &[String],
    deadline: Instant,
) -> Result<Vec<String>, ArenaError> {
    let args = neutral_args(
        neutral,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    );
    let output = git.run(root, args, deadline, git::MAX_LIST_BYTES)?;
    // A cut list could hide a conflict; only a complete one counts.
    if !output.success {
        return Err(output.failure().into());
    }
    Ok(git::parse_status(&output.stdout)
        .into_iter()
        .map(|file| file.path)
        .collect())
}

/// The project folder must be the top of the repository that lists the contestant's worktree, so
/// the changes land where the arena started and paths line up (git applies a patch relative to
/// the folder it runs in, a copy would too).
fn check_owner(git: &Git, worktree: &Worktree, deadline: Instant) -> Result<(), ArenaError> {
    let target = &worktree.repo_root;
    let top = git::toplevel(git, target, deadline)?.ok_or(WorktreeError::NotARepo)?;
    if git::path_key(&top) != git::path_key(target) {
        return Err(WorktreeError::OutsideFolder.into());
    }
    let listed = worktree::list(git, target, deadline)?;
    let wanted = git::path_key(&worktree.path);
    if listed
        .iter()
        .any(|entry| git::path_key(&entry.path) == wanted)
    {
        Ok(())
    } else {
        Err(WorktreeError::OutsideFolder.into())
    }
}

/// Takes the contestant's changes over into the project folder (`worktree.repoRoot`) as
/// uncommitted changes: tracked changes as a patch through `git apply` (all or nothing), new files
/// copied. Refuses, before touching anything, when the project folder has its own changes in any
/// of those files or a new file is already there.
pub fn apply(
    git: &Git,
    base: &Path,
    worktree: &Worktree,
    base_commit: &str,
) -> Result<ApplyOutcome, ArenaError> {
    check_contestant(base, worktree)?;
    check_commit(base_commit)?;
    let target = &worktree.repo_root;
    git::check_folder(target)?;
    let deadline = Instant::now() + GIT_TIMEOUT;
    check_owner(git, worktree, deadline)?;
    let neutral = git::filter_overrides(git, &worktree.path, deadline)?;
    let (tracked, cut) = tracked_stats(git, &worktree.path, &neutral, base_commit, deadline)?;
    let (untracked, cut_untracked) = untracked_files(git, &worktree.path, &neutral, deadline)?;
    if cut || cut_untracked {
        return Err(ArenaError::TooLarge);
    }
    let tracked: Vec<String> = tracked.into_iter().map(|file| file.path).collect();
    if tracked.is_empty() && untracked.is_empty() {
        return Err(ArenaError::NothingToApply);
    }
    let mut new_files = Vec::with_capacity(untracked.len());
    for name in &untracked {
        let relative = safe_relative(name).ok_or_else(|| ArenaError::Copy {
            path: name.clone(),
            reason: "unusual path".to_owned(),
        })?;
        new_files.push((name.clone(), relative));
    }

    let target_neutral = git::filter_overrides(git, target, deadline)?;
    let dirty = dirty_files(git, target, &target_neutral, deadline)?;
    let all_changed: Vec<String> = tracked.iter().chain(untracked.iter()).cloned().collect();
    let mut blocked = conflicts(&all_changed, &dirty);
    for (name, relative) in &new_files {
        if fs::symlink_metadata(target.join(relative)).is_ok() && !blocked.contains(name) {
            blocked.push(name.clone());
        }
    }
    if !blocked.is_empty() {
        blocked.sort();
        return Ok(ApplyOutcome::Conflict { files: blocked });
    }
    // New files must be plain files; links and the like are refused before anything changes.
    for (name, relative) in &new_files {
        let meta = fs::symlink_metadata(worktree.path.join(relative)).map_err(|error| {
            ArenaError::Copy {
                path: name.clone(),
                reason: error.to_string(),
            }
        })?;
        if !meta.is_file() {
            return Err(ArenaError::Copy {
                path: name.clone(),
                reason: "not a regular file".to_owned(),
            });
        }
    }

    if !tracked.is_empty() {
        let mut args = neutral_args(&neutral, git::DIFF_ARGS);
        args.extend([
            "--binary",
            "--full-index",
            "--no-renames",
            base_commit,
            "--",
        ]);
        let patch = git.run(&worktree.path, args, deadline, MAX_PATCH_BYTES)?;
        if patch.truncated {
            return Err(ArenaError::TooLarge);
        }
        if !patch.success {
            return Err(patch.failure().into());
        }
        let args = neutral_args(&target_neutral, &["apply", "--whitespace=nowarn", "-"]);
        let applied = git.run_with_input(target, args, Some(patch.stdout), deadline, 64 * 1024)?;
        if !applied.success {
            return Err(ArenaError::Apply(git::message(&applied.stderr)));
        }
    }
    copy_new_files(&worktree.path, target, &new_files)?;
    let mut files = all_changed;
    files.sort();
    Ok(ApplyOutcome::Applied { files })
}

/// Copies new files from the worktree into the project folder; each target folder must stay
/// inside the project folder once resolved (a linked folder there could lead elsewhere).
fn copy_new_files(
    from: &Path,
    target: &Path,
    files: &[(String, PathBuf)],
) -> Result<(), ArenaError> {
    let copy_error = |name: &str, reason: String| ArenaError::Copy {
        path: name.to_owned(),
        reason,
    };
    let real_target = target
        .canonicalize()
        .map_err(|error| copy_error(".", error.to_string()))?;
    let check_inside = |name: &str, dir: &Path| -> Result<(), ArenaError> {
        let real = dir
            .canonicalize()
            .map_err(|error| copy_error(name, error.to_string()))?;
        if real.starts_with(&real_target) {
            Ok(())
        } else {
            Err(copy_error(
                name,
                "the folder leads outside the project".to_owned(),
            ))
        }
    };
    for (name, relative) in files {
        let destination = target.join(relative);
        if let Some(parent) = destination.parent() {
            // The deepest folder that exists already is checked first, so no folder is ever
            // created behind a link that leads elsewhere.
            let existing = parent
                .ancestors()
                .find(|dir| dir.exists())
                .unwrap_or(target);
            check_inside(name, existing)?;
            fs::create_dir_all(parent).map_err(|error| copy_error(name, error.to_string()))?;
            check_inside(name, parent)?;
        }
        fs::copy(from.join(relative), &destination)
            .map_err(|error| copy_error(name, error.to_string()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Discarding a contestant

/// Removes a contestant's worktree and branch with everything in it, after the user confirmed
/// it in the arena (unlike [`worktree::remove`], which never forces). Only arena branches; no
/// hook (switched off by [`Git`] itself) and no filter of the repository runs. A folder deleted by
/// hand leaves only git's record, which `worktree remove` takes away for this one worktree;
/// `worktree prune` would also take the records of the user's own worktrees.
pub fn discard(git: &Git, base: &Path, worktree: &Worktree) -> Result<(), ArenaError> {
    worktree::check_shape(worktree)?;
    worktree::check_managed(base, &worktree.path, &worktree.repo_root)?;
    if !is_arena_branch(&worktree.branch) {
        return Err(ArenaError::NotArena);
    }
    git::check_folder(&worktree.repo_root)?;
    git::check_git_links(&worktree.path)?;
    let deadline = Instant::now() + GIT_TIMEOUT;
    let root = &worktree.repo_root;
    let existed = worktree.path.exists();
    let neutral = if existed {
        git::filter_overrides(git, &worktree.path, deadline)?
    } else {
        git::filter_overrides(git, root, deadline)?
    };
    let args: Vec<&OsStr> = neutral
        .iter()
        .map(OsStr::new)
        .chain([
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            worktree.path.as_os_str(),
        ])
        .collect();
    let output = git.run(root, args, deadline, 64 * 1024)?;
    if !output.success {
        // Folder and record both gone already (deleted by hand, then pruned): nothing is left.
        let gone = !existed
            && worktree::list(git, root, deadline).is_ok_and(|listed| {
                !listed
                    .iter()
                    .any(|entry| git::path_key(&entry.path) == git::path_key(&worktree.path))
            });
        if !gone {
            return Err(output.failure().into());
        }
    }
    if let Some(parent) = worktree.path.parent() {
        // Only succeeds when no other worktree of this repository is left.
        let _ = fs::remove_dir(parent);
    }
    let output = git.run(
        root,
        ["branch", "-D", worktree.branch.as_str()],
        deadline,
        64 * 1024,
    )?;
    if !output.success {
        return Err(output.failure().into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Test command

/// The test command as an argv array: split at whitespace, no shell syntax (the same rule as an
/// update command), so nothing in it is interpreted by a shell.
pub fn split_test_command(command: &str) -> Result<Vec<String>, ArenaError> {
    crate::updates::split_command(command).map_err(|error| ArenaError::Command(error.to_string()))
}

/// Removes ANSI escape sequences (CSI and OSC) so the tail reads as plain text.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if c == '\r' {
                continue;
            }
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // Parameters and intermediates up to the final byte (@ to ~).
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                // Up to BEL or ESC \.
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' {
                        chars.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The last `keep` bytes of a stream, read to its end on a thread of its own.
fn drain_tail(
    stream: impl Read + Send + 'static,
    index: usize,
    tx: Sender<(usize, Vec<u8>)>,
) -> bool {
    thread::Builder::new()
        .name(format!("arena-test-read-{index}"))
        .spawn(move || {
            let mut stream = stream;
            let mut tail: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        tail.extend_from_slice(chunk.get(..read).unwrap_or_default());
                        if tail.len() > TAIL_BYTES * 2 {
                            let cut = tail.len() - TAIL_BYTES;
                            tail.drain(..cut);
                        }
                    }
                }
            }
            if tail.len() > TAIL_BYTES {
                let cut = tail.len() - TAIL_BYTES;
                tail.drain(..cut);
            }
            let _ = tx.send((index, tail));
        })
        .is_ok()
}

fn end_tree(tree: Option<&ProcTree>, child: &mut Child) {
    if let Some(tree) = tree {
        // Failure usually means everything has already ended.
        let _ = tree.terminate();
    }
    let _ = child.kill();
    // Reap it so no zombie remains on Unix. After kill, wait returns immediately.
    let _ = child.wait();
}

/// Runs the test command in the contestant's worktree: argv without a shell, the program looked
/// up on PATH (never in the worktree), the environment of a session, stdin empty. After
/// [`TEST_TIMEOUT`], or once the command has ended, its whole process tree is ended.
pub fn run_tests(
    base: &Path,
    worktree: &Worktree,
    command: &str,
    timeout: Duration,
) -> Result<TestRun, ArenaError> {
    check_contestant(base, worktree)?;
    let argv = split_test_command(command)?;
    let (name, args) = argv
        .split_first()
        .ok_or_else(|| ArenaError::Command("The test command is empty".to_owned()))?;
    let path = std::env::var_os("PATH");
    let pathext = std::env::var_os("PATHEXT");
    let program = crate::resolve::resolve_program(name, path.as_deref(), pathext.as_deref())
        .ok_or_else(|| ArenaError::ProgramNotFound(name.clone()))?;
    let args = args.to_vec();
    if batch::is_batch(&program) {
        batch::check(&program, &args)?;
    }

    let mut command = Command::new(&program);
    command
        .args(&args)
        .current_dir(&worktree.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut env = BTreeMap::new();
    env.insert("NO_COLOR".to_owned(), "1".to_owned());
    // The worktree's files come from an agent: a `node.cmd` there must not be found by an npm
    // launcher (child_env.rs explains the lookup).
    crate::child_env::apply(&mut command, &env, true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so ProcTree (killpg) hits exactly this tree.
        command.process_group(0);
    }

    let started = Instant::now();
    let deadline = started + timeout;
    let mut child = command
        .spawn()
        .map_err(|error| ArenaError::Spawn(error.to_string()))?;
    let tree = match ProcTree::attach(child.id()) {
        Ok(tree) => Some(tree),
        Err(_) if matches!(child.try_wait(), Ok(Some(_))) => None,
        Err(error) => {
            end_tree(None, &mut child);
            return Err(ArenaError::Spawn(error.to_string()));
        }
    };
    let (tx, rx) = mpsc::channel();
    let mut pending = 0;
    if let Some(stdout) = child.stdout.take() {
        pending += usize::from(drain_tail(stdout, 0, tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        pending += usize::from(drain_tail(stderr, 1, tx));
    }

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => break None,
        }
    };
    let duration = started.elapsed();
    // Always end the tree: a watcher or server the tests started must not keep running.
    end_tree(tree.as_ref(), &mut child);
    let mut outputs = [Vec::new(), Vec::new()];
    let grace = Instant::now() + DRAIN_GRACE;
    while pending > 0 {
        match rx.recv_timeout(grace.saturating_duration_since(Instant::now())) {
            Ok((index, data)) => {
                if let Some(slot) = outputs.get_mut(index) {
                    *slot = data;
                }
                pending -= 1;
            }
            Err(_) => break,
        }
    }
    let [stdout, stderr] = outputs;
    let output = [stdout, stderr]
        .iter()
        .map(|bytes| {
            strip_ansi(&String::from_utf8_lossy(bytes))
                .trim()
                .to_owned()
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(TestRun {
        passed: status.is_some_and(|status| status.success()),
        exit_code: status.and_then(|status| status.code()),
        timed_out: status.is_none(),
        duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;

    fn serialized(error: &ArenaError) -> String {
        match serde_json::to_value(error) {
            Ok(serde_json::Value::String(text)) => text,
            other => panic!("not a string: {other:?}"),
        }
    }

    #[test]
    fn own_errors_carry_a_stable_code() {
        let cases = [
            (ArenaError::Count, "arena_count"),
            (ArenaError::NoCommit, "arena_no_commit"),
            (ArenaError::NotArena, "arena_not_arena"),
            (ArenaError::Missing, "arena_missing"),
            (ArenaError::NothingToApply, "arena_nothing_to_apply"),
            (ArenaError::TooLarge, "arena_too_large"),
            (ArenaError::Apply("conflict".to_owned()), "arena_apply"),
            (
                ArenaError::Copy {
                    path: "a.txt".to_owned(),
                    reason: "outside".to_owned(),
                },
                "arena_copy",
            ),
            (ArenaError::Command("empty".to_owned()), "arena_command"),
            (
                ArenaError::ProgramNotFound("pnpm".to_owned()),
                "arena_program_not_found",
            ),
            (
                ArenaError::Batch(UnsafeBatch {
                    part: batch::Part::Argument(1),
                    found: '&',
                }),
                "arena_unsafe_batch",
            ),
            (ArenaError::Spawn("denied".to_owned()), "arena_spawn"),
        ];
        let mut codes = BTreeSet::new();
        for (error, code) in &cases {
            assert_eq!(error.code(), Some(*code), "{error}");
            assert_eq!(serialized(error), format!("[{code}] {error}"));
            assert!(codes.insert(*code), "duplicate code {code}");
        }
    }

    #[test]
    fn missing_program_names_it_in_a_fixed_form() {
        // arena.ts reads the program back from exactly this text.
        assert_eq!(
            serialized(&ArenaError::ProgramNotFound("pnpm".to_owned())),
            "[arena_program_not_found] The program pnpm was not found"
        );
    }

    #[test]
    fn passed_through_errors_keep_their_own_text() {
        let error = ArenaError::Git(GitError::InvalidCommit);
        assert_eq!(error.code(), None);
        assert_eq!(serialized(&error), error.to_string());
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn contestants_get_arena_branches() {
        let id = new_id();
        assert!(is_id(&id), "{id}");
        let slugs = contestant_slugs(
            "1a2b3c",
            &labels(&["Claude Code", "Codex CLI", "Claude Code", "日本語"]),
        );
        assert_eq!(
            slugs,
            [
                "arena-1a2b3c-claude-code",
                "arena-1a2b3c-codex-cli",
                "arena-1a2b3c-claude-code-2",
                "arena-1a2b3c-session",
            ]
        );
        let long = contestant_slugs("1a2b3c", &labels(&[&"word ".repeat(30)]));
        for slug in slugs.iter().chain(long.iter()) {
            assert!(worktree::is_valid_slug(slug), "{slug}");
            assert!(is_arena_branch(&format!("{BRANCH_PREFIX}{slug}")), "{slug}");
        }
        for bad in [
            "hangar/claude-code-1a2b3c",
            "hangar/arena-1a2b3c",
            "hangar/arena-1A2B3C-x",
            "hangar/arena-xyz123-x",
            "arena-1a2b3c-x",
            "hangar/arena-1a2b3c-",
            "main",
        ] {
            assert!(!is_arena_branch(bad), "{bad}");
        }
    }

    #[test]
    fn line_counts_and_paths() {
        assert_eq!(count_lines(b""), 0);
        assert_eq!(count_lines(b"a\nb\n"), 2);
        assert_eq!(count_lines(b"a\nb"), 2);
        assert_eq!(count_lines(b"a\0b\n"), 0);
        assert_eq!(
            safe_relative("src/a b.txt"),
            Some(Path::new("src").join("a b.txt"))
        );
        for bad in [
            "", "../x", "a/../b", "/abs", "a//b", "./a", "c:x", "a\\b", "C:",
        ] {
            assert_eq!(safe_relative(bad), None, "{bad}");
        }
    }

    #[test]
    fn conflicts_are_the_shared_files() {
        let changed = labels(&["src/a.ts", "README.md", "new.txt"]);
        assert!(conflicts(&changed, &labels(&["other.ts"])).is_empty());
        assert_eq!(
            conflicts(&changed, &labels(&["src/a.ts", "new.txt", "x"])),
            ["new.txt", "src/a.ts"]
        );
        if cfg!(windows) {
            assert_eq!(conflicts(&changed, &labels(&["readme.md"])), ["README.md"]);
        }
    }

    #[test]
    fn test_commands_are_plain_argv() {
        assert_eq!(
            split_test_command("pnpm test -- --run").unwrap(),
            ["pnpm", "test", "--", "--run"]
        );
        for bad in [
            "",
            "   ",
            "pnpm test && rm x",
            "echo $HOME",
            "a | b",
            "npm test > out",
        ] {
            assert!(split_test_command(bad).is_err(), "{bad}");
        }
        assert_eq!(
            strip_ansi("\u{1b}[32mok\u{1b}[0m 3 passed\r\n\u{1b}]8;;http://x\u{7}link"),
            "ok 3 passed\nlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn new_files_never_create_folders_behind_a_link() {
        let tmp = TempDir::new("arena-link");
        let from = tmp.path().join("from");
        let target = tmp.path().join("target");
        let elsewhere = tmp.path().join("elsewhere");
        for dir in [&from, &target, &elsewhere] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::create_dir_all(from.join("link").join("a")).unwrap();
        fs::write(from.join("link").join("a").join("x.txt"), "x\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, target.join("link")).unwrap();
        let files = vec![(
            "link/a/x.txt".to_owned(),
            Path::new("link").join("a").join("x.txt"),
        )];
        assert!(matches!(
            copy_new_files(&from, &target, &files),
            Err(ArenaError::Copy { .. })
        ));
        assert!(!elsewhere.join("a").exists());
    }

    fn commit_all(git: &Git, repo: &Path, message: &str) {
        git_ok(git, repo, &["add", "-A"]);
        git_ok(git, repo, &["commit", "--quiet", "-m", message]);
    }

    fn status(git: &Git, repo: &Path) -> String {
        let output = git
            .run(
                repo,
                ["status", "--porcelain=v1", "--untracked-files=all"],
                Instant::now() + Duration::from_secs(30),
                1 << 20,
            )
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn head(git: &Git, repo: &Path) -> String {
        let output = git
            .run(
                repo,
                ["rev-parse", "HEAD"],
                Instant::now() + Duration::from_secs(30),
                4096,
            )
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn an_arena_from_creation_to_the_winner() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("arena-real");
        let base = tmp.path().join(worktree::DIR_NAME);
        let repo = tmp.path().join("my repo");
        init_repo(&git, &repo);
        fs::write(repo.join("readme.txt"), "one\ntwo\nthree\n").unwrap();
        fs::write(repo.join("gone.txt"), "bye\n").unwrap();
        fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        commit_all(&git, &repo, "first");
        let base_commit = head(&git, &repo);

        // Too few or too many agents are refused before anything is created.
        for names in [&["one"][..], &["a", "b", "c", "d", "e"][..]] {
            assert!(matches!(
                create(&git, &base, &repo, &labels(names)),
                Err(ArenaError::Count)
            ));
        }

        let arena = create(&git, &base, &repo, &labels(&["Claude Code", "Codex"])).unwrap();
        assert_eq!(arena.base_commit, base_commit);
        assert_eq!(arena.worktrees.len(), 2);
        for worktree in &arena.worktrees {
            assert!(is_arena_branch(&worktree.branch), "{}", worktree.branch);
            assert!(worktree.branch.contains(&arena.id));
            assert_eq!(head(&git, &worktree.path), base_commit);
        }
        let [winner, loser] = arena.worktrees.as_slice() else {
            panic!("two worktrees expected");
        };

        // No changes yet: nothing to count, nothing to take over.
        let empty = diff_stat(&git, &base, winner, &base_commit).unwrap();
        assert!(empty.files.is_empty());
        assert!(matches!(
            apply(&git, &base, winner, &base_commit),
            Err(ArenaError::NothingToApply)
        ));

        // The winner edits, deletes, adds a new file (and an ignored one), and commits a part.
        fs::write(winner.path.join("readme.txt"), "one\n2\nthree\nfour\n").unwrap();
        fs::remove_file(winner.path.join("gone.txt")).unwrap();
        fs::create_dir_all(winner.path.join("src")).unwrap();
        fs::write(winner.path.join("src").join("new file.txt"), "a\nb\nc").unwrap();
        fs::write(winner.path.join("debug.log"), "local only\n").unwrap();
        fs::write(winner.path.join("committed.txt"), "x\n").unwrap();
        git_ok(&git, &winner.path, &["add", "committed.txt"]);
        git_ok(&git, &winner.path, &["commit", "--quiet", "-m", "agent"]);

        let stat = diff_stat(&git, &base, winner, &base_commit).unwrap();
        let paths: Vec<&str> = stat.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "committed.txt",
                "gone.txt",
                "readme.txt",
                "src/new file.txt"
            ]
        );
        assert_eq!((stat.added, stat.removed), (1 + 2 + 3, 1 + 1));
        assert!(!stat.truncated);

        // The project folder changed the same file: refused, nothing touched.
        fs::write(repo.join("readme.txt"), "mine\n").unwrap();
        let outcome = apply(&git, &base, winner, &base_commit).unwrap();
        assert_eq!(
            outcome,
            ApplyOutcome::Conflict {
                files: vec!["readme.txt".to_owned()]
            }
        );
        assert_eq!(
            fs::read_to_string(repo.join("readme.txt")).unwrap(),
            "mine\n"
        );
        assert!(repo.join("gone.txt").is_file());
        assert!(!repo.join("src").exists());

        // A file of the same name as a new one is in the way too.
        git_ok(&git, &repo, &["checkout", "--quiet", "--", "readme.txt"]);
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("src").join("new file.txt"), "mine\n").unwrap();
        let outcome = apply(&git, &base, winner, &base_commit).unwrap();
        assert_eq!(
            outcome,
            ApplyOutcome::Conflict {
                files: vec!["src/new file.txt".to_owned()]
            }
        );
        fs::remove_dir_all(repo.join("src")).unwrap();

        // Only into the repository the worktree belongs to, at its top: a subfolder or another
        // repository with a record edited to match is refused before anything changes.
        fs::create_dir_all(repo.join("sub")).unwrap();
        let other = tmp.path().join("other");
        init_repo(&git, &other);
        for root in [repo.join("sub"), other.clone()] {
            let moved = Worktree {
                path: base
                    .join(worktree::repo_dir_name(&root))
                    .join(winner.path.file_name().unwrap_or_default()),
                branch: winner.branch.clone(),
                repo_root: root.clone(),
            };
            fs::create_dir_all(&moved.path).unwrap();
            let refused = apply(&git, &base, &moved, &base_commit);
            assert!(
                matches!(
                    refused,
                    Err(ArenaError::Worktree(WorktreeError::OutsideFolder))
                ),
                "{}: {refused:?}",
                root.display()
            );
            fs::remove_dir_all(&moved.path).unwrap();
        }
        assert!(!repo.join("src").exists());

        // Clean project folder: the changes arrive, uncommitted, HEAD unchanged.
        let outcome = apply(&git, &base, winner, &base_commit).unwrap();
        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                files: labels(&[
                    "committed.txt",
                    "gone.txt",
                    "readme.txt",
                    "src/new file.txt"
                ])
            }
        );
        assert_eq!(head(&git, &repo), base_commit);
        let readme = fs::read_to_string(repo.join("readme.txt")).unwrap();
        assert_eq!(readme.replace("\r\n", "\n"), "one\n2\nthree\nfour\n");
        assert!(!repo.join("gone.txt").exists());
        assert_eq!(
            fs::read_to_string(repo.join("src").join("new file.txt")).unwrap(),
            "a\nb\nc"
        );
        assert!(repo.join("committed.txt").is_file());
        assert!(!repo.join("debug.log").exists());
        let after = status(&git, &repo);
        assert!(after.contains(" M readme.txt"), "{after}");
        assert!(after.contains(" D gone.txt"), "{after}");
        assert!(after.contains("?? committed.txt"), "{after}");

        // Running the tests: a plain program, its exit code and output.
        let run = run_tests(&base, winner, "git --version", Duration::from_secs(60)).unwrap();
        assert!(run.passed, "{run:?}");
        assert!(run.output.contains("git version"), "{run:?}");
        let run = run_tests(
            &base,
            winner,
            "git no-such-command",
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(!run.passed && !run.timed_out, "{run:?}");
        assert!(matches!(
            run_tests(
                &base,
                winner,
                "no-such-program-hangar",
                Duration::from_secs(5)
            ),
            Err(ArenaError::ProgramNotFound(_))
        ));

        // The loser goes with its changes and branch; the repository keeps no trace.
        fs::write(loser.path.join("readme.txt"), "other idea\n").unwrap();
        discard(&git, &base, loser).unwrap();
        assert!(!loser.path.exists());
        let branches = git
            .run(
                &repo,
                ["branch", "--list", "hangar/*"],
                Instant::now() + Duration::from_secs(30),
                1 << 20,
            )
            .unwrap();
        let branches = String::from_utf8_lossy(&branches.stdout).into_owned();
        assert!(!branches.contains(&loser.branch), "{branches}");
        assert!(branches.contains(&winner.branch), "{branches}");

        // Only a full commit id counts as the base, never a ref name.
        assert!(matches!(
            diff_stat(&git, &base, winner, "HEAD"),
            Err(ArenaError::Git(GitError::InvalidCommit))
        ));

        // A folder deleted by hand: git's record of it and the branch still go, nothing else.
        let plain_kept = worktree::create(&git, &base, &repo, "kept").unwrap();
        fs::remove_dir_all(&winner.path).unwrap();
        discard(&git, &base, winner).unwrap();
        let listed = worktree::list(&git, &repo, Instant::now() + Duration::from_secs(30)).unwrap();
        let keys: Vec<String> = listed
            .iter()
            .map(|entry| git::path_key(&entry.path))
            .collect();
        assert!(!keys.contains(&git::path_key(&winner.path)), "{keys:?}");
        assert!(keys.contains(&git::path_key(&plain_kept.path)), "{keys:?}");
        let branches = git
            .run(
                &repo,
                ["branch", "--list", "hangar/*"],
                Instant::now() + Duration::from_secs(30),
                1 << 20,
            )
            .unwrap();
        let branches = String::from_utf8_lossy(&branches.stdout).into_owned();
        assert!(!branches.contains(&winner.branch), "{branches}");
        assert!(branches.contains(&plain_kept.branch), "{branches}");

        // A worktree that is not an arena's is never discarded by force.
        let plain = worktree::create(&git, &base, &repo, "plain").unwrap();
        assert!(matches!(
            discard(&git, &base, &plain),
            Err(ArenaError::NotArena)
        ));
        assert!(matches!(
            apply(&git, &base, &plain, &base_commit),
            Err(ArenaError::NotArena)
        ));
        assert!(plain.path.exists());
    }
}
