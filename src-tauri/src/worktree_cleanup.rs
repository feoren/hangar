//! Worktree cleanup (contract: docs/wiki/pages/syntheses/agent-workflow-features.md, "Worktree
//! cleanup"): the worktrees Hangar created, with the state that decides what may happen to them,
//! and their removal.
//!
//! Only worktrees in Hangar's own layout count: `<base>/<repo dir>/<slug>` with branch
//! `hangar/<slug>`, where the repo dir is the one of the repository git keeps them in
//! ([`worktree::check_managed`]). Every other worktree of a repository, and every other folder,
//! is never listed and never touched. They are found in two ways: the folders below `base`
//! (their `.git` file names the repository), and `git worktree list` of every repository Hangar
//! knows, which also finds the records of folders deleted by hand.
//!
//! The window's list is only a view. Removing names a path and its repository, and the state is
//! checked again right before anything happens; removal itself never forces
//! ([`worktree::remove_folder`]), so a change made after the check still keeps the worktree.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::git::{self, Git};
use crate::persist::read_limited;
use crate::worktree::{self, Kept, Listed, Worktree, WorktreeError, BRANCH_PREFIX};

/// Folders the window may name per call (recent folders, favorites, sessions' repositories).
pub const MAX_FOLDERS: usize = 64;
/// Repositories and entries per listing; beyond that the list is cut.
const MAX_REPOS: usize = 64;
const MAX_ENTRIES: usize = 256;
/// A whole listing; entries not checked in time are listed without a state.
const LIST_BUDGET: Duration = Duration::from_secs(45);
/// A `.git` file holds one line; anything larger is not one.
const MAX_DOT_GIT_BYTES: usize = 4096;
/// The size of one worktree: files counted and time spent before the sum is given as a lower
/// bound.
pub const SIZE_MAX_FILES: u64 = 200_000;
pub const SIZE_BUDGET: Duration = Duration::from_secs(5);
/// Branch names a repository's default branch may have when `origin/HEAD` names none.
const DEFAULT_NAMES: [&str; 2] = ["main", "master"];

/// What decides the actions of one worktree. Serialized as `{ "kind": "...", ... }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum CleanupState {
    /// A session works in it (running or not), or the saved workspace brings one back there.
    InUse,
    /// The folder is gone; git still keeps its record.
    Missing,
    /// Changed or untracked files.
    Changes,
    /// Nothing changed, but ignored files (build output, `.env`), which `git worktree remove`
    /// would delete without asking.
    Ignored,
    /// Nothing changed, and every commit of the branch is in the default branch: folder and
    /// branch go.
    Merged,
    /// Nothing changed, the branch has commits of its own: the folder goes, the branch stays.
    Clean,
    /// Cannot be checked. `detail` is git's own message for [`Cause::GitFailed`] or the branch
    /// checked out for [`Cause::OtherBranch`], never a sentence of Hangar's.
    Unknown {
        cause: Cause,
        detail: Option<String>,
    },
}

/// Why a worktree cannot be checked; the window words each one itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    RepoNotFound,
    NotKnownToGit,
    OtherBranch,
    NoBranch,
    Locked,
    NotCheckedInTime,
    RemoteRepo,
    GitFailed,
}

/// A cause with its detail, as [`CleanupState::Unknown`] carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub cause: Cause,
    pub detail: Option<String>,
}

impl Problem {
    pub fn new(cause: Cause) -> Self {
        Self {
            cause,
            detail: None,
        }
    }

    /// git's own message is kept (`GitError::Failed`); Hangar's own error texts are not shown.
    pub fn of(error: &git::GitError) -> Self {
        use git::GitError as E;
        match error {
            E::Timeout => Self::new(Cause::NotCheckedInTime),
            E::RemoteGitDir => Self::new(Cause::RemoteRepo),
            E::InvalidPath(_) | E::NotARepository | E::ForeignWorkTree => {
                Self::new(Cause::RepoNotFound)
            }
            E::Failed(message) => Self {
                cause: Cause::GitFailed,
                detail: Some(message.clone()),
            },
            _ => Self::new(Cause::GitFailed),
        }
    }

    fn state(&self) -> CleanupState {
        CleanupState::Unknown {
            cause: self.cause,
            detail: self.detail.clone(),
        }
    }
}

/// What the window may offer for a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Remove,
    Prune,
}

