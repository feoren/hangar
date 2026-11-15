//! What a repository's own config names, read before git runs there.
//!
//! git follows `include.path` and every `includeIf.<condition>.path` while it reads the
//! repository's config, and it opens further paths the config names (`core.excludesFile`,
//! `core.attributesFile`, `core.worktree`, `mailmap.file`, `diff.orderFile`) and the object
//! folders listed in `objects/info/alternates`. A cloned repository controls all of them. On
//! Windows a path on a network share makes git hand the user's credentials to that host, and an
//! included file can name filter drivers and hooks that a condition only switches on later (an
//! `onbranch:` include matches once a worktree checks out another branch).
//!
//! Before any git call, this module reads every config file of the repository (and of every
//! repository git could find above the folder) without includes, follows each include target
//! itself whatever its condition, and refuses a path on another share or device. The filter
//! drivers and hook names found are the union over all those files, so they can be switched off
//! even where git would only read them under a condition. The files are parsed by git itself
//! (`git config --file <f> --no-includes`), started in a folder outside every repository, so the
//! parser is git's and the repository is never opened with includes. Results are cached per git
//! folder and checked against the size and modification time of every file read.
//!
//! The check and the git call that follows are two steps: a config file rewritten in between,
//! or rewritten within the timestamp resolution with the same size, is read by git unchecked.
//! Doing that needs write access to the repository's git folder, which already allows far more.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Instant, SystemTime};

use super::{gitdir_allowed, is_remote, parse_gitdir_file, path_key, Git, GitError};
use crate::persist::read_limited;

/// The keys read from each file: includes, filter drivers, hooks named in the config, and paths
/// git opens on its own. Section and key names come lower case from git, subsections as written.
const GUARD_KEYS: &str = r"^(include\.path|includeif\..+\.path|filter\..+\.(clean|smudge|process)|hook\..+\.command|core\.(excludesfile|attributesfile|worktree)|mailmap\.file|diff\.orderfile)$";
/// git's own limit for nested includes.
const MAX_INCLUDE_DEPTH: usize = 10;
/// git's own limit for nested alternates.
const MAX_ALTERNATES_DEPTH: usize = 5;
/// Config files per repository; a repository with more is refused.
const MAX_FILES: usize = 64;
const MAX_SMALL_FILE: usize = 64 * 1024;
const MAX_CONFIG_OUTPUT: usize = 1 << 20;
const CACHE_ENTRIES: usize = 256;
const MAX_LINK_HOPS: usize = 8;

/// Filter drivers and hook names configured in the repository's own config files and everything
/// they include, conditions ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoConfig {
    pub filters: BTreeSet<String>,
    pub hooks: BTreeSet<String>,
}

impl RepoConfig {
    fn merge(&mut self, other: &RepoConfig) {
        self.filters.extend(other.filters.iter().cloned());
        self.hooks.extend(other.hooks.iter().cloned());
    }
}

/// Why git does not run; kept in the cache, so it must be cloneable (GitError is not).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Refusal {
    Remote,
    Unsafe,
    Failed(String),
    /// Never cached: the next call tries again.
    Timeout,
}

impl From<Refusal> for GitError {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Remote => GitError::RemoteGitDir,
            Refusal::Unsafe => GitError::UnsafeFilters,
            Refusal::Failed(message) => GitError::Failed(message),
            Refusal::Timeout => GitError::Timeout,
        }
    }
}

/// Size and modification time of a file, `None` while it does not exist.
type State = Option<(Option<SystemTime>, u64, bool)>;

/// The entry itself (a link is not followed) and, behind a link, the file it leads to. The
/// target is only looked at while the link is unchanged, so a link turned towards a share is
/// noticed before anything follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    entry: State,
    target: State,
}

fn state(meta: io::Result<fs::Metadata>) -> State {
    meta.ok().map(|meta| {
        (
            meta.modified().ok(),
            meta.len(),
            meta.file_type().is_symlink(),
        )
    })
}

fn stamp(path: &Path) -> Stamp {
    let entry = state(fs::symlink_metadata(path));
    let target = match entry {
        Some((_, _, true)) => state(fs::metadata(path)),
        _ => None,
    };
    Stamp {
        path: path.to_path_buf(),
        entry,
        target,
    }
}

fn fresh(old: &Stamp) -> bool {
    let entry = state(fs::symlink_metadata(&old.path));
    if entry != old.entry {
        return false;
    }
    match entry {
        Some((_, _, true)) => state(fs::metadata(&old.path)) == old.target,
        _ => true,
    }
}

