//! Own worktree per session (contract: docs/wiki/pages/syntheses/agent-workflow-features.md).
//!
//! Worktrees live in the app data folder (`<app local data>/worktrees/<repo>-<hash>/<slug>`), not
//! next to the repository, so repositories and synced folders stay clean. Removing never forces:
//! a worktree with changes and an unmerged branch stay, and the reason goes back to the window.
//! Removal only ever touches paths inside that folder and branches named `hangar/<slug>`.
//!
//! No hook and no clean, smudge or process filter of the repository runs for any git call here.
//! A fresh worktree therefore holds Git LFS files (and any other filtered file) as stored in the
//! repository, LFS files as pointers; an agent that needs them runs `git lfs pull` itself.

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::git::{self, Git, GitError};

pub const DIR_NAME: &str = "worktrees";
pub const BRANCH_PREFIX: &str = "hangar/";
/// Label part of the slug.
pub const MAX_LABEL_CHARS: usize = 40;
const SUFFIX_CHARS: usize = 6;
const MAX_SLUG_CHARS: usize = MAX_LABEL_CHARS + 1 + SUFFIX_CHARS;
/// Checking out or removing a large tree takes longer than the 10 s of a status call, and a
/// checkout killed halfway leaves a broken worktree behind.
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    pub repo_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removal {
    pub removed: bool,
    pub branch_deleted: bool,
    /// Why something stayed (git's message), otherwise `None`.
    pub reason: Option<String>,
}

/// Lowercase ASCII letters, digits and single `-`, at most [`MAX_LABEL_CHARS`]. German umlauts
/// are spelled out; everything else becomes `-`. Empty labels give `session`.
pub fn slug_label(label: &str) -> String {
    let mut slug = String::new();
    let push = |c: char, slug: &mut String| {
        if c == '-' {
            if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        } else {
            slug.push(c);
        }
    };
    for c in label.chars().flat_map(char::to_lowercase) {
        match c {
            'a'..='z' | '0'..='9' => push(c, &mut slug),
            'ä' => "ae".chars().for_each(|c| push(c, &mut slug)),
            'ö' => "oe".chars().for_each(|c| push(c, &mut slug)),
            'ü' => "ue".chars().for_each(|c| push(c, &mut slug)),
            'ß' => "ss".chars().for_each(|c| push(c, &mut slug)),
            _ => push('-', &mut slug),
        }
    }
    // ASCII only, so byte and char counts agree.
    slug.truncate(MAX_LABEL_CHARS);
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "session".to_owned()
    } else {
        slug.to_owned()
    }
}

/// Slug from the label plus a random suffix, so two sessions with the same label never collide.
pub fn new_slug(label: &str) -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    let suffix = random.get(..SUFFIX_CHARS).unwrap_or("000000");
    format!("{}-{suffix}", slug_label(label))
}

/// The rules [`new_slug`] produces; checked before anything is removed.
pub fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= MAX_SLUG_CHARS
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A full SHA-1 or SHA-256 commit id in hex, never a ref name or an option.
pub fn is_full_commit_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `<repo name>-<8 hex of sha256(repoRoot)>`: readable, and two repositories with the same name
/// do not share a folder.
pub fn repo_dir_name(repo_root: &Path) -> String {
    let name = repo_root
        .file_name()
        .map(|name| slug_label(&name.to_string_lossy()))
        .unwrap_or_else(|| "repo".to_owned());
    let hash = Sha256::digest(repo_root.to_string_lossy().as_bytes());
    let hex: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{name}-{hex}")
}

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("This folder is not inside a git repository")]
    NotARepo,
    #[error("Refusing a worktree outside Hangar's worktrees folder")]
    OutsideFolder,
    #[error("Refusing a branch Hangar did not create")]
    ForeignBranch,
    #[error("Folder {path} cannot be created: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl Serialize for WorktreeError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// The rules every [`Worktree`] Hangar created follows, checked without knowing the worktrees
