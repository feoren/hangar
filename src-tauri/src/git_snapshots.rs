//! Silent snapshots of a session's work tree after each agent step, the timeline of them and the
//! jump back to one (contract: docs/wiki/pages/syntheses/work-tree-timeline.md).
//!
//! Nothing is written into the user's repository. Every repository gets a side repository of its
//! own in Hangar's data folder, `<local data>/git-snapshots/<hash>.git` (the hash names the
//! repository's common git folder). git runs in the user's work tree with `GIT_OBJECT_DIRECTORY`
//! pointing at the side repository's objects and the user's objects as an alternate
//! (`GIT_ALTERNATE_OBJECT_DIRECTORIES`), so every new blob, tree and commit lands in the side
//! repository, while unchanged files are read from the user's objects. The refs,
//! `refs/hangar/snapshots/<session key>/<n>`, live only in the side repository.
//!
//! A snapshot is what `git stash create` makes: the user's index is copied into a temporary index
//! file (so unchanged files keep their cached stat data), `git add -A` brings that copy up to the
//! work tree, and `write-tree` plus `commit-tree` (parent: HEAD) turn it into a commit. Ignored
//! files never go in; a step with too many or too large changed files is not saved. HEAD, the
//! branch, the index, the stash and the reflog stay as they were. Snapshots of one session are
//! capped and the oldest pruned; they go when the session is closed, and a start of Hangar
//! deletes those of sessions that will not come back.
//!
//! Every call runs through the hardened runner of git.rs (hooks off) with the repository's filter
//! drivers switched off. A jump back first checks every path (links, filters, folders in the way,
//! sparse entries), then saves the current state as a snapshot, so the jump can be undone, then
//! makes the work tree match the chosen snapshot: changed files get their content back, files
//! created since are removed.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant, SystemTime};

use serde::{Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::git::{self, Git, GitDiff, GitError, Output};
use crate::git_actions::{self, ActionError};
use crate::session::SessionId;

/// Namespace of all snapshot refs, in the side repository only.
pub const REF_PREFIX: &str = "refs/hangar/snapshots/";
/// Snapshots kept per session; the oldest go first.
pub const MAX_SNAPSHOTS: usize = 50;
/// Changed files (against HEAD and the index, untracked ones counted one by one) above which a step
/// is not saved: a folder of generated files nobody ignored.
pub const MAX_CHANGED_FILES: usize = 2000;
/// One changed file above this is not saved (a build artefact, a dump).
pub const MAX_FILE_BYTES: u64 = 50 * MIB;
/// All changed files of one step together.
pub const MAX_TOTAL_BYTES: u64 = 200 * MIB;
const MIB: u64 = 1024 * 1024;
/// One snapshot or one jump back, all git calls together.
pub const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(30);
/// Characters of the agent's last prompt kept with a snapshot.
pub const MAX_PROMPT_CHARS: usize = 200;
/// Folder below Hangar's local data folder for the side repositories.
pub const FOLDER: &str = "git-snapshots";
/// Folder below [`FOLDER`] for the temporary index files and the staged files of a jump back.
const TEMP_FOLDER: &str = "tmp";
/// Files below [`FOLDER`] for the lock every running Hangar holds shared ([`InstanceLock`]).
const LOCK_FILE: &str = ".lock";
const GATE_FILE: &str = ".gate";
/// All side repositories together; above it the clean-up at start drops the least recently used.
pub const MAX_FOLDER_BYTES: u64 = 2 * 1024 * MIB;
/// A side repository whose repository cannot be found (a drive that is not plugged in, a network
/// share, a WSL distribution) and whose newest snapshot is older than this goes.
const ORPHAN_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// `ls-files -v` lists every entry of the index; a large repository needs more than a file list.
const MAX_INDEX_LIST_BYTES: usize = 64 * 1024 * 1024;
/// File in a side repository that names the common git folder it belongs to.
const SOURCE_FILE: &str = "hangar-source";
/// A temporary index this old is a leftover: every operation ends after [`SNAPSHOT_TIMEOUT`].
const STALE_TEMP: Duration = Duration::from_secs(600);
/// Loose objects younger than this survive the `gc` at start, so a snapshot written at the same
/// time keeps its objects.
const PRUNE_EXPIRE: &str = "--prune=1.hour.ago";
/// First word of every snapshot's subject, so the commits explain themselves.
const SUBJECT_TAG: &str = "hangar-snapshot";
/// `commit-tree` needs an identity; the snapshot is Hangar's, not the user's.
const IDENTITY: [&str; 4] = [
    "-c",
    "user.name=Hangar",
    "-c",
    "user.email=snapshots@hangar.invalid",
];
const MAX_ID_BYTES: usize = 4096;
/// Paths per `ls-files` call; it reads no pathspec file, so they go on the command line.
const PATH_CHUNK: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error("Unknown session: {0}")]
    UnknownSession(SessionId),
    #[error("This session has no timeline yet")]
    UnboundSession(SessionId),
    #[error("Invalid session key")]
    InvalidKey,
    #[error("This snapshot no longer exists")]
    UnknownSnapshot,
    #[error("A session in this folder is working; go back once it is done")]
    Working,
    #[error("{0} is a folder with other files in it; going back would delete them")]
    FolderInTheWay(String),
    #[error("{0} is an ignored file, which no snapshot holds; going back would overwrite it")]
    IgnoredInTheWay(String),
    #[error("{0} is a symbolic link in the snapshot, which Hangar does not write; go back in the terminal")]
    SymlinkEntry(String),
    #[error("{0} leads outside the work tree or into a git folder")]
    Outside(String),
    #[error("The current state could not be saved first ({0}); nothing was changed")]
    CannotSaveCurrent(String),
    #[error("Hangar's snapshot folder cannot be used: {0}")]
    DataFolder(String),
    #[error("The repository's git folder has a path git cannot take as an alternate")]
    UnusablePath,
    #[error("git printed something unexpected: {0}")]
    Unexpected(&'static str),
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl SnapshotError {
    /// A stable code for the refusals the window shows in its own words (`src/timeline.ts`), and
    /// the path or reason they name.
    fn code(&self) -> Option<(&'static str, Option<&str>)> {
        Some(match self {
            Self::Working => ("snapshot_working", None),
            Self::FolderInTheWay(path) => ("snapshot_folder_in_the_way", Some(path.as_str())),
            Self::IgnoredInTheWay(path) => ("snapshot_ignored_in_the_way", Some(path.as_str())),
            Self::CannotSaveCurrent(reason) => {
                ("snapshot_cannot_save_current", Some(reason.as_str()))
            }
            Self::SymlinkEntry(path) => ("snapshot_symlink", Some(path.as_str())),
            Self::Outside(path) => ("snapshot_outside", Some(path.as_str())),
            Self::Action(ActionError::Linked(path)) => ("snapshot_linked", Some(path.as_str())),
            Self::Action(ActionError::Filtered) => ("snapshot_filtered", None),
            Self::Action(ActionError::TooLargeToCheck(path)) => {
                ("snapshot_too_large_to_check", Some(path.as_str()))
            }
            _ => return None,
        })
    }
}

/// `[code] English text`, then for a refusal that names a path or reason a newline and that
/// detail (`src/backendError.ts`). The English text never holds a newline, so the detail is
/// everything after the first one; it is the fallback for a window that does not know the code.
impl Serialize for SnapshotError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = self.to_string().replace(['\r', '\n'], " ");
        match self.code() {
            Some((code, Some(detail))) => {
                serializer.serialize_str(&format!("[{code}] {text}\n{detail}"))
            }
            Some((code, None)) => serializer.serialize_str(&format!("[{code}] {text}")),
            None => serializer.serialize_str(&text),
        }
    }
}

fn data_error(error: io::Error) -> SnapshotError {
    SnapshotError::DataFolder(error.to_string())
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
    /// Full id of the commit that was HEAD when the snapshot was taken; `None` without one.
    pub parent: Option<String>,
}

