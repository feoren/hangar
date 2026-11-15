//! Silent snapshots of a session's work tree after each agent step, the timeline of them and the
//! jump back to one (contract: docs/wiki/pages/syntheses/work-tree-timeline.md).
//!
//! A snapshot is what `git stash create` makes, built without touching anything of the user's:
//! the user's index is copied into a temporary index file in Hangar's data folder (so unchanged
//! files keep their cached stat data), `git add -A` brings that copy up to the work tree, and
//! `write-tree` plus `commit-tree` (parent: HEAD) turn it into a commit. Ignored files never go
//! in. The commit is kept under a private ref, `refs/hangar/snapshots/<session key>/<n>`; HEAD,
//! the branch, the index, the stash and the reflog stay as they were. Snapshots of one session are
//! capped and the oldest pruned, and all of them go when the session is closed.
//!
//! Every call runs through the hardened runner of git.rs with the repository's filter drivers and
//! hooks switched off (the empty hooks folder of git_actions.rs). A jump back first takes a
//! snapshot of the current state, so the jump itself can be undone, then makes the work tree match
//! the chosen snapshot: changed files get their content back, files created since are removed.
//! Paths through links are refused like `git_discard` refuses them.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};

use crate::git::{self, Git, GitDiff, GitError, Output};
use crate::git_actions::{self, ActionError};
use crate::session::SessionId;

/// Namespace of all snapshot refs. Neither a default fetch, a clone nor a push of branches or tags
/// carries refs outside `refs/heads/` and `refs/tags/` (measured in the tests below).
pub const REF_PREFIX: &str = "refs/hangar/snapshots/";
/// Snapshots kept per session; the oldest go first.
pub const MAX_SNAPSHOTS: usize = 50;
/// Changed files (against HEAD and the index, untracked ones counted one by one) above which a step
/// is not saved: a folder of generated files nobody ignored would bloat the repository.
pub const MAX_CHANGED_FILES: usize = 2000;
/// One snapshot or one jump back, all git calls together.
pub const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(30);
/// Characters of the agent's last prompt kept with a snapshot.
pub const MAX_PROMPT_CHARS: usize = 200;
/// Folder below Hangar's local data folder for the temporary index files.
pub const TEMP_FOLDER: &str = "git-snapshots";
/// First word of every snapshot's subject, so the commits explain themselves in `git log --all`.
const SUBJECT_TAG: &str = "hangar-snapshot";
/// `commit-tree` needs an identity; the snapshot is Hangar's, not the user's.
const IDENTITY: [&str; 4] = [
    "-c",
    "user.name=Hangar",
    "-c",
    "user.email=snapshots@hangar.invalid",
];
const MAX_ID_BYTES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error("Unknown session: {0}")]
    UnknownSession(SessionId),
    #[error("Invalid session key")]
    InvalidKey,
    #[error("This snapshot no longer exists")]
    UnknownSnapshot,
    #[error("The current state could not be saved first ({0}); nothing was changed")]
    CannotSaveCurrent(String),
    #[error("Hangar's snapshot folder cannot be used: {0}")]
    TempFolder(String),
    #[error("git printed something unexpected: {0}")]
    Unexpected(&'static str),
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl Serialize for SnapshotError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Why a snapshot was taken: after an agent step, or right before a jump back (its undo point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotKind {
    Step,
    Undo,
}

impl SnapshotKind {
    fn word(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::Undo => "undo",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        match word {
            "step" => Some(Self::Step),
            "undo" => Some(Self::Undo),
            _ => None,
        }
    }
}

/// One entry of the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Position in the session's timeline, counting up from 1; never reused.
    pub number: u64,
    /// Full id of the snapshot commit.
    pub sha: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Files that differ from the previous snapshot (the first one: from HEAD).
    pub files: u64,
    pub kind: SnapshotKind,
    /// The agent's last prompt before the step, cut; empty when none was known.
    pub prompt: String,
}

/// Why the newest step of a session was not saved; the window says so in the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub enum Skipped {
    TooManyFiles { limit: usize },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotList {
    pub is_repo: bool,
    /// Newest first.
    pub snapshots: Vec<Snapshot>,
    /// The newest step was not saved; `None` once a later one was (or nothing changed).
    pub skipped: Option<Skipped>,
}

/// What [`take`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TakeOutcome {
    Created {
        number: u64,
        files: u64,
    },
    /// The work tree matches the newest snapshot (or, without one, HEAD).
    Unchanged,
    NotARepository,
    TooManyFiles {
        limit: usize,
    },
}

/// What [`restore`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RestoreOutcome {
    /// `undo`: the snapshot that holds the state from before the jump.
    Restored { files: u64, undo: u64 },
    /// The work tree already matched the snapshot; nothing was written.
    AlreadyThere,
}

/// What the snapshot calls need besides git: the empty hooks folder, the folder for temporary
/// index files and the limits (tests use small ones).
#[derive(Debug, Clone)]
pub struct SnapshotEnv<'a> {
    pub git: &'a Git,
    pub no_hooks: &'a Path,
    pub temp_dir: &'a Path,
    pub max_snapshots: usize,
    pub max_files: usize,
}

/// A session key as the window keeps it (`workspace.ts`, a UUID), lower case: the only text that
/// becomes part of a ref name.
pub fn check_key(key: &str) -> Result<String, SnapshotError> {
    let bytes = key.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if valid {
        Ok(key.to_ascii_lowercase())
    } else {
        Err(SnapshotError::InvalidKey)
    }
}

/// The prompt as stored: one line without control characters, at most [`MAX_PROMPT_CHARS`].
pub fn clean_prompt(prompt: &str) -> String {
    let spaced: String = prompt
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let joined = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().take(MAX_PROMPT_CHARS).collect()
}

