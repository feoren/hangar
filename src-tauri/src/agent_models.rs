//! Which model a sub-agent runs on: per definition (Claude Code's frontmatter `model`, Codex
//! CLI's `model` and `model_reasoning_effort`) and as a default per program for every sub-agent
//! of a session Hangar starts (Claude Code's `CLAUDE_CODE_SUBAGENT_MODEL`, Codex CLI's
//! `agents.default_subagent_model` and `agents.default_subagent_reasoning_effort`). A field in the
//! definition beats the default; that is the order both programs apply. Evidence:
//! docs/wiki/pages/sources/sub-agent-models-research.md, contract:
//! docs/wiki/pages/syntheses/sub-agent-library.md.
//!
//! Writing a definition changes exactly the one field: the rest of the file stays byte for byte.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The program a sub-agent definition belongs to, which decides its file format, where it lives
/// and how a model is handed to its sessions. Catalog field `agent_program`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DefProgram {
    /// Markdown with a frontmatter in `.claude/agents`.
    Claude,
    /// TOML in `.codex/agents`.
    Codex,
    /// Markdown in `~/.copilot/agents` ([`crate::md_agents`]); copies only, no model default.
    Copilot,
    /// Markdown in `~/.config/opencode/agents` (OpenCode 2.x); copies only, no model default.
    Opencode,
    /// Markdown in `~/.gemini/agents`; copies only, no model default.
    Gemini,
}

impl DefProgram {
    /// Whether Settings > Sub-agents offers a default model for its sessions and lists its
    /// definitions: only Claude Code and Codex CLI. The others only get copies.
    pub fn takes_model(self) -> bool {
        matches!(self, Self::Claude | Self::Codex)
    }
}

/// Longest model id taken.
pub const MAX_MODEL_LEN: usize = 64;

/// Claude Code's model aliases the window offers (`opusplan` and the `[1m]` forms are not:
/// both change more than the model). Full ids are taken as well.
pub const CLAUDE_ALIASES: [&str; 4] = ["opus", "sonnet", "haiku", "fable"];

/// Codex CLI's reasoning efforts the window offers: those every model of its catalog takes
/// (measured with 0.171.0). `minimal` no model takes, `none` and `ultra` only some; a sub-agent
/// with an effort its model does not take is not started at all.
/// Evidence: docs/wiki/pages/sources/codex-subagent-model-measurement.md.
pub const CODEX_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// Efforts Hangar offered and stored before the measurement; a stored default holding one is
/// read without it.
pub const RETIRED_CODEX_EFFORTS: [&str; 1] = ["minimal"];

/// Claude Code reads the default model of its sub-agents from this variable (2.1.325 binary:
/// used when the definition has no `model`).
pub const CLAUDE_MODEL_ENV: &str = "CLAUDE_CODE_SUBAGENT_MODEL";
/// Codex CLI's config keys for the default model and effort of its sub-agents (0.169.0 binary;
/// a `-c` override of both reaches a spawned sub-agent, measured with 0.171.0).
pub const CODEX_MODEL_KEY: &str = "agents.default_subagent_model";
pub const CODEX_EFFORT_KEY: &str = "agents.default_subagent_reasoning_effort";

/// A model id: ASCII letters, digits and `. _ : -`, starting with a letter or digit, at most
/// [`MAX_MODEL_LEN`]. Such a value needs no quoting in a frontmatter, a TOML string, an
/// environment variable or a cmd.exe launcher. `inherit` is no model: leaving the field out means
/// the same.
pub fn valid_model(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_MODEL_LEN
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
        && !value.eq_ignore_ascii_case("inherit")
}

pub fn valid_effort(value: &str) -> bool {
    CODEX_EFFORTS.contains(&value)
}

/// A model and, for Codex CLI, a reasoning effort; `None` leaves the program's own choice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelChoice {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

