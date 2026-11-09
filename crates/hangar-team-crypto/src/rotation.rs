//! Turning a verified [`TeamState`] into the protocol messages of a key change: a new epoch for
//! every current device (`key.rotate`, which carries the signed rotate epoch entry and the
//! wrapped keys together), or the current epoch's key for one newly approved device
//! (`key.wrap`).
//!
//! Order: the app stores [`Rotation::secret`] in the OS keychain as the key of
//! [`Rotation::epoch`] **before** it sends `key.rotate`, so a crash after the relay accepted the
//! rotation never loses the only copy of the new secret. The relay accepts the entry and the
//! wrapped keys together or neither (`LogVerifier::admit_rotation`); a refused `key.rotate`
//! (another rotation came first) leaves nothing behind, and the app plans again on top of the
//! newer log and forgets the unused secret.

use hangar_team_protocol::ids::{Epoch, UnixMillis};
use hangar_team_protocol::limits::MAX_WRAPPED_KEYS;
use hangar_team_protocol::wire::{Holder, KeyRotate, KeyWrap, WrappedKey};
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
    /// No randomness for the secret or the history link, or a previous secret that is not the
    /// current epoch's.
    #[error(transparent)]
    Wrap(#[from] WrapError),
    /// More devices than one `key.rotate` carries.
    #[error("the team has more devices than one key rotation can carry")]
    TooManyDevices,
}

/// The secret wrapped to every device that could take it, and the devices that could not.
#[derive(Debug, Default)]
pub struct WrapOutcome {
    /// One wrapped key per device that worked.
    pub wrapped: Vec<WrappedKey>,
    /// The devices wrapping failed for, with the reason. The app shows them, and hands them the
    /// key later with `key.wrap` once the cause is gone (randomness, for example); the log
    /// refuses keys HPKE cannot use, so a key itself is not expected to fail.
    pub failed: Vec<(Holder, WrapError)>,
}

/// Everything a rotation needs: keep `secret` (in the OS keychain) as the key of `epoch`, then
/// send `rotate`.
#[derive(Debug)]
pub struct Rotation {
    /// The new epoch.
    pub epoch: Epoch,
    /// Its secret, for the OS keychain; stored before `rotate` is sent.
    pub secret: TeamSecret,
    /// `key.rotate`: the signed rotate epoch entry and the wrapped keys.
    pub rotate: KeyRotate,
    /// Devices whose key could not be wrapped (see [`WrapOutcome::failed`]).
    pub failed: Vec<(Holder, WrapError)>,
}

/// `secret` of `epoch` wrapped to every device the log vouches for now (recovery devices
/// included, so a recovery code later restores this epoch too). A failure for one device does
/// not stop the others.
pub fn wrap_to_all(
    state: &TeamState,
    secret: &TeamSecret,
    epoch: Epoch,
    entropy: &mut impl Entropy,
) -> Result<WrapOutcome, RotationError> {
    if state.devices().count() > MAX_WRAPPED_KEYS {
        return Err(RotationError::TooManyDevices);
    }
    let mut outcome = WrapOutcome::default();
    for (holder, device) in state.devices() {
        match wrap(
            secret,
            state.team(),
            epoch,
            &holder,
            &device.keys.kem,
            entropy,
        ) {
            Ok(wrapped) => outcome.wrapped.push(wrapped),
            Err(error) => outcome.failed.push((holder, error)),
        }
    }
    Ok(outcome)
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
            if !previous
                .commitment(state.team(), current.epoch)
                .matches(&current.commitment)
            {
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
    let outcome = wrap_to_all(state, &secret, epoch, entropy)?;
    let log = body.sign(identity)?;
    Ok(Rotation {
        epoch,
        secret,
        rotate: KeyRotate {
            epoch,
            log,
            wrapped: outcome.wrapped,
        },
        failed: outcome.failed,
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
    if !secret
        .commitment(state.team(), current.epoch)
        .matches(&current.commitment)
    {
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
