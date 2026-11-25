//! Proof of possession at `hello` (protocol version 3): a connection shows that it holds the
//! signing key the membership log lists for the device it named in `hello`, and the relay's rule
//! for devices that have not shown it.
//!
//! The device id in `hello` is the client's word (the app makes it), and nothing outside the
//! membership log ties it to a key. So
//! before version 3 the relay could not tell whether a connection holds that device's key: a
//! blocked device came back under a new id, and a stolen, unused ticket together with a listed
//! device id acted as that device. Version 3 closes both for connections that negotiate it.
//!
//! # Statement
//!
//! The relay sends a random [`Challenge`] in every `welcome` of version 3 (one team on one
//! connection). The device answers with `device.prove { sign, signature }`:
//!
//! ```text
//! statement = field("hangar-team/v1/connection-proof") || field(team) || field(member)
//!             || field(device) || field(sign public key) || field(challenge, 32 bytes)
//!             || field(protocol version, 2 bytes big endian)
//! signature = Ed25519 with the device's sign key over statement
//! ```
//!
//! `field(x)` is the length-prefixed framing of every signature in this crate. The label is used
//! by no other signature, so neither a proof of possession of `device.keys`, an endorsement, a
//! log entry nor a sealed record verifies as a connection proof, and the other way round. The
//! team, the holder and the version bind the proof to the one `hello` it answers: a proof for
//! another team, another member, another device id or another negotiated version does not
//! verify, and the challenge, new with every `welcome`, makes a proof seen once worthless on any
//! other connection (replay). `sign` is part of the statement, so a signature cannot be presented
//! under another key.
//!
//! # Who counts as a device of the log
//!
//! [`check_restricted`] is the relay's rule before every intent of a team with a log (an empty
//! log restricts nothing: the team's first entry is still to be written):
//!
//! | Connection | Device listed in the log | Counts as that device |
//! | --- | --- | --- |
//! | version 3 or newer, proven with the log's sign key | yes | yes |
//! | version 3 or newer, unproven or proven with another key | yes | no |
//! | version 1 or 2 ([`Possession::Legacy`], cannot prove) | yes | yes (transition rule) |
//! | any | no | no |
//!
//! A device that does not count may send only what
//! [`ClientMessage::allowed_before_proof`] lists (register its keys, prove itself, read the
//! directory and the log, keep the connection); everything else is `forbidden`. The relay closes
//! the legacy row for a holder once it has proven possession once (it refuses that holder's
//! `hello` of an older version from then on), and for everyone when the operator requires
//! version 3; both are the relay's state, documented in the relay specification
//! (`docs/wiki/pages/syntheses/relay-v3-server-spec.md`).

use hangar_team_protocol::e2e::{Challenge, KeyProof, PublicKey};
use hangar_team_protocol::ids::TeamId;
use hangar_team_protocol::wire::{ClientMessage, DeviceProve, ErrorCode, Holder};
use hangar_team_protocol::PROOF_PROTOCOL_VERSION;
use thiserror::Error;

use crate::encoding::framed;
use crate::identity::{is_valid_sign_key, verify, DeviceIdentity};
use crate::memberlog::TeamState;

const LABEL_CONNECTION_PROOF: &[u8] = b"hangar-team/v1/connection-proof";

/// What a connection proof signs (module docs).
fn statement(
    team: &TeamId,
    holder: &Holder,
    sign: &PublicKey,
    challenge: &Challenge,
    proto: u16,
) -> Vec<u8> {
    framed(&[
        LABEL_CONNECTION_PROOF,
        team.as_str().as_bytes(),
        holder.member.as_str().as_bytes(),
        holder.device.as_str().as_bytes(),
        sign.as_bytes(),
        challenge.as_bytes(),
        &proto.to_be_bytes(),
    ])
}

