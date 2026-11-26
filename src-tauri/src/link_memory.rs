//! Remembered link groups (ADR 0014, "Remembered groups"), the pure part: which sessions formed
//! a group, by the workspace key that survives a restart (`SavedSession::key` in `persist.rs`),
//! with their member names, the lead and "Ask before pasting". Nothing else of a group is kept:
//! no message, no history, no session id (those change with every start).
//!
//! After a restart the remembered groups are only an offer ([`Offer`]): the person confirms each
//! one before anything is linked, so a planted or stale file can never make agents steer each
//! other on its own. `link_service.rs` holds the offers, `main_store.rs` reads and writes the
//! file.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::links::{checked_name, LinkGroup, Role, MAX_GROUPS, MAX_MEMBERS};
use crate::persist::normalize_key;
use crate::session::SessionId;

/// Version of the file; any other is not read.
pub const VERSION: u32 = 1;
/// The file at its largest: every group full, each key and name escaped, with room.
pub const MAX_FILE_BYTES: usize = MAX_GROUPS * MAX_MEMBERS * 256 + 1024;

/// One member of a remembered group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RememberedMember {
    /// The session's workspace key, a UUID in lower case.
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub lead: bool,
}

/// One remembered group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RememberedGroup {
    pub members: Vec<RememberedMember>,
    #[serde(default)]
    pub ask_first: bool,
}

/// The content of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryFile {
    pub version: u32,
    pub groups: Vec<RememberedGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MemoryError {
    #[error("Unknown version of the remembered link groups")]
    Version,
    #[error("Too many remembered link groups")]
    TooMany,
    #[error("A remembered link group needs two to eight members")]
    Size,
    #[error("A remembered member has no valid workspace key")]
    Key,
    #[error("A session is remembered twice")]
    Twice,
    #[error("A remembered member name is not valid")]
    Name,
    #[error("Two remembered members of a group share a name")]
    NameTaken,
    #[error("A remembered link group has more than one lead")]
    TwoLeads,
}

/// Checks every rule a live group follows: at most [`MAX_GROUPS`] groups of 2 to
/// [`MAX_MEMBERS`], keys that are UUIDs in lower case and appear once in the whole file, member
/// names a link group would accept and unique in their group, at most one lead.
pub fn validate(file: &MemoryFile) -> Result<(), MemoryError> {
    if file.version != VERSION {
        return Err(MemoryError::Version);
    }
    if file.groups.len() > MAX_GROUPS {
        return Err(MemoryError::TooMany);
    }
    let mut keys = BTreeSet::new();
    for group in &file.groups {
        if group.members.len() < 2 || group.members.len() > MAX_MEMBERS {
            return Err(MemoryError::Size);
        }
        if group.members.iter().filter(|member| member.lead).count() > 1 {
            return Err(MemoryError::TwoLeads);
        }
        let mut names = BTreeSet::new();
        for member in &group.members {
            if normalize_key(&member.key).ok().as_deref() != Some(member.key.as_str()) {
                return Err(MemoryError::Key);
            }
            if !keys.insert(member.key.as_str()) {
                return Err(MemoryError::Twice);
            }
            if checked_name(&member.name).ok().as_deref() != Some(member.name.as_str()) {
                return Err(MemoryError::Name);
            }
            if !names.insert(member.name.as_str()) {
                return Err(MemoryError::NameTaken);
            }
        }
    }
    Ok(())
}

/// The remembered groups without the members whose session will not come back (its key is not
/// in `keep`, the keys of the saved workspace); a group left with fewer than two goes.
pub fn prune(groups: Vec<RememberedGroup>, keep: &BTreeSet<String>) -> Vec<RememberedGroup> {
    groups
        .into_iter()
        .filter_map(|mut group| {
            group.members.retain(|member| keep.contains(&member.key));
            (group.members.len() >= 2).then_some(group)
        })
        .collect()
}

/// A live group as it is remembered: each member by its workspace key (`key_of`). A member
/// without a known key is left out; `None` when fewer than two remain.
pub fn remember(
    group: &LinkGroup,
    key_of: &dyn Fn(SessionId) -> Option<String>,
) -> Option<RememberedGroup> {
    let members: Vec<RememberedMember> = group
        .members
        .iter()
        .filter_map(|member| {
            let key = normalize_key(&key_of(member.session)?).ok()?;
            Some(RememberedMember {
                key,
                name: member.name.clone(),
                lead: member.role == Role::Lead,
            })
        })
        .collect();
    (members.len() >= 2).then_some(RememberedGroup {
        members,
        ask_first: group.ask_first,
    })
}

