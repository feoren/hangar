//! Where randomness comes from. Every function that needs random bytes takes an [`Entropy`]
//! source: [`OsEntropy`] in the app, a fixed byte sequence in the test vectors.

use std::convert::Infallible;

use hpke::rand_core::{TryCryptoRng, TryRng};
use thiserror::Error;
use zeroize::Zeroizing;

/// The random number generator failed; nothing was generated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the random number generator failed")]
pub struct EntropyError;

/// A source of cryptographically secure random bytes.
pub trait Entropy {
    /// Fills `dest` completely or fails.
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), EntropyError>;
}

/// The operating system's generator (`getrandom`: `BCryptGenRandom` on Windows, `getrandom` or
/// `getentropy` on Linux and macOS).
#[derive(Debug, Clone, Copy, Default)]
pub struct OsEntropy;

impl Entropy for OsEntropy {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), EntropyError> {
        getrandom::fill(dest).map_err(|_| EntropyError)
    }
}

/// Hands HPKE the 32 bytes of keying material for its ephemeral key, drawn beforehand from an
/// [`Entropy`] source, so a failing generator is an error instead of a panic inside `hpke`.
///
/// `hpke` 0.15 asks for exactly one 32-byte block per encapsulation with X25519
/// (`Kem::gen_keypair_with_rng`, which then runs `DeriveKeyPair`, RFC 9180 section 7.1.3). Any
/// other request marks the generator overdrawn and the caller refuses the result.
pub(crate) struct OneShotRng {
    ikm: Zeroizing<[u8; 32]>,
    used: bool,
    overdrawn: bool,
}

impl OneShotRng {
    pub(crate) fn draw(entropy: &mut impl Entropy) -> Result<Self, EntropyError> {
        let mut ikm = Zeroizing::new([0u8; 32]);
        entropy.fill(ikm.as_mut())?;
        Ok(Self {
            ikm,
            used: false,
            overdrawn: false,
        })
    }

    /// Exactly the one 32-byte block was taken.
    pub(crate) fn consumed_exactly_once(&self) -> bool {
        self.used && !self.overdrawn
    }
}

impl TryRng for OneShotRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        self.overdrawn = true;
        Ok(0)
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        self.overdrawn = true;
        Ok(0)
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        if !self.used && dst.len() == self.ikm.len() {
            dst.copy_from_slice(self.ikm.as_ref());
            self.used = true;
        } else {
            self.overdrawn = true;
            dst.fill(0);
        }
        Ok(())
    }
}

impl TryCryptoRng for OneShotRng {}

/// Test entropy: hands out a fixed byte sequence in order and fails when it runs out.
#[cfg(test)]
pub(crate) struct FixedEntropy {
    bytes: Vec<u8>,
    at: usize,
}

#[cfg(test)]
impl FixedEntropy {
    pub(crate) fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            at: 0,
        }
    }

    /// `len` bytes counting up from `start` (wrapping), a simple reproducible pattern.
    pub(crate) fn counting(start: u8, len: usize) -> Self {
        Self::new(
            (0..len)
                .map(|i| start.wrapping_add(i as u8))
                .collect::<Vec<_>>(),
        )
    }
}

#[cfg(test)]
impl Entropy for FixedEntropy {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), EntropyError> {
        let end = self.at.checked_add(dest.len()).ok_or(EntropyError)?;
        let source = self.bytes.get(self.at..end).ok_or(EntropyError)?;
        dest.copy_from_slice(source);
        self.at = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_entropy_fills() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        OsEntropy.fill(&mut a).expect("os rng");
        OsEntropy.fill(&mut b).expect("os rng");
        assert_ne!(a, b);
    }

    #[test]
    fn one_shot_rng_serves_one_block() {
        let mut rng = OneShotRng::draw(&mut FixedEntropy::counting(1, 32)).expect("draw");
        let mut block = [0u8; 32];
        rng.try_fill_bytes(&mut block).expect("infallible");
        assert_eq!(block[0], 1);
        assert!(rng.consumed_exactly_once());
        rng.try_fill_bytes(&mut block).expect("infallible");
        assert_eq!(block, [0u8; 32]);
        assert!(!rng.consumed_exactly_once());
    }

    #[test]
    fn fixed_entropy_runs_out() {
        let mut entropy = FixedEntropy::counting(0, 4);
        let mut dest = [0u8; 3];
        entropy.fill(&mut dest).expect("enough");
        assert_eq!(entropy.fill(&mut dest), Err(EntropyError));
    }
}
