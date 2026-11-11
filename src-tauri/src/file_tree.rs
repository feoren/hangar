//! File tree of a session's working folder for the side panel: one folder per call, listed lazily
//! as the user expands it.
//!
//! The frontend only names a path below the session's folder; it can never name another root.
//! Paths are checked as text first (no absolute paths, drive letters, UNC prefixes, `..`, `.git`),
//! then walked segment by segment without following any symlink or junction, and finally
//! resolved and compared with the resolved root; the resolved path is checked for `.git` again,
//! because a short name (`GIT~1` on Windows) reaches it without spelling it. Links are listed as
//! entries but never entered: following one could lead out of the folder or to a network share,
//! where Windows would hand the user's credentials to another host.
//!
//! Ignored entries come from one `git check-ignore --stdin -z` per listing, only when git confirmed
//! the repository's top level for the root ([`git::toplevel`], cached per root); without git or on
//! any git failure nothing is marked ignored.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};

use crate::git::{self, Git};
use crate::lock;
use crate::session::SessionId;

/// Time the window waits for one listing. A folder on a share that stopped answering would keep
/// its thread; the window gets an error instead, and [`InFlight`] keeps further requests for the
/// same folder from piling up threads behind it.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
/// Reading the folder's entries stops after this; the listing is then marked truncated.
const READ_TIME: Duration = Duration::from_secs(5);
/// How long git's answer about a root's top level is reused; a `git init` shows up after this.
const TOP_LEVEL_TTL: Duration = Duration::from_secs(60);
/// Roots kept in the top level cache; beyond this it starts over.
const MAX_CACHED_ROOTS: usize = 256;

/// Entries per folder handed to the window; the rest is cut and `truncated` set.
pub const MAX_ENTRIES: usize = 2000;
/// Entries read from one folder at most. Beyond this the folder is not read further, so a folder
/// with millions of files cannot stall the panel; the listing is then marked truncated.
const MAX_SCANNED: usize = 100_000;
/// Output of `git check-ignore`: at most every listed name once.
const MAX_IGNORE_OUTPUT: usize = 4 * 1024 * 1024;
const GIT_DIR_NAME: &str = ".git";

