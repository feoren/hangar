//! Claim overlaps between the sessions of different devices, decided from keyed hashes only.
//!
//! Mapping to the levels of step A (`src-tauri/src/claims.rs`):
//! - `sameFile`: an equal file hash.
//! - `sameArea`: no equal file, but an equal area hash (step A's area is the first folder).
//! - `overlappingLines`: needs line ranges and the base commit, which travel encrypted in
//!   `ClaimSet::detail`. The server reports file level only and passes each side's detail to
//!   the other in `claim.overlap` while they share a file; each client decrypts it and refines
//!   `sameFile` to overlapping lines locally, with the same rule as step A (same base commit,
//!   ranges within 3 lines).
//!
//! Sessions of one device (same member and same device) are never compared here: the app
//! already compares them locally (step A), with exact paths and lines.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::hash::{KeyedHash, PathHashes};
use crate::ids::{Epoch, MemberId, Sealed};
use crate::wire::{ClaimOverlap, Holder, MatchLevel, SessionRef};

/// What one session claims in one repository (the server's copy of its last `claim.set`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionClaims {
    pub repo: KeyedHash,
    pub epoch: Epoch,
    pub files: Vec<PathHashes>,
    /// The encrypted detail, passed on unread.
    pub detail: Option<Sealed>,
}

/// How much two claim sets share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Comparison {
    pub level: MatchLevel,
    /// The first shared file in the first set's order.
    pub file: Option<KeyedHash>,
    /// Shared files in all.
    pub files: u32,
}

impl Comparison {
    const NONE: Comparison = Comparison {
        level: MatchLevel::None,
        file: None,
        files: 0,
    };
}

/// Compares two claim sets; only equal repository and epoch can match.
pub fn compare(a: &SessionClaims, b: &SessionClaims) -> Comparison {
    if a.repo != b.repo || a.epoch != b.epoch {
        return Comparison::NONE;
    }
    let files: HashSet<&KeyedHash> = b.files.iter().map(|f| &f.file).collect();
    let areas: HashSet<&KeyedHash> = b.files.iter().filter_map(|f| f.area.as_ref()).collect();
    let mut result = Comparison::NONE;
    let mut same_area = false;
    for file in &a.files {
        same_area |= file.area.as_ref().is_some_and(|area| areas.contains(area));
        if files.contains(&file.file) {
            result.files = result.files.saturating_add(1);
            result.file.get_or_insert(file.file);
        }
    }
    result.level = if result.files > 0 {
        MatchLevel::SameFile
    } else if same_area {
        MatchLevel::SameArea
    } else {
        MatchLevel::None
    };
    result
}

/// One `claim.overlap` to send: `to` is the receiving session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlapNotice {
    pub to: SessionRef,
    pub overlap: ClaimOverlap,
}

/// All sessions' claims of one team, with the overlaps last reported, so only changes are sent.
///
/// Not persisted across a relay restart, unlike the [`crate::reservation::ReservationBook`]:
/// claims are live state of connected sessions, never logged, and every client sends its
/// sessions' `claim.set` again after each `welcome`. A restart closes every connection, so a
/// restored board would compare sessions whose agents may have ended during the downtime, and
/// the overlaps it reported were never in the log a client could have missed. The relay starts
/// with an empty board and refills it as devices reconnect.
#[derive(Debug, Clone, Default)]
pub struct ClaimBoard {
    sessions: HashMap<SessionRef, SessionClaims>,
    /// Keyed by the ordered pair, so each pair has one entry.
    reported: BTreeMap<(SessionRef, SessionRef), Comparison>,
}

impl ClaimBoard {
    /// An empty board.
    pub fn new() -> Self {
        Self::default()
    }

