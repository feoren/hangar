//! What this device shares with a team and what it shows of the others (team service step F
//! part 2b; design: docs/wiki/pages/syntheses/team-service-v1.md, "Step F as built (part 2b)").
//!
//! Pure: the sealed documents' plaintext (presence status and claim detail) with their size
//! limits, the keyed hashes of `claim.set`, refining the relay's "same file" to overlapping
//! lines with step A's rule, and the window's view of the teammates. The protocol engine
//! (`team_engine.rs`) seals, sends and opens; nothing here touches a key, a socket or a file.
//!
//! What leaves the device (sealed, readable by the team only): the person's display name from the
//! website (never the e-mail address; empty when the account has none), per shared session the
//! agent program, the session title, its state (working, waiting, idle), the repository's root
//! commit, the changed paths and line ranges on the base side, and the base commit. Never file
//! contents, diffs or prompts. Only sessions in repository folders the person linked to the
//! project are shared at all (root commit and clone folder, `team::LinkedRepo`).

use std::collections::BTreeMap;

use hangar_team_protocol::hash::{CaseRule, KeyedHash, MatchKey, PathHashes, RepoId, RepoPath};
use hangar_team_protocol::limits::MAX_CLAIM_FILES;
use serde::{Deserialize, Serialize};

use crate::claims::{self, Claim, LineRange, OverlapLevel};
use crate::session::SessionId;

/// Version of both sealed documents.
const DOC_VERSION: u32 = 1;
/// Longest name, agent and title kept from a teammate's document, in characters.
const MAX_LABEL_CHARS: usize = 120;
/// Sessions read from one teammate's presence.
const MAX_PEER_SESSIONS: usize = 64;
/// Files read from one teammate's presence or claim detail.
const MAX_PEER_FILES: usize = 2000;
/// Ranges read per file of a claim detail.
const MAX_PEER_RANGES: usize = claims::MAX_HUNKS_PER_FILE;
/// Longest path read from a teammate, in bytes (the protocol's path limit).
const MAX_PATH_BYTES: usize = 4096;
/// Longest session key read from a teammate (the protocol's session key limit).
const MAX_SESSION_KEY: usize = 64;
/// Characters of a member id kept by [`person_tag`], like the window's `shortId`.
const SHORT_ID: usize = 8;

/// What a session is doing, as the window's attention logic sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    /// The agent works (output).
    Working,
    /// The agent waits for its person.
    Waiting,
    #[default]
    Idle,
}

/// One of this device's running agent sessions in a git repository with a root commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSession {
    pub id: SessionId,
    /// The program's name, as the session list shows it.
    pub agent: String,
    pub title: String,
    pub state: SessionState,
    /// The repository's root commit (`RepoId`).
    pub repo: String,
    /// The clone on this device: its git common folder (`WorktreeClaims::repo`), shared by all
    /// its worktrees. With `repo`, what a link to a project names.
    pub folder: String,
    /// The worktree's folder name, to name the repository when linking it.
    pub repo_name: String,
    /// HEAD of the worktree.
    pub base: Option<String>,
    pub claims: Vec<Claim>,
    /// git's output was cut: claims past it may be missing.
    pub truncated: bool,
}

/// What one shared project gets from this device: the person's display name and the sessions in
/// the repositories linked to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalShare {
    pub person: String,
    pub sessions: Vec<LocalSession>,
}

/// A running agent session as the session list names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLabel {
    pub id: SessionId,
    pub title: String,
    /// The program's name ("Claude Code").
    pub agent: String,
}

