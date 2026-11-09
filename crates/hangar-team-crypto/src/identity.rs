//! A device's long-term keys: Ed25519 to sign membership log entries and records, X25519 to
//! receive wrapped epoch secrets (HPKE). The private halves leave this crate only as the
//! versioned, zeroizing byte string of [`DeviceIdentity::export`], meant for the OS keychain.
//!
//! # Secret format (version 1, 67 bytes)
//!
//! ```text
//! "HD" || 0x01 || Ed25519 seed (32 bytes) || X25519 private key (32 bytes, clamped)
//! ```
//!
//! # Safety number
//!
//! Two people compare a device's [`SafetyNumber`] out of band (read aloud, on screen) before
//! one approves the other's new device:
//!
//! ```text
//! d = SHA-256( field("hangar-team/v1/safety-number") || field(member) || field(device)
//!              || field(sign public key) || field(kem public key) )
//! group i (0..6) = (bytes d[5i .. 5i+5] as a 40-bit big endian number) mod 100000
//! ```
//!
//! printed as six groups of five digits. 30 digits are about 99.7 bits: finding other keys with
//! the same number is a second-preimage search of that size. [`log_safety_number`] uses the same
//! digits over a membership log head, so two members can tell they see the same log.

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hangar_team_protocol::e2e::{ChainHead, PublicKey};
use hangar_team_protocol::ids::TeamId;
use hangar_team_protocol::wire::{DeviceKeys, Holder};
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, Serializable};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

use crate::encoding::framed;
use crate::entropy::{Entropy, EntropyError};

pub(crate) type KemPrivateKey = <X25519HkdfSha256 as Kem>::PrivateKey;
pub(crate) type KemPublicKey = <X25519HkdfSha256 as Kem>::PublicKey;

const SECRET_MAGIC: &[u8; 2] = b"HD";
const SECRET_VERSION: u8 = 1;
/// Bytes of an exported device secret.
pub const DEVICE_SECRET_LEN: usize = 2 + 1 + 32 + 32;

const LABEL_SAFETY_NUMBER: &[u8] = b"hangar-team/v1/safety-number";
const LABEL_LOG_SAFETY_NUMBER: &[u8] = b"hangar-team/v1/log-safety-number";

/// Why exported key material was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdentityError {
    /// Not a device secret of this crate (wrong length or prefix).
    #[error("not a Hangar device secret")]
    Format,
    /// A device secret of a format version this crate does not know.
    #[error("device secret format version {0} is not supported")]
    Version(u8),
    /// The key library refused the key material (not expected for 32-byte keys).
    #[error("the key material was refused")]
    Key,
    /// No randomness for a new identity.
    #[error(transparent)]
    Entropy(#[from] EntropyError),
}

/// A device's public keys, as the membership log vouches for them and `device.keys` sends them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DevicePublicKeys {
    /// Ed25519.
    pub sign: PublicKey,
    /// X25519.
    pub kem: PublicKey,
}

impl From<DevicePublicKeys> for DeviceKeys {
    fn from(keys: DevicePublicKeys) -> Self {
        DeviceKeys {
            sign: keys.sign,
            kem: keys.kem,
        }
    }
}

impl From<&DeviceKeys> for DevicePublicKeys {
    fn from(keys: &DeviceKeys) -> Self {
        DevicePublicKeys {
            sign: keys.sign,
            kem: keys.kem,
        }
    }
}

/// A device's private keys. Wiped on drop; `Debug` shows only the public keys.
pub struct DeviceIdentity {
    sign: SigningKey,
    kem: KemPrivateKey,
    public: DevicePublicKeys,
}

impl fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}

impl DeviceIdentity {
    /// A new device: 64 random bytes become the Ed25519 seed and the X25519 private key.
    pub fn generate(entropy: &mut impl Entropy) -> Result<Self, IdentityError> {
        let mut seeds = Zeroizing::new([0u8; 64]);
        entropy.fill(seeds.as_mut())?;
        Self::from_seeds(&seeds)
    }

