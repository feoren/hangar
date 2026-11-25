//! Team decisions as shared memory (ADR 0021, protocol version 4): the sealed document and its
//! limits, which decisions count, the Team view's list and what the agents' context reads.
//!
//! Pure: the engine (`team_engine/decisions.rs`) seals, lists, fetches and opens; nothing here
//! touches a key or a socket. A decision is a short statement and its reason, written for the
//! team's agents on purpose; a note never becomes one.
//!
//! What leaves the device, sealed with the team's notes key: the decision, the reason, the
//! person's display name, when, the repository's root commit when it is about one, and the id of
//! a decision it replaces. A withdrawal is a document of its own (`withdrawn`, naming the
//! decision in `supersedes`, without text), stored before the decision is deleted, so a relay
//! that serves the deleted blob again does not bring it back. The relay sees a blob of the kind
//! `decision`, its size and its writer.
//!
//! **Ids are the writer's.** Any member can seal a document with any id, so a decision is named
//! by its writer and its id together ([`Key`]); one member's id never hides or replaces
//! another's.

use std::collections::BTreeSet;

use hangar_team_protocol::hash::RepoId;
use serde::{Deserialize, Serialize};

pub use hangar_team_protocol::limits::{MAX_DECISIONS, MAX_DECISIONS_PER_MEMBER};

/// Version of the sealed document.
const DOC_VERSION: u32 = 1;
/// Longest decision, in characters.
pub const MAX_DECISION_CHARS: usize = 300;
/// Longest reason, in characters.
pub const MAX_REASON_CHARS: usize = 1000;
/// Longest display name kept, in characters (as in reservation notes).
const MAX_NAME_CHARS: usize = crate::team_tasks::MAX_TITLE_CHARS;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    v: u32,
    id: String,
    decision: String,
    reason: String,
    by: String,
    at: u64,
    repo: Option<String>,
    supersedes: Option<String>,
    /// The writer of `supersedes` when it is another member (an owner's or admin's replacement
    /// or withdrawal); absent: the writer's own decision.
    #[serde(
        default,
        rename = "supersedesMember",
        skip_serializing_if = "Option::is_none"
    )]
    supersedes_member: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    withdrawn: bool,
}

/// A decision as its writer sealed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Lower case UUID, made by the writer; bound in the seal.
    pub id: String,
    /// What was decided (at most [`MAX_DECISION_CHARS`], one line); empty for a withdrawal.
    pub decision: String,
    /// Why (at most [`MAX_REASON_CHARS`], may be empty).
    pub reason: String,
    /// The writer's display name; empty without one. The writer itself is the signing device.
    pub name: String,
    /// When it was written (the writer's clock, Unix milliseconds).
    pub at: u64,
    /// The repository's root commit when the decision is about one; `None`: the whole team.
    pub repo: Option<String>,
    /// The decision this one replaces (or withdraws), by its id.
    pub supersedes: Option<String>,
    /// The member who wrote `supersedes`, when it is not this decision's writer: ids are unique
    /// per writer only, so a replacement names whose decision it means.
    pub supersedes_member: Option<String>,
    /// A withdrawal of `supersedes`: shown nowhere, it only hides that decision.
    pub withdrawn: bool,
}

impl Decision {
    /// The withdrawal of the decision `target` (writer and id), written as `name` at `at` by
    /// the member `by`.
    pub fn withdrawal(id: String, name: String, at: u64, by: &str, target: &Key) -> Self {
        Self {
            id,
            decision: String::new(),
            reason: String::new(),
            name,
            at,
            repo: None,
            supersedes: Some(target.1.clone()),
            supersedes_member: (target.0 != by).then(|| target.0.clone()),
            withdrawn: true,
        }
    }

    /// The decision this one hides (writer and id), when `writer` wrote it.
    pub fn hides(&self, writer: &str) -> Option<Key> {
        let id = self.supersedes.clone()?;
        let member = self
            .supersedes_member
            .clone()
            .unwrap_or_else(|| writer.to_owned());
        Some((member, id))
    }
}

/// A decision named by its writer (member id) and its id: ids are unique per writer only.
pub type Key = (String, String);

/// Why a decision cannot be recorded as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftError {
    /// Nothing was decided.
    Empty,
    /// The decision or the reason is longer than its limit.
    TooLong,
    /// Control characters, a line break in the decision, or characters that are not visible.
    Characters,
}