/// The snapshot commit's message: `hangar-snapshot <kind> <files>`, then the prompt, on one line.
fn subject(kind: SnapshotKind, files: u64, prompt: &str) -> String {
    let base = format!("{SUBJECT_TAG} {} {files}", kind.word());
    if prompt.is_empty() {
        base
    } else {
        format!("{base} {prompt}")
    }
}

/// Kind, file count and prompt from a subject written by [`subject`]; `None` for anything else.
pub fn parse_subject(text: &str) -> Option<(SnapshotKind, u64, String)> {
    let mut parts = text.splitn(4, ' ');
    if parts.next()? != SUBJECT_TAG {
        return None;
    }
    let kind = SnapshotKind::parse(parts.next()?)?;
    let files = parts.next()?.parse().ok()?;
    Some((kind, files, parts.next().unwrap_or_default().to_owned()))
}

/// A temporary index file in Hangar's data folder, removed (with its lock) on drop.
struct TempIndex(PathBuf);

impl TempIndex {
    fn new(dir: &Path) -> Result<Self, SnapshotError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        fs::create_dir_all(dir).map_err(|error| SnapshotError::TempFolder(error.to_string()))?;
        let name = format!(
            "{}-{}.index",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = dir.join(name);
        // A leftover of a crashed run with the same name would be read as a stale index.
        remove_quietly(&path);
        Ok(Self(path))
    }

    fn env(&self) -> [(&'static str, &OsStr); 1] {
        [("GIT_INDEX_FILE", self.0.as_os_str())]
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        remove_quietly(&self.0);
        let mut lock = self.0.clone().into_os_string();
        lock.push(".lock");
        remove_quietly(Path::new(&lock));
    }
}

fn remove_quietly(path: &Path) {
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != io::ErrorKind::NotFound {
            eprintln!("hangar: could not remove {}: {error}", path.display());
        }
    }
}

/// The repository of a session's folder and what every call of one operation shares.
struct Repo {
    root: PathBuf,
    deadline: Instant,
    /// `-c` settings before the subcommand: no hooks, no filters, no split index, no reflog.
    overrides: Vec<String>,
}

impl Repo {
    fn open(env: &SnapshotEnv<'_>, cwd: &Path) -> Result<Option<Self>, SnapshotError> {
        git::check_folder(cwd)?;
        let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
        let Some(root) = git::toplevel(env.git, cwd, deadline)? else {
            return Ok(None);
        };
        let mut overrides: Vec<String> = [
            format!("core.hooksPath={}", env.no_hooks.display()),
            // A split index would write shared index files into the repository's git folder.
            "core.splitIndex=false".to_owned(),
            // `always` would give every snapshot ref a reflog of its own.
            "core.logAllRefUpdates=false".to_owned(),
        ]
        .into_iter()
        .flat_map(|setting| ["-c".to_owned(), setting])
        .collect();
        overrides.extend(git::filter_overrides(env.git, &root, deadline)?);
        Ok(Some(Self {
            root,
            deadline,
            overrides,
        }))
    }

    /// Runs `git <overrides> <args>`; a failure is git's message.
    fn run(
        &self,
        env: &SnapshotEnv<'_>,
        args: &[&str],
        extra: &[(&str, &OsStr)],
        input: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<Output, SnapshotError> {
        let all = self
            .overrides
            .iter()
            .map(String::as_str)
            .chain(args.iter().copied());
        let output = env
            .git
            .run_with_env(&self.root, all, extra, input, self.deadline, limit)?;
        if !output.success {
            return Err(output.failure().into());
        }
        Ok(output)
    }

    /// One object id from a call that prints one (`write-tree`, `commit-tree`, `rev-parse`).
    fn id(
        &self,
        env: &SnapshotEnv<'_>,
        args: &[&str],
        extra: &[(&str, &OsStr)],
        input: Option<Vec<u8>>,
    ) -> Result<String, SnapshotError> {
        let output = self.run(env, args, extra, input, MAX_ID_BYTES)?;
        let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if is_object_id(&id) {
            Ok(id)
        } else {
            Err(SnapshotError::Unexpected("no object id"))
        }
    }

    /// The object `revision` names, or `None` when it does not exist.
    fn resolve(
        &self,
        env: &SnapshotEnv<'_>,
        revision: &str,
    ) -> Result<Option<String>, SnapshotError> {
        let output = env.git.run(
            &self.root,
            ["rev-parse", "--verify", "--quiet", revision],
            self.deadline,
            MAX_ID_BYTES,
        )?;
        let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok((output.success && is_object_id(&id)).then_some(id))
    }
}

fn is_object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn ref_folder(key: &str) -> String {
    format!("{REF_PREFIX}{key}/")
}

/// A snapshot ref as listed, oldest first.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    snapshot: Snapshot,
}

/// The session's snapshots, oldest first. Refs under the folder that Hangar did not write
/// (another name, another subject) are left out and never touched.
fn entries(env: &SnapshotEnv<'_>, repo: &Repo, key: &str) -> Result<Vec<Entry>, SnapshotError> {
    let folder = ref_folder(key);
    let output = repo.run(
        env,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(creatordate:unix)%00%(subject)",
            &folder,
        ],
        &[],
        None,
        git::MAX_LIST_BYTES,
    )?;
    let mut list: Vec<Entry> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '\0');
            let name = fields.next()?;
            let sha = fields.next()?;
            let created_at = fields.next()?.trim().parse().ok()?;
            let (kind, files, prompt) = parse_subject(fields.next()?)?;
            let number: u64 = name.strip_prefix(&folder)?.parse().ok()?;
            if number == 0 || !is_object_id(sha) {
                return None;
            }
            Some(Entry {
                name: name.to_owned(),
                snapshot: Snapshot {
                    number,
                    sha: sha.to_owned(),
                    created_at,
                    files,
                    kind,
                    prompt,
                },
            })
        })
        .collect();
    list.sort_by_key(|entry| entry.snapshot.number);
    Ok(list)
}

