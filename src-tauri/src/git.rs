//! Git for the status line, the Changes panel and worktrees
//! (contract: docs/wiki/pages/syntheses/agent-workflow-features.md).
//!
//! Branches are read from the files under `.git` without starting a process, because the sidebar
//! asks for them every 15 seconds. Everything else runs `git` from PATH as an argv array, with
//! optional locks off (a status in the background must not block the agent's own git calls),
//! no prompts, no pager, no fsmonitor, no external diff or textconv, and a time limit after which
//! the whole process tree is killed.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};

use crate::persist::read_limited;
use crate::proctree::ProcTree;

pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Upper limit of the unified diff handed to the window.
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_BRANCH_PATHS: usize = 64;
/// HEAD holds one line; anything larger is not a HEAD file.
const MAX_HEAD_BYTES: usize = 4096;
/// Status and numstat lists. Beyond this the file list is cut and `truncated` set.
pub const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;
/// Only the first part of an error message is kept; the rest is read and dropped.
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_MESSAGE_CHARS: usize = 500;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Inherited variables that git reads all start with this prefix (`GIT_DIR`, `GIT_WORK_TREE`,
/// `GIT_CONFIG_PARAMETERS`, `GIT_EXTERNAL_DIFF`, `GIT_SSH_COMMAND`, ...). Hangar itself may inherit
/// them when it is started from a git hook or an IDE task; every one of them is removed from git's
/// environment before Hangar sets its own.
const GIT_ENV_PREFIX: &str = "GIT_";

/// Upper limit of filter drivers that are switched off; a repository with more is refused rather
/// than diffed with some filters still active.
const MAX_FILTER_DRIVERS: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git was not found on PATH")]
    NotFound,
    #[error("git could not be started: {0}")]
    Spawn(#[source] io::Error),
    #[error("git did not finish in time")]
    Timeout,
    /// git's own message (stderr), shortened.
    #[error("{0}")]
    Failed(String),
    #[error("Invalid folder: {0}")]
    InvalidPath(&'static str),
    #[error("Too many folders: at most {MAX_BRANCH_PATHS} per call")]
    TooManyPaths,
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error("git cannot be watched: {0}")]
    Watch(#[source] io::Error),
    #[error("The repository configures filters Hangar cannot switch off safely")]
    UnsafeFilters,
    #[error("The repository's git folder lies on another network share or device; Hangar does not run git there")]
    RemoteGitDir,
}

impl Serialize for GitError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// Branches without a process

/// Branch per folder for the status line. Keys are the folders exactly as passed in.
pub fn branches(paths: &[String]) -> Result<HashMap<String, Option<String>>, GitError> {
    if paths.len() > MAX_BRANCH_PATHS {
        return Err(GitError::TooManyPaths);
    }
    Ok(paths
        .iter()
        .map(|path| (path.clone(), branch_of(Path::new(path), None)))
        .collect())
}

/// `ref: refs/heads/x` gives `x`, a detached HEAD the first 7 hex characters, no repository
/// `None`. The walk up stops after `ceiling` (tests use it to stay inside their folder).
pub fn branch_of(start: &Path, ceiling: Option<&Path>) -> Option<String> {
    // A relative path would be resolved against Hangar's own working folder.
    if !start.is_absolute() {
        return None;
    }
    let git_dir = find_git_dir(start, ceiling)?;
    let head = read_limited(&git_dir.join("HEAD"), MAX_HEAD_BYTES).ok()??;
    parse_head(&String::from_utf8_lossy(&head))
}

/// The git directory of the repository that contains `start`: a `.git` folder, or the target of
/// a `.git` file (linked worktrees and submodules).
fn find_git_dir(start: &Path, ceiling: Option<&Path>) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        match fs::metadata(&dot_git) {
            Ok(meta) if meta.is_dir() => return Some(dot_git),
            Ok(meta) if meta.is_file() => {
                let content = read_limited(&dot_git, MAX_HEAD_BYTES).ok()??;
                let target = parse_gitdir_file(&String::from_utf8_lossy(&content))?;
                // A relative gitdir is relative to the folder holding the `.git` file.
                let git_dir = dir.join(target);
                return gitdir_allowed(start, &git_dir).then_some(git_dir);
            }
            _ => {}
        }
        if ceiling == Some(dir) {
            break;
        }
    }
    None
}

/// Whether the sidebar may read `git_dir` for a folder below `start`. A `.git` file in a local
/// folder must not lead to a network share or a device: the sidebar reads HEAD every 15 seconds,
/// and on Windows each access to `\\host\share` sends the user's NTLM credentials to that host.
/// The same prefix as the start folder stays allowed (a repository that lives on a share anyway).
pub fn gitdir_allowed(start: &Path, git_dir: &Path) -> bool {
    match (remote_prefix(start), remote_prefix(git_dir)) {
        (_, None) => true,
        (Some(start), Some(target)) => start.eq_ignore_ascii_case(&target),
        (None, Some(_)) => false,
    }
}

