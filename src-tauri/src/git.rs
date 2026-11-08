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
use std::path::{Path, PathBuf};
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
const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;
/// Only the first part of an error message is kept; the rest is read and dropped.
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_MESSAGE_CHARS: usize = 500;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Variables that would point git at another repository than the folder it runs in. Hangar
/// itself may inherit them when it is started from a git hook or an IDE task.
const REPOSITORY_OVERRIDES: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CEILING_DIRECTORIES",
    "GIT_EXTERNAL_DIFF",
];

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
                return Some(dir.join(target));
            }
            _ => {}
        }
        if ceiling == Some(dir) {
            break;
        }
    }
    None
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
            .stderr(Stdio::piped())
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat");
        for key in REPOSITORY_OVERRIDES {
            command.env_remove(key);
        }
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
        let tree = ProcTree::attach(child.id()).ok();

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
            stdout,
            stderr,
            truncated,
        })
    }
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
    if let Some(tree) = tree {
        // Failure usually means everything has already ended.
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

/// Folder argument from the frontend: absolute and an existing folder.
pub fn check_folder(path: &Path) -> Result<(), GitError> {
    if !path.is_absolute() {
        return Err(GitError::InvalidPath("not an absolute path"));
    }
    if !path.is_dir() {
        return Err(GitError::InvalidPath("not a folder"));
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

const DIFF_ARGS: &[&str] = &["diff", "--no-color", "--no-ext-diff", "--no-textconv"];

/// Changes of the work tree against HEAD: file list with +/- and the unified diff.
pub fn diff(git: &Git, cwd: &Path) -> Result<GitDiff, GitError> {
    check_folder(cwd)?;
    let deadline = Instant::now() + TIMEOUT;
    let Some(root) = toplevel(git, cwd, deadline)? else {
        return Ok(GitDiff::not_a_repo());
    };
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
        ["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
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
            DIFF_ARGS.iter().chain(&["--numstat", "-z"]).chain(*target),
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
        let patch = git.run(&root, DIFF_ARGS.iter().chain(*target), deadline, room)?;
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