#[derive(Debug, thiserror::Error)]
pub enum FileTreeError {
    #[error("Unknown session {0}")]
    UnknownSession(SessionId),
    #[error("Invalid folder path: {0}")]
    InvalidPath(&'static str),
    #[error("The folder lies outside the session's folder")]
    OutsideRoot,
    #[error("Links are not followed in the file tree")]
    Link,
    #[error("Not a folder")]
    NotAFolder,
    #[error("The folder cannot be read: {0}")]
    Io(#[source] io::Error),
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error("The folder is still being read")]
    Busy,
    #[error("The folder did not answer in time")]
    TimedOut,
}

impl Serialize for FileTreeError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    Dir,
    File,
    /// Symlink or junction; listed, never entered.
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    /// File name; names that are not valid Unicode are converted lossily.
    pub name: String,
    pub kind: EntryKind,
    /// git ignores the entry (`.gitignore`, `info/exclude`, `core.excludesFile`).
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderListing {
    /// The session's folder, resolved, without a `\\?\` prefix.
    pub root: String,
    /// Top level of the repository that contains `root`, in the same form; `None` outside one.
    /// The frontend maps repository relative paths (team presence) onto the tree with it.
    pub repo_root: Option<String>,
    pub entries: Vec<FolderEntry>,
    /// More than [`MAX_ENTRIES`] entries; only the first ones in sort order are listed.
    pub truncated: bool,
}

/// Checks `relative` as text and splits it into its segments; empty means the root.
pub fn segments(relative: &str) -> Result<Vec<&str>, FileTreeError> {
    if relative.is_empty() {
        return Ok(Vec::new());
    }
    if relative.starts_with('/') {
        return Err(FileTreeError::InvalidPath("absolute path"));
    }
    if relative.contains('\0') {
        return Err(FileTreeError::InvalidPath("NUL character"));
    }
    // A backslash would be a separator on Windows and a `:` names a drive or an alternate data
    // stream there; both are refused everywhere so a path means the same on every platform.
    if relative.contains('\\') {
        return Err(FileTreeError::InvalidPath("only / separates folders"));
    }
    if relative.contains(':') {
        return Err(FileTreeError::InvalidPath("drive or stream name"));
    }
    relative
        .split('/')
        .map(|segment| match segment {
            "" => Err(FileTreeError::InvalidPath("empty folder name")),
            "." | ".." => Err(FileTreeError::InvalidPath("relative folder name")),
            _ if segment.eq_ignore_ascii_case(GIT_DIR_NAME) => {
                Err(FileTreeError::InvalidPath("git folder"))
            }
            _ => Ok(segment),
        })
        .collect()
}

/// The resolved root and the resolved folder `relative` names below it.
pub fn resolve(root: &Path, relative: &str) -> Result<(PathBuf, PathBuf), FileTreeError> {
    let parts = segments(relative)?;
    let root = fs::canonicalize(root).map_err(FileTreeError::Io)?;
    if !root.is_dir() {
        return Err(FileTreeError::NotAFolder);
    }
    let mut folder = root.clone();
    for part in parts {
        folder.push(part);
        let meta = fs::symlink_metadata(&folder).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => FileTreeError::NotAFolder,
            _ => FileTreeError::Io(error),
        })?;
        if meta.file_type().is_symlink() {
            return Err(FileTreeError::Link);
        }
        if !meta.is_dir() {
            return Err(FileTreeError::NotAFolder);
        }
    }
    // No link was passed, so this only normalizes (case, short names); it still guards against
    // anything the walk above did not foresee.
    let folder = fs::canonicalize(&folder).map_err(FileTreeError::Io)?;
    let inside = folder
        .strip_prefix(&root)
        .map_err(|_| FileTreeError::OutsideRoot)?;
    // A short name such as `GIT~1` passes the text check and resolves to `.git` only here.
    if reaches_git_folder(inside) {
        return Err(FileTreeError::InvalidPath("git folder"));
    }
    Ok((root, folder))
}

/// A resolved path below the root passes through a `.git` folder (also a submodule's), or holds
/// anything but plain names.
fn reaches_git_folder(inside: &Path) -> bool {
    inside.components().any(|part| match part {
        Component::Normal(name) => name.eq_ignore_ascii_case(GIT_DIR_NAME),
        _ => true,
    })
}

type InFlightKey = (SessionId, String);

/// Listings that are running, per session and folder. A second request for a folder whose
/// listing still runs (a share that stopped answering) is refused instead of starting another
/// thread that would hang as well.
#[derive(Clone, Default)]
pub struct InFlight(Arc<Mutex<HashSet<InFlightKey>>>);

/// Held by the thread that lists the folder; releases it when that thread ends.
pub struct Claim {
    set: Arc<Mutex<HashSet<InFlightKey>>>,
    key: InFlightKey,
}

impl InFlight {
    pub fn claim(&self, id: SessionId, relative: &str) -> Result<Claim, FileTreeError> {
        let key = (id, relative.to_owned());
        if !lock(&self.0).insert(key.clone()) {
            return Err(FileTreeError::Busy);
        }
        Ok(Claim {
            set: Arc::clone(&self.0),
            key,
        })
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        lock(&self.set).remove(&self.key);
    }
}

/// The file tree's state for the app: running listings and git's top level per root.
#[derive(Clone, Default)]
pub struct FileTree {
    pub in_flight: InFlight,
    tops: Arc<Mutex<TopLevels>>,
}

/// git's top level per resolved root, with the time it was asked.
type TopLevels = HashMap<PathBuf, (Instant, Option<PathBuf>)>;

impl FileTree {
    /// Lists the folder `relative` below the session folder `root`. `git` decides the `ignored`
    /// flags and the repository root; without it nothing is marked and the root is looked up on
    /// the file system.
    pub fn list(
        &self,
        root: &Path,
        relative: &str,
        git: Option<&Git>,
    ) -> Result<FolderListing, FileTreeError> {
        self.list_limited(root, relative, git, MAX_ENTRIES)
    }