/// This device's sessions in repositories with a known root commit, from the claims of step A
/// (every session of a worktree gets its claims), the session list and the window's states.
pub fn local_sessions(
    worktrees: &[claims::WorktreeClaims],
    labels: &[SessionLabel],
    states: &BTreeMap<SessionId, SessionState>,
) -> Vec<LocalSession> {
    let mut sessions = Vec::new();
    for worktree in worktrees {
        let Some(repo) = &worktree.root_commit else {
            continue;
        };
        for id in &worktree.sessions {
            let Some(label) = labels.iter().find(|l| l.id == *id) else {
                continue;
            };
            sessions.push(LocalSession {
                id: *id,
                agent: label.agent.clone(),
                title: label.title.clone(),
                state: states.get(id).copied().unwrap_or_default(),
                repo: repo.clone(),
                folder: worktree.repo.clone(),
                repo_name: worktree.name.clone(),
                base: worktree.base.clone(),
                claims: worktree.claims.clone(),
                truncated: worktree.truncated,
            });
        }
    }
    sessions.sort_by_key(|s| s.id);
    sessions
}

/// A repository folder of a running session, offered for linking in the window.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoChoice {
    pub root: String,
    /// The clone's git common folder ([`LocalSession::folder`]).
    pub folder: String,
    pub name: String,
}

/// The distinct repository folders of `sessions`, sorted by name: two clones (or a fork) of the
/// same repository are two choices, so ticking one never shares the other.
pub fn repo_choices(sessions: &[LocalSession]) -> Vec<RepoChoice> {
    let mut choices: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for session in sessions {
        choices
            .entry((session.repo.as_str(), session.folder.as_str()))
            .or_insert(session.repo_name.as_str());
    }
    let mut list: Vec<RepoChoice> = choices
        .into_iter()
        .map(|((root, folder), name)| RepoChoice {
            root: root.to_owned(),
            folder: folder.to_owned(),
            name: name.to_owned(),
        })
        .collect();
    list.sort_by(|a, b| (&a.name, &a.folder, &a.root).cmp(&(&b.name, &b.folder, &b.root)));
    list
}

/// A path as the team compares it: normalized with the team's case rule (fold, the protocol's
/// default). `None` for a path the protocol refuses; such a file is not shared.
pub fn team_path(path: &str) -> Option<String> {
    RepoPath::normalize(path, CaseRule::Fold)
        .ok()
        .map(|p| p.as_string())
}

/// The keyed hashes of `claim.set`: the repository and per file its first folder and path, at most
/// [`MAX_CLAIM_FILES`]; `true` when files were left out (too many, or not allowed as a path).
pub fn claim_hashes(
    key: &MatchKey,
    repo: &RepoId,
    claims: &[Claim],
) -> (KeyedHash, Vec<PathHashes>, bool) {
    let mut files = Vec::new();
    let mut left_out = false;
    for claim in claims {
        let Ok(path) = RepoPath::normalize(&claim.path, CaseRule::Fold) else {
            left_out = true;
            continue;
        };
        if files.len() == MAX_CLAIM_FILES {
            left_out = true;
            break;
        }
        files.push(key.path(repo, &path));
    }
    (key.repo(repo), files, left_out)
}