/// The prefix of a path that does not name a local drive (UNC share, `\\?\UNC\`, `\\.\` device,
/// other `\\?\` namespaces), as text for comparison; `None` for local paths. Unix paths have no
/// prefix.
fn remote_prefix(path: &Path) -> Option<String> {
    use std::path::Prefix;
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) | Prefix::VerbatimDisk(_) => None,
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(format!(
                "unc:{}\\{}",
                server.to_string_lossy(),
                share.to_string_lossy()
            )),
            Prefix::DeviceNS(name) => Some(format!("device:{}", name.to_string_lossy())),
            Prefix::Verbatim(name) => Some(format!("verbatim:{}", name.to_string_lossy())),
        },
        _ => None,
    }
}

/// The path names a network share or a device rather than a local drive.
pub fn is_remote(path: &Path) -> bool {
    remote_prefix(path).is_some()
}

/// Target of a `.git` file: `gitdir: <path>`.
pub fn parse_gitdir_file(content: &str) -> Option<PathBuf> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(PathBuf::from)
}

pub fn parse_head(content: &str) -> Option<String> {
    let content = content.trim();
    if let Some(reference) = content.strip_prefix("ref:") {
        let reference = reference.trim();
        let name = reference
            .strip_prefix("refs/heads/")
            .or_else(|| reference.strip_prefix("refs/"))
            .unwrap_or(reference);
        return (!name.is_empty()).then(|| name.to_owned());
    }
    let hex = content.len() >= 7 && content.bytes().all(|b| b.is_ascii_hexdigit());
    hex.then(|| content[..7].to_ascii_lowercase())
}

// ---------------------------------------------------------------------------------------------
// Running git

/// Result of one git call. `truncated`: stdout exceeded the limit, git was stopped there.
#[derive(Debug)]
pub struct Output {
    pub success: bool,
    /// Exit code; `None` when git was stopped at the output limit or ended by a signal.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

impl Output {
    /// Finished cleanly, or stopped at the output limit (the kill makes `success` false, but
    /// what was read up to the limit is still valid).
    pub fn usable(&self) -> bool {
        self.success || self.truncated
    }

    /// git's message as an error.
    pub fn failure(&self) -> GitError {
        GitError::Failed(message(&self.stderr))
    }
}

/// First part of git's stderr, on one line.
pub fn message(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        return "git failed without a message".to_owned();
    }
    joined.chars().take(MAX_MESSAGE_CHARS).collect()
}

/// The `git` program found on PATH.
#[derive(Debug, Clone)]
pub struct Git {
    program: PathBuf,
}

impl Git {
    /// Own PATH search (see resolve.rs): Command would also look next to Hangar's executable.
    pub fn locate() -> Result<Self, GitError> {
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        crate::resolve::resolve_program("git", path.as_deref(), pathext.as_deref())
            .map(|program| Self { program })
            .ok_or(GitError::NotFound)
    }

    /// Runs git in `cwd`. stdout is kept up to `limit` bytes; beyond that git is stopped and
    /// `truncated` set. After `deadline` the process tree is killed and the call fails.
    pub fn run<I, S>(
        &self,
        cwd: &Path,
        args: I,
        deadline: Instant,
        limit: usize,
    ) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.program);
        command
            .arg("--no-pager")
            .args(["-c", "core.fsmonitor=false"])
            .args(["-c", "core.quotepath=false"])
            .args(["-c", "color.ui=false"])
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, _) in std::env::vars_os() {
            if is_git_variable(&key) {
                command.env_remove(&key);
            }
        }
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat");
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

        let mut child = command.spawn().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                GitError::NotFound
            } else {
                GitError::Spawn(error)
            }
        })?;
        // Without the tree a timeout could only kill git itself, not a helper it started; such a
        // call is not started at all.
        let tree = match ProcTree::attach(child.id()) {
            Ok(tree) => Some(tree),
            // git that ended before it could join the job has nothing left to kill; its output
            // is still in the pipes.
            Err(_) if matches!(child.try_wait(), Ok(Some(_))) => None,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::Watch(error));
            }
        };

        let (tx, rx) = mpsc::channel();
        let mut pending = 0;
        if let Some(stdout) = child.stdout.take() {
            pending += usize::from(drain(stdout, 0, limit, true, tx.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            pending += usize::from(drain(stderr, 1, MAX_STDERR_BYTES, false, tx));
        }

        let mut outputs = [Vec::new(), Vec::new()];
        let mut truncated = false;
        let mut timed_out = false;
        while pending > 0 {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok((index, mut data)) => {
                    pending -= 1;
                    if index == 0 && data.len() > limit {
                        data.truncate(limit);
                        truncated = true;
                        // Nobody reads the rest; git would block on a full pipe.
                        terminate(tree.as_ref(), &mut child);
                    }
                    if let Some(slot) = outputs.get_mut(index) {
                        *slot = data;
                    }
                }
                Err(_) => {
                    timed_out = true;
                    break;
                }
            }
        }

        let status = if timed_out {
            None
        } else {
            wait_until(&mut child, deadline)
        };
        if status.is_none() && !truncated {
            timed_out = true;
        }
        // Always reap; after a regular exit this returns at once.
        terminate(tree.as_ref(), &mut child);
        if timed_out {
            return Err(GitError::Timeout);
        }

        let [stdout, stderr] = outputs;
        Ok(Output {
            success: status.is_some_and(|status| status.success()),
            code: status.and_then(|status| status.code()),
            stdout,
            stderr,
            truncated,
        })
    }
}