/// folder (also when a workspace is loaded): absolute paths, branch `hangar/<slug>`, and a path
/// that ends in `<repo dir of repoRoot>/<slug>`. A worktree record edited to name another
/// repository or another branch than the one it was created for fails here.
pub fn check_shape(worktree: &Worktree) -> Result<(), WorktreeError> {
    check_branch(&worktree.branch)?;
    let outside = Err(WorktreeError::OutsideFolder);
    if !worktree.path.is_absolute() || !worktree.repo_root.is_absolute() {
        return outside;
    }
    let slug = worktree
        .branch
        .strip_prefix(BRANCH_PREFIX)
        .unwrap_or_default();
    let repo_dir = repo_dir_name(&worktree.repo_root);
    let mut names = worktree.path.components().rev();
    let (Some(Component::Normal(last)), Some(Component::Normal(parent))) =
        (names.next(), names.next())
    else {
        return outside;
    };
    if last != OsStr::new(slug) || parent != OsStr::new(&repo_dir) {
        return outside;
    }
    Ok(())
}

/// `path` must be `<base>/<repo dir>/<slug>` ([`check_layout`]) with the repo dir of `repo_root`.
pub fn check_managed(base: &Path, path: &Path, repo_root: &Path) -> Result<(), WorktreeError> {
    let (repo_dir, _slug) = check_layout(base, path)?;
    if repo_dir != repo_dir_name(repo_root) {
        return Err(WorktreeError::OutsideFolder);
    }
    Ok(())
}

/// `path` must be `<base>/<repo dir>/<slug>`, both names in Hangar's own format, and must not
/// leave `base` through a link once resolved. Gives the two names.
pub fn check_layout(base: &Path, path: &Path) -> Result<(String, String), WorktreeError> {
    let outside = Err(WorktreeError::OutsideFolder);
    if !path.is_absolute() || !base.is_absolute() {
        return outside;
    }
    let Ok(rest) = path.strip_prefix(base) else {
        return outside;
    };
    let names: Vec<&str> = rest
        .components()
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()
        .unwrap_or_default();
    let [repo_dir, slug] = names.as_slice() else {
        return outside;
    };
    if !is_valid_slug(repo_dir) || !is_valid_slug(slug) {
        return outside;
    }
    // Junctions and symlinks: compare the resolved paths too, as far as they exist.
    if let (Ok(real_path), Ok(real_base)) = (path.canonicalize(), base.canonicalize()) {
        if !real_path.starts_with(&real_base) {
            return outside;
        }
    }
    Ok(((*repo_dir).to_owned(), (*slug).to_owned()))
}

pub fn check_branch(branch: &str) -> Result<(), WorktreeError> {
    match branch.strip_prefix(BRANCH_PREFIX) {
        Some(slug) if is_valid_slug(slug) => Ok(()),
        _ => Err(WorktreeError::ForeignBranch),
    }
}

/// Creates branch `hangar/<slug>` from HEAD of the repository that contains `cwd` and checks it
/// out under `base`.
pub fn create(git: &Git, base: &Path, cwd: &Path, label: &str) -> Result<Worktree, WorktreeError> {
    create_at(git, base, cwd, &new_slug(label), "HEAD")
}

/// [`create`] with a slug chosen by the caller (the arena names its own) and the commit to start
/// from: `HEAD` or a full commit id, so several worktrees share one base even if HEAD moves while
/// they are created.
pub fn create_at(
    git: &Git,
    base: &Path,
    cwd: &Path,
    slug: &str,
    start: &str,
) -> Result<Worktree, WorktreeError> {
    if !is_valid_slug(slug) {
        return Err(WorktreeError::ForeignBranch);
    }
    if start != "HEAD" && !is_full_commit_id(start) {
        return Err(GitError::InvalidCommit.into());
    }
    git::check_folder(cwd)?;
    let deadline = Instant::now() + TIMEOUT;
    let repo_root = git::toplevel(git, cwd, deadline)?.ok_or(WorktreeError::NotARepo)?;
    let parent = base.join(repo_dir_name(&repo_root));
    let path = parent.join(slug);
    crate::persist::create_private_dir(&parent).map_err(|source| WorktreeError::Io {
        path: parent.clone(),
        source,
    })?;
    let branch = format!("{BRANCH_PREFIX}{slug}");
    // The checkout runs post-checkout and reference-transaction hooks and smudge filters, any
    // program a cloned repository's config names; switched off like everywhere else (the hooks
    // by Git itself).
    let neutral = git::filter_overrides(git, &repo_root, deadline)?;
    let add = neutral
        .iter()
        .map(OsStr::new)
        .chain([
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--no-checkout"),
            OsStr::new("-b"),
            OsStr::new(&branch),
            path.as_os_str(),
            OsStr::new(start),
        ])
        .collect::<Vec<_>>();
    let output = git.run(&repo_root, add, deadline, 64 * 1024)?;
    if !output.success {
        let _ = fs::remove_dir(&parent);
        return Err(output.failure().into());
    }
    let worktree = Worktree {
        path,
        branch,
        repo_root,
    };
    // The files are checked out only now, from inside the new worktree: there git reads the
    // config as the checkout sees it (HEAD on the new branch, so an `onbranch:hangar/**` include
    // applies, and the worktree's own config.worktree), and the overrides are read the same way.
    if let Err(error) = checkout(git, &worktree.path, deadline) {
        // A fresh deadline: after a checkout that timed out, the old one would fail every step.
        discard_new(git, &worktree, &neutral, &parent, Instant::now() + TIMEOUT);
        return Err(error);
    }
    Ok(worktree)
}