impl CleanupState {
    pub fn action(&self) -> Option<Action> {
        match self {
            Self::Merged | Self::Clean => Some(Action::Remove),
            Self::Missing => Some(Action::Prune),
            Self::InUse | Self::Changes | Self::Ignored | Self::Unknown { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupEntry {
    pub path: PathBuf,
    /// `None` when the repository could not be found.
    pub repo_root: Option<PathBuf>,
    /// `hangar/<slug>`.
    pub branch: String,
    /// Unix time in milliseconds of the newest of the folder, its `.git` file and git's index,
    /// HEAD and reflog of the worktree; `None` for a missing folder.
    pub modified: Option<u64>,
    pub state: CleanupState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    /// Folder and git's record are gone.
    pub done: bool,
    pub branch_deleted: bool,
    /// The state checked right before; when it no longer allowed the action nothing happened.
    pub state: CleanupState,
    /// Why the worktree stayed although its state allowed the action.
    pub reason: Option<Kept>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSize {
    pub bytes: u64,
    pub files: u64,
    /// `false`: cut at [`SIZE_MAX_FILES`] or [`SIZE_BUDGET`]; `bytes` is a lower bound.
    pub complete: bool,
    /// Unix time in milliseconds of the newest file seen.
    pub newest: Option<u64>,
}

/// What a worktree holds, as `git status` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contents {
    Changes,
    Ignored,
    Clean,
    Unreadable(Problem),
}

/// Everything [`classify`] needs, gathered by [`inspect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub in_use: bool,
    pub exists: bool,
    /// Why the repository's side cannot be checked.
    pub problem: Option<Problem>,
    /// Only read for an existing folder without a problem.
    pub contents: Contents,
    pub merged: bool,
}

/// The state of one worktree. In use wins over everything, so nothing is ever offered for a
/// worktree a session works in; then what cannot be checked, then a missing folder.
pub fn classify(facts: &Facts) -> CleanupState {
    if facts.in_use {
        return CleanupState::InUse;
    }
    if let Some(problem) = &facts.problem {
        return problem.state();
    }
    if !facts.exists {
        return CleanupState::Missing;
    }
    match &facts.contents {
        Contents::Changes => CleanupState::Changes,
        Contents::Ignored => CleanupState::Ignored,
        Contents::Unreadable(problem) => problem.state(),
        Contents::Clean if facts.merged => CleanupState::Merged,
        Contents::Clean => CleanupState::Clean,
    }
}

/// A session's folder is `path` or lies inside it, by the text or, when that differs, by the
/// resolved paths: a session may name the worktree through a junction, a symlink or an 8.3
/// short name, and removing the folder under it would lose its work. Paths on a network share
/// are compared by their text alone, so the check never reaches out to another host.
pub fn in_use(path: &Path, occupied: &[PathBuf]) -> bool {
    let key = git::path_key(path);
    if occupied
        .iter()
        .any(|folder| git::key_within(&git::path_key(folder), &key))
    {
        return true;
    }
    let resolved = |path: &Path| {
        (!git::is_remote(path))
            .then(|| fs::canonicalize(path).ok())
            .flatten()
            .map(|real| git::path_key(&real))
    };
    let Some(real) = resolved(path) else {
        return false;
    };
    occupied
        .iter()
        .filter_map(|folder| resolved(folder))
        .any(|folder| git::key_within(&folder, &real))
}

/// How long a freshly created worktree counts as in use without a session in it. Starting a
/// session right after `create_worktree` takes seconds; this only bounds a start that failed.
pub const PENDING_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Worktrees created in this run that no session works in yet: between `create_worktree` and
/// the session's start, the cleanup would otherwise take such a worktree for merged or clean and
/// remove it under the starting session.
#[derive(Debug, Default)]
pub struct PendingWorktrees {
    created: Mutex<HashMap<String, (PathBuf, Instant)>>,
}

impl PendingWorktrees {
    fn created(&self) -> std::sync::MutexGuard<'_, HashMap<String, (PathBuf, Instant)>> {
        // The map stays consistent after a panic elsewhere: every change is a single insert or
        // remove.
        self.created
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn add(&self, path: &Path, now: Instant) {
        self.created()
            .insert(git::path_key(path), (path.to_path_buf(), now));
    }

    pub fn remove(&self, path: &Path) {
        self.created().remove(&git::path_key(path));
    }

    /// `sessions` plus every worktree still pending. A worktree a session works in now is no
    /// longer pending (the session holds it from here on), and one older than `timeout` neither.
    pub fn with_pending(
        &self,
        mut sessions: Vec<PathBuf>,
        now: Instant,
        timeout: Duration,
    ) -> Vec<PathBuf> {
        let mut created = self.created();
        created.retain(|_, (path, since)| {
            now.saturating_duration_since(*since) < timeout && !in_use(path, &sessions)
        });
        sessions.extend(created.values().map(|(path, _)| path.clone()));
        sessions
    }
}

/// What the window and the saved workspace occupy; passed in by the command.
pub struct Context<'a> {
    pub git: &'a Git,
    pub base: &'a Path,
    /// Folders of sessions and of saved sessions (and their worktrees).
    pub occupied: &'a [PathBuf],
}

/// One repository as git keeps it, read once per listing.
struct Repo {
    listed: Vec<Listed>,
    /// Existing refs of the default branch: local and `origin`'s.
    default_refs: Vec<String>,
}

fn repo(git: &Git, root: &Path, deadline: Instant) -> Result<Repo, Problem> {
    // A listing runs without a click; it never makes git contact another computer.
    if git::is_remote(root) {
        return Err(Problem::new(Cause::RemoteRepo));
    }
    git::check_folder(root).map_err(|error| Problem::of(&error))?;
    let listed = worktree::list(git, root, deadline).map_err(|error| Problem::of(&error))?;
    Ok(Repo {
        listed,
        default_refs: default_refs(git, root, deadline),
    })
}

/// `refs/heads/<name>` and `refs/remotes/origin/<name>` of the default branch, the ones that
/// exist: the name `origin/HEAD` points to, otherwise `main` or `master`.
fn default_refs(git: &Git, root: &Path, deadline: Instant) -> Vec<String> {
    let remote_head = git
        .run(
            root,
            ["symbolic-ref", "-q", "refs/remotes/origin/HEAD"],
            deadline,
            64 * 1024,
        )
        .ok()
        .filter(|output| output.success)
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .strip_prefix("refs/remotes/origin/")
                .map(str::to_owned)
        });
    let names: Vec<String> = match remote_head {
        Some(name) => vec![name],
        None => DEFAULT_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    };
    let mut refs = Vec::new();
    for name in names {
        for full in [
            format!("refs/heads/{name}"),
            format!("refs/remotes/origin/{name}"),
        ] {
            let exists = git
                .run(root, ["rev-parse", "--verify", "-q", &full], deadline, 4096)
                .is_ok_and(|output| output.success);
            if exists {
                refs.push(full);
            }
        }
        if !refs.is_empty() {
            break;
        }
    }
    refs
}

