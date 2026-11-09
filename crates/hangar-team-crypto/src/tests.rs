//! Whole-team flows, the membership log rules, record sealing, and the test vectors that
//! `test_vectors.py` recomputes with the Python standard library.

use hangar_team_protocol::e2e::{entry_hash, LogHash, MemberChain};
use hangar_team_protocol::ids::{Ciphertext, DeviceId, Epoch, MemberId, TeamId, UnixMillis};
use hangar_team_protocol::wire::{BlobKind, Holder, MemberLogAppend, MemberLogEntry, Role};
use zeroize::Zeroizing;

use crate::encoding::{from_ciphertext, to_ciphertext};
use crate::entropy::{FixedEntropy, OsEntropy};
use crate::epoch::{wrap, EpochCommitment, HistoryLink, TeamSecret, WrapError};
use crate::identity::{log_safety_number, safety_number, DeviceIdentity};
use crate::memberlog::{DeviceEntry, EntryBody, EntryKind, LogError, LogVerifier, TeamState};
use crate::record::{EpochCipher, OpenError, RecordContext, RecordKind, ReplayGuard, SealError};
use crate::recovery::RecoveryCode;
use crate::rotation::{plan_rotation, wrap_for_device, wrap_to_all, RotationError};

fn team_id() -> TeamId {
    TeamId::new("team-1").expect("id")
}

fn holder(member: &str, device: &str) -> Holder {
    Holder {
        member: MemberId::new(member).expect("id"),
        device: DeviceId::new(device).expect("id"),
    }
}

fn hex(bytes: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(bytes)
}

fn unhex(text: &str) -> Vec<u8> {
    data_encoding::HEXLOWER
        .decode(text.as_bytes())
        .expect("hex")
}

struct Device {
    holder: Holder,
    identity: DeviceIdentity,
}

impl Device {
    fn new(member: &str, device: &str) -> Self {
        Self {
            holder: holder(member, device),
            identity: DeviceIdentity::generate(&mut OsEntropy).expect("rng"),
        }
    }

    fn entry(&self) -> DeviceEntry {
        DeviceEntry {
            device: self.holder.device.clone(),
            keys: self.identity.public_keys(),
            recovery: false,
        }
    }
}

/// A relay's stored log: appends checked like the relay does, read back as entries.
struct Log {
    chain: MemberChain,
    entries: Vec<MemberLogEntry>,
}

impl Log {
    fn new() -> Self {
        Self {
            chain: MemberChain::new(),
            entries: Vec::new(),
        }
    }

    fn append(&mut self, append: &MemberLogAppend, by: &Holder) -> MemberLogEntry {
        let hash = self.chain.append(append).expect("relay accepts the chain");
        let entry = MemberLogEntry {
            index: append.index,
            hash,
            entry: append.entry.clone(),
            by: by.clone(),
            at: UnixMillis(0),
        };
        self.entries.push(entry.clone());
        entry
    }
}

/// A team with an owner, an admin and a member, all applied to a verifier.
struct Team {
    owner: Device,
    admin: Device,
    member: Device,
    log: Log,
    verifier: LogVerifier,
}

impl Team {
    fn new() -> Self {
        let owner = Device::new("owner", "laptop");
        let admin = Device::new("admin", "desktop");
        let member = Device::new("member", "laptop");
        let mut team = Self {
            owner,
            admin,
            member,
            log: Log::new(),
            verifier: LogVerifier::new(team_id()),
        };
        let create = EntryBody::create_team(
            team_id(),
            team.owner.holder.clone(),
            UnixMillis(1),
            vec![team.owner.entry()],
        );
        team.submit_signed(&create, Who::Owner).expect("create");
        let add_admin = EntryKind::AddMember {
            member: team.admin.holder.member.clone(),
            role: Role::Admin,
            devices: vec![team.admin.entry()],
        };
        team.submit(Who::Owner, add_admin).expect("add admin");
        let add_member = EntryKind::AddMember {
            member: team.member.holder.member.clone(),
            role: Role::Member,
            devices: vec![team.member.entry()],
        };
        team.submit(Who::Admin, add_member).expect("add member");
        team
    }

    fn device(&self, who: Who) -> &Device {
        match who {
            Who::Owner => &self.owner,
            Who::Admin => &self.admin,
            Who::Member => &self.member,
        }
    }

    fn state(&self) -> &TeamState {
        self.verifier.state().expect("state")
    }

    fn submit(&mut self, who: Who, kind: EntryKind) -> Result<(), LogError> {
        let body = self
            .state()
            .next_body(self.device(who).holder.clone(), UnixMillis(2), kind);
        self.submit_signed(&body, who)
    }

    /// Signs without the sender-side check, appends like the relay, verifies like a client.
    fn submit_signed(&mut self, body: &EntryBody, who: Who) -> Result<(), LogError> {
        let append = body.sign(&self.device(who).identity)?;
        let by = self.device(who).holder.clone();
        let entry = self.log.append(&append, &by);
        self.verifier.apply(&entry)
    }
}

#[derive(Clone, Copy)]
enum Who {
    Owner,
    Admin,
    Member,
}