/// Why the newest step of a session was not saved; the window says so in the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub enum Skipped {
    TooManyFiles {
        limit: usize,
    },
    #[serde(rename_all = "camelCase")]
    TooLarge {
        file_mib: u64,
        total_mib: u64,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotList {
    pub is_repo: bool,
    /// Newest first.
    pub snapshots: Vec<Snapshot>,
    /// The newest step was not saved; `None` once a later one was (or nothing changed).
    pub skipped: Option<Skipped>,
    /// Other sessions whose folder lies in the same work tree.
    pub others: usize,
    /// A session in the same work tree (this one included) is working: going back is refused.
    pub busy: bool,
    /// The work tree's root as the platform writes it; empty outside a repository.
    pub root: String,
    /// Full id of the current HEAD commit; `None` on a branch without commits. A snapshot whose
    /// `parent` differs was taken on another commit (a branch switch since).
    pub head: Option<String>,
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
    #[serde(rename_all = "camelCase")]
    TooLarge {
        file_mib: u64,
        total_mib: u64,
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
    /// Writing failed part way; the state from before is saved as snapshot `undo`.
    Incomplete { undo: u64, message: String },
}

/// What the snapshot calls need besides git: the folder of the side repositories and the limits
/// (tests use small ones).
#[derive(Debug, Clone)]
pub struct SnapshotEnv<'a> {
    pub git: &'a Git,
    /// `<local data>/git-snapshots`.
    pub folder: &'a Path,
    pub max_snapshots: usize,
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    /// All side repositories together, enforced by the clean-up at start.
    pub max_folder_bytes: u64,
}

impl<'a> SnapshotEnv<'a> {
    pub fn new(git: &'a Git, folder: &'a Path) -> Self {
        Self {
            git,
            folder,
            max_snapshots: MAX_SNAPSHOTS,
            max_files: MAX_CHANGED_FILES,
            max_file_bytes: MAX_FILE_BYTES,
            max_total_bytes: MAX_TOTAL_BYTES,
            max_folder_bytes: MAX_FOLDER_BYTES,
        }
    }

    fn temp_dir(&self) -> PathBuf {
        self.folder.join(TEMP_FOLDER)
    }

    fn too_large(&self) -> (u64, u64) {
        (self.max_file_bytes / MIB, self.max_total_bytes / MIB)
    }
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

/// Name of the side repository of the repository whose common git folder is `common`.
fn side_name(common: &Path) -> String {
    let digest = Sha256::digest(git::path_key(common).as_bytes());
    let hex: String = digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{hex}.git")
}

/// `path` as one entry of `GIT_ALTERNATE_OBJECT_DIRECTORIES`: git splits the variable at the path
/// separator and reads a leading quote as a C string, so such a path cannot be named there.
fn alternate(path: &Path) -> Result<OsString, SnapshotError> {
    let text = path.to_string_lossy();
    let separator = if cfg!(windows) { ';' } else { ':' };
    if text.contains(separator) || text.starts_with('"') {
        return Err(SnapshotError::UnusablePath);
    }
    Ok(path.as_os_str().to_owned())
}

/// A temporary index file in Hangar's data folder, removed (with its lock) on drop.
struct TempIndex(PathBuf);

/// A name no other temporary file of this or another running Hangar has: process ID, a counter,
/// and `extension`.
fn temp_name(extension: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}.{extension}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

impl TempIndex {
    fn new(dir: &Path) -> Result<Self, SnapshotError> {
        fs::create_dir_all(dir).map_err(data_error)?;
        let path = dir.join(temp_name("index"));
        // A leftover of a crashed run with the same name would be read as a stale index.
        remove_quietly(&path);
        Ok(Self(path))
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

/// The side repository of the repository whose common git folder is `common`, created on first
/// use without templates (no hooks, no sample files).
fn side_repo(
    env: &SnapshotEnv<'_>,
    common: &Path,
    deadline: Instant,
) -> Result<PathBuf, SnapshotError> {
    static CREATING: Mutex<()> = Mutex::new(());
    let dir = env.folder.join(side_name(common));
    if !dir.join("HEAD").is_file() {
        let _creating = CREATING.lock().unwrap_or_else(PoisonError::into_inner);
        if !dir.join("HEAD").is_file() {
            fs::create_dir_all(env.folder).map_err(data_error)?;
            let args: Vec<OsString> = ["init", "--bare", "--quiet", "--template="]
                .into_iter()
                .map(OsString::from)
                .chain([dir.as_os_str().to_owned()])
                .collect();
            let output = env.git.run(env.folder, args, deadline, MAX_ID_BYTES)?;
            if !output.success {
                return Err(output.failure().into());
            }
        }
    }
    let source = dir.join(SOURCE_FILE);
    if !source.is_file() {
        fs::write(&source, common.to_string_lossy().as_bytes()).map_err(data_error)?;
    }
    Ok(dir)
}

/// The repository of a session's folder, its side repository, and what every call of one
/// operation shares.
struct Repo {
    root: PathBuf,
    deadline: Instant,
    /// `-c` settings before the subcommand: no filters, no split index, no reflog.
    overrides: Vec<String>,
    /// A filter driver is configured, so written files are checked for filter attributes.
    filters: bool,
    side: PathBuf,
    side_objects: OsString,
    user_objects: OsString,
    /// The work tree's root and its git folders, canonical (links resolved, long names): a file
    /// is written only into a folder inside the first and outside the others.
    real_root: PathBuf,
    real_git_dirs: Vec<PathBuf>,
}

impl Repo {
    fn open(env: &SnapshotEnv<'_>, cwd: &Path) -> Result<Option<Self>, SnapshotError> {
        git::check_folder(cwd)?;
        let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
        let Some(root) = git::toplevel(env.git, cwd, deadline)? else {
            return Ok(None);
        };
        let dirs = env.git.run(
            &root,
            [
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ],
            deadline,
            MAX_ID_BYTES,
        )?;
        if !dirs.success {
            return Err(dirs.failure().into());
        }
        let text = String::from_utf8_lossy(&dirs.stdout);
        let mut lines = text.lines().map(|line| root.join(git::native_path(line)));
        let (Some(git_dir), Some(common)) = (lines.next(), lines.next()) else {
            return Err(SnapshotError::Unexpected("no git folder"));
        };
        let real = |path: &Path| fs::canonicalize(path).map_err(data_error);
        let real_root = real(&root)?;
        let real_git_dirs = vec![real(&git_dir)?, real(&common)?];
        let user_objects = alternate(&common.join("objects"))?;
        let side = side_repo(env, &common, deadline)?;
        let side_objects = side.join("objects").into_os_string();
        let mut overrides: Vec<String> = [
            // A split index would write shared index files into the repository's git folder.
            "core.splitIndex=false",
            // `always` would give every snapshot ref a reflog of its own.
            "core.logAllRefUpdates=false",
            // Paths a checkout must refuse on any file system the work tree may live on (`.git`
            // spelled as an NTFS short name or with code points HFS+ ignores).
            "core.protectNTFS=true",
            "core.protectHFS=true",
            // Would mark the entries `add` updates assume-unchanged in the temporary index.
            "core.ignoreStat=false",
        ]
        .into_iter()
        .flat_map(|setting| ["-c".to_owned(), setting.to_owned()])
        .collect();
        let filter_overrides = git::filter_overrides(env.git, &root, deadline)?;
        let filters = filter_overrides
            .iter()
            .any(|arg| arg.starts_with("filter."));
        overrides.extend(filter_overrides);
        Ok(Some(Self {
            root,
            deadline,
            overrides,
            filters,
            side,
            side_objects,
            user_objects,
            real_root,
            real_git_dirs,
        }))
    }

    /// `GIT_OBJECT_DIRECTORY` at the side repository, the user's objects as an alternate, and the
    /// temporary index if one is named.
    fn vars<'s>(&'s self, index: Option<&'s TempIndex>) -> Vec<(&'static str, &'s OsStr)> {
        let mut vars = vec![
            ("GIT_OBJECT_DIRECTORY", self.side_objects.as_os_str()),
            (
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                self.user_objects.as_os_str(),
            ),
        ];
        if let Some(index) = index {
            vars.push(("GIT_INDEX_FILE", index.0.as_os_str()));
        }
        vars
    }

    /// Runs `git <overrides> <args>` in the work tree; a failure is git's message.
    fn run(
        &self,
        env: &SnapshotEnv<'_>,
        args: &[&str],
        index: Option<&TempIndex>,
        input: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<Output, SnapshotError> {
        let all = self
            .overrides
            .iter()
            .map(String::as_str)
            .chain(args.iter().copied());
        let output = env.git.run_with_env(
            &self.root,
            all,
            &self.vars(index),
            input,
            self.deadline,
            limit,
        )?;
        if !output.success {
            return Err(output.failure().into());
        }
        Ok(output)
    }

    /// Runs `git <args>` in the side repository, the user's objects as an alternate.
    fn side_run(
        &self,
        env: &SnapshotEnv<'_>,
        args: &[&str],
        input: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<Output, SnapshotError> {
        side_run(
            env,
            &self.side,
            Some(&self.user_objects),
            args,
            input,
            self.deadline,
            limit,
        )
    }

    /// One object id from a call that prints one (`write-tree`, `commit-tree`).
    fn id(
        &self,
        env: &SnapshotEnv<'_>,
        args: &[&str],
        index: Option<&TempIndex>,
        input: Option<Vec<u8>>,
    ) -> Result<String, SnapshotError> {
        let output = self.run(env, args, index, input, MAX_ID_BYTES)?;
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
        let output = env.git.run_with_env(
            &self.root,
            ["rev-parse", "--verify", "--quiet", revision],
            &self.vars(None),
            None,
            self.deadline,
            MAX_ID_BYTES,
        )?;
        let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok((output.success && is_object_id(&id)).then_some(id))
    }
}

/// `git -c core.logAllRefUpdates=false <args>` in the side repository `side`; a failure is git's
/// message.
fn side_run(
    env: &SnapshotEnv<'_>,
    side: &Path,
    user_objects: Option<&OsStr>,
    args: &[&str],
    input: Option<Vec<u8>>,
    deadline: Instant,
    limit: usize,
) -> Result<Output, SnapshotError> {
    let vars: Vec<(&str, &OsStr)> = user_objects
        .map(|objects| ("GIT_ALTERNATE_OBJECT_DIRECTORIES", objects))
        .into_iter()
        .collect();
    let all = ["-c", "core.logAllRefUpdates=false"]
        .into_iter()
        .chain(args.iter().copied());
    let output = env
        .git
        .run_with_env(side, all, &vars, input, deadline, limit)?;
    if !output.success {
        return Err(output.failure().into());
    }
    Ok(output)
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
    let output = repo.side_run(
        env,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(creatordate:unix)%00%(parent)%00%(subject)",
            &folder,
        ],
        None,
        git::MAX_LIST_BYTES,
    )?;
    let mut list: Vec<Entry> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\0');
            let name = fields.next()?;
            let sha = fields.next()?;
            let created_at = fields.next()?.trim().parse().ok()?;
            let parent = fields.next()?.trim();
            let parent = is_object_id(parent).then(|| parent.to_owned());
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
                    parent,
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
    let index = TempIndex::new(&env.temp_dir())?;
    let real = repo.run(
        env,
        &["rev-parse", "--git-path", "index"],
        None,
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
        Err(error) => return Err(data_error(error)),
    }
    clear_assume_unchanged(env, repo, &index)?;
    repo.run(
        env,
        &["add", "--all"],
        Some(&index),
        None,
        git::MAX_LIST_BYTES,
    )?;
    repo.id(env, &["write-tree"], Some(&index), None)
}

/// Clears the assume-unchanged bit (`update-index --assume-unchanged`, `core.ignoreStat`) of every
/// entry in the temporary index: `add --all` takes such an entry as unchanged and would leave the
/// file's newer content out of the snapshot, so going back would lose it.
fn clear_assume_unchanged(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    index: &TempIndex,
) -> Result<(), SnapshotError> {
    let listed = repo.run(
        env,
        &["ls-files", "-v", "-z"],
        Some(index),
        None,
        MAX_INDEX_LIST_BYTES,
    )?;
    if listed.truncated {
        return Err(SnapshotError::Unexpected("index too large"));
    }
    let marked = assume_unchanged(&listed.stdout);
    if marked.is_empty() {
        return Ok(());
    }
    repo.run(
        env,
        &["update-index", "--no-assume-unchanged", "-z", "--stdin"],
        Some(index),
        Some(marked),
        MAX_ID_BYTES,
    )?;
    Ok(())
}

/// The NUL separated paths of `ls-files -v -z` whose tag is a lower case letter: assume-unchanged.
fn assume_unchanged(output: &[u8]) -> Vec<u8> {
    let mut paths = Vec::new();
    for record in output.split(|b| *b == 0) {
        if let [tag, b' ', path @ ..] = record {
            if tag.is_ascii_lowercase() && !path.is_empty() {
                paths.extend_from_slice(path);
                paths.push(0);
            }
        }
    }
    paths
}

/// Mode of a symbolic link and of a submodule in a tree.
const LINK_MODE: &str = "120000";
const GITLINK_MODE: &str = "160000";

/// One file that differs between two trees: `D` deleted, `A` added, `M` (or another letter)
/// changed; mode and object id on the side of the second tree (empty for a deleted file).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    letter: u8,
    old_mode: String,
    new_mode: String,
    new_id: String,
    path: String,
}

impl Change {
    fn is_gitlink(&self) -> bool {
        self.old_mode == GITLINK_MODE || self.new_mode == GITLINK_MODE
    }
}

/// `diff-tree -r -z --raw`: `:<old mode> <new mode> <old id> <new id> <letter>\0<path>\0`.
fn parse_raw(output: &[u8]) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut records = output.split(|b| *b == 0);
    while let Some(head) = records.next() {
        let Some(head) = head.strip_prefix(b":") else {
            continue;
        };
        let Some(path) = records.next() else {
            break;
        };
        let head = String::from_utf8_lossy(head);
        let fields: Vec<&str> = head.split(' ').collect();
        let [old_mode, new_mode, _, new_id, status] = fields[..] else {
            continue;
        };
        let Some(&letter) = status.as_bytes().first() else {
            continue;
        };
        changes.push(Change {
            letter,
            old_mode: old_mode.to_owned(),
            new_mode: new_mode.to_owned(),
            new_id: if letter == b'D' {
                String::new()
            } else {
                new_id.to_owned()
            },
            path: String::from_utf8_lossy(path).into_owned(),
        });
    }
    changes
}

/// `ls-tree -r -z`: `<mode> <type> <id>\t<path>\0`, every entry as added.
fn parse_tree(output: &[u8]) -> Vec<Change> {
    output
        .split(|b| *b == 0)
        .filter_map(|record| {
            let tab = record.iter().position(|b| *b == b'\t')?;
            let head = String::from_utf8_lossy(&record[..tab]);
            let fields: Vec<&str> = head.split(' ').collect();
            let [mode, _, id] = fields[..] else {
                return None;
            };
            Some(Change {
                letter: b'A',
                old_mode: "000000".to_owned(),
                new_mode: mode.to_owned(),
                new_id: id.to_owned(),
                path: String::from_utf8_lossy(&record[tab + 1..]).into_owned(),
            })
        })
        .collect()
}

/// Files that differ between two trees (or, without `from`, all files of `to`), submodules left
/// out.
fn changed_files(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    from: Option<&str>,
    to: &str,
) -> Result<Vec<Change>, SnapshotError> {
    type Parse = fn(&[u8]) -> Vec<Change>;
    let (output, parse): (Output, Parse) = match from {
        Some(from) => (
            repo.run(
                env,
                &[
                    "diff-tree",
                    "-r",
                    "-z",
                    "--raw",
                    "--no-abbrev",
                    "--no-renames",
                    "--ignore-submodules=all",
                    from,
                    to,
                    "--",
                ],
                None,
                None,
                git::MAX_LIST_BYTES,
            )?,
            parse_raw,
        ),
        None => (
            repo.run(
                env,
                &["ls-tree", "-r", "-z", "--full-tree", to],
                None,
                None,
                git::MAX_LIST_BYTES,
            )?,
            parse_tree,
        ),
    };
    if output.truncated {
        return Err(SnapshotError::Unexpected("file list too long"));
    }
    Ok(parse(&output.stdout)
        .into_iter()
        .filter(|change| !change.is_gitlink())
        .collect())
}

/// Sizes of the objects `ids` (one `cat-file --batch-check` for all), in order.
fn object_sizes(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    ids: &[&str],
) -> Result<Vec<u64>, SnapshotError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut input = Vec::new();
    for id in ids {
        input.extend_from_slice(id.as_bytes());
        input.push(b'\n');
    }
    let output = repo.run(
        env,
        &["cat-file", "--batch-check=%(objectsize)"],
        None,
        Some(input),
        git::MAX_LIST_BYTES,
    )?;
    let sizes: Vec<u64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    if output.truncated || sizes.len() != ids.len() {
        return Err(SnapshotError::Unexpected("object sizes"));
    }
    Ok(sizes)
}

/// Whether one size exceeds the per file limit or all together the total.
fn sizes_exceed(env: &SnapshotEnv<'_>, sizes: impl IntoIterator<Item = u64>) -> bool {
    let mut total: u64 = 0;
    for size in sizes {
        if size > env.max_file_bytes {
            return true;
        }
        total = total.saturating_add(size);
        if total > env.max_total_bytes {
            return true;
        }
    }
    false
}

/// The work tree as a tree object, unless too much changed to save it.
enum Current {
    Tree(String),
    TooManyFiles,
    TooLarge,
}

/// Whether the changed files on disk exceed one file's or all files' size limit.
fn exceeds_sizes(env: &SnapshotEnv<'_>, root: &Path, paths: &[String]) -> bool {
    // A deleted file has no size; a link counts as the link, never its target.
    let sizes = paths.iter().filter_map(|path| {
        fs::symlink_metadata(root.join(path))
            .ok()
            .filter(fs::Metadata::is_file)
            .map(|meta| meta.len())
    });
    sizes_exceed(env, sizes)
}

/// Whether the blobs `tree` holds beyond HEAD's tree exceed the limits. The check on disk runs
/// before `add`, so a file that grew in between is caught here, by the size git stored.
fn tree_too_large(env: &SnapshotEnv<'_>, repo: &Repo, tree: &str) -> Result<bool, SnapshotError> {
    let head = repo.resolve(env, "HEAD^{tree}")?;
    let changes = changed_files(env, repo, head.as_deref(), tree)?;
    let ids: Vec<&str> = changes
        .iter()
        .filter(|change| change.letter != b'D')
        .map(|change| change.new_id.as_str())
        .collect();
    let sizes = object_sizes(env, repo, &ids)?;
    Ok(sizes_exceed(env, sizes))
}

/// The work tree as a tree object, or why it is not saved: more than `max_files` files changed
/// against HEAD and the index (untracked ones counted one by one), or the changed files are
/// too large.
fn current_tree(env: &SnapshotEnv<'_>, repo: &Repo) -> Result<Current, SnapshotError> {
    let status = env.git.run_with_env(
        &repo.root,
        repo.overrides.iter().map(String::as_str).chain([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ]),
        &repo.vars(None),
        None,
        repo.deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !status.usable() {
        return Err(status.failure().into());
    }
    let changes = git::parse_status(&status.stdout);
    if status.truncated || changes.len() > env.max_files {
        return Ok(Current::TooManyFiles);
    }
    let paths: Vec<String> = changes.into_iter().map(|change| change.path).collect();
    if exceeds_sizes(env, &repo.root, &paths) {
        return Ok(Current::TooLarge);
    }
    let tree = work_tree_tree(env, repo)?;
    if tree_too_large(env, repo, &tree)? {
        // Its blobs stay unreferenced in the side repository until the next `gc` drops them.
        return Ok(Current::TooLarge);
    }
    Ok(Current::Tree(tree))
}

/// The state the work tree is in after [`record`].
enum Taken {
    Created {
        number: u64,
        files: u64,
    },
    /// Matches the newest snapshot (`number`), or HEAD when there is none (`None`).
    Unchanged {
        number: Option<u64>,
    },
}

/// Keeps `tree` as the session's next snapshot, unless it matches the newest one (or, for the
/// first step, HEAD), and prunes the oldest beyond the cap in the same ref transaction. The
/// snapshot numbered `keep` (the target of a jump back) is never pruned, so it stays in the list
/// as the state the work tree now holds; the cap may then be passed by one.
fn record(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    key: &str,
    prompt: &str,
    kind: SnapshotKind,
    tree: &str,
    keep: Option<u64>,
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
    let commit = repo.id(env, &args, None, Some(message))?;

    let number = last.map_or(1, |last| last.number + 1);
    let mut transaction = format!("create {}{number} {commit}\n", ref_folder(key));
    let excess = (list.len() + 1).saturating_sub(env.max_snapshots.max(1));
    for old in list
        .iter()
        .filter(|entry| Some(entry.snapshot.number) != keep)
        .take(excess)
    {
        transaction.push_str(&format!("delete {}\n", old.name));
    }
    repo.side_run(
        env,
        &["update-ref", "--stdin"],
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
    let tree = match current_tree(env, &repo)? {
        Current::Tree(tree) => tree,
        Current::TooManyFiles => {
            return Ok(TakeOutcome::TooManyFiles {
                limit: env.max_files,
            })
        }
        Current::TooLarge => {
            let (file_mib, total_mib) = env.too_large();
            return Ok(TakeOutcome::TooLarge {
                file_mib,
                total_mib,
            });
        }
    };
    Ok(
        match record(env, &repo, &key, prompt, SnapshotKind::Step, &tree, None)? {
            Taken::Created { number, files } => TakeOutcome::Created { number, files },
            Taken::Unchanged { .. } => TakeOutcome::Unchanged,
        },
    )
}

/// The sessions Hangar runs, for the checks of one work tree: each with its folder, and which of
/// them the window reports as working.
#[derive(Debug, Clone, Default)]
pub struct Neighbours {
    pub sessions: Vec<(SessionId, PathBuf)>,
    pub working: BTreeSet<SessionId>,
}

impl Neighbours {
    /// Other sessions than `id` whose folder lies in the work tree `root`, and whether any
    /// session there (`id` included) is working.
    pub fn in_tree(&self, root: &Path, id: SessionId) -> (usize, bool) {
        let real = |path: &Path| {
            git::path_key(&fs::canonicalize(path).unwrap_or_else(|_| path.to_owned()))
        };
        let root = real(root);
        let inside: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, cwd)| git::key_within(&real(cwd), &root))
            .map(|(session, _)| *session)
            .collect();
        let others = inside.iter().filter(|session| **session != id).count();
        let busy = inside.iter().any(|session| self.working.contains(session));
        (others, busy)
    }
}

/// The session's snapshots, newest first, with the work tree's root; `skipped` is the
/// service's to fill.
fn list_in(
    env: &SnapshotEnv<'_>,
    cwd: &Path,
    key: &str,
) -> Result<(SnapshotList, Option<PathBuf>), SnapshotError> {
    let key = check_key(key)?;
    let Some(repo) = Repo::open(env, cwd)? else {
        let empty = SnapshotList {
            is_repo: false,
            snapshots: Vec::new(),
            skipped: None,
            others: 0,
            busy: false,
            root: String::new(),
            head: None,
        };
        return Ok((empty, None));
    };
    let mut snapshots: Vec<Snapshot> = entries(env, &repo, &key)?
        .into_iter()
        .map(|entry| entry.snapshot)
        .collect();
    snapshots.reverse();
    let list = SnapshotList {
        is_repo: true,
        snapshots,
        skipped: None,
        others: 0,
        busy: false,
        root: repo.root.to_string_lossy().into_owned(),
        head: repo.resolve(env, "HEAD^{commit}")?,
    };
    Ok((list, Some(repo.root)))
}

/// The session's snapshots, newest first.
pub fn list(env: &SnapshotEnv<'_>, cwd: &Path, key: &str) -> Result<SnapshotList, SnapshotError> {
    list_in(env, cwd, key).map(|(list, _)| list)
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
        &repo.vars(None),
        from.as_deref(),
        sha,
        repo.deadline,
    )?)
}

