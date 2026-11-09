//! Byte framing shared by every hash, signature and associated data of this crate, and the
//! canonical base64url of protocol ciphertexts.
//!
//! `field(x)` is `x`'s length as a 32-bit big endian number followed by `x`, the same framing as
//! `hangar_team_protocol::hash`, so no two different field lists form the same byte string.

use data_encoding::BASE64URL_NOPAD;
use hangar_team_protocol::ids::{Ciphertext, InvalidValue};

/// Lengths are bounded far below 4 GiB by the protocol limits; saturate rather than wrap (the
/// reader then refuses the field, since the bytes cannot follow).
fn length_prefix(len: usize) -> [u8; 4] {
    u32::try_from(len).unwrap_or(u32::MAX).to_be_bytes()
}

/// Builds a sequence of length-prefixed fields.
#[derive(Default)]
pub(crate) struct Writer(Vec<u8>);

impl Writer {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn field(&mut self, part: &[u8]) -> &mut Self {
        self.0.extend_from_slice(&length_prefix(part.len()));
        self.0.extend_from_slice(part);
        self
    }

    pub(crate) fn u8(&mut self, value: u8) -> &mut Self {
        self.field(&[value])
    }

    pub(crate) fn u32(&mut self, value: u32) -> &mut Self {
        self.field(&value.to_be_bytes())
    }

    pub(crate) fn u64(&mut self, value: u64) -> &mut Self {
        self.field(&value.to_be_bytes())
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// `field(p1) || field(p2) || ...`
pub(crate) fn framed(parts: &[&[u8]]) -> Vec<u8> {
    let mut writer = Writer::new();
    for part in parts {
        writer.field(part);
    }
    writer.into_bytes()
}

/// Reads what [`Writer`] wrote. Every method returns `None` on a short or malformed input.
pub(crate) struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }

    pub(crate) fn field(&mut self) -> Option<&'a [u8]> {
        let (prefix, rest) = self.rest.split_first_chunk::<4>()?;
        let len = usize::try_from(u32::from_be_bytes(*prefix)).ok()?;
        if rest.len() < len {
            return None;
        }
        let (part, rest) = rest.split_at(len);
        self.rest = rest;
        Some(part)
    }

    pub(crate) fn fixed<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.field()?.try_into().ok()
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        Some(self.fixed::<1>()?[0])
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.fixed()?))
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.fixed()?))
    }

    pub(crate) fn text(&mut self) -> Option<&'a str> {
        std::str::from_utf8(self.field()?).ok()
    }

    /// The input is used up; trailing bytes make an encoding non-canonical.
    pub(crate) fn finish(self) -> Option<()> {
        self.rest.is_empty().then_some(())
    }
}

/// Equality of secret bytes in constant time (`subtle`), so the time taken does not reveal how
/// many leading bytes match. Only the length, which is public here, may end it early.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    bool::from(a.ct_eq(b))
}

/// The bytes as a protocol ciphertext (base64url without padding).
pub(crate) fn to_ciphertext(bytes: &[u8]) -> Result<Ciphertext, InvalidValue> {
    Ciphertext::new(BASE64URL_NOPAD.encode(bytes))
}

/// The bytes of a ciphertext; `None` unless it is canonical base64url without padding.
pub(crate) fn from_ciphertext(text: &Ciphertext) -> Option<Vec<u8>> {
    BASE64URL_NOPAD.decode(text.as_str().as_bytes()).ok()
}

/// Characters of `len` bytes as base64url without padding.
pub(crate) fn base64url_len(len: usize) -> usize {
    BASE64URL_NOPAD.encode_len(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip_and_reject_trailing_bytes() {
        let mut writer = Writer::new();
        writer.field(b"ab").u8(7).u32(9).u64(11).field(b"");
        let bytes = writer.into_bytes();
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.field(), Some(&b"ab"[..]));
        assert_eq!(reader.u8(), Some(7));
        assert_eq!(reader.u32(), Some(9));
        assert_eq!(reader.u64(), Some(11));
        assert_eq!(reader.field(), Some(&b""[..]));
        assert_eq!(reader.finish(), Some(()));

        let mut longer = bytes.clone();
        longer.push(0);
        let mut reader = Reader::new(&longer);
        for _ in 0..5 {
            assert!(reader.field().is_some());
        }
        assert_eq!(reader.finish(), None);
        // A length prefix that promises more than is there.
        assert_eq!(Reader::new(&[0, 0, 0, 5, 1]).field(), None);
        // A fixed-size field of another size.
        assert_eq!(Reader::new(&framed(&[b"abc"])).u32(), None);
    }

    #[test]
    fn base64url_is_canonical() {
        let ciphertext = to_ciphertext(&[0xff, 0x00]).expect("ciphertext");
        assert_eq!(ciphertext.as_str(), "_wA");
        assert_eq!(from_ciphertext(&ciphertext), Some(vec![0xff, 0x00]));
        // Non-zero unused trailing bits: a second spelling of the same bytes.
        let other = Ciphertext::new("_wB").expect("alphabet");
        assert_eq!(from_ciphertext(&other), None);
        assert_eq!(base64url_len(2), 3);
    }
}