/// Writes the work tree (without ignored files) as a tree object, through a copy of the user's
/// index. Returns the tree's id.
fn work_tree_tree(env: &SnapshotEnv<'_>, repo: &Repo) -> Result<String, SnapshotError> {
    let index = TempIndex::new(env.temp_dir)?;
    let real = repo.run(
        env,
        &["rev-parse", "--git-path", "index"],
        &[],
        None,
        MAX_ID_BYTES,
    )?;
    // Relative to the work tree root, where git ran; an absolute path stays as it is.
    let real = repo
        .root
        .join(git::native_path(&String::from_utf8_lossy(&real.stdout)));
    match fs::copy(&real, &index.0) {
        Ok(_) => {}
        // A repository whose index was never written starts from an empty one.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(SnapshotError::TempFolder(error.to_string())),
    }
    repo.run(
        env,
        &["add", "--all"],
        &index.env(),
        None,
        git::MAX_LIST_BYTES,
    )?;
    repo.id(env, &["write-tree"], &index.env(), None)
}

/// Files that differ between two trees (or, without `from`, all files of `to`).
fn changed_files(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    from: Option<&str>,
    to: &str,
) -> Result<Vec<(u8, String)>, SnapshotError> {
    let output = match from {
        Some(from) => repo.run(
            env,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--no-renames",
                "--name-status",
                "--ignore-submodules=all",
                from,
                to,
                "--",
            ],
            &[],
            None,
            git::MAX_LIST_BYTES,
        )?,
        None => {
            let output = repo.run(
                env,
                &["ls-tree", "-r", "-z", "--name-only", to],
                &[],
                None,
                git::MAX_LIST_BYTES,
            )?;
            let names = output
                .stdout
                .split(|b| *b == 0)
                .filter(|name| !name.is_empty())
                .map(|name| (b'A', String::from_utf8_lossy(name).into_owned()))
                .collect();
            return Ok(names);
        }
    };
    if output.truncated {
        return Err(SnapshotError::Unexpected("file list too long"));
    }
    let mut files = Vec::new();
    let mut records = output.stdout.split(|b| *b == 0);
    while let Some(code) = records.next() {
        let Some(&letter) = code.first() else {
            continue;
        };
        let Some(path) = records.next() else {
            break;
        };
        files.push((letter, String::from_utf8_lossy(path).into_owned()));
    }
    Ok(files)
}

/// The state the work tree is in after [`take_in`].
enum Taken {
    Created {
        number: u64,
        files: u64,
    },
    /// Matches the newest snapshot (`number`), or HEAD when there is none (`None`).
    Unchanged {
        number: Option<u64>,
    },
    TooManyFiles,
}