/// `tip` is in one of `refs` (`git merge-base --is-ancestor`).
fn merged(git: &Git, root: &Path, tip: &str, refs: &[String], deadline: Instant) -> bool {
    refs.iter().any(|target| {
        git.run(
            root,
            ["merge-base", "--is-ancestor", tip, target.as_str()],
            deadline,
            4096,
        )
        .is_ok_and(|output| output.success)
    })
}

fn contents(git: &Git, path: &Path, deadline: Instant) -> Contents {
    if let Err(error) = git::check_git_links(path) {
        return Contents::Unreadable(Problem::of(&error));
    }
    let neutral = match git::filter_overrides(git, path, deadline) {
        Ok(neutral) => neutral,
        Err(error) => return Contents::Unreadable(Problem::of(&error)),
    };
    match worktree::status_entries(git, path, &neutral, deadline) {
        Ok(entries) if entries.iter().any(|entry| entry.status != "!!") => Contents::Changes,
        Ok(entries) if !entries.is_empty() => Contents::Ignored,
        Ok(_) => Contents::Clean,
        Err(error) => Contents::Unreadable(Problem::of(&error)),
    }
}

/// What [`inspect`] found: the state, and for a branch merged into the default branch the
/// commit it pointed to.
struct Inspection {
    state: CleanupState,
    merged_tip: Option<String>,
}

/// The state of the worktree at `path` of the repository `root` (`None`: not found).
fn inspect(
    cx: &Context<'_>,
    path: &Path,
    root: Option<&Path>,
    repo: Option<&Result<Repo, Problem>>,
    deadline: Instant,
) -> Inspection {
    let exists = path.is_dir();
    let mut facts = Facts {
        in_use: in_use(path, cx.occupied),
        exists,
        problem: None,
        contents: Contents::Clean,
        merged: false,
    };
    if facts.in_use {
        return only(&facts);
    }
    let (Some(root), Some(repo)) = (root, repo) else {
        facts.problem = Some(Problem::new(Cause::RepoNotFound));
        return only(&facts);
    };
    let repo = match repo {
        Ok(repo) => repo,
        Err(problem) => {
            facts.problem = Some(problem.clone());
            return only(&facts);
        }
    };
    let key = git::path_key(path);
    let Some(listed) = repo
        .listed
        .iter()
        .find(|entry| git::path_key(&entry.path) == key)
    else {
        facts.problem = Some(Problem::new(Cause::NotKnownToGit));
        return only(&facts);
    };
    let expected = branch_of(path).map(|branch| format!("refs/heads/{branch}"));
    if listed.branch.is_none() || listed.branch != expected {
        facts.problem = Some(match &listed.branch {
            Some(branch) => Problem {
                cause: Cause::OtherBranch,
                detail: Some(
                    branch
                        .strip_prefix("refs/heads/")
                        .unwrap_or(branch)
                        .to_owned(),
                ),
            },
            None => Problem::new(Cause::NoBranch),
        });
        return only(&facts);
    }
    if listed.locked {
        facts.problem = Some(Problem::new(Cause::Locked));
        return only(&facts);
    }
    if Instant::now() >= deadline {
        facts.problem = Some(Problem::new(Cause::NotCheckedInTime));
        return only(&facts);
    }
    let tip = listed
        .head
        .clone()
        .filter(|tip| merged(cx.git, root, tip, &repo.default_refs, deadline));
    facts.merged = tip.is_some();
    if exists {
        facts.contents = contents(cx.git, path, deadline);
    }
    Inspection {
        state: classify(&facts),
        merged_tip: tip,
    }
}

fn only(facts: &Facts) -> Inspection {
    Inspection {
        state: classify(facts),
        merged_tip: None,
    }
}

/// `hangar/<slug>` from the folder name.
fn branch_of(path: &Path) -> Option<String> {
    let slug = path.file_name()?.to_str()?;
    worktree::is_valid_slug(slug).then(|| format!("{BRANCH_PREFIX}{slug}"))
}

/// The repository a worktree folder belongs to, from its `.git` file (`gitdir:
/// <root>/.git/worktrees/<name>`), as git wrote it when the worktree was created; `None` when
/// the file is missing or names no such folder. Nothing is resolved, so the spelling stays the
/// one the repo dir name was made from.
pub fn root_from_dot_git(path: &Path) -> Option<PathBuf> {
    let content = read_limited(&path.join(".git"), MAX_DOT_GIT_BYTES).ok()??;
    let target = git::parse_gitdir_file(&String::from_utf8_lossy(&content))?;
    let target = git::native_path(&target.to_string_lossy());
    let gitdir = if target.is_absolute() {
        target
    } else {
        path.join(target)
    };
    // `<common>/worktrees/<name>`, `<common>` being `<root>/.git`.
    let worktrees = gitdir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    let common = worktrees.parent()?;
    if common.file_name()? != ".git" {
        return None;
    }
    let root = lexical(common.parent()?);
    root.is_absolute().then_some(root)
}

/// `.` and `..` taken out without touching the file system.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Worktree folders below `base`: `<repo dir>/<slug>`, both valid names, plain folders (no
/// link, no reparse point).
fn scan_base(base: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let Ok(repos) = fs::read_dir(base) else {
        return found;
    };
    for repo in repos.flatten() {
        let Some(repo_dir) = plain_dir_name(&repo) else {
            continue;
        };
        let Ok(slugs) = fs::read_dir(repo.path()) else {
            continue;
        };
        for slug in slugs.flatten() {
            if plain_dir_name(&slug).is_some() {
                found.push((repo_dir.clone(), slug.path()));
                if found.len() >= MAX_ENTRIES {
                    return found;
                }
            }
        }
    }
    found
}

