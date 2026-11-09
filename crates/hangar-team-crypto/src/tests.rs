//! Whole-team flows, the membership log rules (relay and client side), record sealing, and the
//! test vectors that `test_vectors.py` recomputes with the Python standard library.

use std::collections::BTreeMap;

use hangar_team_protocol::e2e::{entry_hash, ChainHead, LogHash, MemberChain, PublicKey};
use hangar_team_protocol::ids::{
    Ciphertext, DeviceId, Epoch, MemberId, Sealed, TeamId, UnixMillis,
};
use hangar_team_protocol::wire::{
    BlobKind, DeviceRemove, ErrorCode, Holder, KeyRotate, MemberLogAppend, MemberLogEntry, Role,
};
use zeroize::Zeroizing;

use crate::encoding::{from_ciphertext, to_ciphertext};
use crate::entropy::{FixedEntropy, OsEntropy};
use crate::epoch::{wrap, EpochCommitment, HistoryLink, TeamSecret, WrapError};
use crate::identity::{log_safety_number, safety_number, DeviceIdentity};
use crate::memberlog::{
    Admitted, DeviceEntry, Endorsement, EntryBody, EntryKind, LogError, LogVerifier, TeamState,
};
use crate::record::{
    CipherError, EpochCipher, OpenError, RecordContext, RecordKind, ReplayGuard, SealError,
    SendCounter,
};
use crate::recovery::RecoveryCode;
use crate::rotation::{plan_rotation, wrap_for_device, wrap_to_all, Rotation, RotationError};

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

    /// The device's entry with its proof of possession in `team-1`.
    fn entry(&self) -> DeviceEntry {
        DeviceEntry::new(&team_id(), &self.holder, &self.identity, false)
    }

    /// This device's endorsement of `new` (a device of the same member).
    fn endorse(&self, new: &DeviceEntry, member: &MemberId) -> Endorsement {
        Endorsement {
            device: self.holder.device.clone(),
            signature: self.identity.endorse(
                &team_id(),
                &Holder {
                    member: member.clone(),
                    device: new.device.clone(),
                },
                &new.keys,
                new.recovery,
            ),
        }
    }
}

/// A team with an owner, an admin and a member. Every entry goes through the relay's check
/// (`admit`) and, when it passes, into the stored log and a client's verifier. Both sides must
/// reach the same verdict.
struct Team {
    owner: Device,
    admin: Device,
    member: Device,
    /// The relay's stored log.
    entries: Vec<MemberLogEntry>,
    relay: LogVerifier,
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
            entries: Vec::new(),
            relay: LogVerifier::new(team_id()),
            verifier: LogVerifier::new(team_id()),
        };
        let create = EntryBody::create_team(
            team_id(),
            team.owner.holder.clone(),
            UnixMillis(1),
            vec![team.owner.entry()],
        );
        let owner_identity = DeviceIdentity::import(&team.owner.identity.export()).expect("copy");
        let owner_holder = team.owner.holder.clone();
        team.submit_signed(&create, &owner_identity, &owner_holder)
            .expect("create");
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
        let device = self.device(who);
        let (identity, by) = (
            DeviceIdentity::import(&device.identity.export()).expect("copy"),
            device.holder.clone(),
        );
        self.submit_as(&identity, &by, kind)
    }

    fn submit_as(
        &mut self,
        identity: &DeviceIdentity,
        by: &Holder,
        kind: EntryKind,
    ) -> Result<(), LogError> {
        let body = self.state().next_body(by.clone(), UnixMillis(2), kind);
        self.submit_signed(&body, identity, by)
    }

    /// The role on `by`'s ticket when the website and the log agree (the owner's for entry 1).
    fn ticket_role(&self, by: &Holder) -> Role {
        self.relay
            .state()
            .and_then(|state| state.member(&by.member))
            .map_or(Role::Owner, |member| member.role)
    }

    /// Signs without the sender-side check; the relay admits it or not, and a client given the
    /// same bytes agrees.
    fn submit_signed(
        &mut self,
        body: &EntryBody,
        identity: &DeviceIdentity,
        by: &Holder,
    ) -> Result<(), LogError> {
        let append = body.sign(identity)?;
        let entry = self.stored(&append, by);
        let mut client = self.verifier.clone();
        let client_verdict = client.apply(&entry);
        let ticket = self.ticket_role(by);
        let relay_verdict =
            relay_admit(&mut self.relay, body, &append, by, ticket).map(|admitted| {
                assert_eq!(
                    admitted.head.hash, entry.hash,
                    "relay and client hash alike"
                );
            });
        assert_eq!(relay_verdict, client_verdict, "relay and client agree");
        relay_verdict?;
        self.entries.push(entry);
        self.verifier = client;
        Ok(())
    }

    /// The entry as the relay would store it after `append` (hash over the stored chain).
    fn stored(&self, append: &MemberLogAppend, by: &Holder) -> MemberLogEntry {
        let head = self.entries.last().map(|e| ChainHead {
            index: e.index,
            hash: e.hash,
        });
        let hash = entry_hash(append.index, head.as_ref().map(|h| &h.hash), &append.entry)
            .expect("canonical");
        MemberLogEntry {
            index: append.index,
            hash,
            entry: append.entry.clone(),
            by: by.clone(),
            at: UnixMillis(0),
        }
    }

    /// Sends a planned rotation like `key.rotate`: the relay admits entry and keys together.
    fn rotate(&mut self, rotation: &Rotation, by: &Holder) -> Result<(), LogError> {
        let ticket = self.ticket_role(by);
        let admitted = self.relay.admit_rotation(&rotation.rotate, by, ticket)?;
        let entry = self.stored(&rotation.rotate.log, by);
        assert_eq!(admitted.head.hash, entry.hash);
        self.verifier.apply(&entry).expect("client accepts");
        self.entries.push(entry);
        Ok(())
    }

    fn plan(&self, who: Who, previous: Option<&TeamSecret>) -> Rotation {
        let device = self.device(who);
        plan_rotation(
            self.state(),
            &device.identity,
            &device.holder,
            previous,
            UnixMillis(3),
            &mut OsEntropy,
        )
        .expect("plan")
    }

    /// Plans and sends the next epoch, issued by `who`.
    fn next_epoch(&mut self, who: Who, previous: Option<&TeamSecret>) -> Rotation {
        let rotation = self.plan(who, previous);
        let by = self.device(who).holder.clone();
        self.rotate(&rotation, &by).expect("rotate");
        rotation
    }

    /// Adds `new` to `member`, issued by `who`, endorsed by `endorser`.
    fn add_device(&mut self, who: Who, new: &Device, endorser: &Device) -> Result<(), LogError> {
        let entry = new.entry();
        let endorsement = endorser.endorse(&entry, &new.holder.member);
        self.submit(
            who,
            EntryKind::AddDevice {
                member: new.holder.member.clone(),
                device: entry,
                endorsement,
            },
        )
    }
}

/// Sends a signed entry the way the app does: a remove device entry inside `device.remove`,
/// every other kind through `memberlog.append`.
fn relay_admit(
    relay: &mut LogVerifier,
    body: &EntryBody,
    append: &MemberLogAppend,
    by: &Holder,
    ticket: Role,
) -> Result<Admitted, LogError> {
    match &body.kind {
        EntryKind::RemoveDevice { holder } => relay.admit_device_remove(
            &DeviceRemove {
                holder: holder.clone(),
                log: append.clone(),
            },
            by,
            ticket,
        ),
        _ => relay.admit(append, by, ticket),
    }
}

#[derive(Clone, Copy)]
enum Who {
    Owner,
    Admin,
    Member,
}

fn seal_as(
    cipher: &EpochCipher,
    state: &TeamState,
    device: &Device,
    counter: u64,
    context: &RecordContext<'_>,
    plaintext: &[u8],
) -> Sealed {
    cipher
        .seal(
            context,
            &device.identity,
            state.head(),
            counter,
            plaintext,
            &mut OsEntropy,
        )
        .expect("seal")
}

#[test]
fn a_new_device_verifies_the_whole_log_into_the_same_state() {
    let team = Team::new();
    let mut fresh = LogVerifier::new(team_id());
    fresh.apply_all(&team.entries).expect("verify");
    assert_eq!(fresh.state(), team.verifier.state());
    assert_eq!(team.relay.state(), team.verifier.state());
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
    // The issuer is on record for the app's "added by" note.
    assert_eq!(
        state.added_by(&team.member.holder),
        Some(&team.admin.holder)
    );
    // Both sides compute the same log safety number, also at an earlier entry.
    assert_eq!(
        log_safety_number(&team_id(), state.head()),
        log_safety_number(&team_id(), team.state().head())
    );
    assert_eq!(
        state.log_safety_number_at(3),
        Some(log_safety_number(&team_id(), state.head()))
    );
    assert_ne!(state.log_safety_number_at(2), state.log_safety_number_at(3));
    assert_eq!(state.log_safety_number_at(4), None);
    assert_eq!(state.log_safety_number_at(0), None);
}

