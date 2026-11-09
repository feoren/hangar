//! Character classes the protocol refuses in names and identifiers. The standard library knows
//! control characters (`Cc`) but not the other invisible classes, so the format characters are
//! listed here; `test_vectors.py` checks the table against Python's `unicodedata`.

/// General category `Cf` (format) as of Unicode 16.0, as inclusive ranges in ascending order.
/// These characters are invisible or change the direction of text (U+202E), so two names that
/// look alike could hash differently and a log line could be shown reordered.
pub(crate) const FORMAT_RANGES: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

/// A format character (`Cf`).
pub(crate) fn is_format(c: char) -> bool {
    let code = u32::from(c);
    FORMAT_RANGES
        .binary_search_by(|&(low, high)| {
            if high < code {
                std::cmp::Ordering::Less
            } else if low > code {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Control (`Cc`), format (`Cf`), line separator (`Zl`) or paragraph separator (`Zp`): never
/// part of a name or an identifier the protocol carries.
pub(crate) fn is_hidden(c: char) -> bool {
    c.is_control() || is_format(c) || c == '\u{2028}' || c == '\u{2029}'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ranges_are_sorted_and_disjoint() {
        for pair in FORMAT_RANGES.windows(2) {
            assert!(pair[0].0 <= pair[0].1);
            assert!(pair[0].1 < pair[1].0, "{pair:?}");
        }
    }

    #[test]
    fn hidden_characters() {
        for c in [
            '\u{0}',
            '\n',
            '\u{7f}',
            '\u{ad}',
            '\u{200b}',
            '\u{200d}',
            '\u{202e}',
            '\u{2066}',
            '\u{feff}',
            '\u{e0001}',
            '\u{e007f}',
            '\u{2028}',
            '\u{2029}',
        ] {
            assert!(is_hidden(c), "{:04x}", u32::from(c));
        }
        for c in [
            'a',
            ' ',
            'ä',
            '\u{2027}',
            '\u{202f}',
            '\u{e0080}',
            '\u{1f600}',
        ] {
            assert!(!is_hidden(c), "{:04x}", u32::from(c));
        }
    }
}
