//! Task reservations in a shared project ("I'm working on ...", team service step F part 2b;
//! design: docs/wiki/pages/syntheses/team-service-v1.md, "Step F as built (part 2b)").
//!
//! Pure: the sealed notes' plaintext (the reservation note with the task's title and the area it
//! covers, the release request note with the asking person's name), the window's view of the
//! team's reservations, and the facts the agent gate and the agent's context read. The relay
//! decides who holds a task ("first wins", `ReservationBook` of the protocol crate); the engine
//! (`team_engine.rs`) seals, sends and opens, nothing here touches a key or a socket.
//!
//! What leaves the device, sealed: the task's title, the person's display name, and when the
//! person chose them the repository's root commit, a folder of it (the area the task covers) and
//! the key of the linked session. The relay sees only the keyed hash of the title.

use hangar_team_protocol::hash::RepoId;
use serde::{Deserialize, Serialize};

use crate::session::SessionId;
use crate::team_share;

/// Version of both sealed documents.
const DOC_VERSION: u32 = 1;
/// Longest title and name kept, in characters (the protocol's task id limit is 256 bytes).
pub const MAX_TITLE_CHARS: usize = 120;
/// Longest area kept, in bytes.
const MAX_AREA_BYTES: usize = 1024;
/// Longest session key read from a teammate (the protocol's session key limit).
const MAX_SESSION_KEY: usize = 64;
/// Notices kept per team for the window; older ones go.
pub const MAX_NOTICES: usize = 8;

// ---------------------------------------------------------------------------------------------
// Reservation note
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NoteDoc {
    v: u32,
    title: String,
    name: String,
    repo: Option<String>,
    area: Option<String>,
    session: Option<String>,
}

/// What a reservation says about its task, readable by the team only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskNote {
    /// As the person typed it (the relay sees the keyed hash of its normalized form).
    pub title: String,
    /// The holder's display name; empty without one.
    pub name: String,
    /// The repository's root commit, when the task covers an area of it.
    pub repo: Option<String>,
    /// A folder of the repository the task covers ([`normalize_area`]).
    pub area: Option<String>,
    /// The session key of the holder's linked session (its presence names it).
    pub session: Option<String>,
}

pub fn note_plaintext(note: &TaskNote) -> Vec<u8> {
    let doc = NoteDoc {
        v: DOC_VERSION,
        title: cut(&note.title),
        name: cut(&note.name),
        repo: note.repo.clone(),
        area: note.area.clone(),
        session: note.session.clone(),
    };
    serde_json::to_vec(&doc).unwrap_or_default()
}

/// Reads a reservation note; `None` when it is not a document of this version or has no title.
/// Texts are cut, an area or repository the team would refuse is dropped, never trusted.
pub fn parse_note(bytes: &[u8]) -> Option<TaskNote> {
    let doc: NoteDoc = serde_json::from_slice(bytes).ok()?;
    if doc.v != DOC_VERSION {
        return None;
    }
    let title = cut(doc.title.trim());
    if title.is_empty() {
        return None;
    }
    let repo = doc
        .repo
        .filter(|root| RepoId::new(root).is_ok())
        .map(|root| root.to_ascii_lowercase());
    // An area means nothing without its repository.
    let area = doc
        .area
        .and_then(|area| normalize_area(&area))
        .filter(|_| repo.is_some());
    let session = doc.session.filter(|key| {
        !key.is_empty()
            && key.len() <= MAX_SESSION_KEY
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    });
    Some(TaskNote {
        title,
        name: cut(&doc.name),
        repo,
        area,
        session,
    })
}

// ---------------------------------------------------------------------------------------------
// Release request note
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDoc {
    v: u32,
    name: String,
}

/// The asking person's display name for the holder's card.
pub fn request_plaintext(name: &str) -> Vec<u8> {
    serde_json::to_vec(&RequestDoc {
        v: DOC_VERSION,
        name: cut(name),
    })
    .unwrap_or_default()
}

