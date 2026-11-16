//! Codex CLI's sub-agent definitions: one TOML file per agent in `~/.codex/agents` (global) and
//! `<project>/.codex/agents` (project) with `name`, `description` and `developer_instructions`,
//! optionally `model` and `model_reasoning_effort` (Codex CLI 0.169.0,
//! docs/wiki/pages/sources/sub-agent-models-research.md). Hangar lists them next to Claude Code's
//! and writes copies of the user's Claude Code definitions there ([`crate::agent_provision`]).
//!
//! A copy starts with a marker line naming its source and the SHA-256 of everything after the
//! line. Only a file whose marker is intact counts as Hangar's own; a copy edited by hand fails the
//! hash and is the user's again.

use std::path::Path;

use sha2::{Digest as _, Sha256};

use crate::agent_defs::{valid_name, Problem};

/// `<base>/.codex/agents`.
pub fn agents_dir(base: &Path) -> std::path::PathBuf {
    base.join(".codex").join("agents")
}

/// What a Codex CLI definition says, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Hangar's own copy with an intact marker.
    pub managed: Option<Marker>,
}

/// The marker line of a copy Hangar wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The name of the Claude Code definition it was made from.
    pub source: String,
    /// The file Hangar set aside when it replaced the user's own (`manager.toml.bak`).
    pub backup: Option<String>,
}

const MARKER_START: &str = "# Managed by Hangar from ~/.claude/agents/";
const MARKER_BACKUP: &str = " Replaced file kept as ";
const MARKER_HASH: &str = " sha256:";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A backup name Hangar writes for the definition `source`: `<source>.toml.bak` or
/// `<source>.toml.bak.<n>`. Anything else (another definition's file, a folder) is never moved
/// back into place.
fn backup_name_of(source: &str, name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(source)
        .and_then(|rest| rest.strip_prefix(".toml.bak"))
    else {
        return false;
    };
    rest.is_empty()
        || rest
            .strip_prefix('.')
            .is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The intact marker of `text`, or `None` (no marker, or the rest was changed).
pub fn marker(text: &str) -> Option<Marker> {
    read_marker(text, true)
}

/// The marker line of `text` without checking the hash: a copy edited by hand still names the
/// backup of the user's original, which must not be lost.
pub fn marker_unchecked(text: &str) -> Option<Marker> {
    read_marker(text, false)
}

fn read_marker(text: &str, check: bool) -> Option<Marker> {
    let (line, rest) = text.split_once('\n')?;
    let line = line.strip_prefix(MARKER_START)?;
    let (head, hash) = line.rsplit_once(MARKER_HASH)?;
    if check && sha256_hex(rest.as_bytes()) != hash {
        return None;
    }
    let (source, backup) = match head.split_once(MARKER_BACKUP) {
        Some((source, backup)) => (source, Some(backup.strip_suffix('.')?)),
        None => (head, None),
    };
    let source = source.strip_suffix(".md.")?;
    if !valid_name(source) || backup.is_some_and(|backup| !backup_name_of(source, backup)) {
        return None;
    }
    Some(Marker {
        source: source.to_owned(),
        backup: backup.map(str::to_owned),
    })
}

fn string_field(table: &toml::Table, key: &str) -> Option<String> {
    table
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Reads one Codex CLI definition. Model and effort are kept as written when they are plain
/// short values, so the window can show a value it does not offer.
pub fn parse_def(text: &str) -> Result<Parsed, Problem> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let table: toml::Table = text.parse().map_err(|_| Problem::NotToml)?;
    let name = string_field(&table, "name").ok_or(Problem::MissingName)?;
    if !valid_name(&name) {
        return Err(Problem::InvalidName);
    }
    let description = string_field(&table, "description").ok_or(Problem::MissingDescription)?;
    if string_field(&table, "developer_instructions").is_none() {
        return Err(Problem::MissingInstructions);
    }
    let short = |key: &str| {
        string_field(&table, key).filter(|value| {
            value.len() <= 64
                && value.chars().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '[' | ']')
                })
        })
    };
    Ok(Parsed {
        name,
        description: crate::agent_defs::cut_description(description),
        model: short("model"),
        effort: short("model_reasoning_effort"),
        managed: marker(text),
    })
}