#[test]
fn a_new_device_verifies_the_whole_log_into_the_same_state() {
    let team = Team::new();
    let mut fresh = LogVerifier::new(team_id());
    fresh.apply_all(&team.log.entries).expect("verify");
    assert_eq!(fresh.state(), team.verifier.state());
    let state = fresh.state().expect("state");
    assert_eq!(state.members().len(), 3);
    assert_eq!(state.owner(), &team.owner.holder.member);
    assert_eq!(
        state.member(&team.admin.holder.member).map(|m| m.role),
        Some(Role::Admin)
    );
    assert_eq!(
        state.device(&team.member.holder).map(|d| d.keys),
        Some(team.member.identity.public_keys())
    );
    assert_eq!(state.head().index, 3);
    // Both sides compute the same log safety number.
    assert_eq!(
        log_safety_number(&team_id(), state.head()),
        log_safety_number(&team_id(), team.state().head())
    );
}

#[test]
fn rotation_reaches_every_device_and_records_open_across_them() {
    let mut team = Team::new();
    let rotation = plan_rotation(
        team.state(),
        &team.admin.identity,
        &team.admin.holder,
        None,
        UnixMillis(3),
        &mut OsEntropy,
    )
    .expect("plan");
    assert_eq!(rotation.epoch, Epoch(1));
    assert_eq!(rotation.rotate.wrapped.len(), 3);
    let entry = team.log.append(&rotation.append, &team.admin.holder);
    team.verifier.apply(&entry).expect("rotate entry");
    let state = team.state();
    assert_eq!(state.current_epoch().map(|e| e.epoch), Some(Epoch(1)));

    // Every device opens its own wrapped key, checked against the logged commitment.
    for who in [Who::Owner, Who::Admin, Who::Member] {
        let device = team.device(who);
        let wrapped = rotation
            .rotate
            .wrapped
            .iter()
            .find(|w| w.holder == device.holder)
            .expect("wrapped for every device");
        let secret = state
            .unwrap_key(&device.identity, &device.holder, Epoch(1), &wrapped.data)
            .expect("unwrap");
        assert_eq!(secret, rotation.secret);
    }

    // The member seals a note, the owner opens it with the member's key from the log.
    let cipher = EpochCipher::new(&rotation.secret, &team_id(), Epoch(1)).expect("keys");
    let context = RecordContext {
        kind: RecordKind::ReservationNote,
        record: b"task hash",
        sender: &team.member.holder,
    };
    let sealed = cipher
        .seal(&context, &team.member.identity, b"on it", &mut OsEntropy)
        .expect("seal");
    let sender_key = state.device(&team.member.holder).expect("member").keys.sign;
    let opened = cipher.open(&context, &sender_key, &sealed).expect("open");
    assert_eq!(opened.plaintext.as_slice(), b"on it");
    let mut guard = ReplayGuard::new(16);
    guard.check(&opened).expect("first time");
    assert_eq!(guard.check(&opened), Err(OpenError::Replay));
}

#[test]
fn removal_makes_rotation_due_and_history_links_reach_back() {
    let mut team = Team::new();
    let first = plan_rotation(
        team.state(),
        &team.owner.identity,
        &team.owner.holder,
        None,
        UnixMillis(3),
        &mut OsEntropy,
    )
    .expect("plan");
    let entry = team.log.append(&first.append, &team.owner.holder);
    team.verifier.apply(&entry).expect("epoch 1");

    let member = team.member.holder.member.clone();
    team.submit(Who::Admin, EntryKind::RemoveMember { member })
        .expect("remove");
    assert!(team.state().rotation_due());
    assert!(team
        .state()
        .find_device(&team.member.identity.public_keys())
        .is_none());

    let second = plan_rotation(
        team.state(),
        &team.admin.identity,
        &team.admin.holder,
        Some(&first.secret),
        UnixMillis(4),
        &mut OsEntropy,
    )
    .expect("plan");
    assert_eq!(second.epoch, Epoch(2));
    // The removed member gets no key of the new epoch.
    assert_eq!(second.rotate.wrapped.len(), 2);
    assert!(second
        .rotate
        .wrapped
        .iter()
        .all(|w| w.holder != team.member.holder));
    let entry = team.log.append(&second.append, &team.admin.holder);
    team.verifier.apply(&entry).expect("epoch 2");
    assert!(!team.state().rotation_due());

    let previous = team
        .state()
        .previous_secret(Epoch(2), &second.secret)
        .expect("history");
    assert_eq!(previous, first.secret);
    // Epoch 1 has no predecessor.
    assert!(matches!(
        team.state().previous_secret(Epoch(1), &first.secret),
        Err(WrapError::UnknownEpoch(_))
    ));
}

#[test]
fn a_new_device_gets_the_current_key_through_key_wrap() {
    let mut team = Team::new();
    let rotation = plan_rotation(
        team.state(),
        &team.owner.identity,
        &team.owner.holder,
        None,
        UnixMillis(3),
        &mut OsEntropy,
    )
    .expect("plan");
    let entry = team.log.append(&rotation.append, &team.owner.holder);
    team.verifier.apply(&entry).expect("epoch");

    let phone = Device::new("member", "phone");
    // Not in the log yet: no key.
    assert_eq!(
        wrap_for_device(
            team.state(),
            &rotation.secret,
            &phone.holder,
            &mut OsEntropy
        )
        .err(),
        Some(WrapError::UnknownDevice)
    );
    team.submit(
        Who::Admin,
        EntryKind::AddDevice {
            member: phone.holder.member.clone(),
            device: phone.entry(),
        },
    )
    .expect("add device");
    let wrap = wrap_for_device(
        team.state(),
        &rotation.secret,
        &phone.holder,
        &mut OsEntropy,
    )
    .expect("wrap");
    assert_eq!(wrap.epoch, Epoch(1));
    let secret = team
        .state()
        .unwrap_key(&phone.identity, &phone.holder, Epoch(1), &wrap.wrapped.data)
        .expect("unwrap");
    assert_eq!(secret, rotation.secret);
    // A secret that is not the current epoch's is refused.
    let other = TeamSecret::generate(&mut OsEntropy).expect("rng");
    assert_eq!(
        wrap_for_device(team.state(), &other, &phone.holder, &mut OsEntropy).err(),
        Some(WrapError::Commitment(1))
    );
}

