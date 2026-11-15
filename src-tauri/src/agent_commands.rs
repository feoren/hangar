//! Agent commands: short texts such as `/compact` that Hangar can type into a running session of
//! a program, offered in the session menu and in quick access (contract:
//! docs/wiki/pages/syntheses/agent-commands.md).
//!
//! They are data, not code: the built-in catalog lists the verified ones per program
//! (`agent_commands` in `agent_catalog.toml`), and a profile in `profiles.toml` adds its own under
//! the same key. Hangar never interprets the text; the frontend sends it like "Send to several"
//! (bracketed paste, then Enter as a separate write).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Longest text of one command, in characters. Slash commands are short; a long text belongs in
/// a snippet.
const MAX_TEXT: usize = 200;
/// Longest own label, in characters; the session menu has one line per command.
const MAX_LABEL: usize = 40;
/// Most commands per program, so a typo in a generated file cannot flood the menu.
const MAX_COMMANDS: usize = 30;

/// A label Hangar translates itself (`src/i18n.ts`, `agentCommand.<key>`). Its meaning is the
/// same for every program, so `Compact` names the command the context meter's compact button
/// sends, whatever its text is (`/compact`, `/compress`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelKey {
    /// Summarize the conversation to free context (Claude Code and Codex `/compact`, Gemini
    /// `/compress`).
    Compact,
    /// Start over with an empty context.
    Clear,
    /// Start a new conversation (Codex `/new`).
    New,
    /// Choose the model.
    Model,
    /// Cost and plan usage (Claude Code `/usage`).
    Usage,
    /// How full the context is (Claude Code `/context`).
    Context,
    /// Session configuration and token use (Codex `/status`).
    Status,
    /// Session statistics (Gemini `/stats`).
    Stats,
}

/// One command of a program. In TOML exactly one of `label_key` (a label Hangar translates) and
/// `label` (the user's own text) names it; the frontend gets the same fields in camelCase.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCommand {
    #[serde(
        default,
        rename(serialize = "labelKey", deserialize = "label_key"),
        skip_serializing_if = "Option::is_none"
    )]
    pub label_key: Option<LabelKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// What is typed into the session, one line.
    pub text: String,
    /// Enter follows the text; `false` leaves it in the prompt to be completed (`/model `).
    #[serde(default = "submit_default")]
    pub submit: bool,
    /// Ask before sending, for commands that throw the conversation away (`/clear`).
    #[serde(default)]
    pub confirm: bool,
}

fn submit_default() -> bool {
    true
}

impl AgentCommand {
    /// The text as it is compared: two commands with the same text are the same command.
    fn key(&self) -> &str {
        self.text.trim()
    }
}

/// Checks a program's command list the way loading does: every entry has exactly one label, a
/// non-empty single-line text without a leading space and fits the limits, and no text appears
/// twice (trailing spaces aside). The reason names
/// the field, so the reload notice says what to fix.
pub fn check(commands: &[AgentCommand]) -> Result<(), &'static str> {
    if commands.len() > MAX_COMMANDS {
        return Err("agent_commands holds more than 30 entries");
    }
    let mut seen = HashSet::new();
    for command in commands {
        match (&command.label_key, &command.label) {
            (Some(_), Some(_)) => {
                return Err("an agent command takes either label_key or label, not both")
            }
            (None, None) => return Err("an agent command needs label_key or label"),
            (None, Some(label)) => {
                if label.trim().is_empty()
                    || label.chars().count() > MAX_LABEL
                    || label.chars().any(char::is_control)
                {
                    return Err("an agent command label must be 1 to 40 characters on one line");
                }
            }
            (Some(_), None) => {}
        }
        let text = command.key();
        if text.is_empty()
            || command.text.chars().count() > MAX_TEXT
            || command.text.chars().any(char::is_control)
        {
            return Err("an agent command text must be 1 to 200 characters on one line");
        }
        // A leading space turns `/clear` into a plain prompt in the agent's input line.
        if command.text.starts_with(char::is_whitespace) {
            return Err("an agent command text must not start with a space");
        }
        if !seen.insert(text) {
            return Err("an agent command text appears more than once");
        }
    }
    Ok(())
}

