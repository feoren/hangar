//! Commit history for the Changes panel: the commits reachable from HEAD, page by page, and the
//! changes of one commit in the same shape as the work tree's changes (contract:
//! docs/wiki/pages/syntheses/commit-history.md).
//!
//! Runs through the hardened [`Git`] runner with the same folder checks, time limit and output
//! caps as [`crate::git::diff`]. Nothing here reads the work tree: log, rev-list and diff-tree
//! only read objects, so neither filters nor the index are involved; the filter overrides are
//! still passed to the diff calls so a future change of subcommand cannot run one.

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::git::{
    check_folder, cut_text, filter_overrides, parse_numstat, toplevel, FileChange, Git, GitDiff,
    GitError, MAX_DIFF_BYTES, MAX_LIST_BYTES, TIMEOUT,
};

/// Commits per page at most; a larger request is clamped.
pub const MAX_LOG_LIMIT: u32 = 100;
/// Shortest abbreviated id accepted (git's own default abbreviation).
pub const MIN_SHA_LEN: usize = 7;
/// SHA-256 object ids are 64 hex characters, SHA-1 ids 40.
pub const MAX_SHA_LEN: usize = 64;
/// The commit message body handed to the window, in bytes; longer bodies are cut.
pub const MAX_BODY_BYTES: usize = 16 * 1024;
/// A subject is one line, but nothing stops it from being megabytes long.
const MAX_SUBJECT_CHARS: usize = 1000;
const MAX_AUTHOR_CHARS: usize = 200;
/// Output of the header call: room for the capped body plus fields; more is cut by the runner.
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_REV_PARSE_BYTES: usize = 4096;

/// Fields are separated by NUL, records end at the newline git appends to every commit
/// (`tformat`). A subject is the first paragraph joined into one line and an author name is one
/// header line, so neither can hold a newline; no field can hold a NUL, since git reads commit
/// objects as C strings. The subject is the last field, so anything else in it stays in it.
const LOG_FORMAT: &str = "--format=%H%x00%h%x00%at%x00%an%x00%s";
/// One commit's header; the body is last and may span lines.
const HEADER_FORMAT: &str = "--format=%H%x00%h%x00%P%x00%at%x00%an%x00%s%x00%b";

/// Options shared by every `log` call: no signature check (it would run gpg), no notes, no
/// decoration, one output encoding.
const LOG_ARGS: &[&str] = &[
    "log",
    "--no-show-signature",
    "--no-notes",
    "--no-decorate",
    "--no-color",
    "--encoding=UTF-8",
];

/// `diff-tree` compares two trees; it never reads the work tree or the index. The options match
/// the work tree diff's (`DIFF_ARGS` in git.rs), plus rename detection and no commit id line.
const DIFF_TREE_ARGS: &[&str] = &[
    "diff-tree",
    "-r",
    "-M",
    "--no-commit-id",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--ignore-submodules=all",
];

/// One commit of the history list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitSummary {
    /// Full object id, 40 (SHA-1) or 64 (SHA-256) hex characters.
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub author: String,
    /// Author date, Unix seconds.
    pub authored_at: i64,
    /// Reachable from the branch's upstream; always `false` without an upstream.
    pub pushed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLog {
    pub is_repo: bool,
    /// Newest first.
    pub commits: Vec<CommitSummary>,
    /// Short name of the upstream (`origin/main`); `None` without one or on a detached HEAD.
    pub upstream: Option<String>,
    /// Commits on HEAD that the upstream does not have.
    pub ahead_count: u64,
    /// More commits follow after this page.
    pub has_more: bool,
}

impl GitLog {
    fn empty(is_repo: bool) -> Self {
        Self {
            is_repo,
            commits: Vec::new(),
            upstream: None,
            ahead_count: 0,
            has_more: false,
        }
    }
}

/// Header of one commit for the diff view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitHeader {
    pub sha: String,
    pub short_sha: String,
    /// Full ids of the parents; empty for a root commit, two or more for a merge.
    pub parents: Vec<String>,
    pub subject: String,
    /// Message after the subject, at most [`MAX_BODY_BYTES`].
    pub body: String,
    pub author: String,
    pub authored_at: i64,
}