#[test]
fn members_cannot_change_the_log() {
    let mut team = Team::new();
    let intruder = Device::new("member", "second");
    let result = team.submit(
        Who::Member,
        EntryKind::AddDevice {
            member: intruder.holder.member.clone(),
            device: intruder.entry(),
        },
    );
    assert_eq!(result, Err(LogError::Forbidden));
    // The log stays stopped at the refused entry: the next one leaves a gap.
    let next = team.state().next_body(
        team.owner.holder.clone(),
        UnixMillis(5),
        EntryKind::RemoveDevice {
            holder: team.member.holder.clone(),
        },
    );
    let append = next.sign(&team.owner.identity).expect("sign");
    let mut relay = MemberChain::resume(Some(*team.state().head()));
    let hash = relay.append(&append).expect("chain");
    let refused = team.verifier.apply(&MemberLogEntry {
        index: append.index + 1,
        hash,
        entry: append.entry,
        by: team.owner.holder.clone(),
        at: UnixMillis(0),
    });
    assert!(matches!(refused, Err(LogError::Chain(_))));
}

#[test]
fn role_rules_hold() {
    let mut team = Team::new();
    let owner = team.owner.holder.member.clone();
    let admin = team.admin.holder.member.clone();
    let member = team.member.holder.member.clone();
    // An admin cannot touch the owner, promote, or create admins.
    for kind in [
        EntryKind::RemoveMember {
            member: owner.clone(),
        },
        EntryKind::RemoveDevice {
            holder: team.owner.holder.clone(),
        },
        EntryKind::ChangeRole {
            member: member.clone(),
            role: Role::Admin,
        },
        EntryKind::AddMember {
            member: MemberId::new("new").expect("id"),
            role: Role::Admin,
            devices: vec![Device::new("new", "d").entry()],
        },
    ] {
        let body = team
            .state()
            .next_body(team.admin.holder.clone(), UnixMillis(5), kind);
        assert_eq!(team.state().check(&body), Err(LogError::Forbidden));
    }
    // No second owner, no self removal, no removal of the issuing device.
    for (who, kind) in [
        (
            Who::Owner,
            EntryKind::ChangeRole {
                member: admin.clone(),
                role: Role::Owner,
            },
        ),
        (
            Who::Admin,
            EntryKind::RemoveMember {
                member: admin.clone(),
            },
        ),
        (
            Who::Admin,
            EntryKind::RemoveDevice {
                holder: team.admin.holder.clone(),
            },
        ),
    ] {
        let body = team
            .state()
            .next_body(team.device(who).holder.clone(), UnixMillis(5), kind);
        assert_eq!(team.state().check(&body), Err(LogError::Forbidden));
    }
    // The owner demotes the admin; then the former admin may not append.
    team.submit(
        Who::Owner,
        EntryKind::ChangeRole {
            member: admin.clone(),
            role: Role::Member,
        },
    )
    .expect("demote");
    let body = team.state().next_body(
        team.admin.holder.clone(),
        UnixMillis(6),
        EntryKind::RemoveMember { member },
    );
    assert_eq!(team.state().check(&body), Err(LogError::Forbidden));
    let body = team.state().next_body(
        team.owner.holder.clone(),
        UnixMillis(6),
        EntryKind::ChangeRole {
            member: admin,
            role: Role::Member,
        },
    );
    assert_eq!(team.state().check(&body), Err(LogError::NoChange));
}

#[test]
fn keys_are_vouched_for_once() {
    let mut team = Team::new();
    // The member's keys again, for a new member.
    let reused = DeviceEntry {
        device: DeviceId::new("x").expect("id"),
        keys: team.member.identity.public_keys(),
        recovery: false,
    };
    let body = team.state().next_body(
        team.owner.holder.clone(),
        UnixMillis(5),
        EntryKind::AddMember {
            member: MemberId::new("copycat").expect("id"),
            role: Role::Member,
            devices: vec![reused.clone()],
        },
    );
    assert_eq!(team.state().check(&body), Err(LogError::KeyReused));
    // Still refused after the member is gone.
    team.submit(
        Who::Owner,
        EntryKind::RemoveMember {
            member: team.member.holder.member.clone(),
        },
    )
    .expect("remove");
    let body = team.state().next_body(
        team.owner.holder.clone(),
        UnixMillis(6),
        EntryKind::AddMember {
            member: MemberId::new("copycat").expect("id"),
            role: Role::Member,
            devices: vec![reused],
        },
    );
    assert_eq!(team.state().check(&body), Err(LogError::KeyReused));
    // An invalid signing key.
    let mut weak = Device::new("weak", "d").entry();
    let mut identity_point = [0u8; 32];
    identity_point[0] = 1;
    weak.keys.sign = hangar_team_protocol::e2e::PublicKey::from_bytes(identity_point);
    let body = team.state().next_body(
        team.owner.holder.clone(),
        UnixMillis(7),
        EntryKind::AddMember {
            member: MemberId::new("weak").expect("id"),
            role: Role::Member,
            devices: vec![weak],
        },
    );
    assert_eq!(team.state().check(&body), Err(LogError::InvalidKey));
}

