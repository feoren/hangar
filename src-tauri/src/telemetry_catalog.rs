//! Allowlists of the anonymous usage statistics (report schema 1), the one place that names every
//! value a daily report may carry. The receiving server checks against the same lists and stores
//! anything else as `other`; Hangar maps to them before sending, so no free text ever leaves the
//! machine. A new agent, scheme or font of the app gets its row here, nowhere else.
//!
//! Field list for users: [`INFO_URL`].

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;
/// The official endpoint. Reports never go anywhere else, not to a self-hosted team server.
pub const ENDPOINT: &str = "https://hangar.feoren.com/api/telemetry/v1/daily";
/// The public page that lists every field of a report.
pub const INFO_URL: &str = "https://hangar.feoren.com/telemetry";
/// The server refuses larger bodies (413).
pub const MAX_REPORT_BYTES: usize = 8 * 1024;
/// Every counter stops here; the server caps at the same value.
pub const MAX_COUNT: u32 = 10_000;
pub const MAX_SHARED_PROJECTS: u32 = 100;
pub const FONT_SIZE_MIN: u32 = 8;
pub const FONT_SIZE_MAX: u32 = 32;
pub const MACOS_RELEASE_MIN: u32 = 13;
pub const MACOS_RELEASE_MAX: u32 = 30;
/// The value for anything outside a list.
pub const OTHER: &str = "other";

pub const LANGUAGES: &[&str] = &[
    "de", "en", "el", "es", "fr", "it", "ja", "pl", "pt", "ru", "tr", "zh",
];
pub const THEMES: &[&str] = &["system", "light", "dark"];
pub const CURSOR_STYLES: &[&str] = &["block", "bar", "underline"];
pub const DOT_ANIMATIONS: &[&str] = &["bounce", "wave", "pulse", "fade"];
pub const TERMINAL_FONTS: &[&str] = &[
    "default",
    "JetBrains Mono",
    "Fira Code",
    "Geist Mono",
    "Hack",
    "Source Code Pro",
    "IBM Plex Mono",
    "Victor Mono",
    "Cascadia Mono",
    "Cascadia Code",
    "Consolas",
    "Lucida Console",
    "SF Mono",
    "Menlo",
    "Monaco",
    "DejaVu Sans Mono",
    "Ubuntu Mono",
    "Noto Sans Mono",
    "Liberation Mono",
];

/// Terminal color schemes: the app's key and the value the schema lists. The schema keeps the
/// palette names Hangar used in 0.6.0; each palette got its own name in 0.7.0 (ADR 0009) and
/// stayed the same, so every key maps to exactly one value (`terminalSchemes.ts`).
pub const TERMINAL_SCHEMES: &[(&str, &str)] = &[
    ("hangar", "hangar"),
    ("dusk", "dusk"),
    ("meridian", "oneDarkPro"),
    ("harbor", "github"),
    ("studio", "vscode"),
    ("anvil", "darcula"),
    ("sundial", "solarized"),
    ("velvet", "catppuccin"),
    ("ember", "monokai"),
    ("nocturne", "dracula"),
    ("fjord", "nord"),
    ("ochre", "gruvbox"),
    ("neon", "tokyoNight"),
    ("heather", "rosePine"),
];

/// File viewer themes, mapped like [`TERMINAL_SCHEMES`] (`viewerThemes.ts`).
pub const VIEWER_THEMES: &[(&str, &str)] = &[
    ("meridian", "oneDarkPro"),
    ("harbor", "github"),
    ("studio", "vscode"),
    ("anvil", "darcula"),
    ("sundial", "solarized"),
    ("velvet", "catppuccin"),
    ("ember", "monokai"),
];

/// Agents of the catalog the schema names by their catalog id. Other catalog agents count as
/// [`OTHER`].
pub const AGENTS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "antigravity",
    "copilot",
    "cursor",
    "kimi",
    "qwen",
    "kiro",
    "opencode",
    "aider",
    "pi",
];

