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
//!          || field(proof of possession, 64 bytes; see `crate::identity`)
//! devices = field(count, u32) || device ...
//! role = 1 owner, 2 admin, 3 member
//! ```
//!
//! | Tag | Kind | Fields |
//! | --- | --- | --- |
//! | 1 | create team | devices of the issuing owner |
//! | 2 | add member | member, role, devices |
//! | 3 | add device | member, device, endorsing device id, endorsement (64 bytes) |
//! | 4 | remove device | member, device id |
//! | 5 | remove member | member |
//! | 6 | change role | member, role |
//! | 7 | rotate epoch | epoch (u32), commitment (32 bytes), history link (73 bytes or empty) |
//!
//! Integers are big endian. The body names the team (the chain hash does not, see
//! `hangar_team_protocol::e2e`), its own index and its predecessor's hash: a validly signed entry
//! fits only one position of one team's log, so a relay can neither move it to another team nor
//! splice it into a different history. Decoding is strict (unknown tags, out-of-range bytes,
//! trailing bytes refused) and re-encoding must give the same bytes. Version 1 is not deployed;
//! it changed in place before the first release.
//!
//! # Rules
//!
//! - Entry 1 creates the team: its issuer becomes the owner, and the issuing device must be among
//!   the listed devices (the entry signs itself). Every later entry is refused if it creates.
//! - Every later entry is issued by a device the log lists. Owners and admins issue every kind;
//!   any member adds a device to itself and removes another device of its own. The owner manages
//!   everyone; an admin manages members and itself.
//! - A member has at most `MAX_DEVICES_PER_MEMBER` (16) devices and the team at most
//!   `MAX_WRAPPED_KEYS` (512), recovery devices included, so every rotation fits one
//!   `key.rotate`; an add member or add device entry over either is refused.
//! - Every listed device carries its proof of possession. An add device entry also carries the
//!   endorsement of a current device of the member who gets it, so an owner or admin cannot add
//!   a device to someone else's identity alone; a member's first devices come with the add
//!   member entry of an owner or admin.
//! - A recovery device issues only add device for its own member and remove device of its
//!   member's other devices.
//! - No one removes the owner, removes itself, or removes the issuing device; only the owner
//!   changes roles, and no entry makes a second owner (ownership transfer is not part of
//!   version 1).
//! - A public key is accepted once per team and never again, even after its device was removed.
//!   Signing keys must be valid points of large order, X25519 keys canonical and of large order.
//! - Removing a device or a member marks a rotation as due; a rotate epoch entry clears it. Epochs
//!   count from 1 without gaps.
//! - A member id that was removed may be added again (the website decides who is a member); the
//!   state records it ([`Member::readmitted_after`]) so the app can show it.
//! - `issued_at` is the issuer's clock, informational only; the order is the log's.
//!
//! # Relay side
//!
//! The relay keeps one [`LogVerifier`] per team and runs every `memberlog.append` through
//! [`LogVerifier::admit`], every `key.rotate` through [`LogVerifier::admit_rotation`] and every
//! `device.remove` through [`LogVerifier::admit_device_remove`] before it stores or changes
//! anything: the same checks as the clients, no secrets needed. A rotate epoch entry is
//! accepted only inside `key.rotate`, together with its wrapped keys, so the log never names an
//! epoch whose keys the relay does not hold; a remove device entry only inside `device.remove`,
//! so the relay removes the device in the same step. After a restart the relay rebuilds the
//! verifier from its stored entries with [`LogVerifier::apply_all`]; a refusal there means the
//! storage is damaged. Clients verify every entry themselves all the same.
//!
//! The relay also passes the role of the sender's verified ticket. The issuer's rights then
//! follow the lower of its role in the log and its ticket's role: a member the website
//! downgraded loses the rights of its old role at once, before an owner records the change in
//! the log. Entry 1 needs an owner's ticket. Clients do not see tickets; they check the log
//! alone. The relay's other checks that need the verified state (`key.wrap`, registered device
//! keys) are in [`crate::relay`].

use std::collections::{BTreeMap, BTreeSet};

use hangar_team_protocol::e2e::{
    check_append, ChainError, ChainHead, KeyProof, LogHash, MemberChain, PublicKey,
};
use hangar_team_protocol::ids::{Ciphertext, DeviceId, Epoch, MemberId, TeamId, UnixMillis};
use hangar_team_protocol::limits::{
    MAX_DEVICES_PER_MEMBER, MAX_INLINE_CIPHERTEXT_LEN, MAX_WRAPPED_KEYS,
};
use hangar_team_protocol::wire::{
    DeviceRemove, ErrorCode, Holder, KeyRotate, MemberLogAppend, MemberLogEntry, Role,
};
use thiserror::Error;