#[test]
fn entries_are_bound_to_team_position_and_signer() {
    let team = Team::new();
    // Another team's verifier refuses the same bytes.
    let mut other = LogVerifier::new(TeamId::new("team-2").expect("id"));
    assert_eq!(
        other.apply(&team.log.entries[0]),
        Err(LogError::TeamMismatch)
    );

    // A relay splicing entry 3 in as entry 2 (recomputing the unkeyed chain hash) is caught by
    // the signed index.
    let mut verifier = LogVerifier::new(team_id());
    verifier.apply(&team.log.entries[0]).expect("create");
    let spliced = MemberLogEntry {
        index: 2,
        hash: entry_hash(
            2,
            Some(&team.log.entries[0].hash),
            &team.log.entries[2].entry,
        )
        .expect("hash"),
        ..team.log.entries[2].clone()
    };
    assert_eq!(verifier.apply(&spliced), Err(LogError::IndexMismatch));

    // A forged signature: flip one bit of the signature.
    let mut bytes = from_ciphertext(&team.log.entries[1].entry).expect("bytes");
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let forged = to_ciphertext(&bytes).expect("ct");
    let entry = MemberLogEntry {
        index: 2,
        hash: entry_hash(2, Some(&team.log.entries[0].hash), &forged).expect("hash"),
        entry: forged,
        ..team.log.entries[1].clone()
    };
    assert_eq!(verifier.apply(&entry), Err(LogError::Signature));

    // An entry signed by a device the log does not list.
    let stranger = Device::new("owner", "stolen");
    let body = verifier.state().expect("state").next_body(
        stranger.holder.clone(),
        UnixMillis(9),
        EntryKind::RemoveMember {
            member: team.member.holder.member.clone(),
        },
    );
    let append = body.sign(&stranger.identity).expect("sign");
    let entry = MemberLogEntry {
        index: 2,
        hash: entry_hash(2, Some(&team.log.entries[0].hash), &append.entry).expect("hash"),
        entry: append.entry,
        by: stranger.holder,
        at: UnixMillis(0),
    };
    assert_eq!(verifier.apply(&entry), Err(LogError::UnknownIssuer));
    // After all refusals the verifier still accepts the real entry 2.
    verifier.apply(&team.log.entries[1]).expect("entry 2");
}

#[test]
fn the_first_entry_must_create_the_team_and_sign_itself() {
    let owner = Device::new("owner", "laptop");
    let other = Device::new("owner", "other");
    // Signed by a device the entry does not list.
    let body = EntryBody::create_team(
        team_id(),
        other.holder.clone(),
        UnixMillis(1),
        vec![owner.entry()],
    );
    let append = body.sign(&other.identity).expect("sign");
    let mut log = Log::new();
    let entry = log.append(&append, &other.holder);
    assert_eq!(
        LogVerifier::new(team_id()).apply(&entry),
        Err(LogError::UnknownIssuer)
    );
    // Lists the device but signed by another key.
    let body = EntryBody::create_team(
        team_id(),
        owner.holder.clone(),
        UnixMillis(1),
        vec![owner.entry()],
    );
    let append = body.sign(&other.identity).expect("sign");
    let mut log = Log::new();
    let entry = log.append(&append, &owner.holder);
    assert_eq!(
        LogVerifier::new(team_id()).apply(&entry),
        Err(LogError::Signature)
    );
    // Starts with something else.
    let body = EntryBody {
        kind: EntryKind::RemoveMember {
            member: owner.holder.member.clone(),
        },
        ..EntryBody::create_team(team_id(), owner.holder.clone(), UnixMillis(1), vec![])
    };
    let append = body.sign(&owner.identity).expect("sign");
    let mut log = Log::new();
    let entry = log.append(&append, &owner.holder);
    assert_eq!(
        LogVerifier::new(team_id()).apply(&entry),
        Err(LogError::NotCreated)
    );
}

#[test]
fn rotation_rules_hold() {
    let team = Team::new();
    // A member cannot rotate.
    assert_eq!(
        plan_rotation(
            team.state(),
            &team.member.identity,
            &team.member.holder,
            None,
            UnixMillis(3),
            &mut OsEntropy,
        )
        .err(),
        Some(RotationError::Log(LogError::Forbidden))
    );
    // Another device's identity.
    assert_eq!(
        plan_rotation(
            team.state(),
            &team.member.identity,
            &team.owner.holder,
            None,
            UnixMillis(3),
            &mut OsEntropy,
        )
        .err(),
        Some(RotationError::Log(LogError::UnknownIssuer))
    );
    // History without a previous epoch.
    let secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
    assert_eq!(
        plan_rotation(
            team.state(),
            &team.owner.identity,
            &team.owner.holder,
            Some(&secret),
            UnixMillis(3),
            &mut OsEntropy,
        )
        .err(),
        Some(RotationError::Log(LogError::NoPreviousEpoch))
    );
    // An epoch out of order.
    let body = team.state().next_body(
        team.owner.holder.clone(),
        UnixMillis(3),
        EntryKind::RotateEpoch {
            epoch: Epoch(2),
            commitment: secret.commitment(&team_id(), Epoch(2)),
            history: None,
        },
    );
    assert_eq!(
        team.state().check(&body),
        Err(LogError::EpochOrder {
            expected: 1,
            got: 2
        })
    );
    assert_eq!(
        wrap_to_all(team.state(), &secret, Epoch(1), &mut OsEntropy)
            .expect("wrap")
            .len(),
        3
    );
}