/// `GIT_*` in any case (Windows variable names ignore case).
pub fn is_git_variable(name: &OsStr) -> bool {
    name.as_encoded_bytes()
        .get(..GIT_ENV_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(GIT_ENV_PREFIX.as_bytes()))
}

/// Reads a stream on a thread of its own; both streams must be read at the same time, or a
/// child writing into a full pipe blocks. Keeps `keep` bytes. With `stop`, it reads one byte more
/// and then stops, so the caller can tell that the limit was exceeded; without, it reads on to
/// EOF and drops the rest.
fn drain(
    stream: impl Read + Send + 'static,
    index: usize,
    keep: usize,
    stop: bool,
    tx: Sender<(usize, Vec<u8>)>,
) -> bool {
    thread::Builder::new()
        .name(format!("git-read-{index}"))
        .spawn(move || {
            let mut data = Vec::new();
            let mut stream = stream;
            let first = u64::try_from(keep.saturating_add(1)).unwrap_or(u64::MAX);
            let _ = (&mut stream).take(first).read_to_end(&mut data);
            if !stop {
                data.truncate(keep);
                let _ = io::copy(&mut stream, &mut io::sink());
            }
            let _ = tx.send((index, data));
        })
        .is_ok()
}

fn wait_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => return None,
        }
    }
}

fn terminate(tree: Option<&ProcTree>, child: &mut Child) {
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
    // Failure usually means everything has already ended.
    if let Some(tree) = tree {
        let _ = tree.terminate();
    }
    let _ = child.kill();
    // Reap it so no zombie remains on Unix. After kill, wait returns immediately.
    let _ = child.wait();
}

/// git prints forward slashes on Windows; the rest of Hangar uses native paths.
pub fn native_path(text: &str) -> PathBuf {
    let text = text.trim_end_matches(['\r', '\n']);
    if cfg!(windows) {
        PathBuf::from(text.replace('/', "\\"))
    } else {
        PathBuf::from(text)
    }
}

/// Root of the work tree that contains `cwd`; `None` outside a repository.
pub fn toplevel(git: &Git, cwd: &Path, deadline: Instant) -> Result<Option<PathBuf>, GitError> {
    let output = git.run(cwd, ["rev-parse", "--show-toplevel"], deadline, 64 * 1024)?;
    let text = String::from_utf8_lossy(&output.stdout);
    if !output.success || text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(native_path(&text)))
}

/// Folder argument from the frontend: absolute, an existing folder, and a repository whose git
/// folder git may open ([`check_git_links`]).
pub fn check_folder(path: &Path) -> Result<(), GitError> {
    if !path.is_absolute() {
        return Err(GitError::InvalidPath("not an absolute path"));
    }
    if !path.is_dir() {
        return Err(GitError::InvalidPath("not a folder"));
    }
    check_git_links(path)
}

/// The rule of [`gitdir_allowed`], applied before git runs: the `.git` file of a linked
/// worktree or submodule, and the `commondir` file of the git folder it names, must not lead
/// from a local folder to a network share or a device, where git would hand the user's
/// credentials to another host. Only the paths are compared; nothing on a share is touched.
/// A folder outside any repository passes (git then finds none). Includes, alternates and other
/// paths inside the repository's config are not followed (contract: agent-workflow-features.md,
/// "Git").
pub fn check_git_links(start: &Path) -> Result<(), GitError> {
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        let git_dir = match fs::metadata(&dot_git) {
            Ok(meta) if meta.is_dir() => dot_git,
            Ok(meta) if meta.is_file() => {
                // Unreadable, too large or no `gitdir:` line: git cannot use it either.
                let Ok(Some(content)) = read_limited(&dot_git, MAX_HEAD_BYTES) else {
                    return Ok(());
                };
                let Some(target) = parse_gitdir_file(&String::from_utf8_lossy(&content)) else {
                    return Ok(());
                };
                let git_dir = dir.join(target);
                if !gitdir_allowed(start, &git_dir) {
                    return Err(GitError::RemoteGitDir);
                }
                git_dir
            }
            _ => continue,
        };
        // A linked worktree's git folder names the main one in `commondir`, relative to itself.
        if let Ok(Some(content)) = read_limited(&git_dir.join("commondir"), MAX_HEAD_BYTES) {
            let common = String::from_utf8_lossy(&content);
            let common = common.trim();
            if !common.is_empty() && !gitdir_allowed(start, &git_dir.join(common)) {
                return Err(GitError::RemoteGitDir);
            }
        }
        return Ok(());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Changes panel

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// Porcelain v1 XY code, for example ` M`, `A `, `??`.
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub is_repo: bool,
    pub files: Vec<FileChange>,
    pub diff: String,
    pub truncated: bool,
}