impl DeviceIdentity {
    /// The `device.prove` answering `challenge`, the `welcome`'s challenge for this device as
    /// `holder` (the welcome's member and this device's id) in `team` on a connection that
    /// negotiated `proto`.
    pub fn prove_connection(
        &self,
        team: &TeamId,
        holder: &Holder,
        proto: u16,
        challenge: &Challenge,
    ) -> DeviceProve {
        let sign = self.public_keys().sign;
        DeviceProve {
            sign,
            signature: KeyProof::from_bytes(
                self.sign(&statement(team, holder, &sign, challenge, proto)),
            ),
        }
    }
}

/// Why a `device.prove` is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProveError {
    /// `sign` is not an Ed25519 key a device may hold (not a valid point, or of small order).
    #[error("the proof names a signing key no device may hold")]
    InvalidKey,
    /// The signature does not verify for this team, holder, key, challenge and version.
    #[error("the connection proof does not verify")]
    BadSignature,
}

impl ProveError {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        ErrorCode::Forbidden
    }
}

/// The relay's check of a `device.prove`: `team` is the frame's team, `holder` the connection's
/// (ticket subject plus `hello.device`), `proto` the version the team negotiated on this
/// connection and `challenge` the one its newest `welcome` carried. Returns the proven signing
/// key, which the relay keeps for this team on this connection ([`Possession::Proven`]); it
/// replaces a key proven before only when this check passes.
pub fn verify_connection_proof(
    team: &TeamId,
    holder: &Holder,
    proto: u16,
    challenge: &Challenge,
    prove: &DeviceProve,
) -> Result<PublicKey, ProveError> {
    if !is_valid_sign_key(&prove.sign) {
        return Err(ProveError::InvalidKey);
    }
    let message = statement(team, holder, &prove.sign, challenge, proto);
    if !verify(&prove.sign, &message, prove.signature.as_bytes()) {
        return Err(ProveError::BadSignature);
    }
    Ok(prove.sign)
}

/// What a connection has shown for one team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Possession<'a> {
    /// Protocol version 1 or 2: the connection cannot prove anything. A listed device counts as
    /// itself, as before version 3 (the transition rule; the relay narrows it, see module docs).
    Legacy,
    /// Version 3 or newer, no `device.prove` has passed yet.
    Unproven,
    /// Version 3 or newer, a `device.prove` passed with this signing key.
    Proven(&'a PublicKey),
}

impl<'a> Possession<'a> {
    /// The possession of a connection that negotiated `proto`, with the key its last passing
    /// `device.prove` proved.
    pub fn of_connection(proto: u16, proven: Option<&'a PublicKey>) -> Self {
        match (proto >= PROOF_PROTOCOL_VERSION, proven) {
            (false, _) => Possession::Legacy,
            (true, None) => Possession::Unproven,
            (true, Some(key)) => Possession::Proven(key),
        }
    }
}

/// Whether the connection counts as `holder`'s device of the verified log: the log lists the
/// device, and on a connection of version 3 or newer the key it proved is the signing key the log
/// lists for it now (a device admitted again with new keys proves the new ones).
pub fn counts_as_listed(state: &TeamState, holder: &Holder, possession: Possession<'_>) -> bool {
    let Some(entry) = state.device(holder) else {
        return false;
    };
    match possession {
        Possession::Legacy => true,
        Possession::Unproven => false,
        Possession::Proven(key) => entry.keys.sign == *key,
    }
}

/// An intent from a device that does not count as a device of the log; answered `forbidden`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("this device has not shown that it is a device of the team's membership log")]
pub struct Restricted;

impl Restricted {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        ErrorCode::Forbidden
    }
}