fn plain_dir_name(entry: &fs::DirEntry) -> Option<String> {
    let name = entry.file_name().into_string().ok()?;
    let meta = entry.metadata().ok()?;
    (worktree::is_valid_slug(&name) && meta.is_dir() && !crate::no_links::is_reparse_point(&meta))
        .then_some(name)
}

fn millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn modified(path: &Path) -> Option<u64> {
    let mut paths = vec![path.to_path_buf(), path.join(".git")];
    if let Some(gitdir) = read_limited(&path.join(".git"), MAX_DOT_GIT_BYTES)
        .ok()
        .flatten()
        .and_then(|content| git::parse_gitdir_file(&String::from_utf8_lossy(&content)))
    {
        let gitdir = git::native_path(&gitdir.to_string_lossy());
        let gitdir = if gitdir.is_absolute() {
            gitdir
        } else {
            path.join(gitdir)
        };
        if !git::is_remote(&gitdir) || git::is_remote(path) {
            paths.extend([
                gitdir.join("index"),
                gitdir.join("HEAD"),
                gitdir.join("logs").join("HEAD"),
            ]);
        }
    }
    paths
        .iter()
        .filter_map(|path| fs::symlink_metadata(path).ok()?.modified().ok())
        .filter_map(millis)
        .max()
}

/// Every worktree Hangar created, found below `base` and in the repositories of `folders` and
/// `known_roots`. Sorted by repository, then folder.
pub fn list(cx: &Context<'_>, folders: &[PathBuf], known_roots: &[PathBuf]) -> Vec<CleanupEntry> {
    let deadline = Instant::now() + LIST_BUDGET;
    let call_deadline = || deadline.min(Instant::now() + git::TIMEOUT);
    let scanned = scan_base(cx.base);

    // Repositories by repo dir name: the ones the folders' `.git` files name, the known ones,
    // and the repositories of the window's folders.
    let mut roots: HashMap<String, PathBuf> = HashMap::new();
    let add_root = |root: PathBuf, roots: &mut HashMap<String, PathBuf>| {
        if roots.len() < MAX_REPOS {
            roots.entry(worktree::repo_dir_name(&root)).or_insert(root);
        }
    };
    for (repo_dir, path) in &scanned {
        if let Some(root) = root_from_dot_git(path) {
            if worktree::repo_dir_name(&root) == *repo_dir {
                add_root(root, &mut roots);
            }
        }
    }
    for root in known_roots.iter().filter(|root| root.is_absolute()) {
        add_root(root.clone(), &mut roots);
    }
    let mut seen_folders = HashSet::new();
    for folder in folders.iter().take(MAX_FOLDERS) {
        if !seen_folders.insert(git::path_key(folder))
            || git::is_remote(folder)
            || git::check_folder(folder).is_err()
        {
            continue;
        }
        if let Ok(Some(root)) = git::toplevel(cx.git, folder, call_deadline()) {
            // A worktree's own folder is not a repository Hangar keeps worktrees for.
            if worktree::check_layout(cx.base, &root).is_err() {
                add_root(root, &mut roots);
            }
        }
    }

    let repos: HashMap<String, Result<Repo, Problem>> = roots
        .iter()
        .map(|(repo_dir, root)| (repo_dir.clone(), repo(cx.git, root, call_deadline())))
        .collect();

    // Entries by path key: every folder below `base`, and every record of a known repository
    // in Hangar's layout (folders deleted by hand only show up there).
    let mut candidates: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut keys = HashSet::new();
    for (repo_dir, path) in scanned {
        if keys.insert(git::path_key(&path)) {
            candidates.push((path, Some(repo_dir)));
        }
    }
    for (repo_dir, repo) in &repos {
        let (Ok(repo), Some(root)) = (repo, roots.get(repo_dir)) else {
            continue;
        };
        for listed in &repo.listed {
            let ours = worktree::check_managed(cx.base, &listed.path, root).is_ok()
                && branch_of(&listed.path).is_some();
            if ours && keys.insert(git::path_key(&listed.path)) {
                candidates.push((listed.path.clone(), Some(repo_dir.clone())));
            }
        }
    }
    candidates.truncate(MAX_ENTRIES);

    let mut entries: Vec<CleanupEntry> = candidates
        .into_iter()
        .filter_map(|(path, repo_dir)| {
            let branch = branch_of(&path)?;
            let root = repo_dir.as_ref().and_then(|dir| roots.get(dir));
            let repo = repo_dir.as_ref().and_then(|dir| repos.get(dir));
            let state = inspect(cx, &path, root.map(PathBuf::as_path), repo, deadline).state;
            Some(CleanupEntry {
                modified: path.is_dir().then(|| modified(&path)).flatten(),
                path,
                repo_root: root.cloned(),
                branch,
                state,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        let root = |entry: &CleanupEntry| entry.repo_root.as_deref().map(git::path_key);
        root(a)
            .cmp(&root(b))
            .then_with(|| git::path_key(&a.path).cmp(&git::path_key(&b.path)))
    });
    entries
}

/// Removes (`Action::Remove`) or prunes (`Action::Prune`) one worktree, after checking its state
/// again: nothing happens unless that state still allows exactly this action. The branch goes
/// only when it is merged into the default branch, and only if it still points to the commit
/// that was checked (`git update-ref -d <ref> <commit>`).
pub fn clean_up(
    cx: &Context<'_>,
    path: &Path,
    repo_root: &Path,
    action: Action,
) -> Result<CleanupOutcome, WorktreeError> {
    let branch = branch_of(path).ok_or(WorktreeError::OutsideFolder)?;
    let worktree = Worktree {
        path: path.to_path_buf(),
        branch,
        repo_root: repo_root.to_path_buf(),
    };
    worktree::check_shape(&worktree)?;
    worktree::check_managed(cx.base, path, repo_root)?;
    let deadline = Instant::now() + git::TIMEOUT;
    let repo = repo(cx.git, repo_root, deadline);
    let Inspection { state, merged_tip } =
        inspect(cx, path, Some(repo_root), Some(&repo), deadline);
    let refused = |state: CleanupState| CleanupOutcome {
        done: false,
        branch_deleted: false,
        state,
        reason: None,
    };
    if state.action() != Some(action) {
        return Ok(refused(state));
    }
    if let Some(reason) = worktree::remove_folder(cx.git, cx.base, &worktree)? {
        return Ok(CleanupOutcome {
            reason: Some(reason),
            ..refused(state)
        });
    }
    // Merged, or a missing folder whose branch is merged; an unmerged branch always stays.
    let branch_deleted = match &merged_tip {
        Some(tip) => delete_branch(cx, &worktree, tip)?,
        None => false,
    };
    Ok(CleanupOutcome {
        done: true,
        branch_deleted,
        state,
        reason: None,
    })
}

fn delete_branch(cx: &Context<'_>, worktree: &Worktree, tip: &str) -> Result<bool, WorktreeError> {
    let full = format!("refs/heads/{}", worktree.branch);
    let output = cx.git.run(
        &worktree.repo_root,
        ["update-ref", "-d", &full, tip],
        Instant::now() + git::TIMEOUT,
        64 * 1024,
    )?;
    Ok(output.success)
}

/// Size and newest file of a worktree folder: links and reparse points are neither followed
/// nor counted; cut after [`SIZE_MAX_FILES`] entries or [`SIZE_BUDGET`].
pub fn measure(path: &Path, max_files: u64, budget: Duration) -> WorktreeSize {
    let deadline = Instant::now() + budget;
    let mut size = WorktreeSize {
        bytes: 0,
        files: 0,
        complete: true,
        newest: None,
    };
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if size.files >= max_files || Instant::now() >= deadline {
                size.complete = false;
                return size;
            }
            // Does not follow links (like `symlink_metadata`).
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if crate::no_links::is_reparse_point(&meta) {
                continue;
            }
            size.files += 1;
            if let Some(time) = meta.modified().ok().and_then(millis) {
                size.newest = Some(size.newest.map_or(time, |newest| newest.max(time)));
            }
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                size.bytes = size.bytes.saturating_add(meta.len());
            }
        }
    }
    size
}