/// Characters that would let a text change how the text around it reads, or hide part of it:
/// control characters (a line break only in a reason), line and paragraph separators and every
/// character that is not visible itself ([`crate::text::is_invisible`]: bidirectional controls,
/// zero width characters, tags, variation selectors and the others).
fn refused(c: char, line_breaks: bool) -> bool {
    if c == '\n' {
        return !line_breaks;
    }
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || crate::text::is_invisible(c)
}

/// The decision and reason as they are sealed: trimmed, CR LF as LF, within the limits.
pub fn check_text(decision: &str, reason: &str) -> Result<(String, String), DraftError> {
    let decision = decision.trim();
    let reason = reason.replace("\r\n", "\n");
    let reason = reason.trim();
    if decision.is_empty() {
        return Err(DraftError::Empty);
    }
    if decision.chars().count() > MAX_DECISION_CHARS || reason.chars().count() > MAX_REASON_CHARS {
        return Err(DraftError::TooLong);
    }
    if decision.chars().any(|c| refused(c, false)) || reason.chars().any(|c| refused(c, true)) {
        return Err(DraftError::Characters);
    }
    Ok((decision.to_owned(), reason.to_owned()))
}

/// A new decision id: a random UUID in lower case.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

/// The document's plaintext.
pub fn plaintext(decision: &Decision) -> Vec<u8> {
    let doc = Doc {
        v: DOC_VERSION,
        id: decision.id.clone(),
        decision: decision.decision.clone(),
        reason: decision.reason.clone(),
        by: decision.name.chars().take(MAX_NAME_CHARS).collect(),
        at: decision.at,
        repo: decision.repo.clone(),
        supersedes: decision.supersedes.clone(),
        supersedes_member: decision.supersedes_member.clone(),
        withdrawn: decision.withdrawn,
    };
    serde_json::to_vec(&doc).unwrap_or_default()
}

fn is_uuid(id: &str) -> bool {
    hangar_team_crypto::notes::is_note_id(id)
}

/// Reads a decision opened as `id` (the id the seal bound). `None` when it is not a document of
/// this version, names another id, or breaks a limit: what a writer cannot send is not trusted
/// from one either. A withdrawal must name the decision it withdraws and carry no text.
pub fn parse(bytes: &[u8], id: &str) -> Option<Decision> {
    if bytes.len() > hangar_team_crypto::decision::MAX_DECISION_PLAINTEXT {
        return None;
    }
    let doc: Doc = serde_json::from_slice(bytes).ok()?;
    if doc.v != DOC_VERSION || doc.id != id || !is_uuid(&doc.id) {
        return None;
    }
    let (decision, reason) = if doc.withdrawn {
        if !doc.decision.is_empty() || !doc.reason.is_empty() || doc.supersedes.is_none() {
            return None;
        }
        (String::new(), String::new())
    } else {
        check_text(&doc.decision, &doc.reason).ok()?
    };
    let repo = match doc.repo {
        Some(root) => Some(RepoId::new(&root).ok().map(|_| root.to_ascii_lowercase())?),
        None => None,
    };
    let supersedes = match doc.supersedes {
        Some(other) if is_uuid(&other) && (other != doc.id || doc.supersedes_member.is_some()) => {
            Some(other)
        }
        Some(_) => return None,
        None => None,
    };
    let supersedes_member = match doc.supersedes_member {
        Some(member)
            if supersedes.is_some()
                && hangar_team_protocol::ids::MemberId::new(member.as_str()).is_ok() =>
        {
            Some(member)
        }
        Some(_) => return None,
        None => None,
    };
    let name: String = doc
        .by
        .chars()
        .filter(|c| !refused(*c, false))
        .take(MAX_NAME_CHARS)
        .collect();
    Some(Decision {
        id: doc.id,
        decision,
        reason,
        name: name.trim().to_owned(),
        at: doc.at,
        repo,
        supersedes,
        supersedes_member,
        withdrawn: doc.withdrawn,
    })
}

/// A decision with what the verified log says about its writer.
#[derive(Debug, Clone, Copy)]
pub struct Listed<'a> {
    pub decision: &'a Decision,
    /// The writer's member id (the signing device's).
    pub member: &'a str,
    /// The writer is an owner or admin in the verified log now.
    pub manager: bool,
}

/// What [`visible`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Visible {
    /// The decisions that count, newest first, as indices into the listing.
    pub shown: Vec<usize>,
    /// Every decision a replacement or a withdrawal hides now, to remember: a relay that later
    /// drops the replacement but serves the old blob again does not bring it back.
    pub hidden: Vec<Key>,
}