// ---------------------------------------------------------------------------------------------
// Presence status
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StatusDoc {
    v: u32,
    name: String,
    sessions: Vec<StatusSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StatusSession {
    /// The session key of its `claim.set`, so a `claim.overlap` finds the session.
    key: String,
    agent: String,
    title: String,
    state: SessionState,
    repo: String,
    files: Vec<String>,
    more_files: u32,
}

/// The presence status of `share`: every session with its changed paths, as many as fit into
/// `limit` bytes; files are dropped from the end (counted in `moreFiles`), and sessions when even
/// that is not enough. `keys` names each session's session key.
pub fn status_plaintext(
    share: &LocalShare,
    keys: &BTreeMap<SessionId, String>,
    limit: usize,
) -> Vec<u8> {
    let mut doc = StatusDoc {
        v: DOC_VERSION,
        name: cut(&share.person),
        sessions: share
            .sessions
            .iter()
            .map(|session| StatusSession {
                key: keys.get(&session.id).cloned().unwrap_or_default(),
                agent: cut(&session.agent),
                title: cut(&session.title),
                state: session.state,
                repo: session.repo.clone(),
                files: session.claims.iter().map(|c| c.path.clone()).collect(),
                more_files: 0,
            })
            .collect(),
    };
    loop {
        let text = serde_json::to_vec(&doc).unwrap_or_default();
        if text.len() <= limit {
            return text;
        }
        // Halve the largest file list first; without files, drop the last session.
        let largest = doc
            .sessions
            .iter_mut()
            .filter(|s| !s.files.is_empty())
            .max_by_key(|s| s.files.len());
        match largest {
            Some(session) => {
                let keep = session.files.len() / 2;
                let dropped = session.files.len() - keep;
                session.files.truncate(keep);
                session.more_files = session
                    .more_files
                    .saturating_add(u32::try_from(dropped).unwrap_or(u32::MAX));
            }
            None => {
                if doc.sessions.pop().is_none() {
                    return serde_json::to_vec(&doc).unwrap_or_default();
                }
            }
        }
    }
}

/// A teammate's session as its presence names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerSession {
    /// The session key of its claims: random, unique per teammate device ([`parse_status`] drops
    /// repeats); the window keys its rows by it and never shows it.
    pub key: String,
    pub agent: String,
    pub title: String,
    pub state: SessionState,
    #[serde(skip)]
    pub repo: String,
    pub files: Vec<String>,
    pub more_files: u32,
}

/// A teammate device's opened presence status.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PeerStatus {
    pub name: String,
    pub sessions: Vec<PeerSession>,
}

/// Reads a teammate's presence status; `None` when it is not a document of this version. Long
/// texts and lists are cut, never trusted to be small.
pub fn parse_status(bytes: &[u8]) -> Option<PeerStatus> {
    let doc: StatusDoc = serde_json::from_slice(bytes).ok()?;
    if doc.v != DOC_VERSION {
        return None;
    }
    let mut keys = std::collections::BTreeSet::new();
    let sessions = doc
        .sessions
        .into_iter()
        .filter(|s| {
            !s.key.is_empty() && s.key.len() <= MAX_SESSION_KEY && keys.insert(s.key.clone())
        })
        .take(MAX_PEER_SESSIONS)
        .map(|s| {
            let total = s.files.len();
            let files: Vec<String> = s
                .files
                .into_iter()
                .filter(|path| path.len() <= MAX_PATH_BYTES)
                .take(MAX_PEER_FILES)
                .collect();
            let unread = u32::try_from(total - files.len()).unwrap_or(u32::MAX);
            PeerSession {
                key: s.key,
                agent: cut(&s.agent),
                title: cut(&s.title),
                state: s.state,
                repo: s.repo,
                files,
                more_files: s.more_files.saturating_add(unread),
            }
        })
        .collect();
    Some(PeerStatus {
        name: cut(&doc.name),
        sessions,
    })
}

// ---------------------------------------------------------------------------------------------
// Claim detail
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DetailDoc {
    v: u32,
    name: String,
    agent: String,
    title: String,
    base: Option<String>,
    files: Vec<DetailFile>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailFile {
    path: String,
    whole: bool,
    ranges: Vec<[u32; 2]>,
}

/// The claim detail of one session: who, the base commit, every changed path with its line
/// ranges, as many files as fit into `limit` bytes (the rest marked as truncated).
pub fn detail_plaintext(person: &str, session: &LocalSession, limit: usize) -> Vec<u8> {
    let mut doc = DetailDoc {
        v: DOC_VERSION,
        name: cut(person),
        agent: cut(&session.agent),
        title: cut(&session.title),
        base: session.base.clone(),
        files: session
            .claims
            .iter()
            .map(|claim| DetailFile {
                path: claim.path.clone(),
                whole: claim.whole_file,
                ranges: claim.ranges.iter().map(|r| [r.start, r.end]).collect(),
            })
            .collect(),
        truncated: session.truncated,
    };
    loop {
        let text = serde_json::to_vec(&doc).unwrap_or_default();
        if text.len() <= limit || doc.files.is_empty() {
            return text;
        }
        let keep = doc.files.len() / 2;
        doc.files.truncate(keep);
        doc.truncated = true;
    }
}

/// A teammate's session as its claim detail describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherDetail {
    pub name: String,
    pub agent: String,
    pub title: String,
    pub base: Option<String>,
    pub claims: Vec<Claim>,
}

