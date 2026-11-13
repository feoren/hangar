//! Global agent instructions (`HANGAR.md`) and first prompts handed to an agent at its start.
//!
//! `HANGAR.md` lives in Hangar's config folder next to `profiles.toml`. When the window turns it on,
//! every start of an agent whose catalog entry has `instruction_args` gets the file's content the
//! way that program takes extra instructions (Claude Code `--append-system-prompt`, Codex CLI
//! `-c developer_instructions=...`, Aider `--read`); the user's repositories stay untouched. A first
//! prompt (`prompt_args`) is the user message an interactive start begins with, such as the
//! upstream check of a fresh worktree session. Which agent takes what, and the evidence:
//! docs/wiki/pages/syntheses/agent-instructions.md.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const FILE_NAME: &str = "HANGAR.md";

/// Written when the file is opened for the first time. Everything inside `<!-- -->` stays out of
/// what the agents get, so the template alone adds nothing; the user writes below it.
pub const DEFAULT_TEXT: &str = "<!--
HANGAR.md: Hangar hands this file to every agent it starts (where the program supports it) as an extra
instruction, like AGENTS.md or CLAUDE.md, without touching your repositories. Comments like this one
are left out, so nothing is sent until you write your instructions below it. Switch it off in
Settings > Programs.

No secrets here: the text is handed to the agents on their command line, which other programs
running as you can read.

Examples:
- Answer in English and keep summaries short.
- Run the project's tests before you say a change is done.
- Never commit or push unless I ask for it.
-->

";

/// Placeholders in `instruction_args`: the text itself, the text as a TOML string (for
/// `key=value` overrides that parse their value as TOML), the path of `HANGAR.md`.
pub const TEXT: &str = "{text}";
pub const TOML_TEXT: &str = "{toml_text}";
pub const FILE: &str = "{file}";
const INSTRUCTION_PLACEHOLDERS: [&str; 3] = [TEXT, TOML_TEXT, FILE];

/// Placeholder in `prompt_args`.
pub const PROMPT: &str = "{prompt}";

/// Most UTF-16 units a whole command line may take: Windows' 32 767 including the terminating
/// zero. Checked on the fully quoted line with every argument of the start (gate options
/// included); when the instructions would push it over, they are left out with a logged warning,
/// never cut in the middle. The same limit applies on macOS and Linux, so a file behaves alike
/// everywhere.
pub const COMMAND_LINE_MAX: usize = 32_766;

/// Larger files are not read at all: they could never fit into [`COMMAND_LINE_MAX`], and Aider's
/// `--read` should not get a file of that size by accident either.
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// What a start may add to an agent's command line, as named by the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartPrompt {
    /// The session starts in a worktree Hangar has just created: check the base branch upstream.
    Worktree,
}

/// The texts of [`StartPrompt`], from the `[start_prompts]` table of the agent catalog.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartPrompts {
    pub worktree: String,
}

impl StartPrompts {
    pub fn text(&self, prompt: StartPrompt) -> &str {
        match prompt {
            StartPrompt::Worktree => &self.worktree,
        }
    }
}

/// The content of `HANGAR.md` that reaches an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instructions {
    /// Without comments and surrounding blank space; never empty.
    pub text: String,
    pub path: PathBuf,
}

/// Why instructions or a first prompt were left out of a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Skipped {
    #[error(
        "the command line would take {0} characters, more than the {COMMAND_LINE_MAX} allowed"
    )]
    TooLong(usize),
    #[error("the program is a cmd.exe launcher, which cannot pass these characters on safely")]
    Launcher,
}

/// Path of `HANGAR.md` in `dir`. If it is missing, it is created from [`DEFAULT_TEXT`]; an existing
/// file stays untouched.
pub fn ensure_file(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join(FILE_NAME);
    if !path.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, DEFAULT_TEXT)?;
    }
    Ok(path)
}

/// The instructions in `path`, `None` when the file is missing, unreadable, too large, not UTF-8
/// or has nothing left once comments are removed. Problems other than a missing file are logged:
/// the start goes on without the instructions.
pub fn read(path: &Path) -> Option<Instructions> {
    match read_text(path) {
        Ok(Some(raw)) => {
            let text = strip_comments(raw.trim_start_matches('\u{feff}'))
                .trim()
                .to_owned();
            (!text.is_empty()).then(|| Instructions {
                text,
                path: path.to_path_buf(),
            })
        }
        Ok(None) => None,
        Err(error) => {
            eprintln!("hangar: {} not used: {error}", path.display());
            None
        }
    }
}