impl ModelChoice {
    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.effort.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("Not a model id: only letters, digits and . _ : - are taken, at most 64")]
    Model,
    #[error("Not a reasoning effort Hangar offers")]
    Effort,
    #[error("This program takes no reasoning effort")]
    NoEffort,
    #[error("Hangar hands this program no model")]
    NoModel,
}

/// Checks a choice for `program`: a valid model, an effort only for Codex CLI, nothing for a
/// program that only gets copies.
pub fn check_choice(program: DefProgram, choice: &ModelChoice) -> Result<(), ModelError> {
    if !program.takes_model() && !choice.is_empty() {
        return Err(ModelError::NoModel);
    }
    if choice
        .model
        .as_deref()
        .is_some_and(|model| !valid_model(model))
    {
        return Err(ModelError::Model);
    }
    match (program, choice.effort.as_deref()) {
        (_, None) => Ok(()),
        (DefProgram::Codex, Some(effort)) if valid_effort(effort) => Ok(()),
        (DefProgram::Codex, Some(_)) => Err(ModelError::Effort),
        (_, Some(_)) => Err(ModelError::NoEffort),
    }
}

/// What a session of `program` gets for its sub-agents' default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelLaunch {
    pub env: Vec<(String, String)>,
    /// Placed before the start arguments, like `HANGAR.md` (`codex -c ... resume --last`).
    pub args: Vec<String>,
}

/// The value of a `-c key=value` override. Codex CLI parses the value as TOML and takes the raw
/// text as a string when that fails (`codex --help` 0.169.0); a value that would parse as
/// something else (`1.5`, `true`, a date) goes in quotes. A checked model or effort never holds a
/// quote or a backslash.
fn override_value(value: &str) -> String {
    let parses = format!("v = {value}").parse::<toml::Table>().is_ok();
    if parses {
        format!("\"{value}\"")
    } else {
        value.to_owned()
    }
}

/// The default `choice` for every sub-agent of a session of `program`. A variable the profile
/// sets itself stays the profile's; a value a cmd.exe launcher cannot pass (a quoted override
/// for npm's `codex.cmd`) is left out with a logged warning, so the session still starts.
pub fn launch(
    program: DefProgram,
    choice: &ModelChoice,
    profile_env: &BTreeMap<String, String>,
    batch: bool,
) -> ModelLaunch {
    if check_choice(program, choice).is_err() {
        return ModelLaunch::default();
    }
    let mut out = ModelLaunch::default();
    match program {
        DefProgram::Claude => {
            if let Some(model) = &choice.model {
                if !profile_env.contains_key(CLAUDE_MODEL_ENV) {
                    out.env.push((CLAUDE_MODEL_ENV.to_owned(), model.clone()));
                }
            }
        }
        DefProgram::Codex => {
            let pairs = [
                (CODEX_MODEL_KEY, choice.model.as_deref()),
                (CODEX_EFFORT_KEY, choice.effort.as_deref()),
            ];
            for (key, value) in pairs {
                let Some(value) = value else { continue };
                let arg = format!("{key}={}", override_value(value));
                if batch && !crate::batch::argument_ok(&arg) {
                    eprintln!("hangar: {key} left out: a cmd.exe launcher cannot pass it");
                    continue;
                }
                out.args.extend(["-c".to_owned(), arg]);
            }
        }
        DefProgram::Copilot | DefProgram::Opencode | DefProgram::Gemini => {}
    }
    out
}

/// One line of a text with its line break, and where it starts.
struct Line<'a> {
    start: usize,
    text: &'a str,
}

impl Line<'_> {
    fn content(&self) -> &str {
        self.text.trim_end_matches(['\n', '\r'])
    }
    fn end(&self) -> usize {
        self.start + self.text.len()
    }
    fn eol(&self) -> &str {
        &self.text[self.content().len()..]
    }
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut start = 0;
    text.split_inclusive('\n')
        .map(|text| {
            let line = Line { start, text };
            start += text.len();
            line
        })
        .collect()
}