/// The changes of one commit against its first parent (a root commit against the empty tree).
/// Serialized as `GitDiff` plus `commit`, so the working tree's diff parser reads it as is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitDiff {
    pub commit: CommitHeader,
    #[serde(flatten)]
    pub changes: GitDiff,
}

/// An abbreviated or full object id: hex only, so it can never be read as an option or a ref
/// expression.
pub fn validate_sha(sha: &str) -> Result<(), GitError> {
    let valid = (MIN_SHA_LEN..=MAX_SHA_LEN).contains(&sha.len())
        && sha.bytes().all(|b| b.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(GitError::InvalidCommit)
    }
}

fn is_full_sha(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn has_head(git: &Git, root: &Path, deadline: Instant) -> Result<bool, GitError> {
    Ok(git
        .run(
            root,
            ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
            deadline,
            MAX_REV_PARSE_BYTES,
        )?
        .success)
}

/// Commits reachable from HEAD, newest first: `limit` (clamped to 1..=[`MAX_LOG_LIMIT`]) after
/// skipping `skip`. A folder outside a repository gives `isRepo: false`, a repository without
/// commits an empty list.
pub fn log(git: &Git, cwd: &Path, skip: u32, limit: u32) -> Result<GitLog, GitError> {
    check_folder(cwd)?;
    let limit = limit.clamp(1, MAX_LOG_LIMIT);
    let deadline = Instant::now() + TIMEOUT;
    let Some(root) = toplevel(git, cwd, deadline)? else {
        return Ok(GitLog::empty(false));
    };
    if !has_head(git, &root, deadline)? {
        return Ok(GitLog::empty(true));
    }

    let skip_arg = format!("--skip={skip}");
    // One more than asked tells whether another page follows.
    let count_arg = format!("--max-count={}", limit + 1);
    let output = git.run(
        &root,
        LOG_ARGS.iter().copied().chain([
            LOG_FORMAT,
            skip_arg.as_str(),
            count_arg.as_str(),
            "HEAD",
            "--",
        ]),
        deadline,
        MAX_LIST_BYTES,
    )?;
    if !output.usable() {
        return Err(output.failure());
    }
    let mut commits = parse_log(&output.stdout);
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let has_more = commits.len() > limit || output.truncated;
    commits.truncate(limit);

    let upstream = upstream_name(git, &root, deadline)?;
    let mut ahead_count = 0;
    if upstream.is_some() {
        ahead_count = mark_pushed(git, &root, &mut commits, deadline)?;
    }
    Ok(GitLog {
        is_repo: true,
        commits,
        upstream,
        ahead_count,
        has_more,
    })
}

/// Short name of the current branch's upstream; `None` without one, on a detached HEAD or when
/// the upstream ref is gone.
fn upstream_name(git: &Git, root: &Path, deadline: Instant) -> Result<Option<String>, GitError> {
    let output = git.run(
        root,
        [
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        deadline,
        MAX_REV_PARSE_BYTES,
    )?;
    if !output.success {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!name.is_empty()).then_some(name))
}

/// Sets `pushed` for every commit the upstream also has and returns the number of commits on
/// HEAD it lacks. The unpushed commits are listed once; only if that list is too long for the
/// output cap are the commits of this page asked one by one.
fn mark_pushed(
    git: &Git,
    root: &Path,
    commits: &mut [CommitSummary],
    deadline: Instant,
) -> Result<u64, GitError> {
    let ahead = git.run(
        root,
        ["rev-list", "@{upstream}..HEAD", "--"],
        deadline,
        MAX_LIST_BYTES,
    )?;
    if !ahead.usable() {
        return Err(ahead.failure());
    }
    if !ahead.truncated {
        let unpushed = parse_sha_lines(&ahead.stdout);
        for commit in commits.iter_mut() {
            commit.pushed = !unpushed.contains(commit.sha.as_str());
        }
        return Ok(u64::try_from(unpushed.len()).unwrap_or(u64::MAX));
    }
    for commit in commits.iter_mut() {
        let check = git.run(
            root,
            [
                "merge-base",
                "--is-ancestor",
                commit.sha.as_str(),
                "@{upstream}",
            ],
            deadline,
            MAX_REV_PARSE_BYTES,
        )?;
        // 0: ancestor, 1: not an ancestor, anything else is an error.
        commit.pushed = match check.code {
            Some(0) => true,
            Some(1) => false,
            _ => return Err(check.failure()),
        };
    }
    let count = git.run(
        root,
        ["rev-list", "--count", "@{upstream}..HEAD", "--"],
        deadline,
        MAX_REV_PARSE_BYTES,
    )?;
    if !count.success {
        return Err(count.failure());
    }
    Ok(String::from_utf8_lossy(&count.stdout)
        .trim()
        .parse()
        .unwrap_or(0))
}

/// Object ids, one per line; a cut last line is dropped.
fn parse_sha_lines(output: &[u8]) -> HashSet<&str> {
    complete_records(output, b'\n')
        .split(|b| *b == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| is_full_sha(line))
        .collect()
}

/// The part of `output` up to and including its last `terminator`; a record cut by the output
/// cap has none yet and is left out.
fn complete_records(output: &[u8], terminator: u8) -> &[u8] {
    output
        .iter()
        .rposition(|b| *b == terminator)
        .and_then(|end| output.get(..end))
        .unwrap_or_default()
}

fn capped(bytes: &[u8], max_chars: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end_matches('\r');
    text.chars().take(max_chars).collect()
}

/// `git log` with [`LOG_FORMAT`]: `sha\0short\0time\0author\0subject\n` per commit. Records with
/// an id that is not a full hex id or a time that is not a number are skipped.
pub fn parse_log(output: &[u8]) -> Vec<CommitSummary> {
    complete_records(output, b'\n')
        .split(|b| *b == b'\n')
        .filter_map(|record| {
            let mut fields = record.splitn(5, |b| *b == 0);
            let sha = std::str::from_utf8(fields.next()?).ok()?;
            let short_sha = std::str::from_utf8(fields.next()?).ok()?;
            let time = std::str::from_utf8(fields.next()?).ok()?;
            let author = fields.next()?;
            let subject = fields.next()?;
            if !is_full_sha(sha) || short_sha.is_empty() || !sha.starts_with(short_sha) {
                return None;
            }
            Some(CommitSummary {
                sha: sha.to_owned(),
                short_sha: short_sha.to_owned(),
                subject: capped(subject, MAX_SUBJECT_CHARS),
                author: capped(author, MAX_AUTHOR_CHARS),
                authored_at: time.trim().parse().ok()?,
                pushed: false,
            })
        })
        .collect()
}

/// `git log -1` with [`HEADER_FORMAT`]. The body is cut at [`MAX_BODY_BYTES`] on a character
/// boundary; `None` if the fixed fields are missing or malformed.
pub fn parse_header(output: &[u8]) -> Option<CommitHeader> {
    let mut fields = output.splitn(7, |b| *b == 0);
    let sha = std::str::from_utf8(fields.next()?).ok()?;
    let short_sha = std::str::from_utf8(fields.next()?).ok()?;
    let parents = std::str::from_utf8(fields.next()?).ok()?;
    let time = std::str::from_utf8(fields.next()?).ok()?;
    let author = fields.next()?;
    let subject = fields.next()?;
    let body = fields.next()?;
    if !is_full_sha(sha) || !sha.starts_with(short_sha) || short_sha.is_empty() {
        return None;
    }
    let parents: Vec<String> = parents.split_whitespace().map(str::to_owned).collect();
    if !parents.iter().all(|parent| is_full_sha(parent)) {
        return None;
    }
    Some(CommitHeader {
        sha: sha.to_owned(),
        short_sha: short_sha.to_owned(),
        parents,
        subject: capped(subject, MAX_SUBJECT_CHARS),
        body: cut_body(body),
        author: capped(author, MAX_AUTHOR_CHARS),
        authored_at: time.trim().parse().ok()?,
    })
}

fn cut_body(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let text = text.trim_end();
    if text.len() <= MAX_BODY_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_BODY_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().to_owned()
}

/// `git diff-tree --name-status -z`: `X\0path\0`, for renames and copies `R100\0old\0new\0`.
/// The status becomes a porcelain-style code with the letter in the index column (`A `, `M `,
/// `D `, `R `, `C `, `T `), so the Changes panel classifies it like a staged change.
pub fn parse_name_status(output: &[u8]) -> Vec<FileChange> {
    let mut files = Vec::new();
    let mut records = complete_records(output, 0).split(|b| *b == 0);
    while let Some(code) = records.next() {
        let Some(&letter) = code.first() else {
            continue;
        };
        if !letter.is_ascii_uppercase() {
            // Out of step with git's records; nothing after this can be trusted.
            break;
        }
        let Some(mut path) = records.next() else {
            break;
        };
        if matches!(letter, b'R' | b'C') {
            match records.next() {
                Some(new) => path = new,
                None => break,
            }
        }
        if path.is_empty() {
            continue;
        }
        files.push(FileChange {
            path: String::from_utf8_lossy(path).into_owned(),
            status: format!("{} ", char::from(letter)),
            additions: 0,
            deletions: 0,
        });
    }
    files
}

/// Full id of the commit `sha` names, after [`validate_sha`]. `^{commit}` makes git peel to a
/// commit and fail for any other object; an unknown or ambiguous id is [`GitError::UnknownCommit`].
fn resolve_commit(
    git: &Git,
    root: &Path,
    sha: &str,
    deadline: Instant,
) -> Result<String, GitError> {
    let revision = format!("{sha}^{{commit}}");
    let output = git.run(
        root,
        ["rev-parse", "--verify", "--quiet", revision.as_str()],
        deadline,
        MAX_REV_PARSE_BYTES,
    )?;
    let full = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.success && is_full_sha(&full) {
        Ok(full)
    } else {
        Err(GitError::UnknownCommit)
    }
}

/// The changes `sha` made: file list with status and +/-, the unified diff (cut at
/// [`MAX_DIFF_BYTES`] on a whole line) and the commit's header. A merge is compared with its first
/// parent, a root commit with the empty tree.
pub fn commit_diff(git: &Git, cwd: &Path, sha: &str) -> Result<GitCommitDiff, GitError> {
    validate_sha(sha)?;
    check_folder(cwd)?;
    let deadline = Instant::now() + TIMEOUT;
    let root = toplevel(git, cwd, deadline)?.ok_or(GitError::NotARepository)?;
    let full = resolve_commit(git, &root, sha, deadline)?;

    let header = git.run(
        &root,
        LOG_ARGS
            .iter()
            .copied()
            .chain(["--max-count=1", HEADER_FORMAT, full.as_str(), "--"]),
        deadline,
        MAX_HEADER_BYTES,
    )?;
    if !header.usable() {
        return Err(header.failure());
    }
    let commit = parse_header(&header.stdout)
        .ok_or_else(|| GitError::Failed("git printed an unreadable commit header".to_owned()))?;

    let neutral = filter_overrides(git, &root, deadline)?;
    let target: Vec<&str> = match commit.parents.first() {
        Some(parent) => vec![parent.as_str(), full.as_str()],
        None => vec!["--root", full.as_str()],
    };
    let run = |mode: &[&str], limit: usize| {
        let output = git.run(
            &root,
            neutral
                .iter()
                .map(String::as_str)
                .chain(DIFF_TREE_ARGS.iter().copied())
                .chain(mode.iter().copied())
                .chain(target.iter().copied())
                .chain(["--"]),
            deadline,
            limit,
        )?;
        if output.usable() {
            Ok(output)
        } else {
            Err(output.failure())
        }
    };

    let names = run(&["--name-status", "-z"], MAX_LIST_BYTES)?;
    let numstat = run(&["--numstat", "-z"], MAX_LIST_BYTES)?;
    let patch = run(&["-p"], MAX_DIFF_BYTES)?;
    let truncated = names.truncated || numstat.truncated || patch.truncated;

    let mut files = parse_name_status(&names.stdout);
    let counts: std::collections::HashMap<String, (u64, u64)> = parse_numstat(&numstat.stdout)
        .into_iter()
        .map(|(path, added, deleted)| (path, (added, deleted)))
        .collect();
    for file in &mut files {
        if let Some((added, deleted)) = counts.get(&file.path) {
            file.additions = *added;
            file.deletions = *deleted;
        }
    }
    Ok(GitCommitDiff {
        commit,
        changes: GitDiff {
            is_repo: true,
            files,
            diff: cut_text(patch.stdout, patch.truncated),
            truncated,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{git, git_ok, init_repo};
    use crate::test_support::TempDir;
    use std::fs;

    const SHA_A: &str = "0123456789abcdef0123456789abcdef01234567";
    const SHA_B: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn sha_validation() {
        for good in ["abcdef1", "ABCDEF1", SHA_A, &"a".repeat(64), "0000000"] {
            assert!(validate_sha(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "-x",
            "--output=x",
            "HEAD",
            "abc",
            "abcdef",
            "abcdefg",
            "abcdef1^",
            "abcdef1..HEAD",
            "abcdef1 ",
            " abcdef1",
            "abcd\u{e9}f12",
            &"a".repeat(65),
        ] {
            assert!(
                matches!(validate_sha(bad), Err(GitError::InvalidCommit)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn log_records() {
        let output = format!(
            "{SHA_A}\x000123456\x001700000000\x00Ana \u{d6}z\x00feat: one\x1ftwo\x1ethree\n\
             {SHA_B}\x0089abcde\x001600000000\x00Bo\x00\n\
             not-a-sha\x00x\x001\x00a\x00s\n\
             {SHA_A}\x00fff\x001\x00a\x00short id that does not match\n\
             {SHA_A}\x000123456\x00soon\x00a\x00bad time\n\
             {SHA_B}\x0089abcde\x001\x00cut"
        );
        let commits = parse_log(output.as_bytes());
        assert_eq!(
            commits,
            [
                CommitSummary {
                    sha: SHA_A.to_owned(),
                    short_sha: "0123456".to_owned(),
                    subject: "feat: one\u{1f}two\u{1e}three".to_owned(),
                    author: "Ana \u{d6}z".to_owned(),
                    authored_at: 1_700_000_000,
                    pushed: false,
                },
                CommitSummary {
                    sha: SHA_B.to_owned(),
                    short_sha: "89abcde".to_owned(),
                    subject: String::new(),
                    author: "Bo".to_owned(),
                    authored_at: 1_600_000_000,
                    pushed: false,
                },
            ]
        );
        assert!(parse_log(b"").is_empty());
        let long = format!("{SHA_A}\x000123456\x001\x00a\x00{}\n", "x".repeat(5000));
        assert_eq!(
            parse_log(long.as_bytes())[0].subject.chars().count(),
            MAX_SUBJECT_CHARS
        );
    }

    #[test]
    fn header_record() {
        let output = format!(
            "{SHA_A}\x000123456\x00{SHA_B} {SHA_A}\x00-5\x00Ana\x00Merge x\x00Line one\n\nLine \x1f three\n\n"
        );
        let header = parse_header(output.as_bytes()).unwrap();
        assert_eq!(header.sha, SHA_A);
        assert_eq!(header.parents, [SHA_B, SHA_A]);
        assert_eq!(header.authored_at, -5);
        assert_eq!(header.subject, "Merge x");
        assert_eq!(header.body, "Line one\n\nLine \u{1f} three");

        // Root commit: no parents, empty body.
        let root = format!("{SHA_A}\x000123456\x00\x001\x00Ana\x00first\x00\n");
        let header = parse_header(root.as_bytes()).unwrap();
        assert!(header.parents.is_empty());
        assert_eq!(header.body, "");

        // Long body: cut on a character boundary.
        let long = format!(
            "{SHA_A}\x000123456\x00\x001\x00a\x00s\x00{}",
            "\u{e4}".repeat(MAX_BODY_BYTES)
        );
        let body = parse_header(long.as_bytes()).unwrap().body;
        assert!(body.len() <= MAX_BODY_BYTES && body.len() >= MAX_BODY_BYTES - 1);

        for bad in [
            String::new(),
            format!("{SHA_A}\x000123456\x00\x001\x00a\x00s"),
            format!("{SHA_A}\x000123456\x00HEAD\x001\x00a\x00s\x00b"),
            "xyz\x00x\x00\x001\x00a\x00s\x00b".to_owned(),
        ] {
            assert_eq!(parse_header(bad.as_bytes()), None, "{bad:?}");
        }
    }

    #[test]
    fn name_status_records() {
        let output = b"M\0src/main.rs\0A\0new file.txt\0R087\0old.rs\0new.rs\0D\0gone.rs\0T\0link\0C100\0a\0b\0M\0cut";
        let summary: Vec<(String, String)> = parse_name_status(output)
            .into_iter()
            .map(|f| (f.status, f.path))
            .collect();
        let expected = [
            ("M ", "src/main.rs"),
            ("A ", "new file.txt"),
            ("R ", "new.rs"),
            ("D ", "gone.rs"),
            ("T ", "link"),
            ("C ", "b"),
        ];
        assert_eq!(summary, expected.map(|(s, p)| (s.to_owned(), p.to_owned())));
        assert!(parse_name_status(b"").is_empty());
        // Cut inside a rename: the rename is left out.
        assert!(parse_name_status(b"R100\0old.rs\0").is_empty());
        // Out of step: stops instead of reading paths as codes.
        assert!(parse_name_status(b"src/x\0M\0y\0").is_empty());
    }

    #[test]
    fn sha_lines() {
        let output = format!("{SHA_A}\n{SHA_B}\r\nshort\n{}", &SHA_A[..20]);
        let set = parse_sha_lines(output.as_bytes());
        assert_eq!(set.len(), 2);
        assert!(set.contains(SHA_A) && set.contains(SHA_B));
    }

    fn write(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
    }

    fn head(git: &Git, repo: &Path) -> String {
        let output = git
            .run(repo, ["rev-parse", "HEAD"], Instant::now() + TIMEOUT, 4096)
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn history_of_a_real_repository() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-history");
        let repo = tmp.path().join("repo with space");
        init_repo(&git, &repo);

        // No commits yet.
        let empty = log(&git, &repo, 0, 20).unwrap();
        assert_eq!(empty, GitLog::empty(true));
        assert!(matches!(
            commit_diff(&git, &repo, "abcdef1"),
            Err(GitError::UnknownCommit)
        ));

        // Root commit.
        write(&repo.join("a.txt"), "one\ntwo\n");
        write(&repo.join("b.txt"), "bee\n");
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "-q", "-m", "first"]);
        let first = head(&git, &repo);
        // Second: modify, add, with a body.
        write(&repo.join("a.txt"), "one\nzwei\n");
        write(&repo.join("c.txt"), "sea\n");
        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(
            &git,
            &repo,
            &[
                "commit",
                "-q",
                "-m",
                "second \u{e4}",
                "-m",
                "Body line\n\nmore",
            ],
        );
        let second = head(&git, &repo);
        // Third: rename and delete.
        git_ok(&git, &repo, &["mv", "c.txt", "renamed.txt"]);
        git_ok(&git, &repo, &["rm", "-q", "b.txt"]);
        git_ok(&git, &repo, &["commit", "-q", "-m", "third"]);
        let third = head(&git, &repo);

        // Without an upstream: everything unpushed, newest first.
        let all = log(&git, &repo, 0, 100).unwrap();
        assert!(all.is_repo && !all.has_more);
        assert_eq!(all.upstream, None);
        assert_eq!(all.ahead_count, 0);
        let shas: Vec<&str> = all.commits.iter().map(|c| c.sha.as_str()).collect();
        assert_eq!(shas, [third.as_str(), second.as_str(), first.as_str()]);
        assert!(all.commits.iter().all(|c| !c.pushed));
        assert_eq!(all.commits[1].subject, "second \u{e4}");
        assert_eq!(all.commits[1].author, "Hangar Test");
        assert!(all.commits[1].authored_at > 1_600_000_000);

        // Paging; a limit of 0 is clamped to 1.
        let page = log(&git, &repo, 0, 2).unwrap();
        assert_eq!(page.commits.len(), 2);
        assert!(page.has_more);
        let rest = log(&git, &repo, 2, 2).unwrap();
        assert_eq!(rest.commits.len(), 1);
        assert_eq!(rest.commits[0].sha, first);
        assert!(!rest.has_more);
        assert_eq!(log(&git, &repo, 0, 0).unwrap().commits.len(), 1);
        assert!(log(&git, &repo, 10, 5).unwrap().commits.is_empty());

        // An upstream that has the first commit only (a local branch serves as one).
        git_ok(&git, &repo, &["branch", "-q", "base", &first]);
        git_ok(&git, &repo, &["branch", "-q", "--set-upstream-to=base"]);
        let tracked = log(&git, &repo, 0, 100).unwrap();
        assert_eq!(tracked.upstream.as_deref(), Some("base"));
        assert_eq!(tracked.ahead_count, 2);
        let pushed: Vec<bool> = tracked.commits.iter().map(|c| c.pushed).collect();
        assert_eq!(pushed, [false, false, true]);

        // Root commit against the empty tree.
        let root = commit_diff(&git, &repo, &first).unwrap();
        assert_eq!(root.commit.sha, first);
        assert!(root.commit.parents.is_empty());
        assert_eq!(root.commit.subject, "first");
        assert!(root.changes.is_repo && !root.changes.truncated);
        let files: Vec<(&str, &str, u64, u64)> = root
            .changes
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str(), f.additions, f.deletions))
            .collect();
        assert_eq!(files, [("a.txt", "A ", 2, 0), ("b.txt", "A ", 1, 0)]);
        assert!(root.changes.diff.contains("+two\n"));

        // A short id from a subfolder; the header has the body.
        fs::create_dir_all(repo.join("sub")).unwrap();
        let middle = commit_diff(&git, &repo.join("sub"), &second[..10]).unwrap();
        assert_eq!(middle.commit.sha, second);
        assert_eq!(middle.commit.parents, std::slice::from_ref(&first));
        assert_eq!(middle.commit.body, "Body line\n\nmore");
        let a = middle
            .changes
            .files
            .iter()
            .find(|f| f.path == "a.txt")
            .unwrap();
        assert_eq!((a.status.as_str(), a.additions, a.deletions), ("M ", 1, 1));
        assert!(middle.changes.diff.contains("-two\n+zwei\n"));

        let last = commit_diff(&git, &repo, &third.to_uppercase()).unwrap();
        let statuses: Vec<(&str, &str)> = last
            .changes
            .files
            .iter()
            .map(|f| (f.status.as_str(), f.path.as_str()))
            .collect();
        assert!(statuses.contains(&("D ", "b.txt")), "{statuses:?}");
        assert!(statuses.contains(&("R ", "renamed.txt")), "{statuses:?}");

        // The JSON shape the window reads.
        let json = serde_json::to_value(&last).unwrap();
        for key in ["commit", "isRepo", "files", "diff", "truncated"] {
            assert!(json.get(key).is_some(), "{key} missing");
        }
        assert!(json["commit"].get("authoredAt").is_some());
        let json = serde_json::to_value(&tracked).unwrap();
        for key in ["isRepo", "commits", "upstream", "aheadCount", "hasMore"] {
            assert!(json.get(key).is_some(), "{key} missing");
        }
        assert!(json["commits"][0].get("shortSha").is_some());

        // Ids that are no commit, or no id at all.
        assert!(matches!(
            commit_diff(&git, &repo, "0000000"),
            Err(GitError::UnknownCommit)
        ));
        let tree = git
            .run(
                &repo,
                ["rev-parse", "HEAD^{tree}"],
                Instant::now() + TIMEOUT,
                4096,
            )
            .unwrap();
        let tree = String::from_utf8_lossy(&tree.stdout).trim().to_owned();
        assert!(matches!(
            commit_diff(&git, &repo, &tree),
            Err(GitError::UnknownCommit)
        ));
        for bad in ["-x", "HEAD", "abc", "--output=x"] {
            assert!(matches!(
                commit_diff(&git, &repo, bad),
                Err(GitError::InvalidCommit)
            ));
        }

        // Outside a work tree (the test folder lies inside Hangar's own checkout, so use the
        // git directory itself).
        assert_eq!(
            log(&git, &repo.join(".git"), 0, 10).unwrap(),
            GitLog::empty(false)
        );
        assert!(matches!(
            commit_diff(&git, &repo.join(".git"), &first),
            Err(GitError::NotARepository)
        ));
        assert!(matches!(
            log(&git, Path::new("relative"), 0, 10),
            Err(GitError::InvalidPath(_))
        ));
    }

    #[test]
    fn large_commit_is_cut_on_a_whole_line() {
        let Some(git) = git() else { return };
        let tmp = TempDir::new("git-history-big");
        let repo = tmp.path().join("repo");
        init_repo(&git, &repo);
        let big: String = (0..300_000).map(|i| format!("line {i}\n")).collect();
        write(&repo.join("big.txt"), &big);
        git_ok(&git, &repo, &["add", "big.txt"]);
        git_ok(&git, &repo, &["commit", "-q", "-m", "big"]);
        let result = commit_diff(&git, &repo, &head(&git, &repo)).unwrap();
        assert!(result.changes.truncated);
        assert!(result.changes.diff.len() <= MAX_DIFF_BYTES);
        assert!(result.changes.diff.ends_with('\n'));
        assert_eq!(result.changes.files[0].additions, 300_000);
    }
}