/// A TOML basic string: `"` and `\` escaped, control characters as `\uXXXX` (`\n`, `\t` short).
fn basic_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A multi-line TOML string holding exactly `text`: a literal one (`'''`, nothing escaped) when it
/// can, else a basic one with escapes. The line break after the opening quotes is not part of
/// the value.
fn multiline_string(text: &str) -> String {
    let literal_ok = !text.contains("'''")
        && !text.ends_with('\'')
        && !text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t');
    if literal_ok {
        return format!("'''\n{text}'''");
    }
    let mut out = String::from("\"\"\"\n");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\t' => out.push(c),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push_str("\"\"\"");
    out
}

/// What a copy of a Claude Code definition holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy<'a> {
    pub name: &'a str,
    pub description: &'a str,
    /// The prompt below the frontmatter.
    pub instructions: &'a str,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub backup: Option<&'a str>,
}

/// The file of a copy: the marker line, then `name`, `description`, the model fields and
/// `developer_instructions`. Line breaks are LF; the prompt's CRLF becomes LF.
pub fn render(copy: &Copy<'_>) -> String {
    let instructions = copy.instructions.replace("\r\n", "\n");
    let mut rest = format!(
        "name = {}\ndescription = {}\n",
        basic_string(copy.name),
        basic_string(copy.description)
    );
    if let Some(model) = copy.model {
        rest.push_str(&format!("model = {}\n", basic_string(model)));
    }
    if let Some(effort) = copy.effort {
        rest.push_str(&format!(
            "model_reasoning_effort = {}\n",
            basic_string(effort)
        ));
    }
    rest.push_str(&format!(
        "developer_instructions = {}\n",
        multiline_string(&instructions)
    ));
    let backup = copy
        .backup
        .map(|backup| format!("{MARKER_BACKUP}{backup}."))
        .unwrap_or_default();
    format!(
        "{MARKER_START}{}.md.{backup}{MARKER_HASH}{}\n{rest}",
        copy.name,
        sha256_hex(rest.as_bytes())
    )
}

/// The main model of Codex CLI's own config (`~/.codex/config.toml`, top-level `model`), as a
/// suggestion for its sub-agents. Read only; a missing, large or broken file gives none.
pub fn main_model(home: &Path) -> Option<String> {
    let path = home.join(".codex").join("config.toml");
    let bytes = crate::persist::read_regular_capped(&path, 256 * 1024).ok()??;
    let table: toml::Table = String::from_utf8(bytes).ok()?.parse().ok()?;
    table
        .get("model")?
        .as_str()
        .filter(|model| crate::agent_models::valid_model(model))
        .map(str::to_owned)
}

