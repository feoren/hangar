//! Write actions of the Changes panel: stage, unstage, discard and commit
//! (contract: docs/wiki/pages/syntheses/commit-history.md, "Git actions").
//!
//! The window names a session and paths relative to the repository root, exactly as `git_diff`
//! listed them; the folder comes from the session manager. Every call runs through the hardened
//! runner of git.rs. Paths are checked to stay inside the work tree before git sees them and are
//! handed to git as literal pathspecs on stdin (`--literal-pathspecs`, `--pathspec-from-file`), so
//! no path is ever read as an option, a glob or pathspec magic, and no list overflows the command
//! line. Stage, unstage and discard switch the repository's filter drivers and hooks off (hooks
//! point at an empty folder Hangar owns), and refuse files that a filter would have to convert
//! (Git LFS, for example): running them without their filter would store or write the wrong
//! content. Discard also refuses any path that leads through or contains a link (symbolic link,
//! junction), where git could delete or write outside the work tree. A commit runs the
//! repository's hooks like a commit in the terminal does; nothing is bypassed there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};

use crate::git::{self, Git, GitError, Output};
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
/// Commands without a pathspec file (`ls-files`, `clean`) take their paths as arguments; a few
/// per call keep the command line short.
const CLEAN_CHUNK: usize = 64;
/// Folder below Hangar's local data folder that stage, unstage and discard name as
/// `core.hooksPath`: it stays empty, so no hook of the repository runs for them.
pub const NO_HOOKS_FOLDER: &str = "git-no-hooks";

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
    #[error("Hangar's empty hooks folder cannot be used: {0}")]
    NoHooksFolder(String),
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

/// The empty hooks folder below `data_dir`, created on first use. A folder that holds anything
/// is refused, so nothing placed there can run as a hook.
pub fn no_hooks_folder(data_dir: &Path) -> Result<PathBuf, ActionError> {
    let folder = data_dir.join(NO_HOOKS_FOLDER);
    let problem = |error: io::Error| ActionError::NoHooksFolder(error.to_string());
    fs::create_dir_all(&folder).map_err(problem)?;
    if fs::read_dir(&folder).map_err(problem)?.next().is_some() {
        return Err(ActionError::NoHooksFolder("it is not empty".to_owned()));
    }
    Ok(folder)
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
        if effective.eq_ignore_ascii_case(".git") {
            return Err(ActionError::InvalidPath("inside a git folder"));
        }
    }
    Ok(())
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
    /// Filter overrides and `core.hooksPath`; must come before the subcommand.
    overrides: Vec<String>,
    /// A filter driver is configured, so files have to be checked for filter attributes.
    filters: bool,
}