/// Reads a teammate's claim detail; `None` when it is not a document of this version.
pub fn parse_detail(bytes: &[u8]) -> Option<OtherDetail> {
    let doc: DetailDoc = serde_json::from_slice(bytes).ok()?;
    if doc.v != DOC_VERSION {
        return None;
    }
    let base = doc
        .base
        .filter(|b| b.len() <= 64 && b.bytes().all(|c| c.is_ascii_hexdigit()));
    let claims = doc
        .files
        .into_iter()
        .filter(|f| f.path.len() <= MAX_PATH_BYTES)
        .take(MAX_PEER_FILES)
        .map(|file| {
            let ranges: Vec<LineRange> = file
                .ranges
                .iter()
                .take(MAX_PEER_RANGES)
                .filter(|[start, end]| start <= end)
                .map(|&[start, end]| LineRange { start, end })
                .collect();
            let whole = file.whole || ranges.is_empty();
            Claim {
                area: claims::area_of(&file.path),
                kind: claims::ChangeKind::Modified,
                whole_file: whole,
                ranges: if whole {
                    Vec::new()
                } else {
                    claims::merge_ranges(ranges)
                },
                path: file.path,
            }
        })
        .collect();
    Some(OtherDetail {
        name: cut(&doc.name),
        agent: cut(&doc.agent),
        title: cut(&doc.title),
        base,
        claims,
    })
}

/// How a teammate's session collides with one of ours, refined with step A's rule: the same
/// file (compared as the team compares paths), overlapping lines when both work on the same base
/// commit with ranges within [`claims::LINE_PADDING`] lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refined {
    pub level: OverlapLevel,
    /// Our spelling of the worst shared file.
    pub path: String,
    /// Where both changed it, for overlapping lines; else where the teammate changed it.
    pub lines: Vec<LineRange>,
    /// Shared files in all.
    pub files: u32,
}

pub fn refine(mine: &LocalSession, theirs: &OtherDetail) -> Option<Refined> {
    let same_base = mine.base.is_some() && mine.base == theirs.base;
    let by_path: BTreeMap<String, &Claim> = theirs
        .claims
        .iter()
        .filter_map(|c| Some((team_path(&c.path)?, c)))
        .collect();
    let mut best: Option<Refined> = None;
    let mut files = 0u32;
    for claim in &mine.claims {
        let Some(other) = team_path(&claim.path).and_then(|p| by_path.get(&p)) else {
            continue;
        };
        files = files.saturating_add(1);
        let (level, shared) = claims::file_level(claim, other, same_base);
        let lines = if level == OverlapLevel::OverlappingLines {
            shared
        } else {
            other.ranges.clone()
        };
        if best.as_ref().is_none_or(|b| level > b.level) {
            best = Some(Refined {
                level,
                path: claim.path.clone(),
                lines,
                files: 0,
            });
        }
    }
    best.map(|b| Refined { files, ..b })
}

/// Our spelling of a file the relay named only by its keyed hash (no detail to read).
pub fn path_for_hash(
    key: &MatchKey,
    repo: &RepoId,
    claims: &[Claim],
    file: &KeyedHash,
) -> Option<String> {
    claims.iter().find_map(|claim| {
        let path = RepoPath::normalize(&claim.path, CaseRule::Fold).ok()?;
        (key.path(repo, &path).file == *file).then(|| claim.path.clone())
    })
}

// ---------------------------------------------------------------------------------------------
// The window's view
// ---------------------------------------------------------------------------------------------

/// A teammate's device as the Team view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerView {
    pub member: String,
    pub device: String,
    /// The display name the device sends; empty when it sent none (the window shows the id).
    pub name: String,
    /// Another device of the signed-in person.
    pub own: bool,
    /// `active`, `idle` or `away`, as the relay reports it.
    pub presence: String,
    pub sessions: Vec<PeerSession>,
}