#[test]
fn rotation_reaches_every_device_and_records_open_across_them() {
    let mut team = Team::new();
    let rotation = team.next_epoch(Who::Admin, None);
    assert_eq!(rotation.epoch, Epoch(1));
    assert_eq!(rotation.rotate.wrapped.len(), 3);
    assert!(rotation.failed.is_empty());
    let state = team.state();
    assert_eq!(state.current_epoch().map(|e| e.epoch), Some(Epoch(1)));
    assert_eq!(
        state.current_epoch().map(|e| e.hash),
        state.hash_at(state.head().index)
    );

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

    // The member seals a note, the owner opens it against the verified log.
    let cipher = EpochCipher::for_epoch(state, &rotation.secret, Epoch(1)).expect("keys");
    let context = RecordContext {
        kind: RecordKind::ReservationNote,
        record: b"task hash",
        sender: &team.member.holder,
    };
    let mut counter = SendCounter::resume(0);
    let sealed = seal_as(
        &cipher,
        state,
        &team.member,
        counter.take().expect("counter"),
        &context,
        b"on it",
    );
    let opened = cipher
        .open_verified(state, &context, &sealed)
        .expect("open");
    assert_eq!(opened.plaintext.as_slice(), b"on it");
    assert_eq!(opened.counter, 1);
    assert_eq!(opened.head, *state.head());
    // The header names the same head and counter without a key (and without checking them).
    assert_eq!(
        crate::record::sealed_header(&sealed),
        Some((*state.head(), 1))
    );
    let garbage = Sealed {
        epoch: sealed.epoch,
        data: hangar_team_protocol::ids::Ciphertext::new("AAAA").expect("text"),
    };
    assert_eq!(crate::record::sealed_header(&garbage), None);
    assert_eq!(counter.last(), 1);
    let mut guard = ReplayGuard::new();
    guard.check(&opened).expect("first time");
    assert_eq!(guard.check(&opened), Err(OpenError::Replay));

    // A secret that is not the epoch's gives no cipher.
    let other = TeamSecret::generate(&mut OsEntropy).expect("rng");
    assert_eq!(
        EpochCipher::for_epoch(state, &other, Epoch(1)).err(),
        Some(CipherError::Commitment(1))
    );
    assert_eq!(
        EpochCipher::for_epoch(state, &rotation.secret, Epoch(2)).err(),
        Some(CipherError::UnknownEpoch(2))
    );
}

#[test]
fn replay_marks_survive_a_restart_and_refuse_older_counters() {
    let mut team = Team::new();
    let rotation = team.next_epoch(Who::Owner, None);
    let state = team.state();
    let cipher = EpochCipher::for_epoch(state, &rotation.secret, Epoch(1)).expect("keys");
    let context = RecordContext {
        kind: RecordKind::PresenceStatus,
        record: b"",
        sender: &team.member.holder,
    };
    let five = cipher
        .open_verified(
            state,
            &context,
            &seal_as(&cipher, state, &team.member, 5, &context, b"five"),
        )
        .expect("open");
    let three = cipher
        .open_verified(
            state,
            &context,
            &seal_as(&cipher, state, &team.member, 3, &context, b"three"),
        )
        .expect("open");
    let mut guard = ReplayGuard::new();
    guard.check(&five).expect("first");
    // An older record from the same sender, never seen before, is refused too.
    assert_eq!(guard.check(&three), Err(OpenError::Replay));
    // The app stores the marks; after a restart the replay is still refused.
    let stored: BTreeMap<(Holder, PublicKey), u64> = guard.marks().clone();
    assert_eq!(
        stored.get(&(
            team.member.holder.clone(),
            team.member.identity.public_keys().sign
        )),
        Some(&5)
    );
    let mut restarted = ReplayGuard::from_marks(stored);
    assert_eq!(restarted.check(&five), Err(OpenError::Replay));
    let six = cipher
        .open_verified(
            state,
            &context,
            &seal_as(&cipher, state, &team.member, 6, &context, b"six"),
        )
        .expect("open");
    restarted.check(&six).expect("newer");
    // Counters are per sender.
    let owner_context = RecordContext {
        sender: &team.owner.holder,
        ..context
    };
    let owner_one = cipher
        .open_verified(
            state,
            &owner_context,
            &seal_as(&cipher, state, &team.owner, 1, &owner_context, b"o"),
        )
        .expect("open");
    restarted.check(&owner_one).expect("other sender");
    // The counter is signed: changing it breaks the signature.
    let sealed = seal_as(&cipher, state, &team.member, 9, &context, b"x");
    let mut bytes = from_ciphertext(&sealed.data).expect("bytes");
    bytes[1 + 8 + 32 + 7] ^= 1;
    let changed = Sealed {
        epoch: sealed.epoch,
        data: to_ciphertext(&bytes).expect("ct"),
    };
    assert_eq!(
        cipher.open_verified(state, &context, &changed).err(),
        Some(OpenError::Signature)
    );
    let mut exhausted = SendCounter::resume(u64::MAX);
    assert_eq!(exhausted.take(), None);
}