/// The work tree as a tree object, or `None` when more than `max_files` files are changed against
/// HEAD and the index (untracked ones counted one by one).
fn current_tree(env: &SnapshotEnv<'_>, repo: &Repo) -> Result<Option<String>, SnapshotError> {
    let status = env.git.run(
        &repo.root,
        repo.overrides.iter().map(String::as_str).chain([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ]),
        repo.deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !status.usable() {
        return Err(status.failure().into());
    }
    if status.truncated || git::parse_status(&status.stdout).len() > env.max_files {
        return Ok(None);
    }
    work_tree_tree(env, repo).map(Some)
}

fn take_in(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    key: &str,
    prompt: &str,
    kind: SnapshotKind,
) -> Result<Taken, SnapshotError> {
    match current_tree(env, repo)? {
        Some(tree) => record(env, repo, key, prompt, kind, &tree),
        None => Ok(Taken::TooManyFiles),
    }
}

/// Keeps `tree` as the session's next snapshot, unless it matches the newest one (or, for the
/// first step, HEAD), and prunes the oldest beyond the cap in the same ref transaction.
fn record(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    key: &str,
    prompt: &str,
    kind: SnapshotKind,
    tree: &str,
) -> Result<Taken, SnapshotError> {
    let list = entries(env, repo, key)?;
    let head = repo.resolve(env, "HEAD^{commit}")?;
    let last = list.last().map(|entry| &entry.snapshot);
    let base_tree = match (last, &head) {
        (Some(last), _) => repo.resolve(env, &format!("{}^{{tree}}", last.sha))?,
        (None, Some(head)) => repo.resolve(env, &format!("{head}^{{tree}}"))?,
        (None, None) => None,
    };
    let same = base_tree.as_deref() == Some(tree);
    // A jump back always needs its undo point; one equal to HEAD is still worth keeping there.
    if same && (last.is_some() || kind == SnapshotKind::Step) {
        return Ok(Taken::Unchanged {
            number: last.map(|last| last.number),
        });
    }

    let files = changed_files(env, repo, base_tree.as_deref(), tree)?;
    let files = u64::try_from(files.len()).unwrap_or(u64::MAX);
    let mut message = subject(kind, files, &clean_prompt(prompt)).into_bytes();
    message.push(b'\n');
    let mut args: Vec<&str> = IDENTITY.to_vec();
    args.extend(["commit-tree", "--no-gpg-sign", tree]);
    if let Some(head) = &head {
        args.extend(["-p", head.as_str()]);
    }
    args.extend(["-F", "-"]);
    let commit = repo.id(env, &args, &[], Some(message))?;

    let number = last.map_or(1, |last| last.number + 1);
    let mut transaction = format!("create {}{number} {commit}\n", ref_folder(key));
    let excess = (list.len() + 1).saturating_sub(env.max_snapshots.max(1));
    for old in list.iter().take(excess) {
        transaction.push_str(&format!("delete {}\n", old.name));
    }
    repo.run(
        env,
        &["update-ref", "--stdin"],
        &[],
        Some(transaction.into_bytes()),
        MAX_ID_BYTES,
    )?;
    Ok(Taken::Created { number, files })
}

/// Takes a snapshot of the work tree that holds `cwd`, for the session `key`, unless nothing
/// changed since the newest one (or, for the first, since HEAD).
pub fn take(
    env: &SnapshotEnv<'_>,
    cwd: &Path,
    key: &str,
    prompt: &str,
) -> Result<TakeOutcome, SnapshotError> {
    let key = check_key(key)?;
    let Some(repo) = Repo::open(env, cwd)? else {
        return Ok(TakeOutcome::NotARepository);
    };
    Ok(
        match take_in(env, &repo, &key, prompt, SnapshotKind::Step)? {
            Taken::Created { number, files, .. } => TakeOutcome::Created { number, files },
            Taken::Unchanged { .. } => TakeOutcome::Unchanged,
            Taken::TooManyFiles => TakeOutcome::TooManyFiles {
                limit: env.max_files,
            },
        },
    )
}

/// The session's snapshots, newest first; `skipped` is the service's to fill.
pub fn list(env: &SnapshotEnv<'_>, cwd: &Path, key: &str) -> Result<SnapshotList, SnapshotError> {
    let key = check_key(key)?;
    let Some(repo) = Repo::open(env, cwd)? else {
        return Ok(SnapshotList {
            is_repo: false,
            snapshots: Vec::new(),
            skipped: None,
        });
    };
    let mut snapshots: Vec<Snapshot> = entries(env, &repo, &key)?
        .into_iter()
        .map(|entry| entry.snapshot)
        .collect();
    snapshots.reverse();
    Ok(SnapshotList {
        is_repo: true,
        snapshots,
        skipped: None,
    })
}

/// What the step of snapshot `number` changed: against the snapshot before it, the first one
/// against the commit that was HEAD when it was taken.
pub fn diff(
    env: &SnapshotEnv<'_>,
    cwd: &Path,
    key: &str,
    number: u64,
) -> Result<GitDiff, SnapshotError> {
    let key = check_key(key)?;
    let repo = Repo::open(env, cwd)?.ok_or(GitError::NotARepository)?;
    let list = entries(env, &repo, &key)?;
    let index = list
        .iter()
        .position(|entry| entry.snapshot.number == number)
        .ok_or(SnapshotError::UnknownSnapshot)?;
    let sha = &list[index].snapshot.sha;
    let from = match index.checked_sub(1).and_then(|before| list.get(before)) {
        Some(before) => Some(before.snapshot.sha.clone()),
        None => repo.resolve(env, &format!("{sha}^1"))?,
    };
    Ok(crate::git_history::diff_trees(
        env.git,
        &repo.root,
        from.as_deref(),
        sha,
        repo.deadline,
    )?)
}

/// Makes the work tree match snapshot `number`: files changed since get its content back, files
/// created since are removed, ignored files, the index, HEAD and the branch stay. The current
/// state is saved as a snapshot first (`undo` in the answer), so the jump can be undone. Refused
/// before anything is written when a path leads through a link or the current state cannot be
/// saved.
pub fn restore(
    env: &SnapshotEnv<'_>,
    cwd: &Path,
    key: &str,
    number: u64,
) -> Result<RestoreOutcome, SnapshotError> {
    let key = check_key(key)?;
    let repo = Repo::open(env, cwd)?.ok_or(GitError::NotARepository)?;
    let target = entries(env, &repo, &key)?
        .into_iter()
        .find(|entry| entry.snapshot.number == number)
        .ok_or(SnapshotError::UnknownSnapshot)?;
    let target_tree = repo
        .resolve(env, &format!("{}^{{tree}}", target.snapshot.sha))?
        .ok_or(SnapshotError::UnknownSnapshot)?;

    let cannot_save = |error: SnapshotError| SnapshotError::CannotSaveCurrent(error.to_string());
    let current_tree = match current_tree(env, &repo) {
        Ok(Some(tree)) => tree,
        Ok(None) => {
            return Err(SnapshotError::CannotSaveCurrent(format!(
                "more than {} changed files",
                env.max_files
            )))
        }
        Err(error) => return Err(cannot_save(error)),
    };
    if current_tree == target_tree {
        return Ok(RestoreOutcome::AlreadyThere);
    }
    let undo = match record(env, &repo, &key, "", SnapshotKind::Undo, &current_tree) {
        Ok(Taken::Created { number, .. })
        | Ok(Taken::Unchanged {
            number: Some(number),
            ..
        }) => number,
        Ok(_) => return Err(SnapshotError::Unexpected("no snapshot to return to")),
        Err(error) => return Err(cannot_save(error)),
    };

    let changes = changed_files(env, &repo, Some(&current_tree), &target_tree)?;
    for (_, path) in &changes {
        git_actions::check_path(path)?;
        git_actions::refuse_links(&repo.root, path)?;
    }
    let (deleted, written): (Vec<_>, Vec<_>) =
        changes.iter().partition(|(letter, _)| *letter == b'D');
    for (_, path) in &deleted {
        remove_file(&repo.root, path)?;
    }
    if !written.is_empty() {
        let index = TempIndex::new(env.temp_dir)?;
        repo.run(
            env,
            &["read-tree", target_tree.as_str()],
            &index.env(),
            None,
            MAX_ID_BYTES,
        )?;
        let mut input = Vec::new();
        for (_, path) in &written {
            input.extend_from_slice(path.as_bytes());
            input.push(0);
        }
        repo.run(
            env,
            &["checkout-index", "--force", "-z", "--stdin"],
            &index.env(),
            Some(input),
            git::MAX_LIST_BYTES,
        )?;
    }
    Ok(RestoreOutcome::Restored {
        files: u64::try_from(changes.len()).unwrap_or(u64::MAX),
        undo,
    })
}

/// Removes a file the snapshot does not have, then its folders up to the root as long as they are
/// empty (a folder that holds an ignored file stays).
fn remove_file(root: &Path, path: &str) -> Result<(), SnapshotError> {
    let target = root.join(path);
    match fs::remove_file(&target) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(GitError::Failed(format!("{path} could not be removed: {error}")).into())
        }
    }
    let mut folder = target.parent();
    while let Some(dir) = folder {
        if dir == root || !dir.starts_with(root) || fs::remove_dir(dir).is_err() {
            break;
        }
        folder = dir.parent();
    }
    Ok(())
}

