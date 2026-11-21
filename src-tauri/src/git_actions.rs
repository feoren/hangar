//! Write actions of the Changes panel: stage, unstage, discard and commit
//! (contract: docs/wiki/pages/syntheses/commit-history.md, "Git actions").
//!
//! The window names a session and paths relative to the repository root, exactly as `git_diff`
//! listed them; the folder comes from the session manager. Every call runs through the hardened
//! runner of git.rs. Paths are checked to stay inside the work tree before git sees them and are
//! handed to git as literal pathspecs on stdin (`--literal-pathspecs`, `--pathspec-from-file`), so
//! no path is ever read as an option, a glob or pathspec magic, and no list overflows the command
//! line. Stage, unstage and discard switch the repository's filter drivers and hooks off (git.rs
//! points `core.hooksPath` where no hook can be), and refuse files that a filter would have to
//! convert (Git LFS, for example): running them without their filter would store or write the
//! wrong content. Discard also refuses any path that leads through or contains a link (symbolic
//! link, junction), where git could delete or write outside the work tree. A commit runs the
//! repository's hooks like a commit in the terminal does; nothing is bypassed there.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};

use crate::git::{self, Git, GitError, Output};
use crate::secret_scan::{self, SecretKind};
use crate::session::SessionId;

/// Files per call; the panel lists at most what `git_diff` found.
pub const MAX_PATHS: usize = 10_000;
/// One path; longer ones are no file the panel listed.
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// A commit runs the repository's hooks (formatters, linters, tests), which may take minutes.
pub const COMMIT_TIMEOUT: Duration = Duration::from_secs(600);
/// Stage, unstage and discard: a base for hashing large files, plus a share per path.
const ACTION_BASE_TIMEOUT: Duration = Duration::from_secs(30);
const ACTION_TIMEOUT_PER_PATH: Duration = Duration::from_millis(10);
/// Entries of an untracked folder checked for links before it is deleted; a larger folder is
/// refused rather than deleted unchecked.
pub const MAX_LINK_SCAN: usize = 100_000;
/// Staged diff read for the secret check before a commit (ADR 0015); the rest is not checked.
pub const MAX_SCAN_BYTES: usize = 8 * 1024 * 1024;
/// How long the secret check may take; after it the commit goes ahead unchecked.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(10);
/// Files named in one answer; more found are counted in `more`.
pub const MAX_FINDINGS: usize = 20;
/// Commands without a pathspec file (`ls-files`, `clean`) take their paths as arguments; a few
/// per call keep the command line short.
const CLEAN_CHUNK: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("Unknown session: {0}")]
    UnknownSession(SessionId),
    #[error("Invalid file path: {0}")]
    InvalidPath(&'static str),
    #[error("No files given")]
    NoPaths,
    #[error("Too many files: at most {MAX_PATHS} per call")]
    TooManyPaths,
    #[error("A file uses a git filter (for example Git LFS); stage or discard it in the terminal")]
    Filtered,
    #[error("{0} leads through or contains a link (symbolic link or junction); discard it in the terminal")]
    Linked(String),
    #[error("{0} is too large to check for links; discard it in the terminal")]
    TooLargeToCheck(String),
    #[error("git did not finish in time. If git later reports that .git/index.lock exists, remove that file once no git command runs anymore")]
    TimedOut,
    #[error("The commit message is empty")]
    EmptyMessage,
    #[error("The commit message is too long or holds control characters")]
    InvalidMessage,
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl Serialize for ActionError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// A timeout gets its own message: git may have left its index lock behind, which Hangar never
/// removes itself (another git may hold it).
fn explain_timeout(error: ActionError) -> ActionError {
    match error {
        ActionError::Git(GitError::Timeout) => ActionError::TimedOut,
        other => other,
    }
}

/// One record of `git status --porcelain=v1 -z`, with the original path of a rename or copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub code: String,
    pub path: String,
    pub original: Option<String>,
}

/// `XY path\0`, for renames and copies `XY new\0old\0`.
pub fn parse_status_entries(output: &[u8]) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut records = output.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 || record.get(2) != Some(&b' ') {
            continue;
        }
        let (code, path) = (&record[..2], &record[3..]);
        let original = if code.iter().any(|c| matches!(c, b'R' | b'C')) {
            records
                .next()
                .filter(|old| !old.is_empty())
                .map(|old| String::from_utf8_lossy(old).into_owned())
        } else {
            None
        };
        entries.push(StatusEntry {
            code: String::from_utf8_lossy(code).into_owned(),
            path: String::from_utf8_lossy(path).into_owned(),
            original,
        });
    }
    entries
}

/// The separators of a path as the panel names it.
fn separators() -> &'static [char] {
    if cfg!(windows) {
        &['/', '\\']
    } else {
        &['/']
    }
}

/// A path as the panel lists it: relative to the repository root, `/` separated, a trailing `/`
/// for an untracked folder. Nothing that could leave the work tree or reach into a git folder.
pub fn check_path(path: &str) -> Result<(), ActionError> {
    if path.is_empty() {
        return Err(ActionError::InvalidPath("empty"));
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(ActionError::InvalidPath("too long"));
    }
    if path.contains('\0') {
        return Err(ActionError::InvalidPath("NUL character"));
    }
    if path.starts_with(['/', '\\']) || Path::new(path).is_absolute() {
        return Err(ActionError::InvalidPath("absolute"));
    }
    // A drive (`C:x` is relative to that drive's folder) or an alternate data stream.
    if cfg!(windows) && path.contains(':') {
        return Err(ActionError::InvalidPath("drive or stream"));
    }
    let body = path.strip_suffix('/').unwrap_or(path);
    for part in body.split(separators()) {
        // Windows drops trailing dots and spaces, so `..` also hides as `.. ` or `...`.
        let effective = if cfg!(windows) {
            part.trim_end_matches(['.', ' '])
        } else {
            part
        };
        if part.is_empty() || part == "." || part == ".." || (cfg!(windows) && effective.is_empty())
        {
            return Err(ActionError::InvalidPath("leaves the repository"));
        }
        if names_git_folder(effective) {
            return Err(ActionError::InvalidPath("inside a git folder"));
        }
    }
    Ok(())
}

/// Code points HFS+ ignores in file names (git's `next_hfs_char`): `.g\u{200c}it` opens `.git`
/// there.
fn hfs_ignored(c: char) -> bool {
    matches!(
        c,
        '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'
    )
}

