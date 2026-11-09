//! Team key epochs: a random 256-bit [`TeamSecret`] per epoch, wrapped to every device with HPKE
//! and committed to in the signed membership log, plus the history link that lets the holder of
//! one epoch's secret recover the previous one.
//!
//! # Commitment
//!
//! ```text
//! commitment = SHA-256( field("hangar-team/v1/epoch-commitment") || field(team)
//!                       || field(epoch, 4 bytes big endian) || field(secret) )
//! ```
//!
//! HPKE base mode does not authenticate the sender: anyone, the relay included, can wrap some
//! secret to a device's public key. A device therefore accepts an unwrapped secret only when it
//! matches the commitment of that epoch in the membership log, which an owner or admin signed.
//!
//! # Wrapped key (version 1, 81 bytes)
//!
//! ```text
//! 0x01 || enc (32 bytes, the ephemeral X25519 public key) || HPKE ciphertext of the secret (48 bytes)
//! ```
//!
//! HPKE (RFC 9180) mode base with DHKEM(X25519, HKDF-SHA256), HKDF-SHA256 and
//! ChaCha20Poly1305; `info = field("hangar-team/v1/wrap") || field(team) || field(epoch) ||
//! field(member) || field(device)` of the recipient, empty associated data. A wrapped key moved to
//! another team, epoch or device does not open.
//!
//! # History link (version 1, 73 bytes)
//!
//! ```text
//! 0x01 || nonce (24 bytes) || XChaCha20-Poly1305(history key, nonce, previous secret, aad) (48 bytes)
//! history key = HKDF-SHA256(salt none, ikm = new secret,
//!                           info = field("hangar-team/v1/history-key") || field(team) || field(new epoch))
//! aad = field("hangar-team/v1/history") || field(team) || field(new epoch)
//! ```
//!
//! The key is derived like the protocol crate's `EpochKeys` (same framing, own label), so the
//! secret itself never keys a cipher. The link cannot bind the rotate entry's log hash, since it
//! is part of that entry; the commitment of the previous epoch it is checked against comes from
//! the verified log.
//!
//! # Epoch keys
//!
//! The matching and sealing keys come from `EpochKeys::derive(secret, team, epoch, rotation)`,
//! `rotation` being the log hash of the epoch's rotate entry. A relay that forks the log gives
//! the two sides different hashes, hence different keys from the same secret.

use std::fmt;

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hangar_team_protocol::e2e::{LogHash, PublicKey};
use hangar_team_protocol::hash::{hkdf_sha256, EpochKeys, KeyError};
use hangar_team_protocol::ids::{Ciphertext, Epoch, TeamId};
use hangar_team_protocol::wire::{Holder, WrappedKey};
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, OpModeR, OpModeS, Serializable};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::encoding::{framed, from_ciphertext, to_ciphertext, Writer};
use crate::entropy::{Entropy, EntropyError, OneShotRng};
use crate::identity::{DeviceIdentity, KemPublicKey};

const LABEL_COMMITMENT: &[u8] = b"hangar-team/v1/epoch-commitment";
const LABEL_WRAP: &[u8] = b"hangar-team/v1/wrap";
const LABEL_HISTORY_KEY: &[u8] = b"hangar-team/v1/history-key";
const LABEL_HISTORY: &[u8] = b"hangar-team/v1/history";

const WRAP_VERSION: u8 = 1;
const HISTORY_VERSION: u8 = 1;
const SECRET_LEN: usize = 32;
const TAG_LEN: usize = 16;
const ENC_LEN: usize = 32;
/// Bytes of a wrapped key.
pub const WRAPPED_KEY_LEN: usize = 1 + ENC_LEN + SECRET_LEN + TAG_LEN;
/// Bytes of a history link.
pub const HISTORY_LINK_LEN: usize = 1 + 24 + SECRET_LEN + TAG_LEN;