/// The decisions that count, newest first (`listed` is oldest first, the relay's order). A
/// replacement or a withdrawal hides the decision it names (writer and id, [`Decision::hides`]),
/// but only when its own writer could withdraw that one: the same member, or an owner or admin.
/// Anyone else's naming is ignored, so a member cannot hide a teammate's decision. Hidden are
/// also those named while the target was not listed (the relay may delete it first), and
/// `remembered` hides what was hidden before. Withdrawals never show, and a decision shows once
/// per writer and id (a copy the relay lists twice shows once).
pub fn visible(listed: &[Listed<'_>], remembered: &BTreeSet<Key>) -> Visible {
    let key = |l: &Listed<'_>| (l.member.to_owned(), l.decision.id.clone());
    let mut hidden: Vec<Key> = Vec::new();
    for other in listed {
        let Some(target) = other.decision.hides(other.member) else {
            continue;
        };
        if (target.0 == other.member || other.manager) && !hidden.contains(&target) {
            hidden.push(target);
        }
    }
    let mut seen = BTreeSet::new();
    let mut shown = Vec::new();
    for index in (0..listed.len()).rev() {
        let entry = &listed[index];
        let named = key(entry);
        if entry.decision.withdrawn
            || !seen.insert(named.clone())
            || hidden.contains(&named)
            || remembered.contains(&named)
        {
            continue;
        }
        shown.push(index);
    }
    Visible { shown, hidden }
}

/// Where the team's decisions stand on this connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DecisionsState {
    /// Not connected, or not known yet.
    #[default]
    Unknown,
    /// The team server speaks a protocol older than version 4: no decisions.
    Unsupported,
    /// The team server keeps decisions, but this device holds no notes key of the team yet: an
    /// owner or admin of the team has to be online with a Hangar that speaks version 4.
    WaitingForKeys,
    /// Listed and readable as far as this device holds the team's notes keys.
    Ready,
}

/// A decision as the Team view lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionView {
    /// The relay's blob id: what Withdraw names, unique in the team.
    pub blob: String,
    pub id: String,
    pub decision: String,
    pub reason: String,
    pub member: String,
    /// The writer's display name; empty without one.
    pub name: String,
    pub at: u64,
    /// Written by this person (any of its devices).
    pub mine: bool,
    /// The writer is no longer in the team's verified log: no agent gets it as context.
    pub former: bool,
    /// This person may withdraw it: its writer, an owner or an admin.
    pub can_withdraw: bool,
}

/// A decision for the agent gate's context; only of writers the verified log lists now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamDecision {
    pub decision: String,
    pub reason: String,
    /// The writer's name (cut) and full member id ([`who`]).
    pub who: String,
    /// When the relay stored it (Unix milliseconds): the writer's own clock is its word only.
    pub at: u64,
    pub repo: Option<String>,
}

/// Longest name in the agents' context, in characters; the member id follows in full.
const MAX_CONTEXT_NAME_CHARS: usize = 40;

/// "Ada #u_ada": the self-chosen name cut short and the full member id the log vouches for, so
/// two writers who call themselves the same stay apart, and a long name cannot push the id out.
pub fn who(name: &str, member: &str) -> String {
    let name: String = name.trim().chars().take(MAX_CONTEXT_NAME_CHARS).collect();
    let name = name.trim();
    if name.is_empty() {
        format!("#{member}")
    } else {
        format!("{name} #{member}")
    }
}

