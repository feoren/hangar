//! Markdown sub-agent definitions of GitHub Copilot CLI, OpenCode 2.x and Gemini CLI: one file
//! per agent with a YAML frontmatter, the prompt below it. Hangar writes copies of the user's
//! Claude Code definitions into each program's global folder ([`crate::agent_provision`]).
//! Formats measured on Windows 11 with Copilot CLI 1.0.114, OpenCode 2.3.2 and Gemini CLI 0.70.0:
//! docs/wiki/pages/sources/sub-agent-formats-measurement.md.
//!
//! | Program | Folder | File | Fields |
//! | --- | --- | --- | --- |
//! | Copilot CLI | `~/.copilot/agents` | `<name>.agent.md` (id = file name) | `name`, `description` |
//! | OpenCode | `~/.config/opencode/agents` | `<name>.md` (id = file name) | `description`, `mode: subagent` |
//! | Gemini CLI | `~/.gemini/agents` | `<name>.md` (id = `name`) | `name`, `description` |
//!
//! Gemini CLI rejects any other frontmatter key, so a copy carries exactly these fields. A copy's
//! second line, right after the opening `---`, is a YAML comment naming its source and the SHA-256
//! of everything after the line; all three programs read past it (measured). Only a file whose
//! marker is intact counts as Hangar's own.

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::agent_defs::valid_name;
use crate::agent_models::DefProgram;
use crate::codex_agents::Marker;

/// One of the Markdown formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdFormat {
    Copilot,
    Opencode,
    Gemini,
}

impl MdFormat {
    pub fn of(program: DefProgram) -> Option<Self> {
        match program {
            DefProgram::Copilot => Some(Self::Copilot),
            DefProgram::Opencode => Some(Self::Opencode),
            DefProgram::Gemini => Some(Self::Gemini),
            DefProgram::Claude | DefProgram::Codex => None,
        }
    }

    /// The global agents folder below `home`. A set `COPILOT_HOME`, `XDG_CONFIG_HOME` or
    /// `GEMINI_CLI_HOME` moves the folder the program reads; Hangar keeps to the default one.
    pub fn agents_dir(self, home: &Path) -> PathBuf {
        match self {
            Self::Copilot => home.join(".copilot").join("agents"),
            Self::Opencode => home.join(".config").join("opencode").join("agents"),
            Self::Gemini => home.join(".gemini").join("agents"),
        }
    }

