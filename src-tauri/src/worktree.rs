//! Own worktree per session (contract: docs/wiki/pages/syntheses/agent-workflow-features.md).
//!
//! Worktrees live in the app data folder (`<app local data>/worktrees/<repo>-<hash>/<slug>`), not
//! next to the repository, so repositories and synced folders stay clean. Removing never forces:
//! a worktree with changes and an unmerged branch stay, and the reason goes back to the window.
//! Removal only ever touches paths inside that folder and branches named `hangar/<slug>`.

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

/// `path` must be `<base>/<repo dir>/<slug>`, both names in Hangar's own format, and must not
/// leave `base` through a link once resolved.
pub fn check_managed(base: &Path, path: &Path) -> Result<(), WorktreeError> {
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
    Ok(())
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
    git::check_folder(cwd)?;
    let deadline = Instant::now() + TIMEOUT;
    let repo_root = git::toplevel(git, cwd, deadline)?.ok_or(WorktreeError::NotARepo)?;
    let slug = new_slug(label);
    let parent = base.join(repo_dir_name(&repo_root));
    let path = parent.join(&slug);
    crate::persist::create_private_dir(&parent).map_err(|source| WorktreeError::Io {
        path: parent.clone(),
        source,
    })?;
    let branch = format!("{BRANCH_PREFIX}{slug}");
    let args = [
        OsStr::new("worktree"),
        OsStr::new("add"),
        OsStr::new("-b"),
        OsStr::new(&branch),
        path.as_os_str(),
        OsStr::new("HEAD"),
    ];
    let output = git.run(&repo_root, args, deadline, 64 * 1024)?;
    if !output.success {
        let _ = fs::remove_dir(&parent);
        return Err(output.failure().into());
    }
    Ok(Worktree {
        path,
        branch,
        repo_root,
    })
}

/// `git worktree remove` without `--force`, then `git branch -d`. What stays is reported in
/// [`Removal::reason`], not as an error.
pub fn remove(git: &Git, base: &Path, worktree: &Worktree) -> Result<Removal, WorktreeError> {
    check_managed(base, &worktree.path)?;
    check_branch(&worktree.branch)?;
    git::check_folder(&worktree.repo_root)?;
    let deadline = Instant::now() + TIMEOUT;
    let root = &worktree.repo_root;

    if worktree.path.exists() {
        let args = [
            OsStr::new("worktree"),
            OsStr::new("remove"),
            worktree.path.as_os_str(),
        ];
        let output = git.run(root, args, deadline, 64 * 1024)?;
        if !output.success {
            return Ok(Removal {
                removed: false,
                branch_deleted: false,
                reason: Some(git::message(&output.stderr)),
            });
        }
    } else {
        // Deleted by hand: only git's bookkeeping is left. Failure changes nothing for the user.
        let _ = git.run(root, ["worktree", "prune"], deadline, 64 * 1024);
    }
    if let Some(parent) = worktree.path.parent() {
        // Only succeeds when no other worktree of this repository is left.
        let _ = fs::remove_dir(parent);
    }

    let output = git.run(
        root,
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
        let good = base.join("repo-0123abcd").join("fix-1a2b3c");
        assert!(check_managed(&base, &good).is_ok());
        for bad in [
            base.clone(),
            base.join("repo-0123abcd"),
            base.join("repo-0123abcd").join("fix").join("deeper"),
            base.join("repo-0123abcd").join("..").join("x"),
            base.join("..").join("repo-0123abcd").join("x"),
            base.join("Repo").join("x"),
            tmp.path().join("elsewhere").join("repo-0123abcd").join("x"),
            PathBuf::from("relative").join("a").join("b"),
        ] {
            assert!(
                matches!(
                    check_managed(&base, &bad),
                    Err(WorktreeError::OutsideFolder)
                ),
                "{}",
                bad.display()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_folder_are_refused() {
        let tmp = TempDir::new("worktree-link");
        let base = tmp.path().join(DIR_NAME);
        let outside = tmp.path().join("outside");
        fs::create_dir_all(outside.join("victim")).unwrap();
        fs::create_dir_all(&base).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("repo-0123abcd")).unwrap();
        assert!(matches!(
            check_managed(&base, &base.join("repo-0123abcd").join("victim")),
            Err(WorktreeError::OutsideFolder)
        ));
    }

    #[test]
    fn create_and_remove_with_a_real_repository() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("worktree-real");
        let base = tmp.path().join(DIR_NAME);
        let repo = tmp.path().join("my repo");
        init_repo(&git, &repo);
        fs::write(repo.join("readme.txt"), "hello\n").unwrap();
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);
        fs::create_dir_all(repo.join("sub")).unwrap();

        // Clean worktree: removed together with its branch.
        let worktree = create(&git, &base, &repo.join("sub"), "Fix the bug").unwrap();
        assert!(worktree.branch.starts_with("hangar/fix-the-bug-"));
        assert!(worktree.path.join("readme.txt").is_file());
        assert!(worktree.path.starts_with(&base));
        check_managed(&base, &worktree.path).unwrap();
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