#[test]
fn records_are_bound_to_their_context() {
    let secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
    let alice = Device::new("alice", "a");
    let bob = Device::new("bob", "b");
    let cipher = EpochCipher::new(&secret, &team_id(), Epoch(4)).expect("keys");
    let context = RecordContext {
        kind: RecordKind::ClaimDetail,
        record: b"session-1",
        sender: &alice.holder,
    };
    let sealed = cipher
        .seal(&context, &alice.identity, b"src/main.rs", &mut OsEntropy)
        .expect("seal");
    let key = alice.identity.public_keys().sign;
    assert!(cipher.open(&context, &key, &sealed).is_ok());

    let other_kind = RecordContext {
        kind: RecordKind::Blob(BlobKind::Decision),
        ..context
    };
    assert_eq!(
        cipher.open(&other_kind, &key, &sealed).err(),
        Some(OpenError::Signature)
    );
    let other_record = RecordContext {
        record: b"session-2",
        ..context
    };
    assert_eq!(
        cipher.open(&other_record, &key, &sealed).err(),
        Some(OpenError::Signature)
    );
    // Bob claims Alice's record as his own.
    let as_bob = RecordContext {
        sender: &bob.holder,
        ..context
    };
    assert_eq!(
        cipher
            .open(&as_bob, &bob.identity.public_keys().sign, &sealed)
            .err(),
        Some(OpenError::Signature)
    );
    // Another epoch's cipher.
    let later = EpochCipher::new(&secret, &team_id(), Epoch(5)).expect("keys");
    assert_eq!(
        later.open(&context, &key, &sealed).err(),
        Some(OpenError::EpochMismatch {
            expected: 5,
            got: 4
        })
    );
    // Another team with the same secret and epoch: the signature covers the team.
    let elsewhere =
        EpochCipher::new(&secret, &TeamId::new("team-2").expect("id"), Epoch(4)).expect("keys");
    assert_eq!(
        elsewhere.open(&context, &key, &sealed).err(),
        Some(OpenError::Signature)
    );
    // A member who signs correctly but seals under a different key.
    let wrong_secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
    let wrong = EpochCipher::new(&wrong_secret, &team_id(), Epoch(4)).expect("keys");
    let foreign = wrong
        .seal(&context, &alice.identity, b"x", &mut OsEntropy)
        .expect("seal");
    assert_eq!(
        cipher.open(&context, &key, &foreign).err(),
        Some(OpenError::Decrypt)
    );
    // Malformed and versioned data.
    let short = hangar_team_protocol::ids::Sealed {
        epoch: Epoch(4),
        data: to_ciphertext(&[1, 2, 3]).expect("ct"),
    };
    assert_eq!(
        cipher.open(&context, &key, &short).err(),
        Some(OpenError::Malformed)
    );
    let mut bytes = from_ciphertext(&sealed.data).expect("bytes");
    bytes[0] = 7;
    let versioned = hangar_team_protocol::ids::Sealed {
        epoch: Epoch(4),
        data: to_ciphertext(&bytes).expect("ct"),
    };
    assert_eq!(
        cipher.open(&context, &key, &versioned).err(),
        Some(OpenError::Version(7))
    );
}

#[test]
fn record_size_follows_the_protocol_limits() {
    let secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
    let alice = Device::new("alice", "a");
    let cipher = EpochCipher::new(&secret, &team_id(), Epoch(1)).expect("keys");
    let context = RecordContext {
        kind: RecordKind::PresenceStatus,
        record: b"",
        sender: &alice.holder,
    };
    let max = RecordKind::PresenceStatus.max_plaintext();
    let fits = cipher
        .seal(&context, &alice.identity, &vec![0u8; max], &mut OsEntropy)
        .expect("fits");
    assert!(fits.data.as_str().len() <= RecordKind::PresenceStatus.max_chars());
    assert_eq!(
        cipher
            .seal(
                &context,
                &alice.identity,
                &vec![0u8; max + 3],
                &mut OsEntropy
            )
            .err(),
        Some(SealError::TooLarge)
    );
    assert!(RecordKind::Blob(BlobKind::Transcript).max_plaintext() > 100_000);
}

