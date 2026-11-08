//! Text from programs (terminal titles, session names) that Hangar shows outside the terminal:
//! in desktop notifications (notify.rs) and in suggested file names (export.rs).

/// Characters that are not visible themselves but change how the text around them is shown, or
/// let two different texts look the same: every character of Unicode category Cf (format: bidi
/// embeddings, overrides, isolates and marks, zero-width characters, the byte order mark, soft
/// hyphen, Arabic and Syriac signs, interlinear annotation, tags, ...) plus the default-ignorable
/// characters of other categories that render as nothing (combining grapheme joiner, variation
/// selectors, Hangul fillers, Khmer inherent vowels). A terminal title could otherwise reorder a
/// toast so it names another program, or hide part of a file name.
///
/// Table of Unicode 16.0 (`DerivedCoreProperties.txt`, `Default_Ignorable_Code_Point` and
/// category Cf); unassigned code points inside a range are included so a later Unicode version
/// adds nothing visible to them.
pub fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_characters() {
        for c in [
            '\u{00AD}',
            '\u{034F}',
            '\u{0600}',
            '\u{061C}',
            '\u{070F}',
            '\u{115F}',
            '\u{17B4}',
            '\u{180E}',
            '\u{200B}',
            '\u{200D}',
            '\u{200E}',
            '\u{200F}',
            '\u{202A}',
            '\u{202E}',
            '\u{2060}',
            '\u{2066}',
            '\u{2069}',
            '\u{206F}',
            '\u{3164}',
            '\u{FE00}',
            '\u{FE0F}',
            '\u{FEFF}',
            '\u{FFA0}',
            '\u{FFF9}',
            '\u{110BD}',
            '\u{13430}',
            '\u{1BCA0}',
            '\u{1D173}',
            '\u{E0000}',
            '\u{E0001}',
            '\u{E0041}',
            '\u{E007F}',
            '\u{E0100}',
            '\u{E01EF}',
        ] {
            assert!(is_invisible(c), "U+{:04X}", u32::from(c));
        }
        for c in [
            'a',
            ' ',
            'ä',
            '\u{2010}',
            '\u{2028}',
            '\u{FFFD}',
            '\u{1F600}',
            '\u{0300}',
            '\u{0627}',
            '\u{4E2D}',
            '\u{FE10}',
            '\u{FFFC}',
        ] {
            assert!(!is_invisible(c), "U+{:04X}", u32::from(c));
        }
    }
}