use crate::encoding::{base64url_len, from_ciphertext, to_ciphertext, Reader, Writer};
use crate::epoch::{self, EpochCommitment, HistoryLink, TeamSecret, WrapError};
use crate::identity::{
    is_valid_kem_key, is_valid_sign_key, log_safety_number, verify, verify_endorsement,
    verify_proof, DeviceIdentity, DevicePublicKeys, SafetyNumber,
};

const LABEL_ENTRY: &[u8] = b"hangar-team/v1/memberlog-entry";
/// The format version of new entries.
pub const ENTRY_VERSION: u8 = 1;
/// Devices one entry may list: a member's first devices, at most as many as one member may
/// have.
pub const MAX_DEVICES_PER_ENTRY: usize = MAX_DEVICES_PER_MEMBER;
const SIGNATURE_LEN: usize = 64;

const TAG_CREATE: u8 = 1;
const TAG_ADD_MEMBER: u8 = 2;
const TAG_ADD_DEVICE: u8 = 3;
const TAG_REMOVE_DEVICE: u8 = 4;
const TAG_REMOVE_MEMBER: u8 = 5;
const TAG_CHANGE_ROLE: u8 = 6;
const TAG_ROTATE: u8 = 7;

/// Why a membership log entry was refused. The log stops at a refused entry: nothing after it
/// is accepted (fail closed), and the app shows the team as untrusted. The relay answers a
/// refused `memberlog.append` or `key.rotate` with [`LogError::code`].
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
    /// Relay side: the entry is signed by another device than the connection that sent it.
    #[error("the membership log entry is not issued by the sending device")]
    NotSender,
    /// The signature does not verify with the issuing device's key.
    #[error("the membership log entry's signature does not verify")]
    Signature,
    /// The issuer's role or kind of device does not allow this entry.
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
    /// An Ed25519 key that is not a valid point or of small order, or an X25519 key that is not
    /// canonical or of small order.
    #[error("a public key in the entry is not a valid device key")]
    InvalidKey,
    /// A device's proof of possession does not verify.
    #[error("a device's proof of possession does not verify")]
    Proof,
    /// The endorsement of an add device entry is not from a current device of the member, or
    /// does not verify.
    #[error("the new device is not endorsed by a device of its member")]
    Endorsement,
    /// No device where at least one is needed.
    #[error("the entry lists no device")]
    NoDevices,
    /// More than [`MAX_DEVICES_PER_ENTRY`] devices in one entry, or an entry that would give a
    /// member more than `MAX_DEVICES_PER_MEMBER` devices or the team more than
    /// `MAX_WRAPPED_KEYS`.
    #[error("the entry exceeds the number of devices a member or a team may have")]
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
    /// Relay side: a rotate epoch entry sent through `memberlog.append`; it travels only inside
    /// `key.rotate`, together with the wrapped keys.
    #[error("a rotate epoch entry travels only inside `key.rotate`")]
    RotationOutsideKeyRotate,
    /// Relay side: `key.rotate` carries another kind of entry, or one for another epoch.
    #[error("`key.rotate` does not carry the rotate epoch entry of its epoch")]
    NotARotation,
    /// Relay side: `key.rotate` wraps a key to a holder the log does not list, or twice to one.
    #[error("`key.rotate` wraps a key to a device the log does not list")]
    WrappedHolder,
    /// Relay side: a remove device entry sent through `memberlog.append`; it travels only inside
    /// `device.remove`, so the relay removes the device in the same step.
    #[error("a remove device entry travels only inside `device.remove`")]
    RemovalOutsideDeviceRemove,
    /// Relay side: `device.remove` carries another kind of entry, or one that removes another
    /// device than the message names.
    #[error("`device.remove` does not carry the remove device entry of its device")]
    NotADeviceRemoval,
    /// Relay side: a device the entry lists did not register these keys over its own
    /// connection (`device.keys`); see [`crate::relay::check_registered_keys`].
    #[error("a device's keys in the entry are not the keys it registered")]
    KeysNotRegistered,
}