/// A link (symbolic link, junction) at `path` must not lead to another share or device, also
/// through further links; the metadata and reads that follow would go there.
fn link_allowed(start: &Path, path: &Path) -> bool {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINK_HOPS {
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let Ok(target) = fs::read_link(&current) else {
                    return false;
                };
                let next = if target.is_absolute() || target.has_root() || is_remote(&target) {
                    target
                } else {
                    current.parent().unwrap_or(Path::new("")).join(target)
                };
                if !allowed(start, &next, false) {
                    return false;
                }
                current = next;
            }
            _ => return true,
        }
    }
    false
}

/// A small file read through links (git follows them too), only if it is a regular file there.
fn read_small(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read as _;
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(super::MAX_HEAD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= super::MAX_HEAD_BYTES).then_some(bytes)
}

struct Entry {
    stamps: Vec<Stamp>,
    outcome: Result<RepoConfig, Refusal>,
}

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// One git folder git may open from the start folder, with its common git folder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    git_dir: PathBuf,
    common: PathBuf,
}

/// Checks the repository config for a git call in `cwd` and gives the filter drivers and hook
/// names it configures. A folder outside every repository gives an empty result.
pub(crate) fn check(git: &Git, cwd: &Path, deadline: Instant) -> Result<RepoConfig, GitError> {
    let mut starts = Vec::new();
    if let Ok(absolute) = std::path::absolute(cwd) {
        starts.push(absolute);
    }
    // git discovers from the resolved folder; a link may lead above another repository.
    if let Ok(real) = fs::canonicalize(cwd) {
        if !starts
            .iter()
            .any(|start| path_key(start) == path_key(&real))
        {
            starts.push(real);
        }
    }
    let mut total = RepoConfig::default();
    for start in &starts {
        for candidate in candidates(start)? {
            total.merge(&cached_scan(git, start, &candidate, deadline)?);
        }
    }
    Ok(total)
}

/// Every git folder git could open from `start` on its way up: a `.git` folder, the target of a
/// `.git` file, and a folder that looks like a bare repository. git stops at the first valid
/// one; all of them are taken here, so a folder git would skip as invalid cannot hide the one
/// above it.
fn candidates(start: &Path) -> Result<Vec<Candidate>, GitError> {
    let mut found = Vec::new();
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        if !link_allowed(start, &dot_git) {
            return Err(GitError::RemoteGitDir);
        }
        let git_dir = match fs::metadata(&dot_git) {
            Ok(meta) if meta.is_dir() => Some(dot_git),
            Ok(meta) if meta.is_file() => read_small(&dot_git).and_then(|content| {
                parse_gitdir_file(&String::from_utf8_lossy(&content)).map(|target| dir.join(target))
            }),
            _ => None,
        };
        let bare = looks_bare(dir).then(|| dir.to_path_buf());
        for git_dir in git_dir.into_iter().chain(bare) {
            if !allowed(start, &git_dir, false) || !link_allowed(start, &git_dir) {
                return Err(GitError::RemoteGitDir);
            }
            let commondir = git_dir.join("commondir");
            if !link_allowed(start, &commondir) {
                return Err(GitError::RemoteGitDir);
            }
            let common = match read_small(&commondir) {
                Some(content) => {
                    let text = String::from_utf8_lossy(&content);
                    match text.lines().next().map(str::trim) {
                        Some(line) if !line.is_empty() => git_dir.join(line),
                        _ => git_dir.clone(),
                    }
                }
                None => git_dir.clone(),
            };
            if !allowed(start, &common, false) || !link_allowed(start, &common) {
                return Err(GitError::RemoteGitDir);
            }
            found.push(Candidate { git_dir, common });
        }
    }
    Ok(found)
}

/// git's test for a git folder, as far as it matters here: HEAD, `objects` and `refs`.
fn looks_bare(dir: &Path) -> bool {
    dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir()
}

/// Whether git may open `target` for a call in `start` ([`gitdir_allowed`]). Separators are
/// normalized first, and on Windows a `\??\` NT path counts as remote unless it names a drive.
/// A path below the user's home (`~`) may also lie on the home's own share.
fn allowed(start: &Path, target: &Path, from_home: bool) -> bool {
    let text = target.to_string_lossy();
    let normalized = if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text.into_owned()
    };
    if cfg!(windows) {
        if let Some(rest) = normalized.strip_prefix(r"\??\") {
            let drive = rest.as_bytes();
            let is_drive = drive.len() >= 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':';
            if !is_drive {
                return false;
            }
        }
    }
    let target = Path::new(&normalized);
    if gitdir_allowed(start, target) {
        return true;
    }
    from_home && home().is_some_and(|home| gitdir_allowed(&home, target))
}