/// Fills a worktree added with `--no-checkout`: index and files from its HEAD, with the filter
/// overrides read in the worktree itself.
fn checkout(git: &Git, path: &Path, deadline: Instant) -> Result<(), WorktreeError> {
    let neutral = git::filter_overrides(git, path, deadline)?;
    let args = neutral.iter().map(String::as_str).chain([
        "reset",
        "--hard",
        "--quiet",
        "--no-recurse-submodules",
        "HEAD",
    ]);
    let output = git.run(path, args, deadline, 64 * 1024)?;
    if !output.success {
        return Err(output.failure().into());
    }
    Ok(())
}

/// Takes back a worktree [`create`] just added but could not check out: it holds no file of the
/// user yet, so folder and branch go by force. Failures stay, a later cleanup sees them.
fn discard_new(
    git: &Git,
    worktree: &Worktree,
    neutral: &[String],
    parent: &Path,
    deadline: Instant,
) {
    let remove = neutral.iter().map(OsStr::new).chain([
        OsStr::new("worktree"),
        OsStr::new("remove"),
        OsStr::new("--force"),
        worktree.path.as_os_str(),
    ]);
    let _ = git.run(&worktree.repo_root, remove, deadline, 64 * 1024);
    let _ = git.run(
        &worktree.repo_root,
        ["branch", "-D", worktree.branch.as_str()],
        deadline,
        64 * 1024,
    );
    let _ = fs::remove_dir(parent);
}

/// How many ignored names the reason lists before "and N more".
const LISTED_IGNORED: usize = 3;

/// Reason for keeping a worktree with ignored files: `git worktree remove` would delete them
/// without asking, even without `--force` (build output, `.env` files, local databases).
pub fn ignored_reason(ignored: &[String]) -> Option<String> {
    let (first, rest) = ignored.split_at(ignored.len().min(LISTED_IGNORED));
    if first.is_empty() {
        return None;
    }
    let mut reason = format!("The worktree contains ignored files: {}", first.join(", "));
    if !rest.is_empty() {
        reason.push_str(&format!(" and {} more", rest.len()));
    }
    Some(reason)
}