/// The relay's check before it applies `message` from `holder` on a connection with
/// `possession`, before every other check of the intent. `state` is the team's verified log,
/// `None` while it has no entry: then nothing is restricted, since the team's first entry comes
/// from a device no log can list yet. Otherwise a device that does not count as listed
/// ([`counts_as_listed`]) may send only [`ClientMessage::allowed_before_proof`].
pub fn check_restricted(
    state: Option<&TeamState>,
    holder: &Holder,
    possession: Possession<'_>,
    message: &ClientMessage,
) -> Result<(), Restricted> {
    let Some(state) = state else {
        return Ok(());
    };
    if message.allowed_before_proof() || counts_as_listed(state, holder, possession) {
        Ok(())
    } else {
        Err(Restricted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use hangar_team_protocol::e2e::entry_hash;
    use hangar_team_protocol::ids::{BlobId, DeviceId, MemberId, UnixMillis};
    use hangar_team_protocol::wire::{
        BlobDelete, DeviceKeys, MemberLogEntry, MemberLogGet, PresenceSet, PresenceState,
    };

    use hangar_team_protocol::wire::Role;

    use crate::entropy::OsEntropy;
    use crate::memberlog::{DeviceEntry, EntryBody, EntryKind, LogVerifier};

    fn team() -> TeamId {
        TeamId::new("team-1").unwrap()
    }

    fn holder(member: &str, device: &str) -> Holder {
        Holder {
            member: MemberId::new(member).unwrap(),
            device: DeviceId::new(device).unwrap(),
        }
    }

    fn challenge(byte: u8) -> Challenge {
        Challenge::from_bytes([byte; 32])
    }

    /// A team whose log lists only `owner` with `identity`.
    fn state_with(owner: &Holder, identity: &DeviceIdentity) -> LogVerifier {
        let body = EntryBody::create_team(
            team(),
            owner.clone(),
            UnixMillis(1),
            vec![DeviceEntry::new(&team(), owner, identity, false)],
        );
        let append = body.sign(identity).unwrap();
        let hash = entry_hash(append.index, append.prev.as_ref(), &append.entry).unwrap();
        let mut verifier = LogVerifier::new(team());
        verifier
            .apply(&MemberLogEntry {
                index: append.index,
                hash,
                entry: append.entry,
                by: owner.clone(),
                at: UnixMillis(1),
            })
            .unwrap();
        verifier
    }

    fn identity() -> DeviceIdentity {
        DeviceIdentity::generate(&mut OsEntropy).unwrap()
    }

    /// Appends `kind`, issued and signed by `by` with `signer`, as a client applies it.
    fn append(verifier: &mut LogVerifier, signer: &DeviceIdentity, by: &Holder, kind: EntryKind) {
        let state = verifier.state().unwrap();
        let head = *state.head();
        let body = state.next_body(by.clone(), UnixMillis(2), kind);
        let append = body.sign(signer).unwrap();
        let hash = entry_hash(append.index, Some(&head.hash), &append.entry).unwrap();
        verifier
            .apply(&MemberLogEntry {
                index: append.index,
                hash,
                entry: append.entry,
                by: by.clone(),
                at: UnixMillis(2),
            })
            .unwrap();
    }

    /// The owner `m1/d1` with an admin `m2/d1` and a member `m3/d1`.
    fn team_of_three() -> (LogVerifier, DeviceIdentity) {
        let owner = holder("m1", "d1");
        let owner_key = identity();
        let mut verifier = state_with(&owner, &owner_key);
        for (member, role) in [("m2", Role::Admin), ("m3", Role::Member)] {
            let device = holder(member, "d1");
            append(
                &mut verifier,
                &owner_key,
                &owner,
                EntryKind::AddMember {
                    member: device.member.clone(),
                    role,
                    devices: vec![DeviceEntry::new(&team(), &device, &identity(), false)],
                },
            );
        }
        (verifier, owner_key)
    }

    #[test]
    fn a_proof_verifies_for_its_own_hello_and_yields_its_key() {
        let device = identity();
        let me = holder("m1", "d1");
        let prove = device.prove_connection(&team(), &me, 3, &challenge(1));
        assert_eq!(prove.sign, device.public_keys().sign);
        assert_eq!(
            verify_connection_proof(&team(), &me, 3, &challenge(1), &prove),
            Ok(device.public_keys().sign)
        );
    }

    /// The challenge is new with every `welcome`: a proof seen on one connection is worthless on
    /// the next.
    #[test]
    fn a_proof_replayed_against_another_challenge_fails() {
        let device = identity();
        let me = holder("m1", "d1");
        let prove = device.prove_connection(&team(), &me, 3, &challenge(1));
        assert_eq!(
            verify_connection_proof(&team(), &me, 3, &challenge(2), &prove),
            Err(ProveError::BadSignature)
        );
    }

    /// Team, member, device and version are all bound: a proof answers exactly one `hello`.
    #[test]
    fn a_proof_for_another_team_holder_or_version_fails() {
        let device = identity();
        let me = holder("m1", "d1");
        let prove = device.prove_connection(&team(), &me, 3, &challenge(1));
        let other_team = TeamId::new("team-2").unwrap();
        let cases = [
            (other_team, me.clone(), 3),
            (team(), holder("m2", "d1"), 3),
            (team(), holder("m1", "d2"), 3),
            (team(), me.clone(), 4),
        ];
        for (team_id, who, proto) in cases {
            assert_eq!(
                verify_connection_proof(&team_id, &who, proto, &challenge(1), &prove),
                Err(ProveError::BadSignature),
                "{team_id:?} {who:?} {proto}"
            );
        }
        // Field boundaries are framed: moving a character from the member into the device
        // changes the statement.
        let split = holder("m", "1d1");
        let joined = holder("m1", "d1");
        assert_ne!(
            statement(&team(), &split, &prove.sign, &challenge(1), 3),
            statement(&team(), &joined, &prove.sign, &challenge(1), 3)
        );
    }

    /// Another device's key under one's own signature, or one's own signature under the listed
    /// device's key, never verifies: `sign` is part of the statement.
    #[test]
    fn a_signature_cannot_be_presented_under_another_key() {
        let attacker = identity();
        let victim = identity();
        let me = holder("m1", "d1");
        let mut prove = attacker.prove_connection(&team(), &me, 3, &challenge(1));
        prove.sign = victim.public_keys().sign;
        assert_eq!(
            verify_connection_proof(&team(), &me, 3, &challenge(1), &prove),
            Err(ProveError::BadSignature)
        );
        prove.sign = PublicKey::from_bytes([0; 32]);
        assert_eq!(
            verify_connection_proof(&team(), &me, 3, &challenge(1), &prove),
            Err(ProveError::InvalidKey)
        );
        assert_eq!(ProveError::InvalidKey.code(), ErrorCode::Forbidden);
    }

    /// The label separates this signature from every other one the same key makes: the proof
    /// of possession of `device.keys` signs the same team and holder, yet never passes here.
    #[test]
    fn other_signatures_of_the_device_never_pass_as_a_connection_proof() {
        let device = identity();
        let me = holder("m1", "d1");
        let keys: DeviceKeys = device.device_keys(&team(), &me);
        let forged = DeviceProve {
            sign: keys.sign,
            signature: keys.proof,
        };
        assert_eq!(
            verify_connection_proof(&team(), &me, 3, &challenge(1), &forged),
            Err(ProveError::BadSignature)
        );
        let endorsement = DeviceProve {
            sign: keys.sign,
            signature: device.endorse(&team(), &me, &device.public_keys(), false),
        };
        assert_eq!(
            verify_connection_proof(&team(), &me, 3, &challenge(1), &endorsement),
            Err(ProveError::BadSignature)
        );
    }

    /// The statement byte for byte, built by hand: a change of the format breaks this test, not
    /// silently every relay.
    #[test]
    fn the_statement_has_its_documented_layout() {
        fn field(out: &mut Vec<u8>, part: &[u8]) {
            out.extend_from_slice(&u32::try_from(part.len()).unwrap().to_be_bytes());
            out.extend_from_slice(part);
        }
        let sign = PublicKey::from_bytes([5; 32]);
        let mut expected = Vec::new();
        field(&mut expected, b"hangar-team/v1/connection-proof");
        field(&mut expected, b"team-1");
        field(&mut expected, b"m1");
        field(&mut expected, b"d1");
        field(&mut expected, &[5; 32]);
        field(&mut expected, &[1; 32]);
        field(&mut expected, &[0, 3]);
        assert_eq!(
            statement(&team(), &holder("m1", "d1"), &sign, &challenge(1), 3),
            expected
        );
    }

    #[test]
    fn possession_follows_the_negotiated_version() {
        let key = PublicKey::from_bytes([1; 32]);
        assert_eq!(Possession::of_connection(2, None), Possession::Legacy);
        assert_eq!(Possession::of_connection(2, Some(&key)), Possession::Legacy);
        assert_eq!(Possession::of_connection(3, None), Possession::Unproven);
        assert_eq!(
            Possession::of_connection(3, Some(&key)),
            Possession::Proven(&key)
        );
    }

    fn presence() -> ClientMessage {
        ClientMessage::PresenceSet(PresenceSet {
            state: PresenceState::Active,
            status: None,
        })
    }

    /// The table of the module docs.
    #[test]
    fn only_a_listed_device_with_its_proven_key_counts_on_version_three() {
        let owner = holder("m1", "d1");
        let device = identity();
        let verifier = state_with(&owner, &device);
        let state = verifier.state();
        let listed = device.public_keys().sign;
        let other = identity().public_keys().sign;
        let delete = ClientMessage::BlobDelete(BlobDelete {
            blob: BlobId::new("b1").unwrap(),
        });
        for message in [presence(), delete] {
            assert_eq!(
                check_restricted(state, &owner, Possession::Proven(&listed), &message),
                Ok(())
            );
            assert_eq!(
                check_restricted(state, &owner, Possession::Legacy, &message),
                Ok(())
            );
            assert_eq!(
                check_restricted(state, &owner, Possession::Unproven, &message),
                Err(Restricted)
            );
            assert_eq!(
                check_restricted(state, &owner, Possession::Proven(&other), &message),
                Err(Restricted)
            );
            // A device id the log does not list, however it connects: a blocked device that
            // came back under a new id.
            let stranger = holder("m1", "d9");
            for possession in [
                Possession::Legacy,
                Possession::Unproven,
                Possession::Proven(&listed),
            ] {
                assert_eq!(
                    check_restricted(state, &stranger, possession, &message),
                    Err(Restricted)
                );
            }
        }
        assert_eq!(Restricted.code(), ErrorCode::Forbidden);
    }

    /// What a device waiting for its admission sends passes, unproven and unlisted.
    #[test]
    fn registration_and_log_reads_pass_before_the_proof() {
        let owner = holder("m1", "d1");
        let device = identity();
        let verifier = state_with(&owner, &device);
        let stranger = holder("m2", "d1");
        let newcomer = identity();
        let allowed = [
            ClientMessage::DeviceKeys(newcomer.device_keys(&team(), &stranger)),
            ClientMessage::DeviceProve(newcomer.prove_connection(
                &team(),
                &stranger,
                3,
                &challenge(1),
            )),
            ClientMessage::MemberLogGet(MemberLogGet { after: 0 }),
            ClientMessage::DevicesGet,
            ClientMessage::Ping,
        ];
        for message in &allowed {
            assert_eq!(
                check_restricted(verifier.state(), &stranger, Possession::Unproven, message),
                Ok(()),
                "{}",
                message.kind()
            );
        }
    }

    /// `blob.delete`: the author from any device, an owner or admin by the lower of log and
    /// ticket role, nobody else.
    #[test]
    fn a_blob_is_deleted_by_its_author_or_an_owner_or_admin() {
        use crate::relay::{check_blob_delete, BlobDeleteForbidden};
        let owner = holder("m1", "d1");
        let device = identity();
        let verifier = state_with(&owner, &device);
        let state = verifier.state();
        let author = MemberId::new("m2").unwrap();
        // Without a log, the author from any device.
        assert_eq!(
            check_blob_delete(None, Role::Member, &holder("m2", "d7"), &author),
            Ok(())
        );
        // The owner of the log with an owner's ticket.
        assert_eq!(
            check_blob_delete(state, Role::Owner, &owner, &author),
            Ok(())
        );
        // The same owner after the website downgraded the ticket.
        assert_eq!(
            check_blob_delete(state, Role::Member, &owner, &author),
            Err(BlobDeleteForbidden)
        );
        // A member the log does not name gains nothing from an admin's ticket.
        assert_eq!(
            check_blob_delete(state, Role::Admin, &holder("m3", "d1"), &author),
            Err(BlobDeleteForbidden)
        );
        // Without a log the ticket decides.
        assert_eq!(
            check_blob_delete(None, Role::Admin, &holder("m3", "d1"), &author),
            Ok(())
        );
        assert_eq!(
            check_blob_delete(None, Role::Member, &holder("m3", "d1"), &author),
            Err(BlobDeleteForbidden)
        );
        assert_eq!(BlobDeleteForbidden.code(), ErrorCode::Forbidden);
    }

    /// Defense in depth behind `check_restricted`: in a team with a log the author counts only
    /// from a device the log lists, and the role is the lower of log role and ticket role.
    #[test]
    fn blob_rights_in_a_team_with_a_log_follow_the_log() {
        use crate::relay::{check_blob_delete, BlobDeleteForbidden};
        let (verifier, _) = team_of_three();
        let state = verifier.state();
        let admin = holder("m2", "d1");
        let member = holder("m3", "d1");
        let author = MemberId::new("m3").unwrap();
        let other = MemberId::new("m9").unwrap();
        // The author on its listed device.
        assert_eq!(
            check_blob_delete(state, Role::Member, &member, &author),
            Ok(())
        );
        // The author on a device the log does not list.
        assert_eq!(
            check_blob_delete(state, Role::Member, &holder("m3", "d2"), &author),
            Err(BlobDeleteForbidden)
        );
        // Admin in the log, member on the ticket: the lower role.
        assert_eq!(
            check_blob_delete(state, Role::Member, &admin, &other),
            Err(BlobDeleteForbidden)
        );
        // Member in the log, admin on the ticket: a ticket never raises a role.
        assert_eq!(
            check_blob_delete(state, Role::Admin, &member, &other),
            Err(BlobDeleteForbidden)
        );
        // Admin on both.
        assert_eq!(
            check_blob_delete(state, Role::Admin, &admin, &other),
            Ok(())
        );
    }

    /// A key proven once stops counting when the log no longer lists it: after the device's
    /// removal, and after the same holder id was admitted again with new keys.
    #[test]
    fn a_proven_old_key_stops_counting_after_removal_or_readmission() {
        let (mut verifier, owner_key) = team_of_three();
        let owner = holder("m1", "d1");
        let member = holder("m3", "d1");
        let old = verifier.state().unwrap().device(&member).unwrap().keys.sign;
        assert!(counts_as_listed(
            verifier.state().unwrap(),
            &member,
            Possession::Proven(&old)
        ));
        append(
            &mut verifier,
            &owner_key,
            &owner,
            EntryKind::RemoveMember {
                member: member.member.clone(),
            },
        );
        assert!(!counts_as_listed(
            verifier.state().unwrap(),
            &member,
            Possession::Proven(&old)
        ));
        assert_eq!(
            check_restricted(
                verifier.state(),
                &member,
                Possession::Proven(&old),
                &presence()
            ),
            Err(Restricted)
        );
        let again = identity();
        append(
            &mut verifier,
            &owner_key,
            &owner,
            EntryKind::AddMember {
                member: member.member.clone(),
                role: Role::Member,
                devices: vec![DeviceEntry::new(&team(), &member, &again, false)],
            },
        );
        let state = verifier.state().unwrap();
        assert!(!counts_as_listed(state, &member, Possession::Proven(&old)));
        let new = again.public_keys().sign;
        assert!(counts_as_listed(state, &member, Possession::Proven(&new)));
    }

    /// Before the team's first entry nothing is restricted: the owner's first device creates
    /// the log and cannot be in it yet.
    #[test]
    fn a_team_without_a_log_restricts_nothing() {
        let me = holder("m1", "d1");
        assert_eq!(
            check_restricted(None, &me, Possession::Unproven, &presence()),
            Ok(())
        );
    }
}
