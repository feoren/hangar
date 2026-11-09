//! The signed membership log: which members and devices belong to a team, with which roles and
//! public keys, and which epoch secrets the team uses. Clients accept a key only when this log
//! vouches for it; the relay's `devices` list is a directory, not a trust anchor.
//!
//! # Entries are signed plaintext, not sealed
//!
//! A new device reads and verifies the log before it has any team key (it learns from the log
//! which keys to trust and which epoch secret to accept), so the log cannot be sealed with a team
//! key. What it shows the relay, it already knows: member ids and roles come from the website's
//! tickets, device ids and public keys from `device.keys`, epochs from `key.rotate`. It adds who
//! approved whom and when. The epoch commitments are hashes of 256-bit random secrets and reveal
//! nothing usable; the history links are AEAD ciphertexts.
//!
//! # Entry format (version 1)
//!
//! ```text
//! entry = body || Ed25519 signature of the issuing device over body (64 bytes)
//! body  = field("hangar-team/v1/memberlog-entry") || field(version = 1) || field(team)
//!         || field(index, u64) || field(prev hash, 32 bytes or empty) || field(issued_at, u64 ms)
//!         || field(issuer member) || field(issuer device) || field(kind tag) || kind fields
//! device = field(device id) || field(sign key) || field(kem key) || field(recovery: 0 or 1)
//! devices = field(count, u32) || device ...
//! role = 1 owner, 2 admin, 3 member
//! ```
//!
//! | Tag | Kind | Fields |
//! | --- | --- | --- |
//! | 1 | create team | devices of the issuing owner |
//! | 2 | add member | member, role, devices |
//! | 3 | add device | member, device |
//! | 4 | remove device | member, device id |
//! | 5 | remove member | member |
//! | 6 | change role | member, role |
//! | 7 | rotate epoch | epoch (u32), commitment (32 bytes), history link (73 bytes or empty) |
//!
//! Integers are big endian. The body names the team (the chain hash does not, see
//! `hangar_team_protocol::e2e`), its own index and its predecessor's hash: a validly signed entry
//! fits only one position of one team's log, so a relay can neither move it to another team nor
//! splice it into a different history. Decoding is strict (unknown tags, out-of-range bytes,
//! trailing bytes refused) and re-encoding must give the same bytes.
//!
//! # Rules
//!
//! - Entry 1 creates the team: its issuer becomes the owner, and the issuing device must be among
//!   the listed devices (the entry signs itself). Every later entry is refused if it creates.
//! - Every later entry is issued by a device the log lists, of an owner or admin (the relay
//!   allows `memberlog.append` only for them as well).
//! - The owner manages everyone; an admin manages members and its own devices. No one removes
//!   the owner, removes itself, or removes the issuing device; only the owner changes roles, and no
//!   entry makes a second owner (ownership transfer is not part of version 1).
//! - A public key is accepted once per team and never again, even after its device was removed.
//! - Removing a device or a member marks a rotation as due; a rotate epoch entry clears it. Epochs
//!   count from 1 without gaps.
//! - `issued_at` is the issuer's clock, informational only; the order is the log's.

use std::collections::{BTreeMap, BTreeSet};

use hangar_team_protocol::e2e::{ChainError, ChainHead, LogHash, MemberChain, PublicKey};
use hangar_team_protocol::ids::{Ciphertext, DeviceId, Epoch, MemberId, TeamId, UnixMillis};
use hangar_team_protocol::limits::MAX_INLINE_CIPHERTEXT_LEN;
use hangar_team_protocol::wire::{Holder, MemberLogAppend, MemberLogEntry, Role};
use thiserror::Error;

use crate::encoding::{base64url_len, from_ciphertext, to_ciphertext, Reader, Writer};
use crate::epoch::{self, EpochCommitment, HistoryLink, TeamSecret, WrapError};
use crate::identity::{is_valid_sign_key, verify, DeviceIdentity, DevicePublicKeys};

const LABEL_ENTRY: &[u8] = b"hangar-team/v1/memberlog-entry";
/// The format version of new entries.
pub const ENTRY_VERSION: u8 = 1;
/// Devices one entry may list.
pub const MAX_DEVICES_PER_ENTRY: usize = 32;
const SIGNATURE_LEN: usize = 64;