/// Deletes every snapshot of the session (it was closed); returns how many there were.
pub fn forget(env: &SnapshotEnv<'_>, cwd: &Path, key: &str) -> Result<usize, SnapshotError> {
    let key = check_key(key)?;
    let Some(repo) = Repo::open(env, cwd)? else {
        return Ok(0);
    };
    let list = entries(env, &repo, &key)?;
    if list.is_empty() {
        return Ok(0);
    }
    let transaction: String = list
        .iter()
        .map(|entry| format!("delete {}\n", entry.name))
        .collect();
    repo.run(
        env,
        &["update-ref", "--stdin"],
        &[],
        Some(transaction.into_bytes()),
        MAX_ID_BYTES,
    )?;
    Ok(list.len())
}

/// The snapshot calls, one at a time per session (two of the same session would race for the
/// next number; other sessions use their own refs and temporary index, so a slow snapshot of one
/// session never holds up another, nor the closing of another), and why each session's newest
/// step was not saved.
#[derive(Debug, Default)]
pub struct Snapshots {
    busy: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    skipped: Mutex<HashMap<String, Skipped>>,
}

impl Snapshots {
    /// The lock of one session (`key` already checked). Entries stay: one per session of a run.
    fn session_lock(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(locks.entry(key.to_owned()).or_default())
    }

    fn skipped(&self) -> MutexGuard<'_, HashMap<String, Skipped>> {
        self.skipped.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// [`take`], remembering a skipped or failed step for the timeline.
    pub fn take(
        &self,
        env: &SnapshotEnv<'_>,
        cwd: &Path,
        key: &str,
        prompt: &str,
    ) -> Result<TakeOutcome, SnapshotError> {
        let key = check_key(key)?;
        let result = {
            let lock = self.session_lock(&key);
            let _busy = lock.lock().unwrap_or_else(PoisonError::into_inner);
            take(env, cwd, &key, prompt)
        };
        let mut skipped = self.skipped();
        match &result {
            Ok(TakeOutcome::TooManyFiles { limit }) => {
                skipped.insert(key, Skipped::TooManyFiles { limit: *limit });
            }
            Ok(_) => {
                skipped.remove(&key);
            }
            Err(error) => {
                skipped.insert(
                    key,
                    Skipped::Failed {
                        message: error.to_string(),
                    },
                );
            }
        }
        result
    }

    pub fn list(
        &self,
        env: &SnapshotEnv<'_>,
        cwd: &Path,
        key: &str,
    ) -> Result<SnapshotList, SnapshotError> {
        let mut answer = list(env, cwd, key)?;
        if answer.is_repo {
            answer.skipped = self.skipped().get(&check_key(key)?).cloned();
        }
        Ok(answer)
    }

    pub fn restore(
        &self,
        env: &SnapshotEnv<'_>,
        cwd: &Path,
        key: &str,
        number: u64,
    ) -> Result<RestoreOutcome, SnapshotError> {
        let key = check_key(key)?;
        let lock = self.session_lock(&key);
        let _busy = lock.lock().unwrap_or_else(PoisonError::into_inner);
        restore(env, cwd, &key, number)
    }

