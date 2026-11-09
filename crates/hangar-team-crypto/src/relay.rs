//! The relay's checks that need the verified membership log ([`TeamState`]) besides the log
//! entries themselves ([`crate::memberlog::LogVerifier`]). They live here, not in
//! `hangar-team-protocol`, because that crate cannot depend on this one; the relay depends on
//! both.
//!
//! The relay passes the role of the connection's verified ticket and the holder it bound to the
//! connection (ticket subject plus `hello.device`). Rights follow the lower of the member's role
//! in the log and its ticket's role ([`TeamState::effective_role`]), so a downgrade on the
//! website applies at once, and a device counts only when the log lists it.
//!
//! | Check | When |
//! | --- | --- |
//! | [`check_wrap`] | every `key.wrap`, after the epoch check |
//! | [`check_registered_keys`] | every `memberlog.append`, before [`crate::memberlog::LogVerifier::admit`] |

use hangar_team_protocol::limits::MAX_KEY_PACKAGES;
use hangar_team_protocol::wire::{ErrorCode, Holder, KeyWrap, MemberLogAppend, Role};
use thiserror::Error;

use crate::identity::DevicePublicKeys;
use crate::memberlog::{EntryBody, LogError, TeamState};

/// Why a `key.wrap` is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyWrapError {
    /// The sending device is not in the log, wraps for itself (it cannot hold a key it is still
    /// waiting for), or is neither owner nor admin and the target belongs to another member.
    #[error("only an owner, an admin or another device of the same member wraps the team key for a device")]
    Forbidden,
    /// The log does not list the target device.
    #[error("the membership log does not list the device")]
    UnknownDevice,
    /// The sender already stored a package for this device in this epoch.
    #[error("this device already wrapped the team key for that device in this epoch")]
    Duplicate,
    /// The relay already keeps [`MAX_KEY_PACKAGES`] wrapped keys for the device in this epoch.
    /// Stored packages are never replaced (first come, first kept); the device tries each.
    #[error("the device has the most wrapped team keys one epoch keeps")]
    Limit,
}

impl KeyWrapError {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        match self {
            KeyWrapError::Forbidden => ErrorCode::Forbidden,
            KeyWrapError::UnknownDevice => ErrorCode::NotFound,
            KeyWrapError::Duplicate => ErrorCode::Conflict,
            KeyWrapError::Limit => ErrorCode::Limit,
        }
    }
}

/// The relay's check of a `key.wrap`, after the epoch check (`check_epoch`):
///
/// - `state` is the team's verified log, `ticket_role` and `sender` are the connection's.
/// - `wrap.wrapped.holder` names the target device with its member.
/// - `stored_from` lists the senders of the packages the relay already keeps for that target in
///   the current epoch, in the order they came (the `key.rotate` issuer's included).
///
/// The sender must be a device of the log, the target another one. An owner or admin (the
/// lower of log role and ticket role) wraps for any other device, a member only for another
/// device of its own. One package per sender, target and epoch, so a single device cannot take
/// all [`MAX_KEY_PACKAGES`] places; up to that many per target, first come first kept. Rights
/// are checked before the stored packages, so a sender who may not wrap does not learn how many
/// the device has. `notFound` for a target the log does not list reveals nothing: every member
/// reads the plaintext log.
pub fn check_wrap(
    state: &TeamState,
    ticket_role: Role,
    sender: &Holder,
    wrap: &KeyWrap,
    stored_from: &[Holder],
) -> Result<(), KeyWrapError> {
    let target = &wrap.wrapped.holder;
    if state.device(sender).is_none() || sender == target {
        return Err(KeyWrapError::Forbidden);
    }
    let role = state
        .effective_role(&sender.member, ticket_role)
        .ok_or(KeyWrapError::Forbidden)?;
    if !matches!(role, Role::Owner | Role::Admin) && sender.member != target.member {
        return Err(KeyWrapError::Forbidden);
    }
    if state.device(target).is_none() {
        return Err(KeyWrapError::UnknownDevice);
    }
    if stored_from.contains(sender) {
        return Err(KeyWrapError::Duplicate);
    }
    if stored_from.len() >= MAX_KEY_PACKAGES {
        return Err(KeyWrapError::Limit);
    }
    Ok(())
}

/// Whether every device a `memberlog.append` adds (create team, add member, add device) comes
/// with the keys that holder registered over its own authenticated connection (`device.keys`
/// on the connection bound to that holder). `registered` returns those keys; `None` when the
/// holder never sent any.
///
/// The log's proofs of possession already stop keys a device does not hold; this check also
/// stops an approver from writing keys of its own choosing for a holder that then never
/// connects with them, so the relay's directory and the log agree. Recovery devices are
/// skipped: they are derived from a printed code and register nothing until they are used. The
/// entry is decoded without its signature; [`crate::memberlog::LogVerifier::admit`] checks
/// that next. [`LogError::KeysNotRegistered`] answers `forbidden`.
pub fn check_registered_keys(
    append: &MemberLogAppend,
    registered: impl Fn(&Holder) -> Option<DevicePublicKeys>,
) -> Result<(), LogError> {
    let body = EntryBody::decode_unverified(&append.entry)?;
    for (holder, device) in body.listed_devices() {
        if device.recovery {
            continue;
        }
        if registered(&holder) != Some(device.keys) {
            return Err(LogError::KeysNotRegistered);
        }
    }
    Ok(())
}