fn indented(line: &str) -> bool {
    line.starts_with([' ', '\t'])
}

/// `text` with its frontmatter's `model` field set to `model` (replaced where it stands, else
/// added as the last field) or removed (`None`, with any indented lines that continue it).
/// Everything else stays byte for byte, including the line breaks the file uses. `None` when the
/// text has no frontmatter.
pub fn set_frontmatter_model(text: &str, model: Option<&str>) -> Option<String> {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let all = lines(&text[bom..]);
    if all.first()?.content().trim_end() != "---" {
        return None;
    }
    let close = all
        .iter()
        .skip(1)
        .position(|line| line.content().trim_end() == "---")?
        + 1;
    let body = &all[1..close];
    let eol = all[0].eol().to_owned();
    let found = body.iter().position(|line| {
        !indented(line.text)
            && line
                .content()
                .split_once(':')
                .is_some_and(|(key, _)| key.trim() == "model")
    });
    let new_line = model.map(|model| format!("model: {model}{eol}"));
    let at = |line: &Line<'_>| bom + line.start;
    let mut out = String::with_capacity(text.len() + 32);
    match found {
        Some(index) => {
            let mut end = index + 1;
            while end < body.len()
                && (indented(body[end].text) || body[end].content().trim().is_empty())
            {
                end += 1;
            }
            // Blank lines after the field stay: they belong to the layout, not the value.
            while end > index + 1 && body[end - 1].content().trim().is_empty() {
                end -= 1;
            }
            let line_eol = body[index].eol();
            let replaced = model.map(|model| format!("model: {model}{line_eol}"));
            out.push_str(&text[..at(&body[index])]);
            out.push_str(replaced.as_deref().unwrap_or(""));
            out.push_str(&text[bom + body[end - 1].end()..]);
        }
        None => {
            let Some(line) = new_line else {
                return Some(text.to_owned());
            };
            let closing = &all[close];
            out.push_str(&text[..at(closing)]);
            out.push_str(&line);
            out.push_str(&text[at(closing)..]);
        }
    }
    Some(out)
}

