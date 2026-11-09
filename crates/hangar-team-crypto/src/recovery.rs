//! The printable recovery code: 128 random bits that deterministically derive an offline device
//! identity. No escrow: neither the relay nor an admin holds it.
//!
//! # Code (version 1)
//!
//! ```text
//! payload  = 0x01 || entropy (16 bytes) || checksum (3 bytes)
//! checksum = SHA-256( field("hangar-team/v1/recovery-code") || field(0x01) || field(entropy) )[0..3]
//! code     = Crockford base32 of payload (20 bytes -> 32 characters), in 8 groups of 4 joined by "-"
//! ```
//!
//! Crockford's alphabet (`0123456789ABCDEFGHJKMNPQRSTVWXYZ`) has no `I`, `L`, `O` or `U`; reading
//! accepts lower case, ignores spaces and hyphens, and reads `O` as `0`, `I` and `L` as `1`. The
//! checksum catches typos (a wrong code passes with probability 2^-24) before a recovery is
//! attempted.
//!
//! # Identity
//!
//! ```text
//! okm = HKDF-SHA256(salt none, ikm = entropy, info = field("hangar-team/v1/recovery-identity"), 64 bytes)
//! Ed25519 seed = okm[0..32], X25519 private key = okm[32..64]
//! ```
//!
//! 128 bits suffice: the recovery device's public keys are in the membership log, so an attacker
//! can test guesses offline, and 2^128 guesses are out of reach.
//!
//! # Flow
//!
//! 1. Hangar generates the code, shows it once for printing, derives the identity and adds it to
//!    the log like a device (`recovery: true`) of the member, with the recovery identity's proof
//!    of possession and an endorsement of one of the member's devices. The code is never
//!    stored.
//! 2. Every rotation wraps the epoch key to the recovery device as well (it is a device of the
//!    log), so the relay keeps a wrapped key for it.
//! 3. To recover, the person enters the code on a new installation: Hangar derives the identity,
//!    finds its holder in the verified log ([`crate::memberlog::TeamState::find_device`]), signs in
//!    as that device and fetches its wrapped keys (`key.get`), history links giving older epochs.
//! 4. The recovery device itself adds the new installation as a regular device of its member
//!    (the one kind of entry a recovery device may issue besides removing its member's other
//!    devices); the new device removes the used recovery device, and an owner or admin rotates
//!    (the removal makes a rotation due). The person prints a fresh code.
//!
//! A recovery device signs nothing else: no other log entry, and records it signs are refused
//! (`EpochCipher::open_verified`), so a stolen code cannot speak for the member in the team.

use std::fmt;

use hangar_team_protocol::hash::hkdf_sha256;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::encoding::framed;
use crate::entropy::{Entropy, EntropyError};
use crate::identity::{DeviceIdentity, IdentityError};

const LABEL_CODE: &[u8] = b"hangar-team/v1/recovery-code";
const LABEL_IDENTITY: &[u8] = b"hangar-team/v1/recovery-identity";
const CODE_VERSION: u8 = 1;
const ENTROPY_LEN: usize = 16;
const PAYLOAD_LEN: usize = 1 + ENTROPY_LEN + 3;
const CHARS: usize = PAYLOAD_LEN * 8 / 5;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Why a typed recovery code was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecoveryError {
    /// Characters outside the alphabet or the wrong number of them.
    #[error("the recovery code has the wrong length or characters")]
    Format,
    /// A code of a version this crate does not know.
    #[error("recovery code version {0} is not supported")]
    Version(u8),
    /// The checksum does not match: a typo.
    #[error("the recovery code contains a typo")]
    Checksum,
}

/// A recovery code. Wiped on drop; `Debug` hides it, and it has no `Display` so it is not
/// printed by accident: [`RecoveryCode::reveal`] gives the text to show once.
#[derive(Clone)]
pub struct RecoveryCode(Zeroizing<[u8; ENTROPY_LEN]>);

impl PartialEq for RecoveryCode {
    fn eq(&self, other: &Self) -> bool {
        crate::encoding::constant_time_eq(self.0.as_ref(), other.0.as_ref())
    }
}

impl Eq for RecoveryCode {}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode(..)")
    }
}

fn checksum(entropy: &[u8; ENTROPY_LEN]) -> [u8; 3] {
    let digest = Sha256::digest(framed(&[LABEL_CODE, &[CODE_VERSION], entropy]));
    [digest[0], digest[1], digest[2]]
}

fn symbol_value(c: char) -> Option<u8> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    let value = ALPHABET.iter().position(|&a| char::from(a) == c)?;
    u8::try_from(value).ok()
}

impl RecoveryCode {
    /// A new random code.
    pub fn generate(entropy: &mut impl Entropy) -> Result<Self, EntropyError> {
        let mut bytes = Zeroizing::new([0u8; ENTROPY_LEN]);
        entropy.fill(bytes.as_mut())?;
        Ok(Self(bytes))
    }