    /// The folder as an issue names it.
    pub fn dir_label(self) -> &'static str {
        match self {
            Self::Copilot => ".copilot/agents",
            Self::Opencode => ".config/opencode/agents",
            Self::Gemini => ".gemini/agents",
        }
    }

    /// The ending of a copy's file name.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Copilot => ".agent.md",
            Self::Opencode | Self::Gemini => ".md",
        }
    }

    /// The agent a file of the folder stands for by its name, when the program reads it at all:
    /// Copilot CLI drops `.agent.md` or `.md`, OpenCode and Gemini CLI read `.md` (Gemini CLI
    /// skips names starting with `_`). Case is ignored, as on Windows' file systems.
    pub fn stem(self, file: &str) -> Option<&str> {
        let lower = file.to_ascii_lowercase();
        let cut = |ending: &str| {
            lower
                .ends_with(ending)
                .then(|| &file[..file.len() - ending.len()])
        };
        let stem = match self {
            Self::Copilot => cut(".agent.md").or_else(|| cut(".md")),
            Self::Opencode => cut(".md"),
            Self::Gemini => cut(".md").filter(|_| !file.starts_with('_')),
        }?;
        (!stem.is_empty()).then_some(stem)
    }

    /// The agent name a file holds: its `name` for Gemini CLI, which ignores the file name, and
    /// only when Gemini CLI would take it (`^[a-z0-9-_]+$`); the file name for the others.
    pub fn name_of(self, file: &str, text: &str) -> Option<String> {
        match self {
            Self::Gemini => crate::agent_defs::frontmatter(text)
                .and_then(|fields| match fields.get("name")? {
                    crate::agent_defs::Value::Text(name) => Some(name.trim().to_owned()),
                    crate::agent_defs::Value::List(_) => None,
                })
                .filter(|name| gemini_name(name)),
            Self::Copilot | Self::Opencode => self.stem(file).map(str::to_owned),
        }
    }

    /// Whether the program knows an agent by the `name` inside the file rather than by the
    /// file name (Gemini CLI). A file named after a definition but holding another valid name
    /// is then another agent, not the definition's.
    pub fn names_inside(self) -> bool {
        self == Self::Gemini
    }

    /// The backup names Hangar writes for `source`: `<source><suffix>.bak` and `.bak.<n>`.
    pub fn backup_name_of(self, source: &str, name: &str) -> bool {
        let Some(rest) = name
            .strip_prefix(source)
            .and_then(|rest| rest.strip_prefix(self.suffix()))
            .and_then(|rest| rest.strip_prefix(".bak"))
        else {
            return false;
        };
        rest.is_empty()
            || rest.strip_prefix('.').is_some_and(|n| {
                !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit())
            })
    }

    /// The intact marker of `text`, or `None` (no marker, or the rest was changed).
    pub fn marker(self, text: &str) -> Option<Marker> {
        self.read_marker(text, true)
    }

    /// The marker without checking the hash: a copy edited by hand still names the backup of the
    /// user's original, which must not be lost.
    pub fn marker_unchecked(self, text: &str) -> Option<Marker> {
        self.read_marker(text, false)
    }

    fn read_marker(self, text: &str, check: bool) -> Option<Marker> {
        let rest = text.strip_prefix("---\n")?;
        let (line, rest) = rest.split_once('\n')?;
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
        if !valid_name(source) || backup.is_some_and(|backup| !self.backup_name_of(source, backup))
        {
            return None;
        }
        Some(Marker {
            source: source.to_owned(),
            backup: backup.map(str::to_owned),
        })
    }

    /// The file of a copy. Line breaks are LF; the prompt's CRLF becomes LF.
    pub fn render(
        self,
        name: &str,
        description: &str,
        instructions: &str,
        backup: Option<&str>,
    ) -> String {
        let instructions = instructions.replace("\r\n", "\n");
        let mut rest = String::new();
        if matches!(self, Self::Copilot | Self::Gemini) {
            rest.push_str(&format!("name: {}\n", yaml_string(name)));
        }
        rest.push_str(&format!("description: {}\n", yaml_string(description)));
        if self == Self::Opencode {
            // Without it OpenCode 2.x makes the agent a primary one, not a sub-agent.
            rest.push_str("mode: subagent\n");
        }
        rest.push_str("---\n\n");
        rest.push_str(instructions.trim_end_matches('\n'));
        rest.push('\n');
        let backup = backup
            .map(|backup| format!("{MARKER_BACKUP}{backup}."))
            .unwrap_or_default();
        format!(
            "---\n{MARKER_START}{name}.md.{backup}{MARKER_HASH}{}\n{rest}",
            sha256_hex(rest.as_bytes())
        )
    }
}

/// Gemini CLI 0.70.0's name rule for an agent (`nameSchema`, a slug).
fn gemini_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
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