/// Why a wrapped key or history link did not give a secret.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WrapError {
    /// Not a wrapped key or history link of this crate (encoding, length).
    #[error("not a wrapped team key")]
    Malformed,
    /// A format version this crate does not know.
    #[error("wrapped key format version {0} is not supported")]
    Version(u8),
    /// HPKE or the AEAD refused: wrapped for another device, team or epoch, or changed.
    #[error("the wrapped team key does not open for this device, team and epoch")]
    Open,
    /// The secret opened but is not the one the membership log committed to for the epoch.
    #[error("the team key does not match the membership log's commitment for epoch {0}")]
    Commitment(u32),
    /// The recipient's public key was refused by HPKE (for example a small-order point).
    #[error("the recipient's key cannot receive a wrapped key")]
    RecipientKey,
    /// The membership log has no such epoch, or no history link for it.
    #[error("the membership log has no key for epoch {0}")]
    UnknownEpoch(u32),
    /// The membership log does not vouch for the target device.
    #[error("the membership log does not vouch for that device")]
    UnknownDevice,
    /// HPKE wanted other randomness than one ephemeral key.
    #[error("unexpected use of randomness while wrapping")]
    Rng,
    /// No randomness.
    #[error(transparent)]
    Entropy(#[from] EntropyError),
    /// The output does not fit a protocol ciphertext.
    #[error("the wrapped key does not fit a protocol ciphertext")]
    TooLarge,
}

/// One epoch's 256-bit secret. Wiped on drop, never printed.
#[derive(Clone)]
pub struct TeamSecret(Zeroizing<[u8; SECRET_LEN]>);

impl PartialEq for TeamSecret {
    fn eq(&self, other: &Self) -> bool {
        crate::encoding::constant_time_eq(self.0.as_ref(), other.0.as_ref())
    }
}

impl Eq for TeamSecret {}

impl fmt::Debug for TeamSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TeamSecret(..)")
    }
}

impl TeamSecret {
    /// A new random secret.
    pub fn generate(entropy: &mut impl Entropy) -> Result<Self, EntropyError> {
        let mut secret = Zeroizing::new([0u8; SECRET_LEN]);
        entropy.fill(secret.as_mut())?;
        Ok(Self(secret))
    }

    /// A secret from the OS keychain (what [`TeamSecret::export`] gave).
    pub fn from_bytes(bytes: Zeroizing<[u8; SECRET_LEN]>) -> Self {
        Self(bytes)
    }

    /// The raw secret for the OS keychain.
    pub fn export(&self) -> Zeroizing<[u8; SECRET_LEN]> {
        self.0.clone()
    }

    /// The matching and sealing keys of `epoch` (`hangar_team_protocol::hash::EpochKeys`);
    /// `rotation` is the membership log hash of the rotate epoch entry that started it.
    pub fn epoch_keys(
        &self,
        team: &TeamId,
        epoch: Epoch,
        rotation: &LogHash,
    ) -> Result<EpochKeys, KeyError> {
        EpochKeys::derive(&self.0, team, epoch, rotation)
    }

    /// What the membership log records for this secret as `epoch` of `team`.
    pub fn commitment(&self, team: &TeamId, epoch: Epoch) -> EpochCommitment {
        let digest = Sha256::digest(framed(&[
            LABEL_COMMITMENT,
            team.as_str().as_bytes(),
            &epoch.0.to_be_bytes(),
            self.0.as_ref(),
        ]));
        EpochCommitment(digest.into())
    }

    pub(crate) fn bytes(&self) -> &[u8; SECRET_LEN] {
        &self.0
    }
}

/// The hash of an epoch's secret in the membership log (see the module docs). Public.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EpochCommitment(pub [u8; 32]);

impl EpochCommitment {
    /// Equality in constant time: one side is computed from a secret the caller holds.
    pub fn matches(&self, other: &EpochCommitment) -> bool {
        crate::encoding::constant_time_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for EpochCommitment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EpochCommitment({})",
            data_encoding::HEXLOWER.encode(&self.0)
        )
    }
}

fn wrap_info(team: &TeamId, epoch: Epoch, recipient: &Holder) -> Vec<u8> {
    framed(&[
        LABEL_WRAP,
        team.as_str().as_bytes(),
        &epoch.0.to_be_bytes(),
        recipient.member.as_str().as_bytes(),
        recipient.device.as_str().as_bytes(),
    ])
}