/// One of our sessions colliding with a teammate's session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeammateOverlap {
    pub session_id: SessionId,
    pub member: String,
    pub device: String,
    /// The teammate's session key: one device may collide with one of ours in several sessions,
    /// so the window keys its rows by it; never shown.
    pub other_session: String,
    pub name: String,
    pub agent: String,
    pub title: String,
    pub level: OverlapLevel,
    /// Our spelling of the worst shared file; `None` for the same area only, or when the relay
    /// named a file this device cannot match.
    pub path: Option<String>,
    pub lines: Vec<LineRange>,
    /// Shared files in all.
    pub files: u32,
}

/// A teammate's device that signed a record naming another membership log at an index this
/// device verified: it reports a different team history. Its presence and claims stay hidden
/// until this device's verified log changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DivergentDevice {
    pub member: String,
    pub device: String,
    /// The display name it sent last; empty when none reached this device.
    pub name: String,
}

/// What the Team view shows of one shared project.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamActivity {
    pub linked: Vec<crate::team::LinkedRepo>,
    pub peers: Vec<PeerView>,
    pub overlaps: Vec<TeammateOverlap>,
    pub divergent: Vec<DivergentDevice>,
    /// The team's reservations (`team_tasks.rs`), oldest first.
    pub tasks: Vec<crate::team_tasks::TaskView>,
    /// This device's reservations that did not go through (yet).
    pub attempts: Vec<crate::team_tasks::AttemptView>,
    /// The newest notices about this person's reservations and requests.
    pub notices: Vec<crate::team_tasks::TaskNotice>,
    /// This device's sessions shared with the project: the ones a reservation can be linked to.
    pub sessions: Vec<SessionId>,
}

/// A file a teammate's session is changing, for the agent gate and the agent's context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeammateFile {
    /// The repository's root commit.
    pub repo: String,
    /// As the teammate spells it.
    pub path: String,
    /// [`team_path`] of it.
    pub key: String,
    /// "Anna (Claude Code)".
    pub who: String,
    /// Where the teammate changed it, when its claim detail reached this device; else empty.
    pub lines: Vec<LineRange>,
}

/// A teammate as the gate and the agent's context name them: the display name the teammate's
/// device sent and its member id, shortened like the window's `shortId` (`Anna #3f9a2c1b…`).
/// Any member can send any name; the member id comes from the verified log, so two people who
/// call themselves the same stay apart. Without a name, the id alone (`#3f9a2c1b…`).
pub fn person_tag(name: &str, member: &str) -> String {
    let id = if member.chars().count() > SHORT_ID + 2 {
        let short: String = member.chars().take(SHORT_ID).collect();
        format!("{short}\u{2026}")
    } else {
        member.to_owned()
    };
    let name = name.trim();
    if name.is_empty() {
        format!("#{id}")
    } else {
        format!("{name} #{id}")
    }
}

/// "Anna (Claude Code)", or the name alone.
pub fn who(name: &str, agent: &str) -> String {
    match (name.is_empty(), agent.is_empty()) {
        (false, false) => format!("{name} ({agent})"),
        (false, true) => name.to_owned(),
        (true, false) => agent.to_owned(),
        (true, true) => String::new(),
    }
}