/// A worktree folder below `base` in Hangar's layout that exists and is no link.
pub fn check_measurable(base: &Path, path: &Path) -> Result<(), WorktreeError> {
    worktree::check_layout(base, path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !crate::no_links::is_reparse_point(&meta) => Ok(()),
        _ => Err(WorktreeError::OutsideFolder),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;

    fn facts() -> Facts {
        Facts {
            in_use: false,
            exists: true,
            problem: None,
            contents: Contents::Clean,
            merged: false,
        }
    }

    #[test]
    fn states_follow_the_facts() {
        assert_eq!(classify(&facts()), CleanupState::Clean);
        assert_eq!(
            classify(&Facts {
                merged: true,
                ..facts()
            }),
            CleanupState::Merged
        );
        assert_eq!(
            classify(&Facts {
                contents: Contents::Changes,
                merged: true,
                ..facts()
            }),
            CleanupState::Changes
        );
        assert_eq!(
            classify(&Facts {
                contents: Contents::Ignored,
                ..facts()
            }),
            CleanupState::Ignored
        );
        assert_eq!(
            classify(&Facts {
                exists: false,
                contents: Contents::Changes,
                ..facts()
            }),
            CleanupState::Missing
        );
        assert_eq!(
            classify(&Facts {
                contents: Contents::Unreadable(Problem::new(Cause::Locked)),
                ..facts()
            }),
            CleanupState::Unknown {
                cause: Cause::Locked,
                detail: None
            }
        );
        // In use wins over everything, also over a missing folder and a problem.
        assert_eq!(
            classify(&Facts {
                in_use: true,
                exists: false,
                problem: Some(Problem::new(Cause::RepoNotFound)),
                ..facts()
            }),
            CleanupState::InUse
        );
        assert_eq!(
            classify(&Facts {
                exists: false,
                problem: Some(Problem::new(Cause::RepoNotFound)),
                ..facts()
            }),
            CleanupState::Unknown {
                cause: Cause::RepoNotFound,
                detail: None
            }
        );
    }

    #[test]
    fn only_safe_states_have_an_action() {
        assert_eq!(CleanupState::Merged.action(), Some(Action::Remove));
        assert_eq!(CleanupState::Clean.action(), Some(Action::Remove));
        assert_eq!(CleanupState::Missing.action(), Some(Action::Prune));
        for state in [
            CleanupState::InUse,
            CleanupState::Changes,
            CleanupState::Ignored,
            Problem::new(Cause::Locked).state(),
        ] {
            assert_eq!(state.action(), None, "{state:?}");
        }
    }

    #[test]
    fn states_serialize_with_a_kind() {
        assert_eq!(
            serde_json::to_value(CleanupState::InUse).unwrap(),
            serde_json::json!({ "kind": "inUse" })
        );
        let causes = [
            (Cause::RepoNotFound, "repoNotFound"),
            (Cause::NotKnownToGit, "notKnownToGit"),
            (Cause::OtherBranch, "otherBranch"),
            (Cause::NoBranch, "noBranch"),
            (Cause::Locked, "locked"),
            (Cause::NotCheckedInTime, "notCheckedInTime"),
            (Cause::RemoteRepo, "remoteRepo"),
            (Cause::GitFailed, "gitFailed"),
        ];
        for (cause, name) in causes {
            assert_eq!(
                serde_json::to_value(Problem::new(cause).state()).unwrap(),
                serde_json::json!({ "kind": "unknown", "cause": name, "detail": null })
            );
        }
        let failed = Problem::of(&git::GitError::Failed("fatal: bad object".into()));
        assert_eq!(
            serde_json::to_value(failed.state()).unwrap(),
            serde_json::json!({ "kind": "unknown", "cause": "gitFailed", "detail": "fatal: bad object" })
        );
        // Hangar's own error texts never reach the window as a detail.
        for error in [
            git::GitError::NotFound,
            git::GitError::UnsafeFilters,
            git::GitError::Task("x".into()),
        ] {
            assert_eq!(Problem::of(&error), Problem::new(Cause::GitFailed));
        }
        assert_eq!(
            Problem::of(&git::GitError::Timeout),
            Problem::new(Cause::NotCheckedInTime)
        );
        assert_eq!(
            Problem::of(&git::GitError::RemoteGitDir),
            Problem::new(Cause::RemoteRepo)
        );
    }

    #[test]
    fn sessions_inside_the_folder_occupy_it() {
        let tmp = TempDir::new("cleanup-in-use");
        let path = tmp.path().join("wt");
        assert!(in_use(&path, std::slice::from_ref(&path)));
        assert!(in_use(&path, &[path.join("sub")]));
        assert!(!in_use(&path, &[tmp.path().join("wt-other")]));
        assert!(!in_use(&path, &[tmp.path().to_path_buf()]));
        assert!(!in_use(&path, &[]));
    }

    #[cfg(unix)]
    fn link_dir(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    /// A junction needs no privilege on Windows, unlike a directory symlink.
    #[cfg(windows)]
    fn link_dir(target: &Path, link: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// A session that works in the worktree through a link (junction, symlink) occupies it as
    /// much as one that names it directly.
    #[test]
    fn sessions_reaching_the_folder_through_a_link_occupy_it() {
        let tmp = TempDir::new("cleanup-in-use-link");
        let path = tmp.path().join("wt");
        fs::create_dir_all(path.join("sub")).unwrap();
        let link = tmp.path().join("linked");
        assert!(link_dir(&path, &link), "the link could not be made");
        assert!(in_use(&path, std::slice::from_ref(&link)));
        assert!(in_use(&path, &[link.join("sub")]));
        // The other way round: the worktree is listed under the link's name.
        assert!(in_use(&link, std::slice::from_ref(&path)));
        let other = tmp.path().join("other");
        fs::create_dir_all(&other).unwrap();
        assert!(!in_use(&path, &[other]));
        // A folder that no longer exists is compared by its text alone.
        assert!(!in_use(&tmp.path().join("gone"), &[link]));
    }

    #[test]
    fn fresh_worktrees_stay_occupied_until_a_session_takes_them() {
        let tmp = TempDir::new("cleanup-pending");
        let fresh = tmp.path().join("fresh");
        let other = tmp.path().join("other");
        let start = Instant::now();
        let pending = PendingWorktrees::default();
        pending.add(&fresh, start);
        pending.add(&other, start);
        let timeout = Duration::from_secs(60);
        let occupied = pending.with_pending(Vec::new(), start, timeout);
        assert!(in_use(&fresh, &occupied) && in_use(&other, &occupied));

        // The session started in it: from now on the session holds it, also once it is gone.
        let session = vec![fresh.join("sub")];
        let occupied = pending.with_pending(session.clone(), start, timeout);
        assert!(in_use(&fresh, &occupied));
        let occupied = pending.with_pending(Vec::new(), start, timeout);
        assert!(!in_use(&fresh, &occupied));
        assert!(in_use(&other, &occupied));

        // A start that never came: free again after the timeout.
        let later = start + timeout;
        assert!(!in_use(
            &other,
            &pending.with_pending(Vec::new(), later, timeout)
        ));

        // Removed with its session.
        pending.add(&other, start);
        pending.remove(&other);
        assert!(!in_use(
            &other,
            &pending.with_pending(Vec::new(), start, timeout)
        ));
    }

    #[test]
    fn worktree_list_records() {
        let output = b"worktree C:/repo\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
worktree C:/wt/a\0HEAD 2222222222222222222222222222222222222222\0branch refs/heads/hangar/a-1a2b3c\0locked\0\0\
worktree C:/wt/b\0HEAD 3333333333333333333333333333333333333333\0detached\0prunable gitdir file points to non-existent location\0\0";
        let list = worktree::parse_list(output);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(list[1].locked && !list[1].prunable);
        assert_eq!(list[1].path, git::native_path("C:/wt/a"));
        assert_eq!(list[2].branch, None);
        assert!(list[2].prunable);
        assert_eq!(
            list[2].head.as_deref(),
            Some("3333333333333333333333333333333333333333")
        );
        assert!(worktree::parse_list(b"").is_empty());
    }

    #[test]
    fn dot_git_files_name_the_repository() {
        let tmp = TempDir::new("cleanup-dot-git");
        let wt = tmp.path().join("wt");
        fs::create_dir_all(&wt).unwrap();
        let root = tmp.path().join("my repo");
        let gitdir = root.join(".git").join("worktrees").join("wt");
        let text = gitdir.to_string_lossy().replace('\\', "/");
        fs::write(wt.join(".git"), format!("gitdir: {text}\n")).unwrap();
        assert_eq!(root_from_dot_git(&wt), Some(root.clone()));
        // Relative (worktree.useRelativePaths).
        fs::write(wt.join(".git"), "gitdir: ../my repo/.git/worktrees/wt\n").unwrap();
        assert_eq!(root_from_dot_git(&wt), Some(root));
        // A submodule's git folder is no worktree record.
        fs::write(wt.join(".git"), "gitdir: ../repo/.git/modules/wt\n").unwrap();
        assert_eq!(root_from_dot_git(&wt), None);
        fs::remove_file(wt.join(".git")).unwrap();
        assert_eq!(root_from_dot_git(&wt), None);
    }

    #[test]
    fn sizes_are_bounded() {
        let tmp = TempDir::new("cleanup-size");
        let dir = tmp.path().join("wt");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "12345").unwrap();
        fs::write(dir.join("sub").join("b.txt"), "123").unwrap();
        let size = measure(&dir, SIZE_MAX_FILES, SIZE_BUDGET);
        assert_eq!(size.bytes, 8);
        assert_eq!(size.files, 3);
        assert!(size.complete);
        assert!(size.newest.is_some());
        let cut = measure(&dir, 1, SIZE_BUDGET);
        assert!(!cut.complete);
        assert_eq!(cut.files, 1);
    }

    #[test]
    fn only_folders_in_the_layout_are_measured() {
        let tmp = TempDir::new("cleanup-measurable");
        let base = tmp.path().join(worktree::DIR_NAME);
        let good = base.join("repo-0123abcd").join("fix-1a2b3c");
        fs::create_dir_all(&good).unwrap();
        check_measurable(&base, &good).unwrap();
        for bad in [
            base.clone(),
            base.join("repo-0123abcd"),
            base.join("repo-0123abcd").join("missing-1a2b3c"),
            tmp.path().to_path_buf(),
        ] {
            assert!(check_measurable(&base, &bad).is_err(), "{}", bad.display());
        }
    }

    /// All states from real repositories, created the way Hangar creates them, and removal that
    /// checks again and refuses everything unsafe.
    #[test]
    fn cleanup_with_real_repositories() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("cleanup-real");
        let base = tmp.path().join(worktree::DIR_NAME);
        let repo = tmp.path().join("my repo");
        init_repo(&git, &repo);
        fs::write(repo.join("readme.txt"), "hello\n").unwrap();
        fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);

        let merged_wt = worktree::create(&git, &base, &repo, "merged").unwrap();
        let clean = worktree::create(&git, &base, &repo, "clean").unwrap();
        fs::write(clean.path.join("new.txt"), "work\n").unwrap();
        git_ok(&git, &clean.path, &["add", "-A"]);
        git_ok(&git, &clean.path, &["commit", "--quiet", "-m", "work"]);
        let changed = worktree::create(&git, &base, &repo, "changed").unwrap();
        fs::write(changed.path.join("readme.txt"), "changed\n").unwrap();
        let untracked = worktree::create(&git, &base, &repo, "untracked").unwrap();
        fs::write(untracked.path.join("draft.txt"), "draft\n").unwrap();
        let ignored = worktree::create(&git, &base, &repo, "ignored").unwrap();
        fs::write(ignored.path.join("debug.log"), "local\n").unwrap();
        let used = worktree::create(&git, &base, &repo, "used").unwrap();
        let missing = worktree::create(&git, &base, &repo, "missing").unwrap();
        fs::remove_dir_all(&missing.path).unwrap();

        // Not Hangar's: a worktree of the user next to the repository, and a stray folder.
        let own = tmp.path().join("own-worktree");
        git_ok(
            &git,
            &repo,
            &["worktree", "add", "-q", "-b", "own", &own.to_string_lossy()],
        );
        fs::create_dir_all(base.join("Not A Slug").join("x")).unwrap();

        // A worktree whose repository was deleted afterwards.
        let gone_repo = tmp.path().join("gone");
        init_repo(&git, &gone_repo);
        git_ok(
            &git,
            &gone_repo,
            &["commit", "--quiet", "--allow-empty", "-m", "x"],
        );
        let orphan = worktree::create(&git, &base, &gone_repo, "orphan").unwrap();
        fs::remove_dir_all(&gone_repo).unwrap();

        let occupied = vec![used.path.join("sub")];
        let cx = Context {
            git: &git,
            base: &base,
            occupied: &occupied,
        };
        // The window names the repository; the missing folder is found through its records.
        let entries = list(&cx, std::slice::from_ref(&repo), &[]);
        let state_of = |path: &Path| {
            entries
                .iter()
                .find(|entry| git::path_key(&entry.path) == git::path_key(path))
                .map(|entry| entry.state.clone())
        };
        assert_eq!(state_of(&merged_wt.path), Some(CleanupState::Merged));
        assert_eq!(state_of(&clean.path), Some(CleanupState::Clean));
        assert_eq!(state_of(&changed.path), Some(CleanupState::Changes));
        assert_eq!(state_of(&untracked.path), Some(CleanupState::Changes));
        assert_eq!(state_of(&ignored.path), Some(CleanupState::Ignored));
        assert_eq!(state_of(&used.path), Some(CleanupState::InUse));
        assert_eq!(state_of(&missing.path), Some(CleanupState::Missing));
        assert_eq!(
            state_of(&orphan.path),
            Some(Problem::new(Cause::RepoNotFound).state())
        );
        assert_eq!(state_of(&own), None);
        assert_eq!(entries.len(), 8, "{entries:#?}");
        let entry = entries
            .iter()
            .find(|entry| entry.path == merged_wt.path)
            .unwrap();
        assert_eq!(entry.branch, merged_wt.branch);
        assert_eq!(entry.repo_root.as_deref(), Some(repo.as_path()));
        assert!(entry.modified.is_some());
        // Without it, the other worktree folders' `.git` files lead to the same repository.
        let without = list(&cx, &[], &[]);
        assert_eq!(without, entries);
        // A repository without any folder left is known only from the window or the workspace.
        let lone = tmp.path().join("lone");
        init_repo(&git, &lone);
        git_ok(
            &git,
            &lone,
            &["commit", "--quiet", "--allow-empty", "-m", "x"],
        );
        let lone_wt = worktree::create(&git, &base, &lone, "lone").unwrap();
        fs::remove_dir_all(&lone_wt.path).unwrap();
        let lone_state = |entries: &[CleanupEntry]| {
            entries
                .iter()
                .find(|entry| git::path_key(&entry.path) == git::path_key(&lone_wt.path))
                .map(|entry| entry.state.clone())
        };
        assert_eq!(lone_state(&list(&cx, &[], &[])), None);
        assert_eq!(
            lone_state(&list(&cx, &[], std::slice::from_ref(&lone))),
            Some(CleanupState::Missing)
        );
        let outcome = clean_up(&cx, &lone_wt.path, &lone, Action::Prune).unwrap();
        assert!(outcome.done && outcome.branch_deleted);

        let branch_exists = |branch: &str| {
            git.run(
                &repo,
                [
                    "rev-parse",
                    "--verify",
                    "-q",
                    &format!("refs/heads/{branch}"),
                ],
                Instant::now() + git::TIMEOUT,
                4096,
            )
            .unwrap()
            .success
        };

        // Refused: changes, ignored files, in use, the wrong action, a foreign folder.
        for (wt, action, state) in [
            (&changed, Action::Remove, CleanupState::Changes),
            (&untracked, Action::Remove, CleanupState::Changes),
            (&ignored, Action::Remove, CleanupState::Ignored),
            (&used, Action::Remove, CleanupState::InUse),
            (&merged_wt, Action::Prune, CleanupState::Merged),
            (&missing, Action::Remove, CleanupState::Missing),
        ] {
            let outcome = clean_up(&cx, &wt.path, &repo, action).unwrap();
            assert!(!outcome.done, "{}", wt.path.display());
            assert_eq!(outcome.state, state);
        }
        assert!(changed.path.join("readme.txt").is_file());
        assert!(ignored.path.join("debug.log").is_file());
        assert!(used.path.is_dir());
        assert!(matches!(
            clean_up(&cx, &own, &repo, Action::Remove),
            Err(WorktreeError::OutsideFolder)
        ));
        // Another repository's root for the same folder.
        assert!(matches!(
            clean_up(&cx, &merged_wt.path, &gone_repo, Action::Remove),
            Err(WorktreeError::OutsideFolder)
        ));

        // The list said "merged"; a change made since keeps the worktree.
        fs::write(merged_wt.path.join("late.txt"), "late\n").unwrap();
        let outcome = clean_up(&cx, &merged_wt.path, &repo, Action::Remove).unwrap();
        assert!(!outcome.done);
        assert_eq!(outcome.state, CleanupState::Changes);
        fs::remove_file(merged_wt.path.join("late.txt")).unwrap();

        // Merged: folder and branch go.
        let outcome = clean_up(&cx, &merged_wt.path, &repo, Action::Remove).unwrap();
        assert_eq!(
            outcome,
            CleanupOutcome {
                done: true,
                branch_deleted: true,
                state: CleanupState::Merged,
                reason: None
            }
        );
        assert!(!merged_wt.path.exists());
        assert!(!branch_exists(&merged_wt.branch));

        // Own commits: the folder goes, the branch stays.
        let outcome = clean_up(&cx, &clean.path, &repo, Action::Remove).unwrap();
        assert!(outcome.done && !outcome.branch_deleted);
        assert!(!clean.path.exists());
        assert!(branch_exists(&clean.branch));

        // Commits merged into the default branch later: merged, folder and branch go.
        let landed = worktree::create(&git, &base, &repo, "landed").unwrap();
        fs::write(
            landed.path.join("landed.txt"),
            "landed
",
        )
        .unwrap();
        git_ok(&git, &landed.path, &["add", "-A"]);
        git_ok(&git, &landed.path, &["commit", "--quiet", "-m", "landed"]);
        git_ok(
            &git,
            &repo,
            &["merge", "--quiet", "--ff-only", &landed.branch],
        );
        let outcome = clean_up(&cx, &landed.path, &repo, Action::Remove).unwrap();
        assert_eq!(outcome.state, CleanupState::Merged);
        assert!(outcome.done && outcome.branch_deleted);
        assert!(!branch_exists(&landed.branch));

        // Missing folder: only its record goes; the user's own worktree, deleted by hand too,
        // stays recorded.
        fs::remove_dir_all(&own).unwrap();
        let outcome = clean_up(&cx, &missing.path, &repo, Action::Prune).unwrap();
        assert!(outcome.done && outcome.branch_deleted, "{outcome:?}");
        let listed = worktree::list(&git, &repo, Instant::now() + git::TIMEOUT).unwrap();
        assert!(listed.iter().all(|entry| entry.path != missing.path));
        assert!(!branch_exists(&missing.branch));
        assert!(
            listed
                .iter()
                .any(|entry| git::path_key(&entry.path) == git::path_key(&own)),
            "the user's own missing worktree was pruned"
        );
    }
}