impl Repo {
    /// The work tree of the session's folder with the folder checks of git.rs, and the
    /// overrides for it. The deadline grows with the number of paths.
    fn open(git: &Git, cwd: &Path, no_hooks: &Path, paths: usize) -> Result<Self, ActionError> {
        let per_path =
            ACTION_TIMEOUT_PER_PATH.saturating_mul(u32::try_from(paths).unwrap_or(u32::MAX));
        let deadline = Instant::now() + ACTION_BASE_TIMEOUT + per_path;
        let root = repo_root(git, cwd, deadline)?;
        let filters = git::filter_overrides(git, &root, deadline)?;
        let mut overrides = vec![
            "-c".to_owned(),
            format!("core.hooksPath={}", no_hooks.display()),
        ];
        let any_filter = !filters.is_empty();
        overrides.extend(filters);
        Ok(Self {
            root,
            deadline,
            overrides,
            filters: any_filter,
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
    if files.is_empty() {
        return Ok(());
    }
    let attributes = git.run_with_input(
        &repo.root,
        ["check-attr", "-z", "--stdin", "filter"],
        Some(files),
        repo.deadline,
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
pub fn stage(git: &Git, no_hooks: &Path, cwd: &Path, paths: &[String]) -> Result<(), ActionError> {
    let run = || {
        check_paths(paths)?;
        let repo = Repo::open(git, cwd, no_hooks, paths.len())?;
        refuse_filtered(git, &repo, paths)?;
        run_with_paths(git, &repo, &["add", "--all"], paths)?;
        Ok(())
    };
    run().map_err(explain_timeout)
}

/// `git reset` for the paths: the index takes them from HEAD again (or drops them in a repository
/// without commits); the work tree stays as it is. Hooks off.
pub fn unstage(
    git: &Git,
    no_hooks: &Path,
    cwd: &Path,
    paths: &[String],
) -> Result<(), ActionError> {
    let run = || {
        check_paths(paths)?;
        let repo = Repo::open(git, cwd, no_hooks, paths.len())?;
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
pub fn discard(
    git: &Git,
    no_hooks: &Path,
    cwd: &Path,
    paths: &[String],
) -> Result<(), ActionError> {
    let run = || {
        check_paths(paths)?;
        let repo = Repo::open(git, cwd, no_hooks, paths.len())?;
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
        let output = git.run_with_input(
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

    /// The empty hooks folder, inside the test's own folder.
    fn hooks(tmp: &TempDir) -> PathBuf {
        no_hooks_folder(tmp.path()).unwrap()
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
        stage(
            &git,
            &hooks(&tmp),
            &sub,
            &owned(&["a.txt", "b.txt", "new folder/", "-n"]),
        )
        .unwrap();
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
        unstage(&git, &hooks(&tmp), &repo, &owned(&["a.txt", "b.txt", "-n"])).unwrap();
        stage(&git, &hooks(&tmp), &repo, &owned(&["[ab].txt"])).unwrap();
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
        unstage(&git, &hooks(&tmp), &repo, &owned(&["renamed.txt"])).unwrap();
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
        stage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        assert_eq!(codes(&git, &repo), [pair("A ", "a.txt")]);
        unstage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        assert_eq!(codes(&git, &repo), [pair("??", "a.txt")]);
        // Discarding a file that exists only in the index and the work tree removes it.
        stage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        discard(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
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
            &hooks(&tmp),
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
        discard(&git, &hooks(&tmp), &repo, &owned(&["moved.txt", "b.txt"])).unwrap();
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
                stage(&git, &hooks(&tmp), &repo, &paths),
                Err(ActionError::InvalidPath(_))
            ));
            assert!(matches!(
                unstage(&git, &hooks(&tmp), &repo, &paths),
                Err(ActionError::InvalidPath(_))
            ));
            assert!(matches!(
                discard(&git, &hooks(&tmp), &repo, &paths),
                Err(ActionError::InvalidPath(_))
            ));
        }
        assert!(outside.exists());
        // No work tree: the test folder lies inside Hangar's own checkout, so only the git
        // directory itself is a folder outside any work tree (as in git.rs).
        let error = stage(&git, &hooks(&tmp), &repo.join(".git"), &owned(&["a.txt"])).unwrap_err();
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
            stage(&git, &hooks(&tmp), &repo, &owned(&["data/"])),
            Err(ActionError::Filtered)
        ));
        assert!(matches!(
            stage(&git, &hooks(&tmp), &repo, &owned(&["data/big.bin"])),
            Err(ActionError::Filtered)
        ));
        // Files without the attribute still work, and no filter ran for them.
        stage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        discard(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        assert!(!marker.exists(), "a repository filter ran");
    }

    #[test]
    fn commit_staged_changes() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-commit");
        let repo = committed_repo(&git, &tmp);
        write(&repo.join("a.txt"), "one\nnew\n");
        write(&repo.join("b.txt"), "two\nnot staged\n");
        stage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
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

    #[test]
    fn commit_without_an_identity_fails_with_gits_message() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-actions-identity");
        let repo = committed_repo(&git, &tmp);
        // An empty identity in the repository's own config wins over a global one.
        git_ok(&git, &repo, &["config", "user.name", ""]);
        git_ok(&git, &repo, &["config", "user.email", ""]);
        write(&repo.join("a.txt"), "one\nnew\n");
        stage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
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

        // The test is only meaningful if plain git runs the hook here.
        git_ok(&git, &repo, &["add", "a.txt"]);
        assert!(marker.exists(), "git did not run the hook; test is void");
        git_ok(&git, &repo, &["reset", "--quiet"]);
        fs::remove_file(&marker).unwrap();

        stage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        unstage(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        discard(&git, &hooks(&tmp), &repo, &owned(&["a.txt"])).unwrap();
        assert!(!marker.exists(), "a repository hook ran");
        assert!(fs::read_dir(hooks(&tmp)).unwrap().next().is_none());

        // A commit keeps the repository's hooks.
        let committed = tmp.path().join("commit-hook-ran");
        marker_hook(&repo, "post-commit", &committed);
        write(&repo.join("b.txt"), "two\nchanged\n");
        stage(&git, &hooks(&tmp), &repo, &owned(&["b.txt"])).unwrap();
        commit(&git, &repo, "test: hooks").unwrap();
        assert!(committed.exists(), "the commit hook did not run");
    }

    #[test]
    fn a_hooks_folder_with_content_is_refused() {
        let tmp = TempDir::new("git-actions-no-hooks");
        let folder = no_hooks_folder(tmp.path()).unwrap();
        assert!(folder.is_dir());
        write(&folder.join("pre-commit"), "#!/bin/sh\n");
        assert!(matches!(
            no_hooks_folder(tmp.path()),
            Err(ActionError::NoHooksFolder(_))
        ));
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
                let error =
                    discard(&git, &hooks(&tmp), &repo, std::slice::from_ref(path)).unwrap_err();
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