/// `YYYY-MM-DD` (UTC) of Unix milliseconds.
pub fn date(at: u64) -> String {
    let days = i64::try_from(at / 86_400_000).unwrap_or(0);
    let (year, month, day) = crate::civil::civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b";
    const OTHER: &str = "1f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b";

    fn decision(id: &str, supersedes: Option<&str>) -> Decision {
        Decision {
            id: id.to_owned(),
            decision: "Marker A".to_owned(),
            reason: "Marker B".to_owned(),
            name: "Ada".to_owned(),
            at: 1_700_000_000_000,
            repo: None,
            supersedes: supersedes.map(str::to_owned),
            supersedes_member: None,
            withdrawn: false,
        }
    }

    fn listed<'a>(decision: &'a Decision, member: &'a str, manager: bool) -> Listed<'a> {
        Listed {
            decision,
            member,
            manager,
        }
    }

    #[test]
    fn texts_are_trimmed_and_checked_against_their_limits() {
        assert_eq!(
            check_text("  Marker A ", " line one\r\nline two "),
            Ok(("Marker A".to_owned(), "line one\nline two".to_owned()))
        );
        assert_eq!(check_text(" ", "x"), Err(DraftError::Empty));
        assert_eq!(check_text("x", ""), Ok(("x".to_owned(), String::new())));
        let long = "a".repeat(MAX_DECISION_CHARS);
        assert!(check_text(&long, &"b".repeat(MAX_REASON_CHARS)).is_ok());
        assert_eq!(
            check_text(&format!("{long}a"), ""),
            Err(DraftError::TooLong)
        );
        assert_eq!(
            check_text("x", &"b".repeat(MAX_REASON_CHARS + 1)),
            Err(DraftError::TooLong)
        );
        // Characters count, not bytes.
        assert!(check_text(&"\u{00E4}".repeat(MAX_DECISION_CHARS), "").is_ok());
        // Control and invisible characters of every kind: bidi, zero width, the BOM, the Arabic
        // letter mark, variation selectors, tags, separators.
        for bad in [
            "a\nb",
            "a\u{0007}",
            "a\u{202E}b",
            "a\u{2066}",
            "\u{200B}a",
            "a\u{FEFF}",
            "a\u{061C}b",
            "a\u{FE0F}",
            "a\u{E0041}",
            "a\u{2028}b",
            "a\u{00AD}b",
        ] {
            assert_eq!(check_text(bad, ""), Err(DraftError::Characters), "{bad:?}");
        }
        assert_eq!(check_text("x", "a\u{202E}b"), Err(DraftError::Characters));
        assert_eq!(check_text("x", "a\u{E0041}b"), Err(DraftError::Characters));
    }

    #[test]
    fn a_document_reads_back_only_under_its_own_id_and_within_the_limits() {
        let mut written = decision(ID, Some(OTHER));
        written.repo = Some("a".repeat(40));
        let bytes = plaintext(&written);
        assert_eq!(parse(&bytes, ID), Some(written.clone()));
        // A decision that is no withdrawal leaves the field out (the vector's plaintext).
        assert!(!String::from_utf8(bytes.clone())
            .unwrap()
            .contains("withdrawn"));
        // Opened under another id (a blob copied by the relay): refused.
        assert_eq!(parse(&bytes, OTHER), None);
        // Another version, unknown fields, broken limits: refused.
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(
            parse(text.replace("\"v\":1", "\"v\":2").as_bytes(), ID),
            None
        );
        assert_eq!(
            parse(text.replace("\"at\"", "\"x\":1,\"at\"").as_bytes(), ID),
            None
        );
        let long = text.replace("Marker A", &"a".repeat(MAX_DECISION_CHARS + 1));
        assert_eq!(parse(long.as_bytes(), ID), None);
        let hostile = text.replace("Marker A", "Marker\\u202eA");
        assert_eq!(parse(hostile.as_bytes(), ID), None);
        // A decision cannot replace itself; a repository must be a root commit.
        let own = text.replace(OTHER, ID);
        assert_eq!(parse(own.as_bytes(), ID), None);
        let repo = text.replace(&"a".repeat(40), "not-a-commit");
        assert_eq!(parse(repo.as_bytes(), ID), None);
        // A long or hostile name is cut and cleaned, never refused.
        let named = text.replace("\"Ada\"", &format!("\"{}\\u202e\"", "n".repeat(200)));
        let read = parse(named.as_bytes(), ID).unwrap();
        assert_eq!(read.name, "n".repeat(MAX_NAME_CHARS));
        assert!(is_uuid(&new_id()));
        // Larger than any decision: not even parsed.
        let huge = format!(
            "{{\"v\":1,\"id\":\"{ID}\",\"decision\":\"x\",\"reason\":\"{}\",\"by\":\"\",\"at\":1,\"repo\":null,\"supersedes\":null}}",
            " ".repeat(hangar_team_crypto::decision::MAX_DECISION_PLAINTEXT)
        );
        assert_eq!(parse(huge.as_bytes(), ID), None);
    }

    #[test]
    fn a_withdrawal_names_its_decision_and_carries_no_text() {
        let gone = Decision::withdrawal(
            OTHER.to_owned(),
            "Ada".to_owned(),
            5,
            "ada",
            &("ada".to_owned(), ID.to_owned()),
        );
        let bytes = plaintext(&gone);
        assert!(!String::from_utf8(bytes.clone())
            .unwrap()
            .contains("supersedesMember"));
        assert_eq!(parse(&bytes, OTHER), Some(gone));
        let text = String::from_utf8(bytes).unwrap();
        let worded = text.replace("\"decision\":\"\"", "\"decision\":\"Marker\"");
        assert_eq!(parse(worded.as_bytes(), OTHER), None);
        let aimless = text.replace(&format!("\"{ID}\""), "null");
        assert_eq!(parse(aimless.as_bytes(), OTHER), None);
    }

    #[test]
    fn only_the_writer_or_an_owner_or_admin_supersedes_and_copies_show_once() {
        let first = decision(ID, None);
        let second = decision(OTHER, Some(ID));
        let none = BTreeSet::new();
        let two = |member: &'static str, manager: bool| {
            vec![
                listed(&first, "ada", false),
                listed(&second, member, manager),
            ]
        };
        // The writer itself replaces it: only the new one counts, and the old one is remembered.
        let by_writer = visible(&two("ada", false), &none);
        assert_eq!(by_writer.shown, vec![1]);
        assert_eq!(by_writer.hidden, vec![("ada".to_owned(), ID.to_owned())]);
        // Without naming Ada, Bob's replacement means a decision of his own: Ada's stays.
        assert_eq!(visible(&two("bob", true), &none).shown, vec![1, 0]);
        // An admin replaces it, naming its writer.
        let mut named = decision(OTHER, Some(ID));
        named.supersedes_member = Some("ada".to_owned());
        let by = |manager: bool| vec![listed(&first, "ada", false), listed(&named, "bob", manager)];
        assert_eq!(visible(&by(true), &none).shown, vec![1]);
        // Another member cannot hide it by naming it: both count, newest first.
        assert_eq!(visible(&by(false), &none).shown, vec![1, 0]);
        // A copy of the same decision shows once.
        let copy = vec![listed(&first, "ada", false), listed(&first, "ada", false)];
        assert_eq!(visible(&copy, &none).shown, vec![1]);
        // What was hidden once stays hidden, also when the relay drops the replacement.
        let remembered: BTreeSet<Key> = [("ada".to_owned(), ID.to_owned())].into();
        assert!(visible(&[listed(&first, "ada", false)], &remembered)
            .shown
            .is_empty());
    }

    #[test]
    fn one_members_id_never_hides_or_replaces_another_members_decision() {
        let ada = decision(ID, None);
        // Bob seals a document with Ada's id.
        let mut bobs = decision(ID, None);
        bobs.decision = "Marker C".to_owned();
        let both = [listed(&ada, "ada", false), listed(&bobs, "bob", false)];
        assert_eq!(visible(&both, &BTreeSet::new()).shown, vec![1, 0]);
        // A withdrawal by Bob of "that id" withdraws only Bob's own.
        let bob_key = ("bob".to_owned(), ID.to_owned());
        let gone = Decision::withdrawal(OTHER.to_owned(), "Bob".to_owned(), 5, "bob", &bob_key);
        let withdrawn = [
            listed(&ada, "ada", false),
            listed(&bobs, "bob", false),
            listed(&gone, "bob", false),
        ];
        let seen = visible(&withdrawn, &BTreeSet::new());
        assert_eq!(seen.shown, vec![0]);
        assert_eq!(seen.hidden, vec![bob_key.clone()]);
        // The owner withdraws Bob's: it names Bob, and Ada's decision of the same id stays.
        let by_owner =
            Decision::withdrawal(OTHER.to_owned(), "Owner".to_owned(), 5, "owner", &bob_key);
        let owner = [
            listed(&ada, "ada", false),
            listed(&bobs, "bob", false),
            listed(&by_owner, "owner", true),
        ];
        assert_eq!(visible(&owner, &BTreeSet::new()).shown, vec![0]);
        // Bob naming Ada in his own withdrawal does nothing: he could not withdraw hers.
        let ada_key = ("ada".to_owned(), ID.to_owned());
        let forged = Decision::withdrawal(OTHER.to_owned(), "Bob".to_owned(), 5, "bob", &ada_key);
        let tried = [listed(&ada, "ada", false), listed(&forged, "bob", false)];
        assert_eq!(visible(&tried, &BTreeSet::new()).shown, vec![0]);
    }

    #[test]
    fn names_in_the_context_are_cut_and_always_carry_the_full_member_id() {
        assert_eq!(who("Ada", "u_ada"), "Ada #u_ada");
        assert_eq!(who("  ", "u_ada"), "#u_ada");
        let long = who(&"n".repeat(200), "u_0123456789abcdef");
        assert_eq!(long, format!("{} #u_0123456789abcdef", "n".repeat(40)));
    }

    #[test]
    fn dates_are_utc_days() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_700_000_000_000), "2023-11-14");
    }
}