/// What goes into the file: the live groups, then the offers not decided yet (each with the id
/// its offer carries). A session is in one group only, so an offer loses every member a live
/// group holds now, and goes when fewer than two remain; `offers` keeps what remains.
pub fn combine(
    live: Vec<RememberedGroup>,
    offers: &mut Vec<(u64, RememberedGroup)>,
) -> Vec<RememberedGroup> {
    let held: BTreeSet<String> = live
        .iter()
        .flat_map(|group| group.members.iter().map(|member| member.key.clone()))
        .collect();
    offers.retain_mut(|(_, group)| {
        group.members.retain(|member| !held.contains(&member.key));
        group.members.len() >= 2
    });
    let mut file = live;
    file.extend(offers.iter().map(|(_, group)| group.clone()));
    file.truncate(MAX_GROUPS);
    file
}

/// One member of an offer, found among the running sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferMember {
    pub session: SessionId,
    pub name: String,
    pub role: Role,
}

/// A remembered group the person may link again ("Link again?").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// Names the offer in `apply_link_offer` and `discard_link_offer`; valid for this run.
    pub id: u64,
    /// The members whose session runs and can be linked now, in the remembered order.
    pub members: Vec<OfferMember>,
    /// The member names of remembered members whose session does not run (yet) or cannot be
    /// linked; they are left out when the offer is applied.
    pub missing: Vec<String>,
    pub ask_first: bool,
}