/// Wraps `secret` (epoch `epoch` of `team`) to the device `recipient` with X25519 key `kem`.
/// The caller takes `kem` from the verified membership log, never from the relay's `devices`.
pub fn wrap(
    secret: &TeamSecret,
    team: &TeamId,
    epoch: Epoch,
    recipient: &Holder,
    kem: &PublicKey,
    entropy: &mut impl Entropy,
) -> Result<WrappedKey, WrapError> {
    let recipient_key =
        KemPublicKey::from_bytes(kem.as_bytes()).map_err(|_| WrapError::RecipientKey)?;
    let mut rng = OneShotRng::draw(entropy)?;
    let (enc, ciphertext) =
        hpke::single_shot_seal_with_rng::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeS::Base,
            &recipient_key,
            &wrap_info(team, epoch, recipient),
            secret.bytes(),
            b"",
            &mut rng,
        )
        .map_err(|_| WrapError::RecipientKey)?;
    if !rng.consumed_exactly_once() {
        return Err(WrapError::Rng);
    }
    let mut bytes = Vec::with_capacity(WRAPPED_KEY_LEN);
    bytes.push(WRAP_VERSION);
    bytes.extend_from_slice(enc.to_bytes().as_slice());
    bytes.extend_from_slice(&ciphertext);
    if bytes.len() != WRAPPED_KEY_LEN {
        return Err(WrapError::Malformed);
    }
    Ok(WrappedKey {
        holder: recipient.clone(),
        data: to_ciphertext(&bytes).map_err(|_| WrapError::TooLarge)?,
    })
}

/// Opens a wrapped key addressed to this device (`me`) for epoch `epoch` of `team` and accepts
/// it only if it matches `commitment`, the epoch's commitment from the verified membership log.
pub fn unwrap(
    identity: &DeviceIdentity,
    team: &TeamId,
    epoch: Epoch,
    me: &Holder,
    data: &Ciphertext,
    commitment: &EpochCommitment,
) -> Result<TeamSecret, WrapError> {
    let bytes = from_ciphertext(data).ok_or(WrapError::Malformed)?;
    let (&version, rest) = bytes.split_first().ok_or(WrapError::Malformed)?;
    if version != WRAP_VERSION {
        return Err(WrapError::Version(version));
    }
    if rest.len() != WRAPPED_KEY_LEN - 1 {
        return Err(WrapError::Malformed);
    }
    let (enc, ciphertext) = rest.split_at(ENC_LEN);
    let enc = <X25519HkdfSha256 as hpke::Kem>::EncappedKey::from_bytes(enc)
        .map_err(|_| WrapError::Malformed)?;
    let plain = Zeroizing::new(
        hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeR::Base,
            identity.kem_private(),
            &enc,
            &wrap_info(team, epoch, me),
            ciphertext,
            b"",
        )
        .map_err(|_| WrapError::Open)?,
    );
    let secret = secret_from(&plain)?;
    if !secret.commitment(team, epoch).matches(commitment) {
        return Err(WrapError::Commitment(epoch.0));
    }
    Ok(secret)
}

/// [`unwrap`] for each package `key.get` returned (the relay keeps up to
/// `MAX_KEY_PACKAGES` per device and epoch, first come first kept): the first that opens and
/// matches `commitment` wins, so a wrong package stored before the real one blocks nothing.
/// With none matching, the error of the last package.
pub fn unwrap_any<'a>(
    identity: &DeviceIdentity,
    team: &TeamId,
    epoch: Epoch,
    me: &Holder,
    packages: impl IntoIterator<Item = &'a Ciphertext>,
    commitment: &EpochCommitment,
) -> Result<TeamSecret, WrapError> {
    let mut last = WrapError::UnknownEpoch(epoch.0);
    for data in packages {
        match unwrap(identity, team, epoch, me, data, commitment) {
            Ok(secret) => return Ok(secret),
            Err(error) => last = error,
        }
    }
    Err(last)
}

fn secret_from(bytes: &[u8]) -> Result<TeamSecret, WrapError> {
    let mut secret = Zeroizing::new([0u8; SECRET_LEN]);
    if bytes.len() != SECRET_LEN {
        return Err(WrapError::Malformed);
    }
    secret.copy_from_slice(bytes);
    Ok(TeamSecret(secret))
}

/// The previous epoch's secret, encrypted under a key derived from the new epoch's secret (see
/// the module docs). Public bytes; part of the rotation's membership log entry.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct HistoryLink(pub(crate) [u8; HISTORY_LINK_LEN]);

impl fmt::Debug for HistoryLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HistoryLink(..)")
    }
}

impl HistoryLink {
    /// The link's bytes as they appear in the membership log entry.
    pub fn as_bytes(&self) -> &[u8; HISTORY_LINK_LEN] {
        &self.0
    }

    pub(crate) fn from_slice(bytes: &[u8]) -> Option<Self> {
        Some(Self(bytes.try_into().ok()?))
    }