impl GitDiff {
    fn not_a_repo() -> Self {
        Self {
            is_repo: false,
            files: Vec::new(),
            diff: String::new(),
            truncated: false,
        }
    }
}

const DIFF_ARGS: &[&str] = &[
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--ignore-submodules=all",
];

/// Filters git must not run for Hangar. `status` and `diff` run a repository's clean filter on
/// files whose stat data changed, and a filter is any program the repository's config names
/// (a cloned repository with a planted `.git/config`, or a `[include]` from it). Reading the
/// config runs nothing.
const FILTER_KEYS: &str = r"^filter\..*\.(clean|smudge|process)$";

/// `-c` arguments that switch off every clean, smudge and process filter configured for the
/// repository at `root`: git skips a filter whose command is empty, and `required=false` keeps
/// it from failing the call instead. Must come before the subcommand.
pub fn filter_overrides(
    git: &Git,
    root: &Path,
    deadline: Instant,
) -> Result<Vec<String>, GitError> {
    let output = git.run(
        root,
        ["config", "-z", "--get-regexp", FILTER_KEYS],
        deadline,
        MAX_LIST_BYTES,
    )?;
    if output.truncated {
        return Err(GitError::UnsafeFilters);
    }
    if !output.success {
        // 1: no such key, which is the normal case.
        return match output.code {
            Some(1) => Ok(Vec::new()),
            _ => Err(output.failure()),
        };
    }
    let drivers = filter_drivers(&output.stdout).ok_or(GitError::UnsafeFilters)?;
    if drivers.len() > MAX_FILTER_DRIVERS {
        return Err(GitError::UnsafeFilters);
    }
    Ok(drivers
        .iter()
        .flat_map(|name| {
            ["clean=", "smudge=", "process=", "required=false"]
                .into_iter()
                .flat_map(move |setting| ["-c".to_owned(), format!("filter.{name}.{setting}")])
        })
        .collect())
}

/// Driver names from `git config -z --get-regexp`: records `filter.<name>.<key>\n<value>\0`; a
/// name may itself contain dots. `None` when a name could not be passed back through `-c`, which
/// splits at the first `=`.
pub fn filter_drivers(output: &[u8]) -> Option<std::collections::BTreeSet<String>> {
    let mut names = std::collections::BTreeSet::new();
    for record in output.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let key = record.split(|b| *b == b'\n').next().unwrap_or_default();
        let key = std::str::from_utf8(key).ok()?;
        let rest = key.strip_prefix("filter.")?;
        let (name, _variable) = rest.rsplit_once('.')?;
        if name.is_empty() || name.contains(['=', '\n', '\0']) {
            return None;
        }
        names.insert(name.to_owned());
    }
    Some(names)
}