/// Paths among `paths` that the user's index marks skip-worktree (sparse checkout): they are not
/// in the work tree on purpose, so going back leaves them alone.
fn skip_worktree(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    paths: &[&str],
) -> Result<HashSet<String>, SnapshotError> {
    let mut skipped = HashSet::new();
    for chunk in paths.chunks(PATH_CHUNK) {
        let all = repo
            .overrides
            .iter()
            .map(String::as_str)
            .chain(["--literal-pathspecs", "ls-files", "-z", "-t", "--"])
            .chain(chunk.iter().copied());
        let output = env.git.run_with_env(
            &repo.root,
            all,
            &repo.vars(None),
            None,
            repo.deadline,
            git::MAX_LIST_BYTES,
        )?;
        if !output.success || output.truncated {
            return Err(output.failure().into());
        }
        for record in output.stdout.split(|b| *b == 0) {
            if let Some(path) = record.strip_prefix(b"S ") {
                skipped.insert(String::from_utf8_lossy(path).into_owned());
            }
        }
    }
    Ok(skipped)
}

/// Whether `path` is a folder that holds anything besides the files in `deleted` (paths relative
/// to `root`, `/` separated): `checkout-index --force` would delete it to write the file.
fn folder_in_the_way(root: &Path, path: &str, deleted: &HashSet<&str>) -> bool {
    let start = root.join(path);
    match fs::symlink_metadata(&start) {
        Ok(meta) if meta.is_dir() && !git_actions::is_link(&meta) => {}
        _ => return false,
    }
    let mut pending = vec![(start, path.to_owned())];
    let mut seen = 0usize;
    while let Some((dir, relative)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            return true;
        };
        for entry in entries {
            seen += 1;
            let Ok(entry) = entry else {
                return true;
            };
            if seen > git_actions::MAX_LINK_SCAN {
                return true;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = format!("{relative}/{name}");
            match fs::symlink_metadata(entry.path()) {
                Ok(meta) if meta.is_dir() && !git_actions::is_link(&meta) => {
                    pending.push((entry.path(), child));
                }
                Ok(_) if deleted.contains(child.as_str()) => {}
                _ => return true,
            }
        }
    }
    false
}

/// Where writing the file `path` (`added`: the current tree lacks it) would replace something
/// that no snapshot holds, an ignored file: the file itself already on disk, or a file where one
/// of its folders goes (`checkout-index --force` replaces both). Returns that spot; spots for
/// which `goes_first` says they are deleted before the write are no obstacle.
fn ignored_in_the_way(
    root: &Path,
    path: &str,
    added: bool,
    goes_first: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let is_file =
        |relative: &str| fs::symlink_metadata(root.join(relative)).is_ok_and(|meta| !meta.is_dir());
    let folders = path.match_indices('/').map(|(end, _)| &path[..end]);
    folders
        .chain(added.then_some(path))
        .find(|spot| is_file(spot) && !goes_first(spot))
        .map(str::to_owned)
}

/// Makes the work tree match snapshot `number`: files changed since get its content back, files
/// created since are removed, ignored files, the index, HEAD and the branch stay, as do entries
/// the index marks skip-worktree. Before anything is written: refused while `busy(root)` says a
/// session works in the work tree, when a path leads through a link, holds a symbolic link in the
/// snapshot, needs a filter driver, or a folder with other files or an ignored file stands where a
/// file goes, and when the current state cannot be saved. The snapshot's files are staged outside
/// the work tree, the current state is saved as a snapshot (`undo` in the answer), so the jump
/// can be undone, and right before the first write `busy` is asked again and the work tree must
/// still be the state just saved.
pub fn restore(
    env: &SnapshotEnv<'_>,
    cwd: &Path,
    key: &str,
    number: u64,
    busy: &dyn Fn(&Path) -> bool,
) -> Result<RestoreOutcome, SnapshotError> {
    let key = check_key(key)?;
    let repo = Repo::open(env, cwd)?.ok_or(GitError::NotARepository)?;
    if busy(&repo.root) {
        return Err(SnapshotError::Working);
    }
    let target = entries(env, &repo, &key)?
        .into_iter()
        .find(|entry| entry.snapshot.number == number)
        .ok_or(SnapshotError::UnknownSnapshot)?;
    let target_tree = repo
        .resolve(env, &format!("{}^{{tree}}", target.snapshot.sha))?
        .ok_or(SnapshotError::UnknownSnapshot)?;

    let current_tree = match current_tree(env, &repo) {
        Ok(Current::Tree(tree)) => tree,
        Ok(Current::TooManyFiles) => {
            return Err(SnapshotError::CannotSaveCurrent(format!(
                "more than {} changed files",
                env.max_files
            )))
        }
        Ok(Current::TooLarge) => {
            let (file_mib, total_mib) = env.too_large();
            return Err(SnapshotError::CannotSaveCurrent(format!(
                "changed files above {file_mib} MiB each or {total_mib} MiB together"
            )));
        }
        Err(error) => return Err(SnapshotError::CannotSaveCurrent(error.to_string())),
    };
    if current_tree == target_tree {
        return Ok(RestoreOutcome::AlreadyThere);
    }

    let all = changed_files(env, &repo, Some(&current_tree), &target_tree)?;
    for change in &all {
        git_actions::check_path(&change.path)?;
        git_actions::refuse_links(&repo.root, &change.path)?;
        // git would write a link whose target the snapshot names, possibly outside the work tree.
        if change.letter != b'D' && change.new_mode == LINK_MODE {
            return Err(SnapshotError::SymlinkEntry(change.path.clone()));
        }
    }
    let names: Vec<&str> = all.iter().map(|change| change.path.as_str()).collect();
    let sparse = skip_worktree(env, &repo, &names)?;
    let changes: Vec<&Change> = all
        .iter()
        .filter(|change| !sparse.contains(&change.path))
        .collect();
    let (deleted, written): (Vec<&Change>, Vec<&Change>) =
        changes.iter().partition(|change| change.letter == b'D');
    let deleted: Vec<&str> = deleted.iter().map(|change| change.path.as_str()).collect();
    let added: HashSet<&str> = written
        .iter()
        .filter(|change| change.letter == b'A')
        .map(|change| change.path.as_str())
        .collect();
    let written: Vec<String> = written.iter().map(|change| change.path.clone()).collect();
    if repo.filters {
        git_actions::refuse_filtered_files(env.git, &repo.root, repo.deadline, &written)?;
    }
    let gone: HashSet<&str> = deleted.iter().copied().collect();
    if let Some(path) = written
        .iter()
        .find(|path| folder_in_the_way(&repo.root, path, &gone))
    {
        return Err(SnapshotError::FolderInTheWay(path.clone()));
    }
    // A file system that ignores case finds a deleted `README.md` under a new `Readme.md`; on
    // one that does not, such a spot is caught again after the deletes, before anything is written.
    let folded: HashSet<String> = deleted.iter().map(|path| path.to_lowercase()).collect();
    let goes_first = |spot: &str| gone.contains(spot) || folded.contains(&spot.to_lowercase());
    if let Some(spot) = written.iter().find_map(|path| {
        ignored_in_the_way(&repo.root, path, added.contains(path.as_str()), &goes_first)
    }) {
        return Err(SnapshotError::IgnoredInTheWay(spot));
    }
    // git writes the files into Hangar's data folder; a failure there changes nothing.
    let staging = Staging::new(&env.temp_dir())?;
    stage(env, &repo, &staging, &written, &target_tree)?;

    let undo = record(
        env,
        &repo,
        &key,
        "",
        SnapshotKind::Undo,
        &current_tree,
        Some(target.snapshot.number),
    );
    let undo = match undo {
        Ok(Taken::Created { number, .. })
        | Ok(Taken::Unchanged {
            number: Some(number),
        }) => number,
        Ok(Taken::Unchanged { number: None }) => {
            return Err(SnapshotError::Unexpected("no snapshot to return to"))
        }
        Err(error) => return Err(SnapshotError::CannotSaveCurrent(error.to_string())),
    };
    // The last look before the first write: an agent that started during the checks, or a file
    // saved meanwhile, would otherwise be overwritten by a state the undo point does not hold.
    if busy(&repo.root) {
        return Err(SnapshotError::Working);
    }
    match work_tree_tree(env, &repo) {
        Ok(tree) if tree == current_tree => {}
        Ok(_) => return Err(SnapshotError::Working),
        Err(error) => return Err(SnapshotError::CannotSaveCurrent(error.to_string())),
    }
    match write_back(&repo, &deleted, &written, &added, &staging) {
        Ok(()) => Ok(RestoreOutcome::Restored {
            files: u64::try_from(changes.len()).unwrap_or(u64::MAX),
            undo,
        }),
        Err(error) => Ok(RestoreOutcome::Incomplete {
            undo,
            message: error.to_string(),
        }),
    }
}

/// A folder below Hangar's data folder that holds the files of one jump back, removed on drop.
struct Staging(PathBuf);