impl LogError {
    /// The `err` code the relay answers a refused `memberlog.append`, `key.rotate` or
    /// `device.remove` with.
    pub fn code(&self) -> ErrorCode {
        match self {
            LogError::Chain(chain) => chain.code(),
            LogError::UnknownIssuer
            | LogError::NotSender
            | LogError::Signature
            | LogError::Forbidden
            | LogError::KeysNotRegistered => ErrorCode::Forbidden,
            LogError::EpochOrder { .. } => ErrorCode::StaleEpoch,
            LogError::TooLarge | LogError::TooManyDevices => ErrorCode::Limit,
            _ => ErrorCode::BadRequest,
        }
    }
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
    /// The device's proof of possession ([`DeviceIdentity::prove`]).
    pub proof: KeyProof,
}

impl DeviceEntry {
    /// The entry for a device whose private keys are at hand (a new team's owner, a recovery
    /// device): its keys and its own proof as `holder` of `team`.
    pub fn new(team: &TeamId, holder: &Holder, identity: &DeviceIdentity, recovery: bool) -> Self {
        Self {
            device: holder.device.clone(),
            keys: identity.public_keys(),
            recovery,
            proof: identity.prove(team, holder, recovery),
        }
    }

    /// The entry for another device from what its `device.keys` published (keys and proof).
    pub fn from_published(device: DeviceId, keys: DevicePublicKeys, proof: KeyProof) -> Self {
        Self {
            device,
            keys,
            recovery: false,
            proof,
        }
    }
}

/// An existing device's consent to a new device of the same member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endorsement {
    /// The endorsing device, a current device of the member who gets the new one.
    pub device: DeviceId,
    /// Its signature ([`DeviceIdentity::endorse`]).
    pub signature: KeyProof,
}