/// `text` (a Codex CLI agent file) with `model` and `model_reasoning_effort` set or removed;
/// `toml_edit` keeps every other byte of the file.
pub fn set_toml_model(text: &str, choice: &ModelChoice) -> Result<String, toml_edit::TomlError> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    for (key, value) in [
        ("model", choice.model.as_deref()),
        ("model_reasoning_effort", choice.effort.as_deref()),
    ] {
        match value {
            Some(value) => {
                let keep_decor = doc
                    .get(key)
                    .and_then(|item| item.as_value())
                    .map(|old| (old.decor().prefix().cloned(), old.decor().suffix().cloned()));
                let mut new = toml_edit::Value::from(value);
                if let Some((prefix, suffix)) = keep_decor {
                    let decor = new.decor_mut();
                    if let Some(prefix) = prefix {
                        decor.set_prefix(prefix);
                    }
                    if let Some(suffix) = suffix {
                        decor.set_suffix(suffix);
                    }
                }
                doc[key] = toml_edit::Item::Value(new);
            }
            None => {
                doc.remove(key);
            }
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_plain() {
        for good in [
            "opus",
            "claude-opus-5-5",
            "gpt-6.1-sol",
            "o3",
            "vendor:model_1",
        ] {
            assert!(valid_model(good), "{good}");
        }
        for bad in [
            "", "inherit", "Inherit", "a b", "opus[1m]", "x\"y", "-lead", "a;b", "%PATH%", "über",
        ] {
            assert!(!valid_model(bad), "{bad}");
        }
        assert!(valid_model(&"a".repeat(MAX_MODEL_LEN)));
        assert!(!valid_model(&"a".repeat(MAX_MODEL_LEN + 1)));
    }

    #[test]
    fn choices_are_checked_per_program() {
        let choice = |model: Option<&str>, effort: Option<&str>| ModelChoice {
            model: model.map(str::to_owned),
            effort: effort.map(str::to_owned),
        };
        assert!(check_choice(DefProgram::Claude, &choice(Some("sonnet"), None)).is_ok());
        assert_eq!(
            check_choice(DefProgram::Claude, &choice(None, Some("high"))),
            Err(ModelError::NoEffort)
        );
        assert!(check_choice(
            DefProgram::Codex,
            &choice(Some("gpt-6.1-sol"), Some("xhigh"))
        )
        .is_ok());
        assert_eq!(
            check_choice(DefProgram::Codex, &choice(None, Some("turbo"))),
            Err(ModelError::Effort)
        );
        // Measured with Codex CLI 0.171.0: no model takes `minimal`, every listed one takes `max`.
        assert_eq!(
            check_choice(DefProgram::Codex, &choice(None, Some("minimal"))),
            Err(ModelError::Effort)
        );
        assert!(check_choice(DefProgram::Codex, &choice(None, Some("max"))).is_ok());
        assert_eq!(
            check_choice(DefProgram::Codex, &choice(Some("a b"), None)),
            Err(ModelError::Model)
        );
        // Programs that only get copies take no model at all, and their sessions get none.
        for program in [
            DefProgram::Copilot,
            DefProgram::Opencode,
            DefProgram::Gemini,
        ] {
            assert!(!program.takes_model());
            assert!(check_choice(program, &ModelChoice::default()).is_ok());
            assert_eq!(
                check_choice(program, &choice(Some("gpt-5"), None)),
                Err(ModelError::NoModel)
            );
            assert_eq!(
                launch(
                    program,
                    &choice(Some("gpt-5"), None),
                    &BTreeMap::new(),
                    false
                ),
                ModelLaunch::default()
            );
        }
        assert!(DefProgram::Claude.takes_model() && DefProgram::Codex.takes_model());
    }

    #[test]
    fn claude_gets_the_variable_unless_the_profile_sets_it() {
        let choice = ModelChoice {
            model: Some("sonnet".into()),
            effort: None,
        };
        let none = BTreeMap::new();
        let launch_with = launch(DefProgram::Claude, &choice, &none, false);
        assert_eq!(
            launch_with.env,
            [(CLAUDE_MODEL_ENV.to_owned(), "sonnet".to_owned())]
        );
        assert!(launch_with.args.is_empty());
        let own = BTreeMap::from([(CLAUDE_MODEL_ENV.to_owned(), "haiku".to_owned())]);
        assert!(launch(DefProgram::Claude, &choice, &own, false)
            .env
            .is_empty());
        assert_eq!(
            launch(DefProgram::Claude, &ModelChoice::default(), &none, false),
            ModelLaunch::default()
        );
    }

    #[test]
    fn codex_gets_config_overrides_as_argv() {
        let choice = ModelChoice {
            model: Some("gpt-6.1-sol".into()),
            effort: Some("high".into()),
        };
        let none = BTreeMap::new();
        let out = launch(DefProgram::Codex, &choice, &none, false);
        assert!(out.env.is_empty());
        assert_eq!(
            out.args,
            [
                "-c",
                "agents.default_subagent_model=gpt-6.1-sol",
                "-c",
                "agents.default_subagent_reasoning_effort=high",
            ]
        );
        // A value TOML reads as a number goes in quotes, which a cmd.exe launcher cannot pass.
        let number = ModelChoice {
            model: Some("1.5".into()),
            effort: None,
        };
        assert_eq!(
            launch(DefProgram::Codex, &number, &none, false).args,
            ["-c", "agents.default_subagent_model=\"1.5\""]
        );
        assert!(launch(DefProgram::Codex, &number, &none, true)
            .args
            .is_empty());
        assert_eq!(
            launch(DefProgram::Codex, &choice, &none, true).args.len(),
            4
        );
        // An unchecked value never reaches the command line.
        let bad = ModelChoice {
            model: Some("a b".into()),
            effort: None,
        };
        assert!(launch(DefProgram::Codex, &bad, &none, false)
            .args
            .is_empty());
    }

    const DEF: &str = "---\nname: backend\ndescription: Backend: logic\nmodel: opus\ncolor: green\n---\n\nYou are\nmodel: not a field\n";

    #[test]
    fn the_model_line_is_replaced_in_place() {
        let out = set_frontmatter_model(DEF, Some("sonnet")).unwrap();
        assert_eq!(out, DEF.replace("model: opus\n", "model: sonnet\n"));
    }

    #[test]
    fn the_model_line_is_removed_and_added() {
        let removed = set_frontmatter_model(DEF, None).unwrap();
        assert_eq!(removed, DEF.replace("model: opus\n", ""));
        let added = set_frontmatter_model(&removed, Some("haiku")).unwrap();
        assert_eq!(
            added,
            "---\nname: backend\ndescription: Backend: logic\ncolor: green\nmodel: haiku\n---\n\nYou are\nmodel: not a field\n"
        );
        // Nothing to remove: the text stays as it is.
        assert_eq!(set_frontmatter_model(&removed, None).unwrap(), removed);
    }

    #[test]
    fn crlf_bom_and_continued_values_are_kept() {
        let text =
            "\u{feff}---\r\nname: x\r\nmodel: >\r\n  opus\r\n\r\ntools: Read\r\n---\r\nBody\r\n";
        let out = set_frontmatter_model(text, Some("sonnet")).unwrap();
        assert_eq!(
            out,
            "\u{feff}---\r\nname: x\r\nmodel: sonnet\r\n\r\ntools: Read\r\n---\r\nBody\r\n"
        );
        let added = set_frontmatter_model("---\r\nname: x\r\n---\r\n", Some("opus")).unwrap();
        assert_eq!(added, "---\r\nname: x\r\nmodel: opus\r\n---\r\n");
        assert!(set_frontmatter_model("no frontmatter", Some("opus")).is_none());
        assert!(set_frontmatter_model("---\nname: open", Some("opus")).is_none());
    }

    #[test]
    fn an_indented_model_is_no_field() {
        let text = "---\nname: x\nhooks:\n  model: inner\n---\n";
        assert_eq!(
            set_frontmatter_model(text, Some("opus")).unwrap(),
            "---\nname: x\nhooks:\n  model: inner\nmodel: opus\n---\n"
        );
    }

    const CODEX: &str = "# my agent\nname = \"manager\"\ndescription = \"Reviews\"  # short\nmodel = \"gpt-5\"   # pinned\ndeveloper_instructions = \"\"\"\nBe strict.\n\"\"\"\n\n[extra]\nkeep = 1\n";

    #[test]
    fn toml_fields_change_alone() {
        let choice = ModelChoice {
            model: Some("gpt-6.1-sol".into()),
            effort: None,
        };
        let out = set_toml_model(CODEX, &choice).unwrap();
        assert_eq!(
            out,
            CODEX.replace(
                "model = \"gpt-5\"   # pinned",
                "model = \"gpt-6.1-sol\"   # pinned"
            )
        );
        let both = ModelChoice {
            model: None,
            effort: Some("high".into()),
        };
        let out = set_toml_model(CODEX, &both).unwrap();
        assert!(!out.contains("gpt-5"));
        assert!(out.contains("model_reasoning_effort = \"high\"\n"));
        assert!(
            out.starts_with("# my agent\nname = \"manager\"\ndescription = \"Reviews\"  # short\n")
        );
        assert!(out.contains("developer_instructions = \"\"\"\nBe strict.\n\"\"\"\n"));
        assert!(out.ends_with("[extra]\nkeep = 1\n"));
        let parsed: toml::Table = out.parse().unwrap();
        assert_eq!(parsed["model_reasoning_effort"].as_str(), Some("high"));
        assert!(set_toml_model("not = = toml", &both).is_err());
    }
}