const TAG_CREATE: u8 = 1;
const TAG_ADD_MEMBER: u8 = 2;
const TAG_ADD_DEVICE: u8 = 3;
const TAG_REMOVE_DEVICE: u8 = 4;
const TAG_REMOVE_MEMBER: u8 = 5;
const TAG_CHANGE_ROLE: u8 = 6;
const TAG_ROTATE: u8 = 7;

/// Why a membership log entry was refused. The log stops at a refused entry: nothing after it
/// is accepted (fail closed), and the app shows the team as untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LogError {
    /// The hash chain (index, `prev`, stated hash) does not continue.
    #[error(transparent)]
    Chain(#[from] ChainError),
    /// Not an entry of this format (encoding, field sizes, unknown tag, trailing bytes).
    #[error("the membership log entry is malformed")]
    Malformed,
    /// An entry format version this crate does not know.
    #[error("membership log entry version {0} is not supported")]
    Version(u8),
    /// The signed body names another team.
    #[error("the membership log entry belongs to another team")]
    TeamMismatch,
    /// The signed body names another index than its position.
    #[error("the membership log entry was signed for another position")]
    IndexMismatch,
    /// The signed body names another predecessor than the verified one.
    #[error("the membership log entry was signed after another entry")]
    PrevMismatch,
    /// The first entry does not create the team.
    #[error("the membership log does not start by creating the team")]
    NotCreated,
    /// A later entry tries to create the team again.
    #[error("the team was already created")]
    AlreadyCreated,
    /// The issuing device is not in the log.
    #[error("the membership log does not vouch for the issuing device")]
    UnknownIssuer,
    /// The signature does not verify with the issuing device's key.
    #[error("the membership log entry's signature does not verify")]
    Signature,
    /// The issuer's role does not allow this entry.
    #[error("the issuer may not make this change")]
    Forbidden,
    /// The entry names a member the log does not list.
    #[error("the member is not in the team")]
    UnknownMember,
    /// The entry names a device the log does not list.
    #[error("the device is not in the team")]
    UnknownDevice,
    /// The member is already in the team.
    #[error("the member is already in the team")]
    MemberExists,
    /// The member already has a device with that id.
    #[error("the member already has a device with that id")]
    DeviceExists,
    /// A public key the log accepted before (also for a removed device).
    #[error("a public key in the entry was used before")]
    KeyReused,
    /// An Ed25519 key that is not a valid point or of small order.
    #[error("a signing key in the entry is not a valid Ed25519 key")]
    InvalidKey,
    /// No device where at least one is needed.
    #[error("the entry lists no device")]
    NoDevices,
    /// More than [`MAX_DEVICES_PER_ENTRY`] devices.
    #[error("the entry lists too many devices")]
    TooManyDevices,
    /// A rotation that does not name the next epoch.
    #[error("the rotation names epoch {got}, expected {expected}")]
    EpochOrder {
        /// The epoch that comes next in the log.
        expected: u32,
        /// The epoch the entry names.
        got: u32,
    },
    /// A history link in the first epoch, which has no predecessor.
    #[error("the first epoch has no previous secret to link")]
    NoPreviousEpoch,
    /// A role change to the role the member already has.
    #[error("the member already has that role")]
    NoChange,
    /// The signed entry does not fit `memberlog.append` (16 KiB of base64url).
    #[error("the membership log entry is too large")]
    TooLarge,
}

/// A device as the log lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEntry {
    /// The device's id, unique within its member.
    pub device: DeviceId,
    /// Its public keys, accepted once per team.
    pub keys: DevicePublicKeys,
    /// An offline device derived from a printed recovery code (see [`crate::recovery`]).
    pub recovery: bool,
}

