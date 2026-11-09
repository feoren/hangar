//! Turning a verified [`TeamState`] into the protocol messages of a key change: a new epoch for
//! every current device (`memberlog.append` with the rotate epoch entry, then `key.rotate`), or
//! the current epoch's key for one newly approved device (`key.wrap`).
//!
//! Order on the wire: the rotate epoch entry is appended first, so every device can check the
//! wrapped key it receives against the logged commitment; then `key.rotate`. If the relay refuses
//! `key.rotate` after the entry was appended, the app repeats `key.rotate` for the same epoch with
//! [`wrap_to_all`] and the secret it kept; it does not plan a second rotation.

use hangar_team_protocol::ids::{Epoch, UnixMillis};
use hangar_team_protocol::limits::MAX_WRAPPED_KEYS;
use hangar_team_protocol::wire::{Holder, KeyRotate, KeyWrap, MemberLogAppend, WrappedKey};
use thiserror::Error;

use crate::entropy::Entropy;
use crate::epoch::{wrap, HistoryLink, TeamSecret, WrapError};
use crate::identity::DeviceIdentity;
use crate::memberlog::{EntryKind, LogError, TeamState};

/// Why no rotation was planned.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RotationError {
    /// The log would refuse the entry (role, epoch order) or the identity is not the issuer's.
    #[error(transparent)]
    Log(#[from] LogError),
    /// Wrapping failed (a device key HPKE refuses, no randomness, a previous secret that is not
    /// the current epoch's).
    #[error(transparent)]
    Wrap(#[from] WrapError),
    /// More devices than one `key.rotate` carries.
    #[error("the team has more devices than one key rotation can carry")]
    TooManyDevices,
}

/// Everything a rotation needs: append `append`, then send `rotate`, then keep `secret` (in the
/// OS keychain) as the key of `epoch`.
#[derive(Debug)]
pub struct Rotation {
    /// The new epoch.
    pub epoch: Epoch,
    /// Its secret, for the OS keychain.
    pub secret: TeamSecret,
    /// The signed rotate epoch entry, sent first.
    pub append: MemberLogAppend,
    /// The secret wrapped to every device, sent after the entry was appended.
    pub rotate: KeyRotate,
}

/// `secret` of `epoch` wrapped to every device the log vouches for now (recovery devices
/// included, so a recovery code later restores this epoch too).
pub fn wrap_to_all(
    state: &TeamState,
    secret: &TeamSecret,
    epoch: Epoch,
    entropy: &mut impl Entropy,
) -> Result<Vec<WrappedKey>, RotationError> {
    if state.devices().count() > MAX_WRAPPED_KEYS {
        return Err(RotationError::TooManyDevices);
    }
    state
        .devices()
        .map(|(holder, device)| {
            wrap(
                secret,
                state.team(),
                epoch,
                &holder,
                &device.keys.kem,
                entropy,
            )
            .map_err(RotationError::from)
        })
        .collect()
}

/// Plans the next epoch, issued by `identity` as device `issuer` (an owner's or admin's).
/// `previous`: the current epoch's secret to link into the new epoch (history granted to whoever
/// receives the new key, including members added later); `None` starts without history.
pub fn plan_rotation(
    state: &TeamState,
    identity: &DeviceIdentity,
    issuer: &Holder,
    previous: Option<&TeamSecret>,
    issued_at: UnixMillis,
    entropy: &mut impl Entropy,
) -> Result<Rotation, RotationError> {
    let issuer_device = state.device(issuer).ok_or(LogError::UnknownIssuer)?;
    if issuer_device.keys != identity.public_keys() {
        return Err(LogError::UnknownIssuer.into());
    }
    let epoch = match state.current_epoch() {
        None => Epoch(1),
        Some(current) => Epoch(current.epoch.0.checked_add(1).ok_or(LogError::EpochOrder {
            expected: u32::MAX,
            got: u32::MAX,
        })?),
    };
    let secret = TeamSecret::generate(entropy).map_err(WrapError::from)?;
    let history = match (previous, state.current_epoch()) {
        (None, _) => None,
        (Some(_), None) => return Err(LogError::NoPreviousEpoch.into()),
        (Some(previous), Some(current)) => {
            if previous.commitment(state.team(), current.epoch) != current.commitment {
                return Err(WrapError::Commitment(current.epoch.0).into());
            }
            Some(HistoryLink::seal(
                &secret,
                state.team(),
                epoch,
                previous,
                entropy,
            )?)
        }
    };
    let body = state.next_body(
        issuer.clone(),
        issued_at,
        EntryKind::RotateEpoch {
            epoch,
            commitment: secret.commitment(state.team(), epoch),
            history,
        },
    );
    state.check(&body)?;
    let wrapped = wrap_to_all(state, &secret, epoch, entropy)?;
    let append = body.sign(identity)?;
    Ok(Rotation {
        epoch,
        secret,
        append,
        rotate: KeyRotate { epoch, wrapped },
    })
}

/// The current epoch's key for one device the log vouches for (`key.wrap`, after its add device
/// or add member entry). `secret` must be the current epoch's.
pub fn wrap_for_device(
    state: &TeamState,
    secret: &TeamSecret,
    target: &Holder,
    entropy: &mut impl Entropy,
) -> Result<KeyWrap, WrapError> {
    let current = state.current_epoch().ok_or(WrapError::UnknownEpoch(0))?;
    if secret.commitment(state.team(), current.epoch) != current.commitment {
        return Err(WrapError::Commitment(current.epoch.0));
    }
    let device = state.device(target).ok_or(WrapError::UnknownDevice)?;
    Ok(KeyWrap {
        epoch: current.epoch,
        wrapped: wrap(
            secret,
            state.team(),
            current.epoch,
            target,
            &device.keys.kem,
            entropy,
        )?,
    })
}