#[test]
fn a_recovery_code_restores_its_device() {
    let mut team = Team::new();
    let code = RecoveryCode::generate(&mut OsEntropy).expect("rng");
    let recovery = code.identity().expect("identity");
    team.submit(
        Who::Owner,
        EntryKind::AddDevice {
            member: team.member.holder.member.clone(),
            device: DeviceEntry {
                device: DeviceId::new("recovery-1").expect("id"),
                keys: recovery.public_keys(),
                recovery: true,
            },
        },
    )
    .expect("add recovery device");
    let rotation = plan_rotation(
        team.state(),
        &team.owner.identity,
        &team.owner.holder,
        None,
        UnixMillis(3),
        &mut OsEntropy,
    )
    .expect("plan");
    let entry = team.log.append(&rotation.append, &team.owner.holder);
    team.verifier.apply(&entry).expect("rotate");

    // On a new installation: type the code, find the device, open its wrapped key.
    let typed = code.reveal().to_lowercase();
    let restored = RecoveryCode::parse(&typed)
        .expect("parse")
        .identity()
        .expect("identity");
    let holder = team
        .state()
        .find_device(&restored.public_keys())
        .expect("vouched for");
    assert_eq!(holder, self::holder("member", "recovery-1"));
    assert_eq!(team.state().device(&holder).map(|d| d.recovery), Some(true));
    let wrapped = rotation
        .rotate
        .wrapped
        .iter()
        .find(|w| w.holder == holder)
        .expect("wrapped to the recovery device");
    let secret = team
        .state()
        .unwrap_key(&restored, &holder, Epoch(1), &wrapped.data)
        .expect("unwrap");
    assert_eq!(secret, rotation.secret);
}

#[test]
fn entry_decoding_is_strict() {
    let team = Team::new();
    let bytes = from_ciphertext(&team.log.entries[1].entry).expect("bytes");
    let body = &bytes[..bytes.len() - 64];
    assert!(EntryBody::decode(body).is_ok());
    let mut longer = body.to_vec();
    longer.extend_from_slice(&[0, 0, 0, 0]);
    assert_eq!(EntryBody::decode(&longer), Err(LogError::Malformed));
    assert_eq!(EntryBody::decode(&body[..40]), Err(LogError::Malformed));
}

// ---------------------------------------------------------------------------------------------
// Test vectors. `test_vectors.py` reads the `VECTOR_*` constants of this file and recomputes
// each one with the Python standard library (its own Ed25519, X25519, ChaCha20-Poly1305 and
// HPKE, each checked first against its RFC vector).
// ---------------------------------------------------------------------------------------------

const VECTOR_TEAM: &str = "team-vectors";
const VECTOR_ISSUED_AT: u64 = 1_700_000_000_000;

const VECTOR_OWNER_SIGN: &str = "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8";
const VECTOR_OWNER_KEM: &str = "358072d6365880d1aeea329adf9121383851ed21a28e3b75e965d0d2cd166254";
const VECTOR_OWNER_SAFETY: &str = "66163 27494 60846 17528 53184 02098";
const VECTOR_CREATE_ENTRY: &str =
    "AAAAHmhhbmdhci10ZWFtL3YxL21lbWJlcmxvZy1lbnRyeQAAAAEBAAAADHRlYW0tdmVjdG9ycwAAAAgAAAAAAAAAAQAAAAAAAAAIAAABi8_laAAAAAAFb3duZXIAAAAGbGFwdG9wAAAAAQEAAAAEAAAAAQAAAAZsYXB0b3AAAAAgA6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbgAAAAgNYBy1jZYgNGu6jKa35EhODhR7SGijjt16WXQ0s0WYlQAAAABAPFpGw_0k3FUraSZb0KYe-vIo_HHvIoYlZB3BTQ3CE09Sq0PfPtjmJMdk1fyO08Bmdbxc_ufdhYRYclY5mvoywc";
const VECTOR_CREATE_HASH: &str = "2419cbed64010b5ae67842166ecf47a0fcc2b03fc81fc3114e853bf29bae5014";
const VECTOR_COMMITMENT: &str = "697f8ef1cb94e90a5a84e4b4f01af743f6cf2578ad55f3abb6e905145c0f6446";
const VECTOR_ROTATE_ENTRY: &str =
    "AAAAHmhhbmdhci10ZWFtL3YxL21lbWJlcmxvZy1lbnRyeQAAAAEBAAAADHRlYW0tdmVjdG9ycwAAAAgAAAAAAAAAAgAAACAkGcvtZAELWuZ4QhZuz0eg_MKwP8gfwxFOhTvym65QFAAAAAgAAAGLz-VoAQAAAAVvd25lcgAAAAZsYXB0b3AAAAABBwAAAAQAAAABAAAAIGl_jvHLlOkKWoTktPAa90P2zyV4rVXzq7bpBRRcD2RGAAAAAGkk95PFknWJjFY96duxHwfhdMcmRIWs6WMhXupHfKImTIXehZVo1iwEo48OFBdQzWLJ7bf_WPAkkqWfyrfc-Ao";
const VECTOR_WRAP: &str =
    "AXrwPfFZ4tdXUcGojrWp6HmI8Tjc51luvaOtfwuwSoc0R1CLX4p-JJW5dm65asMN3vtTv6m5QoiDiOaFrm3qE479oj9ZzlxLDaYWu8RAkSwO";
const VECTOR_HISTORY: &str =
    "01c0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7727996fdc22aa37d1a982b95a5d86bc3f93ed801bb75bb8647c4f177a22aacb1855b87285b9c5f587de1060daf284b54";
const VECTOR_RECORD: &str =
    "AeDh4uPk5ebn6Onq6-zt7u_w8fLz9PX296hmbH6C_z9ZqYuyTXVttv_VF_xNm3DzRIKm5JN6ea2Poqf9HYubBAGR_wEqODKofpPIuzaKfbjuIkqFisKNdpPGfion_9zWkSOMyP17KxzXZftmZMIaTZSaDw";