/// The home folder git expands `~` to: HOME, on Windows USERPROFILE when HOME is not set.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .or_else(|| {
            cfg!(windows)
                .then(|| std::env::var_os("USERPROFILE"))
                .flatten()
        })
        .map(PathBuf::from)
}

fn cache_key(start: &Path, candidate: &Candidate) -> String {
    // The share rule depends on where the call starts, so the start's share is part of the key.
    let share = if is_remote(start) {
        start
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .unwrap_or_default()
    } else {
        String::new()
    };
    // The common folder is part of the key too: `commondir` is read anew on every call, and a
    // changed one must not reuse the result of the config files read for the old one.
    format!(
        "{share}|{}|{}",
        path_key(&candidate.git_dir),
        path_key(&candidate.common)
    )
}

fn cached_scan(
    git: &Git,
    start: &Path,
    candidate: &Candidate,
    deadline: Instant,
) -> Result<RepoConfig, GitError> {
    let key = cache_key(start, candidate);
    {
        let cache = crate::lock(&CACHE);
        if let Some(entry) = cache.get(&key) {
            if entry.stamps.iter().all(fresh) {
                return entry.outcome.clone().map_err(GitError::from);
            }
        }
    }
    // Scanned without the lock: it starts git, and another thread may scan the same folder at
    // the same time, which only costs the duplicate work.
    let mut stamps = Vec::new();
    let outcome = scan(git, start, candidate, deadline, &mut stamps);
    if outcome != Err(Refusal::Timeout) {
        let mut cache = crate::lock(&CACHE);
        if cache.len() >= CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(
            key,
            Entry {
                stamps,
                outcome: outcome.clone(),
            },
        );
    }
    outcome.map_err(GitError::from)
}

fn scan(
    git: &Git,
    start: &Path,
    candidate: &Candidate,
    deadline: Instant,
    stamps: &mut Vec<Stamp>,
) -> Result<RepoConfig, Refusal> {
    check_alternates(start, &candidate.common.join("objects"), 0, stamps)?;

    let mut config = RepoConfig::default();
    let mut queue = vec![(candidate.common.join("config"), 0usize)];
    let worktree_config = candidate.git_dir.join("config.worktree");
    queue.push((worktree_config, 0));
    let mut read = 0usize;
    while let Some((file, depth)) = queue.pop() {
        if !link_allowed(start, &file) {
            return Err(Refusal::Remote);
        }
        stamps.push(stamp(&file));
        let meta = match fs::metadata(&file) {
            Ok(meta) => meta,
            // git skips a missing config or include file as well.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(Refusal::Unsafe),
        };
        // A folder, FIFO or device: git would fail or block on it, and nothing could be checked.
        if !meta.is_file() {
            return Err(Refusal::Unsafe);
        }
        read += 1;
        if read > MAX_FILES {
            return Err(Refusal::Unsafe);
        }
        let records = read_keys(git, &file, deadline)?;
        let base = file.parent().unwrap_or(Path::new(""));
        for (key, value) in parse_records(&records)? {
            if key == "include.path" || (key.starts_with("includeif.") && key.ends_with(".path")) {
                let Some(value) = value else { continue };
                let (target, from_home) = expand(&value, base)?;
                if !allowed(start, &target, from_home) {
                    return Err(Refusal::Remote);
                }
                if depth + 1 > MAX_INCLUDE_DEPTH {
                    return Err(Refusal::Unsafe);
                }
                queue.push((target, depth + 1));
            } else if let Some(rest) = key.strip_prefix("filter.") {
                config.filters.insert(subsection(rest)?);
            } else if let Some(rest) = key.strip_prefix("hook.") {
                config.hooks.insert(subsection(rest)?);
            } else if let Some(value) = value {
                // A path git opens: an exclude, attributes, mailmap or order file, the work tree.
                let (target, from_home) = expand(&value, base)?;
                if !allowed(start, &target, from_home) {
                    return Err(Refusal::Remote);
                }
            }
        }
    }
    Ok(config)
}

/// The name between the section and the last key: `<name>.clean` gives `<name>`. A name that
/// `-c` could not pass back (it splits at the first `=`) is refused.
fn subsection(rest: &str) -> Result<String, Refusal> {
    let (name, _key) = rest.rsplit_once('.').ok_or(Refusal::Unsafe)?;
    if name.is_empty() || name.contains(['=', '\n', '\0']) {
        return Err(Refusal::Unsafe);
    }
    Ok(name.to_owned())
}