/// Every changed, untracked and ignored entry of the worktree (`git status --porcelain=v1 -z
/// --ignored=matching`; ignored ones are `!!`). Untracked files are listed whatever the
/// repository's `status.showUntrackedFiles` says. `neutral` are the filter overrides
/// ([`git::filter_overrides`]).
pub(crate) fn status_entries(
    git: &Git,
    path: &Path,
    neutral: &[String],
    deadline: Instant,
) -> Result<Vec<git::FileChange>, GitError> {
    let status = git.run(
        path,
        neutral.iter().map(String::as_str).chain([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignored=matching",
            "--ignore-submodules=all",
        ]),
        deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !status.success {
        // A cut list could hide changed or ignored files; only a complete one counts.
        return Err(status.failure());
    }
    Ok(git::parse_status(&status.stdout))
}

/// `git worktree remove` without `--force`, then `git branch -d`. A worktree with ignored files
/// stays too. What stays is reported in [`Removal::reason`], not as an error.
pub fn remove(git: &Git, base: &Path, worktree: &Worktree) -> Result<Removal, WorktreeError> {
    if let Some(reason) = remove_folder(git, base, worktree)? {
        return Ok(Removal {
            removed: false,
            branch_deleted: false,
            reason: Some(reason),
        });
    }
    let deadline = Instant::now() + TIMEOUT;
    let output = git.run(
        &worktree.repo_root,
        ["branch", "-d", worktree.branch.as_str()],
        deadline,
        64 * 1024,
    )?;
    Ok(Removal {
        removed: true,
        branch_deleted: output.success,
        reason: (!output.success).then(|| git::message(&output.stderr)),
    })
}

/// Removes the worktree's folder and git's record of it, never by force, and leaves the branch
/// alone. `None` once both are gone; otherwise why it stays. A folder deleted by hand leaves only
/// git's record, which `git worktree remove` takes away for that one worktree (measured with git
/// 2.56; `worktree prune` would also take the records of the user's own worktrees).
pub fn remove_folder(
    git: &Git,
    base: &Path,
    worktree: &Worktree,
) -> Result<Option<String>, WorktreeError> {
    check_shape(worktree)?;
    check_managed(base, &worktree.path, &worktree.repo_root)?;
    git::check_folder(&worktree.repo_root)?;
    git::check_git_links(&worktree.path)?;
    let deadline = Instant::now() + TIMEOUT;
    let root = &worktree.repo_root;

    let mut neutral = Vec::new();
    if worktree.path.exists() {
        // Read in the worktree, so a filter set only in its own config.worktree counts too.
        neutral = match git::filter_overrides(git, &worktree.path, deadline) {
            Ok(neutral) => neutral,
            Err(error) => return Ok(Some(error.to_string())),
        };
        let ignored: Vec<String> = match status_entries(git, &worktree.path, &neutral, deadline) {
            Ok(entries) => entries
                .into_iter()
                .filter(|file| file.status == "!!")
                .map(|file| file.path)
                .collect(),
            Err(error) => return Ok(Some(error.to_string())),
        };
        if let Some(reason) = ignored_reason(&ignored) {
            return Ok(Some(reason));
        }
    }
    // `worktree remove` checks the worktree with a `git status` of its own, which runs clean
    // filters on files whose stat data changed. `-c` reaches that child process through
    // GIT_CONFIG_PARAMETERS. A worktree with populated submodules is refused without `--force`
    // (measured with git 2.56), so their filters do not come into play.
    let args = neutral
        .iter()
        .map(OsStr::new)
        .chain([
            OsStr::new("worktree"),
            OsStr::new("remove"),
            worktree.path.as_os_str(),
        ])
        .collect::<Vec<_>>();
    let existed = worktree.path.exists();
    let output = git.run(root, args, deadline, 64 * 1024)?;
    if !output.success {
        // Folder and record both gone already (deleted by hand, then pruned): nothing is left.
        let gone = !existed
            && list(git, root, deadline).is_ok_and(|listed| {
                !listed
                    .iter()
                    .any(|entry| git::path_key(&entry.path) == git::path_key(&worktree.path))
            });
        if !gone {
            return Ok(Some(git::message(&output.stderr)));
        }
    }
    if worktree.path.exists() {
        return Ok(Some(
            "The worktree folder could not be deleted completely".to_owned(),
        ));
    }
    if let Some(parent) = worktree.path.parent() {
        // Only succeeds when no other worktree of this repository is left.
        let _ = fs::remove_dir(parent);
    }
    Ok(None)
}

/// One entry of `git worktree list --porcelain -z`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listed {
    pub path: PathBuf,
    /// Commit checked out; `None` before the first commit.
    pub head: Option<String>,
    /// Full ref name (`refs/heads/...`); `None` for a detached HEAD or a bare repository.
    pub branch: Option<String>,
    pub locked: bool,
    /// git says the folder is gone (`prunable`).
    pub prunable: bool,
}

