//! Commit activity for the calendar in the empty window: the author times of the user's own
//! commits in the repositories behind the folders Hangar knows (favorites, recent folders,
//! session folders).
//!
//! Every call goes through the hardened [`Git`] runner (argv only, no prompts, no pager, no
//! transports, a time limit per call). `git log` is asked for nothing that runs a program from
//! the repository's config: no diff, no textconv, and signatures are switched off explicitly
//! (`log.showSignature` would start the configured `gpg.program`). Folders that are no
//! repository, repositories without a configured `user.email` and repositories that fail or take
//! too long are skipped; the calendar shows only what could be read.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};

use crate::git::{self, Git, GitError};

/// Folders per call; the frontend sends at most this many.
pub const MAX_FOLDERS: usize = 24;
/// Commits per call; beyond this the answer is cut and `truncated` set.
pub const MAX_COMMITS: usize = 50_000;
/// All repositories together; a slow or huge repository must not keep the calendar away.
const TOTAL_TIME: Duration = Duration::from_secs(20);
/// `git config user.email` prints one line.
const MAX_EMAIL_BYTES: usize = 4096;
/// The calendar covers 53 weeks; a week of margin for time zones and the partial first week.
/// git compares this with the committer date, the calendar later uses the author date.
const SINCE: &str = "--since=378 days ago";