/// What an entry changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// Entry 1: the issuer becomes the owner with these devices (the issuing one among them).
    CreateTeam {
        /// The owner's first devices.
        devices: Vec<DeviceEntry>,
    },
    /// A new member with its first devices.
    AddMember {
        /// The new member.
        member: MemberId,
        /// Admin (only the owner grants it) or member, never owner.
        role: Role,
        /// Its first devices.
        devices: Vec<DeviceEntry>,
    },
    /// Another device of a member (approved after comparing its safety number).
    AddDevice {
        /// The member who gets the device.
        member: MemberId,
        /// The new device.
        device: DeviceEntry,
    },
    /// A lost or retired device. Makes a rotation due.
    RemoveDevice {
        /// The device to remove; never the issuing one.
        holder: Holder,
    },
    /// A member who left or was removed, with all devices. Makes a rotation due.
    RemoveMember {
        /// The member to remove; never the owner or the issuer.
        member: MemberId,
    },
    /// A new role (only the owner, never to owner).
    ChangeRole {
        /// The member whose role changes; never the owner.
        member: MemberId,
        /// Admin or member.
        role: Role,
    },
    /// A new epoch: its number, the commitment to its secret and, when history is granted, the
    /// previous secret encrypted under the new one.
    RotateEpoch {
        /// The next epoch (the first is 1).
        epoch: Epoch,
        /// The commitment to the epoch's secret.
        commitment: EpochCommitment,
        /// The previous secret under the new one; `None` cuts history off.
        history: Option<HistoryLink>,
    },
}

impl EntryKind {
    fn tag(&self) -> u8 {
        match self {
            EntryKind::CreateTeam { .. } => TAG_CREATE,
            EntryKind::AddMember { .. } => TAG_ADD_MEMBER,
            EntryKind::AddDevice { .. } => TAG_ADD_DEVICE,
            EntryKind::RemoveDevice { .. } => TAG_REMOVE_DEVICE,
            EntryKind::RemoveMember { .. } => TAG_REMOVE_MEMBER,
            EntryKind::ChangeRole { .. } => TAG_CHANGE_ROLE,
            EntryKind::RotateEpoch { .. } => TAG_ROTATE,
        }
    }
}

/// The signed content of one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryBody {
    /// The team whose log this entry belongs to.
    pub team: TeamId,
    /// The entry's position in the log, from 1.
    pub index: u64,
    /// The chain hash of the previous entry; `None` only for entry 1.
    pub prev: Option<LogHash>,
    /// The issuer's clock; informational only.
    pub issued_at: UnixMillis,
    /// The device that signs the entry.
    pub issuer: Holder,
    /// What the entry changes.
    pub kind: EntryKind,
}

fn role_byte(role: Role) -> u8 {
    match role {
        Role::Owner => 1,
        Role::Admin => 2,
        Role::Member => 3,
    }
}

fn role_from(byte: u8) -> Option<Role> {
    match byte {
        1 => Some(Role::Owner),
        2 => Some(Role::Admin),
        3 => Some(Role::Member),
        _ => None,
    }
}

fn write_device(w: &mut Writer, device: &DeviceEntry) {
    w.field(device.device.as_str().as_bytes())
        .field(device.keys.sign.as_bytes())
        .field(device.keys.kem.as_bytes())
        .u8(u8::from(device.recovery));
}

fn write_devices(w: &mut Writer, devices: &[DeviceEntry]) -> Result<(), LogError> {
    if devices.len() > MAX_DEVICES_PER_ENTRY {
        return Err(LogError::TooManyDevices);
    }
    w.u32(u32::try_from(devices.len()).map_err(|_| LogError::TooManyDevices)?);
    for device in devices {
        write_device(w, device);
    }
    Ok(())
}

fn read_device(r: &mut Reader<'_>) -> Option<DeviceEntry> {
    let device = DeviceId::new(r.text()?).ok()?;
    let sign = PublicKey::from_bytes(r.fixed()?);
    let kem = PublicKey::from_bytes(r.fixed()?);
    let recovery = match r.u8()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(DeviceEntry {
        device,
        keys: DevicePublicKeys { sign, kem },
        recovery,
    })
}

fn read_devices(r: &mut Reader<'_>) -> Option<Vec<DeviceEntry>> {
    let count = usize::try_from(r.u32()?).ok()?;
    if count > MAX_DEVICES_PER_ENTRY {
        return None;
    }
    (0..count).map(|_| read_device(r)).collect()
}

