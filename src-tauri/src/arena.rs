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
use crate::git_actions;
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
    #[error("The project folder has too many uncommitted changes to check for overlaps")]
    ProjectTooLarge,
    #[error("git could not apply the changes: {0}")]
    Apply(String),
    #[error("{path} cannot be taken over: {reason}")]
    Copy { path: String, reason: String },
    #[error("{0} cannot be taken over: the path could lead into a git folder or to a device")]
    UnsafePath(String),
    #[error("{0} is a link or a submodule, which is never taken over")]
    Link(String),
    #[error("{0} is a file in the project and a folder in the changes, which is not taken over")]
    FileBecameFolder(String),
    #[error("The changes are no longer the ones that were shown")]
    Changed,
    #[error("The worktree could not be deleted: {0}")]
    Remove(String),
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
            Self::ProjectTooLarge => Some("arena_project_too_large"),
            Self::Apply(_) => Some("arena_apply"),
            Self::Copy { .. } => Some("arena_copy"),
            Self::UnsafePath(_) => Some("arena_unsafe_path"),
            Self::Link(_) => Some("arena_link"),
            Self::FileBecameFolder(_) => Some("arena_file_became_folder"),
            Self::Changed => Some("arena_changed"),
            Self::Remove(_) => Some("arena_remove"),
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
/// never leave the folder, name a drive or an alternate data stream, or reach a git folder.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains(['\\', ':', '\0'])
            || Path::new(part).is_absolute()
            || !plain_name(part)
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

/// One component the file system takes as written and that names no git folder. Windows drops
/// trailing dots and spaces (`.git.` opens `.git`, measured in the wiki page) and opens a device
/// for `nul` or `com1.txt`; the NTFS short name `git~1` and the HFS+ spellings of `.git` are
/// refused everywhere ([`git_actions::names_git_folder`]).
fn plain_name(part: &str) -> bool {
    let trimmed = part.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || git_actions::names_git_folder(trimmed) {
        return false;
    }
    !(cfg!(windows) && (trimmed.len() != part.len() || is_device_name(part)))
}

/// A Windows device name, also with an extension or trailing spaces: `CON`, `nul.txt`, `COM1`,
/// `lpt²`.
fn is_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let Some(number) = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    else {
        return false;
    };
    let mut chars = number.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
    )
}

/// The paths of a `git diff --raw -z` whose old or new mode is a symbolic link (120000) or a
/// submodule (160000).
pub fn linked_entries(raw: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut records = raw.split(|b| *b == 0);
    while let Some(header) = records.next() {
        let Some(header) = header.strip_prefix(b":") else {
            continue;
        };
        let Some(path) = records.next() else {
            break;
        };
        let linked = header
            .split(|b| *b == b' ')
            .take(2)
            .any(|mode| mode == b"120000" || mode == b"160000");
        if linked {
            found.push(String::from_utf8_lossy(path).into_owned());
        }
    }
    found
}