    /// Encrypts `previous` under `new`, the secret of epoch `new_epoch`.
    pub fn seal(
        new: &TeamSecret,
        team: &TeamId,
        new_epoch: Epoch,
        previous: &TeamSecret,
        entropy: &mut impl Entropy,
    ) -> Result<Self, WrapError> {
        let mut nonce = [0u8; 24];
        entropy.fill(&mut nonce)?;
        let cipher = history_cipher(new, team, new_epoch)?;
        let sealed = cipher
            .encrypt(
                &XNonce::from(nonce),
                chacha20poly1305::aead::Payload {
                    msg: previous.bytes(),
                    aad: &history_aad(team, new_epoch),
                },
            )
            .map_err(|_| WrapError::Malformed)?;
        let mut out = [0u8; HISTORY_LINK_LEN];
        out[0] = HISTORY_VERSION;
        out[1..25].copy_from_slice(&nonce);
        if sealed.len() != HISTORY_LINK_LEN - 25 {
            return Err(WrapError::Malformed);
        }
        out[25..].copy_from_slice(&sealed);
        Ok(Self(out))
    }

    /// The previous epoch's secret, accepted only if it matches `previous_commitment` (the
    /// membership log's commitment of epoch `new_epoch - 1`).
    pub fn open(
        &self,
        new: &TeamSecret,
        team: &TeamId,
        new_epoch: Epoch,
        previous_commitment: &EpochCommitment,
    ) -> Result<TeamSecret, WrapError> {
        if self.0[0] != HISTORY_VERSION {
            return Err(WrapError::Version(self.0[0]));
        }
        let previous_epoch = new_epoch
            .0
            .checked_sub(1)
            .ok_or(WrapError::UnknownEpoch(0))?;
        let cipher = history_cipher(new, team, new_epoch)?;
        let nonce = XNonce::from(crate::identity::array24(&self.0[1..25]));
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    &nonce,
                    chacha20poly1305::aead::Payload {
                        msg: &self.0[25..],
                        aad: &history_aad(team, new_epoch),
                    },
                )
                .map_err(|_| WrapError::Open)?,
        );
        let previous = secret_from(&plain)?;
        if !previous
            .commitment(team, Epoch(previous_epoch))
            .matches(previous_commitment)
        {
            return Err(WrapError::Commitment(previous_epoch));
        }
        Ok(previous)
    }
}

fn history_cipher(
    secret: &TeamSecret,
    team: &TeamId,
    epoch: Epoch,
) -> Result<XChaCha20Poly1305, WrapError> {
    let hkdf = hkdf_sha256(secret.bytes());
    let mut info = Writer::new();
    info.field(LABEL_HISTORY_KEY)
        .field(team.as_str().as_bytes())
        .field(&epoch.0.to_be_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    hkdf.expand(&info.into_bytes(), key.as_mut())
        .map_err(|_| WrapError::Malformed)?;
    XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| WrapError::Malformed)
}