fn read_member(r: &mut Reader<'_>) -> Option<MemberId> {
    MemberId::new(r.text()?).ok()
}

impl EntryBody {
    /// The first entry of a new team, issued by its owner's device `issuer`.
    pub fn create_team(
        team: TeamId,
        issuer: Holder,
        issued_at: UnixMillis,
        devices: Vec<DeviceEntry>,
    ) -> Self {
        Self {
            team,
            index: 1,
            prev: None,
            issued_at,
            issuer,
            kind: EntryKind::CreateTeam { devices },
        }
    }

    /// The canonical bytes that are signed.
    pub fn encode(&self) -> Result<Vec<u8>, LogError> {
        let mut w = Writer::new();
        w.field(LABEL_ENTRY)
            .u8(ENTRY_VERSION)
            .field(self.team.as_str().as_bytes())
            .u64(self.index)
            .field(self.prev.as_ref().map_or(&[][..], |p| &p.as_bytes()[..]))
            .u64(self.issued_at.0)
            .field(self.issuer.member.as_str().as_bytes())
            .field(self.issuer.device.as_str().as_bytes())
            .u8(self.kind.tag());
        match &self.kind {
            EntryKind::CreateTeam { devices } => write_devices(&mut w, devices)?,
            EntryKind::AddMember {
                member,
                role,
                devices,
            } => {
                w.field(member.as_str().as_bytes()).u8(role_byte(*role));
                write_devices(&mut w, devices)?;
            }
            EntryKind::AddDevice { member, device } => {
                w.field(member.as_str().as_bytes());
                write_device(&mut w, device);
            }
            EntryKind::RemoveDevice { holder } => {
                w.field(holder.member.as_str().as_bytes())
                    .field(holder.device.as_str().as_bytes());
            }
            EntryKind::RemoveMember { member } => {
                w.field(member.as_str().as_bytes());
            }
            EntryKind::ChangeRole { member, role } => {
                w.field(member.as_str().as_bytes()).u8(role_byte(*role));
            }
            EntryKind::RotateEpoch {
                epoch,
                commitment,
                history,
            } => {
                w.u32(epoch.0)
                    .field(&commitment.0)
                    .field(history.as_ref().map_or(&[][..], |h| &h.as_bytes()[..]));
            }
        }
        Ok(w.into_bytes())
    }

    /// Parses the signed bytes; refuses anything that does not re-encode to the same bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, LogError> {
        let mut r = Reader::new(bytes);
        if r.field() != Some(LABEL_ENTRY) {
            return Err(LogError::Malformed);
        }
        let version = r.u8().ok_or(LogError::Malformed)?;
        if version != ENTRY_VERSION {
            return Err(LogError::Version(version));
        }
        let body = Self::decode_v1(r).ok_or(LogError::Malformed)?;
        if body.encode()? != bytes {
            return Err(LogError::Malformed);
        }
        Ok(body)
    }

    fn decode_v1(mut r: Reader<'_>) -> Option<Self> {
        let team = TeamId::new(r.text()?).ok()?;
        let index = r.u64()?;
        let prev = match r.field()? {
            [] => None,
            hash => Some(LogHash::from_bytes(hash.try_into().ok()?)),
        };
        let issued_at = UnixMillis(r.u64()?);
        let issuer = Holder {
            member: read_member(&mut r)?,
            device: DeviceId::new(r.text()?).ok()?,
        };
        let kind = match r.u8()? {
            TAG_CREATE => EntryKind::CreateTeam {
                devices: read_devices(&mut r)?,
            },
            TAG_ADD_MEMBER => EntryKind::AddMember {
                member: read_member(&mut r)?,
                role: role_from(r.u8()?)?,
                devices: read_devices(&mut r)?,
            },
            TAG_ADD_DEVICE => EntryKind::AddDevice {
                member: read_member(&mut r)?,
                device: read_device(&mut r)?,
            },
            TAG_REMOVE_DEVICE => EntryKind::RemoveDevice {
                holder: Holder {
                    member: read_member(&mut r)?,
                    device: DeviceId::new(r.text()?).ok()?,
                },
            },
            TAG_REMOVE_MEMBER => EntryKind::RemoveMember {
                member: read_member(&mut r)?,
            },
            TAG_CHANGE_ROLE => EntryKind::ChangeRole {
                member: read_member(&mut r)?,
                role: role_from(r.u8()?)?,
            },
            TAG_ROTATE => EntryKind::RotateEpoch {
                epoch: Epoch(r.u32()?),
                commitment: EpochCommitment(r.fixed()?),
                history: match r.field()? {
                    [] => None,
                    link => Some(HistoryLink::from_slice(link)?),
                },
            },
            _ => return None,
        };
        r.finish()?;
        Some(Self {
            team,
            index,
            prev,
            issued_at,
            issuer,
            kind,
        })
    }

    /// Signs the body with the issuing device's key: the `memberlog.append` to send. The caller
    /// checks it first ([`TeamState::check`]) so the relay never stores an entry clients refuse.
    pub fn sign(&self, identity: &DeviceIdentity) -> Result<MemberLogAppend, LogError> {
        let mut bytes = self.encode()?;
        let signature = identity.sign(&bytes);
        bytes.extend_from_slice(&signature);
        if base64url_len(bytes.len()) > MAX_INLINE_CIPHERTEXT_LEN {
            return Err(LogError::TooLarge);
        }
        Ok(MemberLogAppend {
            index: self.index,
            prev: self.prev,
            entry: to_ciphertext(&bytes).map_err(|_| LogError::TooLarge)?,
        })
    }
}