    fn list_limited(
        &self,
        root: &Path,
        relative: &str,
        git: Option<&Git>,
        limit: usize,
    ) -> Result<FolderListing, FileTreeError> {
        let (root, folder) = resolve(root, relative)?;
        let (mut entries, mut truncated) = read_entries(&folder, Instant::now() + READ_TIME)?;
        // The folder could have been swapped for a link while it was read.
        if fs::canonicalize(&folder).ok().as_ref() != Some(&folder) {
            return Err(FileTreeError::Link);
        }
        sort_entries(&mut entries);
        if entries.len() > limit {
            entries.truncate(limit);
            truncated = true;
        }
        let repo_root = match git {
            Some(git) => {
                let top = self.top_level(git, &root);
                if top.is_some() {
                    mark_ignored(git, &folder, &mut entries);
                }
                top
            }
            None => repo_root(&root),
        };
        Ok(FolderListing {
            root: display_path(&root),
            repo_root: repo_root.as_deref().map(display_path),
            entries,
            truncated,
        })
    }

    /// git's top level for `root` with the work tree checks of [`git::toplevel`], resolved like
    /// the root so the window can strip one from the other; `None` outside a repository or when
    /// git refuses or fails. Reused for [`TOP_LEVEL_TTL`].
    fn top_level(&self, git: &Git, root: &Path) -> Option<PathBuf> {
        let now = Instant::now();
        if let Some((at, top)) = lock(&self.tops).get(root) {
            if now.duration_since(*at) < TOP_LEVEL_TTL {
                return top.clone();
            }
        }
        // Not under the lock: git may take seconds.
        let top = git::toplevel(git, root, now + git::TIMEOUT)
            .ok()
            .flatten()
            .and_then(|top| fs::canonicalize(top).ok());
        let mut tops = lock(&self.tops);
        if tops.len() >= MAX_CACHED_ROOTS {
            tops.clear();
        }
        tops.insert(root.to_path_buf(), (now, top.clone()));
        top
    }
}

fn read_entries(
    folder: &Path,
    deadline: Instant,
) -> Result<(Vec<FolderEntry>, bool), FileTreeError> {
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in fs::read_dir(folder).map_err(FileTreeError::Io)? {
        if entries.len() >= MAX_SCANNED || Instant::now() >= deadline {
            truncated = true;
            break;
        }
        // An entry that vanished or cannot be read is left out; the rest stays listable.
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case(GIT_DIR_NAME) {
            continue;
        }
        // DirEntry::file_type does not follow links, so a junction or symlink stays a link.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        };
        entries.push(FolderEntry {
            name,
            kind,
            ignored: false,
        });
    }
    Ok((entries, truncated))
}

/// Folders first, then files and links, each in natural order ([`natural_cmp`]).
pub fn sort_entries(entries: &mut [FolderEntry]) {
    entries.sort_by(|a, b| {
        let group = |entry: &FolderEntry| u8::from(entry.kind != EntryKind::Dir);
        group(a)
            .cmp(&group(b))
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
}

/// Case-insensitive order with digit runs compared by value, like a file explorer: `file2`
/// before `file10`, `a` next to `A`. Names equal in that order fall back to plain comparison so
/// the order is total (`file02` before `File2`, `A` before `a`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => {
                return a
                    .to_lowercase()
                    .cmp(&b.to_lowercase())
                    .then_with(|| a.cmp(b));
            }
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) if l.is_ascii_digit() && r.is_ascii_digit() => {
                let l = take_digits(&mut left);
                let r = take_digits(&mut right);
                let (l, r) = (l.trim_start_matches('0'), r.trim_start_matches('0'));
                let order = l.len().cmp(&r.len()).then_with(|| l.cmp(r));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(l), Some(r)) => {
                left.next();
                right.next();
                let order = l.to_lowercase().cmp(r.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut digits = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        digits.push(c);
    }
    digits
}

/// The nearest folder at or above `root` that holds a `.git` folder or file (a linked worktree
/// or submodule), read from the file system without starting git.
fn repo_root(root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .find(|dir| fs::symlink_metadata(dir.join(GIT_DIR_NAME)).is_ok())
        .map(Path::to_path_buf)
}

