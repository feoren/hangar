//! Saving a session's content as a file (contract:
//! docs/wiki/pages/syntheses/agent-workflow-features.md, "Export").
//!
//! The window serializes the terminal (text or HTML) and names a suggestion; the backend opens
//! the save dialog itself, so the window never chooses where a file goes, and writes the file.
//! This module holds the parts without a dialog: format, file name, size limit, writing.

use std::path::Path;

use serde::{Deserialize, Serialize, Serializer};

/// Upper limit of the exported content in UTF-8 bytes.
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
/// File name without extension, in characters.
const MAX_STEM_CHARS: usize = 100;
const FALLBACK_STEM: &str = "session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Text,
    Html,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Html => "html",
        }
    }

    /// Name of the dialog's file type filter.
    pub fn filter_name(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Html => "HTML",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("Too large to export: {size} bytes, at most {max} allowed")]
    TooLarge { size: usize, max: usize },
    #[error("File {path} cannot be written: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("The chosen location is not a file on this computer")]
    NotAPath,
    #[error("Background task aborted: {0}")]
    Task(String),
}

/// Errors reach the window as text; none contains the content.
impl Serialize for ExportError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub fn check_size(content: &str) -> Result<(), ExportError> {
    if content.len() > MAX_BYTES {
        return Err(ExportError::TooLarge {
            size: content.len(),
            max: MAX_BYTES,
        });
    }
    Ok(())
}

/// Device names Windows reserves in every folder, with or without extension.
fn is_reserved_on_windows(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    let base = upper.split('.').next().unwrap_or_default().trim_end();
    matches!(base, "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ["COM", "LPT"].iter().any(|prefix| {
            base.strip_prefix(prefix).is_some_and(|rest| {
                matches!(
                    rest,
                    "1" | "2"
                        | "3"
                        | "4"
                        | "5"
                        | "6"
                        | "7"
                        | "8"
                        | "9"
                        | "\u{B9}"
                        | "\u{B2}"
                        | "\u{B3}"
                )
            })
        })
}

/// A file name that is valid on Windows, macOS and Linux: no path separators or characters
/// Windows forbids, no control or invisible format characters, no leading or trailing dots and
/// spaces, no reserved device name, at most [`MAX_STEM_CHARS`], plus the format's extension.
/// The suggestion usually is the session title, which a program may have set itself.
pub fn file_name(suggested: &str, format: Format) -> String {
    let extension = format.extension();
    let mut stem: String = suggested
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'))
        .collect();
    // The extension is added below; a suggestion that already has it does not get it twice.
    let suffix = format!(".{extension}");
    if stem.to_ascii_lowercase().ends_with(&suffix) {
        stem.truncate(stem.len() - suffix.len());
    }
    let mut stem: String = stem
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .chars()
        .take(MAX_STEM_CHARS)
        .collect();
    stem = stem.trim_end_matches(['.', ' ']).to_owned();
    if stem.is_empty() {
        stem = FALLBACK_STEM.to_owned();
    }
    if is_reserved_on_windows(&stem) {
        stem.insert(0, '_');
    }
    format!("{stem}{suffix}")
}

/// Writes the content to the chosen file: temporary file next to it, then rename, so an
/// existing file is either replaced completely or left as it was.
pub fn write(path: &Path, content: &str) -> Result<(), ExportError> {
    check_size(content)?;
    crate::persist::write_atomic(path, content.as_bytes()).map_err(|source| ExportError::Io {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn formats_from_the_window() {
        let format: Format = serde_json::from_str("\"text\"").unwrap();
        assert_eq!(format, Format::Text);
        let format: Format = serde_json::from_str("\"html\"").unwrap();
        assert_eq!(format.extension(), "html");
        assert!(serde_json::from_str::<Format>("\"pdf\"").is_err());
    }

    #[test]
    fn suggested_names_become_safe_file_names() {
        let text = Format::Text;
        assert_eq!(file_name("Claude Code", text), "Claude Code.txt");
        assert_eq!(file_name("Claude Code", Format::Html), "Claude Code.html");
        assert_eq!(
            file_name("..\\..\\Windows\\evil", text),
            "_.._Windows_evil.txt"
        );
        assert_eq!(file_name("/etc/passwd", text), "_etc_passwd.txt");
        assert_eq!(file_name("a<b>c:d\"e|f?g*h", text), "a_b_c_d_e_f_g_h.txt");
        assert_eq!(file_name("  ..hidden.. ", text), "hidden.txt");
        assert_eq!(
            file_name("line\r\nbreak\x1b[31m", text),
            "line break [31m.txt"
        );
        assert_eq!(file_name("a\u{202E}txt.exe", text), "atxt.exe.txt");
        assert_eq!(file_name("", text), "session.txt");
        assert_eq!(file_name("...", text), "session.txt");
        assert_eq!(file_name("log.TXT", text), "log.txt");
        assert_eq!(file_name("Größe 😀", text), "Größe 😀.txt");
        for reserved in ["CON", "nul", "Com1", "LPT9", "aux.tar", "con "] {
            let name = file_name(reserved, text);
            assert!(name.starts_with('_'), "{reserved} -> {name}");
        }
        assert_eq!(file_name("CONSOLE", text), "CONSOLE.txt");
        assert_eq!(file_name("COM10", text), "COM10.txt");
        let long = file_name(&"x".repeat(500), text);
        assert_eq!(long.chars().count(), MAX_STEM_CHARS + 4);
    }

    #[test]
    fn content_is_written_and_limited() {
        let tmp = TempDir::new("export");
        let path = tmp.path().join("out file.html");
        write(&path, "<pre>äöü</pre>").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<pre>äöü</pre>");
        write(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");

        let secret = "sk-export-secret";
        let too_big = format!("{secret}{}", "x".repeat(MAX_BYTES));
        let error = write(&path, &too_big).unwrap_err();
        assert!(matches!(error, ExportError::TooLarge { .. }));
        assert!(!serde_json::to_string(&error).unwrap().contains(secret));
        // The previous file survives the rejection.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert!(check_size(&"x".repeat(MAX_BYTES)).is_ok());
    }
}