    /// `claim.set`: replaces the session's claims and reports every overlap that changed. When
    /// only the detail changed, sessions sharing a file with this one get it again.
    pub fn set(&mut self, session: SessionRef, claims: SessionClaims) -> Vec<OverlapNotice> {
        let detail = claims.detail.clone();
        let old = self.sessions.insert(session.clone(), claims);
        let detail_changed = old.is_some_and(|old| old.detail != detail);
        let mut notices = self.refresh(&session);
        if detail_changed {
            let partners: Vec<SessionRef> = self
                .reported
                .iter()
                .filter(|(_, seen)| seen.level == MatchLevel::SameFile)
                .filter_map(|((a, b), _)| {
                    if *a == session {
                        Some(b.clone())
                    } else if *b == session {
                        Some(a.clone())
                    } else {
                        None
                    }
                })
                .filter(|partner| !notices.iter().any(|n| n.to == *partner))
                .collect();
            for partner in partners {
                if let Some(notice) = self.notice(&partner, &session) {
                    notices.push(notice);
                }
            }
        }
        notices
    }

    /// `claim.clear`, or the session ended: its overlaps are reported as ended.
    pub fn clear(&mut self, session: &SessionRef) -> Vec<OverlapNotice> {
        if self.sessions.remove(session).is_none() {
            return Vec::new();
        }
        self.refresh(session)
    }

    /// A device was removed: all its sessions are cleared.
    pub fn clear_device(&mut self, holder: &Holder) -> Vec<OverlapNotice> {
        self.clear_where(|session| session.holder == *holder)
    }

    /// A member left: the sessions of all their devices are cleared.
    pub fn clear_member(&mut self, member: &MemberId) -> Vec<OverlapNotice> {
        self.clear_where(|session| session.holder.member == *member)
    }

    fn clear_where(&mut self, gone: impl Fn(&SessionRef) -> bool) -> Vec<OverlapNotice> {
        let mut sessions: Vec<SessionRef> =
            self.sessions.keys().filter(|s| gone(s)).cloned().collect();
        sessions.sort();
        sessions
            .iter()
            .flat_map(|session| self.clear(session))
            .collect()
    }

    fn refresh(&mut self, session: &SessionRef) -> Vec<OverlapNotice> {
        // The whole holder counts, not the device id alone: device ids are only unique per
        // member, so two members' devices could share one.
        let mut others: Vec<SessionRef> = self
            .sessions
            .keys()
            .filter(|other| other.holder != session.holder)
            .cloned()
            .collect();
        // Pairs reported before whose other side is still known or whose overlap must end.
        for (a, b) in self.reported.keys() {
            let other = if a == session {
                b
            } else if b == session {
                a
            } else {
                continue;
            };
            if !others.contains(other) {
                others.push(other.clone());
            }
        }
        others.sort();
        let mut notices = Vec::new();
        for other in others {
            let key = ordered(session, &other);
            let now = match (self.sessions.get(&key.0), self.sessions.get(&key.1)) {
                (Some(a), Some(b)) => compare(a, b),
                _ => Comparison::NONE,
            };
            let before = self.reported.get(&key).copied().unwrap_or(Comparison::NONE);
            if now == before {
                continue;
            }
            if now.level == MatchLevel::None {
                self.reported.remove(&key);
            } else {
                self.reported.insert(key.clone(), now);
            }
            let repo = self
                .sessions
                .get(&key.0)
                .or_else(|| self.sessions.get(&key.1))
                .map(|claims| claims.repo);
            let Some(repo) = repo else { continue };
            notices.push(self.notice_in(&key.0, &key.1, repo));
            notices.push(self.notice_in(&key.1, &key.0, repo));
        }
        notices
    }

    /// The overlap as `to` sees it, if `to` is still known.
    fn notice(&self, to: &SessionRef, other: &SessionRef) -> Option<OverlapNotice> {
        let repo = self.sessions.get(to)?.repo;
        Some(self.notice_in(to, other, repo))
    }

    /// Each side sees the pair from its own claims; the other side's detail only travels while
    /// they share a file.
    fn notice_in(&self, to: &SessionRef, other: &SessionRef, repo: KeyedHash) -> OverlapNotice {
        let mine = self.sessions.get(to);
        let theirs = self.sessions.get(other);
        let seen = match (mine, theirs) {
            (Some(a), Some(b)) => compare(a, b),
            _ => Comparison::NONE,
        };
        let detail = match (seen.level, theirs) {
            (MatchLevel::SameFile, Some(claims)) => claims.detail.clone(),
            _ => None,
        };
        OverlapNotice {
            to: to.clone(),
            overlap: ClaimOverlap {
                repo,
                session: to.session.clone(),
                other: other.clone(),
                level: seen.level,
                file: seen.file,
                files: seen.files,
                detail,
            },
        }
    }
}