/// Splits a stored entry into body bytes and signature.
fn split_entry(entry: &Ciphertext) -> Result<(Vec<u8>, [u8; SIGNATURE_LEN]), LogError> {
    let mut bytes = from_ciphertext(entry).ok_or(LogError::Malformed)?;
    if bytes.len() <= SIGNATURE_LEN {
        return Err(LogError::Malformed);
    }
    let at = bytes.len() - SIGNATURE_LEN;
    let mut signature = [0u8; SIGNATURE_LEN];
    signature.copy_from_slice(&bytes[at..]);
    bytes.truncate(at);
    Ok((bytes, signature))
}

/// A member as the log lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The member's role.
    pub role: Role,
    /// The member's current devices, recovery devices included.
    pub devices: BTreeMap<DeviceId, DeviceEntry>,
}

/// An epoch as the log records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochRecord {
    /// The epoch.
    pub epoch: Epoch,
    /// The commitment to its secret.
    pub commitment: EpochCommitment,
    /// The previous epoch's secret under this one, if history was granted.
    pub history: Option<HistoryLink>,
    /// The log index of the rotate epoch entry.
    pub index: u64,
}

/// The verified state of a team after some prefix of its membership log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamState {
    team: TeamId,
    owner: MemberId,
    members: BTreeMap<MemberId, Member>,
    epochs: Vec<EpochRecord>,
    rotation_due: bool,
    head: ChainHead,
    used_keys: BTreeSet<[u8; 32]>,
}

fn may_manage(issuer: &MemberId, issuer_role: Role, target: &MemberId, target_role: Role) -> bool {
    issuer == target
        || issuer_role == Role::Owner
        || (issuer_role == Role::Admin && target_role == Role::Member)
}

impl TeamState {
    fn create(team: &TeamId, body: &EntryBody, head: ChainHead) -> Result<Self, LogError> {
        let EntryKind::CreateTeam { devices } = &body.kind else {
            return Err(LogError::NotCreated);
        };
        if body.index != 1 || body.prev.is_some() {
            return Err(LogError::NotCreated);
        }
        if !devices.iter().any(|d| d.device == body.issuer.device) {
            return Err(LogError::UnknownIssuer);
        }
        let mut state = Self {
            team: team.clone(),
            owner: body.issuer.member.clone(),
            members: BTreeMap::new(),
            epochs: Vec::new(),
            rotation_due: false,
            head,
            used_keys: BTreeSet::new(),
        };
        state.check_new_devices(devices)?;
        state.insert_member(body.issuer.member.clone(), Role::Owner, devices);
        Ok(state)
    }

    /// The team.
    pub fn team(&self) -> &TeamId {
        &self.team
    }