pub fn parse_request(bytes: &[u8]) -> Option<String> {
    let doc: RequestDoc = serde_json::from_slice(bytes).ok()?;
    (doc.v == DOC_VERSION).then(|| cut(&doc.name))
}

// ---------------------------------------------------------------------------------------------
// Areas
// ---------------------------------------------------------------------------------------------

/// A folder of a repository as a task names it: `\` as `/`, without leading `./` or a trailing
/// `/`, a path the team accepts ([`team_share::team_path`]); `None` when empty or refused.
pub fn normalize_area(raw: &str) -> Option<String> {
    let mut area = raw.trim().replace('\\', "/");
    while let Some(rest) = area.strip_prefix("./") {
        area = rest.to_owned();
    }
    let area = area.trim_end_matches('/').to_owned();
    if area.is_empty() || area.len() > MAX_AREA_BYTES {
        return None;
    }
    team_share::team_path(&area)?;
    Some(area)
}

/// Whether the file (as [`team_share::team_path`] spells it) lies in the area.
pub fn in_area(file_key: &str, area: &str) -> bool {
    let Some(area_key) = team_share::team_path(area) else {
        return false;
    };
    file_key == area_key
        || file_key
            .strip_prefix(area_key.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
}

// ---------------------------------------------------------------------------------------------
// The window's view
// ---------------------------------------------------------------------------------------------

/// Someone who asked the holder to hand a task over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestView {
    pub member: String,
    /// The name the asking person's note gave; empty without one.
    pub name: String,
    /// This person asked.
    pub mine: bool,
    /// One of the requester's listed devices signed the request: only then the holder gets a
    /// card to answer; otherwise it is the relay's word alone.
    pub confirmed: bool,
}

/// A reservation in the team, as the Team view lists it. The release request card of the
/// holder is a view of its own reservations with `request` set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    /// The task's keyed hash (64 hex digits): what the commands name; never shown.
    pub task: String,
    /// Empty while the holder's note is not readable (yet).
    pub title: String,
    pub area: Option<String>,
    pub member: String,
    pub device: String,
    /// The holder's display name from its note; empty without one.
    pub name: String,
    /// Held by this device: only it can release it or answer a request.
    pub mine: bool,
    /// Held by another device of this person.
    pub own: bool,
    /// When the hold began (the relay's clock, Unix milliseconds).
    pub since: u64,
    /// When the hold ends without activity; `null` while it cannot (holder online).
    pub expires_at: Option<u64>,
    /// The holder was warned that the hold runs out.
    pub expiring: bool,
    pub request: Option<RequestView>,
}

/// Where this device's attempt to reserve a task stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AttemptState {
    /// Sent, the relay has not answered yet.
    Reserving,
    /// Someone else holds the task (first wins): the person may ask for it or cancel.
    Blocked,
}

/// A reservation of this device that did not go through (yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    /// The task's keyed hash under the current epoch.
    pub task: String,
    pub title: String,
    pub state: AttemptState,
    /// Who holds it, for `blocked`.
    pub member: Option<String>,
    pub device: Option<String>,
    pub name: String,
    /// This person's release request is open.
    pub asked: bool,
    /// The session it is linked to.
    pub session_id: Option<SessionId>,
}

/// What a notice tells this person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeKind {
    /// The holder handed the task over: it is the person's now.
    ReleaseAccepted,
    ReleaseDeclined,
    /// The holder did not answer in time.
    ReleaseTimedOut,
    /// The person's reservation runs out soon (its device is offline and nothing changed).
    Expiring,
    /// The person's reservation ran out.
    Expired,
    /// An owner or admin ended the person's reservation.
    ForceReleased,
    /// The person holds as many tasks as the team allows.
    Limit,
    /// Another release request for the task is open already, or asked too soon again.
    AskRefused,
    /// The reservation did not go through (the relay refused it, or named a holder the team's
    /// log does not know).
    ReserveFailed,
}

