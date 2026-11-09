//! Local claims (team service step A, docs/wiki/pages/syntheses/team-service-v1.md): which files
//! the agent of a running session changed, and where two of the user's own sessions in the same
//! repository touch the same file or the same lines.
//!
//! This module is the pure part: parsing git's output, building claims, comparing them and the
//! timing of rescans. It starts nothing and reads no files; `claims_service.rs` runs git and the
//! file watchers. Hangar never reads file contents for claims: only paths, the kind of change
//! and line ranges on the base side, taken from hunk headers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::session::SessionId;

/// Beyond this many hunks a file counts as changed as a whole: the ranges would say little and
/// cost a lot to compare.
pub const MAX_HUNKS_PER_FILE: usize = 64;
/// Two ranges closer than this many lines still count as overlapping: git cannot merge changes
/// that touch adjacent lines either, and a few lines of context are usually involved.
pub const LINE_PADDING: u32 = 3;
/// Claims per session sent to the window. Overlaps are computed on all of them.
pub const MAX_LISTED_CLAIMS: usize = 2000;

/// What happened to a file, relative to the session's base commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Untracked,
    Conflicted,
}

/// Lines `start..=end` (1-based) on the base side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    /// Within [`LINE_PADDING`] lines of each other.
    pub fn near(self, other: LineRange) -> bool {
        self.start <= other.end.saturating_add(LINE_PADDING)
            && other.start <= self.end.saturating_add(LINE_PADDING)
    }
}

/// One changed file of a session's worktree. The repository and the base commit belong to the
/// [`ScanResult`] the claim is part of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    /// Relative to the worktree root, with `/`, as git prints it.
    pub path: String,
    pub kind: ChangeKind,
    /// The whole file counts (new, deleted, binary, untracked, too many hunks, or no base).
    pub whole_file: bool,
    /// Merged, sorted ranges on the base side; empty for a whole file.
    pub ranges: Vec<LineRange>,
    /// First folder of the path; `None` for files at the root. A setting per repository may
    /// define areas later (team service plan, step A).
    pub area: Option<String>,
}

/// How much two sessions' changes collide; ordered from harmless to worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OverlapLevel {
    #[default]
    None,
    SameArea,
    SameFile,
    OverlappingLines,
}

/// One collision, seen from `session_id`. Every collision is listed once from each side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overlap {
    pub session_id: SessionId,
    pub other_id: SessionId,
    pub level: OverlapLevel,
    /// The file, for [`OverlapLevel::SameFile`] and [`OverlapLevel::OverlappingLines`].
    pub path: Option<String>,
    /// The area, for [`OverlapLevel::SameArea`].
    pub area: Option<String>,
    /// Lines where both changed, for [`OverlapLevel::OverlappingLines`].
    pub lines: Vec<LineRange>,
}

/// Result of one rescan of a worktree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanResult {
    /// HEAD of the worktree; `None` before the first commit.
    pub base: Option<String>,
    pub claims: Vec<Claim>,
    /// git's output hit a limit; claims past it may be missing or count as whole files.
    pub truncated: bool,
}

// ---------------------------------------------------------------------------------------------
// Keys

/// Folder key: resolved where possible, `/` separators, no trailing separator, lower case where
/// the file system ignores case (Windows, macOS). Two spellings of one folder give one key.
pub fn folder_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text.into_owned()
    };
    let mut key = text.replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    if cfg!(any(windows, target_os = "macos")) {
        key = key.to_lowercase();
    }
    key
}