    pub fn forget(
        &self,
        env: &SnapshotEnv<'_>,
        cwd: &Path,
        key: &str,
    ) -> Result<usize, SnapshotError> {
        let key = check_key(key)?;
        self.skipped().remove(&key);
        let lock = self.session_lock(&key);
        let _busy = lock.lock().unwrap_or_else(PoisonError::into_inner);
        forget(env, cwd, &key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;

    const KEY: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    struct Fixture {
        tmp: TempDir,
        git: Git,
        repo: PathBuf,
        no_hooks: PathBuf,
        temp: PathBuf,
    }

    impl Fixture {
        /// A repository with one commit of `a.txt` and `b.txt`; `*.log` is ignored.
        fn new(name: &str) -> Option<Self> {
            let git = git()?;
            let tmp = TempDir::new(name);
            let repo = tmp.path().join("repo with space");
            init_repo(&git, &repo);
            for (key, value) in [
                ("user.name", "Hangar Test"),
                ("user.email", "test@example.invalid"),
                ("commit.gpgsign", "false"),
                ("core.autocrlf", "false"),
            ] {
                git_ok(&git, &repo, &["config", key, value]);
            }
            write(&repo.join("a.txt"), "one\n");
            write(&repo.join("b.txt"), "two\n");
            write(&repo.join(".gitignore"), "*.log\n");
            git_ok(&git, &repo, &["add", "-A"]);
            git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);
            let no_hooks = git_actions::no_hooks_folder(tmp.path()).unwrap();
            let temp = tmp.path().join(TEMP_FOLDER);
            Some(Self {
                tmp,
                git,
                repo,
                no_hooks,
                temp,
            })
        }

        fn env(&self) -> SnapshotEnv<'_> {
            SnapshotEnv {
                git: &self.git,
                no_hooks: &self.no_hooks,
                temp_dir: &self.temp,
                max_snapshots: MAX_SNAPSHOTS,
                max_files: MAX_CHANGED_FILES,
            }
        }

        fn out(&self, args: &[&str]) -> String {
            let output = self
                .git
                .run(&self.repo, args, Instant::now() + git::TIMEOUT, 1 << 20)
                .unwrap();
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        fn take(&self, prompt: &str) -> TakeOutcome {
            take(&self.env(), &self.repo, KEY, prompt).unwrap()
        }

        fn numbers(&self) -> Vec<u64> {
            list(&self.env(), &self.repo, KEY)
                .unwrap()
                .snapshots
                .iter()
                .map(|s| s.number)
                .collect()
        }
    }

    #[test]
    fn keys_prompts_and_subjects() {
        assert_eq!(check_key(KEY).unwrap(), KEY);
        assert_eq!(check_key(&KEY.to_uppercase()).unwrap(), KEY);
        for bad in [
            "",
            "x",
            "0f8fad5b-d9cb-469f-a165-70867728950",
            "0f8fad5b-d9cb-469f-a165-70867728950e/",
            "../../heads/main-aaaa-bbbb-cccc-dddddddd",
            "0f8fad5bxd9cb-469f-a165-70867728950e",
            "0f8fad5b-d9cb-469f-a165-70867728950g",
        ] {
            assert!(
                matches!(check_key(bad), Err(SnapshotError::InvalidKey)),
                "{bad}"
            );
        }
        assert_eq!(clean_prompt("  fix\nthe\t\x1b[31mbug  "), "fix the [31mbug");
        assert_eq!(
            clean_prompt(&"ä".repeat(500)).chars().count(),
            MAX_PROMPT_CHARS
        );
        assert_eq!(
            parse_subject(&subject(SnapshotKind::Step, 3, "fix the bug")),
            Some((SnapshotKind::Step, 3, "fix the bug".to_owned()))
        );
        assert_eq!(
            parse_subject(&subject(SnapshotKind::Undo, 0, "")),
            Some((SnapshotKind::Undo, 0, String::new()))
        );
        for bad in [
            "",
            "hangar-snapshot",
            "hangar-snapshot jump 1",
            "other step 1",
            "hangar-snapshot step x",
        ] {
            assert_eq!(parse_subject(bad), None, "{bad}");
        }
    }

    /// The user's index (staged and unstaged parts), HEAD, the branch, the stash and the reflogs
    /// look exactly as before; the snapshot holds the work tree, untracked files included and
    /// ignored ones left out.
    #[test]
    fn a_snapshot_touches_nothing_of_the_users() {
        let Some(fx) = Fixture::new("snapshot-untouched") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "one\nstaged\n");
        git_ok(&fx.git, &fx.repo, &["add", "a.txt"]);
        write(&fx.repo.join("a.txt"), "one\nstaged\nunstaged\n");
        write(&fx.repo.join("new folder/c.txt"), "three\n");
        write(&fx.repo.join("build.log"), "ignored\n");
        let before = (
            fx.out(&["status", "--porcelain=v1"]),
            fx.out(&["diff", "--cached"]),
            fx.out(&["rev-parse", "HEAD"]),
            fx.out(&["symbolic-ref", "HEAD"]),
            fx.out(&["reflog", "--all"]),
        );

        let outcome = fx.take("make the change");
        assert_eq!(
            outcome,
            TakeOutcome::Created {
                number: 1,
                files: 2
            }
        );

        let after = (
            fx.out(&["status", "--porcelain=v1"]),
            fx.out(&["diff", "--cached"]),
            fx.out(&["rev-parse", "HEAD"]),
            fx.out(&["symbolic-ref", "HEAD"]),
            fx.out(&["reflog", "--all"]),
        );
        assert_eq!(before, after);
        assert_eq!(fx.out(&["stash", "list"]), "");
        assert!(!fx.repo.join(".git/logs/refs/hangar").exists());
        let reference = format!("{REF_PREFIX}{KEY}/1");
        let files = fx.out(&["ls-tree", "-r", "--name-only", &reference]);
        let files: Vec<&str> = files.lines().collect();
        assert_eq!(files, [".gitignore", "a.txt", "b.txt", "new folder/c.txt"]);
        assert_eq!(
            fx.out(&["show", &format!("{reference}:a.txt")]),
            "one\nstaged\nunstaged\n"
        );
        // The parent is HEAD at that time; the temporary index is gone.
        assert_eq!(
            fx.out(&["rev-parse", &format!("{reference}^")]),
            fx.out(&["rev-parse", "HEAD"])
        );
        assert_eq!(fs::read_dir(&fx.temp).unwrap().count(), 0);

        let listed = list(&fx.env(), &fx.repo, KEY).unwrap();
        assert!(listed.is_repo);
        assert_eq!(listed.snapshots.len(), 1);
        let snapshot = &listed.snapshots[0];
        assert_eq!(
            (
                snapshot.number,
                snapshot.files,
                snapshot.kind,
                snapshot.prompt.as_str()
            ),
            (1, 2, SnapshotKind::Step, "make the change")
        );
        assert!(snapshot.created_at > 1_600_000_000);
    }

    #[test]
    fn an_unchanged_tree_takes_no_snapshot() {
        let Some(fx) = Fixture::new("snapshot-unchanged") else {
            return;
        };
        // Clean work tree: equal to HEAD.
        assert_eq!(fx.take(""), TakeOutcome::Unchanged);
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        assert_eq!(fx.take(""), TakeOutcome::Unchanged);
        // An ignored file alone is no change.
        write(&fx.repo.join("more.log"), "x\n");
        assert_eq!(fx.take(""), TakeOutcome::Unchanged);
        write(&fx.repo.join("b.txt"), "changed\n");
        assert_eq!(
            fx.take(""),
            TakeOutcome::Created {
                number: 2,
                files: 1
            }
        );
        assert_eq!(fx.numbers(), [2, 1]);
    }