/// Changes of the work tree against HEAD: file list with +/- and the unified diff.
pub fn diff(git: &Git, cwd: &Path) -> Result<GitDiff, GitError> {
    check_folder(cwd)?;
    let deadline = Instant::now() + TIMEOUT;
    let Some(root) = toplevel(git, cwd, deadline)? else {
        return Ok(GitDiff::not_a_repo());
    };
    let neutral = filter_overrides(git, &root, deadline)?;
    let has_head = git
        .run(
            &root,
            ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
            deadline,
            4096,
        )?
        .success;

    let status = git.run(
        &root,
        neutral.iter().map(String::as_str).chain([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=all",
        ]),
        deadline,
        MAX_LIST_BYTES,
    )?;
    if !status.usable() {
        return Err(status.failure());
    }
    let mut truncated = status.truncated;
    let mut files = parse_status(&status.stdout);

    // A repository without commits has no HEAD to compare with: staged changes plus the work
    // tree instead.
    let targets: &[&[&str]] = if has_head {
        &[&["HEAD", "--"]]
    } else {
        &[&["--cached", "--"], &["--"]]
    };
    let mut counts: HashMap<String, (u64, u64)> = HashMap::new();
    let mut text = Vec::new();
    for target in targets {
        let numstat = git.run(
            &root,
            neutral
                .iter()
                .map(String::as_str)
                .chain(DIFF_ARGS.iter().copied())
                .chain(["--numstat", "-z"])
                .chain(target.iter().copied()),
            deadline,
            MAX_LIST_BYTES,
        )?;
        if !numstat.usable() {
            return Err(numstat.failure());
        }
        truncated |= numstat.truncated;
        for (path, added, deleted) in parse_numstat(&numstat.stdout) {
            let entry = counts.entry(path).or_default();
            entry.0 += added;
            entry.1 += deleted;
        }

        let room = MAX_DIFF_BYTES.saturating_sub(text.len());
        let patch = git.run(
            &root,
            neutral
                .iter()
                .map(String::as_str)
                .chain(DIFF_ARGS.iter().copied())
                .chain(target.iter().copied()),
            deadline,
            room,
        )?;
        if !patch.usable() {
            return Err(patch.failure());
        }
        text.extend_from_slice(&patch.stdout);
        if patch.truncated {
            truncated = true;
            break;
        }
    }
    for file in &mut files {
        if let Some((added, deleted)) = counts.get(&file.path) {
            file.additions = *added;
            file.deletions = *deleted;
        }
    }
    Ok(GitDiff {
        is_repo: true,
        files,
        diff: cut_text(text, truncated),
        truncated,
    })
}