const VECTOR_RECOVERY_CODE: &str = "0481-24GK-2GAH-C5RR-34D1-P70X-3RFW-2KAP";
const VECTOR_RECOVERY_SIGN: &str =
    "6cbe236cc50eef0b3c6cfd4bdd01608f2e2be38e76f257d39cb609a135616a97";
const VECTOR_RECOVERY_KEM: &str =
    "35208aaea6e62f68521bfa2565cb2ec9059190da11007e49fa44259b6669fe00";

struct Vectors {
    owner_sign: String,
    owner_kem: String,
    owner_safety: String,
    create_entry: String,
    create_hash: String,
    commitment: String,
    rotate_entry: String,
    wrap: String,
    history: String,
    record: String,
    recovery_code: String,
    recovery_sign: String,
    recovery_kem: String,
}

fn compute_vectors() -> Vectors {
    let team = TeamId::new(VECTOR_TEAM).expect("id");
    let owner_holder = holder("owner", "laptop");
    let owner = DeviceIdentity::generate(&mut FixedEntropy::counting(0x00, 64)).expect("id");
    let keys = owner.public_keys();

    let create = EntryBody::create_team(
        team.clone(),
        owner_holder.clone(),
        UnixMillis(VECTOR_ISSUED_AT),
        vec![DeviceEntry {
            device: owner_holder.device.clone(),
            keys,
            recovery: false,
        }],
    );
    let create_append = create.sign(&owner).expect("sign");
    let create_hash: LogHash = entry_hash(1, None, &create_append.entry).expect("hash");

    let secret = TeamSecret::generate(&mut FixedEntropy::counting(0x40, 32)).expect("rng");
    let commitment = secret.commitment(&team, Epoch(1));
    let rotate = EntryBody {
        team: team.clone(),
        index: 2,
        prev: Some(create_hash),
        issued_at: UnixMillis(VECTOR_ISSUED_AT + 1),
        issuer: owner_holder.clone(),
        kind: EntryKind::RotateEpoch {
            epoch: Epoch(1),
            commitment,
            history: None,
        },
    };
    let rotate_append = rotate.sign(&owner).expect("sign");

    let wrapped = wrap(
        &secret,
        &team,
        Epoch(1),
        &owner_holder,
        &keys.kem,
        &mut FixedEntropy::counting(0x80, 32),
    )
    .expect("wrap");

    let next = TeamSecret::generate(&mut FixedEntropy::counting(0xa0, 32)).expect("rng");
    let history = HistoryLink::seal(
        &next,
        &team,
        Epoch(2),
        &secret,
        &mut FixedEntropy::counting(0xc0, 24),
    )
    .expect("link");

    let cipher = EpochCipher::new(&secret, &team, Epoch(1)).expect("keys");
    let record = cipher
        .seal(
            &RecordContext {
                kind: RecordKind::ReservationNote,
                record: b"task-hash",
                sender: &owner_holder,
            },
            &owner,
            b"hello team",
            &mut FixedEntropy::counting(0xe0, 24),
        )
        .expect("seal");

    let code = RecoveryCode::generate(&mut FixedEntropy::counting(0x10, 16)).expect("rng");
    let recovery = code.identity().expect("id").public_keys();

    Vectors {
        owner_sign: hex(keys.sign.as_bytes()),
        owner_kem: hex(keys.kem.as_bytes()),
        owner_safety: safety_number(&owner_holder, &keys).to_string(),
        create_entry: create_append.entry.as_str().to_owned(),
        create_hash: create_hash.to_hex(),
        commitment: hex(&commitment.0),
        rotate_entry: rotate_append.entry.as_str().to_owned(),
        wrap: wrapped.data.as_str().to_owned(),
        history: hex(history.as_bytes()),
        record: record.data.as_str().to_owned(),
        recovery_code: code.reveal().to_string(),
        recovery_sign: hex(recovery.sign.as_bytes()),
        recovery_kem: hex(recovery.kem.as_bytes()),
    }
}

#[test]
fn test_vectors() {
    let v = compute_vectors();
    let pairs = [
        ("VECTOR_OWNER_SIGN", VECTOR_OWNER_SIGN, &v.owner_sign),
        ("VECTOR_OWNER_KEM", VECTOR_OWNER_KEM, &v.owner_kem),
        ("VECTOR_OWNER_SAFETY", VECTOR_OWNER_SAFETY, &v.owner_safety),
        ("VECTOR_CREATE_ENTRY", VECTOR_CREATE_ENTRY, &v.create_entry),
        ("VECTOR_CREATE_HASH", VECTOR_CREATE_HASH, &v.create_hash),
        ("VECTOR_COMMITMENT", VECTOR_COMMITMENT, &v.commitment),
        ("VECTOR_ROTATE_ENTRY", VECTOR_ROTATE_ENTRY, &v.rotate_entry),
        ("VECTOR_WRAP", VECTOR_WRAP, &v.wrap),
        ("VECTOR_HISTORY", VECTOR_HISTORY, &v.history),
        ("VECTOR_RECORD", VECTOR_RECORD, &v.record),
        (
            "VECTOR_RECOVERY_CODE",
            VECTOR_RECOVERY_CODE,
            &v.recovery_code,
        ),
        (
            "VECTOR_RECOVERY_SIGN",
            VECTOR_RECOVERY_SIGN,
            &v.recovery_sign,
        ),
        ("VECTOR_RECOVERY_KEM", VECTOR_RECOVERY_KEM, &v.recovery_kem),
    ];
    let mut wrong = Vec::new();
    for (name, expected, got) in pairs {
        if expected != got.as_str() {
            wrong.push(format!("const {name}: &str = \"{got}\";"));
        }
    }
    assert!(wrong.is_empty(), "vectors differ:\n{}", wrong.join("\n"));
}