fn read_text(path: &Path) -> io::Result<Option<String>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::other(format!(
            "larger than {} KiB",
            MAX_FILE_BYTES / 1024
        )));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8 text"))
}

/// `text` without its HTML comments. An unclosed `<!--` is kept as written, so a typo never
/// swallows the rest of the file unnoticed.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        let Some(length) = rest[start + 4..].find("-->") else {
            break;
        };
        out.push_str(&rest[..start]);
        rest = &rest[start + 4 + length + 3..];
    }
    out.push_str(rest);
    out
}

/// `instruction_args` must name exactly one of the placeholders, once; `prompt_args` must name
/// [`PROMPT`] once. Checked when the catalog is read.
pub fn check_args(
    instruction_args: Option<&[String]>,
    prompt_args: Option<&[String]>,
) -> Result<(), &'static str> {
    let count = |args: &[String], names: &[&str]| -> usize {
        args.iter()
            .map(|arg| {
                names
                    .iter()
                    .map(|name| arg.matches(name).count())
                    .sum::<usize>()
            })
            .sum()
    };
    if instruction_args.is_some_and(|args| count(args, &INSTRUCTION_PLACEHOLDERS) != 1) {
        return Err("instruction_args needs exactly one {text}, {toml_text} or {file}");
    }
    if prompt_args.is_some_and(|args| count(args, &[PROMPT]) != 1) {
        return Err("prompt_args needs {prompt} exactly once");
    }
    Ok(())
}

/// `spec` with the instructions filled in.
pub fn instruction_args(spec: &[String], instructions: &Instructions) -> Vec<String> {
    let toml_text = toml::Value::String(instructions.text.clone()).to_string();
    let file = instructions.path.to_string_lossy();
    spec.iter()
        .map(|arg| {
            arg.replace(TOML_TEXT, &toml_text)
                .replace(TEXT, &instructions.text)
                .replace(FILE, &file)
        })
        .collect()
}

/// `spec` with the first prompt filled in.
pub fn prompt_args(spec: &[String], prompt: &str) -> Vec<String> {
    spec.iter().map(|arg| arg.replace(PROMPT, prompt)).collect()
}

/// `spec` hands over the file's text itself rather than its path.
fn is_inline(spec: &[String]) -> bool {
    spec.iter()
        .any(|arg| arg.contains(TEXT) || arg.contains(TOML_TEXT))
}

/// [`Skipped::Launcher`] when `program` runs through cmd.exe and the arguments cannot pass it
/// ([`crate::batch`]): always for a `spec` that carries the file's text (a Markdown file nearly
/// always holds line breaks or quotes, and whether one text passes must not decide from start to
/// start), otherwise when an argument holds a character cmd.exe would interpret. Such a start goes
/// on without these arguments instead of failing.
pub fn check_launcher(
    program: &Path,
    spec: &[String],
    args: Vec<String>,
) -> Result<Vec<String>, Skipped> {
    if crate::batch::is_batch(program)
        && (is_inline(spec) || !args.iter().all(|arg| crate::batch::argument_ok(arg)))
    {
        return Err(Skipped::Launcher);
    }
    Ok(args)
}

/// Whether `HANGAR.md` reaches a program, for Settings ([`crate::agent_catalog::ProfileView`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstructionsReach {
    /// The program takes no instructions from Hangar.
    None,
    /// It gets them (or would, once installed).
    Yes,
    /// It takes them, but the copy found is a cmd.exe launcher (npm's `codex.cmd`) that cannot
    /// carry the text ([`check_launcher`]).
    Launcher,
}

/// [`InstructionsReach`] of a program with the catalog's `spec`, started as `program` (`None`:
/// not installed).
pub fn reach(spec: Option<&[String]>, program: Option<&Path>) -> InstructionsReach {
    match spec {
        None => InstructionsReach::None,
        Some(spec) if program.is_some_and(crate::batch::is_batch) && is_inline(spec) => {
            InstructionsReach::Launcher
        }
        Some(_) => InstructionsReach::Yes,
    }
}