    /// The member who created the team.
    pub fn owner(&self) -> &MemberId {
        &self.owner
    }

    /// The newest verified entry.
    pub fn head(&self) -> &ChainHead {
        &self.head
    }

    /// All current members.
    pub fn members(&self) -> &BTreeMap<MemberId, Member> {
        &self.members
    }

    /// One member.
    pub fn member(&self, member: &MemberId) -> Option<&Member> {
        self.members.get(member)
    }

    /// One device the log vouches for now.
    pub fn device(&self, holder: &Holder) -> Option<&DeviceEntry> {
        self.members
            .get(&holder.member)?
            .devices
            .get(&holder.device)
    }

    /// Every device the log vouches for now, recovery devices included.
    pub fn devices(&self) -> impl Iterator<Item = (Holder, &DeviceEntry)> {
        self.members.iter().flat_map(|(member, m)| {
            m.devices.values().map(move |d| {
                (
                    Holder {
                        member: member.clone(),
                        device: d.device.clone(),
                    },
                    d,
                )
            })
        })
    }

    /// The device with these public keys (to find the holder of a recovered identity).
    pub fn find_device(&self, keys: &DevicePublicKeys) -> Option<Holder> {
        self.devices()
            .find(|(_, d)| d.keys == *keys)
            .map(|(holder, _)| holder)
    }

    /// The newest epoch, if the team has one yet.
    pub fn current_epoch(&self) -> Option<&EpochRecord> {
        self.epochs.last()
    }

    /// The record of `epoch`.
    pub fn epoch(&self, epoch: Epoch) -> Option<&EpochRecord> {
        let at = usize::try_from(epoch.0.checked_sub(1)?).ok()?;
        self.epochs.get(at)
    }

    /// A device or member was removed after the newest epoch started: the team should rotate.
    pub fn rotation_due(&self) -> bool {
        self.rotation_due
    }

    /// A body for the next entry (index and `prev` filled in).
    pub fn next_body(&self, issuer: Holder, issued_at: UnixMillis, kind: EntryKind) -> EntryBody {
        EntryBody {
            team: self.team.clone(),
            index: self.head.index.saturating_add(1),
            prev: Some(self.head.hash),
            issued_at,
            issuer,
            kind,
        }
    }

    /// Opens this device's wrapped key of `epoch`, checked against the log's commitment.
    pub fn unwrap_key(
        &self,
        identity: &DeviceIdentity,
        me: &Holder,
        epoch: Epoch,
        data: &Ciphertext,
    ) -> Result<TeamSecret, WrapError> {
        let record = self.epoch(epoch).ok_or(WrapError::UnknownEpoch(epoch.0))?;
        epoch::unwrap(identity, &self.team, epoch, me, data, &record.commitment)
    }

    /// The secret of `epoch - 1` from the secret of `epoch`, through the history link the
    /// rotation recorded (none: history was not granted at that rotation).
    pub fn previous_secret(
        &self,
        epoch: Epoch,
        secret: &TeamSecret,
    ) -> Result<TeamSecret, WrapError> {
        let record = self.epoch(epoch).ok_or(WrapError::UnknownEpoch(epoch.0))?;
        let link = record
            .history
            .as_ref()
            .ok_or(WrapError::UnknownEpoch(epoch.0.saturating_sub(1)))?;
        let previous_epoch = Epoch(epoch.0.saturating_sub(1));
        let previous = self
            .epoch(previous_epoch)
            .ok_or(WrapError::UnknownEpoch(previous_epoch.0))?;
        link.open(secret, &self.team, epoch, &previous.commitment)
    }