/// What an entry changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// Entry 1: the issuer becomes the owner with these devices (the issuing one among them).
    CreateTeam {
        /// The owner's first devices.
        devices: Vec<DeviceEntry>,
    },
    /// A new member with its first devices (owner or admin only).
    AddMember {
        /// The new member.
        member: MemberId,
        /// Admin (only the owner grants it) or member, never owner.
        role: Role,
        /// Its first devices.
        devices: Vec<DeviceEntry>,
    },
    /// Another device of a member, endorsed by one of the member's current devices.
    AddDevice {
        /// The member who gets the device.
        member: MemberId,
        /// The new device.
        device: DeviceEntry,
        /// The member's consent.
        endorsement: Endorsement,
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
        .u8(u8::from(device.recovery))
        .field(device.proof.as_bytes());
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
    let proof = KeyProof::from_bytes(r.fixed()?);
    Some(DeviceEntry {
        device,
        keys: DevicePublicKeys { sign, kem },
        recovery,
        proof,
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
            EntryKind::AddDevice {
                member,
                device,
                endorsement,
            } => {
                w.field(member.as_str().as_bytes());
                write_device(&mut w, device);
                w.field(endorsement.device.as_str().as_bytes())
                    .field(endorsement.signature.as_bytes());
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
                endorsement: Endorsement {
                    device: DeviceId::new(r.text()?).ok()?,
                    signature: KeyProof::from_bytes(r.fixed()?),
                },
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

    /// Signs the body with the issuing device's key: the `memberlog.append` to send (a rotate
    /// epoch entry goes into `key.rotate` instead, see [`crate::rotation`]). The caller checks
    /// it first ([`TeamState::check`]) so the relay never stores an entry clients refuse.
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

impl EntryBody {
    /// Decodes a signed entry's body **without** checking its signature or anything else; for
    /// checks that come before [`LogVerifier::admit`] (such as
    /// [`crate::relay::check_registered_keys`]) and for display. Trust only what a
    /// [`LogVerifier`] applied.
    pub fn decode_unverified(entry: &Ciphertext) -> Result<Self, LogError> {
        let (bytes, _) = split_entry(entry)?;
        Self::decode(&bytes)
    }

    /// The devices the entry lists with the holder each one gets (create team, add member, add
    /// device; empty for the other kinds).
    pub fn listed_devices(&self) -> Vec<(Holder, &DeviceEntry)> {
        match &self.kind {
            EntryKind::CreateTeam { devices } => devices
                .iter()
                .map(|d| (holder_of(&self.issuer.member, &d.device), d))
                .collect(),
            EntryKind::AddMember {
                member, devices, ..
            } => devices
                .iter()
                .map(|d| (holder_of(member, &d.device), d))
                .collect(),
            EntryKind::AddDevice { member, device, .. } => {
                vec![(holder_of(member, &device.device), device)]
            }
            _ => Vec::new(),
        }
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
    /// The log index of the entry that added the member (1 for the owner).
    pub added_at: u64,
    /// The device that issued that entry (for the app's "added by" note).
    pub added_by: Holder,
    /// The log index of the remove member entry that removed this member id before it was
    /// added again, if it was. The website decides who is a member, and an id it hands out
    /// again comes back with new devices; the app shows that the member was re-admitted, by
    /// [`Member::added_by`] at [`Member::added_at`].
    pub readmitted_after: Option<u64>,
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
    /// The log hash of the rotate epoch entry; part of the epoch's key derivation.
    pub hash: LogHash,
}

/// One time span in which the log vouched for a holder's keys. A holder can have several (a
/// device id used again after its removal, with new keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPeriod {
    /// The keys of the span.
    pub keys: DevicePublicKeys,
    /// A recovery device.
    pub recovery: bool,
    /// The log index of the entry that added the device.
    pub added_at: u64,
    /// The device that issued that entry (for the app's "added by" note).
    pub added_by: Holder,
    /// The member's device that endorsed it (add device entries only).
    pub endorsed_by: Option<DeviceId>,
    /// The log index of the entry that removed it; `None` while it is current.
    pub removed_at: Option<u64>,
}

impl KeyPeriod {
    /// Whether the log vouched for these keys after the entry at `index` (added at or before it,
    /// not removed by then).
    pub fn covers(&self, index: u64) -> bool {
        self.added_at <= index && self.removed_at.is_none_or(|removed| index < removed)
    }
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
    /// The chain hash of every entry, index `i` at position `i - 1`.
    hashes: Vec<LogHash>,
    /// Every key period of every holder, removed ones included.
    history: BTreeMap<Holder, Vec<KeyPeriod>>,
    used_keys: BTreeSet<[u8; 32]>,
    /// The index of each removed member's last remove member entry.
    departed: BTreeMap<MemberId, u64>,
}

/// The lower (less privileged) of two roles.
pub(crate) fn lower_role(a: Role, b: Role) -> Role {
    fn rank(role: Role) -> u8 {
        match role {
            Role::Owner => 0,
            Role::Admin => 1,
            Role::Member => 2,
        }
    }
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

fn may_manage(issuer: &MemberId, issuer_role: Role, target: &MemberId, target_role: Role) -> bool {
    issuer == target
        || issuer_role == Role::Owner
        || (issuer_role == Role::Admin && target_role == Role::Member)
}

fn holder_of(member: &MemberId, device: &DeviceId) -> Holder {
    Holder {
        member: member.clone(),
        device: device.clone(),
    }
}

impl TeamState {
    fn create(team: &TeamId, body: &EntryBody, head: ChainHead) -> Result<Self, LogError> {
        let EntryKind::CreateTeam { devices } = &body.kind else {
            return Err(LogError::NotCreated);
        };
        if body.index != 1 || body.prev.is_some() {
            return Err(LogError::NotCreated);
        }
        if !devices
            .iter()
            .any(|d| d.device == body.issuer.device && !d.recovery)
        {
            return Err(LogError::UnknownIssuer);
        }
        let mut state = Self {
            team: team.clone(),
            owner: body.issuer.member.clone(),
            members: BTreeMap::new(),
            epochs: Vec::new(),
            rotation_due: false,
            head,
            hashes: vec![head.hash],
            history: BTreeMap::new(),
            used_keys: BTreeSet::new(),
            departed: BTreeMap::new(),
        };
        state.check_new_devices(&body.issuer.member, devices)?;
        state.insert_member(body, body.issuer.member.clone(), Role::Owner, devices);
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

    /// The chain hash of the verified entry at `index` (from 1).
    pub fn hash_at(&self, index: u64) -> Option<LogHash> {
        let at = usize::try_from(index.checked_sub(1)?).ok()?;
        self.hashes.get(at).copied()
    }

    /// The log safety number at `index`, so two members can compare the log at an entry both
    /// have (the newest one of the one who is behind), not only at their heads.
    pub fn log_safety_number_at(&self, index: u64) -> Option<SafetyNumber> {
        let hash = self.hash_at(index)?;
        Some(log_safety_number(&self.team, &ChainHead { index, hash }))
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
            m.devices
                .values()
                .map(move |d| (holder_of(member, &d.device), d))
        })
    }

    /// How many devices the log vouches for now, recovery devices included.
    pub fn device_count(&self) -> usize {
        self.members.values().map(|m| m.devices.len()).sum()
    }

    /// The role `member` may act with when the relay's ticket says `ticket_role`: the lower of
    /// the two. `None` when the log does not list the member.
    pub fn effective_role(&self, member: &MemberId, ticket_role: Role) -> Option<Role> {
        self.member(member).map(|m| lower_role(m.role, ticket_role))
    }

    /// The device with these public keys (to find the holder of a recovered identity).
    pub fn find_device(&self, keys: &DevicePublicKeys) -> Option<Holder> {
        self.devices()
            .find(|(_, d)| d.keys == *keys)
            .map(|(holder, _)| holder)
    }

    /// Every key period of `holder`, oldest first, removed ones included (to verify records a
    /// device sealed before its removal).
    pub fn key_periods(&self, holder: &Holder) -> &[KeyPeriod] {
        self.history.get(holder).map_or(&[], Vec::as_slice)
    }

    /// The device that issued the entry adding `holder`'s current keys.
    pub fn added_by(&self, holder: &Holder) -> Option<&Holder> {
        self.key_periods(holder)
            .last()
            .filter(|p| p.removed_at.is_none())
            .map(|p| &p.added_by)
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
        self.unwrap_any(identity, me, epoch, std::iter::once(data))
    }

    /// Opens the first of the wrapped keys `key.get` returned (up to `MAX_KEY_PACKAGES`) that
    /// matches the log's commitment of `epoch`.
    pub fn unwrap_any<'a>(
        &self,
        identity: &DeviceIdentity,
        me: &Holder,
        epoch: Epoch,
        packages: impl IntoIterator<Item = &'a Ciphertext>,
    ) -> Result<TeamSecret, WrapError> {
        let record = self.epoch(epoch).ok_or(WrapError::UnknownEpoch(epoch.0))?;
        epoch::unwrap_any(
            identity,
            &self.team,
            epoch,
            me,
            packages,
            &record.commitment,
        )
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

    /// New devices of `member`: within the per-entry bounds, ids unique, keys valid and new,
    /// each with a valid proof of possession.
    fn check_new_devices(
        &self,
        member: &MemberId,
        devices: &[DeviceEntry],
    ) -> Result<(), LogError> {
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
            if !is_valid_sign_key(&device.keys.sign) || !is_valid_kem_key(&device.keys.kem) {
                return Err(LogError::InvalidKey);
            }
            for key in [device.keys.sign, device.keys.kem] {
                let bytes = *key.as_bytes();
                if self.used_keys.contains(&bytes) || !keys.insert(bytes) {
                    return Err(LogError::KeyReused);
                }
            }
            let holder = holder_of(member, &device.device);
            if !verify_proof(
                &self.team,
                &holder,
                &device.keys,
                device.recovery,
                &device.proof,
            ) {
                return Err(LogError::Proof);
            }
        }
        Ok(())
    }

    fn add_device(&mut self, body: &EntryBody, member: &MemberId, device: &DeviceEntry) {
        self.remember_keys(&device.keys);
        let endorsed_by = match &body.kind {
            EntryKind::AddDevice { endorsement, .. } => Some(endorsement.device.clone()),
            _ => None,
        };
        self.history
            .entry(holder_of(member, &device.device))
            .or_default()
            .push(KeyPeriod {
                keys: device.keys,
                recovery: device.recovery,
                added_at: body.index,
                added_by: body.issuer.clone(),
                endorsed_by,
                removed_at: None,
            });
        if let Some(target) = self.members.get_mut(member) {
            target.devices.insert(device.device.clone(), device.clone());
        }
    }

    fn insert_member(
        &mut self,
        body: &EntryBody,
        member: MemberId,
        role: Role,
        devices: &[DeviceEntry],
    ) {
        let readmitted_after = self.departed.get(&member).copied();
        self.members.insert(
            member.clone(),
            Member {
                role,
                devices: BTreeMap::new(),
                added_at: body.index,
                added_by: body.issuer.clone(),
                readmitted_after,
            },
        );
        for device in devices {
            self.add_device(body, &member, device);
        }
    }

    fn remove_device(&mut self, holder: &Holder, index: u64) {
        if let Some(target) = self.members.get_mut(&holder.member) {
            target.devices.remove(&holder.device);
        }
        if let Some(period) = self
            .history
            .get_mut(holder)
            .and_then(|periods| periods.last_mut())
        {
            period.removed_at.get_or_insert(index);
        }
    }

    fn remember_keys(&mut self, keys: &DevicePublicKeys) {
        self.used_keys.insert(*keys.sign.as_bytes());
        self.used_keys.insert(*keys.kem.as_bytes());
    }

    /// Whether this state accepts `body` as its next entry: team, position, issuer and the
    /// rules of the module docs. The signature is checked by [`LogVerifier::apply`] and
    /// [`LogVerifier::admit`].
    pub fn check(&self, body: &EntryBody) -> Result<(), LogError> {
        self.check_capped(body, None)
    }

    /// [`TeamState::check`] with the issuer's role capped at `cap` (the relay's ticket role).
    fn check_capped(&self, body: &EntryBody, cap: Option<Role>) -> Result<(), LogError> {
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
            .ok_or(LogError::UnknownIssuer)?;
        let issuer_device = issuer
            .devices
            .get(&body.issuer.device)
            .ok_or(LogError::UnknownIssuer)?;
        let issuer_role = cap.map_or(issuer.role, |cap| lower_role(issuer.role, cap));
        let own = |member: &MemberId| *member == body.issuer.member;
        // A recovery device only restores its member: add a device to it, remove its others.
        if issuer_device.recovery {
            let allowed = match &body.kind {
                EntryKind::AddDevice { member, .. } => own(member),
                EntryKind::RemoveDevice { holder } => own(&holder.member),
                _ => false,
            };
            if !allowed {
                return Err(LogError::Forbidden);
            }
        }
        let admin = matches!(issuer_role, Role::Owner | Role::Admin);
        let manage = |target: &MemberId| -> Result<&Member, LogError> {
            let member = self.member(target).ok_or(LogError::UnknownMember)?;
            if own(target)
                || (admin && may_manage(&body.issuer.member, issuer_role, target, member.role))
            {
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
                if !admin {
                    return Err(LogError::Forbidden);
                }
                if self.members.contains_key(member) {
                    return Err(LogError::MemberExists);
                }
                if *role == Role::Owner || (*role == Role::Admin && issuer_role != Role::Owner) {
                    return Err(LogError::Forbidden);
                }
                self.check_room(0, devices.len())?;
                self.check_new_devices(member, devices)
            }
            EntryKind::AddDevice {
                member,
                device,
                endorsement,
            } => {
                let target = manage(member)?;
                if target.devices.contains_key(&device.device) {
                    return Err(LogError::DeviceExists);
                }
                self.check_room(target.devices.len(), 1)?;
                self.check_new_devices(member, std::slice::from_ref(device))?;
                let endorser = target
                    .devices
                    .get(&endorsement.device)
                    .ok_or(LogError::Endorsement)?;
                if !verify_endorsement(
                    &self.team,
                    &holder_of(member, &device.device),
                    &device.keys,
                    device.recovery,
                    &endorser.keys.sign,
                    &endorsement.signature,
                ) {
                    return Err(LogError::Endorsement);
                }
                Ok(())
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
                if !admin {
                    return Err(LogError::Forbidden);
                }
                let target = manage(member)?;
                if own(member) || target.role == Role::Owner {
                    return Err(LogError::Forbidden);
                }
                Ok(())
            }
            EntryKind::ChangeRole { member, role } => {
                if issuer_role != Role::Owner || own(member) || *role == Role::Owner {
                    return Err(LogError::Forbidden);
                }
                let target = self.member(member).ok_or(LogError::UnknownMember)?;
                if target.role == *role {
                    return Err(LogError::NoChange);
                }
                Ok(())
            }
            EntryKind::RotateEpoch { epoch, history, .. } => {
                if !admin {
                    return Err(LogError::Forbidden);
                }
                let expected = self.next_epoch()?;
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

    /// Room for `adding` devices on a member who has `has`, and on the team.
    fn check_room(&self, has: usize, adding: usize) -> Result<(), LogError> {
        if has.saturating_add(adding) > MAX_DEVICES_PER_MEMBER
            || self.device_count().saturating_add(adding) > MAX_WRAPPED_KEYS
        {
            return Err(LogError::TooManyDevices);
        }
        Ok(())
    }

    /// The number of the next epoch.
    fn next_epoch(&self) -> Result<u32, LogError> {
        match self.current_epoch() {
            None => Ok(1),
            Some(current) => current.epoch.0.checked_add(1).ok_or(LogError::EpochOrder {
                expected: u32::MAX,
                got: u32::MAX,
            }),
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
            } => self.insert_member(body, member.clone(), *role, devices),
            EntryKind::AddDevice { member, device, .. } => self.add_device(body, member, device),
            EntryKind::RemoveDevice { holder } => {
                self.remove_device(holder, body.index);
                self.rotation_due = true;
            }
            EntryKind::RemoveMember { member } => {
                let devices: Vec<DeviceId> = self
                    .members
                    .get(member)
                    .map(|m| m.devices.keys().cloned().collect())
                    .unwrap_or_default();
                for device in devices {
                    self.remove_device(&holder_of(member, &device), body.index);
                }
                self.members.remove(member);
                self.departed.insert(member.clone(), body.index);
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
                    hash: head.hash,
                });
                self.rotation_due = false;
            }
        }
        self.head = head;
        self.hashes.push(head.hash);
    }
}

/// What the relay admitted (`memberlog.append`, `key.rotate` or `device.remove`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    /// The new head; its hash goes into `memberlog.appended`.
    pub head: ChainHead,
    /// Devices the entry took out of the team (remove device, remove member): the relay drops
    /// their reservations, claims, public keys and wrapped keys and closes their connections.
    pub removed: Vec<Holder>,
}

/// Verifies a team's membership log entry by entry and keeps the resulting [`TeamState`].
/// Clients feed it `memberlog.entries` in order ([`LogVerifier::apply`]); the relay checks each
/// append before storing it ([`LogVerifier::admit`], [`LogVerifier::admit_rotation`],
/// [`LogVerifier::admit_device_remove`]). On a
/// refused entry nothing changes, and since the next entry then leaves a gap, a client's log
/// stays stopped there.
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

    /// Client side: verifies and applies the next entry from `memberlog.entries`: the hash the
    /// relay states, team and position binding, signature of the issuing device the log already
    /// vouches for (entry 1: of a device it lists itself), and the rules.
    pub fn apply(&mut self, entry: &MemberLogEntry) -> Result<(), LogError> {
        let mut chain = self.chain.clone();
        chain.verify(entry)?;
        let head = *chain.head().ok_or(LogError::Malformed)?;
        let prev = self.chain.head().map(|h| h.hash);
        let (body, first) = self.verify_entry(entry.index, prev, &entry.entry, head, None)?;
        self.commit(&body, first, head);
        Ok(())
    }

    /// [`LogVerifier::apply`] for each entry in order; stops at the first refused one.
    pub fn apply_all<'a>(
        &mut self,
        entries: impl IntoIterator<Item = &'a MemberLogEntry>,
    ) -> Result<(), LogError> {
        entries.into_iter().try_for_each(|entry| self.apply(entry))
    }

    /// Relay side: checks a `memberlog.append` from the connection bound to `sender`, whose
    /// verified ticket says `ticket_role` (chain, signature, rights with the lower of log role
    /// and ticket role, team, index, `prev`, and that `sender` issued it) and applies it. Entry
    /// 1 needs an owner's ticket. A rotate epoch entry is refused
    /// ([`LogError::RotationOutsideKeyRotate`]), it comes only with `key.rotate`; so is a remove
    /// device entry ([`LogError::RemovalOutsideDeviceRemove`]), it comes only with
    /// `device.remove`. On an error nothing changes and the relay stores nothing.
    pub fn admit(
        &mut self,
        append: &MemberLogAppend,
        sender: &Holder,
        ticket_role: Role,
    ) -> Result<Admitted, LogError> {
        let (body, first, head) = self.check_append(append, sender, ticket_role)?;
        match body.kind {
            EntryKind::RotateEpoch { .. } => return Err(LogError::RotationOutsideKeyRotate),
            EntryKind::RemoveDevice { .. } => return Err(LogError::RemovalOutsideDeviceRemove),
            _ => {}
        }
        Ok(self.commit(&body, first, head))
    }

    /// Relay side: checks a `device.remove` from the connection bound to `sender` with
    /// `ticket_role`: the remove device entry in `remove.log` as in [`LogVerifier::admit`], and
    /// the device it removes equal to `remove.holder`. Applies it only when all of it holds;
    /// the relay then stores the entry and removes the device ([`Admitted::removed`]) in one
    /// step.
    pub fn admit_device_remove(
        &mut self,
        remove: &DeviceRemove,
        sender: &Holder,
        ticket_role: Role,
    ) -> Result<Admitted, LogError> {
        let (body, first, head) = self.check_append(&remove.log, sender, ticket_role)?;
        match &body.kind {
            EntryKind::RemoveDevice { holder } if *holder == remove.holder => {}
            _ => return Err(LogError::NotADeviceRemoval),
        }
        Ok(self.commit(&body, first, head))
    }

    /// Relay side: checks a `key.rotate` from the connection bound to `sender` with
    /// `ticket_role` as one unit: the rotate epoch entry in `rotate.log` as in
    /// [`LogVerifier::admit`], its epoch equal to `rotate.epoch` and the next one of the log, and
    /// every wrapped key addressed to a distinct device the log lists. Applies the entry only
    /// when all of it holds; the relay then stores the entry and the wrapped keys in one step.
    pub fn admit_rotation(
        &mut self,
        rotate: &KeyRotate,
        sender: &Holder,
        ticket_role: Role,
    ) -> Result<Admitted, LogError> {
        let (body, first, head) = self.check_append(&rotate.log, sender, ticket_role)?;
        match &body.kind {
            EntryKind::RotateEpoch { epoch, .. } if *epoch == rotate.epoch => {}
            _ => return Err(LogError::NotARotation),
        }
        let state = self.state.as_ref().ok_or(LogError::NotCreated)?;
        let mut seen = BTreeSet::new();
        for wrapped in &rotate.wrapped {
            if state.device(&wrapped.holder).is_none() || !seen.insert(&wrapped.holder) {
                return Err(LogError::WrappedHolder);
            }
        }
        Ok(self.commit(&body, first, head))
    }

    fn check_append(
        &self,
        append: &MemberLogAppend,
        sender: &Holder,
        ticket_role: Role,
    ) -> Result<(EntryBody, Option<TeamState>, ChainHead), LogError> {
        let head = check_append(self.chain.head(), append)?;
        let (body, first) = self.verify_entry(
            append.index,
            append.prev,
            &append.entry,
            head,
            Some((sender, ticket_role)),
        )?;
        Ok((body, first, head))
    }

    /// Applies a verified entry; `first` is the state entry 1 created.
    fn commit(&mut self, body: &EntryBody, first: Option<TeamState>, head: ChainHead) -> Admitted {
        let removed = match &body.kind {
            EntryKind::RemoveDevice { holder } => vec![holder.clone()],
            EntryKind::RemoveMember { member } => self
                .state
                .as_ref()
                .and_then(|s| s.member(member))
                .map(|m| m.devices.keys().map(|d| holder_of(member, d)).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        self.chain = MemberChain::resume(Some(head));
        match (&mut self.state, first) {
            (_, Some(first)) => self.state = Some(first),
            (Some(state), None) => state.apply_checked(body, head),
            (None, None) => {}
        }
        Admitted { head, removed }
    }

    /// Decodes and checks the entry at `index` after `prev`, whose chain hash gives `head`,
    /// without changing `self`; returns the body and, for entry 1, the state it creates.
    /// `relay` is the sending connection's holder and ticket role on the relay (`None` on
    /// clients): the issuer must be the sender, and its role is capped at the ticket's.
    fn verify_entry(
        &self,
        index: u64,
        prev: Option<LogHash>,
        entry: &Ciphertext,
        head: ChainHead,
        relay: Option<(&Holder, Role)>,
    ) -> Result<(EntryBody, Option<TeamState>), LogError> {
        let (bytes, signature) = split_entry(entry)?;
        let body = EntryBody::decode(&bytes)?;
        if relay.is_some_and(|(sender, _)| body.issuer != *sender) {
            return Err(LogError::NotSender);
        }
        let cap = relay.map(|(_, role)| role);
        if body.team != self.team {
            return Err(LogError::TeamMismatch);
        }
        if body.index != index {
            return Err(LogError::IndexMismatch);
        }
        if body.prev != prev {
            return Err(LogError::PrevMismatch);
        }
        let first = match &self.state {
            None => {
                if cap.is_some_and(|role| role != Role::Owner) {
                    return Err(LogError::Forbidden);
                }
                Some(TeamState::create(&self.team, &body, head)?)
            }
            Some(state) => {
                state.check_capped(&body, cap)?;
                None
            }
        };
        // The issuer's key as the state before the entry knew it (entry 1: as it lists itself).
        let key = first
            .as_ref()
            .or(self.state.as_ref())
            .and_then(|state| state.device(&body.issuer))
            .ok_or(LogError::UnknownIssuer)?
            .keys
            .sign;
        if !verify(&key, &bytes, &signature) {
            return Err(LogError::Signature);
        }
        Ok((body, first))
    }
}