/// A YAML double-quoted scalar: `"` and `\` escaped, line breaks, tabs and every other control
/// or line separator character as an escape, so the value stays on one line.
fn yaml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}') => {
                out.push_str(&format!("\\u{:04X}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [MdFormat; 3] = [MdFormat::Copilot, MdFormat::Opencode, MdFormat::Gemini];

    #[test]
    fn copies_have_the_measured_shape() {
        let copilot = MdFormat::Copilot.render("backend", "Backend: APIs", "Do it.\r\nWell.", None);
        let (first, rest) = copilot.split_once('\n').unwrap();
        assert_eq!(first, "---");
        let (marker, rest) = rest.split_once('\n').unwrap();
        assert!(marker.starts_with("# Managed by Hangar from ~/.claude/agents/backend.md. sha256:"));
        assert_eq!(
            rest,
            "name: \"backend\"\ndescription: \"Backend: APIs\"\n---\n\nDo it.\nWell.\n"
        );
        let opencode = MdFormat::Opencode.render("backend", "d", "x", None);
        assert!(opencode.ends_with("description: \"d\"\nmode: subagent\n---\n\nx\n"));
        assert!(!opencode.contains("name:"));
        let gemini = MdFormat::Gemini.render("backend", "d", "x", None);
        assert!(gemini.ends_with("name: \"backend\"\ndescription: \"d\"\n---\n\nx\n"));
        // Gemini CLI rejects any other key: none slips in.
        let fields = crate::agent_defs::frontmatter(&gemini).unwrap();
        assert_eq!(fields.keys().collect::<Vec<_>>(), ["description", "name"]);
    }

    #[test]
    fn descriptions_stay_one_quoted_line() {
        let text = MdFormat::Gemini.render("a", "say \"hi\"\\ now\nnext\u{2028}", "x", None);
        assert!(text.contains("description: \"say \\\"hi\\\"\\\\ now\\nnext\\u2028\"\n"));
        let fields = crate::agent_defs::frontmatter(&text).unwrap();
        assert!(fields.contains_key("description"));
    }

    #[test]
    fn markers_check_their_hash_and_backup() {
        for format in ALL {
            let backup = format!("manager{}.bak.2", format.suffix());
            let text = format.render("manager", "d", "Be strict.", Some(&backup));
            let marker = format.marker(&text).unwrap();
            assert_eq!(marker.source, "manager");
            assert_eq!(marker.backup.as_deref(), Some(backup.as_str()));
            let edited = text.replace("strict", "kind");
            assert!(format.marker(&edited).is_none());
            assert_eq!(format.marker_unchecked(&edited), Some(marker));
            // Another definition's file is never named as a backup.
            let foreign = format.render("manager", "d", "x", Some("other.md.bak"));
            assert!(format.marker(&foreign).is_none());
        }
        // A Codex CLI marker line is no Markdown marker.
        assert!(MdFormat::Gemini
            .marker("# Managed by Hangar from ~/.claude/agents/a.md. sha256:00\n")
            .is_none());
    }

    #[test]
    fn file_names_map_to_agents_as_each_program_reads_them() {
        assert_eq!(MdFormat::Copilot.stem("manager.agent.md"), Some("manager"));
        assert_eq!(MdFormat::Copilot.stem("manager.md"), Some("manager"));
        assert_eq!(MdFormat::Copilot.stem("Manager.Agent.MD"), Some("Manager"));
        assert_eq!(MdFormat::Copilot.stem("manager.agent.md.bak"), None);
        assert_eq!(MdFormat::Opencode.stem("manager.md"), Some("manager"));
        assert_eq!(MdFormat::Opencode.stem("manager.toml"), None);
        assert_eq!(MdFormat::Gemini.stem("_draft.md"), None);
        assert_eq!(MdFormat::Gemini.stem(".md"), None);
        let gemini = "---\nname: lead\ndescription: d\n---\nx\n";
        assert_eq!(
            MdFormat::Gemini.name_of("other.md", gemini).as_deref(),
            Some("lead")
        );
        assert_eq!(
            MdFormat::Opencode.name_of("other.md", gemini).as_deref(),
            Some("other")
        );
        assert_eq!(MdFormat::Gemini.name_of("x.md", "no frontmatter"), None);
        // A name Gemini CLI refuses is no name: the file fails there and counts by its file name.
        let refused = "---\nname: Lead Agent\ndescription: d\n---\nx\n";
        assert_eq!(MdFormat::Gemini.name_of("x.md", refused), None);
        let quoted = "---\nname: \"lead_2\"\ndescription: d\n---\nx\n";
        assert_eq!(
            MdFormat::Gemini.name_of("x.md", quoted).as_deref(),
            Some("lead_2")
        );
        assert!(MdFormat::Gemini.names_inside());
        assert!(!MdFormat::Copilot.names_inside() && !MdFormat::Opencode.names_inside());
    }

    #[test]
    fn folders_are_the_programs_global_ones() {
        let home = Path::new("home");
        assert_eq!(
            MdFormat::Copilot.agents_dir(home),
            home.join(".copilot").join("agents")
        );
        assert_eq!(
            MdFormat::Opencode.agents_dir(home),
            home.join(".config").join("opencode").join("agents")
        );
        assert_eq!(
            MdFormat::Gemini.agents_dir(home),
            home.join(".gemini").join("agents")
        );
        assert_eq!(MdFormat::of(DefProgram::Codex), None);
        assert_eq!(MdFormat::of(DefProgram::Gemini), Some(MdFormat::Gemini));
    }
}