/// A cut diff ends at the last whole line, so no half UTF-8 character or half line remains.
fn cut_text(mut text: Vec<u8>, truncated: bool) -> String {
    if truncated {
        let end = text.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        text.truncate(end);
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// `git status --porcelain=v1 -z`: `XY path\0`, for renames and copies `XY new\0old\0`.
pub fn parse_status(output: &[u8]) -> Vec<FileChange> {
    let mut files = Vec::new();
    let mut records = output.split(|b| *b == 0);
    while let Some(record) = records.next() {
        // An incomplete last record (cut output) has no path yet.
        if record.len() < 4 || record.get(2) != Some(&b' ') {
            continue;
        }
        let (code, path) = (&record[..2], &record[3..]);
        if code.iter().any(|c| matches!(c, b'R' | b'C')) {
            // The original path follows as a record of its own.
            records.next();
        }
        files.push(FileChange {
            path: String::from_utf8_lossy(path).into_owned(),
            status: String::from_utf8_lossy(code).into_owned(),
            additions: 0,
            deletions: 0,
        });
    }
    files
}

/// `git diff --numstat -z`: `added\tdeleted\tpath\0`, for renames `added\tdeleted\t\0old\0new\0`.
/// Binary files show `-` and count as 0.
pub fn parse_numstat(output: &[u8]) -> Vec<(String, u64, u64)> {
    let mut entries = Vec::new();
    let mut records = output.split(|b| *b == 0);
    while let Some(record) = records.next() {
        let mut fields = record.splitn(3, |b| *b == b'\t');
        let (Some(added), Some(deleted), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let count = |field: &[u8]| {
            std::str::from_utf8(field)
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
                .unwrap_or(0)
        };
        let path = if path.is_empty() {
            let _old = records.next();
            match records.next() {
                Some(new) if !new.is_empty() => new,
                _ => continue,
            }
        } else {
            path
        };
        entries.push((
            String::from_utf8_lossy(path).into_owned(),
            count(added),
            count(deleted),
        ));
    }
    entries
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn head_contents() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n").as_deref(),
            Some("main")
        );
        assert_eq!(
            parse_head("ref: refs/heads/hangar/fix-a1\r\n").as_deref(),
            Some("hangar/fix-a1")
        );
        assert_eq!(
            parse_head("ref: refs/remotes/origin/x").as_deref(),
            Some("remotes/origin/x")
        );
        assert_eq!(
            parse_head("4945822ABCDEF0123456789abcdef0123456789\n").as_deref(),
            Some("4945822")
        );
        for bad in [
            "",
            "ref:",
            "ref: refs/",
            "xyz",
            "12345",
            "not a head file at all",
        ] {
            assert_eq!(parse_head(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn gitdir_file_contents() {
        assert_eq!(
            parse_gitdir_file("gitdir: ../.git/worktrees/x\n"),
            Some(PathBuf::from("../.git/worktrees/x"))
        );
        assert_eq!(parse_gitdir_file("gitdir:   \n"), None);
        assert_eq!(parse_gitdir_file("something else"), None);
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn branches_from_git_files() {
        let tmp = TempDir::new("git-branches");
        let root = tmp.path();
        let ceiling = Some(root);

        // Normal repository, asked from the root and from a subfolder.
        let repo = root.join("repo with space");
        write(&repo.join(".git/HEAD"), "ref: refs/heads/main\n");
        fs::create_dir_all(repo.join("src/deep")).unwrap();
        assert_eq!(branch_of(&repo, ceiling).as_deref(), Some("main"));
        assert_eq!(
            branch_of(&repo.join("src/deep"), ceiling).as_deref(),
            Some("main")
        );

        // Linked worktree with a relative gitdir.
        write(
            &repo.join(".git/worktrees/feature/HEAD"),
            "ref: refs/heads/hangar/feature-1a2b3c\n",
        );
        let relative = root.join("wt-relative");
        write(
            &relative.join(".git"),
            "gitdir: ../repo with space/.git/worktrees/feature\n",
        );
        assert_eq!(
            branch_of(&relative, ceiling).as_deref(),
            Some("hangar/feature-1a2b3c")
        );

        // Linked worktree with an absolute gitdir and a detached HEAD.
        write(
            &repo.join(".git/worktrees/detached/HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        );
        let absolute = root.join("wt-absolute");
        let target = repo.join(".git/worktrees/detached");
        write(
            &absolute.join(".git"),
            &format!("gitdir: {}\n", target.display()),
        );
        assert_eq!(branch_of(&absolute, ceiling).as_deref(), Some("0123456"));

        // No repository up to the ceiling, a relative path, a missing folder.
        let plain = root.join("plain/sub");
        fs::create_dir_all(&plain).unwrap();
        assert_eq!(branch_of(&plain, ceiling), None);
        assert_eq!(branch_of(Path::new("relative/path"), None), None);
        assert_eq!(branch_of(&root.join("missing"), ceiling), None);
        // A .git file pointing nowhere.
        let broken = root.join("broken");
        write(&broken.join(".git"), "gitdir: does-not-exist\n");
        assert_eq!(branch_of(&broken, ceiling), None);
    }

    #[test]
    fn branches_limits_the_number_of_paths() {
        let paths: Vec<String> = (0..=MAX_BRANCH_PATHS).map(|i| format!("p{i}")).collect();
        assert!(matches!(branches(&paths), Err(GitError::TooManyPaths)));
        let result = branches(&paths[..2]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("p0"), Some(&None));
    }

    #[test]
    fn status_records() {
        let output =
            b" M src/main.rs\0A  new file.txt\0R  renamed.rs\0original.rs\0?? untracked/\0D  gone.rs\0MM";
        let files = parse_status(output);
        let summary: Vec<(&str, &str)> = files
            .iter()
            .map(|f| (f.status.as_str(), f.path.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (" M", "src/main.rs"),
                ("A ", "new file.txt"),
                ("R ", "renamed.rs"),
                ("??", "untracked/"),
                ("D ", "gone.rs"),
            ]
        );
        assert!(parse_status(b"").is_empty());
    }

    #[test]
    fn numstat_records() {
        let output = b"3\t1\tsrc/main.rs\0-\t-\timage.png\x002\t0\t\0old.rs\0new.rs\0";
        assert_eq!(
            parse_numstat(output),
            [
                ("src/main.rs".to_owned(), 3, 1),
                ("image.png".to_owned(), 0, 0),
                ("new.rs".to_owned(), 2, 0),
            ]
        );
        // Cut in the middle of a rename.
        assert!(parse_numstat(b"2\t0\t\0old.rs\0").is_empty());
    }

    #[test]
    fn cut_diff_ends_on_a_whole_line() {
        let text = "line one\nline twö\nhalf".as_bytes().to_vec();
        assert_eq!(cut_text(text.clone(), true), "line one\nline twö\n");
        assert_eq!(cut_text(text, false), "line one\nline twö\nhalf");
    }

    #[test]
    fn messages_are_one_short_line() {
        assert_eq!(
            message(b"fatal: first\n\n  second line\n"),
            "fatal: first second line"
        );
        assert_eq!(message(b""), "git failed without a message");
        assert_eq!(message(&[b'x'; 5000]).chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn output_cut_at_the_limit_is_usable() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-limit");
        // `git version` prints more than 4 bytes; git is stopped there.
        let output = git
            .run(tmp.path(), ["version"], Instant::now() + TIMEOUT, 4)
            .unwrap();
        assert!(output.truncated);
        assert_eq!(output.stdout, b"git ");
        assert!(output.usable());
        let failed = Output {
            success: false,
            code: Some(128),
            stdout: Vec::new(),
            stderr: b"fatal: x".to_vec(),
            truncated: false,
        };
        assert!(!failed.usable());
    }

    /// git on PATH or `None`; integration tests skip without it.
    pub(crate) fn git() -> Option<Git> {
        let git = Git::locate().ok();
        if git.is_none() {
            eprintln!("git not on PATH, skipping");
        }
        git
    }

    /// Runs a git command in a test repository with a fixed identity and no signing.
    pub(crate) fn git_ok(git: &Git, cwd: &Path, args: &[&str]) {
        let mut all = vec![
            "-c",
            "user.name=Hangar Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ];
        all.extend_from_slice(args);
        let output = git
            .run(cwd, &all, Instant::now() + Duration::from_secs(30), 1 << 20)
            .unwrap();
        assert!(output.success, "git {args:?}: {}", message(&output.stderr));
    }

    pub(crate) fn init_repo(git: &Git, dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git_ok(git, dir, &["init", "--quiet", "--initial-branch=main"]);
    }

    #[test]
    fn inherited_git_variables_are_recognized() {
        for name in [
            "GIT_DIR",
            "git_dir",
            "Git_Config_Parameters",
            "GIT_SSH_COMMAND",
            "GIT_",
        ] {
            assert!(is_git_variable(OsStr::new(name)), "{name}");
        }
        for name in ["PATH", "GITHUB_TOKEN", "GI", "", "MY_GIT_DIR"] {
            assert!(!is_git_variable(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn filter_driver_names() {
        let output =
            b"filter.lfs.clean\ngit-lfs clean -- %f\0filter.lfs.process\ngit-lfs filter-process\0\
              filter.with.dots.smudge\nx\0filter.bare.clean\0";
        let names: Vec<String> = filter_drivers(output).unwrap().into_iter().collect();
        assert_eq!(names, ["bare", "lfs", "with.dots"]);
        assert!(filter_drivers(b"").unwrap().is_empty());
        // A name `-c` would split wrongly, and records that are no filter keys.
        assert_eq!(filter_drivers(b"filter.a=b.clean\nx\0"), None);
        assert_eq!(filter_drivers(b"filter..clean\nx\0"), None);
        assert_eq!(filter_drivers(b"core.editor\nx\0"), None);
    }

    #[cfg(windows)]
    #[test]
    fn gitdir_files_do_not_lead_to_network_shares() {
        let local = Path::new(r"C:\Users\x\repo");
        for allowed in [
            r"C:\Users\x\main\.git\worktrees\a",
            r"D:\elsewhere\.git",
            r"\\?\C:\long\path\.git",
        ] {
            assert!(gitdir_allowed(local, Path::new(allowed)), "{allowed}");
        }
        for refused in [
            r"\\attacker.example\share\.git",
            r"//attacker.example/share/.git",
            r"\\?\UNC\attacker.example\share\.git",
            r"\\.\pipe\x",
            r"\\?\GLOBALROOT\Device\Mup\host\share",
        ] {
            assert!(!gitdir_allowed(local, Path::new(refused)), "{refused}");
        }
        // A repository on a share may point into the same share, not into another one.
        let share = Path::new(r"\\server\share\repo");
        assert!(gitdir_allowed(
            share,
            Path::new(r"\\SERVER\share\main\.git")
        ));
        assert!(gitdir_allowed(
            share,
            Path::new(r"\\?\UNC\server\share\.git")
        ));
        assert!(gitdir_allowed(share, Path::new(r"C:\local\.git")));
        assert!(!gitdir_allowed(share, Path::new(r"\\other\share\.git")));

        // The sidebar never follows such a file: no HEAD is read from the share.
        let tmp = TempDir::new("git-unc");
        let repo = tmp.path().join("cloned");
        write(&repo.join(".git"), "gitdir: \\\\192.0.2.1\\share\\.git\n");
        assert_eq!(branch_of(&repo, Some(tmp.path())), None);

        // git itself never starts there: the check runs before any process.
        assert!(matches!(check_folder(&repo), Err(GitError::RemoteGitDir)));
        if let Some(git) = git() {
            assert!(matches!(diff(&git, &repo), Err(GitError::RemoteGitDir)));
            assert!(matches!(
                crate::worktree::create(&git, &tmp.path().join("wt"), &repo, "x"),
                Err(crate::worktree::WorktreeError::Git(GitError::RemoteGitDir))
            ));
        }
        // A linked worktree whose local git folder names a share as its common folder.
        let linked = tmp.path().join("linked");
        let gitdir = tmp.path().join("main/.git/worktrees/linked");
        write(&gitdir.join("commondir"), "\\\\192.0.2.1\\share\\.git\n");
        write(
            &linked.join(".git"),
            &format!("gitdir: {}\n", gitdir.display()),
        );
        assert!(matches!(
            check_git_links(&linked),
            Err(GitError::RemoteGitDir)
        ));
        // The usual relative commondir is fine.
        write(&gitdir.join("commondir"), "../..\n");
        check_git_links(&linked).unwrap();
    }

    #[test]
    fn local_git_links_pass() {
        let tmp = TempDir::new("git-links");
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        check_git_links(&plain).unwrap();
        let repo = tmp.path().join("repo");
        write(&repo.join(".git/HEAD"), "ref: refs/heads/main\n");
        check_git_links(&repo.join(".git")).unwrap();
        let linked = tmp.path().join("linked");
        write(
            &linked.join(".git"),
            "gitdir: ../repo/.git/worktrees/linked\n",
        );
        write(&repo.join(".git/worktrees/linked/commondir"), "../..\n");
        check_git_links(&linked).unwrap();
        assert!(matches!(
            check_folder(Path::new("relative")),
            Err(GitError::InvalidPath(_))
        ));
    }

    /// git config value that runs `touch <marker>` before passing the content through. git runs
    /// filter commands through its shell (Git for Windows brings its own `sh`).
    pub(crate) fn marker_filter(marker: &Path) -> String {
        let marker = marker.to_string_lossy().replace('\\', "/");
        format!("touch '{marker}' && cat")
    }

    #[test]
    fn repository_filters_never_run() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-filter");
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        write(
            &repo.join(".gitattributes"),
            "*.txt filter=marker\n*.md filter=with.dot\n",
        );
        write(&repo.join("a.txt"), "one\n");
        write(&repo.join("b.md"), "one\n");
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);

        let marker = tmp.path().join("marker");
        let filter = marker_filter(&marker);
        for key in [
            "filter.marker.clean",
            "filter.marker.smudge",
            "filter.with.dot.clean",
        ] {
            git_ok(&git, &repo, &["config", key, &filter]);
        }
        git_ok(&git, &repo, &["config", "filter.marker.required", "true"]);
        write(&repo.join("a.txt"), "one\ntwo\n");
        write(&repo.join("b.md"), "one\ntwo\n");

        // The test is only meaningful if plain git would run the filter here.
        git_ok(&git, &repo, &["diff", "HEAD", "--stat"]);
        assert!(marker.exists(), "git did not run the filter; test is void");
        fs::remove_file(&marker).unwrap();

        let result = diff(&git, &repo).unwrap();
        assert!(!marker.exists(), "a repository filter ran during git_diff");
        assert_eq!(result.files.len(), 2);
        assert!(result.diff.contains("+two"));
        let overrides = filter_overrides(&git, &repo, Instant::now() + TIMEOUT).unwrap();
        assert!(overrides.contains(&"filter.with.dot.clean=".to_owned()));
        assert!(overrides.contains(&"filter.marker.required=false".to_owned()));
    }

    #[test]
    fn diff_of_a_real_repository() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-diff");
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);

        // Without commits: staged and unstaged changes, untracked file.
        write(&repo.join("a.txt"), "one\ntwo\n");
        git_ok(&git, &repo, &["add", "a.txt"]);
        write(&repo.join("a.txt"), "one\ntwo\nthree\n");
        write(&repo.join("loose.txt"), "x\n");
        let result = diff(&git, &repo).unwrap();
        assert!(result.is_repo);
        assert!(!result.truncated);
        let a = result.files.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!(a.status, "AM");
        assert_eq!((a.additions, a.deletions), (3, 0));
        let loose = result.files.iter().find(|f| f.path == "loose.txt").unwrap();
        assert_eq!(loose.status, "??");
        assert!(result.diff.contains("+three"));

        // With a commit: diff against HEAD, also from a subfolder.
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);
        write(&repo.join("a.txt"), "one\nzwei\nthree\n");
        fs::create_dir_all(repo.join("sub")).unwrap();
        let result = diff(&git, &repo.join("sub")).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].status, " M");
        assert_eq!(
            (result.files[0].additions, result.files[0].deletions),
            (1, 1)
        );
        assert!(result.diff.contains("-two\n+zwei"));

        // Large change: cut at the limit, on a whole line.
        let big: String = (0..300_000).map(|i| format!("line {i}\n")).collect();
        write(&repo.join("big.txt"), &big);
        git_ok(&git, &repo, &["add", "big.txt"]);
        let result = diff(&git, &repo).unwrap();
        assert!(result.truncated);
        assert!(result.diff.len() <= MAX_DIFF_BYTES);
        assert!(result.diff.ends_with('\n'));
        let big_entry = result.files.iter().find(|f| f.path == "big.txt").unwrap();
        assert_eq!(big_entry.additions, 300_000);

        // No work tree. The test folder lies inside Hangar's own checkout, so a plain folder
        // would still be a repository; the git directory itself is not a work tree.
        let result = diff(&git, &repo.join(".git")).unwrap();
        assert_eq!(result, GitDiff::not_a_repo());

        assert!(matches!(
            diff(&git, Path::new("relative")),
            Err(GitError::InvalidPath(_))
        ));
        assert!(matches!(
            diff(&git, &tmp.path().join("missing")),
            Err(GitError::InvalidPath(_))
        ));
    }
}