    /// The code as text to show once: 8 groups of 4 characters.
    pub fn reveal(&self) -> Zeroizing<String> {
        let mut payload = Zeroizing::new([0u8; PAYLOAD_LEN]);
        payload[0] = CODE_VERSION;
        payload[1..1 + ENTROPY_LEN].copy_from_slice(self.0.as_ref());
        payload[1 + ENTROPY_LEN..].copy_from_slice(&checksum(&self.0));
        let mut out = Zeroizing::new(String::with_capacity(CHARS + CHARS / 4));
        let mut acc: u64 = 0;
        let mut bits = 0u32;
        let mut written = 0usize;
        for &byte in payload.iter() {
            acc = (acc << 8) | u64::from(byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                let index = usize::try_from((acc >> bits) & 0x1f).unwrap_or(0);
                if written > 0 && written.is_multiple_of(4) {
                    out.push('-');
                }
                out.push(char::from(ALPHABET[index]));
                written += 1;
            }
            acc &= (1 << bits) - 1;
        }
        out
    }

    /// Reads a typed code (case, spaces, hyphens and look-alike letters forgiven).
    pub fn parse(text: &str) -> Result<Self, RecoveryError> {
        let mut payload = Zeroizing::new([0u8; PAYLOAD_LEN]);
        let mut acc: u64 = 0;
        let mut bits = 0u32;
        let mut at = 0usize;
        let mut symbols = 0usize;
        for c in text.chars().filter(|c| !matches!(c, ' ' | '-')) {
            let value = symbol_value(c).ok_or(RecoveryError::Format)?;
            symbols += 1;
            if symbols > CHARS {
                return Err(RecoveryError::Format);
            }
            acc = (acc << 5) | u64::from(value);
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                payload[at] = u8::try_from((acc >> bits) & 0xff).unwrap_or(0);
                at += 1;
            }
            acc &= (1 << bits) - 1;
        }
        if symbols != CHARS {
            return Err(RecoveryError::Format);
        }
        if payload[0] != CODE_VERSION {
            return Err(RecoveryError::Version(payload[0]));
        }
        let mut entropy = Zeroizing::new([0u8; ENTROPY_LEN]);
        entropy.copy_from_slice(&payload[1..1 + ENTROPY_LEN]);
        if checksum(&entropy) != payload[1 + ENTROPY_LEN..] {
            return Err(RecoveryError::Checksum);
        }
        Ok(Self(entropy))
    }

    /// The offline device identity this code stands for.
    pub fn identity(&self) -> Result<DeviceIdentity, IdentityError> {
        let hkdf = hkdf_sha256(self.0.as_ref());
        let mut okm = Zeroizing::new([0u8; 64]);
        hkdf.expand(&framed(&[LABEL_IDENTITY]), okm.as_mut())
            .map_err(|_| IdentityError::Key)?;
        DeviceIdentity::from_seeds(&okm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::{FixedEntropy, OsEntropy};

    #[test]
    fn code_round_trips_and_derives_the_same_identity() {
        let code = RecoveryCode::generate(&mut OsEntropy).expect("rng");
        let text = code.reveal();
        assert_eq!(text.len(), 32 + 7);
        assert_eq!(text.split('-').count(), 8);
        let parsed = RecoveryCode::parse(&text).expect("parse");
        assert_eq!(parsed, code);
        assert_eq!(
            parsed.identity().expect("id").public_keys(),
            code.identity().expect("id").public_keys()
        );
    }

    #[test]
    fn parsing_forgives_case_spaces_and_look_alikes() {
        let code = RecoveryCode::generate(&mut FixedEntropy::counting(0, 16)).expect("rng");
        let text = code.reveal().replace('-', " ").to_lowercase();
        assert_eq!(RecoveryCode::parse(&text).expect("parse"), code);
        let look_alike = code.reveal().replace('0', "O").replace('1', "l");
        assert_eq!(RecoveryCode::parse(&look_alike).expect("parse"), code);
    }

    #[test]
    fn typos_and_bad_codes_are_typed() {
        let code = RecoveryCode::generate(&mut FixedEntropy::counting(0, 16)).expect("rng");
        let text = code.reveal().to_string();
        // Change one character of the entropy part.
        let mut chars: Vec<char> = text.chars().collect();
        chars[10] = if chars[10] == '2' { '3' } else { '2' };
        let typo: String = chars.into_iter().collect();
        assert_eq!(RecoveryCode::parse(&typo), Err(RecoveryError::Checksum));
        assert_eq!(RecoveryCode::parse(&text[..20]), Err(RecoveryError::Format));
        assert_eq!(
            RecoveryCode::parse(&format!("{text}0")),
            Err(RecoveryError::Format)
        );
        assert_eq!(
            RecoveryCode::parse(&text.replace(&text[..1], "U")),
            Err(RecoveryError::Format)
        );
        // Version 2: the first character carries the version's top bits.
        let other = format!("2{}", &text[1..]);
        assert!(matches!(
            RecoveryCode::parse(&other),
            Err(RecoveryError::Version(_))
        ));
    }

    #[test]
    fn debug_hides_the_code() {
        let code = RecoveryCode::generate(&mut OsEntropy).expect("rng");
        assert_eq!(format!("{code:?}"), "RecoveryCode(..)");
    }
}