/// Sets `ignored` from one `git check-ignore` call in `folder`. Names that are not valid UTF-8
/// cannot be passed to git and stay unmarked, as does everything when git fails.
fn mark_ignored(git: &Git, folder: &Path, entries: &mut [FolderEntry]) {
    if entries.is_empty() || git::check_git_links(folder).is_err() {
        return;
    }
    let mut input = Vec::new();
    for entry in entries.iter() {
        if entry.name.contains('\u{FFFD}') {
            continue;
        }
        // check-ignore refuses `--literal-pathspecs`; magic is only read at the start of a
        // pathspec, so `./` keeps a name like `:(glob)x` a plain name. git echoes it as given.
        input.extend_from_slice(b"./");
        input.extend_from_slice(entry.name.as_bytes());
        input.push(0);
    }
    let output = git.run_with_input(
        folder,
        ["check-ignore", "--stdin", "-z"],
        Some(input),
        Instant::now() + git::TIMEOUT,
        MAX_IGNORE_OUTPUT,
    );
    // 0: some names are ignored, 1: none; anything else is an error (not a repository, a name
    // inside a submodule) and leaves every entry unmarked.
    let Ok(output) = output else { return };
    if output.code != Some(0) {
        return;
    }
    let ignored: HashSet<&[u8]> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| name.strip_prefix(b"./").unwrap_or(name))
        .collect();
    for entry in entries {
        entry.ignored = ignored.contains(entry.name.as_bytes());
    }
}