impl Staging {
    fn new(dir: &Path) -> Result<Self, SnapshotError> {
        fs::create_dir_all(dir).map_err(data_error)?;
        let path = dir.join(temp_name("stage"));
        // A leftover of a crashed run with the same name.
        if fs::symlink_metadata(&path).is_ok() {
            fs::remove_dir_all(&path).map_err(data_error)?;
        }
        fs::create_dir(&path).map_err(data_error)?;
        Ok(Self(path))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!("hangar: could not remove {}: {error}", self.0.display());
            }
        }
    }
}

/// Writes the files `written` of `tree` below `staging` with `checkout-index --prefix`, so git
/// converts them (line endings, `ident`) as it would in the work tree, which it never touches.
fn stage(
    env: &SnapshotEnv<'_>,
    repo: &Repo,
    staging: &Staging,
    written: &[String],
    tree: &str,
) -> Result<(), SnapshotError> {
    if written.is_empty() {
        return Ok(());
    }
    let index = TempIndex::new(&env.temp_dir())?;
    repo.run(env, &["read-tree", tree], Some(&index), None, MAX_ID_BYTES)?;
    let mut prefix = staging.0.to_string_lossy().replace('\\', "/");
    prefix.push('/');
    let prefix = format!("--prefix={prefix}");
    let mut input = Vec::new();
    for path in written {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
    }
    repo.run(
        env,
        &[
            // The staging folder adds to the path's length (Git for Windows only).
            "-c",
            "core.longpaths=true",
            "checkout-index",
            "--force",
            "-z",
            "--stdin",
            &prefix,
        ],
        Some(&index),
        Some(input),
        git::MAX_LIST_BYTES,
    )?;
    Ok(())
}

/// The writing half of [`restore`], after every check passed and the undo point is saved.
fn write_back(
    repo: &Repo,
    deleted: &[&str],
    written: &[String],
    added: &HashSet<&str>,
    staging: &Staging,
) -> Result<(), SnapshotError> {
    for path in deleted {
        // Again right before the delete: a folder on the way may have become a link since.
        git_actions::refuse_links(&repo.root, path)?;
        remove_file(repo, path)?;
    }
    if written.is_empty() {
        return Ok(());
    }
    // The deletes are done: any folder or file left where a file goes holds something else.
    let none = HashSet::new();
    if let Some(path) = written
        .iter()
        .find(|path| folder_in_the_way(&repo.root, path, &none))
    {
        return Err(SnapshotError::FolderInTheWay(path.clone()));
    }
    if let Some(spot) = written.iter().find_map(|path| {
        ignored_in_the_way(&repo.root, path, added.contains(path.as_str()), &|_| false)
    }) {
        return Err(SnapshotError::IgnoredInTheWay(spot));
    }
    for path in written {
        install(repo, path, &staging.0.join(path))?;
    }
    Ok(())
}

fn write_error(path: &str, error: io::Error) -> SnapshotError {
    GitError::Failed(format!("{path} could not be written: {error}")).into()
}

/// The canonical form of the existing folder `dir`, refused unless it lies inside the work tree
/// and outside its git folders. Canonical means links resolved and long names, so a folder
/// reached through a junction swapped in since the checks, or named by an NTFS short name of a git
/// folder (`GIT~1`, `GI7EBA~1`), is caught.
fn inside(repo: &Repo, dir: &Path, path: &str) -> Result<PathBuf, SnapshotError> {
    let real = fs::canonicalize(dir).map_err(|error| write_error(path, error))?;
    if !real.starts_with(&repo.real_root)
        || repo.real_git_dirs.iter().any(|git| real.starts_with(git))
    {
        return Err(SnapshotError::Outside(path.to_owned()));
    }
    Ok(real)
}

/// Puts the staged file at `path` (relative to the root, `/` separated). Right before: every
/// folder on the way is created if missing and must be a plain folder (no link, no junction),
/// the deepest must lie inside the work tree ([`inside`]), and a file already there must be the
/// one the path names. The file goes in as a new temporary file beside it and a rename, which
/// replaces a file but never a folder and never follows a link at the target.
fn install(repo: &Repo, path: &str, staged: &Path) -> Result<(), SnapshotError> {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    let mut dir = repo.root.clone();
    let mut walked = String::new();
    for part in folder.split('/').filter(|part| !part.is_empty()) {
        dir.push(part);
        if !walked.is_empty() {
            walked.push('/');
        }
        walked.push_str(part);
        if let Err(error) = fs::create_dir(&dir) {
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(write_error(path, error));
            }
        }
        match fs::symlink_metadata(&dir) {
            Ok(meta) if git_actions::is_link(&meta) => {
                return Err(ActionError::Linked(path.to_owned()).into())
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(SnapshotError::IgnoredInTheWay(walked)),
            Err(error) => return Err(write_error(path, error)),
        }
    }
    let real_dir = inside(repo, &dir, path)?;
    let target = dir.join(name);
    match fs::symlink_metadata(&target) {
        Ok(meta) if git_actions::is_link(&meta) => {
            return Err(ActionError::Linked(path.to_owned()).into())
        }
        Ok(meta) if meta.is_dir() => return Err(SnapshotError::FolderInTheWay(path.to_owned())),
        Ok(_) => {
            // An NTFS short name (`README~1.MD`) would name another file than the snapshot's.
            let real = fs::canonicalize(&target).map_err(|error| write_error(path, error))?;
            let same = real.parent() == Some(real_dir.as_path())
                && real.file_name().is_some_and(|real| {
                    real.to_string_lossy().to_lowercase() == name.to_lowercase()
                });
            if !same {
                return Err(SnapshotError::Outside(path.to_owned()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(write_error(path, error)),
    }
    let temp = dir.join(format!(".hangar-{}", temp_name("restore")));
    copy_new(staged, &temp).map_err(|error| write_error(path, error))?;
    if let Err(error) = fs::rename(&temp, &target) {
        remove_quietly(&temp);
        return Err(write_error(path, error));
    }
    Ok(())
}

/// Copies `from` into the new file `to` (never an existing one, never through a link there),
/// with the permissions git gave the staged file (the executable bit).
fn copy_new(from: &Path, to: &Path) -> io::Result<()> {
    let mut source = fs::File::open(from)?;
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    let copied = io::copy(&mut source, &mut target)
        .and_then(|_| target.set_permissions(source.metadata()?.permissions()));
    drop(target);
    if copied.is_err() {
        remove_quietly(to);
    }
    copied
}

/// Removes a file the snapshot does not have, then its folders up to the root as long as they are
/// empty (a folder that holds an ignored file stays).
fn remove_file(repo: &Repo, path: &str) -> Result<(), SnapshotError> {
    let root = repo.root.as_path();
    let target = root.join(path);
    if let Some(parent) = target.parent().filter(|parent| parent.exists()) {
        inside(repo, parent, path)?;
    }
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

/// The snapshot refs below `prefix` in the side repository `side`.
fn side_refs(
    env: &SnapshotEnv<'_>,
    side: &Path,
    prefix: &str,
    deadline: Instant,
) -> Result<Vec<String>, SnapshotError> {
    let output = side_run(
        env,
        side,
        None,
        &["for-each-ref", "--format=%(refname)", prefix],
        None,
        deadline,
        git::MAX_LIST_BYTES,
    )?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|name| name.starts_with(REF_PREFIX))
        .map(str::to_owned)
        .collect())
}

fn delete_refs(
    env: &SnapshotEnv<'_>,
    side: &Path,
    names: &[String],
    deadline: Instant,
) -> Result<(), SnapshotError> {
    if names.is_empty() {
        return Ok(());
    }
    let transaction: String = names
        .iter()
        .map(|name| format!("delete {name}\n"))
        .collect();
    side_run(
        env,
        side,
        None,
        &["update-ref", "--stdin"],
        Some(transaction.into_bytes()),
        deadline,
        MAX_ID_BYTES,
    )?;
    Ok(())
}

/// Deletes every snapshot of the session (it was closed); returns how many there were.
pub fn forget(env: &SnapshotEnv<'_>, cwd: &Path, key: &str) -> Result<usize, SnapshotError> {
    let key = check_key(key)?;
    let Some(repo) = Repo::open(env, cwd)? else {
        return Ok(0);
    };
    let names = side_refs(env, &repo.side, &ref_folder(&key), repo.deadline)?;
    delete_refs(env, &repo.side, &names, repo.deadline)?;
    Ok(names.len())
}

/// The session key in a snapshot ref name.
fn key_of_ref(name: &str) -> Option<&str> {
    name.strip_prefix(REF_PREFIX)?.split('/').next()
}

/// What [`clean_up`] removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanUp {
    pub temp_files: usize,
    pub side_repos: usize,
    pub refs: usize,
}

/// At start, before any session takes a snapshot: removes leftover temporary files, the side
/// repositories whose repository is gone, and with `keep` (the keys of the sessions that will be
/// restored) the snapshots of every other session; a side repository left without snapshots goes
/// as a whole, the others drop their unreferenced objects (`gc`). Last, all side repositories
/// together are brought below [`SnapshotEnv::max_folder_bytes`]. `keep: None` (the saved
/// workspace could not be read) keeps every snapshot of an existing repository. Runs only while
/// no other Hangar uses the folder ([`InstanceLock`]).
pub fn clean_up(env: &SnapshotEnv<'_>, keep: Option<&BTreeSet<String>>) -> CleanUp {
    let mut done = CleanUp::default();
    let now = SystemTime::now();
    if let Ok(files) = fs::read_dir(env.temp_dir()) {
        for file in files.flatten() {
            let Ok(meta) = fs::symlink_metadata(file.path()) else {
                continue;
            };
            let stale = meta
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > STALE_TEMP);
            if !stale {
                continue;
            }
            let removed = if meta.is_dir() && !git_actions::is_link(&meta) {
                fs::remove_dir_all(file.path())
            } else {
                fs::remove_file(file.path())
            };
            if removed.is_ok() {
                done.temp_files += 1;
            }
        }
    }
    let Ok(sides) = fs::read_dir(env.folder) else {
        return done;
    };
    let mut kept = Vec::new();
    for side in sides.flatten() {
        let path = side.path();
        let is_side = path.extension() == Some(OsStr::new("git"))
            && fs::symlink_metadata(&path)
                .is_ok_and(|meta| meta.is_dir() && !git_actions::is_link(&meta));
        if !is_side {
            continue;
        }
        match clean_side(env, &path, keep, now) {
            Ok(SideState::Removed) => done.side_repos += 1,
            Ok(SideState::Kept { refs }) => {
                done.refs += refs;
                kept.push(path);
            }
            Err(error) => {
                eprintln!(
                    "hangar: snapshot clean-up of {} skipped: {error}",
                    path.display()
                );
                kept.push(path);
            }
        }
    }
    enforce_cap(env, kept, &mut done);
    done
}

enum SideState {
    Removed,
    Kept { refs: usize },
}

/// Whether the drive a repository lived on is there. Only a local drive letter is looked at: a
/// network share or a WSL distribution may take long to answer or start on access, and a mount
/// point on Unix stays as an empty folder when its drive is unplugged.
fn volume_present(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                Path::new(&format!("{}:\\", char::from(letter))).is_dir()
            }
            _ => false,
        },
        _ => false,
    }
}

/// The newest snapshot's time (Unix seconds) in the side repository, `None` without snapshots.
fn newest_snapshot(
    env: &SnapshotEnv<'_>,
    side: &Path,
    deadline: Instant,
) -> Result<Option<i64>, SnapshotError> {
    let output = side_run(
        env,
        side,
        None,
        &["for-each-ref", "--format=%(creatordate:unix)", REF_PREFIX],
        None,
        deadline,
        git::MAX_LIST_BYTES,
    )?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .max())
}

fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |age| i64::try_from(age.as_secs()).unwrap_or(i64::MAX))
}

fn clean_side(
    env: &SnapshotEnv<'_>,
    side: &Path,
    keep: Option<&BTreeSet<String>>,
    now: SystemTime,
) -> Result<SideState, SnapshotError> {
    let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
    let source = fs::read_to_string(side.join(SOURCE_FILE))
        .ok()
        .map(|text| PathBuf::from(text.trim_end()));
    let Some(source) = source else {
        // Not one Hangar wrote completely.
        fs::remove_dir_all(side).map_err(data_error)?;
        return Ok(SideState::Removed);
    };
    let objects = source.join("objects");
    if !objects.is_dir() {
        // Gone for sure only when its drive is there; otherwise (unplugged, a share, WSL) once
        // its newest snapshot is old.
        let newest = newest_snapshot(env, side, deadline)?;
        let old = newest.is_none_or(|newest| {
            unix_seconds(now).saturating_sub(newest)
                > i64::try_from(ORPHAN_AGE.as_secs()).unwrap_or(i64::MAX)
        });
        if volume_present(&source) || old {
            fs::remove_dir_all(side).map_err(data_error)?;
            return Ok(SideState::Removed);
        }
        return Ok(SideState::Kept { refs: 0 });
    }
    let Some(keep) = keep else {
        return Ok(SideState::Kept { refs: 0 });
    };
    let names = side_refs(env, side, REF_PREFIX, deadline)?;
    let (kept, dropped): (Vec<String>, Vec<String>) = names
        .into_iter()
        .partition(|name| key_of_ref(name).is_some_and(|key| keep.contains(key)));
    if kept.is_empty() {
        fs::remove_dir_all(side).map_err(data_error)?;
        return Ok(SideState::Removed);
    }
    delete_refs(env, side, &dropped, deadline)?;
    gc(env, side, &objects, deadline);
    Ok(SideState::Kept {
        refs: dropped.len(),
    })
}