/// Whether a component of `relative` below `root` is a link (on Windows any reparse point, so
/// junctions too). The walk ends at the first component that does not exist.
fn linked_on_the_way(root: &Path, relative: &Path) -> bool {
    let mut current = root.to_path_buf();
    for part in relative.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if git_actions::is_link(&meta) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
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

/// Tracked files the winner turned into a folder of new files (`a` became `a/b`), sorted. New
/// files are copied before the patch removes the file, so such a folder cannot be made.
pub fn files_turned_folders(tracked: &[String], untracked: &[String]) -> Vec<String> {
    let folders: BTreeSet<String> = untracked
        .iter()
        .flat_map(|path| {
            path.match_indices('/')
                .map(|(end, _)| fold(&path[..end]))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut found: Vec<String> = tracked
        .iter()
        .filter(|path| folders.contains(&fold(path)))
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
    // A cut list could hide a conflict; only a complete one counts. git can exit cleanly after
    // writing past the limit, before it is stopped, so `success` alone does not prove the list
    // is whole.
    if output.truncated {
        return Err(ArenaError::ProjectTooLarge);
    }
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
/// copied. `expected` is the file list the user confirmed (from [`diff_stat`]); when the changes
/// differ from it by now, nothing is touched. Refuses, before touching anything, when the project
/// folder has its own changes in any of those files or a new file is already there, when a
/// tracked file became a folder of new files, and when a path could reach a git folder or passes
/// a link. New files go in before the patch, and are
/// taken back when the patch fails.
pub fn apply(
    git: &Git,
    base: &Path,
    worktree: &Worktree,
    base_commit: &str,
    expected: &[String],
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
    let mut all_changed: Vec<String> = tracked.iter().chain(untracked.iter()).cloned().collect();
    all_changed.sort();
    all_changed.dedup();
    let mut confirmed = expected.to_vec();
    confirmed.sort();
    confirmed.dedup();
    if all_changed != confirmed {
        return Err(ArenaError::Changed);
    }
    let relative_paths = |names: &[String]| -> Result<Vec<(String, PathBuf)>, ArenaError> {
        names
            .iter()
            .map(|name| {
                safe_relative(name)
                    .map(|relative| (name.clone(), relative))
                    .ok_or_else(|| ArenaError::UnsafePath(name.clone()))
            })
            .collect()
    };
    let tracked_files = relative_paths(&tracked)?;
    let new_files = relative_paths(&untracked)?;
    if let Some(file) = files_turned_folders(&tracked, &untracked)
        .into_iter()
        .next()
    {
        return Err(ArenaError::FileBecameFolder(file));
    }
    if !tracked.is_empty() {
        // The same arguments as the patch below, so this sees exactly what `git apply` would get.
        let mut args = neutral_args(&neutral, git::DIFF_ARGS);
        args.extend([
            "--raw",
            "-z",
            "--no-renames",
            "--no-abbrev",
            base_commit,
            "--",
        ]);
        let raw = git.run(&worktree.path, args, deadline, git::MAX_LIST_BYTES)?;
        if raw.truncated {
            return Err(ArenaError::TooLarge);
        }
        if !raw.success {
            return Err(raw.failure().into());
        }
        if let Some(linked) = linked_entries(&raw.stdout).into_iter().next() {
            return Err(ArenaError::Link(linked));
        }
    }

    let target_neutral = git::filter_overrides(git, target, deadline)?;
    let dirty = dirty_files(git, target, &target_neutral, deadline)?;
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
    // New files must be plain files reached without a link; checked again on the open handle
    // when copied.
    for (name, relative) in &new_files {
        if linked_on_the_way(&worktree.path, relative) {
            return Err(ArenaError::Link(name.clone()));
        }
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
    // Nothing is written through a link in the project folder, and no new folder lands in its
    // git folder (a short name or an alias of `.git` that got past the name check).
    let place = Place::of(git, target, deadline)?;
    for (name, relative) in tracked_files.iter().chain(new_files.iter()) {
        if linked_on_the_way(target, relative) {
            return Err(ArenaError::Link(name.clone()));
        }
    }
    for (name, relative) in &new_files {
        place.check(name, &target.join(relative))?;
    }

    // The patch is made before anything changes; new files go in first, because only the copy
    // can fail halfway (and is taken back), while `git apply` is all or nothing. Taking a patch
    // back with `git apply -R` would not restore the bytes (line endings), so it is never needed.
    let patch = if tracked.is_empty() {
        None
    } else {
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
        Some(patch.stdout)
    };
    let created = copy_new_files(&worktree.path, target, &place, &new_files)?;
    if let Some(patch) = patch {
        let args = neutral_args(&target_neutral, &["apply", "--whitespace=nowarn", "-"]);
        let applied = git
            .run_with_input(target, args, Some(patch), deadline, 64 * 1024)
            .map_err(ArenaError::from)
            .and_then(|applied| {
                if applied.success {
                    Ok(())
                } else {
                    Err(ArenaError::Apply(git::message(&applied.stderr)))
                }
            });
        if let Err(error) = applied {
            created.undo();
            return Err(error);
        }
    }
    Ok(ApplyOutcome::Applied { files: all_changed })
}

/// Where new files may go: inside the project folder once resolved, never in its git folder.
struct Place {
    real_target: PathBuf,
    git_dir: PathBuf,
}

impl Place {
    fn of(git: &Git, target: &Path, deadline: Instant) -> Result<Self, ArenaError> {
        let real_target = target.canonicalize().map_err(|error| ArenaError::Copy {
            path: ".".to_owned(),
            reason: error.to_string(),
        })?;
        let output = git.run(
            target,
            ["rev-parse", "--absolute-git-dir"],
            deadline,
            64 * 1024,
        )?;
        if !output.success {
            return Err(output.failure().into());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let git_dir = Path::new(text.trim_end_matches(['\r', '\n']))
            .canonicalize()
            .map_err(|error| ArenaError::Copy {
                path: ".git".to_owned(),
                reason: error.to_string(),
            })?;
        Ok(Self {
            real_target,
            git_dir,
        })
    }

    /// The deepest folder of `destination` that exists, resolved, must be inside the project
    /// folder and outside its git folder, so no folder is ever created behind a link or in `.git`.
    fn check(&self, name: &str, destination: &Path) -> Result<(), ArenaError> {
        let Some(parent) = destination.parent() else {
            return Ok(());
        };
        let existing = parent
            .ancestors()
            .find(|dir| dir.exists())
            .unwrap_or(parent);
        self.check_folder(name, existing)
    }

    fn check_folder(&self, name: &str, dir: &Path) -> Result<(), ArenaError> {
        let real = dir.canonicalize().map_err(|error| ArenaError::Copy {
            path: name.to_owned(),
            reason: error.to_string(),
        })?;
        if !real.starts_with(&self.real_target) {
            return Err(ArenaError::Copy {
                path: name.to_owned(),
                reason: "the folder leads outside the project".to_owned(),
            });
        }
        if real.starts_with(&self.git_dir) {
            return Err(ArenaError::UnsafePath(name.to_owned()));
        }
        Ok(())
    }
}

/// Files and folders a copy created, so a failed copy can take them back.
#[derive(Debug, Default)]
struct Created {
    files: Vec<PathBuf>,
    /// In the order they were made, the outermost first.
    dirs: Vec<PathBuf>,
}

impl Created {
    fn undo(self) {
        for file in &self.files {
            let _ = fs::remove_file(file);
        }
        for dir in self.dirs.iter().rev() {
            // Only empty folders go; anything else in them was there before or is not ours.
            let _ = fs::remove_dir(dir);
        }
    }
}

/// Opens a new file of the worktree for reading without following a link at its end, and checks
/// its type on the open handle: an agent could swap the checked file for a link in between.
fn open_plain(path: &Path) -> std::io::Result<(fs::File, fs::Metadata)> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || git_actions::is_link(&meta) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok((file, meta))
}

/// Copies new files from the worktree into the project folder. Each target folder is checked by
/// [`Place`] before and after it is created, each file is created new (never written through
/// something already there). On the first failure everything this copy created goes again;
/// otherwise the caller gets what was created, to take it back if the patch fails.
fn copy_new_files(
    from: &Path,
    target: &Path,
    place: &Place,
    files: &[(String, PathBuf)],
) -> Result<Created, ArenaError> {
    let mut created = Created::default();
    for (name, relative) in files {
        if let Err(error) = copy_new_file(from, target, place, name, relative, &mut created) {
            created.undo();
            return Err(error);
        }
    }
    Ok(created)
}

fn copy_new_file(
    from: &Path,
    target: &Path,
    place: &Place,
    name: &str,
    relative: &Path,
    created: &mut Created,
) -> Result<(), ArenaError> {
    let copy_error = |error: std::io::Error| ArenaError::Copy {
        path: name.to_owned(),
        reason: error.to_string(),
    };
    let destination = target.join(relative);
    if let Some(parent) = destination.parent() {
        place.check(name, &destination)?;
        let missing: Vec<PathBuf> = parent
            .ancestors()
            // `symlink_metadata`, not `exists`: a dangling link is there already and not ours, and
            // `remove_dir` in `undo` would take a dangling junction away.
            .take_while(|dir| fs::symlink_metadata(dir).is_err())
            .map(Path::to_path_buf)
            .collect();
        // Noted first, so a folder made before `create_dir_all` failed goes again too.
        created.dirs.extend(missing.into_iter().rev());
        fs::create_dir_all(parent).map_err(copy_error)?;
        place.check_folder(name, parent)?;
    }
    let (mut source, meta) = open_plain(&from.join(relative)).map_err(copy_error)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(meta.permissions().mode() & 0o777);
    }
    #[cfg(not(unix))]
    let _ = &meta;
    let mut out = options.open(&destination).map_err(copy_error)?;
    created.files.push(destination);
    std::io::copy(&mut source, &mut out).map_err(copy_error)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Discarding a contestant

/// Removes a contestant's worktree and branch with everything in it, after the user confirmed
/// it in the arena (unlike [`worktree::remove`], which never forces). Only arena branches and only
/// a folder git lists as a worktree of the repository; no hook (switched off by [`Git`] itself)
/// and no filter of the repository runs. Hangar deletes the folder itself, because its content
/// comes from an agent: `remove_dir_all` removes a link or junction without entering it (Rust
/// 1.58.1 and later, CVE-2022-21658), while `git worktree remove --force` left a junction behind
/// (measured, wiki page). git's record then goes with `worktree remove` for this one worktree;
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
    let listed = |deadline: Instant| {
        worktree::list(git, root, deadline).map(|listed| {
            listed
                .iter()
                .any(|entry| git::path_key(&entry.path) == git::path_key(&worktree.path))
        })
    };
    if fs::symlink_metadata(&worktree.path).is_ok() {
        // Only a folder git names as this repository's worktree is ever deleted.
        if !listed(deadline)? {
            return Err(WorktreeError::OutsideFolder.into());
        }
        fs::remove_dir_all(&worktree.path)
            .map_err(|error| ArenaError::Remove(error.to_string()))?;
    }
    let neutral = git::filter_overrides(git, root, deadline)?;
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
    // Record gone already (pruned after the folder was deleted by hand): nothing is left.
    if !output.success && listed(deadline).unwrap_or(true) {
        return Err(output.failure().into());
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
            (ArenaError::ProjectTooLarge, "arena_project_too_large"),
            (ArenaError::Apply("conflict".to_owned()), "arena_apply"),
            (
                ArenaError::Copy {
                    path: "a.txt".to_owned(),
                    reason: "outside".to_owned(),
                },
                "arena_copy",
            ),
            (
                ArenaError::UnsafePath(".git./x".to_owned()),
                "arena_unsafe_path",
            ),
            (ArenaError::Link("lnk".to_owned()), "arena_link"),
            (
                ArenaError::FileBecameFolder("a".to_owned()),
                "arena_file_became_folder",
            ),
            (ArenaError::Changed, "arena_changed"),
            (ArenaError::Remove("in use".to_owned()), "arena_remove"),
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
        // Every spelling of a git folder, on every platform.
        for bad in [
            ".git/x",
            ".GIT/x",
            "sub/.Git/hooks/pre-commit",
            ".git./x",
            ".git /x",
            ".git. ./x",
            "GIT~1/hooks/x",
            "sub/git~12/config",
            ".g\u{200c}it/x",
            "\u{feff}.git/x",
            "...",
        ] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
        for good in [
            ".github/workflows/ci.yml",
            ".gitignore",
            "git~notes.txt",
            "git~/x",
            "nullable.rs",
            "console.txt",
            "com10",
        ] {
            assert!(safe_relative(good).is_some(), "{good}");
        }
        // Names Windows changes on the way: trailing dots and spaces, devices.
        for name in [
            "a./b",
            "a /b",
            "notes.",
            "nul",
            "NUL.txt",
            "sub/aux.c",
            "con ",
            "COM1",
            "lpt9.log",
            "com\u{b9}",
            "CONOUT$",
        ] {
            assert_eq!(safe_relative(name).is_none(), cfg!(windows), "{name:?}");
        }
    }

    #[test]
    fn raw_diff_names_links_and_submodules() {
        let zero = "0".repeat(40);
        let one = "1".repeat(40);
        let raw = format!(
            ":100644 100644 {one} {zero} M\0plain.txt\0\
             :000000 120000 {zero} {one} A\0new link\0\
             :120000 100644 {one} {zero} T\0was a link\0\
             :160000 000000 {one} {zero} D\0vendor/sub\0\
             :100755 100644 {one} {zero} M\0script.sh\0"
        );
        assert_eq!(
            linked_entries(raw.as_bytes()),
            ["new link", "was a link", "vendor/sub"]
        );
        assert!(linked_entries(b"").is_empty());
        // A hash that starts like a mode is no mode.
        let raw = format!(":100644 100644 120000{} {zero} M\0a\0", &one[6..]);
        assert!(linked_entries(raw.as_bytes()).is_empty());
    }

    /// A place for new files in `target`, with `target/.git` as its git folder.
    fn place(target: &Path) -> Place {
        fs::create_dir_all(target.join(".git").join("hooks")).unwrap();
        Place {
            real_target: target.canonicalize().unwrap(),
            git_dir: target.join(".git").canonicalize().unwrap(),
        }
    }

    fn new_file(name: &str) -> (String, PathBuf) {
        (name.to_owned(), name.split('/').collect())
    }

    #[test]
    fn new_files_never_land_in_the_git_folder() {
        let tmp = TempDir::new("arena-gitdir");
        let from = tmp.path().join("from");
        let target = tmp.path().join("target");
        let place = place(&target);
        fs::create_dir_all(from.join(".git").join("hooks")).unwrap();
        fs::write(from.join(".git").join("hooks").join("x"), "x\n").unwrap();
        // Past the name check (apply never hands such a name over): the resolved folder still
        // stops it.
        let refused = copy_new_files(&from, &target, &place, &[new_file(".git/hooks/x")]);
        assert!(
            matches!(refused, Err(ArenaError::UnsafePath(_))),
            "{refused:?}"
        );
        assert!(!target.join(".git").join("hooks").join("x").exists());
        if cfg!(windows) {
            // Measured: Windows drops the trailing dot, so `.git.` opens the git folder.
            assert!(target.join(".git.").join("hooks").is_dir());
            let refused = copy_new_files(
                &from,
                &target,
                &place,
                &[(
                    ".git./hooks/x".to_owned(),
                    Path::new(".git.").join("hooks").join("x"),
                )],
            );
            assert!(
                matches!(refused, Err(ArenaError::UnsafePath(_))),
                "{refused:?}"
            );
            assert!(!target.join(".git").join("hooks").join("x").exists());
        }
        #[cfg(windows)]
        {
            // The NTFS short name, where the volume has one or fsutil may set one (it needs the
            // privilege to restore files; without it this case is skipped).
            let _ = Command::new("fsutil")
                .args(["file", "setshortname"])
                .arg(target.join(".git"))
                .arg("GIT~1")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if target.join("GIT~1").join("hooks").is_dir() {
                let refused = copy_new_files(
                    &from,
                    &target,
                    &place,
                    &[(
                        "GIT~1/hooks/x".to_owned(),
                        Path::new("GIT~1").join("hooks").join("x"),
                    )],
                );
                assert!(
                    matches!(refused, Err(ArenaError::UnsafePath(_))),
                    "{refused:?}"
                );
                assert!(!target.join(".git").join("hooks").join("x").exists());
            }
        }
    }

    /// A link to a folder: a junction on Windows (no privilege needed), a symlink elsewhere.
    fn folder_link(link: &Path, to: &Path) {
        #[cfg(windows)]
        {
            let status = Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(to)
                .stdout(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "mklink /J failed");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(to, link).unwrap();
    }

    #[test]
    fn new_files_never_create_folders_behind_a_link() {
        let tmp = TempDir::new("arena-link");
        let from = tmp.path().join("from");
        let target = tmp.path().join("target");
        let elsewhere = tmp.path().join("elsewhere");
        for dir in [&from, &target, &elsewhere] {
            fs::create_dir_all(dir).unwrap();
        }
        let place = place(&target);
        fs::create_dir_all(from.join("link").join("a")).unwrap();
        fs::write(from.join("link").join("a").join("x.txt"), "x\n").unwrap();
        folder_link(&target.join("link"), &elsewhere);
        assert!(linked_on_the_way(&target, &Path::new("link").join("a")));
        assert!(!linked_on_the_way(&from, &Path::new("link").join("a")));
        let refused = copy_new_files(&from, &target, &place, &[new_file("link/a/x.txt")]);
        assert!(
            matches!(refused, Err(ArenaError::Copy { .. })),
            "{refused:?}"
        );
        assert!(!elsewhere.join("a").exists());
    }

    #[test]
    fn a_failed_copy_takes_back_what_it_made() {
        let tmp = TempDir::new("arena-undo");
        let from = tmp.path().join("from");
        let target = tmp.path().join("target");
        let place = place(&target);
        fs::create_dir_all(from.join("a").join("b")).unwrap();
        fs::write(from.join("a").join("b").join("one.txt"), "1\n").unwrap();
        fs::write(from.join("mine.txt"), "agent\n").unwrap();
        fs::write(target.join("mine.txt"), "mine\n").unwrap();
        // A source that is gone, and a file that would overwrite the user's own.
        for second in ["a/c/missing.txt", "mine.txt"] {
            let files = [new_file("a/b/one.txt"), new_file(second)];
            let refused = copy_new_files(&from, &target, &place, &files);
            assert!(
                matches!(refused, Err(ArenaError::Copy { .. })),
                "{second}: {refused:?}"
            );
            assert!(!target.join("a").exists(), "{second}");
            assert_eq!(
                fs::read_to_string(target.join("mine.txt")).unwrap(),
                "mine\n"
            );
        }
        copy_new_files(&from, &target, &place, &[new_file("a/b/one.txt")]).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("a").join("b").join("one.txt")).unwrap(),
            "1\n"
        );
    }

    #[test]
    fn sources_are_opened_without_following_a_link() {
        let tmp = TempDir::new("arena-open");
        let real = tmp.path().join("real.txt");
        fs::write(&real, "secret\n").unwrap();
        assert!(open_plain(&real).is_ok());
        assert!(open_plain(tmp.path()).is_err());
        let folder = tmp.path().join("folder");
        fs::create_dir_all(&folder).unwrap();
        folder_link(&tmp.path().join("folder-link"), &folder);
        assert!(open_plain(&tmp.path().join("folder-link")).is_err());
        let link = tmp.path().join("link.txt");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&real, &link).is_ok();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&real, &link).is_ok();
        // A file symlink needs developer mode on Windows; without it the junction case stands.
        if made {
            assert!(open_plain(&link).is_err());
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
    fn files_turned_folders_are_the_parents_of_new_files() {
        let tracked = labels(&["a", "src/b.ts", "c.txt"]);
        assert!(files_turned_folders(
            &tracked,
            &labels(&["a.txt", "ab/x", "src/b.ts.d", "x/a/y", "a"])
        )
        .is_empty());
        assert_eq!(
            files_turned_folders(&tracked, &labels(&["a/x", "a/y", "src/b.ts/deep/z"])),
            ["a", "src/b.ts"]
        );
        let other_case = files_turned_folders(&tracked, &labels(&["C.TXT/x"]));
        if cfg!(any(windows, target_os = "macos")) {
            assert_eq!(other_case, ["c.txt"]);
        } else {
            assert!(other_case.is_empty());
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

    /// The file list the comparison shows, as the user confirms it.
    fn shown(git: &Git, base: &Path, worktree: &Worktree, base_commit: &str) -> Vec<String> {
        diff_stat(git, base, worktree, base_commit)
            .map(|stat| stat.files.into_iter().map(|file| file.path).collect())
            .unwrap_or_default()
    }

    fn apply_shown(
        git: &Git,
        base: &Path,
        worktree: &Worktree,
        base_commit: &str,
    ) -> Result<ApplyOutcome, ArenaError> {
        apply(
            git,
            base,
            worktree,
            base_commit,
            &shown(git, base, worktree, base_commit),
        )
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
            apply_shown(&git, &base, winner, &base_commit),
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
        let outcome = apply_shown(&git, &base, winner, &base_commit).unwrap();
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
        let outcome = apply_shown(&git, &base, winner, &base_commit).unwrap();
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
            let refused = apply_shown(&git, &base, &moved, &base_commit);
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
        let outcome = apply_shown(&git, &base, winner, &base_commit).unwrap();
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
            apply_shown(&git, &base, &plain, &base_commit),
            Err(ArenaError::NotArena)
        ));
        assert!(plain.path.exists());
    }

    /// A repository with one committed `readme.txt` and an arena of two from that commit.
    fn small_arena(git: &Git, tmp: &TempDir) -> (PathBuf, PathBuf, String, Arena) {
        let base = tmp.path().join(worktree::DIR_NAME);
        let repo = tmp.path().join("repo");
        init_repo(git, &repo);
        fs::write(repo.join("readme.txt"), "one\n").unwrap();
        commit_all(git, &repo, "first");
        let base_commit = head(git, &repo);
        let arena = create(git, &base, &repo, &labels(&["A", "B"])).unwrap();
        (base, repo, base_commit, arena)
    }

    fn remove_folder_link(link: &Path) {
        // A junction goes as a folder, a symlink as a file; neither touches what it points to.
        fs::remove_dir(link)
            .or_else(|_| fs::remove_file(link))
            .unwrap();
    }

    #[test]
    fn links_late_changes_and_failed_copies_leave_the_project_untouched() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("arena-guard");
        let (base, repo, base_commit, arena) = small_arena(&git, &tmp);
        let winner = &arena.worktrees[0];
        let readme = || fs::read_to_string(repo.join("readme.txt")).unwrap();

        // A tracked symbolic link (where symlinks are off a plain file, mode 120000 still).
        let blob = git
            .run_with_input(
                &winner.path,
                ["hash-object", "-w", "--stdin"],
                Some(b"../outside".to_vec()),
                Instant::now() + Duration::from_secs(30),
                4096,
            )
            .unwrap();
        let blob = String::from_utf8_lossy(&blob.stdout).trim().to_owned();
        let entry = format!("120000,{blob},lnk");
        git_ok(
            &git,
            &winner.path,
            &["update-index", "--add", "--cacheinfo", &entry],
        );
        git_ok(&git, &winner.path, &["checkout", "--", "lnk"]);
        fs::write(winner.path.join("readme.txt"), "two\n").unwrap();
        let refused = apply_shown(&git, &base, winner, &base_commit);
        assert!(
            matches!(&refused, Err(ArenaError::Link(path)) if path == "lnk"),
            "{refused:?}"
        );
        assert_eq!(status(&git, &repo), "");
        git_ok(&git, &winner.path, &["rm", "--quiet", "--cached", "lnk"]);
        fs::remove_file(winner.path.join("lnk")).unwrap();

        // The user confirmed a list; a file the agent adds afterwards never comes along unseen.
        let confirmed = shown(&git, &base, winner, &base_commit);
        assert_eq!(confirmed, ["readme.txt"]);
        fs::write(winner.path.join(".envrc"), "x\n").unwrap();
        assert!(matches!(
            apply(&git, &base, winner, &base_commit, &confirmed),
            Err(ArenaError::Changed)
        ));
        fs::remove_file(winner.path.join(".envrc")).unwrap();
        assert_eq!(status(&git, &repo), "");

        // A new file whose folder in the project is a link: refused before the patch goes in.
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(winner.path.join("docs")).unwrap();
        let new_md = winner.path.join("docs").join("new.md");
        fs::write(&new_md, "new\n").unwrap();
        folder_link(&repo.join("docs"), &outside);
        let refused = apply_shown(&git, &base, winner, &base_commit);
        assert!(matches!(refused, Err(ArenaError::Link(_))), "{refused:?}");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(readme(), "one\n");
        remove_folder_link(&repo.join("docs"));

        // A new file that cannot be read once the patch is in: the patch is taken back.
        {
            #[cfg(windows)]
            let lock = {
                use std::os::windows::fs::OpenOptionsExt;
                Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(&new_md)
                        .unwrap(),
                )
            };
            #[cfg(unix)]
            let lock = {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&new_md, fs::Permissions::from_mode(0o000)).unwrap();
                // Root reads it anyway; then there is no failure to provoke.
                fs::File::open(&new_md).is_err().then_some(())
            };
            if lock.is_some() {
                let refused = apply_shown(&git, &base, winner, &base_commit);
                assert!(
                    matches!(refused, Err(ArenaError::Copy { .. })),
                    "{refused:?}"
                );
                assert_eq!(readme(), "one\n");
                assert_eq!(status(&git, &repo), "");
                assert!(!repo.join("docs").exists());
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&new_md, fs::Permissions::from_mode(0o644)).unwrap();
        }

        let outcome = apply_shown(&git, &base, winner, &base_commit).unwrap();
        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                files: labels(&["docs/new.md", "readme.txt"])
            }
        );
        assert_eq!(readme().replace("\r\n", "\n"), "two\n");
    }

    #[test]
    fn discarding_never_follows_a_link_out_of_the_worktree() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("arena-discard");
        let (base, repo, _, arena) = small_arena(&git, &tmp);
        let loser = &arena.worktrees[1];
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("marker.txt"), "keep\n").unwrap();
        folder_link(&loser.path.join("linked"), &outside);
        // A read-only file an agent left behind goes too.
        let read_only = loser.path.join("read-only.txt");
        fs::write(&read_only, "x\n").unwrap();
        let mut permissions = fs::metadata(&read_only).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&read_only, permissions).unwrap();

        discard(&git, &base, loser).unwrap();
        assert!(!loser.path.exists());
        assert_eq!(
            fs::read_to_string(outside.join("marker.txt")).unwrap(),
            "keep\n"
        );
        let listed = worktree::list(&git, &repo, Instant::now() + Duration::from_secs(30)).unwrap();
        assert!(!listed
            .iter()
            .any(|entry| git::path_key(&entry.path) == git::path_key(&loser.path)));

        // A folder of the right shape that git does not list as a worktree is never deleted.
        let fake = Worktree {
            path: base
                .join(worktree::repo_dir_name(&repo))
                .join("arena-1a2b3c-fake"),
            branch: "hangar/arena-1a2b3c-fake".to_owned(),
            repo_root: repo.clone(),
        };
        fs::create_dir_all(&fake.path).unwrap();
        fs::write(fake.path.join("keep.txt"), "keep\n").unwrap();
        let refused = discard(&git, &base, &fake);
        assert!(
            matches!(
                refused,
                Err(ArenaError::Worktree(WorktreeError::OutsideFolder))
            ),
            "{refused:?}"
        );
        assert!(fake.path.join("keep.txt").is_file());
    }

    #[test]
    fn a_failed_patch_takes_the_new_files_back() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("arena-patch");
        let (base, repo, base_commit, arena) = small_arena(&git, &tmp);
        let winner = &arena.worktrees[0];
        fs::write(winner.path.join("readme.txt"), "agent\n").unwrap();
        fs::create_dir_all(winner.path.join("docs").join("deep")).unwrap();
        fs::write(
            winner.path.join("docs").join("deep").join("new.md"),
            "new\n",
        )
        .unwrap();
        // The project moved on in the same file: the patch no longer fits.
        fs::write(repo.join("readme.txt"), "user\n").unwrap();
        commit_all(&git, &repo, "user");
        let refused = apply_shown(&git, &base, winner, &base_commit);
        assert!(matches!(refused, Err(ArenaError::Apply(_))), "{refused:?}");
        assert!(!repo.join("docs").exists());
        assert_eq!(status(&git, &repo), "");
    }

    #[test]
    fn a_file_that_became_a_folder_is_refused_before_anything_changes() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("arena-file-folder");
        let (base, repo, base_commit, arena) = small_arena(&git, &tmp);
        let winner = &arena.worktrees[0];
        fs::write(winner.path.join("new.txt"), "new\n").unwrap();
        fs::remove_file(winner.path.join("readme.txt")).unwrap();
        fs::create_dir_all(winner.path.join("readme.txt")).unwrap();
        fs::write(winner.path.join("readme.txt").join("part.md"), "part\n").unwrap();
        let refused = apply_shown(&git, &base, winner, &base_commit);
        assert!(
            matches!(&refused, Err(ArenaError::FileBecameFolder(path)) if path == "readme.txt"),
            "{refused:?}"
        );
        assert_eq!(
            fs::read_to_string(repo.join("readme.txt")).unwrap(),
            "one\n"
        );
        assert!(!repo.join("new.txt").exists());
        assert_eq!(status(&git, &repo), "");
    }
}