/// Something about the person's reservations or requests, shown once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNotice {
    /// Rises within a run of the app; the window announces each id once.
    pub id: u64,
    pub kind: NoticeKind,
    pub title: String,
    /// The other person (the holder for answers), for the text.
    pub member: String,
    pub name: String,
    /// `expiring`: when the hold ends.
    pub at: Option<u64>,
    /// The linked session on this device, for the desktop notification.
    pub session_id: Option<SessionId>,
}

// ---------------------------------------------------------------------------------------------
// Facts for the agent gate and the agent's context
// ---------------------------------------------------------------------------------------------

/// A reservation as the agent gate and the agent's context read it. Only reservations whose note
/// the holder signed are listed: the title and the area come from the holder, never the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamTask {
    pub title: String,
    /// The holder, tagged with its short member id (`team_share::person_tag`); empty for this
    /// person's own reservations.
    pub who: String,
    /// Held by this person (any of its devices).
    pub mine: bool,
    /// The repository's root commit of the area.
    pub repo: Option<String>,
    pub area: Option<String>,
    /// This device's session the reservation or the attempt is linked to.
    pub session: Option<SessionId>,
    /// This device wanted the task and someone else holds it (`who`).
    pub wanted: bool,
}

fn cut(text: &str) -> String {
    text.chars().take(MAX_TITLE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reservation_note_round_trips_and_refuses_what_the_team_would() {
        let note = TaskNote {
            title: "Fix login".into(),
            name: "Anna".into(),
            repo: Some("a".repeat(40)),
            area: Some("src/auth".into()),
            session: Some("k1".into()),
        };
        assert_eq!(parse_note(&note_plaintext(&note)), Some(note.clone()));
        // Not this version, unknown fields, no title: nothing.
        assert!(parse_note(
            br#"{"v":2,"title":"x","name":"","repo":null,"area":null,"session":null}"#
        )
        .is_none());
        assert!(parse_note(
            br#"{"v":1,"title":"x","name":"","repo":null,"area":null,"session":null,"x":1}"#
        )
        .is_none());
        assert!(parse_note(
            br#"{"v":1,"title":"  ","name":"","repo":null,"area":null,"session":null}"#
        )
        .is_none());
        // An area the team refuses, an area without a repository, a bad session key: dropped.
        let odd = br#"{"v":1,"title":"t","name":"n","repo":"zz","area":"../x","session":"a b"}"#;
        let parsed = parse_note(odd).unwrap();
        assert_eq!(
            (parsed.repo, parsed.area, parsed.session),
            (None, None, None)
        );
        // Long texts are cut when read.
        let long = format!(
            r#"{{"v":1,"title":"{}","name":"{}","repo":null,"area":null,"session":null}}"#,
            "t".repeat(500),
            "n".repeat(500)
        );
        let parsed = parse_note(long.as_bytes()).unwrap();
        assert_eq!(parsed.title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(parsed.name.chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn the_request_note_carries_a_cut_name() {
        assert_eq!(
            parse_request(&request_plaintext("Ben")).as_deref(),
            Some("Ben")
        );
        assert!(parse_request(br#"{"v":1,"name":"x","more":1}"#).is_none());
        let long = format!(r#"{{"v":1,"name":"{}"}}"#, "n".repeat(300));
        assert_eq!(
            parse_request(long.as_bytes()).unwrap().chars().count(),
            MAX_TITLE_CHARS
        );
    }

    #[test]
    fn areas_are_folders_the_team_accepts_and_cover_their_files() {
        assert_eq!(normalize_area("./src\\auth/").as_deref(), Some("src/auth"));
        assert_eq!(normalize_area("  "), None);
        assert_eq!(normalize_area("../secret"), None);
        assert_eq!(normalize_area("/"), None);
        assert!(in_area("src/auth/login.rs", "src/Auth"));
        assert!(in_area("src/auth", "src/auth"));
        assert!(!in_area("src/authority.rs", "src/auth"));
        assert!(!in_area("docs/auth/x.md", "src/auth"));
    }
}