/// Length in UTF-16 units of the command line Windows builds from `program` and `args`, quoted the
/// way the C runtime parses it back (Rust's std and portable-pty quote the same way): an argument
/// with a blank, a tab or a quote, or an empty one, goes in quotes; a quote gets a backslash, and
/// the backslashes before a quote or the closing quote are doubled.
pub fn command_line_units(program: &str, args: &[String]) -> usize {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(quoted_units)
        .sum::<usize>()
        // The blanks between the parts.
        + args.len()
}

fn quoted_units(arg: &str) -> usize {
    let plain = arg.encode_utf16().count();
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return plain;
    }
    let mut extra = 2;
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                extra += backslashes + 1;
                backslashes = 0;
            }
            _ => backslashes = 0,
        }
    }
    plain + extra + backslashes
}

/// [`Skipped::TooLong`] when the start's whole command line ([`command_line_units`]) would not fit
/// into [`COMMAND_LINE_MAX`].
pub fn check_length(program: &str, args: &[String]) -> Result<(), Skipped> {
    let units = command_line_units(program, args);
    if units > COMMAND_LINE_MAX {
        return Err(Skipped::TooLong(units));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn instructions(text: &str) -> Instructions {
        Instructions {
            text: text.to_owned(),
            path: PathBuf::from("/config/HANGAR.md"),
        }
    }

    #[test]
    fn comments_are_left_out_and_an_unclosed_one_is_kept() {
        assert_eq!(strip_comments("a<!-- x -->b<!--y-->c"), "abc");
        assert_eq!(strip_comments("a <!-- open"), "a <!-- open");
        assert_eq!(strip_comments("no comment"), "no comment");
        assert_eq!(strip_comments("<!--\nmulti\nline\n-->\nrule"), "\nrule");
    }

    #[test]
    fn the_template_alone_adds_nothing() {
        let tmp = TempDir::new("instructions-template");
        let path = ensure_file(tmp.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_TEXT);
        assert_eq!(read(&path), None);
        // An edited file is never overwritten.
        std::fs::write(&path, "Use tabs.").unwrap();
        assert_eq!(ensure_file(tmp.path()).unwrap(), path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Use tabs.");
    }

    #[test]
    fn missing_empty_and_filled_files() {
        let tmp = TempDir::new("instructions-read");
        let path = tmp.path().join(FILE_NAME);
        assert_eq!(read(&path), None);
        std::fs::write(&path, "  \n\n").unwrap();
        assert_eq!(read(&path), None);
        std::fs::write(&path, "\u{feff}<!-- note -->\nUse tabs.\n").unwrap();
        assert_eq!(read(&path), Some(instructions_at("Use tabs.", &path)));
        std::fs::write(&path, vec![b'a'; 300 * 1024]).unwrap();
        assert_eq!(read(&path), None);
        std::fs::write(&path, [0xff, 0xfe, 0x41]).unwrap();
        assert_eq!(read(&path), None);
    }

    fn instructions_at(text: &str, path: &Path) -> Instructions {
        Instructions {
            text: text.to_owned(),
            path: path.to_path_buf(),
        }
    }

    #[test]
    fn placeholders_are_checked() {
        let ok = strings(&["--append-system-prompt", "{text}"]);
        assert_eq!(check_args(Some(&ok), Some(&strings(&["{prompt}"]))), Ok(()));
        assert_eq!(check_args(None, None), Ok(()));
        for bad in [
            strings(&["--x"]),
            strings(&["{text}", "{file}"]),
            strings(&["{text}{text}"]),
        ] {
            assert!(check_args(Some(&bad), None).is_err(), "{bad:?}");
        }
        assert!(check_args(None, Some(&strings(&["-m"]))).is_err());
        assert!(check_args(None, Some(&strings(&["{prompt}", "{prompt}"]))).is_err());
    }

    #[test]
    fn arguments_per_form() {
        let text = instructions("Say \"hi\".\nUse C:\\tools.");
        assert_eq!(
            instruction_args(&strings(&["--append-system-prompt={text}"]), &text),
            strings(&["--append-system-prompt=Say \"hi\".\nUse C:\\tools."])
        );
        assert_eq!(
            instruction_args(&strings(&["--read", "{file}"]), &text),
            strings(&["--read", "/config/HANGAR.md"])
        );
        // Codex parses the value of `-c key=value` as TOML: the text must come back unchanged.
        let codex = instruction_args(
            &strings(&["-c", "developer_instructions={toml_text}"]),
            &text,
        );
        assert_eq!(codex[0], "-c");
        let parsed: toml::Table = toml::from_str(&codex[1]).unwrap();
        assert_eq!(
            parsed["developer_instructions"].as_str(),
            Some(text.text.as_str())
        );
        assert_eq!(
            prompt_args(&strings(&["{prompt}"]), "Check upstream."),
            strings(&["Check upstream."])
        );
    }

    #[test]
    fn the_quoted_command_line_is_measured() {
        // Plain parts, one blank between them.
        assert_eq!(command_line_units("a.exe", &strings(&["-x", "y"])), 10);
        // Blank: in quotes. Quote: a backslash. Backslashes before a quote and before the closing
        // quote double; others stay single.
        assert_eq!(quoted_units("a b"), 5);
        assert_eq!(quoted_units(""), 2);
        assert_eq!(quoted_units(r#"say "hi""#), 8 + 2 + 2);
        assert_eq!(quoted_units(r"C:\x y\"), 7 + 2 + 1);
        assert_eq!(quoted_units(r#"a\"b"#), 4 + 2 + 2);
        assert_eq!(quoted_units(r"C:\tools"), 8);
        // UTF-16 units, not bytes.
        assert_eq!(quoted_units("äö"), 2);
    }

    #[test]
    fn too_long_command_lines_are_refused() {
        let fits = vec!["x".repeat(COMMAND_LINE_MAX - 6)];
        assert_eq!(check_length("p.exe", &fits), Ok(()));
        let over = vec!["x".repeat(COMMAND_LINE_MAX - 5)];
        assert_eq!(
            check_length("p.exe", &over),
            Err(Skipped::TooLong(COMMAND_LINE_MAX + 1))
        );
        // Quotes count twice once quoted.
        let quotes = vec!["\"".repeat(COMMAND_LINE_MAX / 2)];
        assert!(check_length("p.exe", &quotes).is_err());
    }

    #[test]
    fn a_launcher_never_gets_the_text_and_a_safe_path_only() {
        let launcher = Path::new(r"C:\npm\codex.cmd");
        let exe = Path::new(r"C:\bin\claude.exe");
        let inline = strings(&["--append-system-prompt={text}"]);
        let one_line = strings(&["--append-system-prompt=Use tabs."]);
        assert_eq!(
            check_launcher(exe, &inline, one_line.clone()),
            Ok(one_line.clone())
        );
        let file_spec = strings(&["--read", "{file}"]);
        let safe = strings(&["--read", r"C:\config\HANGAR.md"]);
        assert_eq!(check_launcher(launcher, &file_spec, safe.clone()), Ok(safe));
        if cfg!(windows) {
            // Even a text that would pass: the outcome must not depend on what the file holds.
            assert_eq!(
                check_launcher(launcher, &inline, one_line),
                Err(Skipped::Launcher)
            );
            let unsafe_path = strings(&["--read", r"C:\100%\HANGAR.md"]);
            assert_eq!(
                check_launcher(launcher, &file_spec, unsafe_path),
                Err(Skipped::Launcher)
            );
        }
    }

    #[test]
    fn reach_per_program() {
        let inline = strings(&["-c", "developer_instructions={toml_text}"]);
        let file = strings(&["--read", "{file}"]);
        let shim = Path::new(r"C:\npm\codex.cmd");
        let exe = Path::new(r"C:\bin\codex.exe");
        assert_eq!(reach(None, Some(exe)), InstructionsReach::None);
        assert_eq!(reach(Some(&inline), Some(exe)), InstructionsReach::Yes);
        assert_eq!(reach(Some(&inline), None), InstructionsReach::Yes);
        assert_eq!(reach(Some(&file), Some(shim)), InstructionsReach::Yes);
        let expected = if cfg!(windows) {
            InstructionsReach::Launcher
        } else {
            InstructionsReach::Yes
        };
        assert_eq!(reach(Some(&inline), Some(shim)), expected);
    }

    #[test]
    fn start_prompts_by_name() {
        let prompts = StartPrompts {
            worktree: "Check.".into(),
        };
        assert_eq!(prompts.text(StartPrompt::Worktree), "Check.");
        let parsed: StartPrompt = serde_json::from_str("\"worktree\"").unwrap();
        assert_eq!(parsed, StartPrompt::Worktree);
    }
}