/// A copy Hangar keeps, written again with `model` and `effort`; `None` when `text` is no intact
/// copy.
pub fn copy_with_model(text: &str, model: Option<&str>, effort: Option<&str>) -> Option<String> {
    let marker = marker(text)?;
    let table: toml::Table = text.parse().ok()?;
    let field = |key: &str| table.get(key).and_then(toml::Value::as_str);
    Some(render(&Copy {
        name: field("name")?,
        description: field("description")?,
        instructions: field("developer_instructions")?,
        model,
        effort,
        backup: marker.backup.as_deref(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy<'a>(instructions: &'a str) -> Copy<'a> {
        Copy {
            name: "backend",
            description: "Backend: \"logic\" \\ jobs\nsecond line",
            instructions,
            model: None,
            effort: None,
            backup: None,
        }
    }

    fn round_trip(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn a_copy_is_written_byte_for_byte() {
        let text = render(&copy("You are the backend.\n\nUse `cargo test`.\n"));
        let rest = "name = \"backend\"\ndescription = \"Backend: \\\"logic\\\" \\\\ jobs\\nsecond line\"\ndeveloper_instructions = '''\nYou are the backend.\n\nUse `cargo test`.\n'''\n";
        assert_eq!(
            text,
            format!(
                "# Managed by Hangar from ~/.claude/agents/backend.md. sha256:{}\n{rest}",
                sha256_hex(rest.as_bytes())
            )
        );
        let table = round_trip(&text);
        assert_eq!(
            table["description"].as_str(),
            Some("Backend: \"logic\" \\ jobs\nsecond line")
        );
        assert_eq!(
            table["developer_instructions"].as_str(),
            Some("You are the backend.\n\nUse `cargo test`.\n")
        );
        assert_eq!(
            marker(&text),
            Some(Marker {
                source: "backend".into(),
                backup: None
            })
        );
        let parsed = parse_def(&text).unwrap();
        assert_eq!(parsed.name, "backend");
        assert!(parsed.managed.is_some());
    }

    #[test]
    fn prompts_that_a_literal_string_cannot_hold_are_escaped() {
        for prompt in [
            "Quote ''' inside",
            "Ends with a quote '",
            "Path C:\\Users\\x and \"quotes\" and '''",
            "Bell \u{7} here",
            "Tabs\tand\nlines\n",
            "Windows\r\nlines\r\n",
        ] {
            let text = render(&copy(prompt));
            let table = round_trip(&text);
            assert_eq!(
                table["developer_instructions"].as_str(),
                Some(prompt.replace("\r\n", "\n").as_str()),
                "{prompt:?}"
            );
            assert!(marker(&text).is_some(), "{prompt:?}");
        }
        // Backslashes and double quotes stay unescaped in a literal string.
        assert!(render(&copy("C:\\x \"y\"")).contains("'''\nC:\\x \"y\"'''"));
    }

    #[test]
    fn model_and_backup_are_part_of_the_copy() {
        let mut with = copy("Body");
        with.model = Some("gpt-6.1-sol");
        with.effort = Some("high");
        with.backup = Some("backend.toml.bak");
        let text = render(&with);
        assert!(text.starts_with(
            "# Managed by Hangar from ~/.claude/agents/backend.md. Replaced file kept as backend.toml.bak. sha256:"
        ));
        let parsed = parse_def(&text).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(parsed.effort.as_deref(), Some("high"));
        assert_eq!(
            parsed.managed.unwrap().backup.as_deref(),
            Some("backend.toml.bak")
        );
    }

    #[test]
    fn an_edited_copy_is_no_longer_hangars() {
        let text = render(&copy("Body"));
        assert!(marker(&text.replace("Body", "Changed")).is_none());
        assert!(marker(&text.replacen("backend.md", "../x.md", 1)).is_none());
        assert!(marker("name = \"x\"\n").is_none());
    }

    #[test]
    fn a_marker_names_only_a_backup_of_its_own_definition() {
        let with = |backup: &'static str| {
            let mut copy = copy("Body");
            copy.backup = Some(backup);
            render(&copy)
        };
        for good in [
            "backend.toml.bak",
            "backend.toml.bak.2",
            "backend.toml.bak.100",
        ] {
            assert_eq!(
                marker(&with(good)).and_then(|m| m.backup).as_deref(),
                Some(good)
            );
        }
        // Another definition's file, a path or an odd suffix is never moved back into place.
        for bad in [
            "planner.toml",
            "planner.toml.bak",
            "../backend.toml.bak",
            "backend.toml.bak.x",
            "backend.toml.bak.",
            "backend.toml.bak.1000",
            "backend.toml",
        ] {
            assert!(marker(&with(bad)).is_none(), "{bad}");
            assert!(marker_unchecked(&with(bad)).is_none(), "{bad}");
        }
    }

    #[test]
    fn foreign_definitions_are_read_and_checked() {
        let text = "name = \"manager\"\ndescription = \"Reviews\"\nmodel = \"gpt-5\"\nmodel_reasoning_effort = \"high\"\ndeveloper_instructions = \"\"\"\nBe strict.\n\"\"\"\n";
        let parsed = parse_def(text).unwrap();
        assert_eq!(parsed.name, "manager");
        assert_eq!(parsed.model.as_deref(), Some("gpt-5"));
        assert!(parsed.managed.is_none());
        assert_eq!(parse_def("= nope"), Err(Problem::NotToml));
        assert_eq!(
            parse_def("description = \"d\"\ndeveloper_instructions = \"x\"\n"),
            Err(Problem::MissingName)
        );
        assert_eq!(
            parse_def("name = \"Big\"\ndescription = \"d\"\ndeveloper_instructions = \"x\"\n"),
            Err(Problem::InvalidName)
        );
        assert_eq!(
            parse_def("name = \"x\"\ndeveloper_instructions = \"x\"\n"),
            Err(Problem::MissingDescription)
        );
        assert_eq!(
            parse_def("name = \"x\"\ndescription = \"d\"\n"),
            Err(Problem::MissingInstructions)
        );
    }
}