/// A config path as git expands it: `~/` is the home folder, a relative path is relative to the
/// folder of the file that names it. `~user/` and `%(prefix)/` are not resolved here; such a
/// path is refused rather than left unchecked. The flag tells that the path came from `~`.
fn expand(value: &str, base: &Path) -> Result<(PathBuf, bool), Refusal> {
    if value.starts_with("%(prefix)") {
        return Err(Refusal::Unsafe);
    }
    if let Some(rest) = value.strip_prefix('~') {
        let rest = match rest.chars().next() {
            None => "",
            Some('/') | Some('\\') => &rest[1..],
            Some(_) => return Err(Refusal::Unsafe),
        };
        let home = home().ok_or(Refusal::Unsafe)?;
        return Ok((home.join(rest), true));
    }
    let path = Path::new(value);
    if path.is_absolute() || path.has_root() || is_remote(path) {
        Ok((path.to_path_buf(), false))
    } else {
        Ok((base.join(path), false))
    }
}

/// `key\nvalue\0` records of `git config -z`; a key without value (a bare `key` line) has no
/// newline.
fn parse_records(output: &[u8]) -> Result<Vec<(String, Option<String>)>, Refusal> {
    let mut records = Vec::new();
    for record in output.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let mut parts = record.splitn(2, |b| *b == b'\n');
        let key = parts.next().unwrap_or_default();
        let key = std::str::from_utf8(key).map_err(|_| Refusal::Unsafe)?;
        let value = parts
            .next()
            .map(|value| String::from_utf8_lossy(value).into_owned());
        records.push((key.to_owned(), value));
    }
    Ok(records)
}

/// `git config --file <file> --no-includes -z --get-regexp <GUARD_KEYS>`, started in a folder
/// outside every repository. In a repository's folder git reads that repository's config with
/// its includes at startup, even for `--file` (measured with git 2.56).
fn read_keys(git: &Git, file: &Path, deadline: Instant) -> Result<Vec<u8>, Refusal> {
    let neutral = git
        .program
        .parent()
        .filter(|dir| dir.is_absolute() && dir.is_dir())
        .ok_or(Refusal::Unsafe)?;
    let mut command = git.command(
        neutral,
        [
            std::ffi::OsStr::new("config"),
            std::ffi::OsStr::new("--no-includes"),
            std::ffi::OsStr::new("-z"),
            std::ffi::OsStr::new("--file"),
            file.as_os_str(),
            std::ffi::OsStr::new("--get-regexp"),
            std::ffi::OsStr::new(GUARD_KEYS),
        ],
        false,
    );
    // git looks for a repository in the start folder only, never above it.
    command.env(
        "GIT_CEILING_DIRECTORIES",
        neutral.parent().unwrap_or(neutral),
    );
    let output = match git.execute(command, None, deadline, MAX_CONFIG_OUTPUT) {
        Ok(output) => output,
        Err(GitError::Timeout) => return Err(Refusal::Timeout),
        Err(error) => return Err(Refusal::Failed(error.to_string())),
    };
    if output.truncated {
        return Err(Refusal::Unsafe);
    }
    if output.success {
        return Ok(output.stdout);
    }
    match output.code {
        // No such key.
        Some(1) => Ok(Vec::new()),
        _ => Err(Refusal::Failed(super::message(&output.stderr))),
    }
}

/// `objects/info/alternates` of an object folder: every listed folder must stay off other
/// shares and devices, also the ones their own alternates list.
fn check_alternates(
    start: &Path,
    objects: &Path,
    depth: usize,
    stamps: &mut Vec<Stamp>,
) -> Result<(), Refusal> {
    let file = objects.join("info").join("alternates");
    if !link_allowed(start, &file) || !link_allowed(start, objects) {
        return Err(Refusal::Remote);
    }
    stamps.push(stamp(&file));
    let content = match fs::metadata(&file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Refusal::Unsafe),
        Ok(meta) if !meta.is_file() => return Err(Refusal::Unsafe),
        Ok(_) => match read_limited(&file, MAX_SMALL_FILE) {
            Ok(Some(content)) => content,
            _ => return Err(Refusal::Unsafe),
        },
    };
    let text = String::from_utf8_lossy(&content);
    for line in text.lines().map(|line| line.trim_end_matches('\r')) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = if line.starts_with('"') {
            unquote(line).ok_or(Refusal::Unsafe)?
        } else {
            line.to_owned()
        };
        let path = Path::new(&entry);
        let target = if path.is_absolute() || path.has_root() || is_remote(path) {
            path.to_path_buf()
        } else {
            objects.join(path)
        };
        if !allowed(start, &target, false) {
            return Err(Refusal::Remote);
        }
        if depth + 1 >= MAX_ALTERNATES_DEPTH {
            continue;
        }
        check_alternates(start, &target, depth + 1, stamps)?;
    }
    Ok(())
}

