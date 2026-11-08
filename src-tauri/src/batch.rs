//! cmd.exe scripts (`.cmd`, `.bat`) as session targets on Windows, such as npm shims like
//! `codex.cmd` or `gemini.cmd`.
//!
//! CreateProcessW starts such a target through cmd.exe, and cmd.exe parses the command line
//! itself. portable-pty 0.9 only quotes by the C runtime rules (parts with whitespace
//! or `"` in quotes, `"` as `\"`); `&`, `|`, `%` … in the path or arguments would thus
//! become command syntax (BatBadBut). Escaping as Rust std does since 1.77.2
//! (`cmd.exe /e:ON /v:OFF /d /c "…"`, `%` as `%%cd:~,%`) is not possible here: portable-pty accepts no
//! raw command line and quotes every string again. So anything cmd.exe could interpret is
//! rejected before a process starts. Measurement and trade-offs:
//! `docs/wiki/pages/syntheses/cmd-launcher.md`.

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

/// Always rejected, even inside quotes: cmd.exe expands `%` and (with delayed expansion from the
/// registry) `!` there as well, and `"` flips the quote state because cmd.exe does not know `\"`.
/// `& | < > ^` would be harmless inside quotes, but an argument without spaces stays
/// unquoted, and npm shims insert their own folder unquoted
/// (`SET dp0=%~dp0`), where an `&` in the program path takes effect despite the quotes on our line.
const ALWAYS: &[char] = &['%', '!', '"', '&', '|', '<', '>', '^'];

/// Rejected only in unquoted parts: inside a `( … )` block of the script, a `)` from
/// `%*` ends the block. `C:\Program Files (x86)\…` contains spaces, gets quoted and stays allowed.
const UNQUOTED: &[char] = &['(', ')'];

/// Does `program` run through cmd.exe on Windows? Never on Unix.
pub fn is_batch(program: &Path) -> bool {
    cfg!(windows) && has_batch_extension(program)
}

fn has_batch_extension(program: &Path) -> bool {
    program
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Program,
    /// Counted from 1, as read in the profile file.
    Argument(usize),
}

/// A character that cmd.exe would interpret in a script call. The argument's value is left out
/// on purpose: it may be a token, and it reaches the frontend as error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub struct UnsafeBatch {
    pub part: Part,
    pub found: char,
}

impl fmt::Display for UnsafeBatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.part {
            Part::Program => f.write_str("The program path")?,
            Part::Argument(n) => write!(f, "Argument {n}")?,
        }
        if self.found.is_control() {
            write!(
                f,
                " contains the control character U+{:04X}",
                u32::from(self.found)
            )?;
        } else {
            write!(f, " contains '{}'", self.found)?;
        }
        f.write_str(", which cmd.exe would interpret as command syntax")
    }
}

/// Checks path and arguments of a `.cmd`/`.bat` target. Everything that is plain text for
/// cmd.exe after portable-pty's quoting stays allowed.
pub fn check(program: &Path, args: &[String]) -> Result<(), UnsafeBatch> {
    let unsafe_in = |part, token: &str| match find_unsafe(token) {
        Some(found) => Err(UnsafeBatch { part, found }),
        None => Ok(()),
    };
    unsafe_in(Part::Program, &program.to_string_lossy())?;
    for (index, arg) in args.iter().enumerate() {
        unsafe_in(Part::Argument(index + 1), arg)?;
    }
    Ok(())
}

fn find_unsafe(token: &str) -> Option<char> {
    // portable-pty quotes parts with whitespace or `"`; tab and `"` are rejected anyway.
    let quoted = token.contains(' ');
    token
        .chars()
        .find(|&c| ALWAYS.contains(&c) || c.is_control() || (!quoted && UNQUOTED.contains(&c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn cmd_and_bat_extension_in_any_case() {
        for name in [
            r"C:\x\codex.cmd",
            r"C:\x\a.CMD",
            r"C:\x\b.bat",
            r"C:\x\c.Bat",
        ] {
            assert!(has_batch_extension(Path::new(name)), "{name}");
        }
        for name in [
            r"C:\x\claude.exe",
            r"C:\x\cmd",
            r"C:\x\a.cmdx",
            r"C:\x\a.ps1",
            "codex",
        ] {
            assert!(!has_batch_extension(Path::new(name)), "{name}");
        }
        // Shells are .exe and get neither the check nor the variable.
        assert!(!is_batch(Path::new(r"C:\Windows\System32\cmd.exe")));
        assert_eq!(is_batch(Path::new(r"C:\x\codex.cmd")), cfg!(windows));
    }

    #[test]
    fn common_calls_are_allowed() {
        let program = Path::new(r"C:\Program Files (x86)\nodejs\node_modules\npm.cmd");
        let list = args(&[
            "install",
            "-g",
            "@google/gemini-cli@latest",
            "--resume",
            "fix (this) now",
            r"C:\Users\Test User\",
            "--model=gpt-5.1",
            "key=value,x+y#1~$*?",
            "",
            "Café äöü",
        ]);
        assert_eq!(check(program, &list), Ok(()));
    }

    #[test]
    fn special_characters_in_argument_are_rejected() {
        let program = Path::new(r"C:\npm\codex.cmd");
        for (arg, found) in [
            ("x&calc", '&'),
            ("a b & c", '&'),
            ("a|b", '|'),
            ("a>b", '>'),
            ("a<b", '<'),
            ("a^b", '^'),
            ("%PATH%", '%'),
            ("100 %", '%'),
            ("hi!", '!'),
            ("a \"b\" c", '"'),
            ("a)", ')'),
            ("(a", '('),
            ("a\nb", '\n'),
            ("a\tb", '\t'),
        ] {
            assert_eq!(
                check(program, &args(&["ok", arg])),
                Err(UnsafeBatch {
                    part: Part::Argument(2),
                    found
                }),
                "{arg:?}"
            );
        }
    }

    #[test]
    fn special_characters_in_path_are_rejected() {
        for (path, found) in [
            (r"C:\a&b\codex.cmd", '&'),
            (r"C:\a b&c\codex.cmd", '&'),
            (r"C:\100%\codex.cmd", '%'),
            (r"C:\a^b\codex.cmd", '^'),
            (r"C:\x!\codex.cmd", '!'),
            (r"C:\tools(x)\codex.cmd", '('),
        ] {
            assert_eq!(
                check(Path::new(path), &[]),
                Err(UnsafeBatch {
                    part: Part::Program,
                    found
                }),
                "{path}"
            );
        }
    }

    #[test]
    fn message_names_position_and_character_but_not_value() {
        let error = check(Path::new(r"C:\x\a.cmd"), &args(&["--token=secret&x"])).unwrap_err();
        let text = error.to_string();
        assert_eq!(
            text,
            "Argument 1 contains '&', which cmd.exe would interpret as command syntax"
        );
        assert!(!text.contains("secret"));
        let control = UnsafeBatch {
            part: Part::Program,
            found: '\r',
        };
        assert!(control
            .to_string()
            .starts_with("The program path contains the control character U+000D"));
    }
}