/// `git gc` in the side repository with the user's objects as an alternate: packs what the
/// snapshots hold and drops unreferenced objects older than an hour. A failure (an object of the
/// user's repository gone after a gc there) deletes nothing.
fn gc(env: &SnapshotEnv<'_>, side: &Path, objects: &Path, deadline: Instant) {
    let result = alternate(objects).and_then(|objects| {
        side_run(
            env,
            side,
            Some(&objects),
            &["gc", "--quiet", PRUNE_EXPIRE],
            None,
            deadline,
            MAX_ID_BYTES,
        )
    });
    if let Err(error) = result {
        eprintln!("hangar: snapshot gc of {} skipped: {error}", side.display());
    }
}

/// Bytes of the files below `dir`, links not followed.
fn folder_bytes(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() && !git_actions::is_link(&meta) {
                pending.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

/// Above the cap the side repositories whose newest snapshot is oldest go first; the last one
/// left keeps the newer half of each session's snapshots instead of going.
fn enforce_cap(env: &SnapshotEnv<'_>, sides: Vec<PathBuf>, done: &mut CleanUp) {
    let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
    let mut sized: Vec<(PathBuf, u64, i64)> = sides
        .into_iter()
        .map(|side| {
            let bytes = folder_bytes(&side);
            let newest = newest_snapshot(env, &side, deadline)
                .ok()
                .flatten()
                .unwrap_or(0);
            (side, bytes, newest)
        })
        .collect();
    let mut total: u64 = sized.iter().map(|(_, bytes, _)| *bytes).sum();
    if total <= env.max_folder_bytes {
        return;
    }
    // Newest first, so the oldest is popped.
    sized.sort_by_key(|(_, _, newest)| std::cmp::Reverse(*newest));
    while total > env.max_folder_bytes && sized.len() > 1 {
        let Some((side, bytes, _)) = sized.pop() else {
            break;
        };
        match fs::remove_dir_all(&side) {
            Ok(()) => {
                done.side_repos += 1;
                total = total.saturating_sub(bytes);
            }
            Err(error) => eprintln!("hangar: could not remove {}: {error}", side.display()),
        }
    }
    if total > env.max_folder_bytes {
        if let Some((side, _, _)) = sized.first() {
            match thin(env, side, deadline) {
                Ok(refs) => done.refs += refs,
                Err(error) => eprintln!(
                    "hangar: snapshot thinning of {} skipped: {error}",
                    side.display()
                ),
            }
        }
    }
}

/// Deletes the older half of every session's snapshots in `side`, then runs `gc`.
fn thin(env: &SnapshotEnv<'_>, side: &Path, deadline: Instant) -> Result<usize, SnapshotError> {
    let mut by_key: HashMap<String, Vec<(u64, String)>> = HashMap::new();
    for name in side_refs(env, side, REF_PREFIX, deadline)? {
        let Some(key) = key_of_ref(&name) else {
            continue;
        };
        let Some(number) = name
            .rsplit('/')
            .next()
            .and_then(|number| number.parse::<u64>().ok())
        else {
            continue;
        };
        by_key
            .entry(key.to_owned())
            .or_default()
            .push((number, name.clone()));
    }
    let mut dropped = Vec::new();
    for refs in by_key.values_mut() {
        refs.sort();
        let older = refs.len() / 2;
        dropped.extend(refs.drain(..older).map(|(_, name)| name));
    }
    delete_refs(env, side, &dropped, deadline)?;
    let source = fs::read_to_string(side.join(SOURCE_FILE)).map_err(data_error)?;
    gc(
        env,
        side,
        &PathBuf::from(source.trim_end()).join("objects"),
        deadline,
    );
    Ok(dropped.len())
}

/// Every running Hangar holds this lock on `<folder>/.lock` shared for its whole run, so the
/// clean-up at start, which deletes snapshots of sessions that are not in the saved workspace,
/// runs only in an instance that gets it exclusively: no other Hangar (an installed one next to a
/// debug build, both share the data folder) is using any snapshot. `<folder>/.gate`, held
/// exclusively for the few steps of [`InstanceLock::enter`], makes the change from exclusive to
/// shared one step for every other instance.
#[derive(Debug)]
pub struct InstanceLock {
    _file: fs::File,
}

impl InstanceLock {
    /// Takes the lock; `alone` runs first if no other Hangar holds it. The answer says whether
    /// `alone` ran.
    pub fn enter(folder: &Path, alone: impl FnOnce()) -> io::Result<(Self, bool)> {
        fs::create_dir_all(folder)?;
        let open = |name: &str| {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(folder.join(name))
        };
        let gate = open(GATE_FILE)?;
        gate.lock()?;
        let file = open(LOCK_FILE)?;
        let ran = match file.try_lock() {
            Ok(()) => {
                alone();
                file.unlock()?;
                file.lock_shared()?;
                true
            }
            Err(fs::TryLockError::WouldBlock) => {
                file.lock_shared()?;
                false
            }
            Err(fs::TryLockError::Error(error)) => return Err(error),
        };
        drop(gate);
        Ok((Self { _file: file }, ran))
    }
}

/// The session a key is bound to, and its folder (fixed for the session's life).
#[derive(Debug, Clone)]
struct Bound {
    key: String,
    cwd: PathBuf,
}

/// The snapshot calls, one at a time per session (two of the same session would race for the
/// next number; other sessions use their own refs and temporary index, so a slow snapshot of one
/// session never holds up another, nor the closing of another), why each session's newest step
/// was not saved, and which workspace key each session has. Windows name a session by its ID
/// only; the key comes from this binding, which only the main window sets.
#[derive(Debug, Default)]
pub struct Snapshots {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    skipped: Mutex<HashMap<String, Skipped>>,
    bound: Mutex<HashMap<SessionId, Bound>>,
    /// Held for writing by the clean-up at start, for reading by every other call.
    upkeep: RwLock<()>,
    /// One jump back at a time in this Hangar, whichever session: two sessions in the same work
    /// tree would otherwise write it at once, each with an undo point the other one changes.
    restoring: Mutex<()>,
    /// This Hangar's share of the folder's [`InstanceLock`], held for the whole run.
    instance: Mutex<Option<InstanceLock>>,
}

impl Snapshots {
    /// The lock of one session (`key` already checked). Entries stay: one per session of a run.
    fn session_lock(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(locks.entry(key.to_owned()).or_default())
    }

    fn skipped(&self) -> MutexGuard<'_, HashMap<String, Skipped>> {
        self.skipped.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn binding(&self) -> MutexGuard<'_, HashMap<SessionId, Bound>> {
        self.bound.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Binds the workspace `key` to session `id` in folder `cwd`. A session keeps its key; a key
    /// held by another session that is still `alive` is refused, one of a closed session moves.
    pub fn bind(
        &self,
        id: SessionId,
        key: &str,
        cwd: &Path,
        alive: &dyn Fn(SessionId) -> bool,
    ) -> Result<(), SnapshotError> {
        let key = check_key(key)?;
        let mut bound = self.binding();
        if let Some(existing) = bound.get(&id) {
            return if existing.key == key {
                Ok(())
            } else {
                Err(SnapshotError::InvalidKey)
            };
        }
        if bound
            .iter()
            .any(|(other, entry)| entry.key == key && alive(*other))
        {
            return Err(SnapshotError::InvalidKey);
        }
        bound.retain(|_, entry| entry.key != key);
        bound.insert(
            id,
            Bound {
                key,
                cwd: cwd.to_owned(),
            },
        );
        Ok(())
    }

    fn bound(&self, id: SessionId) -> Result<Bound, SnapshotError> {
        self.binding()
            .get(&id)
            .cloned()
            .ok_or(SnapshotError::UnboundSession(id))
    }

    fn upkeep(&self) -> std::sync::RwLockReadGuard<'_, ()> {
        self.upkeep.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// [`take`] for session `id`, remembering a skipped or failed step for the timeline.
    pub fn take(
        &self,
        env: &SnapshotEnv<'_>,
        id: SessionId,
        prompt: &str,
    ) -> Result<TakeOutcome, SnapshotError> {
        let Bound { key, cwd } = self.bound(id)?;
        let result = {
            let _upkeep = self.upkeep();
            let lock = self.session_lock(&key);
            let _busy = lock.lock().unwrap_or_else(PoisonError::into_inner);
            take(env, &cwd, &key, prompt)
        };
        let mut skipped = self.skipped();
        match &result {
            Ok(TakeOutcome::TooManyFiles { limit }) => {
                skipped.insert(key, Skipped::TooManyFiles { limit: *limit });
            }
            Ok(TakeOutcome::TooLarge {
                file_mib,
                total_mib,
            }) => {
                skipped.insert(
                    key,
                    Skipped::TooLarge {
                        file_mib: *file_mib,
                        total_mib: *total_mib,
                    },
                );
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

    /// The timeline of session `id`, with the other sessions in its work tree. A session without
    /// a key yet has an empty one.
    pub fn list(
        &self,
        env: &SnapshotEnv<'_>,
        id: SessionId,
        neighbours: &Neighbours,
    ) -> Result<SnapshotList, SnapshotError> {
        let Bound { key, cwd } = self.bound(id)?;
        let _upkeep = self.upkeep();
        let (mut answer, root) = list_in(env, &cwd, &key)?;
        if let Some(root) = root {
            answer.skipped = self.skipped().get(&key).cloned();
            (answer.others, answer.busy) = neighbours.in_tree(&root, id);
        }
        Ok(answer)
    }

    pub fn diff(
        &self,
        env: &SnapshotEnv<'_>,
        id: SessionId,
        number: u64,
    ) -> Result<GitDiff, SnapshotError> {
        let Bound { key, cwd } = self.bound(id)?;
        let _upkeep = self.upkeep();
        diff(env, &cwd, &key, number)
    }

    /// [`restore`] for session `id`; refused while a session in the same work tree works.
    /// `neighbours` is asked each time [`restore`] looks, so the last look is fresh.
    pub fn restore(
        &self,
        env: &SnapshotEnv<'_>,
        id: SessionId,
        number: u64,
        neighbours: &dyn Fn() -> Neighbours,
    ) -> Result<RestoreOutcome, SnapshotError> {
        let Bound { key, cwd } = self.bound(id)?;
        let _upkeep = self.upkeep();
        let _restoring = self
            .restoring
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let lock = self.session_lock(&key);
        let _busy = lock.lock().unwrap_or_else(PoisonError::into_inner);
        restore(env, &cwd, &key, number, &|root| {
            neighbours().in_tree(root, id).1
        })
    }

    /// Deletes the snapshots of session `id` and its binding (it was closed, or will not come
    /// back); a session without a binding has nothing to delete.
    pub fn forget(&self, env: &SnapshotEnv<'_>, id: SessionId) -> Result<usize, SnapshotError> {
        let Some(Bound { key, cwd }) = self.binding().remove(&id) else {
            return Ok(0);
        };
        self.skipped().remove(&key);
        let _upkeep = self.upkeep();
        let lock = self.session_lock(&key);
        let _busy = lock.lock().unwrap_or_else(PoisonError::into_inner);
        forget(env, &cwd, &key)
    }

    /// Joins the folder's [`InstanceLock`] and runs [`clean_up`] if no other Hangar holds it,
    /// with every other call waiting until it is done. `None`: another Hangar runs, nothing was
    /// cleaned up; also when the lock cannot be taken (then nothing is held either).
    pub fn clean_up(
        &self,
        env: &SnapshotEnv<'_>,
        keep: Option<&BTreeSet<String>>,
    ) -> Option<CleanUp> {
        let _upkeep = self.upkeep.write().unwrap_or_else(PoisonError::into_inner);
        let mut done = None;
        match InstanceLock::enter(env.folder, || done = Some(clean_up(env, keep))) {
            Ok((lock, _)) => {
                *self.instance.lock().unwrap_or_else(PoisonError::into_inner) = Some(lock);
            }
            Err(error) => eprintln!("hangar: snapshot clean-up skipped, no lock: {error}"),
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo, marker_filter_config};
    use crate::test_support::TempDir;

    const KEY: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const OTHER: &str = "11111111-2222-3333-4444-555555555555";

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn idle(_: &Path) -> bool {
        false
    }

    /// Every file below `dir` with its size, `/` separated relative paths.
    fn tree_listing(dir: &Path) -> Vec<(String, u64)> {
        let mut found = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(folder) = pending.pop() {
            for entry in fs::read_dir(&folder).unwrap().flatten() {
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    pending.push(entry.path());
                } else {
                    let relative = entry.path().strip_prefix(dir).unwrap().to_owned();
                    found.push((git::path_key(&relative), meta.len()));
                }
            }
        }
        found.sort();
        found
    }

    struct Fixture {
        tmp: TempDir,
        git: Git,
        repo: PathBuf,
        folder: PathBuf,
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
            let folder = tmp.path().join("data").join(FOLDER);
            Some(Self {
                tmp,
                git,
                repo,
                folder,
            })
        }

        fn env(&self) -> SnapshotEnv<'_> {
            SnapshotEnv::new(&self.git, &self.folder)
        }

        fn out(&self, args: &[&str]) -> String {
            let output = self
                .git
                .run(&self.repo, args, Instant::now() + git::TIMEOUT, 1 << 20)
                .unwrap();
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        /// The one side repository.
        fn side(&self) -> PathBuf {
            let sides: Vec<PathBuf> = fs::read_dir(&self.folder)
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension() == Some(OsStr::new("git")))
                .collect();
            assert_eq!(sides.len(), 1, "{sides:?}");
            sides[0].clone()
        }

        /// `git <args>` in the side repository with the user's objects as an alternate.
        fn side_out(&self, args: &[&str]) -> String {
            let objects = self.repo.join(".git").join("objects");
            let output = self
                .git
                .run_with_env(
                    &self.side(),
                    args,
                    &[("GIT_ALTERNATE_OBJECT_DIRECTORIES", objects.as_os_str())],
                    None,
                    Instant::now() + git::TIMEOUT,
                    1 << 20,
                )
                .unwrap();
            assert!(
                output.success,
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
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
        assert_eq!(key_of_ref(&format!("{REF_PREFIX}{KEY}/3")), Some(KEY));
        assert_eq!(key_of_ref("refs/heads/main"), None);
    }

    #[test]
    fn side_repositories_are_named_by_the_git_folder() {
        let a = side_name(Path::new("C:/work/repo/.git"));
        assert!(a.ends_with(".git") && a.len() == 36, "{a}");
        assert_ne!(a, side_name(Path::new("C:/work/other/.git")));
        if cfg!(windows) {
            assert_eq!(a, side_name(Path::new(r"c:\work\repo\.git")));
        }
        let separator = if cfg!(windows) { "a;b" } else { "a:b" };
        assert!(matches!(
            alternate(Path::new(separator)),
            Err(SnapshotError::UnusablePath)
        ));
        assert!(matches!(
            alternate(Path::new("\"quoted")),
            Err(SnapshotError::UnusablePath)
        ));
    }

    /// The user's repository is not written at all: its git folder holds the same files with the
    /// same sizes, so no object, ref, reflog or index changed. The snapshot (work tree with
    /// untracked files, ignored ones left out) lives in the side repository.
    #[test]
    fn nothing_is_written_into_the_users_repository() {
        let Some(fx) = Fixture::new("snapshot-untouched") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "one\nstaged\n");
        git_ok(&fx.git, &fx.repo, &["add", "a.txt"]);
        write(&fx.repo.join("a.txt"), "one\nstaged\nunstaged\n");
        write(&fx.repo.join("new folder/c.txt"), "three\n");
        write(&fx.repo.join("build.log"), "ignored\n");
        let git_folder = fx.repo.join(".git");
        let before = (
            tree_listing(&git_folder),
            fx.out(&["status", "--porcelain=v1"]),
            fx.out(&["diff", "--cached"]),
        );

        assert_eq!(
            fx.take("make the change"),
            TakeOutcome::Created {
                number: 1,
                files: 2
            }
        );
        write(&fx.repo.join("a.txt"), "back\n");
        assert!(matches!(
            restore(&fx.env(), &fx.repo, KEY, 1, &idle).unwrap(),
            RestoreOutcome::Restored { undo: 2, .. }
        ));

        let after = (
            tree_listing(&git_folder),
            fx.out(&["status", "--porcelain=v1"]),
            fx.out(&["diff", "--cached"]),
        );
        assert_eq!(before, after);
        assert_eq!(fx.out(&["for-each-ref", "refs/hangar/"]), "");
        let reference = format!("{REF_PREFIX}{KEY}/1");
        let files = fx.side_out(&["ls-tree", "-r", "--name-only", &reference]);
        let files: Vec<&str> = files.lines().collect();
        assert_eq!(files, [".gitignore", "a.txt", "b.txt", "new folder/c.txt"]);
        assert_eq!(
            fx.side_out(&["show", &format!("{reference}:a.txt")]),
            "one\nstaged\nunstaged\n"
        );
        // The parent is HEAD at that time, read through the alternate.
        assert_eq!(
            fx.side_out(&["rev-parse", &format!("{reference}^")]),
            fx.out(&["rev-parse", "HEAD"])
        );
        assert_eq!(read(&fx.repo.join("a.txt")), "one\nstaged\nunstaged\n");
        // The temporary indexes are gone.
        assert_eq!(
            fs::read_dir(fx.folder.join(TEMP_FOLDER)).unwrap().count(),
            0
        );

        let listed = list(&fx.env(), &fx.repo, KEY).unwrap();
        assert!(listed.is_repo);
        let first = listed.snapshots.last().unwrap();
        assert_eq!(
            (first.number, first.files, first.kind, first.prompt.as_str()),
            (1, 2, SnapshotKind::Step, "make the change")
        );
        assert!(first.created_at > 1_600_000_000);
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
        write(&fx.repo.join("a.txt"), "other\n");
        take(&env, &fx.repo, OTHER, "").unwrap();
        assert_eq!(forget(&env, &fx.repo, KEY).unwrap(), 3);
        assert!(fx.numbers().is_empty());
        assert_eq!(list(&env, &fx.repo, OTHER).unwrap().snapshots.len(), 1);
        assert_eq!(forget(&env, &fx.repo, KEY).unwrap(), 0);
    }

    #[test]
    fn a_jump_back_to_the_oldest_snapshot_keeps_it_in_the_list() {
        let Some(fx) = Fixture::new("snapshot-prune-target") else {
            return;
        };
        let env = SnapshotEnv {
            max_snapshots: 3,
            ..fx.env()
        };
        for step in 1..=3 {
            write(&fx.repo.join("a.txt"), &format!("step {step}\n"));
            take(&env, &fx.repo, KEY, "").unwrap();
        }
        write(&fx.repo.join("a.txt"), "unsaved\n");
        assert!(matches!(
            restore(&env, &fx.repo, KEY, 1, &idle).unwrap(),
            RestoreOutcome::Restored { undo: 4, .. }
        ));
        // Step 2 makes room for the undo point; the target stays where the work tree now is.
        assert_eq!(fx.numbers(), [4, 3, 1]);
        assert_eq!(read(&fx.repo.join("a.txt")), "step 1\n");
    }

    #[test]
    fn too_many_or_too_large_changes_are_not_saved() {
        let Some(fx) = Fixture::new("snapshot-many") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "saved\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        let few = SnapshotEnv {
            max_files: 2,
            ..fx.env()
        };
        for name in ["x/1.txt", "x/2.txt", "x/3.txt"] {
            write(&fx.repo.join(name), "generated\n");
        }
        assert_eq!(
            take(&few, &fx.repo, KEY, "").unwrap(),
            TakeOutcome::TooManyFiles { limit: 2 }
        );
        assert_eq!(fx.numbers(), [1]);
        // A jump back is refused before anything is written: its undo point cannot be saved.
        assert!(matches!(
            restore(&few, &fx.repo, KEY, 1, &idle),
            Err(SnapshotError::CannotSaveCurrent(_))
        ));
        assert!(fx.repo.join("x/3.txt").exists());

        // One file above the per file limit, or all together above the total.
        let small_file = SnapshotEnv {
            max_file_bytes: 15,
            ..fx.env()
        };
        write(&fx.repo.join("big.bin"), "sixteen bytes!!\n");
        assert_eq!(
            take(&small_file, &fx.repo, KEY, "").unwrap(),
            TakeOutcome::TooLarge {
                file_mib: 0,
                total_mib: MAX_TOTAL_BYTES / MIB
            }
        );
        let small_total = SnapshotEnv {
            max_total_bytes: 40,
            ..fx.env()
        };
        assert!(matches!(
            take(&small_total, &fx.repo, KEY, "").unwrap(),
            TakeOutcome::TooLarge { .. }
        ));
        assert_eq!(fx.numbers(), [1]);
        assert!(matches!(
            take(&fx.env(), &fx.repo, KEY, "").unwrap(),
            TakeOutcome::Created { number: 2, .. }
        ));
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

        // Refused while a session works in the work tree; nothing saved, nothing written.
        assert!(matches!(
            restore(&fx.env(), &fx.repo, KEY, 1, &|_| true),
            Err(SnapshotError::Working)
        ));
        assert_eq!(fx.numbers(), [2, 1]);

        let outcome = restore(&fx.env(), &fx.repo, KEY, 1, &idle).unwrap();
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
        let back = restore(&fx.env(), &fx.repo, KEY, 3, &idle).unwrap();
        assert!(
            matches!(back, RestoreOutcome::Restored { undo: 4, .. }),
            "{back:?}"
        );
        assert_eq!(read(&fx.repo.join("a.txt")), "third\n");
        assert!(!fx.repo.join("b.txt").exists());
        assert_eq!(read(&fx.repo.join("deep/new/n.txt")), "new\n");
        assert_eq!(
            restore(&fx.env(), &fx.repo, KEY, 3, &idle).unwrap(),
            RestoreOutcome::AlreadyThere
        );
        assert!(matches!(
            restore(&fx.env(), &fx.repo, KEY, 99, &idle),
            Err(SnapshotError::UnknownSnapshot)
        ));
    }

    /// A folder with other files where the snapshot has a file: refused before the undo point is
    /// saved, and the folder stays. A folder that only holds files going away is no obstacle.
    #[test]
    fn restore_refuses_a_folder_in_the_way() {
        let Some(fx) = Fixture::new("snapshot-folder") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        // `b.txt` becomes a folder: one tracked-to-be file in it, and an ignored one.
        fs::remove_file(fx.repo.join("b.txt")).unwrap();
        write(&fx.repo.join("b.txt/inner.txt"), "new\n");
        write(&fx.repo.join("b.txt/keep.log"), "ignored\n");
        let result = restore(&fx.env(), &fx.repo, KEY, 1, &idle);
        assert!(
            matches!(&result, Err(SnapshotError::FolderInTheWay(path)) if path == "b.txt"),
            "{result:?}"
        );
        assert_eq!(fx.numbers(), [1]);
        assert_eq!(read(&fx.repo.join("b.txt/keep.log")), "ignored\n");

        // Without the ignored file the folder only holds what goes: the jump works.
        fs::remove_file(fx.repo.join("b.txt/keep.log")).unwrap();
        let outcome = restore(&fx.env(), &fx.repo, KEY, 1, &idle).unwrap();
        assert!(
            matches!(outcome, RestoreOutcome::Restored { .. }),
            "{outcome:?}"
        );
        assert_eq!(read(&fx.repo.join("b.txt")), "two\n");
    }

    /// An ignored file is in no snapshot, so it must never be overwritten or deleted: not where
    /// the snapshot has a file (it was not ignored back then), and not where the snapshot needs a
    /// folder. Refused before the undo point is saved, and the ignored file keeps its content.
    #[test]
    fn restore_refuses_to_overwrite_an_ignored_file() {
        let Some(fx) = Fixture::new("snapshot-ignored") else {
            return;
        };
        write(&fx.repo.join("notes.txt"), "back then\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        write(&fx.repo.join(".gitignore"), "*.log\nnotes.txt\n");
        write(&fx.repo.join("notes.txt"), "precious\n");
        let result = restore(&fx.env(), &fx.repo, KEY, 1, &idle);
        assert!(
            matches!(&result, Err(SnapshotError::IgnoredInTheWay(path)) if path == "notes.txt"),
            "{result:?}"
        );
        assert_eq!(read(&fx.repo.join("notes.txt")), "precious\n");
        assert_eq!(fx.numbers(), [1]);

        // The snapshot has `deep/x.txt`; now an ignored file named `deep` stands there.
        write(&fx.repo.join(".gitignore"), "*.log\n");
        fs::remove_file(fx.repo.join("notes.txt")).unwrap();
        write(&fx.repo.join("deep/x.txt"), "inside\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 2, .. }
        ));
        fs::remove_dir_all(fx.repo.join("deep")).unwrap();
        write(&fx.repo.join(".gitignore"), "*.log\ndeep\n");
        write(&fx.repo.join("deep"), "precious\n");
        let result = restore(&fx.env(), &fx.repo, KEY, 2, &idle);
        assert!(
            matches!(&result, Err(SnapshotError::IgnoredInTheWay(path)) if path == "deep"),
            "{result:?}"
        );
        assert_eq!(read(&fx.repo.join("deep")), "precious\n");
        assert_eq!(fx.numbers(), [2, 1]);

        // Without the ignored file the jump works.
        fs::remove_file(fx.repo.join("deep")).unwrap();
        write(&fx.repo.join(".gitignore"), "*.log\n");
        let outcome = restore(&fx.env(), &fx.repo, KEY, 2, &idle).unwrap();
        assert!(
            matches!(outcome, RestoreOutcome::Restored { .. }),
            "{outcome:?}"
        );
        assert_eq!(read(&fx.repo.join("deep/x.txt")), "inside\n");

        // A file renamed only in case is deleted first, then written: no obstacle, also on a
        // file system that ignores case.
        write(&fx.repo.join("case.txt"), "lower\n");
        let taken = fx.take("");
        let TakeOutcome::Created { number, .. } = taken else {
            panic!("{taken:?}");
        };
        fs::rename(fx.repo.join("case.txt"), fx.repo.join("CASE.txt")).unwrap();
        write(&fx.repo.join("CASE.txt"), "upper\n");
        let outcome = restore(&fx.env(), &fx.repo, KEY, number, &idle).unwrap();
        assert!(
            matches!(outcome, RestoreOutcome::Restored { .. }),
            "{outcome:?}"
        );
        assert_eq!(read(&fx.repo.join("case.txt")), "lower\n");
    }

    /// A file the index marks skip-worktree stays as it is; a file that needs a filter driver
    /// refuses the jump.
    #[test]
    fn restore_skips_sparse_entries_and_refuses_filtered_files() {
        let Some(fx) = Fixture::new("snapshot-sparse") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "step\n");
        write(&fx.repo.join("b.txt"), "step\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        git_ok(&fx.git, &fx.repo, &["checkout", "--", "b.txt"]);
        git_ok(
            &fx.git,
            &fx.repo,
            &["update-index", "--skip-worktree", "b.txt"],
        );
        fs::remove_file(fx.repo.join("b.txt")).unwrap();
        write(&fx.repo.join("a.txt"), "later\n");
        let outcome = restore(&fx.env(), &fx.repo, KEY, 1, &idle).unwrap();
        assert_eq!(outcome, RestoreOutcome::Restored { files: 1, undo: 2 });
        assert_eq!(read(&fx.repo.join("a.txt")), "step\n");
        assert!(!fx.repo.join("b.txt").exists());

        // A filter driver for `*.txt`: writing without it would put the wrong bytes there.
        let marker = fx.tmp.path().join("filter-ran");
        let config = fx.repo.join(".git/config");
        let mut text = read(&config);
        text.push_str(&marker_filter_config(&marker));
        fs::write(&config, text).unwrap();
        write(&fx.repo.join(".gitattributes"), "a.txt filter=marker\n");
        write(&fx.repo.join("a.txt"), "filtered\n");
        let result = restore(&fx.env(), &fx.repo, KEY, 1, &idle);
        assert!(
            matches!(result, Err(SnapshotError::Action(ActionError::Filtered))),
            "{result:?}"
        );
        assert_eq!(read(&fx.repo.join("a.txt")), "filtered\n");
        assert!(!marker.exists());
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
        let result = restore(&fx.env(), &fx.repo, KEY, 1, &idle);
        assert!(
            matches!(result, Err(SnapshotError::Action(ActionError::Linked(_)))),
            "{result:?}"
        );
        assert_eq!(read(&outside.join("c.txt")), "must survive\n");
        // Refused before the undo point: the timeline is unchanged.
        assert_eq!(fx.numbers(), [1]);
    }

    #[test]
    fn neighbours_in_the_same_work_tree() {
        let tmp = TempDir::new("snapshot-neighbours");
        let root = tmp.path().join("repo");
        let other = tmp.path().join("repo-other");
        for folder in [root.join("sub"), other.clone()] {
            fs::create_dir_all(folder).unwrap();
        }
        let neighbours = Neighbours {
            sessions: vec![(1, root.clone()), (2, root.join("sub")), (3, other.clone())],
            working: BTreeSet::from([3]),
        };
        assert_eq!(neighbours.in_tree(&root, 1), (1, false));
        assert_eq!(neighbours.in_tree(&other, 3), (0, true));
        let working = Neighbours {
            working: BTreeSet::from([2]),
            ..neighbours
        };
        assert_eq!(working.in_tree(&root, 1), (1, true));
    }

    /// The service names a session by its ID only: the key is bound once, a session keeps it,
    /// a key of a live session cannot be taken, and forgetting ends the binding.
    #[test]
    fn the_service_binds_keys_and_remembers_a_skipped_step() {
        let Some(fx) = Fixture::new("snapshot-service") else {
            return;
        };
        let service = Snapshots::default();
        let alive = |_: SessionId| true;
        assert!(matches!(
            service.take(&fx.env(), 1, ""),
            Err(SnapshotError::UnboundSession(1))
        ));
        assert!(matches!(
            service.bind(1, "not a key", &fx.repo, &alive),
            Err(SnapshotError::InvalidKey)
        ));
        service
            .bind(1, &KEY.to_uppercase(), &fx.repo, &alive)
            .unwrap();
        service.bind(1, KEY, &fx.repo, &alive).unwrap();
        assert!(service.bind(1, OTHER, &fx.repo, &alive).is_err());
        assert!(service.bind(2, KEY, &fx.repo, &alive).is_err());

        let small = SnapshotEnv {
            max_files: 1,
            ..fx.env()
        };
        write(&fx.repo.join("x/1.txt"), "generated\n");
        write(&fx.repo.join("x/2.txt"), "generated\n");
        assert_eq!(
            service.take(&small, 1, "").unwrap(),
            TakeOutcome::TooManyFiles { limit: 1 }
        );
        let neighbours = Neighbours {
            sessions: vec![(1, fx.repo.clone()), (2, fx.repo.join("x"))],
            working: BTreeSet::from([2]),
        };
        let listed = service.list(&fx.env(), 1, &neighbours).unwrap();
        assert_eq!(listed.skipped, Some(Skipped::TooManyFiles { limit: 1 }));
        assert_eq!((listed.others, listed.busy), (1, true));
        assert!(matches!(
            service.take(&fx.env(), 1, "").unwrap(),
            TakeOutcome::Created { number: 1, .. }
        ));
        assert_eq!(
            service
                .list(&fx.env(), 1, &Neighbours::default())
                .unwrap()
                .skipped,
            None
        );
        assert!(matches!(
            service.restore(&fx.env(), 1, 1, &|| neighbours.clone()),
            Err(SnapshotError::Working)
        ));
        assert_eq!(service.forget(&fx.env(), 1).unwrap(), 1);
        assert_eq!(service.forget(&fx.env(), 1).unwrap(), 0);
        // A closed session's key moves to the session that restores it.
        service.bind(1, KEY, &fx.repo, &alive).unwrap();
        service.bind(5, KEY, &fx.repo, &|id| id != 1).unwrap();
        assert!(matches!(
            service.diff(&fx.env(), 1, 1),
            Err(SnapshotError::UnboundSession(1))
        ));

        // The JSON shape the window reads.
        let json = serde_json::to_value(TakeOutcome::TooManyFiles { limit: 1 }).unwrap();
        assert_eq!(json["kind"], "tooManyFiles");
        let json = serde_json::to_value(Skipped::TooLarge {
            file_mib: 50,
            total_mib: 200,
        })
        .unwrap();
        assert_eq!(json["reason"], "tooLarge");
        assert_eq!(json["fileMib"], 50);
        assert_eq!(json["totalMib"], 200);
        let json = serde_json::to_value(RestoreOutcome::Incomplete {
            undo: 2,
            message: "x".to_owned(),
        })
        .unwrap();
        assert_eq!(json["kind"], "incomplete");
        assert_eq!(json["undo"], 2);
        let json = serde_json::to_value(&listed).unwrap();
        assert_eq!(json["others"], 1);
        assert_eq!(json["busy"], true);
    }

    /// A session whose snapshot is still running holds up neither another session's snapshot nor
    /// its closing (`forget`).
    #[test]
    fn one_session_does_not_wait_for_another() {
        let Some(fx) = Fixture::new("snapshot-locks") else {
            return;
        };
        let service = Snapshots::default();
        let alive = |_: SessionId| true;
        service.bind(1, KEY, &fx.repo, &alive).unwrap();
        service.bind(2, OTHER, &fx.repo, &alive).unwrap();
        let lock = service.session_lock(KEY);
        let _held = lock.lock().unwrap();
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(
            service.take(&fx.env(), 2, "").unwrap(),
            TakeOutcome::Created { number: 1, .. }
        ));
        assert_eq!(service.forget(&fx.env(), 2).unwrap(), 1);
    }

    /// Two jumps back in one Hangar never run at once, not even for two sessions (their keys
    /// differ, so the session locks alone would let both in): the second waits for the first.
    #[test]
    fn jumps_back_run_one_at_a_time() {
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        let Some(fx) = Fixture::new("snapshot-restoring") else {
            return;
        };
        let service = Snapshots::default();
        let alive = |_: SessionId| true;
        service.bind(1, KEY, &fx.repo, &alive).unwrap();
        service.bind(2, OTHER, &fx.repo, &alive).unwrap();
        let (entered, first_inside) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let second_inside = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let (service, fx, second_inside) = (&service, &fx, &second_inside);
            // The first look of a jump back asks for the neighbours; the first one waits there.
            let first = scope.spawn(move || {
                service.restore(&fx.env(), 1, 1, &|| {
                    entered.send(()).unwrap();
                    // Bounded, so a failed assertion below ends the test instead of hanging it.
                    let _ = released.recv_timeout(Duration::from_secs(5));
                    Neighbours::default()
                })
            });
            first_inside.recv().unwrap();
            let second = scope.spawn(move || {
                service.restore(&fx.env(), 2, 1, &|| {
                    second_inside.store(true, Ordering::SeqCst);
                    Neighbours::default()
                })
            });
            std::thread::sleep(Duration::from_millis(300));
            assert!(!second_inside.load(Ordering::SeqCst));
            release.send(()).unwrap();
            // Neither session has a snapshot 1; what counts is that both ran, one after the other.
            assert!(matches!(
                first.join().unwrap(),
                Err(SnapshotError::UnknownSnapshot)
            ));
            assert!(matches!(
                second.join().unwrap(),
                Err(SnapshotError::UnknownSnapshot)
            ));
        });
        assert!(second_inside.load(Ordering::SeqCst));
    }

    /// Writing that fails part way leaves the deletes and the files written before it done and
    /// reports the error; `restore` answers `Incomplete` with the undo point saved before.
    #[test]
    fn a_failed_write_stops_and_names_the_file() {
        let Some(fx) = Fixture::new("snapshot-partial") else {
            return;
        };
        let env = fx.env();
        let repo = Repo::open(&env, &fx.repo).unwrap().unwrap();
        let staging = Staging::new(&env.temp_dir()).unwrap();
        write(&staging.0.join("a.txt"), "from the snapshot\n");
        // `c.txt` is missing in the staging folder, so its copy fails.
        let written = ["a.txt".to_owned(), "c.txt".to_owned()];
        let added = HashSet::from(["c.txt"]);
        let result = write_back(&repo, &["b.txt"], &written, &added, &staging);
        assert!(
            matches!(&result, Err(SnapshotError::Git(GitError::Failed(text))) if text.starts_with("c.txt ")),
            "{result:?}"
        );
        assert!(!fx.repo.join("b.txt").exists());
        assert_eq!(read(&fx.repo.join("a.txt")), "from the snapshot\n");
        assert!(!fx.repo.join("c.txt").exists());
    }

    /// At start: stale temporary indexes go, the snapshots of sessions that do not come back go,
    /// a side repository without snapshots or without its repository goes as a whole.
    #[test]
    fn clean_up_at_start() {
        let Some(fx) = Fixture::new("snapshot-clean") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(fx.take(""), TakeOutcome::Created { .. }));
        take(&fx.env(), &fx.repo, OTHER, "").unwrap();
        write(&fx.repo.join("a.txt"), "again\n");
        take(&fx.env(), &fx.repo, OTHER, "").unwrap();
        let temp = fx.folder.join(TEMP_FOLDER);
        let fresh = temp.join("1-1.index");
        let stale = temp.join("1-2.index");
        write(&fresh, "x");
        write(&stale, "x");
        let old = SystemTime::now() - Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();

        // Unknown workspace: only the leftovers go.
        let done = clean_up(&fx.env(), None);
        assert_eq!(done.temp_files, 1);
        assert!(fresh.exists() && !stale.exists());
        assert_eq!(fx.numbers(), [1]);

        let keep = BTreeSet::from([KEY.to_owned()]);
        let done = clean_up(&fx.env(), Some(&keep));
        assert_eq!(done.refs, 2);
        assert_eq!(fx.numbers(), [1]);
        assert!(list(&fx.env(), &fx.repo, OTHER)
            .unwrap()
            .snapshots
            .is_empty());

        let done = clean_up(&fx.env(), Some(&BTreeSet::new()));
        assert_eq!(done.side_repos, 1);
        assert!(fs::read_dir(&fx.folder)
            .unwrap()
            .flatten()
            .all(|entry| entry.path().extension() != Some(OsStr::new("git"))));

        // A side repository whose repository is gone.
        assert!(matches!(fx.take(""), TakeOutcome::Created { .. }));
        let side = fx.side();
        fs::write(
            side.join(SOURCE_FILE),
            fx.tmp.path().join("gone").to_string_lossy().as_bytes(),
        )
        .unwrap();
        let done = clean_up(&fx.env(), Some(&keep));
        // Gone for sure only where its drive can be seen (a local drive letter); on Unix a side
        // repository with fresh snapshots stays (`a_missing_drive_keeps_its_snapshots`).
        if cfg!(windows) {
            assert_eq!(done.side_repos, 1);
            assert!(!side.exists());
        } else {
            assert_eq!(done.side_repos, 0);
            assert!(side.exists());
        }
    }

    /// A link (junction on Windows) at `link` to the folder `target`.
    fn link_folder(link: &Path, target: &Path) {
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap()
                .status;
            assert!(status.success());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    /// The writing step checks each folder right before the file goes in, not only during the
    /// checks before: a folder swapped for a link since, a path into the git folder, a short
    /// name that names another file. Nothing outside the work tree is written.
    #[test]
    fn install_checks_every_folder_right_before_writing() {
        let Some(fx) = Fixture::new("snapshot-install") else {
            return;
        };
        let env = fx.env();
        let repo = Repo::open(&env, &fx.repo).unwrap().unwrap();
        let staged = fx.tmp.path().join("staged.txt");
        write(&staged, "from the snapshot\n");

        // A plain new file in a new folder goes in; a changed one is replaced.
        install(&repo, "new/deep/n.txt", &staged).unwrap();
        assert_eq!(read(&fx.repo.join("new/deep/n.txt")), "from the snapshot\n");
        install(&repo, "a.txt", &staged).unwrap();
        assert_eq!(read(&fx.repo.join("a.txt")), "from the snapshot\n");
        // No temporary file is left beside them.
        let left: Vec<String> = fs::read_dir(&fx.repo)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".hangar-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");

        // A folder that became a link after the checks.
        let outside = fx.tmp.path().join("outside");
        write(&outside.join("c.txt"), "must survive\n");
        link_folder(&fx.repo.join("sub"), &outside);
        let result = install(&repo, "sub/c.txt", &staged);
        assert!(
            matches!(result, Err(SnapshotError::Action(ActionError::Linked(_)))),
            "{result:?}"
        );
        assert_eq!(read(&outside.join("c.txt")), "must survive\n");

        // Into the git folder: refused by its canonical path, whatever spelled the name.
        let result = install(&repo, ".git/hangar-test", &staged);
        assert!(
            matches!(&result, Err(SnapshotError::Outside(path)) if path == ".git/hangar-test"),
            "{result:?}"
        );
        assert!(!fx.repo.join(".git/hangar-test").exists());
        // An NTFS short name of the git folder, where the volume makes short names.
        if fx.repo.join("GIT~1").is_dir() {
            let result = install(&repo, "GIT~1/hangar-test", &staged);
            assert!(
                matches!(result, Err(SnapshotError::Outside(_))),
                "{result:?}"
            );
        }
        // A folder where the file goes is never replaced.
        fs::create_dir_all(fx.repo.join("folder/inner")).unwrap();
        assert!(matches!(
            install(&repo, "folder", &staged),
            Err(SnapshotError::FolderInTheWay(_))
        ));
        assert!(fx.repo.join("folder/inner").is_dir());
    }

    /// Line endings come out as git would write them in the work tree: staged with the work
    /// tree's attributes, then moved in.
    #[test]
    fn restore_writes_line_endings_as_git_does() {
        let Some(fx) = Fixture::new("snapshot-eol") else {
            return;
        };
        write(&fx.repo.join(".gitattributes"), "*.txt text eol=crlf\n");
        write(&fx.repo.join("a.txt"), "one\r\ntwo\r\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        write(&fx.repo.join("a.txt"), "later\r\n");
        let outcome = restore(&fx.env(), &fx.repo, KEY, 1, &idle).unwrap();
        assert!(
            matches!(outcome, RestoreOutcome::Restored { .. }),
            "{outcome:?}"
        );
        assert_eq!(read(&fx.repo.join("a.txt")), "one\r\ntwo\r\n");
        // The staging folder is gone.
        let left: Vec<PathBuf> = fs::read_dir(fx.folder.join(TEMP_FOLDER))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    /// A file saved while the jump ran its checks, or an agent that started meanwhile: the jump
    /// stops before the first write, the file keeps its newer content.
    #[test]
    fn restore_stops_when_the_work_tree_changes_meanwhile() {
        let Some(fx) = Fixture::new("snapshot-meanwhile") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "step\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        write(&fx.repo.join("a.txt"), "current\n");

        // The second look (right before writing) finds a change saved after the first.
        let looks = std::cell::Cell::new(0);
        let saves_meanwhile = |_: &Path| {
            looks.set(looks.get() + 1);
            if looks.get() == 2 {
                write(&fx.repo.join("b.txt"), "saved meanwhile\n");
            }
            false
        };
        let result = restore(&fx.env(), &fx.repo, KEY, 1, &saves_meanwhile);
        assert!(matches!(result, Err(SnapshotError::Working)), "{result:?}");
        assert_eq!(looks.get(), 2);
        assert_eq!(read(&fx.repo.join("a.txt")), "current\n");
        assert_eq!(read(&fx.repo.join("b.txt")), "saved meanwhile\n");

        // An agent that started meanwhile.
        let looks = std::cell::Cell::new(0);
        let starts_meanwhile = |_: &Path| {
            looks.set(looks.get() + 1);
            looks.get() == 2
        };
        let result = restore(&fx.env(), &fx.repo, KEY, 1, &starts_meanwhile);
        assert!(matches!(result, Err(SnapshotError::Working)), "{result:?}");
        assert_eq!(read(&fx.repo.join("a.txt")), "current\n");
    }

    /// An entry marked assume-unchanged keeps its newer content in the snapshot, so a jump back
    /// can return to it.
    #[test]
    fn assume_unchanged_entries_are_saved_with_their_content() {
        let Some(fx) = Fixture::new("snapshot-assume") else {
            return;
        };
        git_ok(
            &fx.git,
            &fx.repo,
            &["update-index", "--assume-unchanged", "a.txt"],
        );
        write(&fx.repo.join("a.txt"), "hidden change\n");
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created {
                number: 1,
                files: 1
            }
        ));
        let reference = format!("{REF_PREFIX}{KEY}/1");
        assert_eq!(
            fx.side_out(&["show", &format!("{reference}:a.txt")]),
            "hidden change\n"
        );
        // The user's index keeps its bit.
        assert!(fx.out(&["ls-files", "-v", "a.txt"]).starts_with("h "));
        assert_eq!(assume_unchanged(b"h a.txt\0H b.txt\0S c\0"), b"a.txt\0");
    }

    /// A symbolic link in the snapshot is not written: its target could lie anywhere.
    #[test]
    fn restore_refuses_a_symbolic_link_in_the_snapshot() {
        let Some(fx) = Fixture::new("snapshot-symlink") else {
            return;
        };
        // With `core.symlinks=false` git keeps the link as a file with its target, mode 120000.
        git_ok(&fx.git, &fx.repo, &["config", "core.symlinks", "false"]);
        write(&fx.repo.join("link"), "../outside");
        let blob = fx.out(&["hash-object", "-w", "link"]);
        let info = format!("120000,{},link", blob.trim());
        git_ok(
            &fx.git,
            &fx.repo,
            &["update-index", "--add", "--cacheinfo", &info],
        );
        assert!(matches!(
            fx.take(""),
            TakeOutcome::Created { number: 1, .. }
        ));
        fs::remove_file(fx.repo.join("link")).unwrap();
        let result = restore(&fx.env(), &fx.repo, KEY, 1, &idle);
        assert!(
            matches!(&result, Err(SnapshotError::SymlinkEntry(path)) if path == "link"),
            "{result:?}"
        );
        assert!(!fx.repo.join("link").exists());
        assert_eq!(fx.numbers(), [1]);

        let raw = b":100644 120000 aaaa bbbb T\0link\0:100644 000000 cccc 0000 D\0gone\0";
        let changes = parse_raw(raw);
        assert_eq!(changes.len(), 2);
        assert_eq!(
            (changes[0].new_mode.as_str(), changes[0].letter),
            ("120000", b'T')
        );
        assert_eq!((changes[1].new_id.as_str(), changes[1].letter), ("", b'D'));
    }

    /// The size check runs on disk before `add` and on the stored blobs after it, so a file that
    /// grew in between is caught too.
    #[test]
    fn the_stored_blobs_are_checked_for_size() {
        let Some(fx) = Fixture::new("snapshot-blob-size") else {
            return;
        };
        write(&fx.repo.join("grown.bin"), "twenty bytes long!!\n");
        let env = fx.env();
        let repo = Repo::open(&env, &fx.repo).unwrap().unwrap();
        let tree = work_tree_tree(&env, &repo).unwrap();
        assert!(!tree_too_large(&env, &repo, &tree).unwrap());
        let small = SnapshotEnv {
            max_file_bytes: 19,
            ..fx.env()
        };
        assert!(tree_too_large(&small, &repo, &tree).unwrap());
    }

    #[test]
    fn checkouts_protect_git_folders_on_every_file_system() {
        let Some(fx) = Fixture::new("snapshot-protect") else {
            return;
        };
        let env = fx.env();
        let repo = Repo::open(&env, &fx.repo).unwrap().unwrap();
        for setting in [
            "core.protectNTFS=true",
            "core.protectHFS=true",
            "core.ignoreStat=false",
        ] {
            assert!(repo.overrides.iter().any(|arg| arg == setting), "{setting}");
        }
    }

    /// The refusals reach the window with a stable code and the path after a newline.
    #[test]
    fn refusals_are_serialized_with_a_code() {
        let json = |error: SnapshotError| serde_json::to_value(error).unwrap();
        assert_eq!(
            json(SnapshotError::Working),
            "[snapshot_working] A session in this folder is working; go back once it is done"
        );
        let text = json(SnapshotError::FolderInTheWay("a\nb".to_owned()));
        let text = text.as_str().unwrap();
        let (first, detail) = text.split_once('\n').unwrap();
        assert!(first.starts_with("[snapshot_folder_in_the_way] a b is a folder"));
        assert_eq!(detail, "a\nb");
        assert!(
            json(SnapshotError::Action(ActionError::Linked("x".to_owned())))
                .as_str()
                .unwrap()
                .starts_with("[snapshot_linked] ")
        );
        assert!(json(SnapshotError::Action(ActionError::Filtered))
            .as_str()
            .unwrap()
            .starts_with("[snapshot_filtered] "));
        assert_eq!(
            json(SnapshotError::UnknownSnapshot),
            "This snapshot no longer exists"
        );
    }

    /// The list names the work tree, the current HEAD, and each snapshot's parent, so the window
    /// can warn when a branch was switched since.
    #[test]
    fn the_list_names_root_head_and_parents() {
        let Some(fx) = Fixture::new("snapshot-head") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(fx.take(""), TakeOutcome::Created { .. }));
        let head = fx.out(&["rev-parse", "HEAD"]).trim().to_owned();
        let listed = list(&fx.env(), &fx.repo, KEY).unwrap();
        assert_eq!(listed.head.as_deref(), Some(head.as_str()));
        assert_eq!(listed.snapshots[0].parent.as_deref(), Some(head.as_str()));
        assert_eq!(
            fs::canonicalize(&listed.root).unwrap(),
            fs::canonicalize(&fx.repo).unwrap()
        );
        git_ok(&fx.git, &fx.repo, &["commit", "--quiet", "-am", "second"]);
        let listed = list(&fx.env(), &fx.repo, KEY).unwrap();
        assert_ne!(listed.head, listed.snapshots[0].parent);
    }

    /// Only one running Hangar cleans up: a second one holding the lock shared keeps the first
    /// from deleting snapshots it may be using.
    #[test]
    fn clean_up_runs_only_without_another_instance() {
        let Some(fx) = Fixture::new("snapshot-instances") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(fx.take(""), TakeOutcome::Created { .. }));
        let (other, ran) = InstanceLock::enter(&fx.folder, || {}).unwrap();
        assert!(ran);
        let (second, ran) = InstanceLock::enter(&fx.folder, || panic!("not alone")).unwrap();
        assert!(!ran);

        // The other instance runs: nothing goes, although no key is kept.
        let service = Snapshots::default();
        assert_eq!(service.clean_up(&fx.env(), Some(&BTreeSet::new())), None);
        assert_eq!(fx.numbers(), [1]);
        drop(other);
        drop(second);
        drop(service);

        let service = Snapshots::default();
        let done = service.clean_up(&fx.env(), Some(&BTreeSet::new())).unwrap();
        assert_eq!(done.side_repos, 1);
        // This one now holds it shared: a later start does not clean up.
        let (_third, ran) = InstanceLock::enter(&fx.folder, || {}).unwrap();
        assert!(!ran);
    }

    /// The clean-up packs the side repository without copying the user's objects into it, and
    /// keeps all side repositories together below the cap.
    #[test]
    fn clean_up_runs_gc_and_keeps_the_folder_below_its_cap() {
        let Some(fx) = Fixture::new("snapshot-cap") else {
            return;
        };
        let keep = BTreeSet::from([KEY.to_owned()]);
        for step in 1..=4 {
            write(&fx.repo.join("a.txt"), &format!("step {step}\n"));
            fx.take("");
        }
        let done = clean_up(&fx.env(), Some(&keep));
        assert_eq!(done, CleanUp::default());
        let side = fx.side();
        // Packed: no loose snapshot objects left.
        let loose = fx.side_out(&["count-objects", "-v"]);
        assert!(loose.contains("count: 0\n"), "{loose}");
        // The unchanged `b.txt` stays in the user's objects only.
        let blob = fx.out(&["rev-parse", "HEAD:b.txt"]);
        let alone = fx
            .git
            .run(
                &side,
                ["cat-file", "-e", blob.trim()],
                Instant::now() + git::TIMEOUT,
                1 << 10,
            )
            .unwrap();
        assert!(!alone.success);

        // Over the cap with one side repository: the older half of each session's snapshots go.
        let tiny = SnapshotEnv {
            max_folder_bytes: 1,
            ..fx.env()
        };
        let done = clean_up(&tiny, Some(&keep));
        assert_eq!(done.refs, 2);
        assert_eq!(fx.numbers(), [4, 3]);

        // Two side repositories: the one with the older newest snapshot goes as a whole.
        let second = fx.tmp.path().join("second");
        init_repo(&fx.git, &second);
        write(&second.join("x.txt"), "x\n");
        std::thread::sleep(Duration::from_millis(1100));
        assert!(matches!(
            take(&fx.env(), &second, OTHER, "").unwrap(),
            TakeOutcome::Created { .. }
        ));
        let both = BTreeSet::from([KEY.to_owned(), OTHER.to_owned()]);
        let done = clean_up(&tiny, Some(&both));
        assert_eq!(done.side_repos, 1);
        assert!(!side.exists());
        assert_eq!(list(&fx.env(), &second, OTHER).unwrap().snapshots.len(), 1);
    }

    /// A repository on a drive that is not there (unplugged, not mapped) keeps its snapshots.
    #[test]
    fn a_missing_drive_keeps_its_snapshots() {
        if cfg!(windows) {
            assert!(volume_present(Path::new(r"C:\Windows")));
            assert!(!volume_present(Path::new(r"\\server\share\repo\.git")));
        } else {
            assert!(!volume_present(Path::new("/media/usb/repo/.git")));
        }
        let Some(fx) = Fixture::new("snapshot-drive") else {
            return;
        };
        write(&fx.repo.join("a.txt"), "changed\n");
        assert!(matches!(fx.take(""), TakeOutcome::Created { .. }));
        let side = fx.side();
        let gone = if cfg!(windows) {
            let Some(letter) = (b'D'..=b'Z')
                .rev()
                .map(char::from)
                .find(|letter| !Path::new(&format!("{letter}:\\")).exists())
            else {
                return;
            };
            PathBuf::from(format!(r"{letter}:\repo\.git"))
        } else {
            PathBuf::from("/media/hangar-test-unplugged/repo/.git")
        };
        fs::write(side.join(SOURCE_FILE), gone.to_string_lossy().as_bytes()).unwrap();
        let done = clean_up(&fx.env(), Some(&BTreeSet::new()));
        assert_eq!(done.side_repos, 0);
        assert!(side.exists());
    }
}