/// Records of `git worktree list --porcelain -z`: `key value ` fields, an empty field ends a
/// record. The first record is the main worktree.
pub fn parse_list(output: &[u8]) -> Vec<Listed> {
    let mut list = Vec::new();
    let mut current: Option<Listed> = None;
    for field in output.split(|b| *b == 0) {
        let field = String::from_utf8_lossy(field);
        if field.is_empty() {
            list.extend(current.take());
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((&field, ""));
        match key {
            "worktree" => {
                list.extend(current.take());
                current = Some(Listed {
                    path: git::native_path(value),
                    ..Listed::default()
                });
            }
            _ => {
                let Some(entry) = current.as_mut() else {
                    continue;
                };
                match key {
                    "HEAD" => entry.head = Some(value.to_owned()),
                    "branch" => entry.branch = Some(value.to_owned()),
                    "locked" => entry.locked = true,
                    "prunable" => entry.prunable = true,
                    _ => {}
                }
            }
        }
    }
    list.extend(current);
    list
}

/// All worktrees of the repository at `root`, as git keeps them.
pub fn list(git: &Git, root: &Path, deadline: Instant) -> Result<Vec<Listed>, GitError> {
    let output = git.run(
        root,
        ["worktree", "list", "--porcelain", "-z"],
        deadline,
        git::MAX_LIST_BYTES,
    )?;
    if !output.success || output.truncated {
        return Err(output.failure());
    }
    Ok(parse_list(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;

    #[test]
    fn labels_become_slugs() {
        assert_eq!(slug_label("Fix the Login Bug"), "fix-the-login-bug");
        assert_eq!(slug_label("  --Prüfung: Größe!  "), "pruefung-groesse");
        assert_eq!(slug_label("a__b..c//d"), "a-b-c-d");
        assert_eq!(slug_label(""), "session");
        assert_eq!(slug_label("日本語"), "session");
        assert_eq!(slug_label("../../etc"), "etc");
        let long = slug_label(&"word ".repeat(20));
        assert!(long.len() <= MAX_LABEL_CHARS);
        assert!(!long.ends_with('-'));
        let long_label = "x".repeat(100);
        for label in ["Fix", "", long_label.as_str(), "Ünïcödé label"] {
            let slug = new_slug(label);
            assert!(is_valid_slug(&slug), "{slug}");
        }
        assert_ne!(new_slug("same"), new_slug("same"));
    }

    #[test]
    fn slug_rules() {
        for good in ["a", "fix-1a2b3c", "repo-0123abcd"] {
            assert!(is_valid_slug(good), "{good}");
        }
        let too_long = "x".repeat(MAX_SLUG_CHARS + 1);
        for bad in [
            "", "-a", "a-", "a--b", "A", "a b", "a/b", "..", "a.b", "ä", &too_long,
        ] {
            assert!(!is_valid_slug(bad), "{bad}");
        }
        assert!(check_branch("hangar/fix-1a2b3c").is_ok());
        for bad in [
            "main",
            "hangar/",
            "hangar/../main",
            "other/fix",
            "hangar/a/b",
        ] {
            assert!(matches!(
                check_branch(bad),
                Err(WorktreeError::ForeignBranch)
            ));
        }
    }

    #[test]
    fn repo_dir_names_differ_per_root() {
        let a = repo_dir_name(Path::new("/home/user/My Project"));
        let b = repo_dir_name(Path::new("/srv/My Project"));
        assert!(a.starts_with("my-project-"), "{a}");
        assert_ne!(a, b);
        assert_eq!(a.len(), "my-project-".len() + 8);
        assert!(is_valid_slug(&a));
    }

    #[test]
    fn only_paths_inside_the_worktrees_folder() {
        let tmp = TempDir::new("worktree-paths");
        let base = tmp.path().join(DIR_NAME);
        let root = tmp.path().join("repo");
        let dir = repo_dir_name(&root);
        let good = base.join(&dir).join("fix-1a2b3c");
        assert!(check_managed(&base, &good, &root).is_ok());
        // Same shape, but the folder of another repository.
        let other_root = tmp.path().join("other");
        assert!(check_managed(&base, &good, &other_root).is_err());
        for bad in [
            base.clone(),
            base.join(&dir),
            base.join(&dir).join("fix").join("deeper"),
            base.join(&dir).join("..").join("x"),
            base.join("..").join(&dir).join("x"),
            base.join("Repo").join("x"),
            // A dot name is no valid slug, such as the hooks folder of earlier versions.
            base.join(".no-hooks").join("x"),
            tmp.path().join("elsewhere").join(&dir).join("x"),
            PathBuf::from("relative").join("a").join("b"),
        ] {
            assert!(
                matches!(
                    check_managed(&base, &bad, &root),
                    Err(WorktreeError::OutsideFolder)
                ),
                "{}",
                bad.display()
            );
        }
    }

    #[test]
    fn worktree_records_must_keep_their_shape() {
        let tmp = TempDir::new("worktree-shape");
        let root = tmp.path().join("repo");
        let good = Worktree {
            path: tmp
                .path()
                .join(DIR_NAME)
                .join(repo_dir_name(&root))
                .join("fix-1a2b3c"),
            branch: "hangar/fix-1a2b3c".into(),
            repo_root: root.clone(),
        };
        check_shape(&good).unwrap();
        let mut cases = Vec::new();
        let mut w = good.clone();
        w.branch = "hangar/other-1a2b3c".into();
        cases.push(w);
        let mut w = good.clone();
        w.branch = "main".into();
        cases.push(w);
        let mut w = good.clone();
        w.repo_root = tmp.path().join("victim");
        cases.push(w);
        let mut w = good.clone();
        w.repo_root = PathBuf::from("repo");
        cases.push(w);
        let mut w = good.clone();
        w.path = PathBuf::from(repo_dir_name(&root)).join("fix-1a2b3c");
        cases.push(w);
        let mut w = good;
        w.path = PathBuf::from("fix-1a2b3c");
        cases.push(w);
        for w in cases {
            assert!(check_shape(&w).is_err(), "{w:?}");
        }
    }

    #[test]
    fn reasons_for_ignored_files() {
        assert_eq!(ignored_reason(&[]), None);
        let names = |n: usize| (0..n).map(|i| format!("f{i}")).collect::<Vec<_>>();
        assert_eq!(
            ignored_reason(&names(2)).as_deref(),
            Some("The worktree contains ignored files: f0, f1")
        );
        assert_eq!(
            ignored_reason(&names(5)).as_deref(),
            Some("The worktree contains ignored files: f0, f1, f2 and 2 more")
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_folder_are_refused() {
        let tmp = TempDir::new("worktree-link");
        let base = tmp.path().join(DIR_NAME);
        let outside = tmp.path().join("outside");
        fs::create_dir_all(outside.join("victim")).unwrap();
        fs::create_dir_all(&base).unwrap();
        let root = tmp.path().join("repo");
        let dir = repo_dir_name(&root);
        std::os::unix::fs::symlink(&outside, base.join(&dir)).unwrap();
        assert!(matches!(
            check_managed(&base, &base.join(&dir).join("victim"), &root),
            Err(WorktreeError::OutsideFolder)
        ));
    }

    #[test]
    fn create_runs_no_repository_hook_or_filter() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("worktree-create-hooks");
        let base = tmp.path().join(DIR_NAME);
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        fs::write(repo.join(".gitattributes"), "*.txt filter=marker\n").unwrap();
        fs::write(repo.join("readme.txt"), "hello\n").unwrap();
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);

        let hook_marker = tmp.path().join("hook-ran");
        let hook_text = hook_marker.to_string_lossy().replace('\\', "/");
        let hooks = repo.join(".git").join("hooks");
        fs::create_dir_all(&hooks).unwrap();
        for name in ["post-checkout", "reference-transaction"] {
            let hook = hooks.join(name);
            fs::write(&hook, format!("#!/bin/sh\ntouch '{hook_text}'\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let filter_marker = tmp.path().join("filter-ran");
        let filter = crate::git::tests::marker_filter(&filter_marker);
        git_ok(&git, &repo, &["config", "filter.marker.smudge", &filter]);

        // The same checkout through plain git runs both, otherwise the test proves nothing.
        let probe = tmp.path().join("probe");
        let probe_arg = probe.to_string_lossy().into_owned();
        git_ok(
            &git.with_repo_hooks(),
            &repo,
            &["worktree", "add", "-b", "probe", &probe_arg, "HEAD"],
        );
        assert!(
            hook_marker.exists(),
            "git did not run the hook; test is void"
        );
        assert!(
            filter_marker.exists(),
            "git did not run the smudge filter; test is void"
        );
        fs::remove_file(&hook_marker).unwrap();
        fs::remove_file(&filter_marker).unwrap();

        let worktree = create(&git, &base, &repo, "hooks").unwrap();
        assert!(!hook_marker.exists(), "a repository hook ran during create");
        assert!(
            !filter_marker.exists(),
            "a repository filter ran during create"
        );
        // Without its smudge filter the file is checked out as stored.
        assert_eq!(
            fs::read_to_string(worktree.path.join("readme.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "hello\n"
        );
    }

    /// A smudge filter that an include defines only for Hangar's branches: the new worktree's
    /// HEAD is such a branch while the files are checked out.
    #[test]
    fn create_runs_no_filter_from_an_onbranch_include() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("worktree-onbranch");
        let base = tmp.path().join(DIR_NAME);
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        fs::write(repo.join(".gitattributes"), "*.txt filter=marker\n").unwrap();
        fs::write(repo.join("readme.txt"), "hello\n").unwrap();
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);
        let marker = tmp.path().join("marker-smudge");
        fs::write(
            repo.join(".git").join("evil.cfg"),
            crate::git::tests::marker_filter_config(&marker),
        )
        .unwrap();
        crate::git::tests::append(
            &repo.join(".git").join("config"),
            "[includeIf \"onbranch:hangar/**\"]\n\tpath = evil.cfg\n",
        );

        // Plain git runs the filter for a branch under hangar/, otherwise the test proves nothing.
        let probe = tmp.path().join("probe");
        git_ok(
            &git,
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "hangar/probe-1a2b3c",
                &probe.to_string_lossy(),
                "HEAD",
            ],
        );
        assert!(marker.exists(), "git did not run the filter; test is void");
        fs::remove_file(&marker).unwrap();

        let worktree = create(&git, &base, &repo, "onbranch").unwrap();
        assert!(!marker.exists(), "a filter from an onbranch include ran");
        assert_eq!(
            fs::read_to_string(worktree.path.join("readme.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "hello\n"
        );
        assert_eq!(
            crate::git::branch_of(&worktree.path, None).as_deref(),
            Some(worktree.branch.as_str())
        );
        // Checked out completely: nothing to commit, nothing staged.
        assert_eq!(
            crate::git::changed_count(&git, &worktree.path).unwrap(),
            Some(0)
        );
    }

    #[test]
    fn remove_after_the_folder_was_deleted_by_hand() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("worktree-deleted");
        let base = tmp.path().join(DIR_NAME);
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        git_ok(
            &git,
            &repo,
            &["commit", "--quiet", "--allow-empty", "-m", "x"],
        );
        // The user's own worktree, deleted by hand as well: its record must stay.
        let own = tmp.path().join("own");
        git_ok(
            &git,
            &repo,
            &["worktree", "add", "-q", "-b", "own", &own.to_string_lossy()],
        );
        fs::remove_dir_all(&own).unwrap();

        let deleted = create(&git, &base, &repo, "deleted").unwrap();
        fs::remove_dir_all(&deleted.path).unwrap();
        let removal = remove(&git, &base, &deleted).unwrap();
        assert!(removal.removed && removal.branch_deleted, "{removal:?}");
        let listed = list(&git, &repo, Instant::now() + TIMEOUT).unwrap();
        assert!(listed.iter().all(|entry| entry.path != deleted.path));
        assert!(
            listed.iter().any(|entry| entry.prunable),
            "the user's own record was pruned"
        );

        // Folder and record already gone (pruned by hand): nothing is left to keep.
        let pruned = create(&git, &base, &repo, "pruned").unwrap();
        fs::remove_dir_all(&pruned.path).unwrap();
        git_ok(&git, &repo, &["worktree", "prune"]);
        let removal = remove(&git, &base, &pruned).unwrap();
        assert!(removal.removed && removal.branch_deleted, "{removal:?}");
    }

    #[test]
    fn create_and_remove_with_a_real_repository() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("worktree-real");
        let base = tmp.path().join(DIR_NAME);
        let repo = tmp.path().join("my repo");
        init_repo(&git, &repo);
        fs::write(repo.join("readme.txt"), "hello\n").unwrap();
        fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);
        fs::create_dir_all(repo.join("sub")).unwrap();

        // Clean worktree: removed together with its branch.
        let worktree = create(&git, &base, &repo.join("sub"), "Fix the bug").unwrap();
        assert!(worktree.branch.starts_with("hangar/fix-the-bug-"));
        assert!(worktree.path.join("readme.txt").is_file());
        assert!(worktree.path.starts_with(&base));
        check_managed(&base, &worktree.path, &worktree.repo_root).unwrap();
        check_shape(&worktree).unwrap();
        assert_eq!(
            crate::git::branch_of(&worktree.path, None).as_deref(),
            Some(worktree.branch.as_str())
        );
        let removal = remove(&git, &base, &worktree).unwrap();
        assert_eq!(
            removal,
            Removal {
                removed: true,
                branch_deleted: true,
                reason: None
            }
        );
        assert!(!worktree.path.exists());

        // Worktree with changes stays, and says why.
        let dirty = create(&git, &base, &repo, "dirty").unwrap();
        fs::write(dirty.path.join("readme.txt"), "changed\n").unwrap();
        let removal = remove(&git, &base, &dirty).unwrap();
        assert!(!removal.removed);
        assert!(!removal.branch_deleted);
        assert!(removal.reason.is_some());
        assert!(dirty.path.exists());

        // Committed but unmerged work: worktree goes, branch stays.
        git_ok(&git, &dirty.path, &["commit", "--quiet", "-am", "work"]);
        let removal = remove(&git, &base, &dirty).unwrap();
        assert!(removal.removed);
        assert!(!removal.branch_deleted);
        assert!(removal.reason.is_some());
        assert!(!dirty.path.exists());

        // Ignored files would be deleted by `worktree remove` even without --force: it stays.
        let ignored = create(&git, &base, &repo, "ignored").unwrap();
        fs::write(ignored.path.join("debug.log"), "local only\n").unwrap();
        let removal = remove(&git, &base, &ignored).unwrap();
        assert!(!removal.removed);
        assert!(removal.reason.as_deref().unwrap().contains("debug.log"));
        assert!(ignored.path.join("debug.log").is_file());
        fs::remove_file(ignored.path.join("debug.log")).unwrap();

        // No hook of the repository runs while Hangar removes the worktree and its branch.
        let marker = tmp.path().join("hook-ran");
        let hook = repo
            .join(".git")
            .join("hooks")
            .join("reference-transaction");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        let marker_text = marker.to_string_lossy().replace('\\', "/");
        fs::write(&hook, format!("#!/bin/sh\ntouch '{marker_text}'\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        git_ok(&git.with_repo_hooks(), &repo, &["branch", "hook-probe"]);
        assert!(marker.exists(), "git did not run the hook; test is void");
        fs::remove_file(&marker).unwrap();
        let removal = remove(&git, &base, &ignored).unwrap();
        assert_eq!(
            removal,
            Removal {
                removed: true,
                branch_deleted: true,
                reason: None
            }
        );
        assert!(!marker.exists(), "a repository hook ran during removal");

        // No clean filter of the repository runs during removal, also not in the `git status`
        // that `worktree remove` starts itself for a tracked file whose stat data changed.
        fs::write(repo.join(".gitattributes"), "*.txt filter=marker\n").unwrap();
        git_ok(&git, &repo, &["add", ".gitattributes"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "attributes"]);
        let filtered = create(&git, &base, &repo, "filtered").unwrap();
        let filter_marker = tmp.path().join("filter-ran");
        let filter = crate::git::tests::marker_filter(&filter_marker);
        git_ok(&git, &repo, &["config", "filter.marker.clean", &filter]);
        let touch = |age: u64| {
            let file = fs::File::options()
                .write(true)
                .open(filtered.path.join("readme.txt"))
                .unwrap();
            let when = std::time::SystemTime::now() - Duration::from_secs(age);
            file.set_modified(when).unwrap();
        };
        touch(3600);
        git_ok(&git, &filtered.path, &["status", "--porcelain"]);
        assert!(
            filter_marker.exists(),
            "git did not run the filter; test is void"
        );
        fs::remove_file(&filter_marker).unwrap();
        touch(7200);
        let removal = remove(&git, &base, &filtered).unwrap();
        assert!(
            !filter_marker.exists(),
            "a repository filter ran during removal"
        );
        assert_eq!(
            removal,
            Removal {
                removed: true,
                branch_deleted: true,
                reason: None
            }
        );

        // Foreign paths and branches are refused before git runs.
        let mut foreign = worktree.clone();
        foreign.path = repo.clone();
        assert!(matches!(
            remove(&git, &base, &foreign),
            Err(WorktreeError::OutsideFolder)
        ));
        let mut foreign = worktree;
        foreign.branch = "main".into();
        assert!(matches!(
            remove(&git, &base, &foreign),
            Err(WorktreeError::ForeignBranch)
        ));
    }
}