    fn check_new_devices(&self, devices: &[DeviceEntry]) -> Result<(), LogError> {
        if devices.is_empty() {
            return Err(LogError::NoDevices);
        }
        if devices.len() > MAX_DEVICES_PER_ENTRY {
            return Err(LogError::TooManyDevices);
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for device in devices {
            if !ids.insert(&device.device) {
                return Err(LogError::DeviceExists);
            }
            if !is_valid_sign_key(&device.keys.sign) {
                return Err(LogError::InvalidKey);
            }
            for key in [device.keys.sign, device.keys.kem] {
                let bytes = *key.as_bytes();
                if self.used_keys.contains(&bytes) || !keys.insert(bytes) {
                    return Err(LogError::KeyReused);
                }
            }
        }
        Ok(())
    }

    fn insert_member(&mut self, member: MemberId, role: Role, devices: &[DeviceEntry]) {
        let mut entry = Member {
            role,
            devices: BTreeMap::new(),
        };
        for device in devices {
            self.remember_keys(&device.keys);
            entry.devices.insert(device.device.clone(), device.clone());
        }
        self.members.insert(member, entry);
    }

    fn remember_keys(&mut self, keys: &DevicePublicKeys) {
        self.used_keys.insert(*keys.sign.as_bytes());
        self.used_keys.insert(*keys.kem.as_bytes());
    }

    /// Whether this state accepts `body` as its next entry: team, position, issuer and the
    /// rules of the module docs. The signature is checked by [`LogVerifier::apply`].
    pub fn check(&self, body: &EntryBody) -> Result<(), LogError> {
        if body.team != self.team {
            return Err(LogError::TeamMismatch);
        }
        if Some(body.index) != self.head.index.checked_add(1) {
            return Err(LogError::IndexMismatch);
        }
        if body.prev != Some(self.head.hash) {
            return Err(LogError::PrevMismatch);
        }
        let issuer = self
            .member(&body.issuer.member)
            .filter(|m| m.devices.contains_key(&body.issuer.device))
            .ok_or(LogError::UnknownIssuer)?;
        let issuer_role = issuer.role;
        if issuer_role == Role::Member {
            return Err(LogError::Forbidden);
        }
        let manage = |target: &MemberId| -> Result<&Member, LogError> {
            let member = self.member(target).ok_or(LogError::UnknownMember)?;
            if may_manage(&body.issuer.member, issuer_role, target, member.role) {
                Ok(member)
            } else {
                Err(LogError::Forbidden)
            }
        };
        match &body.kind {
            EntryKind::CreateTeam { .. } => Err(LogError::AlreadyCreated),
            EntryKind::AddMember {
                member,
                role,
                devices,
            } => {
                if self.members.contains_key(member) {
                    return Err(LogError::MemberExists);
                }
                if *role == Role::Owner || (*role == Role::Admin && issuer_role != Role::Owner) {
                    return Err(LogError::Forbidden);
                }
                self.check_new_devices(devices)
            }
            EntryKind::AddDevice { member, device } => {
                let target = manage(member)?;
                if target.devices.contains_key(&device.device) {
                    return Err(LogError::DeviceExists);
                }
                self.check_new_devices(std::slice::from_ref(device))
            }
            EntryKind::RemoveDevice { holder } => {
                let target = manage(&holder.member)?;
                if !target.devices.contains_key(&holder.device) {
                    return Err(LogError::UnknownDevice);
                }
                if *holder == body.issuer {
                    return Err(LogError::Forbidden);
                }
                Ok(())
            }
            EntryKind::RemoveMember { member } => {
                let target = manage(member)?;
                if *member == body.issuer.member || target.role == Role::Owner {
                    return Err(LogError::Forbidden);
                }
                Ok(())
            }
            EntryKind::ChangeRole { member, role } => {
                if issuer_role != Role::Owner
                    || *member == body.issuer.member
                    || *role == Role::Owner
                {
                    return Err(LogError::Forbidden);
                }
                let target = self.member(member).ok_or(LogError::UnknownMember)?;
                if target.role == *role {
                    return Err(LogError::NoChange);
                }
                Ok(())
            }
            EntryKind::RotateEpoch { epoch, history, .. } => {
                let expected = match self.current_epoch() {
                    None => 1,
                    Some(current) => {
                        current.epoch.0.checked_add(1).ok_or(LogError::EpochOrder {
                            expected: u32::MAX,
                            got: epoch.0,
                        })?
                    }
                };
                if epoch.0 != expected {
                    return Err(LogError::EpochOrder {
                        expected,
                        got: epoch.0,
                    });
                }
                if history.is_some() && expected == 1 {
                    return Err(LogError::NoPreviousEpoch);
                }
                Ok(())
            }
        }
    }

    /// Applies an entry [`TeamState::check`] accepted.
    fn apply_checked(&mut self, body: &EntryBody, head: ChainHead) {
        match &body.kind {
            EntryKind::CreateTeam { .. } => {}
            EntryKind::AddMember {
                member,
                role,
                devices,
            } => self.insert_member(member.clone(), *role, devices),
            EntryKind::AddDevice { member, device } => {
                self.remember_keys(&device.keys);
                if let Some(target) = self.members.get_mut(member) {
                    target.devices.insert(device.device.clone(), device.clone());
                }
            }
            EntryKind::RemoveDevice { holder } => {
                if let Some(target) = self.members.get_mut(&holder.member) {
                    target.devices.remove(&holder.device);
                }
                self.rotation_due = true;
            }
            EntryKind::RemoveMember { member } => {
                self.members.remove(member);
                self.rotation_due = true;
            }
            EntryKind::ChangeRole { member, role } => {
                if let Some(target) = self.members.get_mut(member) {
                    target.role = *role;
                }
            }
            EntryKind::RotateEpoch {
                epoch,
                commitment,
                history,
            } => {
                self.epochs.push(EpochRecord {
                    epoch: *epoch,
                    commitment: *commitment,
                    history: history.clone(),
                    index: body.index,
                });
                self.rotation_due = false;
            }
        }
        self.head = head;
    }
}

/// Verifies a team's membership log entry by entry (from `memberlog.entries`, in order) and
/// keeps the resulting [`TeamState`]. On a refused entry nothing changes, and since the next
/// entry then leaves a gap, the log stays stopped there.
#[derive(Debug, Clone)]
pub struct LogVerifier {
    team: TeamId,
    chain: MemberChain,
    state: Option<TeamState>,
}

impl LogVerifier {
    /// Verification of `team`'s log from its first entry.
    pub fn new(team: TeamId) -> Self {
        Self {
            team,
            chain: MemberChain::new(),
            state: None,
        }
    }