#[derive(Debug, thiserror::Error)]
pub enum ActivityError {
    #[error("Too many folders: at most {MAX_FOLDERS} per call")]
    TooManyFolders,
    #[error(transparent)]
    Git(#[from] GitError),
}

impl Serialize for ActivityError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActivity {
    /// Author time of each commit in seconds since the Unix epoch, unsorted. A commit reachable
    /// from several folders (worktrees, two clones) appears once. The window buckets them into
    /// local days.
    pub commits: Vec<i64>,
    /// The commit limit or the total time limit was reached; some commits are missing.
    pub truncated: bool,
}

/// The user's own commits of roughly the last year across the repositories of `folders`.
pub fn activity(git: &Git, folders: &[PathBuf]) -> Result<GitActivity, ActivityError> {
    if folders.len() > MAX_FOLDERS {
        return Err(ActivityError::TooManyFolders);
    }
    let total = Instant::now() + TOTAL_TIME;
    let mut roots = HashSet::new();
    let mut hashes = HashSet::new();
    let mut result = GitActivity::default();
    for folder in folders {
        if Instant::now() >= total {
            result.truncated = true;
            break;
        }
        let deadline = total.min(Instant::now() + git::TIMEOUT);
        let Some(root) = repository_root(git, folder, deadline) else {
            continue;
        };
        if !roots.insert(git::path_key(&root)) {
            continue;
        }
        let Some(lines) = own_commits(git, &root, deadline) else {
            continue;
        };
        for (hash, time) in lines {
            if result.commits.len() >= MAX_COMMITS {
                result.truncated = true;
                break;
            }
            if hashes.insert(hash) {
                result.commits.push(time);
            }
        }
        if result.truncated {
            break;
        }
    }
    Ok(result)
}

/// Work tree root of `folder`, or `None` for anything that is not a usable repository: a
/// relative or missing folder, a git folder on a share, no repository at all.
fn repository_root(git: &Git, folder: &Path, deadline: Instant) -> Option<PathBuf> {
    git::check_folder(folder).ok()?;
    git::toplevel(git, folder, deadline).ok().flatten()
}

/// `(hash, author time)` of the commits by the repository's configured `user.email`, or `None`
/// when there is no such address or git fails.
fn own_commits(git: &Git, root: &Path, deadline: Instant) -> Option<Vec<(String, i64)>> {
    let email = git
        .run(
            root,
            ["config", "--get", "user.email"],
            deadline,
            MAX_EMAIL_BYTES,
        )
        .ok()
        .filter(|output| output.success && !output.truncated)?;
    let email = String::from_utf8_lossy(&email.stdout).trim().to_owned();
    if email.is_empty() || email.contains(['\n', '\r', '\0']) {
        return None;
    }
    // `-F -i`: the address is a fixed string, not a pattern, and its case does not matter. The
    // angle brackets match the whole address, not a longer one that ends with it.
    let author = format!("--author=<{email}>");
    let log = git
        .run(
            root,
            [
                "log",
                "--branches",
                "--tags",
                "--remotes",
                "--no-merges",
                "--no-show-signature",
                "--fixed-strings",
                "--regexp-ignore-case",
                author.as_str(),
                SINCE,
                "--format=%H %at",
            ],
            deadline,
            git::MAX_LIST_BYTES,
        )
        .ok()
        .filter(git::Output::usable)?;
    Some(parse_log(&log.stdout, log.truncated))
}

/// Lines `<hash> <unix seconds>`. A cut output ends in a partial line, which is dropped; a
/// partial number would be a wrong time.
pub fn parse_log(output: &[u8], truncated: bool) -> Vec<(String, i64)> {
    let text = String::from_utf8_lossy(output);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if truncated {
        lines.pop();
    }
    lines
        .into_iter()
        .filter_map(|line| {
            let (hash, time) = line.trim_end_matches('\r').split_once(' ')?;
            let valid = hash.len() >= 40 && hash.bytes().all(|b| b.is_ascii_hexdigit());
            let time = time.parse::<i64>().ok()?;
            valid.then(|| (hash.to_ascii_lowercase(), time))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;

    const HASH_A: &str = "0123456789abcdef0123456789abcdef01234567";
    const HASH_B: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn log_lines() {
        let output = format!("{HASH_A} 1767344400\n{HASH_B} 1700000000\r\n\nnot a line\n");
        assert_eq!(
            parse_log(output.as_bytes(), false),
            [
                (HASH_A.to_owned(), 1_767_344_400),
                (HASH_B.to_owned(), 1_700_000_000)
            ]
        );
        // Cut in the middle of the last time: that line is dropped, not misread.
        let cut = format!("{HASH_A} 1767344400\n{HASH_B} 17000");
        assert_eq!(
            parse_log(cut.as_bytes(), true),
            [(HASH_A.to_owned(), 1_767_344_400)]
        );
        assert!(parse_log(b"", false).is_empty());
        assert!(parse_log(b"abc 123\n", false).is_empty());
        assert!(parse_log(format!("{HASH_A} soon\n").as_bytes(), false).is_empty());
    }

    #[test]
    fn too_many_folders_are_refused() {
        let Some(git) = git() else { return };
        let folders: Vec<PathBuf> = (0..=MAX_FOLDERS)
            .map(|i| PathBuf::from(format!("p{i}")))
            .collect();
        assert!(matches!(
            activity(&git, &folders),
            Err(ActivityError::TooManyFolders)
        ));
    }

    fn commit(git: &Git, repo: &Path, author: &str, date: &str) {
        git_ok(
            git,
            repo,
            &[
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "activity fixture",
                &format!("--author={author}"),
                &format!("--date={date}"),
            ],
        );
    }

    #[test]
    fn own_commits_across_folders_once_each() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-activity");
        let repo = tmp.path().join("repo with space");
        init_repo(&git, &repo);
        // Configured in another case than the commits carry: addresses ignore case.
        git_ok(
            &git,
            &repo,
            &["config", "user.email", "Test@Example.Invalid"],
        );
        commit(
            &git,
            &repo,
            "Hangar Test <test@example.invalid>",
            "2026-11-02T10:00:00Z",
        );
        commit(
            &git,
            &repo,
            "Hangar Test <test@example.invalid>",
            "2026-11-03T10:00:00Z",
        );
        // Someone else, and an address that only ends with the user's.
        commit(
            &git,
            &repo,
            "Other <other@example.invalid>",
            "2026-11-04T10:00:00Z",
        );
        commit(
            &git,
            &repo,
            "Prefix <xtest@example.invalid>",
            "2026-11-04T11:00:00Z",
        );
        fs_create(&repo.join("sub"));
        // A linked worktree of the same repository: another root, the same commits.
        let linked = tmp.path().join("linked");
        git_ok(
            &git,
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "side",
                &linked.to_string_lossy(),
            ],
        );

        let folders = vec![
            repo.clone(),
            repo.join("sub"),
            linked,
            // Skipped: no work tree, relative, missing.
            repo.join(".git"),
            PathBuf::from("relative"),
            tmp.path().join("missing"),
        ];
        let result = activity(&git, &folders).unwrap();
        let mut commits = result.commits.clone();
        commits.sort_unstable();
        // 2026-11-02T10:00:00Z and 2026-11-03T10:00:00Z.
        assert_eq!(commits, [1_793_613_600, 1_793_700_000]);
        assert!(!result.truncated);
    }

    fn fs_create(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
    }
}