fn cut(text: &str) -> String {
    text.chars().take(MAX_LABEL_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claims::ChangeKind;

    fn claim(path: &str, ranges: &[(u32, u32)]) -> Claim {
        Claim {
            path: path.into(),
            kind: ChangeKind::Modified,
            whole_file: ranges.is_empty(),
            ranges: ranges
                .iter()
                .map(|&(start, end)| LineRange { start, end })
                .collect(),
            area: claims::area_of(path),
        }
    }

    fn session(id: SessionId, claims: Vec<Claim>) -> LocalSession {
        LocalSession {
            id,
            agent: "Claude Code".into(),
            title: "Fix login".into(),
            state: SessionState::Working,
            repo: "a".repeat(40),
            folder: "c:/src/hangar/.git".into(),
            repo_name: "hangar".into(),
            base: Some("b".repeat(40)),
            claims,
            truncated: false,
        }
    }

    #[test]
    fn the_presence_status_round_trips_and_fits_its_limit() {
        let share = LocalShare {
            person: "Anna".into(),
            sessions: vec![session(
                1,
                (0..400)
                    .map(|n| claim(&format!("src/file{n}.rs"), &[]))
                    .collect(),
            )],
        };
        let keys = BTreeMap::from([(1, "k1".to_owned())]);
        let full = status_plaintext(&share, &keys, 1 << 20);
        let status = parse_status(&full).unwrap();
        assert_eq!(status.name, "Anna");
        assert_eq!(status.sessions[0].key, "k1");
        assert_eq!(status.sessions[0].files.len(), 400);
        assert_eq!(status.sessions[0].state, SessionState::Working);
        // A small limit: files go first, counted.
        let small = status_plaintext(&share, &keys, 2000);
        assert!(small.len() <= 2000);
        let status = parse_status(&small).unwrap();
        let kept = status.sessions[0].files.len();
        assert!(kept > 0 && kept < 400);
        assert_eq!(status.sessions[0].more_files as usize + kept, 400);
        // Not a document of this version, or with unknown fields: nothing.
        assert!(parse_status(br#"{"v":2,"name":"x","sessions":[]}"#).is_none());
        assert!(parse_status(br#"{"v":1,"name":"x","sessions":[],"x":1}"#).is_none());
        assert!(parse_status(b"nonsense").is_none());
    }

    #[test]
    fn a_long_name_is_cut_when_read() {
        let doc = format!(r#"{{"v":1,"name":"{}","sessions":[]}}"#, "n".repeat(500));
        assert_eq!(
            parse_status(doc.as_bytes()).unwrap().name.len(),
            MAX_LABEL_CHARS
        );
    }

    #[test]
    fn the_claim_detail_round_trips_and_is_cut_to_fit() {
        let mine = session(
            1,
            vec![claim("src/a.rs", &[(10, 12)]), claim("docs/b.md", &[])],
        );
        let text = detail_plaintext("Anna", &mine, 1 << 20);
        let detail = parse_detail(&text).unwrap();
        assert_eq!(detail.name, "Anna");
        assert_eq!(detail.base, mine.base);
        assert_eq!(detail.claims, mine.claims);
        let big = session(
            2,
            (0..2000)
                .map(|n| claim(&format!("src/f{n}.rs"), &[(1, 2)]))
                .collect(),
        );
        let text = detail_plaintext("Anna", &big, 10_000);
        assert!(text.len() <= 10_000);
        assert!(parse_detail(&text).unwrap().claims.len() < 2000);
        assert!(parse_detail(br#"{"v":1}"#).is_none());
    }

    #[test]
    fn refining_finds_overlapping_lines_on_the_same_base_only() {
        let mine = session(
            1,
            vec![claim("src/Auth.rs", &[(10, 12)]), claim("x.rs", &[])],
        );
        let theirs = OtherDetail {
            name: "Ben".into(),
            agent: "Codex".into(),
            title: "t".into(),
            base: mine.base.clone(),
            claims: vec![claim("src/auth.rs", &[(14, 15)]), claim("y.rs", &[])],
        };
        let refined = refine(&mine, &theirs).unwrap();
        assert_eq!(refined.level, OverlapLevel::OverlappingLines);
        assert_eq!(refined.path, "src/Auth.rs", "our spelling");
        assert_eq!(refined.lines, [LineRange { start: 10, end: 15 }]);
        assert_eq!(refined.files, 1);
        // Another base: lines say nothing, the file still counts.
        let other_base = OtherDetail {
            base: Some("c".repeat(40)),
            ..theirs.clone()
        };
        let refined = refine(&mine, &other_base).unwrap();
        assert_eq!(refined.level, OverlapLevel::SameFile);
        assert_eq!(refined.lines, [LineRange { start: 14, end: 15 }]);
        // Nothing shared.
        let apart = OtherDetail {
            claims: vec![claim("z.rs", &[])],
            ..theirs
        };
        assert!(refine(&mine, &apart).is_none());
    }

    #[test]
    fn paths_the_protocol_refuses_are_not_shared() {
        assert_eq!(team_path("Src/A.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(team_path("../x"), None);
        assert_eq!(team_path("a\u{200B}b"), None);
    }

    #[test]
    fn repository_choices_are_distinct_and_sorted() {
        let mut a = session(1, Vec::new());
        a.repo_name = "zeta".into();
        let mut b = session(2, Vec::new());
        b.repo = "b".repeat(40);
        b.repo_name = "alpha".into();
        let c = LocalSession { id: 3, ..a.clone() };
        let choices = repo_choices(&[a.clone(), b, c]);
        assert_eq!(
            choices.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        // A second clone (or a fork) with the same root commit is a choice of its own.
        let clone = LocalSession {
            id: 4,
            folder: "c:/src/fork/.git".into(),
            ..a.clone()
        };
        let choices = repo_choices(&[a, clone]);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].root, choices[1].root);
        assert_ne!(choices[0].folder, choices[1].folder);
    }

    #[test]
    fn a_status_with_repeated_session_keys_keeps_the_first() {
        let doc = br#"{"v":1,"name":"x","sessions":[
            {"key":"k","agent":"a","title":"one","state":"idle","repo":"r","files":[],"moreFiles":0},
            {"key":"k","agent":"a","title":"two","state":"idle","repo":"r","files":[],"moreFiles":0},
            {"key":"","agent":"a","title":"three","state":"idle","repo":"r","files":[],"moreFiles":0}]}"#;
        let status = parse_status(doc).unwrap();
        assert_eq!(status.sessions.len(), 1);
        assert_eq!(status.sessions[0].title, "one");
    }

    #[test]
    fn a_teammate_is_tagged_with_a_short_member_id() {
        assert_eq!(person_tag("Anna", "m1"), "Anna #m1");
        assert_eq!(person_tag("  ", "m1"), "#m1");
        assert_eq!(
            person_tag("Anna", "3f9a2c1b-77aa-4e0f"),
            "Anna #3f9a2c1b\u{2026}"
        );
    }

    #[test]
    fn local_sessions_come_from_worktrees_with_a_root_commit() {
        let worktree = |root: Option<&str>, sessions: &[SessionId]| claims::WorktreeClaims {
            repo: "c:/r/.git".into(),
            root_key: "c:/r".into(),
            name: "r".into(),
            root_commit: root.map(str::to_owned),
            base: Some("b".repeat(40)),
            sessions: sessions.to_vec(),
            claims: vec![claim("src/a.rs", &[(1, 2)])],
            truncated: false,
        };
        let labels = [
            SessionLabel {
                id: 1,
                title: "One".into(),
                agent: "Claude Code".into(),
            },
            SessionLabel {
                id: 2,
                title: "Two".into(),
                agent: "Codex".into(),
            },
        ];
        let states = BTreeMap::from([(2, SessionState::Waiting)]);
        let root = "a".repeat(40);
        let local = local_sessions(
            &[worktree(Some(&root), &[2, 1]), worktree(None, &[3])],
            &labels,
            &states,
        );
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].id, 1);
        assert_eq!(local[0].state, SessionState::Idle);
        assert_eq!(local[1].state, SessionState::Waiting);
        assert_eq!(local[1].agent, "Codex");
        assert_eq!(local[1].claims.len(), 1);
    }

    #[test]
    fn who_names_person_and_agent() {
        assert_eq!(who("Anna", "Claude Code"), "Anna (Claude Code)");
        assert_eq!(who("Anna", ""), "Anna");
        assert_eq!(who("", "Codex"), "Codex");
    }
}