/// `key` is `folder` or lies below it (both from [`folder_key`]).
pub fn key_within(key: &str, folder: &str) -> bool {
    key == folder
        || key
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Comparison key of a path inside a repository, case-insensitive where the file system is.
fn file_key(path: &str) -> String {
    if cfg!(any(windows, target_os = "macos")) {
        path.to_lowercase()
    } else {
        path.to_owned()
    }
}

pub fn area_of(path: &str) -> Option<String> {
    path.split_once('/')
        .map(|(first, _)| first.to_owned())
        .filter(|first| !first.is_empty())
}

// ---------------------------------------------------------------------------------------------
// git status --porcelain=v2 -z --branch

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusScan {
    pub base: Option<String>,
    pub files: Vec<(String, ChangeKind)>,
}

fn kind_of(xy: &str) -> ChangeKind {
    if xy.contains('D') {
        ChangeKind::Deleted
    } else if xy.contains('A') {
        ChangeKind::Added
    } else {
        ChangeKind::Modified
    }
}

/// Records: `# branch.oid <oid>|(initial)`, `1 XY sub mH mI mW hH hI path`,
/// `2 XY sub mH mI mW hH hI Xscore path\0orig`, `u XY sub m1 m2 m3 mW h1 h2 h3 path`, `? path`,
/// `! path`. Paths are not quoted with `-z`. An incomplete last record (cut output) is skipped
/// when it has too few fields.
pub fn parse_status_v2(output: &[u8]) -> StatusScan {
    let mut scan = StatusScan::default();
    let mut records = output.split(|b| *b == 0);
    while let Some(record) = records.next() {
        let text = String::from_utf8_lossy(record);
        if let Some(oid) = text.strip_prefix("# branch.oid ") {
            let oid = oid.trim();
            if oid.len() >= 7 && oid.bytes().all(|b| b.is_ascii_hexdigit()) {
                scan.base = Some(oid.to_ascii_lowercase());
            }
            continue;
        }
        let (fields, kind) = match text.get(..2) {
            Some("1 ") => (9, None),
            Some("2 ") => (10, None),
            Some("u ") => (11, Some(ChangeKind::Conflicted)),
            Some("? ") => {
                scan.files
                    .push((text[2..].to_owned(), ChangeKind::Untracked));
                continue;
            }
            _ => continue,
        };
        if text.starts_with("2 ") {
            // The original path follows as a record of its own (only with renames, which the
            // rescan turns off).
            records.next();
        }
        let parts: Vec<&str> = text.splitn(fields, ' ').collect();
        let (Some(xy), Some(path)) = (parts.get(1), parts.get(fields - 1)) else {
            continue;
        };
        if parts.len() != fields || path.is_empty() {
            continue;
        }
        let kind = kind.unwrap_or_else(|| kind_of(xy));
        scan.files.push(((*path).to_owned(), kind));
    }
    scan
}

// ---------------------------------------------------------------------------------------------
// git diff -U0

/// Hunks of one file in a `-U0` diff.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileHunks {
    pub ranges: Vec<LineRange>,
    pub hunks: usize,
    pub binary: bool,
    /// The output ended inside this file; its hunks may be incomplete.
    pub cut: bool,
}

/// Base side of a hunk header `@@ -a[,b] +c[,d] @@`. A pure insertion (`b = 0`) after line `a`
/// touches the boundary between `a` and `a + 1`.
pub fn parse_hunk_header(line: &str) -> Option<LineRange> {
    let rest = line.strip_prefix("@@ -")?;
    let old = rest.split(' ').next()?;
    let (start, count) = match old.split_once(',') {
        Some((start, count)) => (start.parse::<u32>().ok()?, count.parse::<u32>().ok()?),
        None => (old.parse::<u32>().ok()?, 1),
    };
    if count == 0 {
        Some(LineRange {
            start: start.max(1),
            end: start.saturating_add(1).max(1),
        })
    } else {
        Some(LineRange {
            start,
            end: start.saturating_add(count - 1),
        })
    }
}

/// C-style quoted path as git prints it (`"a/x\"y\303\244"`), without the quotes' content
/// prefix handling. Returns the bytes and the rest after the closing quote.
fn unquote_c(text: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let mut rest = text.strip_prefix(b"\"")?;
    let mut out = Vec::new();
    loop {
        let (&byte, tail) = rest.split_first()?;
        rest = tail;
        match byte {
            b'"' => return Some((out, rest)),
            b'\\' => {
                let (&escaped, tail) = rest.split_first()?;
                rest = tail;
                let plain = match escaped {
                    b'a' => Some(0x07),
                    b'b' => Some(0x08),
                    b'f' => Some(0x0c),
                    b'n' => Some(b'\n'),
                    b'r' => Some(b'\r'),
                    b't' => Some(b'\t'),
                    b'v' => Some(0x0b),
                    b'\\' | b'"' => Some(escaped),
                    _ => None,
                };
                if let Some(plain) = plain {
                    out.push(plain);
                    continue;
                }
                // Three octal digits.
                let digits = [escaped, *rest.first()?, *rest.get(1)?];
                if !digits.iter().all(|d| (b'0'..=b'7').contains(d)) {
                    return None;
                }
                rest = &rest[2..];
                let value = digits
                    .iter()
                    .fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0'));
                out.push(u8::try_from(value).ok()?);
            }
            other => out.push(other),
        }
    }
}