fn ordered(a: &SessionRef, b: &SessionRef) -> (SessionRef, SessionRef) {
    if a <= b {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{CaseRule, MatchKey, RepoId, RepoPath};
    use crate::ids::{Ciphertext, DeviceId, SessionKey};

    const REPO: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

    fn key() -> MatchKey {
        MatchKey::from_bytes(&[9u8; 32]).unwrap()
    }

    fn claims(paths: &[&str]) -> SessionClaims {
        let key = key();
        let repo = RepoId::new(REPO).unwrap();
        SessionClaims {
            repo: key.repo(&repo),
            epoch: Epoch(1),
            files: paths
                .iter()
                .map(|p| key.path(&repo, &RepoPath::normalize(p, CaseRule::Fold).unwrap()))
                .collect(),
            detail: None,
        }
    }

    fn with_detail(paths: &[&str], detail: &str) -> SessionClaims {
        SessionClaims {
            detail: Some(Sealed {
                epoch: Epoch(1),
                data: Ciphertext::new(detail).unwrap(),
            }),
            ..claims(paths)
        }
    }

    fn session(member: &str, device: &str, key: &str) -> SessionRef {
        SessionRef {
            holder: Holder {
                member: MemberId::new(member).unwrap(),
                device: DeviceId::new(device).unwrap(),
            },
            session: SessionKey::new(key).unwrap(),
        }
    }

    #[test]
    fn levels_follow_step_a() {
        let base = claims(&["src/auth/login.rs", "README.md"]);
        let same_file = compare(&base, &claims(&["SRC/Auth/Login.rs"]));
        assert_eq!(same_file.level, MatchLevel::SameFile);
        assert_eq!(same_file.files, 1);
        assert_eq!(same_file.file, Some(base.files[0].file));

        let same_area = compare(&base, &claims(&["src/ui/view.rs"]));
        assert_eq!(same_area.level, MatchLevel::SameArea);
        assert_eq!(same_area.file, None);

        // Root files have no area (step A): a shared root file is still the same file.
        assert_eq!(
            compare(&base, &claims(&["Cargo.toml"])).level,
            MatchLevel::None
        );
        assert_eq!(
            compare(&base, &claims(&["readme.md"])).level,
            MatchLevel::SameFile
        );
        assert_eq!(
            compare(&base, &claims(&["docs/a.md"])).level,
            MatchLevel::None
        );
    }

    #[test]
    fn other_repository_or_epoch_never_matches() {
        let a = claims(&["src/a.rs"]);
        let mut b = claims(&["src/a.rs"]);
        b.epoch = Epoch(2);
        assert_eq!(compare(&a, &b).level, MatchLevel::None);
        let mut c = claims(&["src/a.rs"]);
        c.repo = KeyedHash::from_bytes([0; 32]);
        assert_eq!(compare(&a, &c).level, MatchLevel::None);
    }

    #[test]
    fn board_reports_changes_to_both_sides_only_once() {
        let mut board = ClaimBoard::new();
        let a = session("m1", "d1", "s1");
        let b = session("m2", "d2", "s2");
        assert!(board.set(a.clone(), claims(&["src/a.rs"])).is_empty());
        let notices = board.set(b.clone(), claims(&["src/a.rs", "src/b.rs"]));
        assert_eq!(notices.len(), 2);
        let to_a = notices.iter().find(|n| n.to == a).unwrap();
        let to_b = notices.iter().find(|n| n.to == b).unwrap();
        assert_eq!(to_a.overlap.other, b);
        assert_eq!(to_b.overlap.other, a);
        assert_eq!(to_a.overlap.level, MatchLevel::SameFile);
        assert_eq!(to_a.overlap.session, a.session);
        // Unchanged: nothing new.
        assert!(board
            .set(b.clone(), claims(&["src/b.rs", "src/a.rs"]))
            .is_empty());
        // Down to the same area.
        let notices = board.set(b.clone(), claims(&["src/b.rs"]));
        assert!(notices
            .iter()
            .all(|n| n.overlap.level == MatchLevel::SameArea));
        // Cleared: both hear that it ended.
        let notices = board.clear(&b);
        assert_eq!(notices.len(), 2);
        assert!(notices.iter().all(|n| n.overlap.level == MatchLevel::None));
        assert!(board.clear(&b).is_empty());
    }

    #[test]
    fn sessions_of_one_device_are_left_to_the_app() {
        let mut board = ClaimBoard::new();
        board.set(session("m1", "d1", "s1"), claims(&["src/a.rs"]));
        assert!(board
            .set(session("m1", "d1", "s2"), claims(&["src/a.rs"]))
            .is_empty());
        // Another device of the same member is compared.
        assert_eq!(
            board
                .set(session("m1", "d9", "s3"), claims(&["src/a.rs"]))
                .len(),
            4
        );
    }

    #[test]
    fn two_members_with_the_same_device_id_are_compared() {
        let mut board = ClaimBoard::new();
        board.set(session("m1", "d1", "s1"), claims(&["src/a.rs"]));
        assert_eq!(
            board
                .set(session("m2", "d1", "s2"), claims(&["src/a.rs"]))
                .len(),
            2
        );
    }

    #[test]
    fn the_detail_reaches_sessions_sharing_a_file() {
        let mut board = ClaimBoard::new();
        let a = session("m1", "d1", "s1");
        let b = session("m2", "d2", "s2");
        board.set(a.clone(), with_detail(&["src/a.rs"], "AAAA"));
        let notices = board.set(b.clone(), with_detail(&["src/a.rs"], "BBBB"));
        let to_a = notices.iter().find(|n| n.to == a).unwrap();
        let to_b = notices.iter().find(|n| n.to == b).unwrap();
        assert_eq!(to_a.overlap.detail.as_ref().unwrap().data.as_str(), "BBBB");
        assert_eq!(to_b.overlap.detail.as_ref().unwrap().data.as_str(), "AAAA");
        // Only the detail changed (other lines in the same file): the other side hears it.
        let notices = board.set(b.clone(), with_detail(&["src/a.rs"], "CCCC"));
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].to, a);
        assert_eq!(notices[0].overlap.level, MatchLevel::SameFile);
        assert_eq!(
            notices[0].overlap.detail.as_ref().unwrap().data.as_str(),
            "CCCC"
        );
        // Only the same area: no detail travels.
        let notices = board.set(b.clone(), with_detail(&["src/b.rs"], "DDDD"));
        assert!(notices.iter().all(|n| n.overlap.detail.is_none()));
        assert!(board.set(b, with_detail(&["src/b.rs"], "EEEE")).is_empty());
    }

    #[test]
    fn removing_a_device_ends_its_overlaps() {
        let mut board = ClaimBoard::new();
        let a = session("m1", "d1", "s1");
        let b = session("m2", "d2", "s2");
        board.set(a.clone(), claims(&["src/a.rs"]));
        board.set(b.clone(), claims(&["src/a.rs"]));
        let notices = board.clear_device(&b.holder);
        assert_eq!(notices.len(), 2);
        assert!(notices.iter().any(|n| n.to == a));
    }

    #[test]
    fn a_member_who_leaves_takes_all_their_devices_overlaps() {
        let mut board = ClaimBoard::new();
        let a = session("m1", "d1", "s1");
        let laptop = session("m2", "d2", "s2");
        let desktop = session("m2", "d3", "s3");
        board.set(a.clone(), claims(&["src/a.rs"]));
        board.set(laptop.clone(), claims(&["src/a.rs"]));
        board.set(desktop.clone(), claims(&["src/a.rs"]));
        let notices = board.clear_member(&laptop.holder.member);
        // Each of the three pairs ends once, told to both sides.
        assert_eq!(notices.len(), 6);
        assert!(notices.iter().all(|n| n.overlap.level == MatchLevel::None));
        assert_eq!(notices.iter().filter(|n| n.to == a).count(), 2);
        assert!(board.clear(&laptop).is_empty());
        assert!(board.clear(&desktop).is_empty());
        // Untouched: the remaining member has nothing to compare with.
        assert!(board.set(a, claims(&["src/a.rs"])).is_empty());
    }
}