    /// The identity of 64 bytes of key material: Ed25519 seed, then X25519 private key.
    pub(crate) fn from_seeds(seeds: &[u8; 64]) -> Result<Self, IdentityError> {
        let mut sign_seed = Zeroizing::new([0u8; 32]);
        sign_seed.copy_from_slice(&seeds[..32]);
        let sign = SigningKey::from_bytes(&sign_seed);
        // X25519 clamps any 32 bytes into a valid private key; only a wrong length fails.
        let kem = KemPrivateKey::from_bytes(&seeds[32..]).map_err(|_| IdentityError::Key)?;
        let mut kem_public = X25519HkdfSha256::sk_to_pk(&kem).to_bytes();
        let public = DevicePublicKeys {
            sign: PublicKey::from_bytes(sign.verifying_key().to_bytes()),
            kem: PublicKey::from_bytes(array32(&kem_public)),
        };
        kem_public.as_mut_slice().zeroize();
        Ok(Self { sign, kem, public })
    }

    /// The public keys to publish (`device.keys`) and to put into the membership log.
    pub fn public_keys(&self) -> DevicePublicKeys {
        self.public
    }

    /// The private keys for the OS keychain, in the versioned format of this module.
    pub fn export(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(DEVICE_SECRET_LEN));
        out.extend_from_slice(SECRET_MAGIC);
        out.push(SECRET_VERSION);
        out.extend_from_slice(self.sign.as_bytes());
        let mut kem = self.kem.to_bytes();
        out.extend_from_slice(kem.as_slice());
        kem.as_mut_slice().zeroize();
        out
    }

    /// Reads what [`DeviceIdentity::export`] wrote.
    pub fn import(bytes: &[u8]) -> Result<Self, IdentityError> {
        if bytes.len() < 3 || &bytes[..2] != SECRET_MAGIC {
            return Err(IdentityError::Format);
        }
        if bytes[2] != SECRET_VERSION {
            return Err(IdentityError::Version(bytes[2]));
        }
        if bytes.len() != DEVICE_SECRET_LEN {
            return Err(IdentityError::Format);
        }
        let mut seeds = Zeroizing::new([0u8; 64]);
        seeds.copy_from_slice(&bytes[3..]);
        Self::from_seeds(&seeds)
    }

    pub(crate) fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.sign.sign(message).to_bytes()
    }

    pub(crate) fn kem_private(&self) -> &KemPrivateKey {
        &self.kem
    }
}

/// The first 32 bytes of `bytes` (callers pass exactly 32).
pub(crate) fn array32(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let len = bytes.len().min(32);
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

/// The first 24 bytes of `bytes` (callers pass exactly 24).
pub(crate) fn array24(bytes: &[u8]) -> [u8; 24] {
    let mut out = [0u8; 24];
    let len = bytes.len().min(24);
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

/// Whether `key` is an Ed25519 public key a device may hold: a valid point, not of small order.
pub(crate) fn is_valid_sign_key(key: &PublicKey) -> bool {
    VerifyingKey::from_bytes(key.as_bytes()).is_ok_and(|key| !key.is_weak())
}

/// Strict Ed25519 verification (RFC 8032 with canonical encodings, no small-order keys).
pub(crate) fn verify(key: &PublicKey, message: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(key.as_bytes()) else {
        return false;
    };
    key.verify_strict(message, &Signature::from_bytes(signature))
        .is_ok()
}

/// 30 decimal digits in six groups for comparing keys out of band (derivation: module docs).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SafetyNumber([u32; 6]);

impl SafetyNumber {
    fn from_digest(digest: &[u8; 32]) -> Self {
        let mut groups = [0u32; 6];
        for (i, group) in groups.iter_mut().enumerate() {
            let mut wide = [0u8; 8];
            wide[3..].copy_from_slice(&digest[5 * i..5 * i + 5]);
            // Below 100000, so the conversion cannot fail.
            *group = u32::try_from(u64::from_be_bytes(wide) % 100_000).unwrap_or(0);
        }
        Self(groups)
    }

    /// The six groups of five digits.
    pub fn groups(&self) -> [u32; 6] {
        self.0
    }
}

impl fmt::Display for SafetyNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, group) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{group:05}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for SafetyNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SafetyNumber({self})")
    }
}

/// The safety number of one device (its holder and both public keys).
pub fn safety_number(holder: &Holder, keys: &DevicePublicKeys) -> SafetyNumber {
    let digest = Sha256::digest(framed(&[
        LABEL_SAFETY_NUMBER,
        holder.member.as_str().as_bytes(),
        holder.device.as_str().as_bytes(),
        keys.sign.as_bytes(),
        keys.kem.as_bytes(),
    ]));
    SafetyNumber::from_digest(&digest.into())
}