/// A C-style quoted line as git writes it for alternates: `\\`, `\"`, `\a\b\f\n\r\t\v` and three
/// octal digits.
fn unquote(line: &str) -> Option<String> {
    let inner = line.strip_prefix('"')?.strip_suffix('"')?;
    let mut bytes = Vec::new();
    let mut chars = inner.bytes();
    while let Some(b) = chars.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        let escaped = chars.next()?;
        let value = match escaped {
            b'\\' | b'"' => escaped,
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 11,
            b'0'..=b'3' => {
                let d2 = chars.next()?;
                let d3 = chars.next()?;
                if !(b'0'..=b'7').contains(&d2) || !(b'0'..=b'7').contains(&d3) {
                    return None;
                }
                (escaped - b'0') * 64 + (d2 - b'0') * 8 + (d3 - b'0')
            }
            _ => return None,
        };
        bytes.push(value);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_alternates() {
        assert_eq!(unquote(r#""a\\b\"c""#).as_deref(), Some(r#"a\b"c"#));
        assert_eq!(unquote(r#""\134\134host""#).as_deref(), Some(r"\\host"));
        assert_eq!(unquote(r#""bad\q""#), None);
        assert_eq!(unquote("no quotes"), None);
    }

    #[test]
    fn config_paths_expand_like_git() {
        let base = Path::new("/repo/.git");
        assert_eq!(
            expand("other.cfg", base).unwrap(),
            (base.join("other.cfg"), false)
        );
        assert!(matches!(expand("~alice/x", base), Err(Refusal::Unsafe)));
        assert!(matches!(
            expand("%(prefix)/etc/x", base),
            Err(Refusal::Unsafe)
        ));
        if let Some(home) = home() {
            assert_eq!(expand("~/x.cfg", base).unwrap(), (home.join("x.cfg"), true));
        }
    }

    #[test]
    fn names_must_survive_the_command_line() {
        assert_eq!(subsection("lfs.clean").unwrap(), "lfs");
        assert_eq!(subsection("with.dots.smudge").unwrap(), "with.dots");
        assert!(subsection("a=b.clean").is_err());
        assert!(subsection(".clean").is_err());
        assert!(subsection("clean").is_err());
    }

    /// A `commondir` turned towards another repository must not reuse the result read for the
    /// old one: the cache key holds the common folder.
    #[test]
    fn a_changed_commondir_is_scanned_again() {
        use crate::git::tests::{append, git, init_repo};
        use crate::test_support::TempDir;
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-guard-commondir");
        let plain = tmp.path().join("plain");
        let filtered = tmp.path().join("filtered");
        init_repo(&git, &plain);
        init_repo(&git, &filtered);
        append(
            &filtered.join(".git").join("config"),
            "[filter \"marker\"]\n\tclean = marker-clean\n",
        );
        let git_dir = tmp.path().join("linked-git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let work = tmp.path().join("work");
        fs::create_dir_all(&work).unwrap();
        fs::write(
            work.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        let deadline = Instant::now() + crate::git::TIMEOUT;

        fs::write(
            git_dir.join("commondir"),
            format!("{}\n", plain.join(".git").display()),
        )
        .unwrap();
        assert!(check(&git, &work, deadline).unwrap().filters.is_empty());
        fs::write(
            git_dir.join("commondir"),
            format!("{}\n", filtered.join(".git").display()),
        )
        .unwrap();
        assert!(check(&git, &work, deadline)
            .unwrap()
            .filters
            .contains("marker"));
    }

    #[cfg(windows)]
    #[test]
    fn remote_targets_in_every_spelling() {
        let start = Path::new(r"C:\repo");
        for remote in [
            r"\\192.0.2.1\share\x",
            r"//192.0.2.1/share/x",
            r"/\192.0.2.1\share\x",
            r"\\?\UNC\192.0.2.1\share\x",
            r"\??\UNC\192.0.2.1\share\x",
            r"\\.\pipe\x",
        ] {
            assert!(!allowed(start, Path::new(remote), false), "{remote}");
        }
        for local in [r"C:\x\y.cfg", r"\x.cfg", r"..\x.cfg", r"\??\C:\x"] {
            assert!(allowed(start, Path::new(local), false), "{local}");
        }
    }
}