/// The offer for remembered group `group` with id `id`: its members whose key a running,
/// linkable session in no group carries now (`session_of`). `None` while fewer than two do.
pub fn offer(
    id: u64,
    group: &RememberedGroup,
    session_of: &dyn Fn(&str) -> Option<SessionId>,
) -> Option<Offer> {
    let members: Vec<OfferMember> = group
        .members
        .iter()
        .filter_map(|member| {
            Some(OfferMember {
                session: session_of(&member.key)?,
                name: member.name.clone(),
                role: if member.lead {
                    Role::Lead
                } else {
                    Role::Member
                },
            })
        })
        .collect();
    (members.len() >= 2).then(|| Offer {
        id,
        missing: group
            .members
            .iter()
            .filter(|member| !members.iter().any(|found| found.name == member.name))
            .map(|member| member.name.clone())
            .collect(),
        members,
        ask_first: group.ask_first,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::{Links, NewMember};

    fn key(n: u8) -> String {
        format!("00000000-0000-4000-8000-0000000000{n:02x}")
    }

    fn member(n: u8, name: &str, lead: bool) -> RememberedMember {
        RememberedMember {
            key: key(n),
            name: name.into(),
            lead,
        }
    }

    fn group(members: Vec<RememberedMember>) -> RememberedGroup {
        RememberedGroup {
            members,
            ask_first: false,
        }
    }

    fn file(groups: Vec<RememberedGroup>) -> MemoryFile {
        MemoryFile {
            version: VERSION,
            groups,
        }
    }

    #[test]
    fn the_file_follows_the_rules_of_a_live_group() {
        let good = group(vec![member(1, "app", true), member(2, "web", false)]);
        assert_eq!(validate(&file(vec![good.clone()])), Ok(()));
        let cases = [
            (
                MemoryFile {
                    version: 2,
                    groups: vec![],
                },
                MemoryError::Version,
            ),
            (
                file(vec![group(vec![member(1, "app", false)])]),
                MemoryError::Size,
            ),
            (
                file(vec![group(vec![
                    member(1, "app", true),
                    member(2, "web", true),
                ])]),
                MemoryError::TwoLeads,
            ),
            (
                file(vec![group(vec![
                    RememberedMember {
                        key: "../x".into(),
                        ..member(1, "app", false)
                    },
                    member(2, "web", false),
                ])]),
                MemoryError::Key,
            ),
            (
                file(vec![group(vec![
                    RememberedMember {
                        key: key(0xab).to_uppercase(),
                        ..member(0xab, "app", false)
                    },
                    member(2, "web", false),
                ])]),
                MemoryError::Key,
            ),
            (file(vec![good.clone(), good.clone()]), MemoryError::Twice),
            (
                file(vec![group(vec![
                    member(1, "all", false),
                    member(2, "web", false),
                ])]),
                MemoryError::Name,
            ),
            (
                file(vec![group(vec![
                    member(1, "No Name", false),
                    member(2, "w", false),
                ])]),
                MemoryError::Name,
            ),
            (
                file(vec![group(vec![
                    member(1, "web", false),
                    member(2, "web", false),
                ])]),
                MemoryError::NameTaken,
            ),
            (
                file(
                    (0..=MAX_GROUPS as u8)
                        .map(|n| {
                            group(vec![
                                member(n * 2, "a", false),
                                member(n * 2 + 1, "b", false),
                            ])
                        })
                        .collect(),
                ),
                MemoryError::TooMany,
            ),
        ];
        for (file, error) in cases {
            assert_eq!(validate(&file), Err(error));
        }
        // The file reads and writes in the shape the store keeps.
        let json = serde_json::to_string(&file(vec![good])).unwrap();
        assert!(json.contains("\"askFirst\":false"), "{json}");
        let back: MemoryFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.groups[0].members[0].name, "app");
        assert!(serde_json::from_str::<MemoryFile>(r#"{"version":1,"groups":[],"x":1}"#).is_err());
    }

    #[test]
    fn members_that_do_not_come_back_are_left_out() {
        let groups = vec![
            group(vec![
                member(1, "a", true),
                member(2, "b", false),
                member(3, "c", false),
            ]),
            group(vec![member(4, "d", false), member(5, "e", false)]),
        ];
        let keep: BTreeSet<String> = [key(1), key(3), key(4)].into_iter().collect();
        let pruned = prune(groups, &keep);
        assert_eq!(pruned.len(), 1);
        assert_eq!(
            pruned[0]
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
    }

    #[test]
    fn a_live_group_is_remembered_by_its_keys_without_messages() {
        let mut links = Links::default();
        let id = links
            .link_members(&[
                NewMember {
                    session: 7,
                    title: "App",
                    name: None,
                    lead: true,
                },
                NewMember {
                    session: 8,
                    title: "Web",
                    name: Some("site"),
                    lead: false,
                },
                NewMember {
                    session: 9,
                    title: "Docs",
                    name: None,
                    lead: false,
                },
            ])
            .unwrap()
            .id;
        links.set_ask_first(id, true).unwrap();
        let live = links.group(id).unwrap();
        let key_of = |session: SessionId| match session {
            7 => Some(key(1).to_uppercase()),
            8 => Some(key(2)),
            _ => None,
        };
        let remembered = remember(live, &key_of).unwrap();
        assert_eq!(
            remembered,
            RememberedGroup {
                members: vec![member(1, "app", true), member(2, "site", false)],
                ask_first: true,
            }
        );
        // One known key is not enough for a group.
        assert!(remember(live, &|session| (session == 7).then(|| key(1))).is_none());
    }

    #[test]
    fn offers_lose_the_sessions_a_live_group_took() {
        let live = vec![group(vec![member(1, "a", false), member(2, "b", false)])];
        let mut offers = vec![
            (1, group(vec![member(2, "b", false), member(3, "c", false)])),
            (
                2,
                group(vec![
                    member(4, "d", false),
                    member(5, "e", false),
                    member(1, "a", false),
                ]),
            ),
        ];
        let file = combine(live, &mut offers);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].0, 2);
        assert_eq!(offers[0].1.members.len(), 2);
        assert_eq!(file.len(), 2);
        assert_eq!(
            validate(&MemoryFile {
                version: VERSION,
                groups: file
            }),
            Ok(())
        );
    }

    #[test]
    fn an_offer_needs_two_running_members() {
        let remembered = RememberedGroup {
            members: vec![
                member(1, "a", true),
                member(2, "b", false),
                member(3, "c", false),
            ],
            ask_first: true,
        };
        let running = |key_text: &str| {
            if key_text == key(1) {
                Some(11)
            } else if key_text == key(3) {
                Some(13)
            } else {
                None
            }
        };
        let offered = offer(5, &remembered, &running).unwrap();
        assert_eq!(offered.id, 5);
        assert_eq!(offered.missing, ["b"]);
        assert!(offered.ask_first);
        assert_eq!(
            offered.members,
            vec![
                OfferMember {
                    session: 11,
                    name: "a".into(),
                    role: Role::Lead
                },
                OfferMember {
                    session: 13,
                    name: "c".into(),
                    role: Role::Member
                },
            ]
        );
        assert!(offer(5, &remembered, &|k| (k == key(1)).then_some(11)).is_none());
        let json = serde_json::to_value(&offered).unwrap();
        assert_eq!(json["askFirst"], true);
        assert_eq!(json["members"][0]["role"], "lead");
    }
}