/// One path component that names a git folder on some file system: `.git` in any case, with the
/// code points HFS+ ignores, or the NTFS short name `git~<n>`. Checked on every platform, since a
/// work tree may live on any of them.
pub fn names_git_folder(part: &str) -> bool {
    let folded: String = part.chars().filter(|c| !hfs_ignored(*c)).collect();
    if folded.eq_ignore_ascii_case(".git") {
        return true;
    }
    let lower = part.to_ascii_lowercase();
    lower
        .strip_prefix("git~")
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

fn check_paths(paths: &[String]) -> Result<(), ActionError> {
    if paths.is_empty() {
        return Err(ActionError::NoPaths);
    }
    if paths.len() > MAX_PATHS {
        return Err(ActionError::TooManyPaths);
    }
    paths.iter().try_for_each(|path| check_path(path))
}

/// A symbolic link, or on Windows any reparse point (junctions included, which Git for Windows
/// may take for a plain folder).
pub fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// Refuses a path whose components below `root` include a link, the path itself included. A
/// folder at the end is walked (at most [`MAX_LINK_SCAN`] entries) and refused if it holds a link.
/// Components that do not exist (a deleted file) end the check: nothing behind them exists.
pub fn refuse_links(root: &Path, path: &str) -> Result<(), ActionError> {
    let body = path.strip_suffix('/').unwrap_or(path);
    let mut current = root.to_path_buf();
    for part in body.split(separators()) {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if is_link(&meta) => return Err(ActionError::Linked(path.to_owned())),
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
    match fs::symlink_metadata(&current) {
        Ok(meta) if meta.is_dir() => refuse_links_below(&current, path),
        _ => Ok(()),
    }
}

/// Walks `folder` without following links; refuses on the first link, an unreadable entry or
/// more than [`MAX_LINK_SCAN`] entries.
fn refuse_links_below(folder: &Path, path: &str) -> Result<(), ActionError> {
    let mut pending = vec![folder.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = pending.pop() {
        let entries =
            fs::read_dir(&dir).map_err(|_| ActionError::TooLargeToCheck(path.to_owned()))?;
        for entry in entries {
            seen += 1;
            if seen > MAX_LINK_SCAN {
                return Err(ActionError::TooLargeToCheck(path.to_owned()));
            }
            let entry = entry.map_err(|_| ActionError::TooLargeToCheck(path.to_owned()))?;
            let meta = fs::symlink_metadata(entry.path())
                .map_err(|_| ActionError::TooLargeToCheck(path.to_owned()))?;
            if is_link(&meta) {
                return Err(ActionError::Linked(path.to_owned()));
            }
            if meta.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}

/// What every write call shares: the repository root, its deadline and the `-c` overrides that
/// switch filters and hooks off.
struct Repo {
    root: PathBuf,
    deadline: Instant,
    /// Filter overrides (and hooks the config names); must come before the subcommand.
    overrides: Vec<String>,
    /// A filter driver is configured, so files have to be checked for filter attributes.
    filters: bool,
}

impl Repo {
    /// The work tree of the session's folder with the folder checks of git.rs, and the
    /// overrides for it. The deadline grows with the number of paths.
    fn open(git: &Git, cwd: &Path, paths: usize) -> Result<Self, ActionError> {
        let per_path =
            ACTION_TIMEOUT_PER_PATH.saturating_mul(u32::try_from(paths).unwrap_or(u32::MAX));
        let deadline = Instant::now() + ACTION_BASE_TIMEOUT + per_path;
        let root = repo_root(git, cwd, deadline)?;
        // Hooks are off through Git itself (a `core.hooksPath` that can never hold a hook); a
        // `core.hooksPath` named here would replace it with a folder that can be filled.
        let overrides = git::filter_overrides(git, &root, deadline)?;
        let filters = overrides.iter().any(|arg| arg.starts_with("filter."));
        Ok(Self {
            root,
            deadline,
            overrides,
            filters,
        })
    }
}

/// Root of the work tree that holds the session's folder, with the folder checks of git.rs.
fn repo_root(git: &Git, cwd: &Path, deadline: Instant) -> Result<PathBuf, ActionError> {
    git::check_folder(cwd)?;
    git::toplevel(git, cwd, deadline)?.ok_or(ActionError::Git(GitError::NotARepository))
}

/// git's message for a failed call: stderr, or stdout where git reports there ("nothing to
/// commit").
fn failure(output: &Output) -> GitError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() && !output.stdout.is_empty() {
        GitError::Failed(git::message(&output.stdout))
    } else {
        output.failure()
    }
}

fn nul_list<S: AsRef<str>>(paths: &[S]) -> Vec<u8> {
    let mut input = Vec::new();
    for path in paths {
        input.extend_from_slice(path.as_ref().as_bytes());
        input.push(0);
    }
    input
}

/// Runs `git --literal-pathspecs <overrides> <args> --pathspec-from-file=- --pathspec-file-nul`
/// with `paths` on stdin; a failure is git's message.
fn run_with_paths<S: AsRef<str>>(
    git: &Git,
    repo: &Repo,
    args: &[&str],
    paths: &[S],
) -> Result<Output, ActionError> {
    let all = ["--literal-pathspecs"]
        .into_iter()
        .chain(repo.overrides.iter().map(String::as_str))
        .chain(args.iter().copied())
        .chain(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    let output = git.run_with_input(
        &repo.root,
        all,
        Some(nul_list(paths)),
        repo.deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !output.success {
        return Err(failure(&output).into());
    }
    Ok(output)
}

/// Runs `git --literal-pathspecs <overrides> <args> -- <paths>` for commands that read no
/// pathspec file, [`CLEAN_CHUNK`] paths per call so the command line stays short.
fn run_chunked<S: AsRef<str>>(
    git: &Git,
    repo: &Repo,
    args: &[&str],
    paths: &[S],
) -> Result<Vec<Output>, ActionError> {
    let mut outputs = Vec::new();
    for chunk in paths.chunks(CLEAN_CHUNK) {
        let all = ["--literal-pathspecs"]
            .into_iter()
            .chain(repo.overrides.iter().map(String::as_str))
            .chain(args.iter().copied())
            .chain(["--"])
            .chain(chunk.iter().map(AsRef::as_ref));
        let output = git.run(&repo.root, all, repo.deadline, git::MAX_LIST_BYTES)?;
        if !output.usable() {
            return Err(failure(&output).into());
        }
        outputs.push(output);
    }
    Ok(outputs)
}

/// Refuses when a filter driver would convert one of the files at or below `paths`. Without any
/// configured driver no filter can run, and nothing is asked.
fn refuse_filtered<S: AsRef<str>>(git: &Git, repo: &Repo, paths: &[S]) -> Result<(), ActionError> {
    if !repo.filters || paths.is_empty() {
        return Ok(());
    }
    // `ls-files` reads no pathspec file; the paths go in as arguments, a few at a time.
    let mut files = Vec::new();
    for output in run_chunked(
        git,
        repo,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
        paths,
    )? {
        files.extend_from_slice(&output.stdout);
        // Too many files to check: refused rather than staged unchecked.
        if output.truncated || files.len() > git::MAX_LIST_BYTES {
            return Err(ActionError::Filtered);
        }
    }
    filter_attributes(git, &repo.root, repo.deadline, files)
}

/// Refuses when a filter driver would convert one of `files` (paths of files below `root`, `/`
/// separated, as git lists them). The caller checks first whether any driver is configured.
pub fn refuse_filtered_files(
    git: &Git,
    root: &Path,
    deadline: Instant,
    files: &[String],
) -> Result<(), ActionError> {
    filter_attributes(git, root, deadline, nul_list(files))
}

/// `git check-attr filter` for the NUL separated `files`; refused when one names a driver.
fn filter_attributes(
    git: &Git,
    root: &Path,
    deadline: Instant,
    files: Vec<u8>,
) -> Result<(), ActionError> {
    if files.is_empty() {
        return Ok(());
    }
    let attributes = git.run_with_input(
        root,
        ["check-attr", "-z", "--stdin", "filter"],
        Some(files),
        deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !attributes.success {
        return Err(failure(&attributes).into());
    }
    if attributes.truncated || filtered(&attributes.stdout) {
        return Err(ActionError::Filtered);
    }
    Ok(())
}

/// `git check-attr -z`: `path\0attribute\0value\0`. A value other than unspecified or unset names
/// a filter driver.
pub fn filtered(output: &[u8]) -> bool {
    output
        .split(|b| *b == 0)
        .collect::<Vec<_>>()
        .chunks(3)
        .filter(|record| record.len() == 3)
        .any(|record| !matches!(record[2], b"unspecified" | b"unset"))
}

fn status(
    git: &Git,
    root: &Path,
    overrides: &[String],
    deadline: Instant,
) -> Result<Vec<StatusEntry>, ActionError> {
    let output = git.run(
        root,
        overrides.iter().map(String::as_str).chain([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=all",
        ]),
        deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !output.usable() {
        return Err(failure(&output).into());
    }
    Ok(parse_status_entries(&output.stdout))
}

fn has_head(git: &Git, repo: &Repo) -> Result<bool, ActionError> {
    Ok(git
        .run(
            &repo.root,
            ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
            repo.deadline,
            4096,
        )?
        .success)
}

/// The paths plus the original path of each rename or copy among them, so a renamed file is
/// unstaged or restored as a whole.
fn with_originals(paths: &[String], entries: &[StatusEntry]) -> Vec<String> {
    let mut all: Vec<String> = Vec::with_capacity(paths.len());
    for path in paths {
        all.push(path.clone());
        if let Some(original) = entries
            .iter()
            .find(|entry| &entry.path == path)
            .and_then(|entry| entry.original.clone())
        {
            all.push(original);
        }
    }
    all.sort();
    all.dedup();
    all
}

/// `git add --all` for the paths: new, changed and deleted files go into the index. Hooks off.
pub fn stage(git: &Git, cwd: &Path, paths: &[String]) -> Result<(), ActionError> {
    let run = || {
        check_paths(paths)?;
        let repo = Repo::open(git, cwd, paths.len())?;
        refuse_filtered(git, &repo, paths)?;
        run_with_paths(git, &repo, &["add", "--all"], paths)?;
        Ok(())
    };
    run().map_err(explain_timeout)
}

/// `git reset` for the paths: the index takes them from HEAD again (or drops them in a repository
/// without commits); the work tree stays as it is. Hooks off.
pub fn unstage(git: &Git, cwd: &Path, paths: &[String]) -> Result<(), ActionError> {
    let run = || {
        check_paths(paths)?;
        let repo = Repo::open(git, cwd, paths.len())?;
        let entries = status(git, &repo.root, &repo.overrides, repo.deadline)?;
        let all = with_originals(paths, &entries);
        run_with_paths(git, &repo, &["reset", "--quiet"], &all)?;
        Ok(())
    };
    run().map_err(explain_timeout)
}

/// Throws the changes of the paths away: tracked files go back to HEAD in the index and the work
/// tree (a file new in the index is removed), untracked files and folders are deleted with
/// `git clean`, which keeps ignored files and nested repositories. Paths without changes are
/// skipped; a path through or holding a link is refused before git runs. Hooks off. The window
/// asks before it calls this.
pub fn discard(git: &Git, cwd: &Path, paths: &[String]) -> Result<(), ActionError> {
    let run = || {
        check_paths(paths)?;
        let repo = Repo::open(git, cwd, paths.len())?;
        let entries = status(git, &repo.root, &repo.overrides, repo.deadline)?;
        let mut tracked = Vec::new();
        let mut untracked = Vec::new();
        for path in paths {
            match entries.iter().find(|entry| &entry.path == path) {
                Some(entry) if entry.code == "??" => untracked.push(path.clone()),
                Some(_) => tracked.push(path.clone()),
                None => {}
            }
        }
        let tracked = with_originals(&tracked, &entries);
        for path in tracked.iter().chain(&untracked) {
            refuse_links(&repo.root, path)?;
        }
        if !tracked.is_empty() {
            refuse_filtered(git, &repo, &tracked)?;
            let args: &[&str] = if has_head(git, &repo)? {
                &["restore", "--source=HEAD", "--staged", "--worktree"]
            } else {
                // Nothing to go back to: the files exist only in the index and the work tree.
                // Status lists files here, never folders, so no `-r`.
                &["rm", "-f", "--quiet"]
            };
            run_with_paths(git, &repo, args, &tracked)?;
        }
        // `git clean` reads no pathspec file either.
        for output in run_chunked(git, &repo, &["clean", "-f", "-d", "--quiet"], &untracked)? {
            if !output.success {
                return Err(failure(&output).into());
            }
        }
        Ok(())
    };
    run().map_err(explain_timeout)
}

/// A staged file whose added lines seem to hold a secret: its path and the kinds, never the
/// line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFinding {
    pub path: String,
    pub kinds: Vec<SecretKind>,
}

/// What the commit box's commit did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommitOutcome {
    /// Committed; `checked` says how far the staged changes were checked for secrets.
    #[serde(rename_all = "camelCase")]
    Committed { checked: Checked },
    /// Nothing was committed: the staged changes seem to hold secrets; the person decides.
    /// `complete`: the whole staged diff was read; `more`: files found beyond [`MAX_FINDINGS`];
    /// `tree`: the index as `git write-tree` names it, which "Commit anyway" hands back
    /// (`allow_tree`), so the answer covers exactly what was checked.
    #[serde(rename_all = "camelCase")]
    Secrets {
        findings: Vec<SecretFinding>,
        more: usize,
        complete: bool,
        tree: Option<String>,
    },
}

/// How far a commit's staged changes were checked for secrets before it went in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Checked {
    /// The whole diff, or the person committed the checked index anyway.
    Full,
    /// Only the start: the diff was cut at [`MAX_SCAN_BYTES`] or the check ran out of time.
    Partial,
    /// Not at all: git could not show the diff.
    Skipped,
}

/// What the check of the staged changes found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedScan {
    pub findings: Vec<SecretFinding>,
    /// Everything was read and scanned in time.
    pub complete: bool,
}

/// A path as git prints it after `+++ `: `b/` in front, quoted with C escapes when it holds a
/// quote, a backslash or a control character (`core.quotePath` is off, so other characters come
/// as they are).
fn diff_path(raw: &str) -> String {
    let Some(quoted) = raw
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return raw.strip_prefix("b/").unwrap_or(raw).to_owned();
    };
    let mut out = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out.strip_prefix("b/").map(str::to_owned).unwrap_or(out)
}

/// Lines read between two looks at the deadline.
const DEADLINE_EVERY: usize = 4096;

/// The files of a `git diff --unified=0` whose added lines seem to hold secrets, in diff order,
/// and whether all of it was scanned before `deadline`. Removed and context lines do not count; a
/// deleted file has no added lines.
pub fn added_secrets(diff: &[u8], deadline: Instant) -> (Vec<SecretFinding>, bool) {
    let mut findings = Vec::new();
    let mut path: Option<String> = None;
    let mut added = String::new();
    let mut in_hunk = false;
    let mut finish = |path: &mut Option<String>, added: &mut String| {
        if let Some(path) = path.take() {
            let kinds = secret_scan::scan(added);
            if !kinds.is_empty() {
                findings.push(SecretFinding { path, kinds });
            }
        }
        added.clear();
    };
    for (number, line) in diff.split(|b| *b == b'\n').enumerate() {
        if number % DEADLINE_EVERY == 0 && Instant::now() >= deadline {
            return (findings, false);
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"diff --git ") {
            finish(&mut path, &mut added);
            in_hunk = false;
        } else if !in_hunk && line.starts_with(b"+++ ") {
            let target = String::from_utf8_lossy(&line[4..]).into_owned();
            path = (target != "/dev/null").then(|| diff_path(&target));
        } else if line.starts_with(b"@@") {
            in_hunk = true;
        } else if in_hunk && line.first() == Some(&b'+') {
            added.push_str(&String::from_utf8_lossy(&line[1..]));
            added.push('\n');
        }
    }
    if Instant::now() >= deadline {
        return (findings, false);
    }
    finish(&mut path, &mut added);
    (findings, true)
}

/// Joins the files found by name ([`secret_scan::secret_file_name`]) with those found by their
/// added lines, one entry per path, in the order of `names` (the diff's order), content-only
/// paths after them.
fn merge_findings(names: &[String], content: Vec<SecretFinding>) -> Vec<SecretFinding> {
    let mut merged: Vec<SecretFinding> = names
        .iter()
        .filter(|name| secret_scan::secret_file_name(name))
        .map(|name| SecretFinding {
            path: name.clone(),
            kinds: vec![SecretKind::SecretFile],
        })
        .collect();
    for finding in content {
        match merged.iter_mut().find(|known| known.path == finding.path) {
            Some(known) => {
                known.kinds.extend(finding.kinds);
                known.kinds.sort();
                known.kinds.dedup();
            }
            None => merged.push(finding),
        }
    }
    let order = |path: &str| {
        names
            .iter()
            .position(|name| name == path)
            .unwrap_or(usize::MAX)
    };
    merged.sort_by_key(|finding| order(&finding.path));
    merged
}

/// The index as a tree id (`git write-tree`), or `None` when git cannot name it (an unmerged
/// index, which git would not commit either).
fn staged_tree(git: &Git, root: &Path, deadline: Instant) -> Option<String> {
    let output = git.run(root, ["write-tree"], deadline, 4096).ok()?;
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.success && valid_tree(&tree)).then_some(tree)
}

/// A tree id as git prints it: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits.
pub fn valid_tree(tree: &str) -> bool {
    matches!(tree.len(), 40 | 64)
        && tree
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The staged changes' files whose added lines seem to hold secrets, and whether the whole diff
/// was read. Filters, external diff drivers and text conversions do not run; at most
/// [`MAX_SCAN_BYTES`] are read within [`SCAN_TIMEOUT`].
pub fn staged_secrets(
    git: &Git,
    root: &Path,
    deadline: Instant,
) -> Result<StagedScan, ActionError> {
    let overrides = git::filter_overrides(git, root, deadline)?;
    let diff = |extra: &[&'static str], limit: usize| {
        let output = git.run(
            root,
            overrides
                .iter()
                .map(String::as_str)
                .chain(["-c", "core.quotePath=false"])
                .chain(git::DIFF_ARGS.iter().copied())
                .chain(["--cached", "--no-renames"])
                .chain(extra.iter().copied())
                .chain(["--"]),
            deadline,
            limit,
        )?;
        // stderr only: git's own message, never a line of the diff.
        if !output.usable() {
            return Err(ActionError::from(output.failure()));
        }
        Ok(output)
    };
    // Staged files whose name usually holds a secret (`.env` and the like), deleted ones aside.
    let listed = diff(
        &["--name-only", "-z", "--diff-filter=d"],
        git::MAX_LIST_BYTES,
    )?;
    let names: Vec<String> = listed
        .stdout
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect();
    // `--text`: a file git takes for binary (a NUL byte, an attribute) is read line by line too.
    let patch = diff(
        &[
            "--text",
            "--unified=0",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ],
        MAX_SCAN_BYTES,
    )?;
    let (content, scanned) = added_secrets(&patch.stdout, deadline);
    Ok(StagedScan {
        findings: merge_findings(&names, content),
        complete: scanned && !listed.truncated && !patch.truncated,
    })
}

/// The commit box's commit. The staged changes are checked first ([`staged_secrets`]) and
/// nothing is committed when they seem to hold a secret, unless `allow_tree` names the index as
/// it is now: the person saw exactly these findings and chose "Commit anyway". Any other index
/// is checked anew. A check that cannot run or takes too long lets the commit go ahead (it is a
/// helper, not a gate), and the answer says so.
pub fn commit_checked(
    git: &Git,
    cwd: &Path,
    message: &str,
    allow_tree: Option<&str>,
) -> Result<CommitOutcome, ActionError> {
    check_message(message)?;
    let deadline = Instant::now() + SCAN_TIMEOUT;
    let root = repo_root(git, cwd, deadline)?;
    let tree = staged_tree(git, &root, deadline);
    let allowed = allow_tree.filter(|allowed| valid_tree(allowed));
    let checked = if tree.is_some() && allowed == tree.as_deref() {
        Checked::Full
    } else {
        match staged_secrets(git, &root, deadline) {
            Ok(mut scan) if !scan.findings.is_empty() => {
                let more = scan.findings.len().saturating_sub(MAX_FINDINGS);
                scan.findings.truncate(MAX_FINDINGS);
                return Ok(CommitOutcome::Secrets {
                    findings: scan.findings,
                    more,
                    complete: scan.complete,
                    tree,
                });
            }
            Ok(scan) if scan.complete => Checked::Full,
            Ok(_) => Checked::Partial,
            // git's message (stderr) names no file content.
            Err(error) => {
                eprintln!("hangar: staged changes not checked for secrets: {error}");
                Checked::Skipped
            }
        }
    };
    commit(git, cwd, message)?;
    Ok(CommitOutcome::Committed { checked })
}

/// A commit message: not empty after trimming, at most [`MAX_MESSAGE_BYTES`], and no control
/// characters except tab and line breaks (an escape sequence would reach every log reader's
/// terminal).
pub fn check_message(message: &str) -> Result<&str, ActionError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(ActionError::EmptyMessage);
    }
    let control = |c: char| {
        let code = u32::from(c);
        (code < 0x20 && !matches!(c, '\t' | '\n' | '\r')) || (0x7f..=0x9f).contains(&code)
    };
    if message.len() > MAX_MESSAGE_BYTES || message.chars().any(control) {
        return Err(ActionError::InvalidMessage);
    }
    Ok(message)
}

/// Commits what is staged with `message` (read from stdin, so it is never an argument). Hooks and
/// signing run as configured; a missing identity or an empty index fails with git's message.
pub fn commit(git: &Git, cwd: &Path, message: &str) -> Result<(), ActionError> {
    let run = || {
        let message = check_message(message)?;
        let deadline = Instant::now() + COMMIT_TIMEOUT;
        let root = repo_root(git, cwd, deadline)?;
        let mut input = message.as_bytes().to_vec();
        input.push(b'\n');
        let output = git.with_repo_hooks().run_with_input(
            &root,
            ["commit", "--quiet", "--file=-"],
            Some(input),
            deadline,
            git::MAX_LIST_BYTES,
        )?;
        if !output.success {
            return Err(failure(&output).into());
        }
        Ok(())
    };
    run().map_err(explain_timeout)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::git::tests::{git, git_ok, init_repo, marker_filter};
    use crate::test_support::TempDir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn owned(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    /// Porcelain codes by path, as the panel would list them.
    fn codes(git: &Git, repo: &Path) -> Vec<(String, String)> {
        let entries = status(git, repo, &[], Instant::now() + git::TIMEOUT).unwrap();
        entries
            .into_iter()
            .map(|entry| (entry.code, entry.path))
            .collect()
    }

    fn pair(code: &str, path: &str) -> (String, String) {
        (code.to_owned(), path.to_owned())
    }

    /// A repository with one commit of `a.txt` and `b.txt`, identity and no signing set locally.
    fn committed_repo(git: &Git, tmp: &TempDir) -> PathBuf {
        let repo = tmp.path().join("repo with space");
        init_repo(git, &repo);
        for (key, value) in [
            ("user.name", "Hangar Test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            git_ok(git, &repo, &["config", key, value]);
        }
        write(&repo.join("a.txt"), "one\n");
        write(&repo.join("b.txt"), "two\n");
        git_ok(git, &repo, &["add", "-A"]);
        git_ok(git, &repo, &["commit", "--quiet", "-m", "first"]);
        repo
    }

    #[test]
    fn paths_must_stay_inside_the_repository() {
        for good in [
            "a.txt",
            "src/main.rs",
            "untracked folder/",
            "name with spaces.md",
            "ünïcode/file.txt",
            ".github/workflows/ci.yml",
            ".gitignore",
            "git~notes.txt",
            "git~/x",
            "-n",
            ":(glob)*",
        ] {
            if cfg!(windows) && good.contains(':') {
                continue;
            }
            assert!(check_path(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "\\server\\share",
            "../outside.txt",
            "a/../../outside.txt",
            "a/./b",
            "a//b",
            ".git/config",
            "sub/.GIT/hooks/pre-commit",
            "a\0b",
            // The NTFS short name and the HFS+ spellings of `.git`.
            "GIT~1/config",
            "sub/git~12/hooks/pre-commit",
            ".g\u{200c}it/config",
            "\u{feff}.GIT/config",
        ] {
            assert!(check_path(bad).is_err(), "{bad:?}");
        }
        assert!(check_path(&"x".repeat(MAX_PATH_BYTES + 1)).is_err());
        if cfg!(windows) {
            for bad in [
                "C:\\Windows",
                "C:x",
                "a.txt:stream",
                "a\\..\\..\\b",
                "... /x",
                ".git./x",
            ] {
                assert!(check_path(bad).is_err(), "{bad:?}");
            }
        }
        assert!(matches!(check_paths(&[]), Err(ActionError::NoPaths)));
        let many: Vec<String> = (0..=MAX_PATHS).map(|i| format!("f{i}")).collect();
        assert!(matches!(check_paths(&many), Err(ActionError::TooManyPaths)));
    }

    #[test]
    fn status_records_keep_the_original_path() {
        let entries = parse_status_entries(b"R  new.rs\0old.rs\0?? dir/\0 M a.txt\0");
        assert_eq!(
            entries,
            [
                StatusEntry {
                    code: "R ".into(),
                    path: "new.rs".into(),
                    original: Some("old.rs".into())
                },
                StatusEntry {
                    code: "??".into(),
                    path: "dir/".into(),
                    original: None
                },
                StatusEntry {
                    code: " M".into(),
                    path: "a.txt".into(),
                    original: None
                },
            ]
        );
    }

    #[test]
    fn filter_attributes() {
        assert!(!filtered(
            b"a.txt\0filter\0unspecified\0b.bin\0filter\0unset\0"
        ));
        assert!(filtered(
            b"a.txt\0filter\0unspecified\0big.psd\0filter\0lfs\0"
        ));
        assert!(!filtered(b""));
    }

    #[test]
    fn stage_and_unstage() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-stage");
        let repo = committed_repo(&git, &tmp);
        write(&repo.join("a.txt"), "one\nchanged\n");
        fs::remove_file(repo.join("b.txt")).unwrap();
        write(&repo.join("new folder/c.txt"), "three\n");
        write(&repo.join("-n"), "looks like an option\n");
        write(&repo.join("[ab].txt"), "looks like a glob\n");

        // From a subfolder; paths stay relative to the root.
        let sub = repo.join("new folder");
        stage(&git, &sub, &owned(&["a.txt", "b.txt", "new folder/", "-n"])).unwrap();
        let mut got = codes(&git, &repo);
        got.sort();
        let mut expected = vec![
            pair("??", "[ab].txt"),
            pair("A ", "-n"),
            pair("M ", "a.txt"),
            pair("D ", "b.txt"),
            pair("A ", "new folder/c.txt"),
        ];
        expected.sort();
        assert_eq!(got, expected);

        // `[ab].txt` is a literal name, not a glob that would stage `a.txt` and `b.txt`.
        unstage(&git, &repo, &owned(&["a.txt", "b.txt", "-n"])).unwrap();
        stage(&git, &repo, &owned(&["[ab].txt"])).unwrap();
        let mut got = codes(&git, &repo);
        got.sort();
        let mut expected = vec![
            pair("A ", "[ab].txt"),
            pair("??", "-n"),
            pair(" M", "a.txt"),
            pair(" D", "b.txt"),
            pair("A ", "new folder/c.txt"),
        ];
        expected.sort();
        assert_eq!(got, expected);
        assert_eq!(
            fs::read_to_string(repo.join("a.txt")).unwrap(),
            "one\nchanged\n"
        );

        // A rename is unstaged as a whole.
        git_ok(&git, &repo, &["mv", "a.txt", "renamed.txt"]);
        unstage(&git, &repo, &owned(&["renamed.txt"])).unwrap();
        let got = codes(&git, &repo);
        assert!(got.contains(&pair(" D", "a.txt")), "{got:?}");
        assert!(got.contains(&pair("??", "renamed.txt")), "{got:?}");
    }

    #[test]
    fn unstage_in_a_repository_without_commits() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-unborn");
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        write(&repo.join("a.txt"), "one\n");
        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        assert_eq!(codes(&git, &repo), [pair("A ", "a.txt")]);
        unstage(&git, &repo, &owned(&["a.txt"])).unwrap();
        assert_eq!(codes(&git, &repo), [pair("??", "a.txt")]);
        // Discarding a file that exists only in the index and the work tree removes it.
        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        discard(&git, &repo, &owned(&["a.txt"])).unwrap();
        assert!(!repo.join("a.txt").exists());
        assert!(codes(&git, &repo).is_empty());
    }

    #[test]
    fn discard_tracked_and_untracked() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-discard");
        let repo = committed_repo(&git, &tmp);
        // Staged and unstaged changes of one file, a deleted file, a file new in the index.
        write(&repo.join("a.txt"), "one\nstaged\n");
        git_ok(&git, &repo, &["add", "a.txt"]);
        write(&repo.join("a.txt"), "one\nstaged\nunstaged\n");
        fs::remove_file(repo.join("b.txt")).unwrap();
        write(&repo.join("added.txt"), "added\n");
        git_ok(&git, &repo, &["add", "added.txt"]);
        // Untracked: a file and a folder holding an ignored file, which stays.
        write(&repo.join("loose.txt"), "loose\n");
        write(&repo.join("scratch/notes.txt"), "scratch\n");
        write(&repo.join("scratch/build.log"), "ignored\n");
        write(&repo.join(".git/info/exclude"), "*.log\n");
        let kept = repo.join("kept.txt");
        write(&kept, "untouched\n");

        discard(
            &git,
            &repo,
            &owned(&["a.txt", "b.txt", "added.txt", "loose.txt", "scratch/"]),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "one\n");
        assert_eq!(fs::read_to_string(repo.join("b.txt")).unwrap(), "two\n");
        assert!(!repo.join("added.txt").exists());
        assert!(!repo.join("loose.txt").exists());
        assert!(!repo.join("scratch/notes.txt").exists());
        assert!(repo.join("scratch/build.log").exists());
        assert!(kept.exists());
        assert_eq!(codes(&git, &repo), [pair("??", "kept.txt")]);

        // A rename goes back as a whole; a path without changes is skipped.
        git_ok(&git, &repo, &["mv", "a.txt", "moved.txt"]);
        discard(&git, &repo, &owned(&["moved.txt", "b.txt"])).unwrap();
        assert!(repo.join("a.txt").exists());
        assert!(!repo.join("moved.txt").exists());
        assert_eq!(codes(&git, &repo), [pair("??", "kept.txt")]);
    }

    #[test]
    fn paths_outside_the_repository_are_refused_before_git_runs() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-outside");
        let repo = committed_repo(&git, &tmp);
        let outside = tmp.path().join("outside.txt");
        write(&outside, "keep me\n");
        for bad in ["../outside.txt", ".git/config", "/outside.txt"] {
            let paths = owned(&[bad]);
            assert!(matches!(
                stage(&git, &repo, &paths),
                Err(ActionError::InvalidPath(_))
            ));
            assert!(matches!(
                unstage(&git, &repo, &paths),
                Err(ActionError::InvalidPath(_))
            ));
            assert!(matches!(
                discard(&git, &repo, &paths),
                Err(ActionError::InvalidPath(_))
            ));
        }
        assert!(outside.exists());
        // No work tree: the test folder lies inside Hangar's own checkout, so only the git
        // directory itself is a folder outside any work tree (as in git.rs).
        let error = stage(&git, &repo.join(".git"), &owned(&["a.txt"])).unwrap_err();
        assert!(
            matches!(error, ActionError::Git(GitError::NotARepository)),
            "{error}"
        );
    }

    #[test]
    fn files_with_a_filter_are_refused() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-filter");
        let repo = committed_repo(&git, &tmp);
        write(&repo.join(".gitattributes"), "*.bin filter=marker\n");
        git_ok(&git, &repo, &["add", ".gitattributes"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "attributes"]);
        let marker = tmp.path().join("marker");
        let filter = marker_filter(&marker);
        git_ok(&git, &repo, &["config", "filter.marker.clean", &filter]);
        git_ok(&git, &repo, &["config", "filter.marker.smudge", &filter]);
        write(&repo.join("data/big.bin"), "binary\n");
        write(&repo.join("a.txt"), "one\nmore\n");

        assert!(matches!(
            stage(&git, &repo, &owned(&["data/"])),
            Err(ActionError::Filtered)
        ));
        assert!(matches!(
            stage(&git, &repo, &owned(&["data/big.bin"])),
            Err(ActionError::Filtered)
        ));
        // Files without the attribute still work, and no filter ran for them.
        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        discard(&git, &repo, &owned(&["a.txt"])).unwrap();
        assert!(!marker.exists(), "a repository filter ran");
    }

    #[test]
    fn commit_staged_changes() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-commit");
        let repo = committed_repo(&git, &tmp);
        write(&repo.join("a.txt"), "one\nnew\n");
        write(&repo.join("b.txt"), "two\nnot staged\n");
        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        commit(&git, &repo, "  feat: a change\n\n--looks like an option\n").unwrap();
        let log = git
            .run(
                &repo,
                ["log", "-1", "--format=%B"],
                Instant::now() + git::TIMEOUT,
                4096,
            )
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&log.stdout).trim_end(),
            "feat: a change\n\n--looks like an option"
        );
        assert_eq!(codes(&git, &repo), [pair(" M", "b.txt")]);

        // Nothing staged: git's own message, from stdout.
        let error = commit(&git, &repo, "nothing").unwrap_err();
        assert!(error.to_string().contains("no changes added"), "{error}");
        assert!(matches!(
            commit(&git, &repo, " \n "),
            Err(ActionError::EmptyMessage)
        ));
        for bad in ["a\0b", "a\x1b[31mb", "a\x7fb", "a\u{9b}31mb"] {
            assert!(
                matches!(commit(&git, &repo, bad), Err(ActionError::InvalidMessage)),
                "{bad:?}"
            );
        }
        assert!(check_message("feat: x\r\n\n\tbody").is_ok());
    }

    /// A made-up GitHub-shaped token, put together at run time so no complete shape is stored.
    fn fake_token() -> String {
        format!("gh{}_{}", "p", "Fake0Marker1Value2Test3Hangar4Only5N")
    }

    #[test]
    fn added_lines_with_a_secret_name_their_file_and_kind_only() {
        let token = fake_token();
        let diff = [
            "diff --git a/src/config.rs b/src/config.rs",
            "index 1..2 100644",
            "--- a/src/config.rs",
            "+++ b/src/config.rs",
            "@@ -1 +1 @@",
            &format!("-let old = \"{token}\";"),
            "+let token = load();",
            "diff --git a/deploy/app env.txt b/deploy/app env.txt",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/deploy/app env.txt",
            "@@ -0,0 +1,2 @@",
            &format!("+TOKEN={token}\r"),
            "++++ looks like a header",
            "diff --git a/gone.txt b/gone.txt",
            "deleted file mode 100644",
            "--- a/gone.txt",
            "+++ /dev/null",
            "@@ -1 +0,0 @@",
            &format!("-{token}"),
            r#"diff --git "a/tab\there.txt" "b/tab\there.txt""#,
            "--- /dev/null",
            r#"+++ "b/tab\there.txt""#,
            "@@ -0,0 +1 @@",
            &format!("+{token}"),
            "",
        ]
        .join("\n");
        let (findings, complete) = added_secrets(diff.as_bytes(), Instant::now() + SCAN_TIMEOUT);
        assert!(complete);
        assert_eq!(
            findings,
            vec![
                SecretFinding {
                    path: "deploy/app env.txt".into(),
                    kinds: vec![SecretKind::GithubToken],
                },
                SecretFinding {
                    path: "tab\there.txt".into(),
                    kinds: vec![SecretKind::GithubToken],
                },
            ]
        );
        let shown = format!("{findings:?} {}", serde_json::to_string(&findings).unwrap());
        assert!(
            !shown.contains("Marker1"),
            "the token leaked into the shown findings"
        );
        assert_eq!(
            added_secrets(b"", Instant::now() + SCAN_TIMEOUT),
            (Vec::new(), true)
        );
        // Past the deadline nothing more is scanned and the result says so.
        assert_eq!(
            added_secrets(diff.as_bytes(), Instant::now()),
            (Vec::new(), false)
        );
    }

    #[test]
    fn a_commit_with_a_staged_secret_waits_for_the_person() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-secret");
        let repo = committed_repo(&git, &tmp);
        write(
            &repo.join("a.txt"),
            &format!("one\nkey = {}\n", fake_token()),
        );
        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        let outcome = commit_checked(&git, &repo, "feat: key", None).unwrap();
        let CommitOutcome::Secrets {
            findings,
            more,
            complete,
            tree: Some(tree),
        } = outcome.clone()
        else {
            panic!("{outcome:?}");
        };
        assert_eq!(
            findings,
            vec![SecretFinding {
                path: "a.txt".into(),
                kinds: vec![SecretKind::GithubToken],
            }]
        );
        assert_eq!((more, complete), (0, true));
        assert!(valid_tree(&tree), "{tree}");
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["kind"], "secrets");
        assert_eq!(json["findings"][0]["kinds"][0], "githubToken");
        assert_eq!(json["tree"], tree.as_str());
        // Nothing was committed.
        assert_eq!(codes(&git, &repo), [pair("M ", "a.txt")]);
        // "Commit anyway" for another index (here: a made-up tree) checks again and asks again.
        let other = "0".repeat(40);
        assert!(matches!(
            commit_checked(&git, &repo, "feat: key", Some(&other)).unwrap(),
            CommitOutcome::Secrets { .. }
        ));
        // A file staged after the question changes the index: the old answer does not cover it.
        write(&repo.join("b.txt"), &format!("two\n{}\n", fake_token()));
        stage(&git, &repo, &owned(&["b.txt"])).unwrap();
        let again = commit_checked(&git, &repo, "feat: key", Some(&tree)).unwrap();
        let CommitOutcome::Secrets {
            findings,
            tree: Some(new_tree),
            ..
        } = again
        else {
            panic!("{again:?}");
        };
        assert_eq!(findings.len(), 2);
        assert_ne!(new_tree, tree);
        // The answer for exactly this index commits it.
        assert_eq!(
            commit_checked(&git, &repo, "feat: key", Some(&new_tree)).unwrap(),
            CommitOutcome::Committed {
                checked: Checked::Full
            }
        );
        assert!(codes(&git, &repo).is_empty());
        // A change without a secret commits at once, fully checked.
        write(&repo.join("a.txt"), "one\nplain\n");
        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        let committed = commit_checked(&git, &repo, "fix: plain", None).unwrap();
        assert_eq!(
            committed,
            CommitOutcome::Committed {
                checked: Checked::Full
            }
        );
        assert_eq!(
            serde_json::to_value(&committed).unwrap(),
            serde_json::json!({ "kind": "committed", "checked": "full" })
        );
        // The message is checked before the scan.
        assert!(matches!(
            commit_checked(&git, &repo, " ", None),
            Err(ActionError::EmptyMessage)
        ));
    }

    #[test]
    fn a_staged_secret_file_and_a_binary_file_with_a_key_are_found() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-secret-file");
        let repo = committed_repo(&git, &tmp);
        write(&repo.join(".env"), "MARKER=harmless\n");
        write(&repo.join(".env.example"), "MARKER=\n");
        // A NUL byte makes git take the file for binary; `--text` still shows its lines.
        std::fs::write(
            repo.join("blob.bin"),
            format!("\0head\n{}\n", fake_token()).as_bytes(),
        )
        .unwrap();
        stage(&git, &repo, &owned(&[".env", ".env.example", "blob.bin"])).unwrap();
        let scan = staged_secrets(&git, &repo, Instant::now() + SCAN_TIMEOUT).unwrap();
        assert!(scan.complete);
        assert_eq!(
            scan.findings,
            vec![
                SecretFinding {
                    path: ".env".into(),
                    kinds: vec![SecretKind::SecretFile],
                },
                SecretFinding {
                    path: "blob.bin".into(),
                    kinds: vec![SecretKind::GithubToken],
                },
            ]
        );
    }

    #[test]
    fn name_and_content_findings_merge_per_file() {
        let names = vec![
            "a.txt".to_owned(),
            "x/id_rsa".to_owned(),
            "c.txt".to_owned(),
        ];
        let content = vec![
            SecretFinding {
                path: "c.txt".into(),
                kinds: vec![SecretKind::StripeKey],
            },
            SecretFinding {
                path: "x/id_rsa".into(),
                kinds: vec![SecretKind::PrivateKey],
            },
        ];
        assert_eq!(
            merge_findings(&names, content),
            vec![
                SecretFinding {
                    path: "x/id_rsa".into(),
                    kinds: vec![SecretKind::PrivateKey, SecretKind::SecretFile],
                },
                SecretFinding {
                    path: "c.txt".into(),
                    kinds: vec![SecretKind::StripeKey],
                },
            ]
        );
        assert!(valid_tree(&"a".repeat(40)));
        assert!(valid_tree(&"0".repeat(64)));
        assert!(!valid_tree(&"A".repeat(40)));
        assert!(!valid_tree("HEAD"));
    }

    #[test]
    fn commit_without_an_identity_fails_with_gits_message() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-identity");
        let repo = committed_repo(&git, &tmp);
        // An empty identity in the repository's own config wins over a global one.
        git_ok(&git, &repo, &["config", "user.name", ""]);
        git_ok(&git, &repo, &["config", "user.email", ""]);
        write(&repo.join("a.txt"), "one\nnew\n");
        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        let error = commit(&git, &repo, "fix: something").unwrap_err();
        assert!(
            matches!(error, ActionError::Git(GitError::Failed(_))),
            "{error}"
        );
    }

    /// A hook script that writes `marker` when it runs (git runs hooks through its shell; Git for
    /// Windows brings its own `sh`).
    fn marker_hook(repo: &Path, name: &str, marker: &Path) {
        let marker = marker.to_string_lossy().replace('\\', "/");
        let hook = repo.join(".git/hooks").join(name);
        write(&hook, &format!("#!/bin/sh\necho hit > '{marker}'\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn stage_unstage_and_discard_run_no_hooks_but_commit_does() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-hooks");
        let repo = committed_repo(&git, &tmp);
        let marker = tmp.path().join("index-hook-ran");
        marker_hook(&repo, "post-index-change", &marker);
        write(&repo.join("a.txt"), "one\nchanged\n");

        // The test is only meaningful if git with the repository's hooks runs the hook here.
        git_ok(&git.with_repo_hooks(), &repo, &["add", "a.txt"]);
        assert!(marker.exists(), "git did not run the hook; test is void");
        git_ok(&git, &repo, &["reset", "--quiet"]);
        fs::remove_file(&marker).unwrap();

        stage(&git, &repo, &owned(&["a.txt"])).unwrap();
        unstage(&git, &repo, &owned(&["a.txt"])).unwrap();
        discard(&git, &repo, &owned(&["a.txt"])).unwrap();
        assert!(!marker.exists(), "a repository hook ran");

        // A commit keeps the repository's hooks.
        let committed = tmp.path().join("commit-hook-ran");
        marker_hook(&repo, "post-commit", &committed);
        write(&repo.join("b.txt"), "two\nchanged\n");
        stage(&git, &repo, &owned(&["b.txt"])).unwrap();
        commit(&git, &repo, "test: hooks").unwrap();
        assert!(committed.exists(), "the commit hook did not run");
    }

    /// Git switches hooks off with a `core.hooksPath` no hook can live under; a second one from
    /// the caller would replace it.
    #[test]
    fn stage_names_core_hooks_path_once() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-hooks-path");
        let repo = committed_repo(&git, &tmp);
        let opened = Repo::open(&git, &repo, 1).unwrap();
        let args: Vec<&str> = opened
            .overrides
            .iter()
            .map(String::as_str)
            .chain(["add", "--all"])
            .collect();
        let argv = git.test_argv(&repo, &args);
        let named = argv
            .iter()
            .filter(|arg| arg.to_ascii_lowercase().starts_with("core.hookspath"))
            .count();
        assert_eq!(named, 1, "{argv:?}");
    }

    #[test]
    fn timeouts_get_their_own_message() {
        let error = explain_timeout(ActionError::Git(GitError::Timeout));
        assert!(matches!(error, ActionError::TimedOut));
        assert!(error.to_string().contains(".git/index.lock"));
        assert!(matches!(
            explain_timeout(ActionError::NoPaths),
            ActionError::NoPaths
        ));
    }

    /// Creates a link at `link` to the folder `target`: a junction on Windows (`mklink /J`, what
    /// Git for Windows may take for a plain folder), a symbolic link elsewhere. Both lie in the
    /// test's own folder under tests/.tmp.
    fn folder_link(link: &Path, target: &Path) {
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "mklink /J failed");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    /// Discard never deletes or writes through a link: not an untracked link itself, not a folder
    /// holding one, not a tracked file whose parent folder became a link.
    #[test]
    fn discard_refuses_paths_through_links() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-links");
        let repo = committed_repo(&git, &tmp);
        let target = tmp.path().join("outside target");
        let kept = target.join("keep.txt");
        write(&kept, "must survive\n");

        // An untracked link to a folder outside the work tree.
        folder_link(&repo.join("linked"), &target);
        // An untracked folder that holds such a link.
        fs::create_dir_all(repo.join("holder")).unwrap();
        write(&repo.join("holder/own.txt"), "own\n");
        folder_link(&repo.join("holder/inner"), &target);
        // A tracked file whose folder is replaced by a link.
        write(&repo.join("sub/c.txt"), "three\n");
        git_ok(&git, &repo, &["add", "sub/c.txt"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "sub"]);
        fs::remove_dir_all(repo.join("sub")).unwrap();
        folder_link(&repo.join("sub"), &target);

        let listed = codes(&git, &repo);
        for (code, path) in &listed {
            if code == "??" || path.starts_with("sub") {
                let error = discard(&git, &repo, std::slice::from_ref(path)).unwrap_err();
                assert!(matches!(error, ActionError::Linked(_)), "{path}: {error}");
            }
        }
        // Even if git did not list the paths this way, the check itself refuses them.
        for path in ["linked/", "holder/", "sub/c.txt", "sub/"] {
            assert!(
                matches!(refuse_links(&repo, path), Err(ActionError::Linked(_))),
                "{path}"
            );
        }
        assert_eq!(fs::read_to_string(&kept).unwrap(), "must survive\n");
        assert!(repo.join("holder/own.txt").exists());
        // A plain folder and a missing file pass.
        refuse_links(&repo, "a.txt").unwrap();
        refuse_links(&repo, "gone/missing.txt").unwrap();
        fs::create_dir_all(repo.join("plain/deep")).unwrap();
        refuse_links(&repo, "plain/").unwrap();
    }
}