#[test]
fn removal_makes_rotation_due_and_history_links_reach_back() {
    let mut team = Team::new();
    let first = team.next_epoch(Who::Owner, None);

    let member = team.member.holder.member.clone();
    team.submit(Who::Admin, EntryKind::RemoveMember { member })
        .expect("remove");
    assert!(team.state().rotation_due());
    assert!(team
        .state()
        .find_device(&team.member.identity.public_keys())
        .is_none());

    let second = team.next_epoch(Who::Admin, Some(&first.secret));
    assert_eq!(second.epoch, Epoch(2));
    // The removed member gets no key of the new epoch.
    assert_eq!(second.rotate.wrapped.len(), 2);
    assert!(second
        .rotate
        .wrapped
        .iter()
        .all(|w| w.holder != team.member.holder));
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
fn records_of_a_removed_device_open_only_for_heads_where_it_was_valid() {
    let mut team = Team::new();
    let rotation = team.next_epoch(Who::Owner, None);
    let cipher = EpochCipher::for_epoch(team.state(), &rotation.secret, Epoch(1)).expect("keys");
    let member_holder = team.member.holder.clone();
    let context = RecordContext {
        kind: RecordKind::ClaimDetail,
        record: b"session",
        sender: &member_holder,
    };
    // Sealed while the member was in the team.
    let before = {
        let state = team.state();
        seal_as(&cipher, state, &team.member, 1, &context, b"before")
    };
    let member = team.member.holder.member.clone();
    team.submit(Who::Owner, EntryKind::RemoveMember { member })
        .expect("remove");
    let state = team.state();
    // Its old record still opens: the key history keeps the removed keys.
    assert!(team.state().device(&team.member.holder).is_none());
    assert_eq!(state.key_periods(&team.member.holder).len(), 1);
    assert_eq!(
        cipher
            .open_verified(state, &context, &before)
            .expect("still valid")
            .plaintext
            .as_slice(),
        b"before"
    );
    // A record naming a head after its removal is refused.
    let after = seal_as(&cipher, state, &team.member, 2, &context, b"after");
    assert_eq!(
        cipher.open_verified(state, &context, &after).err(),
        Some(OpenError::SenderNotValid)
    );
    // A stranger the log never listed.
    let stranger = Device::new("stranger", "d");
    let as_stranger = RecordContext {
        sender: &stranger.holder,
        ..context
    };
    let sealed = seal_as(&cipher, state, &stranger, 1, &as_stranger, b"x");
    assert_eq!(
        cipher.open_verified(state, &as_stranger, &sealed).err(),
        Some(OpenError::UnknownSender)
    );
}

#[test]
fn record_heads_detect_forks_and_newer_logs() {
    let mut team = Team::new();
    let rotation = team.next_epoch(Who::Owner, None);
    let behind = team.state().clone();
    let cipher = EpochCipher::for_epoch(&behind, &rotation.secret, Epoch(1)).expect("keys");
    let admin_holder = team.admin.holder.clone();
    let context = RecordContext {
        kind: RecordKind::PresenceStatus,
        record: b"",
        sender: &admin_holder,
    };
    // The admin's log moves on; a device that is behind is told to fetch.
    let phone = Device::new("admin", "phone");
    let admin = Device {
        holder: team.admin.holder.clone(),
        identity: DeviceIdentity::import(&team.admin.identity.export()).expect("copy"),
    };
    team.add_device(Who::Admin, &phone, &admin).expect("add");
    let ahead = seal_as(&cipher, team.state(), &team.admin, 1, &context, b"x");
    assert_eq!(
        cipher.open_verified(&behind, &context, &ahead).err(),
        Some(OpenError::AheadOfLog { index: 5 })
    );
    assert!(cipher.open_verified(team.state(), &context, &ahead).is_ok());

    // A record naming another hash at a known index: the relay forked the log.
    let forged_head = ChainHead {
        index: 4,
        hash: LogHash::from_bytes([7; 32]),
    };
    let forked = cipher
        .seal(
            &context,
            &team.admin.identity,
            &forged_head,
            2,
            b"y",
            &mut OsEntropy,
        )
        .expect("seal");
    assert_eq!(
        cipher.open_verified(team.state(), &context, &forked).err(),
        Some(OpenError::Fork { index: 4 })
    );
    // The same wrong head without a valid signature is no fork: anyone can write a head, and
    // only a listed device's signature over it may make the app stop trusting the team.
    let unsigned = {
        let mut data = crate::encoding::from_ciphertext(&forked.data).expect("data");
        let last = data.len() - 1;
        data[last] ^= 1;
        Sealed {
            epoch: forked.epoch,
            data: crate::encoding::to_ciphertext(&data).expect("data"),
        }
    };
    assert_eq!(
        cipher
            .open_verified(team.state(), &context, &unsigned)
            .err(),
        Some(OpenError::Signature)
    );
    // Signed by a device the log never listed, or by a listed device under another's name.
    let stranger = Device::new("stranger", "d");
    let as_stranger = RecordContext {
        sender: &stranger.holder,
        ..context
    };
    let foreign_fork = cipher
        .seal(
            &as_stranger,
            &stranger.identity,
            &forged_head,
            1,
            b"y",
            &mut OsEntropy,
        )
        .expect("seal");
    assert_eq!(
        cipher
            .open_verified(team.state(), &as_stranger, &foreign_fork)
            .err(),
        Some(OpenError::UnknownSender)
    );
    let member_holder = team.member.holder.clone();
    let as_member = RecordContext {
        sender: &member_holder,
        ..context
    };
    let impersonated = cipher
        .seal(
            &as_member,
            &team.admin.identity,
            &forged_head,
            3,
            b"y",
            &mut OsEntropy,
        )
        .expect("seal");
    assert_eq!(
        cipher
            .open_verified(team.state(), &as_member, &impersonated)
            .err(),
        Some(OpenError::Signature)
    );
    // A head from before the epoch began.
    let old_head = ChainHead {
        index: 3,
        hash: team.state().hash_at(3).expect("hash"),
    };
    let stale = cipher
        .seal(
            &context,
            &team.admin.identity,
            &old_head,
            3,
            b"z",
            &mut OsEntropy,
        )
        .expect("seal");
    assert_eq!(
        cipher.open_verified(team.state(), &context, &stale).err(),
        Some(OpenError::HeadBeforeEpoch)
    );
    // A cipher built for another log's epoch (the same secret after a different rotate entry)
    // does not open against this log, and its records do not decrypt here.
    let foreign = EpochCipher::new(
        &rotation.secret,
        &team_id(),
        Epoch(1),
        &LogHash::from_bytes([9; 32]),
    )
    .expect("keys");
    assert_eq!(
        foreign.open_verified(team.state(), &context, &ahead).err(),
        Some(OpenError::UnknownEpoch)
    );
    let from_fork = seal_as(&foreign, team.state(), &team.admin, 4, &context, b"w");
    assert_eq!(
        cipher
            .open_verified(team.state(), &context, &from_fork)
            .err(),
        Some(OpenError::Decrypt)
    );
}

#[test]
fn a_new_device_gets_the_current_key_through_key_wrap() {
    let mut team = Team::new();
    let rotation = team.next_epoch(Who::Owner, None);

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
    // The member adds its own phone, endorsed by its laptop.
    let laptop = Device {
        holder: team.member.holder.clone(),
        identity: DeviceIdentity::import(&team.member.identity.export()).expect("copy"),
    };
    team.add_device(Who::Member, &phone, &laptop)
        .expect("add own device");
    assert_eq!(team.state().added_by(&phone.holder), Some(&laptop.holder));
    let wrap = wrap_for_device(
        team.state(),
        &rotation.secret,
        &phone.holder,
        &mut OsEntropy,
    )
    .expect("wrap");
    assert_eq!(wrap.epoch, Epoch(1));
    // Packages stored first by someone else do not block the real one.
    let garbage = TeamSecret::generate(&mut OsEntropy).expect("rng");
    let wrong = crate::epoch::wrap(
        &garbage,
        &team_id(),
        Epoch(1),
        &phone.holder,
        &phone.identity.public_keys().kem,
        &mut OsEntropy,
    )
    .expect("wrap");
    let secret = team
        .state()
        .unwrap_any(
            &phone.identity,
            &phone.holder,
            Epoch(1),
            [&wrong.data, &wrap.wrapped.data],
        )
        .expect("unwrap");
    assert_eq!(secret, rotation.secret);
    assert_eq!(
        team.state()
            .unwrap_any(&phone.identity, &phone.holder, Epoch(1), [&wrong.data]),
        Err(WrapError::Commitment(1))
    );
    // A secret that is not the current epoch's is refused.
    let other = TeamSecret::generate(&mut OsEntropy).expect("rng");
    assert_eq!(
        wrap_for_device(team.state(), &other, &phone.holder, &mut OsEntropy).err(),
        Some(WrapError::Commitment(1))
    );
}

#[test]
fn an_admin_cannot_add_a_device_to_another_member_alone() {
    let mut team = Team::new();
    let phone = Device::new("member", "phone");
    // Endorsed by the admin's own device instead of one of the member's.
    let entry = phone.entry();
    let endorsement = team.admin.endorse(&entry, &phone.holder.member);
    let result = team.submit(
        Who::Admin,
        EntryKind::AddDevice {
            member: phone.holder.member.clone(),
            device: entry.clone(),
            endorsement,
        },
    );
    assert_eq!(result, Err(LogError::Endorsement));
    // A forged signature in the member's device's name.
    let forged = Endorsement {
        device: team.member.holder.device.clone(),
        signature: team
            .admin
            .identity
            .endorse(&team_id(), &phone.holder, &entry.keys, false),
    };
    let result = team.submit(
        Who::Admin,
        EntryKind::AddDevice {
            member: phone.holder.member.clone(),
            device: entry,
            endorsement: forged,
        },
    );
    assert_eq!(result, Err(LogError::Endorsement));
    // With the member's consent the admin may add it.
    let laptop = Device {
        holder: team.member.holder.clone(),
        identity: DeviceIdentity::import(&team.member.identity.export()).expect("copy"),
    };
    team.add_device(Who::Admin, &phone, &laptop)
        .expect("endorsed by the member");
    let period = &team.state().key_periods(&phone.holder)[0];
    assert_eq!(period.added_by, team.admin.holder);
    assert_eq!(period.endorsed_by, Some(laptop.holder.device.clone()));
}

#[test]
fn a_device_without_proof_of_possession_is_refused() {
    let mut team = Team::new();
    let victim = Device::new("x", "y");
    // Someone else's keys with a proof the submitter made itself.
    let copycat = DeviceEntry {
        device: DeviceId::new("copy").expect("id"),
        keys: victim.identity.public_keys(),
        recovery: false,
        proof: Device::new("copycat", "copy").identity.prove(
            &team_id(),
            &holder("copycat", "copy"),
            false,
        ),
    };
    let result = team.submit(
        Who::Owner,
        EntryKind::AddMember {
            member: MemberId::new("copycat").expect("id"),
            role: Role::Member,
            devices: vec![copycat],
        },
    );
    assert_eq!(result, Err(LogError::Proof));
    // A proof made for another holder or another team.
    let mut moved = Device::new("newbie", "d").entry();
    moved.device = DeviceId::new("other").expect("id");
    let result = team.submit(
        Who::Owner,
        EntryKind::AddMember {
            member: MemberId::new("newbie").expect("id"),
            role: Role::Member,
            devices: vec![moved],
        },
    );
    assert_eq!(result, Err(LogError::Proof));
    let elsewhere = Device::new("other-team", "d");
    let foreign_proof = DeviceEntry::new(
        &TeamId::new("team-2").expect("id"),
        &elsewhere.holder,
        &elsewhere.identity,
        false,
    );
    let result = team.submit(
        Who::Owner,
        EntryKind::AddMember {
            member: elsewhere.holder.member.clone(),
            role: Role::Member,
            devices: vec![foreign_proof],
        },
    );
    assert_eq!(result, Err(LogError::Proof));
}

#[test]
fn members_manage_only_their_own_devices() {
    let mut team = Team::new();
    let phone = Device::new("member", "phone");
    let laptop = Device {
        holder: team.member.holder.clone(),
        identity: DeviceIdentity::import(&team.member.identity.export()).expect("copy"),
    };
    team.add_device(Who::Member, &phone, &laptop)
        .expect("own device");
    // The member removes its own other device (a lost phone, from the laptop).
    team.submit(
        Who::Member,
        EntryKind::RemoveDevice {
            holder: phone.holder.clone(),
        },
    )
    .expect("own other device");
    assert!(team.state().rotation_due());
    // But no other member's device, no member, no role, no rotation, never itself.
    let intruder = Device::new("admin", "second");
    for kind in [
        EntryKind::RemoveDevice {
            holder: team.admin.holder.clone(),
        },
        EntryKind::RemoveDevice {
            holder: team.member.holder.clone(),
        },
        EntryKind::RemoveMember {
            member: team.admin.holder.member.clone(),
        },
        EntryKind::AddMember {
            member: MemberId::new("friend").expect("id"),
            role: Role::Member,
            devices: vec![Device::new("friend", "d").entry()],
        },
        EntryKind::AddDevice {
            member: intruder.holder.member.clone(),
            device: intruder.entry(),
            endorsement: team
                .admin
                .endorse(&intruder.entry(), &intruder.holder.member),
        },
    ] {
        let body = team
            .state()
            .next_body(team.member.holder.clone(), UnixMillis(5), kind);
        assert_eq!(team.state().check(&body), Err(LogError::Forbidden));
    }
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
}

#[test]
fn the_relay_admits_only_what_the_sender_signed_and_rotations_only_with_keys() {
    let mut team = Team::new();
    // An entry signed by the admin but sent over the member's connection.
    let body = team.state().next_body(
        team.admin.holder.clone(),
        UnixMillis(5),
        EntryKind::RemoveMember {
            member: team.member.holder.member.clone(),
        },
    );
    let append = body.sign(&team.admin.identity).expect("sign");
    let refused = team.relay.admit(&append, &team.member.holder, Role::Member);
    assert_eq!(refused, Err(LogError::NotSender));
    assert_eq!(LogError::NotSender.code(), ErrorCode::Forbidden);

    // A rotate epoch entry through `memberlog.append` is refused: the keys must come with it.
    let rotation = team.plan(Who::Owner, None);
    assert_eq!(
        team.relay
            .admit(&rotation.rotate.log, &team.owner.holder, Role::Owner),
        Err(LogError::RotationOutsideKeyRotate)
    );
    // `key.rotate` with keys for a device the log does not list, or twice for one device.
    let mut stray = rotation.rotate.clone();
    stray.wrapped[0].holder = holder("ghost", "d");
    assert_eq!(
        team.relay
            .admit_rotation(&stray, &team.owner.holder, Role::Owner),
        Err(LogError::WrappedHolder)
    );
    let mut twice = rotation.rotate.clone();
    let first = twice.wrapped[0].clone();
    twice.wrapped.push(first);
    assert_eq!(
        team.relay
            .admit_rotation(&twice, &team.owner.holder, Role::Owner),
        Err(LogError::WrappedHolder)
    );
    // An epoch that does not match the entry's, or another kind of entry.
    let other_epoch = KeyRotate {
        epoch: Epoch(2),
        ..rotation.rotate.clone()
    };
    assert_eq!(
        team.relay
            .admit_rotation(&other_epoch, &team.owner.holder, Role::Owner),
        Err(LogError::NotARotation)
    );
    let not_rotate = KeyRotate {
        log: append.clone(),
        ..rotation.rotate.clone()
    };
    assert_eq!(
        team.relay
            .admit_rotation(&not_rotate, &team.admin.holder, Role::Admin),
        Err(LogError::NotARotation)
    );
    // Nothing was admitted so far; the real rotation goes through as one unit.
    assert_eq!(team.relay.state(), team.verifier.state());
    team.rotate(&rotation, &team.owner.holder.clone())
        .expect("rotate");
    assert_eq!(team.relay.state(), team.verifier.state());
    // A second rotation for the same epoch now loses the race.
    let late = KeyRotate {
        ..rotation.rotate.clone()
    };
    assert!(matches!(
        team.relay
            .admit_rotation(&late, &team.owner.holder, Role::Owner),
        Err(LogError::Chain(_))
    ));
    // Removals tell the relay whom to disconnect.
    let body = team.state().next_body(
        team.admin.holder.clone(),
        UnixMillis(6),
        EntryKind::RemoveMember {
            member: team.member.holder.member.clone(),
        },
    );
    let append = body.sign(&team.admin.identity).expect("sign");
    let admitted = team
        .relay
        .admit(&append, &team.admin.holder, Role::Admin)
        .expect("admit");
    assert_eq!(admitted.removed, vec![team.member.holder.clone()]);
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
    // The owner demotes the admin; then the former admin may not remove members.
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
fn keys_are_vouched_for_once_and_must_be_valid() {
    let mut team = Team::new();
    // The member's keys again, for a new member (with a proof the copier cannot make, so the
    // reuse check is what refuses it first).
    let reused = DeviceEntry {
        device: DeviceId::new("x").expect("id"),
        ..team.member.entry()
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
    // An invalid signing key, an X25519 key of small order, and one that is not canonical.
    let mut identity_point = [0u8; 32];
    identity_point[0] = 1;
    let mut not_canonical = [0xff; 32];
    not_canonical[31] = 0x7f;
    for (sign, kem) in [
        (Some(identity_point), None),
        (None, Some([0u8; 32])),
        (None, Some(not_canonical)),
    ] {
        let mut weak = Device::new("weak", "d").entry();
        if let Some(sign) = sign {
            weak.keys.sign = PublicKey::from_bytes(sign);
        }
        if let Some(kem) = kem {
            weak.keys.kem = PublicKey::from_bytes(kem);
        }
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
}

#[test]
fn entries_are_bound_to_team_position_and_signer() {
    let team = Team::new();
    // Another team's verifier refuses the same bytes.
    let mut other = LogVerifier::new(TeamId::new("team-2").expect("id"));
    assert_eq!(other.apply(&team.entries[0]), Err(LogError::TeamMismatch));

    // A relay splicing entry 3 in as entry 2 (recomputing the unkeyed chain hash) is caught by
    // the signed index.
    let mut verifier = LogVerifier::new(team_id());
    verifier.apply(&team.entries[0]).expect("create");
    let spliced = MemberLogEntry {
        index: 2,
        hash: entry_hash(2, Some(&team.entries[0].hash), &team.entries[2].entry).expect("hash"),
        ..team.entries[2].clone()
    };
    assert_eq!(verifier.apply(&spliced), Err(LogError::IndexMismatch));

    // A forged signature: flip one bit of the signature.
    let mut bytes = from_ciphertext(&team.entries[1].entry).expect("bytes");
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let forged = to_ciphertext(&bytes).expect("ct");
    let entry = MemberLogEntry {
        index: 2,
        hash: entry_hash(2, Some(&team.entries[0].hash), &forged).expect("hash"),
        entry: forged,
        ..team.entries[1].clone()
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
        hash: entry_hash(2, Some(&team.entries[0].hash), &append.entry).expect("hash"),
        entry: append.entry,
        by: stranger.holder,
        at: UnixMillis(0),
    };
    assert_eq!(verifier.apply(&entry), Err(LogError::UnknownIssuer));
    // After all refusals the verifier still accepts the real entry 2.
    verifier.apply(&team.entries[1]).expect("entry 2");
    // The log stays stopped at a refused entry: the next one leaves a gap.
    let gap = MemberLogEntry {
        index: 4,
        ..team.entries[2].clone()
    };
    assert!(matches!(verifier.apply(&gap), Err(LogError::Chain(_))));
}

#[test]
fn the_first_entry_must_create_the_team_and_sign_itself() {
    let owner = Device::new("owner", "laptop");
    let other = Device::new("owner", "other");
    let apply = |append: &MemberLogAppend, by: &Holder| {
        let mut chain = MemberChain::new();
        let hash = chain.append(append).expect("chain");
        let entry = MemberLogEntry {
            index: 1,
            hash,
            entry: append.entry.clone(),
            by: by.clone(),
            at: UnixMillis(0),
        };
        let client = LogVerifier::new(team_id()).apply(&entry);
        let relay = LogVerifier::new(team_id())
            .admit(append, by, Role::Owner)
            .map(|_| ());
        assert_eq!(client, relay);
        client
    };
    // Signed by a device the entry does not list.
    let body = EntryBody::create_team(
        team_id(),
        other.holder.clone(),
        UnixMillis(1),
        vec![owner.entry()],
    );
    let append = body.sign(&other.identity).expect("sign");
    assert_eq!(apply(&append, &other.holder), Err(LogError::UnknownIssuer));
    // Lists the device but signed by another key.
    let body = EntryBody::create_team(
        team_id(),
        owner.holder.clone(),
        UnixMillis(1),
        vec![owner.entry()],
    );
    let append = body.sign(&other.identity).expect("sign");
    assert_eq!(apply(&append, &owner.holder), Err(LogError::Signature));
    // Starts with something else.
    let body = EntryBody {
        kind: EntryKind::RemoveMember {
            member: owner.holder.member.clone(),
        },
        ..EntryBody::create_team(team_id(), owner.holder.clone(), UnixMillis(1), vec![])
    };
    let append = body.sign(&owner.identity).expect("sign");
    assert_eq!(apply(&append, &owner.holder), Err(LogError::NotCreated));
}

#[test]
fn rotation_rules_hold() {
    let team = Team::new();
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
    let outcome = wrap_to_all(team.state(), &secret, Epoch(1), &mut OsEntropy).expect("wrap");
    assert_eq!(outcome.wrapped.len(), 3);
    assert!(outcome.failed.is_empty());
    // Without randomness each device fails on its own, none aborts the others.
    let outcome = wrap_to_all(
        team.state(),
        &secret,
        Epoch(1),
        &mut FixedEntropy::counting(0, 64),
    )
    .expect("wrap");
    assert_eq!(outcome.wrapped.len(), 2);
    assert_eq!(outcome.failed.len(), 1);
    assert!(matches!(outcome.failed[0].1, WrapError::Entropy(_)));
}

#[test]
fn records_are_bound_to_their_context() {
    let secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
    let alice = Device::new("alice", "a");
    let bob = Device::new("bob", "b");
    let rotation = LogHash::from_bytes([1; 32]);
    let head = ChainHead {
        index: 4,
        hash: LogHash::from_bytes([2; 32]),
    };
    let cipher = EpochCipher::new(&secret, &team_id(), Epoch(4), &rotation).expect("keys");
    let context = RecordContext {
        kind: RecordKind::ClaimDetail,
        record: b"session-1",
        sender: &alice.holder,
    };
    let seal = |cipher: &EpochCipher, context: &RecordContext<'_>, plaintext: &[u8]| {
        cipher
            .seal(
                context,
                &alice.identity,
                &head,
                1,
                plaintext,
                &mut OsEntropy,
            )
            .expect("seal")
    };
    let sealed = seal(&cipher, &context, b"src/main.rs");
    let key = alice.identity.public_keys().sign;
    let opened = cipher.open(&context, &key, &sealed).expect("open");
    assert_eq!(opened.sender, alice.holder);
    assert_eq!(opened.head, head);

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
    let later = EpochCipher::new(&secret, &team_id(), Epoch(5), &rotation).expect("keys");
    assert_eq!(
        later.open(&context, &key, &sealed).err(),
        Some(OpenError::EpochMismatch {
            expected: 5,
            got: 4
        })
    );
    // Another team with the same secret and epoch: the signature covers the team.
    let elsewhere = EpochCipher::new(
        &secret,
        &TeamId::new("team-2").expect("id"),
        Epoch(4),
        &rotation,
    )
    .expect("keys");
    assert_eq!(
        elsewhere.open(&context, &key, &sealed).err(),
        Some(OpenError::Signature)
    );
    // A member who signs correctly but seals under a different key.
    let wrong_secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
    let wrong = EpochCipher::new(&wrong_secret, &team_id(), Epoch(4), &rotation).expect("keys");
    let foreign = seal(&wrong, &context, b"x");
    assert_eq!(
        cipher.open(&context, &key, &foreign).err(),
        Some(OpenError::Decrypt)
    );
    // Malformed and versioned data.
    let short = Sealed {
        epoch: Epoch(4),
        data: to_ciphertext(&[1, 2, 3]).expect("ct"),
    };
    assert_eq!(
        cipher.open(&context, &key, &short).err(),
        Some(OpenError::Malformed)
    );
    let mut bytes = from_ciphertext(&sealed.data).expect("bytes");
    bytes[0] = 7;
    let versioned = Sealed {
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
    let cipher = EpochCipher::new(&secret, &team_id(), Epoch(1), &LogHash::from_bytes([1; 32]))
        .expect("keys");
    let head = ChainHead {
        index: 1,
        hash: LogHash::from_bytes([1; 32]),
    };
    let context = RecordContext {
        kind: RecordKind::PresenceStatus,
        record: b"",
        sender: &alice.holder,
    };
    let max = RecordKind::PresenceStatus.max_plaintext();
    let fits = cipher
        .seal(
            &context,
            &alice.identity,
            &head,
            1,
            &vec![0u8; max],
            &mut OsEntropy,
        )
        .expect("fits");
    assert!(fits.data.as_str().len() <= RecordKind::PresenceStatus.max_chars());
    assert_eq!(
        cipher
            .seal(
                &context,
                &alice.identity,
                &head,
                1,
                &vec![0u8; max + 3],
                &mut OsEntropy
            )
            .err(),
        Some(SealError::TooLarge)
    );
    assert!(RecordKind::Blob(BlobKind::Transcript).max_plaintext() > 100_000);
}

#[test]
fn a_recovery_code_restores_its_device_and_does_nothing_else() {
    let mut team = Team::new();
    let code = RecoveryCode::generate(&mut OsEntropy).expect("rng");
    let recovery = code.identity().expect("identity");
    let recovery_holder = holder("member", "recovery-1");
    let entry = DeviceEntry::new(&team_id(), &recovery_holder, &recovery, true);
    let endorsement = team.member.endorse(&entry, &recovery_holder.member);
    team.submit(
        Who::Member,
        EntryKind::AddDevice {
            member: recovery_holder.member.clone(),
            device: entry,
            endorsement,
        },
    )
    .expect("add recovery device");
    let rotation = team.next_epoch(Who::Owner, None);

    // On a new installation: type the code, find the device, open its wrapped key.
    let typed = code.reveal().to_lowercase();
    let restored = RecoveryCode::parse(&typed)
        .expect("parse")
        .identity()
        .expect("identity");
    let found = team
        .state()
        .find_device(&restored.public_keys())
        .expect("vouched for");
    assert_eq!(found, recovery_holder);
    assert_eq!(team.state().device(&found).map(|d| d.recovery), Some(true));
    let wrapped = rotation
        .rotate
        .wrapped
        .iter()
        .find(|w| w.holder == found)
        .expect("wrapped to the recovery device");
    let secret = team
        .state()
        .unwrap_key(&restored, &found, Epoch(1), &wrapped.data)
        .expect("unwrap");
    assert_eq!(secret, rotation.secret);

    // The recovery device may not touch anyone else or any other kind of entry.
    for kind in [
        EntryKind::RemoveMember {
            member: team.admin.holder.member.clone(),
        },
        EntryKind::RemoveDevice {
            holder: team.admin.holder.clone(),
        },
        EntryKind::ChangeRole {
            member: team.admin.holder.member.clone(),
            role: Role::Member,
        },
    ] {
        let body = team
            .state()
            .next_body(recovery_holder.clone(), UnixMillis(8), kind);
        assert_eq!(team.state().check(&body), Err(LogError::Forbidden));
    }
    // Records it signs are refused.
    let cipher = EpochCipher::for_epoch(team.state(), &secret, Epoch(1)).expect("keys");
    let context = RecordContext {
        kind: RecordKind::PresenceStatus,
        record: b"",
        sender: &recovery_holder,
    };
    let sealed = cipher
        .seal(
            &context,
            &restored,
            team.state().head(),
            1,
            b"hi",
            &mut OsEntropy,
        )
        .expect("seal");
    assert_eq!(
        cipher.open_verified(team.state(), &context, &sealed).err(),
        Some(OpenError::RecoverySender)
    );

    // It adds the new installation to its own member, which then removes it.
    let fresh = Device::new("member", "new-laptop");
    let fresh_entry = fresh.entry();
    let endorsement = Endorsement {
        device: recovery_holder.device.clone(),
        signature: restored.endorse(&team_id(), &fresh.holder, &fresh_entry.keys, false),
    };
    team.submit_as(
        &restored,
        &recovery_holder,
        EntryKind::AddDevice {
            member: fresh.holder.member.clone(),
            device: fresh_entry,
            endorsement,
        },
    )
    .expect("recovery adds the new device");
    team.submit_as(
        &fresh.identity,
        &fresh.holder,
        EntryKind::RemoveDevice {
            holder: recovery_holder.clone(),
        },
    )
    .expect("the new device removes the used recovery device");
    assert!(team.state().rotation_due());
}

#[test]
fn entry_decoding_is_strict() {
    let team = Team::new();
    let bytes = from_ciphertext(&team.entries[1].entry).expect("bytes");
    let body = &bytes[..bytes.len() - 64];
    assert!(EntryBody::decode(body).is_ok());
    let mut longer = body.to_vec();
    longer.extend_from_slice(&[0, 0, 0, 0]);
    assert_eq!(EntryBody::decode(&longer), Err(LogError::Malformed));
    assert_eq!(EntryBody::decode(&body[..40]), Err(LogError::Malformed));
}

#[test]
fn log_errors_map_to_relay_codes() {
    use hangar_team_protocol::e2e::ChainError;
    assert_eq!(
        LogError::Chain(ChainError::Taken { index: 2 }).code(),
        ErrorCode::Conflict
    );
    assert_eq!(LogError::Forbidden.code(), ErrorCode::Forbidden);
    assert_eq!(LogError::Signature.code(), ErrorCode::Forbidden);
    assert_eq!(
        LogError::EpochOrder {
            expected: 2,
            got: 3
        }
        .code(),
        ErrorCode::StaleEpoch
    );
    assert_eq!(LogError::Proof.code(), ErrorCode::BadRequest);
    assert_eq!(
        LogError::RotationOutsideKeyRotate.code(),
        ErrorCode::BadRequest
    );
    assert_eq!(LogError::TooLarge.code(), ErrorCode::Limit);
}

/// A copy of `device` (same holder, same keys), so a test can endorse with it while `team` is
/// borrowed mutably.
fn copy(device: &Device) -> Device {
    Device {
        holder: device.holder.clone(),
        identity: DeviceIdentity::import(&device.identity.export()).expect("copy"),
    }
}

/// The relay's verdict on `kind`, issued by `by` with `ticket` as its ticket role, on a copy of
/// the relay's verifier (nothing is stored).
fn relay_verdict(team: &Team, by: &Device, kind: EntryKind, ticket: Role) -> Result<(), LogError> {
    let body = team
        .state()
        .next_body(by.holder.clone(), UnixMillis(8), kind);
    let append = body.sign(&by.identity)?;
    let mut relay = team.relay.clone();
    relay_admit(&mut relay, &body, &append, &by.holder, ticket).map(|_| ())
}

#[test]
fn the_ticket_role_caps_what_the_relay_admits() {
    // Entry 1 needs an owner's ticket.
    let owner = Device::new("owner", "laptop");
    let create = EntryBody::create_team(
        team_id(),
        owner.holder.clone(),
        UnixMillis(1),
        vec![owner.entry()],
    );
    let append = create.sign(&owner.identity).expect("sign");
    for ticket in [Role::Member, Role::Admin] {
        assert_eq!(
            LogVerifier::new(team_id())
                .admit(&append, &owner.holder, ticket)
                .map(|_| ()),
            Err(LogError::Forbidden),
            "{ticket:?}"
        );
    }
    let mut relay = LogVerifier::new(team_id());
    relay
        .admit(&append, &owner.holder, Role::Owner)
        .expect("owner creates");

    let team = Team::new();
    let newcomer = || EntryKind::AddMember {
        member: MemberId::new("newcomer").expect("id"),
        role: Role::Member,
        devices: vec![Device::new("newcomer", "d").entry()],
    };
    // An admin of the log whom the website downgraded to member adds no one, removes no one and
    // rotates nothing; the clients' log alone would still accept it.
    let admin = copy(&team.admin);
    let body = team
        .state()
        .next_body(admin.holder.clone(), UnixMillis(8), newcomer());
    assert_eq!(team.state().check(&body), Ok(()));
    assert_eq!(
        relay_verdict(&team, &admin, newcomer(), Role::Member),
        Err(LogError::Forbidden)
    );
    assert_eq!(
        relay_verdict(
            &team,
            &admin,
            EntryKind::RemoveMember {
                member: team.member.holder.member.clone()
            },
            Role::Member
        ),
        Err(LogError::Forbidden)
    );
    assert_eq!(
        relay_verdict(
            &team,
            &admin,
            EntryKind::RemoveDevice {
                holder: team.member.holder.clone()
            },
            Role::Member
        ),
        Err(LogError::Forbidden)
    );
    assert_eq!(
        relay_verdict(&team, &admin, newcomer(), Role::Admin),
        Ok(())
    );
    let rotation = team.plan(Who::Admin, None);
    assert_eq!(
        team.relay
            .clone()
            .admit_rotation(&rotation.rotate, &admin.holder, Role::Member)
            .map(|_| ()),
        Err(LogError::Forbidden)
    );
    // An owner whose ticket says admin acts as an admin: no role changes, no new admins.
    let owner = copy(&team.owner);
    for kind in [
        EntryKind::ChangeRole {
            member: team.member.holder.member.clone(),
            role: Role::Admin,
        },
        EntryKind::AddMember {
            member: MemberId::new("deputy").expect("id"),
            role: Role::Admin,
            devices: vec![Device::new("deputy", "d").entry()],
        },
    ] {
        assert_eq!(
            relay_verdict(&team, &owner, kind, Role::Admin),
            Err(LogError::Forbidden)
        );
    }
    // A ticket never raises a role: a member with an owner's ticket is still a member.
    let member = copy(&team.member);
    assert_eq!(
        relay_verdict(&team, &member, newcomer(), Role::Owner),
        Err(LogError::Forbidden)
    );
    // What any member may do stays allowed: its own new device.
    let phone = Device::new("admin", "phone");
    let entry = phone.entry();
    let endorsement = admin.endorse(&entry, &phone.holder.member);
    assert_eq!(
        relay_verdict(
            &team,
            &admin,
            EntryKind::AddDevice {
                member: phone.holder.member.clone(),
                device: entry,
                endorsement,
            },
            Role::Member
        ),
        Ok(())
    );
}

#[test]
fn a_member_has_at_most_sixteen_devices() {
    use hangar_team_protocol::limits::MAX_DEVICES_PER_MEMBER;
    let mut team = Team::new();
    let laptop = copy(&team.member);
    for i in 1..MAX_DEVICES_PER_MEMBER {
        let device = Device::new("member", &format!("d{i}"));
        team.add_device(Who::Member, &device, &laptop)
            .expect("room left");
    }
    let member = team.member.holder.member.clone();
    assert_eq!(
        team.state().member(&member).map(|m| m.devices.len()),
        Some(MAX_DEVICES_PER_MEMBER)
    );
    let extra = Device::new("member", "d17");
    let error = team
        .add_device(Who::Member, &extra, &laptop)
        .expect_err("17 devices");
    assert_eq!(error, LogError::TooManyDevices);
    assert_eq!(error.code(), ErrorCode::Limit);
    // A new member's first devices are bound the same way.
    let devices: Vec<DeviceEntry> = (0..=MAX_DEVICES_PER_MEMBER)
        .map(|i| Device::new("crowd", &format!("d{i}")).entry())
        .collect();
    let body = team.state().next_body(
        team.owner.holder.clone(),
        UnixMillis(8),
        EntryKind::AddMember {
            member: MemberId::new("crowd").expect("id"),
            role: Role::Member,
            devices,
        },
    );
    assert_eq!(team.state().check(&body), Err(LogError::TooManyDevices));
    assert_eq!(
        body.sign(&team.owner.identity).err(),
        Some(LogError::TooManyDevices)
    );
}

#[test]
fn a_team_has_at_most_as_many_devices_as_one_rotation_wraps() {
    use hangar_team_protocol::limits::{MAX_DEVICES_PER_MEMBER, MAX_WRAPPED_KEYS};
    let mut team = Team::new();
    let mut i = 0;
    while team.state().device_count() < MAX_WRAPPED_KEYS {
        let room = MAX_WRAPPED_KEYS - team.state().device_count();
        let name = format!("m{i}");
        let devices = (0..room.min(MAX_DEVICES_PER_MEMBER))
            .map(|d| Device::new(&name, &format!("d{d}")).entry())
            .collect();
        team.submit(
            Who::Owner,
            EntryKind::AddMember {
                member: MemberId::new(name.as_str()).expect("id"),
                role: Role::Member,
                devices,
            },
        )
        .expect("room left");
        i += 1;
    }
    assert_eq!(team.state().device_count(), MAX_WRAPPED_KEYS);
    // Device 513, as a new member or as another device of a member with room of its own.
    let late = Device::new("late", "d");
    assert_eq!(
        team.submit(
            Who::Owner,
            EntryKind::AddMember {
                member: late.holder.member.clone(),
                role: Role::Member,
                devices: vec![late.entry()],
            },
        ),
        Err(LogError::TooManyDevices)
    );
    let laptop = copy(&team.member);
    assert_eq!(
        team.add_device(Who::Member, &Device::new("member", "phone"), &laptop),
        Err(LogError::TooManyDevices)
    );
    // Removing one makes room again.
    let removed = holder("m0", "d0");
    team.submit(Who::Owner, EntryKind::RemoveDevice { holder: removed })
        .expect("remove");
    team.add_device(Who::Member, &Device::new("member", "phone"), &laptop)
        .expect("room again");
}

#[test]
fn key_wrap_needs_devices_of_the_log_and_keeps_one_package_per_sender() {
    use crate::relay::{check_wrap, KeyWrapError};
    use hangar_team_protocol::limits::MAX_KEY_PACKAGES;
    use hangar_team_protocol::wire::{KeyWrap, WrappedKey};

    let mut team = Team::new();
    let phone = Device::new("member", "phone");
    let laptop = copy(&team.member);
    team.add_device(Who::Member, &phone, &laptop)
        .expect("phone");
    let state = team.state().clone();
    let wrap = |target: &Holder| KeyWrap {
        epoch: Epoch(1),
        wrapped: WrappedKey {
            holder: target.clone(),
            data: Ciphertext::new("AAAA").expect("ct"),
        },
    };
    let (owner, admin, member) = (
        team.owner.holder.clone(),
        team.admin.holder.clone(),
        team.member.holder.clone(),
    );
    // Owners and admins wrap for any other device of the log; a member for its own other one.
    for sender in [&owner, &admin] {
        let role = state.member(&sender.member).expect("member").role;
        assert_eq!(
            check_wrap(&state, role, sender, &wrap(&phone.holder), &[]),
            Ok(())
        );
    }
    assert_eq!(
        check_wrap(&state, Role::Member, &member, &wrap(&phone.holder), &[]),
        Ok(())
    );
    for (sender, ticket, target) in [
        // Not another member's device, not itself.
        (&member, Role::Member, &admin),
        (&member, Role::Member, &member),
        (&owner, Role::Owner, &owner),
        // A ticket never raises the log's role, and a downgrade applies at once.
        (&member, Role::Owner, &admin),
        (&admin, Role::Member, &owner),
    ] {
        let error = check_wrap(&state, ticket, sender, &wrap(target), &[]).expect_err("refused");
        assert_eq!(error, KeyWrapError::Forbidden);
        assert_eq!(error.code(), ErrorCode::Forbidden);
    }
    // A sender the log does not list, even with an owner's ticket.
    let ghost = holder("owner", "ghost");
    assert_eq!(
        check_wrap(&state, Role::Owner, &ghost, &wrap(&phone.holder), &[]),
        Err(KeyWrapError::Forbidden)
    );
    // A target the log does not list.
    let error = check_wrap(&state, Role::Owner, &owner, &wrap(&ghost), &[]).expect_err("unknown");
    assert_eq!(error, KeyWrapError::UnknownDevice);
    assert_eq!(error.code(), ErrorCode::NotFound);
    // One package per sender, target and epoch.
    let error = check_wrap(
        &state,
        Role::Owner,
        &owner,
        &wrap(&phone.holder),
        std::slice::from_ref(&owner),
    )
    .expect_err("duplicate");
    assert_eq!(error, KeyWrapError::Duplicate);
    assert_eq!(error.code(), ErrorCode::Conflict);
    // Up to four per target, from different senders.
    let others: Vec<Holder> = (0..MAX_KEY_PACKAGES)
        .map(|i| holder("x", &format!("d{i}")))
        .collect();
    assert_eq!(
        check_wrap(
            &state,
            Role::Owner,
            &owner,
            &wrap(&phone.holder),
            &others[..MAX_KEY_PACKAGES - 1]
        ),
        Ok(())
    );
    let error =
        check_wrap(&state, Role::Owner, &owner, &wrap(&phone.holder), &others).expect_err("full");
    assert_eq!(error, KeyWrapError::Limit);
    assert_eq!(error.code(), ErrorCode::Limit);
    // Rights come first: a refused sender does not learn how many packages are stored.
    assert_eq!(
        check_wrap(&state, Role::Member, &member, &wrap(&admin), &others),
        Err(KeyWrapError::Forbidden)
    );
    // A removed device wraps nothing.
    team.submit(
        Who::Owner,
        EntryKind::RemoveDevice {
            holder: phone.holder.clone(),
        },
    )
    .expect("remove");
    assert_eq!(
        check_wrap(
            team.state(),
            Role::Member,
            &phone.holder,
            &wrap(&member),
            &[]
        ),
        Err(KeyWrapError::Forbidden)
    );
}

#[test]
fn device_remove_carries_its_log_entry() {
    let mut team = Team::new();
    let phone = Device::new("member", "phone");
    let laptop = copy(&team.member);
    team.add_device(Who::Member, &phone, &laptop)
        .expect("phone");
    let body = team.state().next_body(
        team.member.holder.clone(),
        UnixMillis(8),
        EntryKind::RemoveDevice {
            holder: phone.holder.clone(),
        },
    );
    let append = body.sign(&team.member.identity).expect("sign");
    let by = team.member.holder.clone();
    // Through `memberlog.append` the removal is refused: the relay would not remove the device.
    let error = team
        .relay
        .admit(&append, &by, Role::Member)
        .expect_err("outside device.remove");
    assert_eq!(error, LogError::RemovalOutsideDeviceRemove);
    assert_eq!(error.code(), ErrorCode::BadRequest);
    // A `device.remove` naming another device than its entry, or carrying another entry.
    let other = DeviceRemove {
        holder: team.admin.holder.clone(),
        log: append.clone(),
    };
    assert_eq!(
        team.relay.admit_device_remove(&other, &by, Role::Member),
        Err(LogError::NotADeviceRemoval)
    );
    let tablet = Device::new("member", "tablet");
    let tablet_entry = tablet.entry();
    let endorsement = laptop.endorse(&tablet_entry, &by.member);
    let add = team
        .state()
        .next_body(
            by.clone(),
            UnixMillis(8),
            EntryKind::AddDevice {
                member: by.member.clone(),
                device: tablet_entry,
                endorsement,
            },
        )
        .sign(&team.member.identity)
        .expect("sign");
    let not_removal = DeviceRemove {
        holder: phone.holder.clone(),
        log: add,
    };
    assert_eq!(
        team.relay
            .admit_device_remove(&not_removal, &by, Role::Member)
            .map(|_| ()),
        Err(LogError::NotADeviceRemoval)
    );
    // Nothing changed; the real `device.remove` goes through and names the device to drop.
    assert_eq!(team.relay.state(), team.verifier.state());
    let remove = DeviceRemove {
        holder: phone.holder.clone(),
        log: append.clone(),
    };
    let admitted = team
        .relay
        .admit_device_remove(&remove, &by, Role::Member)
        .expect("admitted");
    assert_eq!(admitted.removed, vec![phone.holder.clone()]);
    team.verifier
        .apply(&team.stored(&append, &by))
        .expect("client agrees");
    assert_eq!(team.relay.state(), team.verifier.state());
    assert!(team.state().device(&phone.holder).is_none());
}

#[test]
fn a_readmitted_member_shows_and_its_reused_device_id_starts_a_fresh_counter() {
    let mut team = Team::new();
    let member_id = team.member.holder.member.clone();
    let first = team.state().member(&member_id).expect("member").clone();
    assert_eq!(first.added_at, 3);
    assert_eq!(first.added_by, team.admin.holder);
    assert_eq!(first.readmitted_after, None);
    let owner_entry = team
        .state()
        .member(&team.owner.holder.member)
        .expect("owner");
    assert_eq!(
        (owner_entry.added_at, &owner_entry.added_by),
        (1, &team.owner.holder)
    );

    let rotation = team.next_epoch(Who::Owner, None);
    let cipher = EpochCipher::for_epoch(team.state(), &rotation.secret, Epoch(1)).expect("keys");
    let member_holder = team.member.holder.clone();
    let context = RecordContext {
        kind: RecordKind::PresenceStatus,
        record: b"",
        sender: &member_holder,
    };
    let mut guard = ReplayGuard::new();
    let old = cipher
        .open_verified(
            team.state(),
            &context,
            &seal_as(&cipher, team.state(), &team.member, 5, &context, b"old"),
        )
        .expect("open");
    guard.check(&old).expect("first");

    // Removed, then the website hands the same member id out again, with the same device id
    // and new keys.
    team.submit(
        Who::Owner,
        EntryKind::RemoveMember {
            member: member_id.clone(),
        },
    )
    .expect("remove");
    let removed_at = team.state().head().index;
    // Before it is added again, the app can tell that confirming would re-admit it.
    assert!(team.state().member(&member_id).is_none());
    assert_eq!(team.state().departed_at(&member_id), Some(removed_at));
    let again = Device::new("member", "laptop");
    team.submit(
        Who::Owner,
        EntryKind::AddMember {
            member: member_id.clone(),
            role: Role::Member,
            devices: vec![again.entry()],
        },
    )
    .expect("re-add");
    let readmitted = team.state().member(&member_id).expect("member");
    assert_eq!(readmitted.readmitted_after, Some(removed_at));
    assert_eq!(readmitted.added_at, removed_at + 1);
    assert_eq!(readmitted.added_by, team.owner.holder);
    assert_eq!(team.state().key_periods(&member_holder).len(), 2);

    // The re-added device numbers its records from 1 again; the old mark does not refuse them.
    let rotation = team.next_epoch(Who::Owner, Some(&rotation.secret));
    let cipher = EpochCipher::for_epoch(team.state(), &rotation.secret, Epoch(2)).expect("keys");
    let new = cipher
        .open_verified(
            team.state(),
            &context,
            &seal_as(&cipher, team.state(), &again, 1, &context, b"new"),
        )
        .expect("open");
    assert_eq!(new.sign, again.identity.public_keys().sign);
    guard.check(&new).expect("fresh mark for the new keys");
    assert_eq!(guard.check(&new), Err(OpenError::Replay));
    // The old keys keep their own mark.
    assert_eq!(guard.check(&old), Err(OpenError::Replay));
    assert_eq!(guard.marks().len(), 2);

    // Whoever holds the old signing key cannot make the honest device look forked: a head of
    // the old key period with a wrong hash ends before the epoch, never as a fork.
    let forged = ChainHead {
        index: 3,
        hash: LogHash::from_bytes([7; 32]),
    };
    let old_key = cipher
        .seal(
            &context,
            &team.member.identity,
            &forged,
            u64::MAX - 1,
            b"forked",
            &mut OsEntropy,
        )
        .expect("seal");
    assert_eq!(
        cipher.open_verified(team.state(), &context, &old_key).err(),
        Some(OpenError::HeadBeforeEpoch)
    );
}

#[test]
fn entries_list_only_keys_their_devices_registered() {
    use crate::identity::DevicePublicKeys;
    use crate::relay::check_registered_keys;

    let team = Team::new();
    let phone = Device::new("member", "phone");
    let laptop = copy(&team.member);
    let entry = phone.entry();
    let endorsement = laptop.endorse(&entry, &phone.holder.member);
    let append = team
        .state()
        .next_body(
            laptop.holder.clone(),
            UnixMillis(8),
            EntryKind::AddDevice {
                member: phone.holder.member.clone(),
                device: entry,
                endorsement,
            },
        )
        .sign(&laptop.identity)
        .expect("sign");
    let phone_keys = phone.identity.public_keys();
    let registered = |keys: Option<DevicePublicKeys>| {
        let phone_holder = phone.holder.clone();
        move |holder: &Holder| keys.filter(|_| *holder == phone_holder)
    };
    assert_eq!(
        check_registered_keys(&append, registered(Some(phone_keys))),
        Ok(())
    );
    // Other keys than the device sent over its own connection, or none at all.
    let other = Device::new("x", "y").identity.public_keys();
    for keys in [Some(other), None] {
        let error = check_registered_keys(&append, registered(keys)).expect_err("refused");
        assert_eq!(error, LogError::KeysNotRegistered);
        assert_eq!(error.code(), ErrorCode::Forbidden);
    }
    // A new member's devices are checked the same way; recovery devices register nothing.
    let newcomer = Device::new("newcomer", "d");
    let code = RecoveryCode::generate(&mut OsEntropy).expect("code");
    let recovery_holder = holder("newcomer", "recovery");
    let recovery = DeviceEntry::new(
        &team_id(),
        &recovery_holder,
        &code.identity().expect("identity"),
        true,
    );
    let append = team
        .state()
        .next_body(
            team.owner.holder.clone(),
            UnixMillis(8),
            EntryKind::AddMember {
                member: newcomer.holder.member.clone(),
                role: Role::Member,
                devices: vec![newcomer.entry(), recovery],
            },
        )
        .sign(&team.owner.identity)
        .expect("sign");
    let newcomer_keys = newcomer.identity.public_keys();
    let newcomer_holder = newcomer.holder.clone();
    assert_eq!(
        check_registered_keys(&append, |holder: &Holder| {
            (*holder == newcomer_holder).then_some(newcomer_keys)
        }),
        Ok(())
    );
    assert_eq!(
        check_registered_keys(&append, |_: &Holder| None),
        Err(LogError::KeysNotRegistered)
    );
    // Entries that add no device need nothing; garbage is malformed.
    let remove = team
        .state()
        .next_body(
            team.owner.holder.clone(),
            UnixMillis(8),
            EntryKind::RemoveMember {
                member: team.member.holder.member.clone(),
            },
        )
        .sign(&team.owner.identity)
        .expect("sign");
    assert_eq!(check_registered_keys(&remove, |_: &Holder| None), Ok(()));
    let garbage = MemberLogAppend {
        entry: Ciphertext::new("AAAA").expect("ct"),
        ..remove
    };
    assert_eq!(
        check_registered_keys(&garbage, |_: &Holder| None),
        Err(LogError::Malformed)
    );
}

// ---------------------------------------------------------------------------------------------
// Test vectors. `test_vectors.py` reads the `VECTOR_*` constants of this file and recomputes
// each one with the Python standard library (its own Ed25519, X25519, ChaCha20-Poly1305 and
// HPKE, each checked first against its RFC vector).
// ---------------------------------------------------------------------------------------------

const VECTOR_TEAM: &str = "team-vectors";
const VECTOR_ISSUED_AT: u64 = 1_700_000_000_000;
const VECTOR_COUNTER: u64 = 7;

const VECTOR_OWNER_SIGN: &str = "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8";
const VECTOR_OWNER_KEM: &str = "358072d6365880d1aeea329adf9121383851ed21a28e3b75e965d0d2cd166254";
const VECTOR_OWNER_SAFETY: &str = "66163 27494 60846 17528 53184 02098";
const VECTOR_CREATE_ENTRY: &str =
    "AAAAHmhhbmdhci10ZWFtL3YxL21lbWJlcmxvZy1lbnRyeQAAAAEBAAAADHRlYW0tdmVjdG9ycwAAAAgAAAAAAAAAAQAAAAAAAAAIAAABi8_laAAAAAAFb3duZXIAAAAGbGFwdG9wAAAAAQEAAAAEAAAAAQAAAAZsYXB0b3AAAAAgA6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbgAAAAgNYBy1jZYgNGu6jKa35EhODhR7SGijjt16WXQ0s0WYlQAAAABAAAAAEA3eRwEdPCh8MjR7B0ZvLvI3Mk0cGFseb-C0m1BrDbxy1Y5U-2Uo-RGXVDyw28fttWdiIC_XCfSL3hdFoYy2k0HARvzoARfD650Wwkm6M8t4YPfnlfH-Jjz8TaA73sKv1-F5s1Ln3dOvpliOvlhxhf9mrCkS8OSsUvdXkct2qWqAw";
const VECTOR_CREATE_HASH: &str = "363dc8f8148101f1779b175a1ca45696b60c29f04601f19ad7fd0754fafc277c";
const VECTOR_COMMITMENT: &str = "697f8ef1cb94e90a5a84e4b4f01af743f6cf2578ad55f3abb6e905145c0f6446";
const VECTOR_ROTATE_ENTRY: &str =
    "AAAAHmhhbmdhci10ZWFtL3YxL21lbWJlcmxvZy1lbnRyeQAAAAEBAAAADHRlYW0tdmVjdG9ycwAAAAgAAAAAAAAAAgAAACA2Pcj4FIEB8XebF1ocpFaWtgwp8EYB8ZrX_QdU-vwnfAAAAAgAAAGLz-VoAQAAAAVvd25lcgAAAAZsYXB0b3AAAAABBwAAAAQAAAABAAAAIGl_jvHLlOkKWoTktPAa90P2zyV4rVXzq7bpBRRcD2RGAAAAADANWZ0G8QKhHvAkf29Om86rVmTUye76_RRX-Zt2mLER_3dQSyUGwqyT4HD00aENQZ__YyCbxpuWzwwkkbxL9Qc";
const VECTOR_ROTATE_HASH: &str = "a91afd3d4220714a27d71a43c87d0314a8eeb52f30665494bf11a171c0bb3f2c";
const VECTOR_ADD_DEVICE_ENTRY: &str =
    "AAAAHmhhbmdhci10ZWFtL3YxL21lbWJlcmxvZy1lbnRyeQAAAAEBAAAADHRlYW0tdmVjdG9ycwAAAAgAAAAAAAAAAwAAACCpGv09QiBxSifXGkPIfQMUqO61LzBmVJS_EaFxwLs_LAAAAAgAAAGLz-VoAgAAAAVvd25lcgAAAAZsYXB0b3AAAAABAwAAAAVvd25lcgAAAAVwaG9uZQAAACAXRVO0Vt3fxpCOyrHBAf5qsh4rqgYXeVt9Q6Y0gpk_1QAAACBJPoL8dEZKWSaIF2I9IFPF644sxKmItP7heexrAQ1THQAAAAEAAAAAQLAk5KI__NQozEtYjZug0GWL7Vjhd4edTXG4DtfXwE3CiICrmLgWJOwAtwTXqmw2KmmnSbi221u9YX9B0Cye1w4AAAAGbGFwdG9wAAAAQHGQjax68UfKrmjMUzxXs_g9e8GYjFbFwBvbExuwQeZSCxu-LfmwtaMcNwO5reCLdUFYI_DcaOUVxduZIGVksQHTrNo8AYZsHEygHKQ8pB0u2EP998flukmkJbL91TRR4zJzLqBI1WrJTIjL_ncsYv2Nh3EUL7yoaHKkDXlRTSYB";
const VECTOR_ADD_DEVICE_HASH: &str =
    "8805717a3ade262a476f3aa9e1909f05768c61fab1fe80d7fd7fa14a48b3d00d";
const VECTOR_WRAP: &str =
    "AXrwPfFZ4tdXUcGojrWp6HmI8Tjc51luvaOtfwuwSoc0R1CLX4p-JJW5dm65asMN3vtTv6m5QoiDiOaFrm3qE479oj9ZzlxLDaYWu8RAkSwO";
const VECTOR_HISTORY: &str =
    "01c0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7727996fdc22aa37d1a982b95a5d86bc3f93ed801bb75bb8647c4f177a22aacb1855b87285b9c5f587de1060daf284b54";
const VECTOR_RECORD: &str =
    "AQAAAAAAAAADiAVxejreJipHbzqp4ZCfBXaMYfqx_oDX_X-hSkiz0A0AAAAAAAAAB-Dh4uPk5ebn6Onq6-zt7u_w8fLz9PX298VmEyuqm_tphctEJ9ZVlHSCUAXEQD7gjN0sDQbgfbWxMC6O0_ne7CbbHRAmtsUeIknkx2chX7iIHKwrvnEdS4oyBln0vVJIpbB9-XM1MR9pOxEC9--q2YrODA";
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
    rotate_hash: String,
    add_device_entry: String,
    add_device_hash: String,
    wrap: String,
    history: String,
    record: String,
    recovery_code: String,
    recovery_sign: String,
    recovery_kem: String,
}

fn vector_owner() -> (Holder, DeviceIdentity) {
    (
        holder("owner", "laptop"),
        DeviceIdentity::generate(&mut FixedEntropy::counting(0x00, 64)).expect("id"),
    )
}

fn vector_phone() -> (Holder, DeviceIdentity) {
    (
        holder("owner", "phone"),
        DeviceIdentity::generate(&mut FixedEntropy::counting(0x60, 64)).expect("id"),
    )
}

fn vector_record_context(sender: &Holder) -> RecordContext<'_> {
    RecordContext {
        kind: RecordKind::ReservationNote,
        record: b"task-hash",
        sender,
    }
}

fn compute_vectors() -> Vectors {
    let team = TeamId::new(VECTOR_TEAM).expect("id");
    let (owner_holder, owner) = vector_owner();
    let keys = owner.public_keys();

    let create = EntryBody::create_team(
        team.clone(),
        owner_holder.clone(),
        UnixMillis(VECTOR_ISSUED_AT),
        vec![DeviceEntry::new(&team, &owner_holder, &owner, false)],
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
    let rotate_hash = entry_hash(2, Some(&create_hash), &rotate_append.entry).expect("hash");

    let (phone_holder, phone) = vector_phone();
    let phone_entry = DeviceEntry::new(&team, &phone_holder, &phone, false);
    let add = EntryBody {
        team: team.clone(),
        index: 3,
        prev: Some(rotate_hash),
        issued_at: UnixMillis(VECTOR_ISSUED_AT + 2),
        issuer: owner_holder.clone(),
        kind: EntryKind::AddDevice {
            member: owner_holder.member.clone(),
            endorsement: Endorsement {
                device: owner_holder.device.clone(),
                signature: owner.endorse(&team, &phone_holder, &phone_entry.keys, false),
            },
            device: phone_entry,
        },
    };
    let add_append = add.sign(&owner).expect("sign");
    let add_hash = entry_hash(3, Some(&rotate_hash), &add_append.entry).expect("hash");

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

    let cipher = EpochCipher::new(&secret, &team, Epoch(1), &rotate_hash).expect("keys");
    let record = cipher
        .seal(
            &vector_record_context(&owner_holder),
            &owner,
            &ChainHead {
                index: 3,
                hash: add_hash,
            },
            VECTOR_COUNTER,
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
        rotate_hash: rotate_hash.to_hex(),
        add_device_entry: add_append.entry.as_str().to_owned(),
        add_device_hash: add_hash.to_hex(),
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
        ("VECTOR_ROTATE_HASH", VECTOR_ROTATE_HASH, &v.rotate_hash),
        (
            "VECTOR_ADD_DEVICE_ENTRY",
            VECTOR_ADD_DEVICE_ENTRY,
            &v.add_device_entry,
        ),
        (
            "VECTOR_ADD_DEVICE_HASH",
            VECTOR_ADD_DEVICE_HASH,
            &v.add_device_hash,
        ),
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
    // The vectors are valid protocol data, not only stable bytes: the relay admits them (the
    // rotation together with its wrapped key) and a client verifies them.
    let team = TeamId::new(VECTOR_TEAM).expect("id");
    let (owner_holder, owner) = vector_owner();
    let mut verifier = LogVerifier::new(team.clone());
    let mut relay = LogVerifier::new(team.clone());
    let mut chain = MemberChain::new();
    for (index, text) in [
        (1, VECTOR_CREATE_ENTRY),
        (2, VECTOR_ROTATE_ENTRY),
        (3, VECTOR_ADD_DEVICE_ENTRY),
    ] {
        let append = MemberLogAppend {
            index,
            prev: chain.head().map(|h| h.hash),
            entry: Ciphertext::new(text).expect("ct"),
        };
        let hash = chain.append(&append).expect("chain");
        if index == 2 {
            let rotate = KeyRotate {
                epoch: Epoch(1),
                log: append.clone(),
                wrapped: vec![hangar_team_protocol::wire::WrappedKey {
                    holder: owner_holder.clone(),
                    data: Ciphertext::new(VECTOR_WRAP).expect("ct"),
                }],
            };
            relay
                .admit_rotation(&rotate, &owner_holder, Role::Owner)
                .expect("relay admits");
        } else {
            relay
                .admit(&append, &owner_holder, Role::Owner)
                .expect("relay admits");
        }
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
    assert_eq!(
        state.hash_at(2).map(|h| h.to_hex()).as_deref(),
        Some(VECTOR_ROTATE_HASH)
    );
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
    let cipher = EpochCipher::for_epoch(state, &secret, Epoch(1)).expect("keys");
    let opened = cipher
        .open_verified(
            state,
            &vector_record_context(&owner_holder),
            &Sealed {
                epoch: Epoch(1),
                data: Ciphertext::new(VECTOR_RECORD).expect("ct"),
            },
        )
        .expect("open");
    assert_eq!(opened.plaintext.as_slice(), b"hello team");
    assert_eq!(opened.counter, VECTOR_COUNTER);
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

#[test]
fn a_task_note_is_named_by_the_hash_as_lowercase_hex_and_opens_only_under_it() {
    use crate::record::task_record_id;
    use hangar_team_protocol::hash::KeyedHash;
    let task = KeyedHash::from_bytes([0xAB; 32]);
    let id = task_record_id(&task);
    assert_eq!(id, "ab".repeat(32));
    assert_eq!(id, task.to_hex(), "the spelling the wire carries");
    assert_ne!(task_record_id(&KeyedHash::from_bytes([0xAC; 32])), id);
}