/// The safety number of a team's membership log at `head`: equal on two devices exactly when
/// both verified the same log up to the same entry (a relay that shows two members different
/// logs is caught by comparing it).
pub fn log_safety_number(team: &TeamId, head: &ChainHead) -> SafetyNumber {
    let digest = Sha256::digest(framed(&[
        LABEL_LOG_SAFETY_NUMBER,
        team.as_str().as_bytes(),
        &head.index.to_be_bytes(),
        head.hash.as_bytes(),
    ]));
    SafetyNumber::from_digest(&digest.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::{FixedEntropy, OsEntropy};
    use hangar_team_protocol::ids::{DeviceId, MemberId};

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn ed25519_matches_rfc_8032_test_1() {
        let mut seeds = [0u8; 64];
        seeds[..32].copy_from_slice(&hex(
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        ));
        let identity = DeviceIdentity::from_seeds(&seeds).expect("keys");
        assert_eq!(
            identity.public_keys().sign.as_bytes().to_vec(),
            hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        );
        let signature = identity.sign(b"");
        assert_eq!(
            signature.to_vec(),
            hex(concat!(
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
                "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
            ))
        );
        assert!(verify(&identity.public_keys().sign, b"", &signature));
        assert!(!verify(&identity.public_keys().sign, b"x", &signature));
    }

    #[test]
    fn x25519_matches_rfc_9180_recipient_key() {
        // RFC 9180 A.2.1: skRm -> pkRm.
        let mut seeds = [0u8; 64];
        seeds[32..].copy_from_slice(&hex(
            "8057991eef8f1f1af18f4a9491d16a1ce333f695d4db8e38da75975c4478e0fb",
        ));
        let identity = DeviceIdentity::from_seeds(&seeds).expect("keys");
        assert_eq!(
            identity.public_keys().kem.as_bytes().to_vec(),
            hex("4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a")
        );
    }

    #[test]
    fn export_round_trips_and_checks_format() {
        let identity = DeviceIdentity::generate(&mut OsEntropy).expect("rng");
        let exported = identity.export();
        assert_eq!(exported.len(), DEVICE_SECRET_LEN);
        let imported = DeviceIdentity::import(&exported).expect("import");
        assert_eq!(imported.public_keys(), identity.public_keys());
        assert_eq!(*imported.export(), *exported);

        assert_eq!(
            DeviceIdentity::import(&exported[..10]).err(),
            Some(IdentityError::Format)
        );
        let mut other = exported.to_vec();
        other[2] = 2;
        assert_eq!(
            DeviceIdentity::import(&other).err(),
            Some(IdentityError::Version(2))
        );
        other[0] = b'X';
        assert_eq!(
            DeviceIdentity::import(&other).err(),
            Some(IdentityError::Format)
        );
    }

    #[test]
    fn debug_shows_no_private_key() {
        let identity = DeviceIdentity::generate(&mut FixedEntropy::counting(0, 64)).expect("rng");
        let debug = format!("{identity:?}");
        let exported = identity.export();
        let seed = data_encoding::HEXLOWER.encode(&exported[3..35]);
        assert!(!debug.contains(&seed));
        assert!(debug.contains("DeviceIdentity"));
    }

    #[test]
    fn safety_number_formats_six_groups() {
        let identity = DeviceIdentity::generate(&mut FixedEntropy::counting(0, 64)).expect("rng");
        let holder = Holder {
            member: MemberId::new("member").expect("id"),
            device: DeviceId::new("device").expect("id"),
        };
        let number = safety_number(&holder, &identity.public_keys());
        let text = number.to_string();
        assert_eq!(text.len(), 6 * 5 + 5);
        assert!(text.split(' ').all(|g| g.len() == 5));
        let other = Holder {
            member: MemberId::new("member").expect("id"),
            device: DeviceId::new("other").expect("id"),
        };
        assert_ne!(number, safety_number(&other, &identity.public_keys()));
    }

    #[test]
    fn weak_sign_keys_are_refused() {
        // The identity point (small order).
        let mut identity_point = [0u8; 32];
        identity_point[0] = 1;
        assert!(!is_valid_sign_key(&PublicKey::from_bytes(identity_point)));
        let identity = DeviceIdentity::generate(&mut OsEntropy).expect("rng");
        assert!(is_valid_sign_key(&identity.public_keys().sign));
    }
}