/// Shells by the file name of their program (without extension, lower case) and their key.
pub const SHELLS: &[(&str, &str)] = &[
    ("pwsh", "shell:pwsh"),
    ("powershell", "shell:powershell"),
    ("cmd", "shell:cmd"),
    ("bash", "shell:bash"),
    ("zsh", "shell:zsh"),
    ("fish", "shell:fish"),
    ("wsl", "shell:wsl"),
];

/// A program the user set up in profiles.toml that is neither a known agent nor a shell.
pub const CUSTOM: &str = "custom";

/// Every program key a report may carry.
pub fn is_program(key: &str) -> bool {
    AGENTS.contains(&key)
        || SHELLS.iter().any(|(_, shell)| *shell == key)
        || key == CUSTOM
        || key == OTHER
}

/// The functions whose use a report counts. Unit variants with fixed names: the window can name
/// nothing else, and an unknown name fails to deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    FileViewerOpened,
    CommitHistoryOpened,
    NotesCreated,
    SidePanelDetached,
    BroadcastSent,
    PathMentioned,
    FolderRevealed,
    UpdateInstalled,
}

impl Feature {
    pub const ALL: [Self; 8] = [
        Self::FileViewerOpened,
        Self::CommitHistoryOpened,
        Self::NotesCreated,
        Self::SidePanelDetached,
        Self::BroadcastSent,
        Self::PathMentioned,
        Self::FolderRevealed,
        Self::UpdateInstalled,
    ];
}

/// `value` when the list holds it, otherwise [`OTHER`].
pub fn pick(value: &str, allowed: &[&'static str]) -> &'static str {
    allowed
        .iter()
        .find(|item| **item == value)
        .copied()
        .unwrap_or(OTHER)
}

/// The schema's value for an app key of a mapped list, otherwise [`OTHER`].
pub fn map(value: &str, mapping: &[(&str, &'static str)]) -> &'static str {
    mapping
        .iter()
        .find(|(app, _)| *app == value)
        .map_or(OTHER, |(_, wire)| *wire)
}

/// A schema value of a mapped list kept as it is (a stored snapshot read back), otherwise
/// [`OTHER`].
pub fn wire(value: &str, mapping: &[(&str, &'static str)]) -> &'static str {
    mapping
        .iter()
        .find(|(_, wire)| *wire == value)
        .map_or(OTHER, |(_, wire)| *wire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_lists_are_one_to_one() {
        for mapping in [TERMINAL_SCHEMES, VIEWER_THEMES] {
            for (index, (app, wire_value)) in mapping.iter().enumerate() {
                assert_eq!(map(app, mapping), *wire_value);
                assert_eq!(wire(wire_value, mapping), *wire_value);
                assert!(mapping[index + 1..]
                    .iter()
                    .all(|(a, w)| a != app && w != wire_value));
            }
        }
    }

    #[test]
    fn values_outside_a_list_become_other() {
        assert_eq!(pick("dark", THEMES), "dark");
        assert_eq!(pick("Dark", THEMES), OTHER);
        assert_eq!(pick("", LANGUAGES), OTHER);
        assert_eq!(map("oneDarkPro", TERMINAL_SCHEMES), OTHER);
        assert_eq!(map("meridian", TERMINAL_SCHEMES), "oneDarkPro");
        assert_eq!(wire("meridian", VIEWER_THEMES), OTHER);
    }

    #[test]
    fn program_keys_cover_agents_shells_custom_and_other() {
        for key in ["claude", "pi", "shell:pwsh", "shell:wsl", "custom", "other"] {
            assert!(is_program(key), "{key}");
        }
        for key in ["goose", "pwsh", "shell:sh", "Claude", ""] {
            assert!(!is_program(key), "{key}");
        }
    }

    #[test]
    fn features_have_the_schema_names() {
        let names: Vec<String> = Feature::ALL
            .iter()
            .map(|feature| {
                serde_json::to_value(feature)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            names,
            [
                "fileViewerOpened",
                "commitHistoryOpened",
                "notesCreated",
                "sidePanelDetached",
                "broadcastSent",
                "pathMentioned",
                "folderRevealed",
                "updateInstalled",
            ]
        );
        assert!(serde_json::from_str::<Feature>("\"somethingElse\"").is_err());
    }
}