/// Path from `diff --git a/<p> b/<p>` (the part after `diff --git `). Without renames both sides
/// name the same path, which makes a path with spaces unambiguous.
pub fn parse_diff_path(rest: &[u8]) -> Option<String> {
    if rest.first() == Some(&b'"') {
        let (path, _) = unquote_c(rest)?;
        let path = path.strip_prefix(b"a/")?;
        return Some(String::from_utf8_lossy(path).into_owned());
    }
    let length = rest.len().checked_sub(5)?;
    if length % 2 != 0 {
        return None;
    }
    let half = length / 2;
    let (left, right) = (rest.get(..half + 2)?, rest.get(half + 3..)?);
    let path = left.strip_prefix(b"a/")?;
    if rest.get(half + 2) != Some(&b' ') || right.strip_prefix(b"b/")? != path {
        return None;
    }
    Some(String::from_utf8_lossy(path).into_owned())
}

/// Hunk ranges per path from `git diff -U0`. Content lines are skipped unread; only headers count.
pub fn parse_diff(output: &[u8], truncated: bool) -> HashMap<String, FileHunks> {
    let mut files: HashMap<String, FileHunks> = HashMap::new();
    let mut current: Option<String> = None;
    for line in output.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some(rest) = line.strip_prefix(b"diff --git ") {
            current = parse_diff_path(rest);
            if let Some(path) = &current {
                files.entry(path.clone()).or_default();
            }
            continue;
        }
        let Some(path) = &current else { continue };
        let Some(entry) = files.get_mut(path) else {
            continue;
        };
        if line.starts_with(b"@@ -") {
            entry.hunks += 1;
            if entry.hunks <= MAX_HUNKS_PER_FILE {
                if let Some(range) = parse_hunk_header(&String::from_utf8_lossy(line)) {
                    entry.ranges.push(range);
                }
            }
        } else if line.starts_with(b"Binary files ") || line.starts_with(b"GIT binary patch") {
            entry.binary = true;
        }
    }
    if truncated {
        if let Some(entry) = current.and_then(|path| files.get_mut(&path)) {
            entry.cut = true;
        }
    }
    files
}

/// Sorted, with overlapping and adjacent ranges joined.
pub fn merge_ranges(mut ranges: Vec<LineRange>) -> Vec<LineRange> {
    ranges.sort();
    let mut merged: Vec<LineRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end.saturating_add(1) => {
                last.end = last.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }
    merged
}

/// Claims from a status and, when the worktree has a base commit, the `-U0` diff against it.
/// `diff_truncated`: files the diff did not reach count as whole files.
pub fn build_claims(
    status: StatusScan,
    diff: Option<&HashMap<String, FileHunks>>,
    diff_truncated: bool,
) -> ScanResult {
    let has_base = status.base.is_some();
    let claims = status
        .files
        .into_iter()
        .map(|(path, kind)| {
            let hunks = diff.and_then(|diff| diff.get(&path));
            let ranges = match (kind, hunks) {
                (ChangeKind::Modified, Some(hunks))
                    if has_base
                        && !hunks.binary
                        && !hunks.cut
                        && hunks.hunks > 0
                        && hunks.hunks <= MAX_HUNKS_PER_FILE =>
                {
                    merge_ranges(hunks.ranges.clone())
                }
                _ => Vec::new(),
            };
            Claim {
                area: area_of(&path),
                whole_file: ranges.is_empty(),
                ranges,
                kind,
                path,
            }
        })
        .collect();
    ScanResult {
        base: status.base,
        claims,
        truncated: diff_truncated,
    }
}

// ---------------------------------------------------------------------------------------------
// Overlaps

/// One worktree with its sessions, as the overlap computation sees it.
#[derive(Debug, Clone)]
pub struct RootView<'a> {
    /// Repository key (the git common folder, [`folder_key`]).
    pub repo: &'a str,
    pub base: Option<&'a str>,
    /// Sessions in this worktree. Several sessions in one folder share its changes; they are
    /// never compared with each other.
    pub sessions: &'a [SessionId],
    pub claims: &'a [Claim],
}