/// `\\?\C:\x` from `canonicalize` as `C:\x`, `\\?\UNC\host\share` as `\\host\share`.
fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;

    fn list(
        root: &Path,
        relative: &str,
        git: Option<&Git>,
    ) -> Result<FolderListing, FileTreeError> {
        FileTree::default().list(root, relative, git)
    }

    fn list_limited(
        root: &Path,
        relative: &str,
        git: Option<&Git>,
        limit: usize,
    ) -> Result<FolderListing, FileTreeError> {
        FileTree::default().list_limited(root, relative, git, limit)
    }

    fn names(listing: &FolderListing) -> Vec<&str> {
        listing.entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn entry<'a>(listing: &'a FolderListing, name: &str) -> &'a FolderEntry {
        listing
            .entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} not listed"))
    }

    #[test]
    fn valid_relative_paths() {
        assert!(segments("").unwrap().is_empty());
        assert_eq!(segments("src").unwrap(), ["src"]);
        assert_eq!(segments("src/a b/c.d").unwrap(), ["src", "a b", "c.d"]);
        assert_eq!(segments(".github/x").unwrap(), [".github", "x"]);
        assert_eq!(segments("..hidden").unwrap(), ["..hidden"]);
    }

    #[test]
    fn invalid_relative_paths() {
        for bad in [
            "/",
            "/etc",
            "..",
            "a/../b",
            "a/..",
            ".",
            "./a",
            "a//b",
            "a/",
            "C:",
            "C:/Windows",
            "c:\\Windows",
            "\\\\server\\share",
            "//server/share",
            "a\\b",
            "file.txt:stream",
            ".git",
            "a/.GIT/objects",
            "a\0b",
        ] {
            assert!(
                matches!(segments(bad), Err(FileTreeError::InvalidPath(_))),
                "{bad:?} was accepted"
            );
        }
    }

    #[test]
    fn natural_order() {
        let mut names = vec![
            "file10", "File2", "file1", "b", "A", "a", "file02", "_x", "Z",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            ["_x", "A", "a", "b", "file1", "file02", "File2", "file10", "Z"]
        );
        assert_eq!(natural_cmp("x", "x"), Ordering::Equal);
        assert_eq!(natural_cmp("x9", "x10"), Ordering::Less);
        assert_eq!(natural_cmp("x", "x1"), Ordering::Less);
    }

    #[test]
    fn folders_first_then_files() {
        let entry = |name: &str, kind| FolderEntry {
            name: name.to_owned(),
            kind,
            ignored: false,
        };
        let mut entries = vec![
            entry("b.txt", EntryKind::File),
            entry("link", EntryKind::Symlink),
            entry("Src", EntryKind::Dir),
            entry("a.txt", EntryKind::File),
            entry("docs", EntryKind::Dir),
        ];
        sort_entries(&mut entries);
        let order: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(order, ["docs", "Src", "a.txt", "b.txt", "link"]);
    }

    #[test]
    fn lists_a_folder_and_hides_git() {
        let dir = TempDir::new("file-tree-list");
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src/inner")).unwrap();
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(dir.path().join("README.md"), "x").unwrap();
        fs::write(dir.path().join("src/main.rs"), "x").unwrap();

        let listing = list(dir.path(), "", None).unwrap();
        assert_eq!(names(&listing), ["docs", "src", "README.md"]);
        assert_eq!(entry(&listing, "src").kind, EntryKind::Dir);
        assert_eq!(entry(&listing, "README.md").kind, EntryKind::File);
        assert!(!listing.truncated);
        let root = display_path(&fs::canonicalize(dir.path()).unwrap());
        assert_eq!(listing.root, root);
        // The fake `.git` makes the folder its own repository root.
        assert_eq!(listing.repo_root.as_deref(), Some(root.as_str()));
        assert!(!listing.root.starts_with(r"\\?\"));

        let inner = list(dir.path(), "src", None).unwrap();
        assert_eq!(names(&inner), ["inner", "main.rs"]);
        assert_eq!(inner.root, root);
    }

    #[test]
    fn missing_folders_and_files_are_refused() {
        let dir = TempDir::new("file-tree-missing");
        fs::write(dir.path().join("file.txt"), "x").unwrap();
        assert!(matches!(
            list(dir.path(), "file.txt", None),
            Err(FileTreeError::NotAFolder)
        ));
        assert!(matches!(
            list(dir.path(), "nope", None),
            Err(FileTreeError::NotAFolder)
        ));
        assert!(matches!(
            list(dir.path(), "../x", None),
            Err(FileTreeError::InvalidPath(_))
        ));
        assert!(list(&dir.path().join("gone"), "", None).is_err());
    }

    #[test]
    fn long_folders_are_cut() {
        let dir = TempDir::new("file-tree-cut");
        for name in ["d", "e"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        for name in ["c.txt", "a.txt", "b.txt"] {
            fs::write(dir.path().join(name), "x").unwrap();
        }
        let listing = list_limited(dir.path(), "", None, 3).unwrap();
        assert!(listing.truncated);
        assert_eq!(names(&listing), ["d", "e", "a.txt"]);
        let all = list_limited(dir.path(), "", None, 5).unwrap();
        assert!(!all.truncated);
        assert_eq!(all.entries.len(), 5);
    }

    /// What the check after resolving sees when a short name led into `.git`; runs everywhere,
    /// unlike the Windows test below, which needs a volume with 8.3 names.
    #[test]
    fn resolved_paths_through_git_folders_are_caught() {
        for bad in [".git", ".GIT/objects", "sub/.git", "a/b/.Git/x"] {
            assert!(reaches_git_folder(Path::new(bad)), "{bad}");
        }
        assert!(reaches_git_folder(Path::new("a/../b")));
        for good in ["", "src", "a/.github/x", "a/.gitignore", "x.git"] {
            assert!(!reaches_git_folder(Path::new(good)), "{good}");
        }
    }

    /// A repository whose config moves the work tree elsewhere fails git's work tree check: no
    /// repository root, nothing marked, and the answer is cached for the root.
    #[test]
    fn a_moved_work_tree_gets_no_git_answers() {
        let Some(git) = git() else { return };
        let dir = TempDir::new("file-tree-moved");
        let repo = dir.path().join("repo");
        let other = dir.path().join("other");
        init_repo(&git, &repo);
        fs::create_dir_all(&other).unwrap();
        fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        fs::write(repo.join("app.log"), "x").unwrap();
        git_ok(
            &git,
            &repo,
            &["config", "core.worktree", &other.to_string_lossy()],
        );
        let tree = FileTree::default();
        let listing = tree.list(&repo, "", Some(&git)).unwrap();
        assert!(listing.repo_root.is_none());
        assert!(!entry(&listing, "app.log").ignored);
        let root = fs::canonicalize(&repo).unwrap();
        assert!(matches!(lock(&tree.tops).get(&root), Some((_, None))));
    }

    #[test]
    fn reading_stops_at_the_deadline() {
        let dir = TempDir::new("file-tree-deadline");
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        let (entries, truncated) = read_entries(dir.path(), Instant::now()).unwrap();
        assert!(entries.is_empty());
        assert!(truncated);
        let (entries, truncated) =
            read_entries(dir.path(), Instant::now() + Duration::from_secs(60)).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!truncated);
    }

    #[test]
    fn one_listing_per_session_and_folder_at_a_time() {
        let in_flight = InFlight::default();
        let first = in_flight.claim(1, "src").unwrap();
        assert!(matches!(
            in_flight.claim(1, "src"),
            Err(FileTreeError::Busy)
        ));
        // Other folders and other sessions are not held up.
        let other = in_flight.claim(1, "docs").unwrap();
        let session = in_flight.claim(2, "src").unwrap();
        drop(first);
        assert!(in_flight.claim(1, "src").is_ok());
        drop((other, session));
        assert!(lock(&in_flight.0).is_empty());
    }

    /// The short name of `.git` (`GIT~1`) passes the text check; the resolved path must still be
    /// refused. Volumes without 8.3 names have nothing to check.
    #[cfg(windows)]
    #[test]
    fn short_name_of_the_git_folder_is_refused() {
        use std::os::windows::process::CommandExt;
        let dir = TempDir::new("file-tree-short");
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        let output = std::process::Command::new("cmd")
            .raw_arg("/D /C \"for %I in (.git) do @echo %~snxI\"")
            .current_dir(dir.path())
            .output()
            .unwrap();
        let short = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if short.is_empty() || short.eq_ignore_ascii_case(GIT_DIR_NAME) || short.contains(' ') {
            eprintln!("no 8.3 name for .git here ({short:?}), skipping");
            return;
        }
        assert!(
            segments(&short).is_ok(),
            "{short} is refused as text already"
        );
        for relative in [short.clone(), format!("{short}/objects")] {
            assert!(
                matches!(
                    list(dir.path(), &relative, None),
                    Err(FileTreeError::InvalidPath(_))
                ),
                "{relative} was listed"
            );
        }
    }

    #[cfg(unix)]
    fn link_dir(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    /// A symlink on Windows needs the privilege or developer mode; a junction, the link a user
    /// more likely has in a project, does not.
    #[cfg(windows)]
    fn link_dir(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
            || std::process::Command::new("cmd")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .stdout(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }

    #[test]
    fn links_are_listed_but_not_entered() {
        let outside = TempDir::new("file-tree-outside");
        fs::write(outside.path().join("secret.txt"), "x").unwrap();
        let dir = TempDir::new("file-tree-links");
        fs::create_dir(dir.path().join("inside")).unwrap();
        let out_link = dir.path().join("out");
        let in_link = dir.path().join("in");
        if !link_dir(outside.path(), &out_link) || !link_dir(&dir.path().join("inside"), &in_link) {
            eprintln!("symlinks cannot be created here, skipping");
            return;
        }
        let listing = list(dir.path(), "", None).unwrap();
        assert_eq!(names(&listing), ["inside", "in", "out"]);
        assert_eq!(entry(&listing, "out").kind, EntryKind::Symlink);
        assert!(matches!(
            list(dir.path(), "out", None),
            Err(FileTreeError::Link)
        ));
        assert!(matches!(
            list(dir.path(), "in", None),
            Err(FileTreeError::Link)
        ));
        // A session started in a linked folder lists its target.
        let through = list(&out_link, "", None).unwrap();
        assert_eq!(names(&through), ["secret.txt"]);
    }

    #[cfg(windows)]
    #[test]
    fn names_that_are_not_unicode_stay_listable() {
        use std::os::windows::ffi::OsStringExt;
        let dir = TempDir::new("file-tree-lossy");
        let name = std::ffi::OsString::from_wide(&[u16::from(b'a'), 0xD800]);
        if fs::write(dir.path().join(&name), "x").is_err() {
            eprintln!("unpaired surrogate not accepted here, skipping");
            return;
        }
        let listing = list(dir.path(), "", None).unwrap();
        assert_eq!(names(&listing), ["a\u{FFFD}"]);
    }

    #[test]
    fn ignored_entries_in_a_repository() {
        let Some(git) = git() else { return };
        let dir = TempDir::new("file-tree-ignored");
        let repo = dir.path().join("repo");
        init_repo(&git, &repo);
        fs::write(repo.join(".gitignore"), "build/\n*.log\n:odd\n").unwrap();
        fs::create_dir_all(repo.join("build/out")).unwrap();
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("build/out/x.bin"), "x").unwrap();
        fs::write(repo.join("app.log"), "x").unwrap();
        fs::write(repo.join("keep.txt"), "x").unwrap();
        fs::write(repo.join("src/debug.log"), "x").unwrap();
        fs::write(repo.join("src/lib.rs"), "x").unwrap();
        // A tracked file stays visible as not ignored even when a pattern matches it.
        fs::write(repo.join("tracked.log"), "x").unwrap();
        git_ok(&git, &repo, &["add", "-f", "tracked.log"]);

        let listing = list(&repo, "", Some(&git)).unwrap();
        assert_eq!(
            names(&listing),
            [
                "build",
                "src",
                ".gitignore",
                "app.log",
                "keep.txt",
                "tracked.log"
            ]
        );
        assert!(entry(&listing, "build").ignored);
        assert!(entry(&listing, "app.log").ignored);
        assert!(!entry(&listing, "keep.txt").ignored);
        assert!(!entry(&listing, "src").ignored);
        assert!(!entry(&listing, ".gitignore").ignored);
        assert!(!entry(&listing, "tracked.log").ignored);
        let root = display_path(&fs::canonicalize(&repo).unwrap());
        assert_eq!(listing.repo_root.as_deref(), Some(root.as_str()));

        let src = list(&repo, "src", Some(&git)).unwrap();
        assert!(entry(&src, "debug.log").ignored);
        assert!(!entry(&src, "lib.rs").ignored);

        // Below an ignored folder everything is ignored.
        let out = list(&repo, "build/out", Some(&git)).unwrap();
        assert!(entry(&out, "x.bin").ignored);

        // A session in a subfolder still reports the repository's top level.
        let sub = list(&repo.join("src"), "", Some(&git)).unwrap();
        assert_eq!(sub.repo_root.as_deref(), Some(root.as_str()));
        assert!(entry(&sub, "debug.log").ignored);
    }

    /// `:` is not allowed in Windows file names, so this runs where it can be created.
    #[cfg(unix)]
    #[test]
    fn names_that_look_like_pathspec_magic_are_plain_names() {
        let Some(git) = git() else { return };
        let dir = TempDir::new("file-tree-magic");
        init_repo(&git, dir.path());
        fs::write(dir.path().join(".gitignore"), ":odd\n").unwrap();
        fs::write(dir.path().join(":odd"), "x").unwrap();
        fs::write(dir.path().join(":(glob)*"), "x").unwrap();
        let listing = list(dir.path(), "", Some(&git)).unwrap();
        assert!(entry(&listing, ":odd").ignored);
        assert!(!entry(&listing, ":(glob)*").ignored);
    }

    #[test]
    fn errors_are_plain_text_for_the_window() {
        let json = serde_json::to_value(FileTreeError::UnknownSession(7)).unwrap();
        assert_eq!(json, "Unknown session 7");
    }

    #[test]
    fn listing_serializes_camel_case() {
        let listing = FolderListing {
            root: "C:\\p".into(),
            repo_root: None,
            entries: vec![FolderEntry {
                name: "a".into(),
                kind: EntryKind::Symlink,
                ignored: true,
            }],
            truncated: false,
        };
        let json = serde_json::to_value(listing).unwrap();
        assert!(json["repoRoot"].is_null());
        assert_eq!(json["entries"][0]["kind"], "symlink");
        assert_eq!(json["entries"][0]["ignored"], true);
    }
}