fn history_aad(team: &TeamId, epoch: Epoch) -> Vec<u8> {
    framed(&[
        LABEL_HISTORY,
        team.as_str().as_bytes(),
        &epoch.0.to_be_bytes(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::{FixedEntropy, OsEntropy};
    use hangar_team_protocol::ids::{DeviceId, MemberId};

    fn holder(member: &str, device: &str) -> Holder {
        Holder {
            member: MemberId::new(member).expect("id"),
            device: DeviceId::new(device).expect("id"),
        }
    }

    fn team() -> TeamId {
        TeamId::new("team-1").expect("id")
    }

    #[test]
    fn wrap_and_unwrap_bind_team_epoch_and_device() {
        let device = DeviceIdentity::generate(&mut OsEntropy).expect("rng");
        let me = holder("m", "d");
        let secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
        let commitment = secret.commitment(&team(), Epoch(3));
        let wrapped = wrap(
            &secret,
            &team(),
            Epoch(3),
            &me,
            &device.public_keys().kem,
            &mut OsEntropy,
        )
        .expect("wrap");
        assert_eq!(wrapped.holder, me);
        assert_eq!(wrapped.data.as_str().len(), 108);
        let opened =
            unwrap(&device, &team(), Epoch(3), &me, &wrapped.data, &commitment).expect("unwrap");
        assert_eq!(opened, secret);

        // Another epoch, team or device: the HPKE info differs, so it does not open.
        assert_eq!(
            unwrap(&device, &team(), Epoch(4), &me, &wrapped.data, &commitment),
            Err(WrapError::Open)
        );
        let other_team = TeamId::new("team-2").expect("id");
        assert_eq!(
            unwrap(
                &device,
                &other_team,
                Epoch(3),
                &me,
                &wrapped.data,
                &commitment
            ),
            Err(WrapError::Open)
        );
        assert_eq!(
            unwrap(
                &device,
                &team(),
                Epoch(3),
                &holder("m", "e"),
                &wrapped.data,
                &commitment
            ),
            Err(WrapError::Open)
        );
        let stranger = DeviceIdentity::generate(&mut OsEntropy).expect("rng");
        assert_eq!(
            unwrap(
                &stranger,
                &team(),
                Epoch(3),
                &me,
                &wrapped.data,
                &commitment
            ),
            Err(WrapError::Open)
        );
    }

    #[test]
    fn a_secret_the_log_did_not_commit_to_is_refused() {
        // The relay can wrap a secret of its own choosing; the commitment catches it.
        let device = DeviceIdentity::generate(&mut OsEntropy).expect("rng");
        let me = holder("m", "d");
        let real = TeamSecret::generate(&mut OsEntropy).expect("rng");
        let forged = TeamSecret::generate(&mut OsEntropy).expect("rng");
        let wrapped = wrap(
            &forged,
            &team(),
            Epoch(1),
            &me,
            &device.public_keys().kem,
            &mut OsEntropy,
        )
        .expect("wrap");
        assert_eq!(
            unwrap(
                &device,
                &team(),
                Epoch(1),
                &me,
                &wrapped.data,
                &real.commitment(&team(), Epoch(1))
            ),
            Err(WrapError::Commitment(1))
        );
    }

    #[test]
    fn malformed_wrapped_keys_are_typed() {
        let device = DeviceIdentity::generate(&mut OsEntropy).expect("rng");
        let me = holder("m", "d");
        let commitment = EpochCommitment([0; 32]);
        let short = to_ciphertext(&[WRAP_VERSION, 1, 2]).expect("ct");
        assert_eq!(
            unwrap(&device, &team(), Epoch(1), &me, &short, &commitment),
            Err(WrapError::Malformed)
        );
        let mut versioned = vec![9u8; WRAPPED_KEY_LEN];
        versioned[0] = 9;
        let versioned = to_ciphertext(&versioned).expect("ct");
        assert_eq!(
            unwrap(&device, &team(), Epoch(1), &me, &versioned, &commitment),
            Err(WrapError::Version(9))
        );
    }

    #[test]
    fn wrapping_fails_cleanly_without_randomness() {
        let device = DeviceIdentity::generate(&mut OsEntropy).expect("rng");
        let secret = TeamSecret::generate(&mut OsEntropy).expect("rng");
        assert_eq!(
            wrap(
                &secret,
                &team(),
                Epoch(1),
                &holder("m", "d"),
                &device.public_keys().kem,
                &mut FixedEntropy::new(vec![1; 8]),
            ),
            Err(WrapError::Entropy(EntropyError))
        );
    }

    #[test]
    fn history_link_opens_the_previous_secret() {
        let old = TeamSecret::generate(&mut OsEntropy).expect("rng");
        let new = TeamSecret::generate(&mut OsEntropy).expect("rng");
        let link = HistoryLink::seal(&new, &team(), Epoch(5), &old, &mut OsEntropy).expect("seal");
        let commitment = old.commitment(&team(), Epoch(4));
        assert_eq!(
            link.open(&new, &team(), Epoch(5), &commitment)
                .expect("open"),
            old
        );
        // Wrong epoch or a secret that is not the new one.
        assert_eq!(
            link.open(&new, &team(), Epoch(6), &commitment),
            Err(WrapError::Open)
        );
        assert_eq!(
            link.open(&old, &team(), Epoch(5), &commitment),
            Err(WrapError::Open)
        );
        assert_eq!(
            link.open(&new, &team(), Epoch(5), &old.commitment(&team(), Epoch(3))),
            Err(WrapError::Commitment(4))
        );
    }

    #[test]
    fn debug_hides_secrets() {
        let secret = TeamSecret::from_bytes(Zeroizing::new([0xab; 32]));
        assert_eq!(format!("{secret:?}"), "TeamSecret(..)");
    }
}