/// Where both changed, padded by [`LINE_PADDING`]: the span of each pair of near ranges.
fn shared_lines(a: &[LineRange], b: &[LineRange]) -> Vec<LineRange> {
    let mut spans = Vec::new();
    for x in a {
        for y in b {
            if x.near(*y) {
                spans.push(LineRange {
                    start: x.start.min(y.start),
                    end: x.end.max(y.end),
                });
            }
        }
    }
    merge_ranges(spans)
}

/// Collision of two claims on the same path.
fn file_level(a: &Claim, b: &Claim, same_base: bool) -> (OverlapLevel, Vec<LineRange>) {
    if same_base && !a.whole_file && !b.whole_file {
        let lines = shared_lines(&a.ranges, &b.ranges);
        if !lines.is_empty() {
            return (OverlapLevel::OverlappingLines, lines);
        }
    }
    (OverlapLevel::SameFile, Vec::new())
}

/// Overlaps between worktrees of the same repository. On different base commits line numbers
/// mean different things, so only the file level is reported there.
pub fn overlaps(roots: &[RootView<'_>]) -> Vec<Overlap> {
    let mut found = Vec::new();
    for (index, a) in roots.iter().enumerate() {
        for b in roots.iter().skip(index + 1) {
            if a.repo != b.repo || a.sessions.is_empty() || b.sessions.is_empty() {
                continue;
            }
            let same_base = a.base.is_some() && a.base == b.base;
            let by_path: HashMap<String, &Claim> =
                b.claims.iter().map(|c| (file_key(&c.path), c)).collect();
            let mut pair = Vec::new();
            let mut file_areas = BTreeSet::new();
            for claim in a.claims {
                let Some(other) = by_path.get(&file_key(&claim.path)) else {
                    continue;
                };
                let (level, lines) = file_level(claim, other, same_base);
                file_areas.insert(claim.area.clone());
                pair.push((level, Some(claim.path.clone()), None, lines));
            }
            let areas_a: BTreeSet<&String> =
                a.claims.iter().filter_map(|c| c.area.as_ref()).collect();
            let areas_b: BTreeSet<String> = b
                .claims
                .iter()
                .filter_map(|c| c.area.as_deref().map(file_key))
                .collect();
            for area in areas_a {
                if areas_b.contains(&file_key(area)) && !file_areas.contains(&Some(area.clone())) {
                    pair.push((OverlapLevel::SameArea, None, Some(area.clone()), Vec::new()));
                }
            }
            for &x in a.sessions {
                for &y in b.sessions {
                    for (level, path, area, lines) in &pair {
                        for (session_id, other_id) in [(x, y), (y, x)] {
                            found.push(Overlap {
                                session_id,
                                other_id,
                                level: *level,
                                path: path.clone(),
                                area: area.clone(),
                                lines: lines.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    found.sort_by(|x, y| {
        (
            x.session_id,
            std::cmp::Reverse(x.level),
            &x.path,
            &x.area,
            x.other_id,
        )
            .cmp(&(
                y.session_id,
                std::cmp::Reverse(y.level),
                &y.path,
                &y.area,
                y.other_id,
            ))
    });
    found
}

/// Worst level per session.
pub fn levels(overlaps: &[Overlap]) -> BTreeMap<SessionId, OverlapLevel> {
    let mut levels = BTreeMap::new();
    for overlap in overlaps {
        let level: &mut OverlapLevel = levels.entry(overlap.session_id).or_default();
        *level = (*level).max(overlap.level);
    }
    levels
}

// ---------------------------------------------------------------------------------------------
// Snapshot for the window

/// Where the claims of one session stand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClaimState {
    /// Not scanned yet.
    Pending,
    Ready,
    /// The working folder is not inside a git repository.
    NotRepo,
    /// git failed or refused the folder; the message is git's or Hangar's.
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionClaims {
    pub session_id: SessionId,
    pub state: ClaimState,
    /// Worktree root the session works in.
    pub root: Option<String>,
    pub base: Option<String>,
    /// Other sessions in the same folder: they see the same changes, which belong to the folder
    /// rather than to one session.
    pub shared_with: Vec<SessionId>,
    pub claims: Vec<Claim>,
    /// More claims than [`MAX_LISTED_CLAIMS`], or git's output was cut.
    pub truncated: bool,
    pub level: OverlapLevel,
}

/// Event `claims-changed` and the answer of `claims_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaimsSnapshot {
    pub enabled: bool,
    pub sessions: Vec<SessionClaims>,
    pub overlaps: Vec<Overlap>,
}

// ---------------------------------------------------------------------------------------------
// Timing of rescans

/// Quiet time after the last file event before a rescan.
pub const QUIET: Duration = Duration::from_millis(750);
/// A steady stream of events still gets a rescan this long after its first event.
pub const MAX_WAIT: Duration = Duration::from_secs(3);
/// Shortest gap between two rescans of one worktree.
pub const MIN_GAP: Duration = Duration::from_secs(1);
/// The gap grows with the cost of the last rescan: at most a tenth of the time in git.
pub const GAP_FACTOR: u32 = 10;
pub const MAX_GAP: Duration = Duration::from_secs(30);
/// Safety poll with a working watcher: repairs missed events.
pub const POLL_WATCHED: Duration = Duration::from_secs(60);
/// Poll without a watcher (network folder, watcher failed or limit reached).
pub const POLL_ONLY: Duration = Duration::from_secs(30);

/// Why a rescan is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanReason {
    Events,
    Poll,
}

/// Decides when a worktree is scanned again: events are coalesced (quiet time, maximum wait),
/// scans keep a gap that grows with their cost, and a slow poll runs regardless. Pure; the
/// caller passes the time.
#[derive(Debug, Clone)]
pub struct Coalescer {
    first: Option<Instant>,
    last: Option<Instant>,
    not_before: Instant,
    next_poll: Instant,
}

impl Coalescer {
    /// Due at once: the first scan follows the start.
    pub fn new(now: Instant) -> Self {
        Self {
            first: None,
            last: None,
            not_before: now,
            next_poll: now,
        }
    }

    pub fn event(&mut self, at: Instant) {
        self.first.get_or_insert(at);
        self.last = Some(self.last.map_or(at, |last| last.max(at)));
    }

    pub fn pending(&self) -> bool {
        self.first.is_some()
    }

    /// Earliest time a scan becomes due.
    pub fn deadline(&self) -> Instant {
        match (self.first, self.last) {
            (Some(first), Some(last)) => {
                let settled = (last + QUIET).min(first + MAX_WAIT);
                settled.max(self.not_before).min(self.next_poll)
            }
            _ => self.next_poll,
        }
    }

    pub fn due(&self, now: Instant) -> Option<ScanReason> {
        if self.pending() && now >= self.deadline() {
            return Some(ScanReason::Events);
        }
        (now >= self.next_poll).then_some(ScanReason::Poll)
    }

    /// A scan starts: events so far are covered by it.
    pub fn begin(&mut self) {
        self.first = None;
        self.last = None;
    }

    /// A scan that started at `started` ended at `now`; the next poll follows after `poll`.
    pub fn finished(&mut self, started: Instant, now: Instant, poll: Duration) {
        let cost = now.saturating_duration_since(started);
        let gap = cost
            .saturating_mul(GAP_FACTOR)
            .clamp(MIN_GAP, MAX_GAP.max(MIN_GAP));
        self.not_before = now + gap;
        self.next_poll = now + poll.max(gap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(start: u32, end: u32) -> LineRange {
        LineRange { start, end }
    }

    #[test]
    fn hunk_headers() {
        assert_eq!(
            parse_hunk_header("@@ -10,3 +10,4 @@ fn x()"),
            Some(r(10, 12))
        );
        assert_eq!(parse_hunk_header("@@ -7 +7 @@"), Some(r(7, 7)));
        assert_eq!(parse_hunk_header("@@ -7,1 +7,2 @@"), Some(r(7, 7)));
        // Pure insertion after line 5, and at the very top.
        assert_eq!(parse_hunk_header("@@ -5,0 +6,2 @@"), Some(r(5, 6)));
        assert_eq!(parse_hunk_header("@@ -0,0 +1,3 @@"), Some(r(1, 1)));
        // Pure deletion.
        assert_eq!(parse_hunk_header("@@ -20,4 +19,0 @@"), Some(r(20, 23)));
        for bad in [
            "",
            "@@ +1,2 @@",
            "@@ -x,1 +1 @@",
            "@@ -1,y +1 @@",
            "diff --git",
        ] {
            assert_eq!(parse_hunk_header(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn status_v2_records() {
        let output = b"# branch.oid 0123456789ABCDEF0123456789abcdef01234567\0\
# branch.head main\0\
1 .M N... 100644 100644 100644 aaaa bbbb src/main.rs\0\
1 A. N... 000000 100644 100644 0000 cccc new file.txt\0\
1 .D N... 100644 100644 000000 aaaa aaaa gone.rs\0\
2 R. N... 100644 100644 100644 aaaa aaaa R100 renamed.rs\0original.rs\0\
u UU N... 100644 100644 100644 100644 a b c both.rs\0\
? untracked dir/x.txt\0\
! ignored.log\0\
1 .M N... 100644";
        let scan = parse_status_v2(output);
        assert_eq!(
            scan.base.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(
            scan.files,
            [
                ("src/main.rs".to_owned(), ChangeKind::Modified),
                ("new file.txt".to_owned(), ChangeKind::Added),
                ("gone.rs".to_owned(), ChangeKind::Deleted),
                ("renamed.rs".to_owned(), ChangeKind::Modified),
                ("both.rs".to_owned(), ChangeKind::Conflicted),
                ("untracked dir/x.txt".to_owned(), ChangeKind::Untracked),
            ]
        );
        let initial = parse_status_v2(b"# branch.oid (initial)\0? a\0");
        assert_eq!(initial.base, None);
        assert_eq!(initial.files.len(), 1);
        assert_eq!(parse_status_v2(b""), StatusScan::default());
    }

    #[test]
    fn diff_paths() {
        assert_eq!(
            parse_diff_path(b"a/src/main.rs b/src/main.rs").as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(
            parse_diff_path(b"a/with space b/x b/with space b/x").as_deref(),
            Some("with space b/x")
        );
        assert_eq!(
            parse_diff_path(br#""a/tab\there \"q\" \303\244" "b/tab\there \"q\" \303\244""#)
                .as_deref(),
            Some("tab\there \"q\" ä")
        );
        for bad in [&b"a/x b/y"[..], b"x", b"", b"\"a/open", b"\"a/\\9xx\""] {
            assert_eq!(parse_diff_path(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn diff_hunks_per_file() {
        let output = b"diff --git a/a.rs b/a.rs\n\
index 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n\
@@ -3,2 +3,2 @@\n-old\n-old\n+new\n+new\n\
@@ -10,0 +11 @@ fn x\n+diff --git a/fake b/fake\n\
diff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n\
diff --git a/c.rs b/c.rs\n@@ -1 +1 @@\n-x\n+y\n@@ -5,";
        let files = parse_diff(output, true);
        let a = &files["a.rs"];
        assert_eq!(a.ranges, [r(3, 4), r(10, 11)]);
        assert!(!a.binary && !a.cut);
        assert!(files["img.png"].binary);
        assert!(files["c.rs"].cut);
        // A content line that looks like a header is not one.
        assert!(!files.contains_key("fake"));
    }

    #[test]
    fn ranges_merge() {
        assert_eq!(
            merge_ranges(vec![r(10, 12), r(1, 2), r(3, 3), r(11, 20), r(30, 30)]),
            [r(1, 3), r(10, 20), r(30, 30)]
        );
        assert!(r(1, 2).near(r(5, 6)));
        assert!(!r(1, 2).near(r(6, 6)));
    }

    #[test]
    fn claims_from_status_and_diff() {
        let status = StatusScan {
            base: Some("abc1234".into()),
            files: vec![
                ("src/a.rs".into(), ChangeKind::Modified),
                ("src/img.png".into(), ChangeKind::Modified),
                ("README.md".into(), ChangeKind::Untracked),
                ("docs/gone.md".into(), ChangeKind::Deleted),
                ("many.rs".into(), ChangeKind::Modified),
                ("mode-only.sh".into(), ChangeKind::Modified),
            ],
        };
        let mut diff = HashMap::new();
        diff.insert(
            "src/a.rs".to_owned(),
            FileHunks {
                ranges: vec![r(5, 5), r(1, 2)],
                hunks: 2,
                ..FileHunks::default()
            },
        );
        diff.insert(
            "src/img.png".to_owned(),
            FileHunks {
                binary: true,
                ..FileHunks::default()
            },
        );
        diff.insert(
            "many.rs".to_owned(),
            FileHunks {
                ranges: vec![r(1, 1)],
                hunks: MAX_HUNKS_PER_FILE + 1,
                ..FileHunks::default()
            },
        );
        diff.insert("mode-only.sh".to_owned(), FileHunks::default());
        let result = build_claims(status, Some(&diff), false);
        let by_path: HashMap<&str, &Claim> =
            result.claims.iter().map(|c| (c.path.as_str(), c)).collect();
        assert_eq!(by_path["src/a.rs"].ranges, [r(1, 2), r(5, 5)]);
        assert!(!by_path["src/a.rs"].whole_file);
        assert_eq!(by_path["src/a.rs"].area.as_deref(), Some("src"));
        for whole in [
            "src/img.png",
            "README.md",
            "docs/gone.md",
            "many.rs",
            "mode-only.sh",
        ] {
            assert!(by_path[whole].whole_file, "{whole}");
            assert!(by_path[whole].ranges.is_empty(), "{whole}");
        }
        assert_eq!(by_path["README.md"].area, None);
    }

    fn claim(path: &str, ranges: &[LineRange]) -> Claim {
        Claim {
            path: path.into(),
            kind: ChangeKind::Modified,
            whole_file: ranges.is_empty(),
            ranges: ranges.to_vec(),
            area: area_of(path),
        }
    }

    #[test]
    fn overlap_levels() {
        let a = [
            claim("src/x.rs", &[r(10, 12)]),
            claim("src/y.rs", &[r(1, 1)]),
            claim("docs/a.md", &[]),
            claim("top.txt", &[r(1, 1)]),
        ];
        let b = [
            claim("src/x.rs", &[r(15, 15)]),
            claim("src/y.rs", &[r(50, 50)]),
            claim("docs/b.md", &[]),
            claim("other.txt", &[r(1, 1)]),
        ];
        let roots = [
            RootView {
                repo: "repo",
                base: Some("b1"),
                sessions: &[1],
                claims: &a,
            },
            RootView {
                repo: "repo",
                base: Some("b1"),
                sessions: &[2],
                claims: &b,
            },
        ];
        let found = overlaps(&roots);
        let from_one: Vec<_> = found.iter().filter(|o| o.session_id == 1).collect();
        assert_eq!(from_one.len(), 3, "{from_one:#?}");
        assert_eq!(from_one[0].level, OverlapLevel::OverlappingLines);
        assert_eq!(from_one[0].path.as_deref(), Some("src/x.rs"));
        assert_eq!(from_one[0].lines, [r(10, 15)]);
        assert_eq!(from_one[0].other_id, 2);
        assert_eq!(from_one[1].level, OverlapLevel::SameFile);
        assert_eq!(from_one[1].path.as_deref(), Some("src/y.rs"));
        // "src" already collides on files; "docs" only shares the area. Root files have none.
        assert_eq!(from_one[2].level, OverlapLevel::SameArea);
        assert_eq!(from_one[2].area.as_deref(), Some("docs"));
        assert_eq!(found.len(), 6);
        assert_eq!(levels(&found)[&2], OverlapLevel::OverlappingLines);

        // Different bases: lines say nothing, file level only.
        let mut other_base = roots.clone();
        other_base[1].base = Some("b2");
        let found = overlaps(&other_base);
        assert!(found
            .iter()
            .all(|o| o.level != OverlapLevel::OverlappingLines));
        assert!(found
            .iter()
            .any(|o| o.level == OverlapLevel::SameFile && o.path.as_deref() == Some("src/x.rs")));

        // Another repository, or one root with two sessions: nothing.
        let mut other_repo = roots.clone();
        other_repo[1].repo = "elsewhere";
        assert!(overlaps(&other_repo).is_empty());
        let shared = [RootView {
            repo: "repo",
            base: Some("b1"),
            sessions: &[1, 2],
            claims: &a,
        }];
        assert!(overlaps(&shared).is_empty());
        assert!(levels(&[]).is_empty());
    }

    #[test]
    fn overlaps_expand_to_every_session_of_a_shared_folder() {
        let a = [claim("src/x.rs", &[])];
        let roots = [
            RootView {
                repo: "repo",
                base: None,
                sessions: &[1, 3],
                claims: &a,
            },
            RootView {
                repo: "repo",
                base: None,
                sessions: &[2],
                claims: &a,
            },
        ];
        let found = overlaps(&roots);
        let pairs: BTreeSet<(SessionId, SessionId)> =
            found.iter().map(|o| (o.session_id, o.other_id)).collect();
        assert_eq!(
            pairs,
            [(1, 2), (2, 1), (2, 3), (3, 2)].into_iter().collect()
        );
        assert!(found.iter().all(|o| o.level == OverlapLevel::SameFile));
    }

    #[test]
    fn folder_keys() {
        assert!(key_within("c:/a/b", "c:/a"));
        assert!(key_within("c:/a", "c:/a"));
        assert!(!key_within("c:/ab", "c:/a"));
        let key = folder_key(Path::new("/does/not/exist/"));
        assert!(!key.ends_with('/'));
        assert!(!key.contains('\\'));
    }

    #[cfg(windows)]
    #[test]
    fn folder_keys_ignore_case_and_prefix_on_windows() {
        assert_eq!(
            folder_key(Path::new(r"\\?\C:\Missing\Repo\")),
            folder_key(Path::new(r"c:\missing\repo"))
        );
        assert_eq!(
            folder_key(Path::new(r"\\?\UNC\Server\Share\x")),
            "//server/share/x"
        );
    }

    #[test]
    fn coalescing() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut c = Coalescer::new(t0);
        // The first scan is due at once, as a poll.
        assert_eq!(c.due(t0), Some(ScanReason::Poll));
        c.begin();
        c.finished(t0, ms(100), POLL_WATCHED);
        // 100 ms of git: the gap is the 1 s floor.
        assert_eq!(c.due(ms(200)), None);

        // Events in a burst: due 750 ms after the last one, not before the gap ends.
        c.event(ms(150));
        c.event(ms(300));
        assert_eq!(c.deadline(), ms(1100));
        assert_eq!(c.due(ms(1000)), None);
        assert_eq!(c.due(ms(1100)), Some(ScanReason::Events));

        // A steady stream: due 3 s after its first event at the latest.
        c.begin();
        c.finished(ms(1100), ms(1200), POLL_WATCHED);
        for n in (2000..6000).step_by(500) {
            c.event(ms(n));
        }
        assert_eq!(c.deadline(), ms(5000));
        assert_eq!(c.due(ms(4999)), None);
        assert_eq!(c.due(ms(5000)), Some(ScanReason::Events));

        // An expensive scan (1.2 s) pushes the next one 12 s out.
        c.begin();
        c.finished(ms(5000), ms(6200), POLL_WATCHED);
        c.event(ms(6300));
        assert_eq!(c.deadline(), ms(18_200));
        // The gap never exceeds MAX_GAP, and the poll follows without events.
        let mut slow = Coalescer::new(t0);
        slow.begin();
        slow.finished(t0, ms(10_000), POLL_ONLY);
        assert_eq!(slow.deadline(), ms(10_000) + POLL_ONLY.max(MAX_GAP));
        assert_eq!(slow.due(ms(10_000) + POLL_ONLY), Some(ScanReason::Poll));
        assert!(!slow.pending());
    }

    #[test]
    fn snapshot_serializes_camel_case() {
        let snapshot = ClaimsSnapshot {
            enabled: true,
            sessions: vec![SessionClaims {
                session_id: 1,
                state: ClaimState::Failed {
                    message: "x".into(),
                },
                root: None,
                base: None,
                shared_with: vec![2],
                claims: vec![claim("src/a.rs", &[r(1, 2)])],
                truncated: false,
                level: OverlapLevel::OverlappingLines,
            }],
            overlaps: Vec::new(),
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        let session = &json["sessions"][0];
        assert_eq!(session["sessionId"], 1);
        assert_eq!(session["state"]["kind"], "failed");
        assert_eq!(session["state"]["message"], "x");
        assert_eq!(session["sharedWith"][0], 2);
        assert_eq!(session["level"], "overlappingLines");
        assert_eq!(session["claims"][0]["wholeFile"], false);
        assert_eq!(session["claims"][0]["kind"], "modified");
        assert_eq!(session["claims"][0]["ranges"][0]["start"], 1);
        assert_eq!(
            serde_json::to_value(ClaimState::NotRepo).unwrap()["kind"],
            "notRepo"
        );
    }
}