#[test]
fn test_vectors_open_and_verify() {
    // The vectors are valid protocol data, not only stable bytes.
    let team = TeamId::new(VECTOR_TEAM).expect("id");
    let owner_holder = holder("owner", "laptop");
    let owner = DeviceIdentity::generate(&mut FixedEntropy::counting(0x00, 64)).expect("id");
    let mut verifier = LogVerifier::new(team.clone());
    let mut chain = MemberChain::new();
    for (index, text) in [(1, VECTOR_CREATE_ENTRY), (2, VECTOR_ROTATE_ENTRY)] {
        let append = MemberLogAppend {
            index,
            prev: chain.head().map(|h| h.hash),
            entry: Ciphertext::new(text).expect("ct"),
        };
        let hash = chain.append(&append).expect("chain");
        verifier
            .apply(&MemberLogEntry {
                index,
                hash,
                entry: append.entry,
                by: owner_holder.clone(),
                at: UnixMillis(0),
            })
            .expect("verify");
    }
    let state = verifier.state().expect("state");
    let secret = state
        .unwrap_key(
            &owner,
            &owner_holder,
            Epoch(1),
            &Ciphertext::new(VECTOR_WRAP).expect("ct"),
        )
        .expect("unwrap");
    assert_eq!(
        secret.commitment(&team, Epoch(1)),
        EpochCommitment(unhex(VECTOR_COMMITMENT).try_into().expect("32"))
    );
    let cipher = EpochCipher::new(&secret, &team, Epoch(1)).expect("keys");
    let opened = cipher
        .open(
            &RecordContext {
                kind: RecordKind::ReservationNote,
                record: b"task-hash",
                sender: &owner_holder,
            },
            &owner.public_keys().sign,
            &hangar_team_protocol::ids::Sealed {
                epoch: Epoch(1),
                data: Ciphertext::new(VECTOR_RECORD).expect("ct"),
            },
        )
        .expect("open");
    assert_eq!(opened.plaintext.as_slice(), b"hello team");
    let next = TeamSecret::from_bytes(Zeroizing::new(std::array::from_fn(|i| {
        0xa0u8.wrapping_add(i as u8)
    })));
    let link = HistoryLink::from_slice(&unhex(VECTOR_HISTORY)).expect("link");
    assert_eq!(
        link.open(&next, &team, Epoch(2), &secret.commitment(&team, Epoch(1)))
            .expect("history"),
        secret
    );
}

#[test]
fn hpke_matches_rfc_9180_a_2_1() {
    use crate::entropy::OneShotRng;
    use hpke::aead::ChaCha20Poly1305;
    use hpke::kdf::HkdfSha256;
    use hpke::kem::X25519HkdfSha256;
    use hpke::{Deserializable, Kem, OpModeS, Serializable};

    let pk_r = <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(&unhex(
        "4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a",
    ))
    .expect("pk");
    let ikm_e = unhex("909a9b35d3dc4713a5e72a4da274b55d3d3821a37e5d099e74a647db583a904b");
    let mut rng = OneShotRng::draw(&mut FixedEntropy::new(ikm_e)).expect("draw");
    let (enc, ct) =
        hpke::single_shot_seal_with_rng::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeS::Base,
            &pk_r,
            &unhex("4f6465206f6e2061204772656369616e2055726e"),
            &unhex("4265617574792069732074727574682c20747275746820626561757479"),
            &unhex("436f756e742d30"),
            &mut rng,
        )
        .expect("seal");
    assert!(rng.consumed_exactly_once());
    assert_eq!(
        hex(enc.to_bytes().as_slice()),
        "1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a"
    );
    assert_eq!(
        hex(&ct),
        concat!(
            "1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db",
            "21993c62ce81883d2dd1b51a28"
        )
    );
}

#[test]
fn xchacha20_poly1305_matches_the_cfrg_draft() {
    use chacha20poly1305::aead::{Aead, Payload};
    use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};

    let cipher = XChaCha20Poly1305::new_from_slice(&unhex(
        "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f",
    ))
    .expect("key");
    let nonce: [u8; 24] = unhex("404142434445464748494a4b4c4d4e4f5051525354555657")
        .try_into()
        .expect("24");
    let plaintext = unhex(concat!(
        "4c616469657320616e642047656e746c656d656e206f662074686520636c6173",
        "73206f66202739393a204966204920636f756c64206f6666657220796f75206f",
        "6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73",
        "637265656e20776f756c642062652069742e"
    ));
    let out = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plaintext,
                aad: &unhex("50515253c0c1c2c3c4c5c6c7"),
            },
        )
        .expect("encrypt");
    assert_eq!(
        hex(&out),
        concat!(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb",
            "731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452",
            "2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9",
            "21f9664c97637da9768812f615c68b13b52e",
            "c0875924c1c7987947deafd8780acf49"
        )
    );
}