    /// The verified state; `None` before the first entry.
    pub fn state(&self) -> Option<&TeamState> {
        self.state.as_ref()
    }

    /// Verifies and applies the next entry: chain hash, team and position binding, signature of
    /// the issuing device the log already vouches for (entry 1: of a device it lists itself),
    /// and the rules.
    pub fn apply(&mut self, entry: &MemberLogEntry) -> Result<(), LogError> {
        let prev = self.chain.head().map(|h| h.hash);
        let mut chain = self.chain.clone();
        chain.verify(entry)?;
        let head = *chain.head().ok_or(LogError::Malformed)?;
        let (bytes, signature) = split_entry(&entry.entry)?;
        let body = EntryBody::decode(&bytes)?;
        if body.team != self.team {
            return Err(LogError::TeamMismatch);
        }
        if body.index != entry.index {
            return Err(LogError::IndexMismatch);
        }
        if body.prev != prev {
            return Err(LogError::PrevMismatch);
        }
        match &mut self.state {
            None => {
                let state = TeamState::create(&self.team, &body, head)?;
                let key = state
                    .device(&body.issuer)
                    .ok_or(LogError::UnknownIssuer)?
                    .keys
                    .sign;
                if !verify(&key, &bytes, &signature) {
                    return Err(LogError::Signature);
                }
                self.state = Some(state);
            }
            Some(state) => {
                state.check(&body)?;
                let key = state
                    .device(&body.issuer)
                    .ok_or(LogError::UnknownIssuer)?
                    .keys
                    .sign;
                if !verify(&key, &bytes, &signature) {
                    return Err(LogError::Signature);
                }
                state.apply_checked(&body, head);
            }
        }
        self.chain = chain;
        Ok(())
    }

    /// [`LogVerifier::apply`] for each entry in order; stops at the first refused one.
    pub fn apply_all<'a>(
        &mut self,
        entries: impl IntoIterator<Item = &'a MemberLogEntry>,
    ) -> Result<(), LogError> {
        entries.into_iter().try_for_each(|entry| self.apply(entry))
    }
}