    #[test]
    fn old_snapshots_are_pruned_and_forget_removes_all() {
        let Some(fx) = Fixture::new("snapshot-prune") else {
            return;
        };
        let env = SnapshotEnv {
            max_snapshots: 3,
            ..fx.env()
        };
        for step in 1..=5 {
            write(&fx.repo.join("a.txt"), &format!("step {step}\n"));
            let outcome = take(&env, &fx.repo, KEY, "").unwrap();
            assert!(matches!(outcome, TakeOutcome::Created { number, .. } if number == step));
        }
        assert_eq!(fx.numbers(), [5, 4, 3]);
        // Another session's snapshots stay apart.
        let other = "11111111-2222-3333-4444-555555555555";
        write(&fx.repo.join("a.txt"), "other\n");
        take(&env, &fx.repo, other, "").unwrap();
        assert_eq!(forget(&env, &fx.repo, KEY).unwrap(), 3);
        assert!(fx.numbers().is_empty());
        assert_eq!(list(&env, &fx.repo, other).unwrap().snapshots.len(), 1);
        assert_eq!(forget(&env, &fx.repo, KEY).unwrap(), 0);
    }

    #[test]
    fn too_many_changed_files_are_not_saved() {
        let Some(fx) = Fixture::new("snapshot-many") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "saved\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        let env = SnapshotEnv {
            max_files: 2,
            ..fx.env()
        };
        for name in ["x/1.txt", "x/2.txt", "x/3.txt"] {
            write(&fx.repo.join(name), "generated\n");
        }
        assert_eq!(
            take(&env, &fx.repo, KEY, "").unwrap(),
            TakeOutcome::TooManyFiles { limit: 2 }
        );
        assert_eq!(fx.numbers(), [1]);
        // A jump back is refused before anything is written: its undo point cannot be saved.
        assert!(matches!(
            restore(&env, &fx.repo, KEY, 1),
            Err(SnapshotError::CannotSaveCurrent(_))
        ));
        assert!(fx.repo.join("x/3.txt").exists());
        assert_eq!(fx.numbers(), [1]);
    }

    #[test]
    fn restore_and_undo_the_jump() {
        let Some(fx) = Fixture::new("snapshot-restore") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "first step\n");
        assert!(matches!(
            fx.take("one"),
            TakeOutcome::Created { number: 1, .. }
        ));
        // Second step: change, delete, create a file in a new folder; an ignored file appears.
        write(&fx.repo.join("a.txt"), "second step\n");
        fs::remove_file(fx.repo.join("b.txt")).unwrap();
        write(&fx.repo.join("deep/new/n.txt"), "new\n");
        write(&fx.repo.join("deep/keep.log"), "ignored\n");
        assert!(matches!(
            fx.take("two"),
            TakeOutcome::Created { number: 2, .. }
        ));
        // Third state, not saved yet; something staged.
        write(&fx.repo.join("a.txt"), "third\n");
        git_ok(&fx.git, &fx.repo, &["add", "a.txt"]);
        let staged = fx.out(&["diff", "--cached"]);
        let head = fx.out(&["rev-parse", "HEAD"]);

        // The first step's diff, against HEAD; the second's against the first.
        let first = diff(&fx.env(), &fx.repo, KEY, 1).unwrap();
        assert_eq!(first.files.len(), 1);
        assert!(first.diff.contains("+first step"));
        let second = diff(&fx.env(), &fx.repo, KEY, 2).unwrap();
        let paths: Vec<&str> = second.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "b.txt", "deep/new/n.txt"]);

        let outcome = restore(&fx.env(), &fx.repo, KEY, 1).unwrap();
        assert_eq!(outcome, RestoreOutcome::Restored { files: 3, undo: 3 });
        assert_eq!(read(&fx.repo.join("a.txt")), "first step\n");
        assert_eq!(read(&fx.repo.join("b.txt")), "two\n");
        assert!(!fx.repo.join("deep/new").exists());
        assert_eq!(read(&fx.repo.join("deep/keep.log")), "ignored\n");
        // Index, HEAD and branch stay.
        assert_eq!(fx.out(&["diff", "--cached"]), staged);
        assert_eq!(fx.out(&["rev-parse", "HEAD"]), head);
        let listed = list(&fx.env(), &fx.repo, KEY).unwrap();
        assert_eq!(listed.snapshots[0].kind, SnapshotKind::Undo);
        assert_eq!(listed.snapshots[0].number, 3);

        // Undo the jump: the third state is back, the new file too.
        let back = restore(&fx.env(), &fx.repo, KEY, 3).unwrap();
        assert!(
            matches!(back, RestoreOutcome::Restored { undo: 4, .. }),
            "{back:?}"
        );
        assert_eq!(read(&fx.repo.join("a.txt")), "third\n");
        assert!(!fx.repo.join("b.txt").exists());
        assert_eq!(read(&fx.repo.join("deep/new/n.txt")), "new\n");
        assert_eq!(
            restore(&fx.env(), &fx.repo, KEY, 3).unwrap(),
            RestoreOutcome::AlreadyThere
        );
        assert!(matches!(
            restore(&fx.env(), &fx.repo, KEY, 99),
            Err(SnapshotError::UnknownSnapshot)
        ));
    }

    /// A folder the snapshot wrote to was replaced by a link (junction on Windows) to a folder
    /// outside the work tree: the jump is refused and nothing outside is touched.
    #[test]
    fn restore_refuses_paths_through_links() {
        let Some(fx) = Fixture::new("snapshot-links") else {
            return;
        };
        write(&fx.repo.join("sub/c.txt"), "inside\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        let outside = fx.tmp.path().join("outside");
        write(&outside.join("c.txt"), "must survive\n");
        fs::remove_dir_all(fx.repo.join("sub")).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(fx.repo.join("sub"))
                .arg(&outside)
                .output()
                .unwrap()
                .status;
            assert!(status.success());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, fx.repo.join("sub")).unwrap();
        // git sees `sub` as a new link instead of the folder; restoring would write through it.
        write(&fx.repo.join("a.txt"), "changed\n");
        let result = restore(&fx.env(), &fx.repo, KEY, 1);
        assert!(
            matches!(result, Err(SnapshotError::Action(ActionError::Linked(_)))),
            "{result:?}"
        );
        assert_eq!(read(&outside.join("c.txt")), "must survive\n");
    }

    /// The service remembers why the newest step was not saved, until a later step is.
    #[test]
    fn the_service_remembers_a_skipped_step() {
        let Some(fx) = Fixture::new("snapshot-service") else {
            return;
        };
        let service = Snapshots::default();
        let small = SnapshotEnv {
            max_files: 1,
            ..fx.env()
        };
        write(&fx.repo.join("x/1.txt"), "generated\n");
        write(&fx.repo.join("x/2.txt"), "generated\n");
        assert_eq!(
            service.take(&small, &fx.repo, KEY, "").unwrap(),
            TakeOutcome::TooManyFiles { limit: 1 }
        );
        let listed = service
            .list(&fx.env(), &fx.repo, &KEY.to_uppercase())
            .unwrap();
        assert_eq!(listed.skipped, Some(Skipped::TooManyFiles { limit: 1 }));
        assert!(matches!(
            service.take(&fx.env(), &fx.repo, KEY, "").unwrap(),
            TakeOutcome::Created { number: 1, .. }
        ));
        assert_eq!(
            service.list(&fx.env(), &fx.repo, KEY).unwrap().skipped,
            None
        );
        assert!(matches!(
            service.take(&fx.env(), &fx.repo, "not a key", ""),
            Err(SnapshotError::InvalidKey)
        ));
        assert_eq!(service.forget(&fx.env(), &fx.repo, KEY).unwrap(), 1);
        // The JSON shape the window reads.
        let json = serde_json::to_value(TakeOutcome::TooManyFiles { limit: 1 }).unwrap();
        assert_eq!(json["kind"], "tooManyFiles");
        let json = serde_json::to_value(Skipped::Failed {
            message: "x".to_owned(),
        })
        .unwrap();
        assert_eq!(json["reason"], "failed");
        let json = serde_json::to_value(RestoreOutcome::Restored { files: 1, undo: 2 }).unwrap();
        assert_eq!(json["kind"], "restored");
    }

    /// A session whose snapshot is still running holds up neither another session's snapshot nor
    /// its closing (`forget`).
    #[test]
    fn one_session_does_not_wait_for_another() {
        let Some(fx) = Fixture::new("snapshot-locks") else {
            return;
        };
        let service = Snapshots::default();
        let lock = service.session_lock(KEY);
        let _held = lock.lock().unwrap();
        let other = "11111111-2222-3333-4444-555555555555";
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(
            service.take(&fx.env(), &fx.repo, other, "").unwrap(),
            TakeOutcome::Created { number: 1, .. }
        ));
        assert_eq!(service.forget(&fx.env(), &fx.repo, other).unwrap(), 1);
    }

    /// Neither a clone, a fetch nor a push of branches and tags carries the snapshot refs; a mirror
    /// push does.
    #[test]
    fn snapshot_refs_stay_local() {
        let Some(fx) = Fixture::new("snapshot-refspecs") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(fx.take(""), TakeOutcome::Created { .. }));
        // `--no-local`: the same transport as a clone from a server.
        let source = fx.repo.to_string_lossy().into_owned();
        git_ok(
            &fx.git,
            fx.tmp.path(),
            &["clone", "--quiet", "--no-local", &source, "clone"],
        );
        let clone = fx.tmp.path().join("clone");
        git_ok(&fx.git, &clone, &["fetch", "--quiet", "origin"]);
        git_ok(
            &fx.git,
            fx.tmp.path(),
            &["init", "--quiet", "--bare", "bare.git"],
        );
        let bare = fx.tmp.path().join("bare.git");
        let target = bare.to_string_lossy().into_owned();
        git_ok(&fx.git, &fx.repo, &["push", "--quiet", &target, "main"]);
        git_ok(&fx.git, &fx.repo, &["push", "--quiet", "--all", &target]);
        git_ok(&fx.git, &fx.repo, &["push", "--quiet", "--tags", &target]);
        // What does carry them: a mirror push (every ref) and a mirror clone of this repository.
        git_ok(
            &fx.git,
            fx.tmp.path(),
            &["init", "--quiet", "--bare", "mirror.git"],
        );
        let mirror = fx.tmp.path().join("mirror.git");
        let mirror_target = mirror.to_string_lossy().into_owned();
        git_ok(
            &fx.git,
            &fx.repo,
            &["push", "--quiet", "--mirror", &mirror_target],
        );
        let mirrored = fx
            .git
            .run(
                &mirror,
                ["for-each-ref", "refs/hangar/"],
                Instant::now() + git::TIMEOUT,
                4096,
            )
            .unwrap();
        assert!(!mirrored.stdout.is_empty());
        for folder in [&clone, &bare] {
            let output = fx
                .git
                .run(
                    folder,
                    ["for-each-ref", "refs/hangar/"],
                    Instant::now() + git::TIMEOUT,
                    4096,
                )
                .unwrap();
            assert!(output.success);
            assert!(output.stdout.is_empty(), "{}", folder.display());
        }
    }
}