/// The program's commands from the catalog followed by the profile's own. An own command with the
/// same text replaces the catalog's in its place, so a profile can relabel one or drop its
/// confirmation without repeating the whole list.
pub fn merge(catalog: &[AgentCommand], own: &[AgentCommand]) -> Vec<AgentCommand> {
    let mut merged: Vec<AgentCommand> = catalog
        .iter()
        .map(|command| {
            own.iter()
                .find(|mine| mine.key() == command.key())
                .unwrap_or(command)
                .clone()
        })
        .collect();
    merged.extend(
        own.iter()
            .filter(|mine| catalog.iter().all(|command| command.key() != mine.key()))
            .cloned(),
    );
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Wrap {
        agent_commands: Vec<AgentCommand>,
    }

    fn parse(source: &str) -> Result<Vec<AgentCommand>, toml::de::Error> {
        toml::from_str::<Wrap>(source).map(|wrap| wrap.agent_commands)
    }

    fn keyed(key: LabelKey, text: &str) -> AgentCommand {
        AgentCommand {
            label_key: Some(key),
            label: None,
            text: text.into(),
            submit: true,
            confirm: false,
        }
    }

    #[test]
    fn fields_and_defaults() {
        let commands = parse(
            "agent_commands = [\n\
             { label_key = 'compact', text = '/compact' },\n\
             { label = 'Pick a model', text = '/model ', submit = false },\n\
             { label_key = 'clear', text = '/clear', confirm = true },\n]",
        )
        .unwrap();
        assert_eq!(commands[0], keyed(LabelKey::Compact, "/compact"));
        assert!(commands[0].submit, "submit defaults to true");
        assert!(!commands[0].confirm, "confirm defaults to false");
        assert_eq!(commands[1].label.as_deref(), Some("Pick a model"));
        assert!(!commands[1].submit);
        assert!(commands[2].confirm);
        assert_eq!(check(&commands), Ok(()));
    }

    #[test]
    fn unknown_fields_and_label_keys_fail_to_parse() {
        for bad in [
            "agent_commands = [{ label_key = 'explode', text = '/x' }]",
            "agent_commands = [{ label = 'X', text = '/x', enter = true }]",
            "agent_commands = [{ label = 'X' }]",
            "agent_commands = [{ labelKey = 'compact', text = '/x' }]",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn invalid_entries_are_reported() {
        let long_label = "x".repeat(41);
        let long_text = format!("/{}", "x".repeat(200));
        let cases: Vec<(String, &str)> = vec![
            (
                "{ label_key = 'compact', label = 'Compact', text = '/compact' }".into(),
                "not both",
            ),
            ("{ text = '/compact' }".into(), "needs label_key or label"),
            ("{ label = '  ', text = '/x' }".into(), "label must be"),
            (
                format!("{{ label = '{long_label}', text = '/x' }}"),
                "label must be",
            ),
            ("{ label = 'X', text = '   ' }".into(), "text must be"),
            ("{ label = 'X', text = \"/a\\nrm\" }".into(), "text must be"),
            (
                format!("{{ label = 'X', text = '{long_text}' }}"),
                "text must be",
            ),
            (
                "{ label = 'X', text = ' /x' }".into(),
                "not start with a space",
            ),
            (
                "{ label = 'A', text = '/x' }, { label = 'B', text = '/x ' }".into(),
                "more than once",
            ),
        ];
        for (entries, problem) in cases {
            let commands = parse(&format!("agent_commands = [{entries}]")).unwrap();
            let error = check(&commands).unwrap_err();
            assert!(error.contains(problem), "{entries}: {error}");
        }
        let many: Vec<AgentCommand> = (0..31)
            .map(|n| keyed(LabelKey::Model, &format!("/m{n}")))
            .collect();
        assert!(check(&many).is_err());
    }

    #[test]
    fn own_commands_follow_the_catalog_and_replace_the_same_text() {
        let catalog = [
            keyed(LabelKey::Compact, "/compact"),
            AgentCommand {
                confirm: true,
                ..keyed(LabelKey::Clear, "/clear")
            },
        ];
        let own = [
            AgentCommand {
                label: Some("Review".into()),
                label_key: None,
                ..keyed(LabelKey::Model, "/review")
            },
            // Same text as the catalog's: replaces it in place, here without the confirmation.
            keyed(LabelKey::Clear, "/clear "),
        ];
        let merged = merge(&catalog, &own);
        let texts: Vec<&str> = merged.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["/compact", "/clear ", "/review"]);
        assert!(!merged[1].confirm);
        assert_eq!(merge(&catalog, &[]), catalog);
        assert_eq!(merge(&[], &own), own);
    }

    #[test]
    fn the_frontend_gets_camel_case_without_empty_fields() {
        let json = serde_json::to_value(keyed(LabelKey::Compact, "/compact")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "labelKey": "compact",
                "text": "/compact",
                "submit": true,
                "confirm": false,
            })
        );
    }
}
