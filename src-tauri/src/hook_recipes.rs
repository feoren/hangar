//! How the agent gate's hooks reach one session of each agent CLI (`HookFormat`): the files,
//! start arguments and environment names per format. Pure: nothing here writes a file or starts a
//! process; `gate_service.rs` writes the files and hands the arguments to the session manager.
//!
//! Recipes and evidence: docs/wiki/pages/sources/agent-hooks-per-session-research.md and
//! docs/wiki/pages/syntheses/agent-gate-adapters.md. No recipe ever holds the session's token:
//! it travels in the child's environment only ([`crate::gate::TOKEN_ENV`] for HTTP hooks,
//! [`HELPER_ENV`] for the helper command `hangar gate-hook` and for the in-process extensions of
//! Pi, Oh My Pi and OpenCode, which read it themselves).

use std::path::Path;

use crate::agent_catalog::HookFormat;

use crate::gate::{
    HOOK_TIMEOUT_SECS, ROUTE_PRE_TOOL_USE_JSON, ROUTE_USER_PROMPT_SUBMIT_JSON, TOKEN_ENV,
};

/// First argument of the Hangar executable that runs the helper instead of the app.
pub const HELPER_SUBCOMMAND: &str = "gate-hook";
/// Environment variable of the helper with `<port>:<token>`. A neutral name on purpose: Gemini
/// CLI may strip variables whose names contain TOKEN, KEY or AUTH from a hook's environment,
/// and Codex's shell environment policy drops such names by default.
pub const HELPER_ENV: &str = "HANGAR_GATE";

/// Qwen Code reads its system settings from this file instead of the administrator's one.
pub const QWEN_SETTINGS_ENV: &str = "QWEN_CODE_SYSTEM_SETTINGS_PATH";
/// Gemini CLI reads its system settings from this file instead of the administrator's one.
pub const GEMINI_SETTINGS_ENV: &str = "GEMINI_CLI_SYSTEM_SETTINGS_PATH";
/// Gemini CLI drops every hook in a folder it does not trust; this trusts the session's folder
/// for the session only.
pub const GEMINI_TRUST_ENV: &str = "GEMINI_CLI_TRUST_WORKSPACE";
/// Codex runs session hooks only with this flag (no persisted hook trust for a `-c` layer).
pub const CODEX_TRUST_FLAG: &str = "--dangerously-bypass-hook-trust";
/// Copilot CLI loads a plugin (with its hooks) from this folder for one run.
pub const COPILOT_PLUGIN_FLAG: &str = "--plugin-dir";
/// Pi and Oh My Pi load one extension file for one run (measured with Pi 1.5.2).
pub const PI_EXTENSION_FLAG: &str = "-e";
/// Oh My Pi's allowlist flag, which cannot be combined with `-e`.
pub const OMP_TRUSTED_FLAG: &str = "--trusted-extension";
/// OpenCode merges this inline configuration over the user's; its `plugin` list names Hangar's
/// plugin. Measured with 1.19.5: the plugin loads next to the user's own plugins, and unlike a
/// configuration folder (`OPENCODE_CONFIG_DIR`) nothing is installed with npm.
pub const OPENCODE_CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";
/// Droid applies a settings file on top of the user's for one process.
pub const DROID_SETTINGS_FLAG: &str = "--settings";

/// Qwen Code's tool ids for file edits (`write-file.ts`, `edit.ts`).
pub const QWEN_EDIT_MATCHER: &str = "^(write_file|edit)$";
/// Gemini CLI's tool names for file edits (`base-declarations.ts`).
pub const GEMINI_EDIT_MATCHER: &str = "write_file|replace";
/// Copilot CLI's edit tools; Copilot anchors the matcher itself (`^(?:PATTERN)$`).
pub const COPILOT_EDIT_MATCHER: &str = "edit|create|apply_patch";
/// Codex's only file editing tool. Unanchored: `^` is one of the characters a cmd.exe launcher
/// (npm's `codex.cmd`) must not get ([`crate::batch`]); no other Codex tool name contains it.
pub const CODEX_EDIT_MATCHER: &str = "apply_patch";
/// Droid's file editing tools (hooks guide: `Edit`, `Create`, `ApplyPatch`).
pub const DROID_EDIT_MATCHER: &str = "Edit|Create|ApplyPatch";

/// The helper's agent names, as the hook commands pass them (`hangar gate-hook <agent> <event>`).
pub const AGENT_GEMINI: &str = "gemini";
pub const AGENT_COPILOT: &str = "copilot";
pub const AGENT_CODEX: &str = "codex";
pub const AGENT_DROID: &str = "droid";
/// The helper's event names.
pub const EVENT_BEFORE_TOOL: &str = "before-tool";
pub const EVENT_PRE_TOOL_USE: &str = "pre-tool-use";

/// Gemini CLI takes hook timeouts in milliseconds.
const GEMINI_TIMEOUT_MS: u64 = HOOK_TIMEOUT_SECS * 1000;

/// Folder and file names inside a session's own folder.
pub const QWEN_FILE: &str = "qwen-settings.json";
pub const GEMINI_FILE: &str = "gemini-settings.json";
pub const COPILOT_DIR: &str = "copilot-plugin";
pub const COPILOT_MANIFEST: &str = "plugin.json";
pub const COPILOT_HOOKS: &str = "hooks.json";
pub const PI_FILE: &str = "hangar-gate.ts";
pub const OPENCODE_FILE: &str = "hangar-gate.ts";
pub const DROID_FILE: &str = "droid-settings.json";

/// The shared part of the in-process gate (request, paths, fail-open policy) and each agent's
/// entry. Plain JavaScript, which is valid TypeScript, so the agents' loaders (jiti for Pi, Bun
/// for Oh My Pi and OpenCode) take it as a `.ts` module without a build step.
const PLUGIN_CORE: &str = include_str!("gate_plugins/core.js");
const PI_ENTRY: &str = include_str!("gate_plugins/pi.js");
const OPENCODE_ENTRY: &str = include_str!("gate_plugins/opencode.js");

/// Every environment variable the gate sets for a session, of any format. A Hangar started
/// inside a gated session must not hand them on to its own children (`child_env.rs`).
pub const GATE_ENV_NAMES: &[&str] = &[
    TOKEN_ENV,
    HELPER_ENV,
    QWEN_SETTINGS_ENV,
    GEMINI_SETTINGS_ENV,
    GEMINI_TRUST_ENV,
    OPENCODE_CONFIG_CONTENT_ENV,
];

/// Where an administrator's system settings file sits by default: Gemini CLI from its docs and
/// `settings.ts`; Qwen Code's Windows path from its settings docs, the other two by analogy with
/// Gemini CLI, of which Qwen Code is a fork [UNVERIFIED].
pub fn default_system_settings(format: HookFormat) -> Option<&'static Path> {
    let path = match format {
        HookFormat::GeminiSystemSettings if cfg!(windows) => {
            r"C:\ProgramData\gemini-cli\settings.json"
        }
        HookFormat::GeminiSystemSettings if cfg!(target_os = "macos") => {
            "/Library/Application Support/GeminiCli/settings.json"
        }
        HookFormat::GeminiSystemSettings => "/etc/gemini-cli/settings.json",
        HookFormat::QwenSystemSettings if cfg!(windows) => {
            r"C:\ProgramData\qwen-code\settings.json"
        }
        HookFormat::QwenSystemSettings if cfg!(target_os = "macos") => {
            "/Library/Application Support/QwenCode/settings.json"
        }
        HookFormat::QwenSystemSettings => "/etc/qwen-code/settings.json",
        _ => return None,
    };
    Some(Path::new(path))
}

/// The environment variable whose settings the session's own would take the place of, if the
/// format has one. For OpenCode it is an inline configuration of the user's own.
pub fn system_settings_env(format: HookFormat) -> Option<&'static str> {
    match format {
        HookFormat::GeminiSystemSettings => Some(GEMINI_SETTINGS_ENV),
        HookFormat::QwenSystemSettings => Some(QWEN_SETTINGS_ENV),
        HookFormat::OpencodePlugin => Some(OPENCODE_CONFIG_CONTENT_ENV),
        _ => None,
    }
}

/// An administrator's system settings would be replaced by the session's file: Hangar's own
/// environment already names one (`env_value`), or the default file (`default_file`) exists.
/// Then the program is not offered for team sessions at all.
pub fn admin_settings_in(env_value: Option<&std::ffi::OsStr>, default_file: Option<&Path>) -> bool {
    env_value.is_some_and(|value| !value.is_empty()) || default_file.is_some_and(Path::exists)
}

/// [`admin_settings_in`] for this machine; `false` for formats without a system settings file.
pub fn admin_settings_present(format: HookFormat) -> bool {
    let Some(name) = system_settings_env(format) else {
        return false;
    };
    admin_settings_in(
        std::env::var_os(name).as_deref(),
        default_system_settings(format),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecipeError {
    /// The Hangar executable's path cannot stand in the agent's hook command line unchanged.
    #[error("The path of Hangar's program contains characters a hook command cannot carry")]
    InstallPath,
}

/// Which shell reads a hook command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// `cmd.exe /C "<command>"` (Codex on Windows).
    Cmd,
    /// `powershell -NoProfile -NonInteractive -Command <command>` (Gemini CLI on Windows).
    PowerShell,
    /// `sh -c`, `bash -c` or `$SHELL -lc` (both CLIs elsewhere).
    Posix,
}

impl Shell {
    /// The shell the agent uses on this platform: Codex runs cmd.exe, Gemini CLI PowerShell.
    pub fn for_agent(powershell_on_windows: bool) -> Self {
        match (cfg!(windows), powershell_on_windows) {
            (true, true) => Shell::PowerShell,
            (true, false) => Shell::Cmd,
            (false, _) => Shell::Posix,
        }
    }
}

/// A character of the executable's path that every shell above reads as plain text inside the
/// quoting used here. Everything else is refused rather than escaped: a wrong escape would run
/// something else, a refusal only leaves the session without the gate.
/// A backslash is a separator on Windows only; inside a POSIX double quoted string it escapes.
fn path_char_ok(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(c, '/' | ':' | '.' | '_' | '-' | '~' | ' ' | '(' | ')')
        || (cfg!(windows) && c == '\\')
}

/// The command line that runs `<exe> gate-hook <agent> <event>` in `shell`. `bare` asks for no
/// double quote at all (Codex through a cmd.exe launcher, which must not get `"`); then the path
/// must not need quoting either.
pub fn helper_command(
    exe: &Path,
    agent: &str,
    event: &str,
    shell: Shell,
    bare: bool,
) -> Result<String, RecipeError> {
    let path = exe.to_str().ok_or(RecipeError::InstallPath)?;
    if path.is_empty() || !path.chars().all(path_char_ok) || !exe.is_absolute() {
        return Err(RecipeError::InstallPath);
    }
    // cmd.exe reads parentheses as a command block and `@` as echo-off where they start a word;
    // refused whether quoted or not.
    if shell == Shell::Cmd && path.contains(['(', ')', '@']) {
        return Err(RecipeError::InstallPath);
    }
    let needs_quotes = path.contains([' ', '(', ')']);
    let program = match shell {
        // PowerShell: a single quoted string is literal; `&` calls it.
        Shell::PowerShell => format!("& '{path}'"),
        Shell::Posix if bare => return Err(RecipeError::InstallPath),
        // Double quotes: Codex wraps the whole command in a TOML literal string (`'…'`). The
        // path holds none of the characters special inside them (`$`, `` ` ``, `"`, `\`).
        Shell::Posix => format!("\"{path}\""),
        Shell::Cmd if bare && needs_quotes => return Err(RecipeError::InstallPath),
        Shell::Cmd if bare => path.to_owned(),
        Shell::Cmd => format!("\"{path}\""),
    };
    Ok(format!("{program} {HELPER_SUBCOMMAND} {agent} {event}"))
}

/// Qwen Code's system settings for one session: the gate's HTTP hooks in Claude Code's shape.
/// The token is referenced by name only; Qwen fills the header from the environment.
pub fn qwen_settings(port: u16, context: bool) -> String {
    let hook = |route: &str, timeout: u64| {
        serde_json::json!({
            "type": "http",
            "url": format!("http://127.0.0.1:{port}{route}"),
            "timeout": timeout,
            "headers": { "Authorization": format!("Bearer ${{{TOKEN_ENV}}}") },
            "allowedEnvVars": [TOKEN_ENV],
        })
    };
    let mut hooks = serde_json::Map::new();
    hooks.insert(
        "PreToolUse".to_owned(),
        serde_json::json!([{
            "matcher": QWEN_EDIT_MATCHER,
            "hooks": [hook(ROUTE_PRE_TOOL_USE_JSON, HOOK_TIMEOUT_SECS)],
        }]),
    );
    if context {
        hooks.insert(
            "UserPromptSubmit".to_owned(),
            serde_json::json!([{
                "hooks": [hook(ROUTE_USER_PROMPT_SUBMIT_JSON, crate::gate::CONTEXT_HOOK_TIMEOUT_SECS)],
            }]),
        );
    }
    pretty(&serde_json::json!({ "hooks": hooks }))
}

/// Gemini CLI's system settings for one session: a `BeforeTool` command hook that runs the
/// helper. Gemini concatenates it with the user's and the project's hooks.
pub fn gemini_settings(command: &str) -> String {
    pretty(&serde_json::json!({
        "hooks": {
            "BeforeTool": [{
                "matcher": GEMINI_EDIT_MATCHER,
                "hooks": [{
                    "type": "command",
                    "name": "hangar-gate",
                    "command": command,
                    "timeout": GEMINI_TIMEOUT_MS,
                }],
            }],
        },
    }))
}

/// `plugin.json` of the Copilot CLI plugin that carries the gate's hook.
pub fn copilot_manifest() -> String {
    pretty(&serde_json::json!({
        "name": "hangar-gate",
        "version": "1.0.0",
        "description": "Hangar's agent gate for this session",
        "hooks": COPILOT_HOOKS,
    }))
}

/// `hooks.json` of that plugin: the helper runs as a program with arguments, without a shell,
/// so the path needs no quoting.
pub fn copilot_hooks(exe: &Path) -> String {
    pretty(&serde_json::json!({
        "version": 1,
        "hooks": {
            "preToolUse": [{
                "type": "command",
                "exec": exe,
                "args": [HELPER_SUBCOMMAND, AGENT_COPILOT, EVENT_PRE_TOOL_USE],
                "matcher": COPILOT_EDIT_MATCHER,
                "timeoutSec": HOOK_TIMEOUT_SECS,
            }],
        },
    }))
}

/// The `-c` value that gives one Codex run the gate's `PreToolUse` hook. TOML literal strings
/// (`'…'`) need no escapes; [`helper_command`] already refused a path with a `'`.
pub fn codex_hooks_value(command: &str) -> String {
    format!(
        "hooks.PreToolUse=[{{matcher='{CODEX_EDIT_MATCHER}',hooks=[{{type='command',\
         command='{command}',timeout={HOOK_TIMEOUT_SECS}}}]}}]"
    )
}

/// Arguments placed before the user's for Codex: the trust bypass (unless the user's arguments
/// carry it already; Codex refuses it twice) and the hook layer.
pub fn codex_args(command: &str, user_args: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    if !user_args.iter().any(|arg| arg == CODEX_TRUST_FLAG) {
        args.push(CODEX_TRUST_FLAG.to_owned());
    }
    args.push("-c".to_owned());
    args.push(codex_hooks_value(command));
    args
}

/// The extension Pi and Oh My Pi load with `-e`: a `tool_call` handler for `write`, `edit` and
/// (Oh My Pi) `ast_edit` that asks the endpoint with port and token from [`HELPER_ENV`]. The same
/// text for every session; it holds neither port nor token.
pub fn pi_extension() -> String {
    format!("{PLUGIN_CORE}{PI_ENTRY}")
}

/// The OpenCode plugin: a `tool.execute.before` hook for `write`, `edit`, `multiedit`, `patch`
/// and `apply_patch` that throws Hangar's reason on a deny.
pub fn opencode_plugin() -> String {
    format!("{PLUGIN_CORE}{OPENCODE_ENTRY}")
}

/// OpenCode's inline configuration for one session: the plugin file as a `file://` URL (measured
/// form). `None` when the path is not absolute and so has no URL.
pub fn opencode_config(plugin: &Path) -> Option<String> {
    let url = url::Url::from_file_path(plugin).ok()?;
    Some(serde_json::json!({ "plugin": [url.as_str()] }).to_string())
}

/// Droid's settings for one process: a `PreToolUse` command hook on the edit tools that runs the
/// helper; Droid takes the timeout in seconds.
pub fn droid_settings(command: &str) -> String {
    pretty(&serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": DROID_EDIT_MATCHER,
                "hooks": [{
                    "type": "command",
                    "command": command,
                    "timeout": HOOK_TIMEOUT_SECS,
                }],
            }],
        },
    }))
}

fn pretty(value: &serde_json::Value) -> String {
    // A Value always serializes; the fallback keeps the types honest.
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn exe(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    #[test]
    fn an_admins_settings_file_or_variable_counts() {
        let dir = crate::test_support::TempDir::new("admin-settings");
        let file = dir.path().join("settings.json");
        assert!(!admin_settings_in(None, Some(&file)));
        std::fs::write(&file, "{}").unwrap();
        assert!(admin_settings_in(None, Some(&file)));
        assert!(admin_settings_in(
            Some(std::ffi::OsStr::new("x.json")),
            None
        ));
        assert!(!admin_settings_in(Some(std::ffi::OsStr::new("")), None));
        assert!(!admin_settings_in(None, None));
        // Only the two formats with a system settings file have a default one.
        for format in [
            HookFormat::GeminiSystemSettings,
            HookFormat::QwenSystemSettings,
        ] {
            assert!(default_system_settings(format).is_some_and(Path::is_absolute));
            assert!(system_settings_env(format).is_some());
        }
        for format in [
            HookFormat::ClaudeSettings,
            HookFormat::CopilotPlugin,
            HookFormat::CodexConfigFlags,
            HookFormat::PiExtension,
            HookFormat::OmpExtension,
            HookFormat::DroidSettings,
        ] {
            assert!(default_system_settings(format).is_none());
            assert!(system_settings_env(format).is_none());
            assert!(!admin_settings_present(format));
        }
        // OpenCode: only the user's own folder variable counts; there is no system file.
        assert!(default_system_settings(HookFormat::OpencodePlugin).is_none());
        assert_eq!(
            system_settings_env(HookFormat::OpencodePlugin),
            Some("OPENCODE_CONFIG_CONTENT")
        );
    }

    #[test]
    fn helper_command_quotes_per_shell() {
        if cfg!(windows) {
            let plain = exe(r"C:\Users\Ann\AppData\Local\Hangar\hangar.exe");
            assert_eq!(
                helper_command(&plain, "codex", "pre-tool-use", Shell::Cmd, true).unwrap(),
                r"C:\Users\Ann\AppData\Local\Hangar\hangar.exe gate-hook codex pre-tool-use"
            );
            assert_eq!(
                helper_command(&plain, "codex", "pre-tool-use", Shell::Cmd, false).unwrap(),
                r#""C:\Users\Ann\AppData\Local\Hangar\hangar.exe" gate-hook codex pre-tool-use"#
            );
            let spaced = exe(r"C:\Program Files (x86)\Hangar\hangar.exe");
            assert_eq!(
                helper_command(&spaced, "gemini", "before-tool", Shell::PowerShell, false).unwrap(),
                r"& 'C:\Program Files (x86)\Hangar\hangar.exe' gate-hook gemini before-tool"
            );
            // Through a cmd.exe launcher a path that needs quotes cannot go.
            let program_files = exe(r"C:\Program Files\Hangar\hangar.exe");
            assert_eq!(
                helper_command(&program_files, "codex", "pre-tool-use", Shell::Cmd, true),
                Err(RecipeError::InstallPath)
            );
            assert!(
                helper_command(&program_files, "codex", "pre-tool-use", Shell::Cmd, false).is_ok()
            );
            // Parentheses are a command block for cmd.exe, quoted or not.
            for shell_bare in [true, false] {
                assert_eq!(
                    helper_command(&spaced, "codex", "pre-tool-use", Shell::Cmd, shell_bare),
                    Err(RecipeError::InstallPath)
                );
            }
            // Letters of any script stay.
            assert!(helper_command(
                &exe(r"C:\Users\Jürgen\Hangar\hangar.exe"),
                "codex",
                "pre-tool-use",
                Shell::Cmd,
                true
            )
            .is_ok());
        } else {
            let plain = exe("/opt/Hangar App/hangar");
            assert_eq!(
                helper_command(&plain, "gemini", "before-tool", Shell::Posix, false).unwrap(),
                "\"/opt/Hangar App/hangar\" gate-hook gemini before-tool"
            );
            assert_eq!(
                helper_command(
                    &exe("/opt/a\\b/hangar"),
                    "codex",
                    "pre-tool-use",
                    Shell::Posix,
                    false
                ),
                Err(RecipeError::InstallPath)
            );
        }
    }

    #[test]
    fn helper_command_refuses_what_a_shell_would_read() {
        let root = if cfg!(windows) { r"C:\x\" } else { "/x/" };
        for odd in [
            "a'b",
            "a\"b",
            "a%b",
            "a&b",
            "a^b",
            "a!b",
            "a$b",
            "a`b",
            "a;b",
            "a|b",
            "a<b",
            "a>b",
            "a\u{2018}b",
            "a\u{2019}b",
            "a\nb",
            "a\tb",
            "a,b",
            "a=b",
            "a@b",
            "a#b",
        ] {
            let path = exe(&format!(
                "{root}{odd}{}hangar.exe",
                std::path::MAIN_SEPARATOR
            ));
            for shell in [Shell::Cmd, Shell::PowerShell, Shell::Posix] {
                assert_eq!(
                    helper_command(&path, "codex", "pre-tool-use", shell, false),
                    Err(RecipeError::InstallPath),
                    "{odd:?} {shell:?}"
                );
            }
        }
        // A relative path would be found against the agent's folder.
        assert_eq!(
            helper_command(
                &exe("hangar.exe"),
                "codex",
                "pre-tool-use",
                Shell::Cmd,
                false
            ),
            Err(RecipeError::InstallPath)
        );
    }

    #[test]
    fn qwen_settings_reference_the_token_by_name_only() {
        let text = qwen_settings(4711, true);
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let group = &json["hooks"]["PreToolUse"][0];
        assert_eq!(group["matcher"], QWEN_EDIT_MATCHER);
        let hook = &group["hooks"][0];
        assert_eq!(hook["type"], "http");
        assert_eq!(hook["url"], "http://127.0.0.1:4711/v1/pre-tool-use/json");
        assert_eq!(hook["timeout"], 660);
        assert_eq!(
            hook["headers"]["Authorization"],
            "Bearer ${HANGAR_HOOK_TOKEN}"
        );
        assert_eq!(
            hook["allowedEnvVars"],
            serde_json::json!(["HANGAR_HOOK_TOKEN"])
        );
        assert_eq!(
            json["hooks"]["UserPromptSubmit"][0]["hooks"][0]["url"],
            "http://127.0.0.1:4711/v1/user-prompt-submit/json"
        );
        let without: serde_json::Value = serde_json::from_str(&qwen_settings(4711, false)).unwrap();
        assert!(without["hooks"].get("UserPromptSubmit").is_none());
    }

    #[test]
    fn gemini_settings_run_the_helper_with_a_millisecond_timeout() {
        let json: serde_json::Value = serde_json::from_str(&gemini_settings(
            "& 'C:\\h\\hangar.exe' gate-hook gemini before-tool",
        ))
        .unwrap();
        let group = &json["hooks"]["BeforeTool"][0];
        assert_eq!(group["matcher"], "write_file|replace");
        let hook = &group["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert_eq!(
            hook["command"],
            "& 'C:\\h\\hangar.exe' gate-hook gemini before-tool"
        );
        assert_eq!(hook["timeout"], 660_000);
    }

    #[test]
    fn copilot_plugin_runs_the_helper_without_a_shell() {
        let manifest: serde_json::Value = serde_json::from_str(&copilot_manifest()).unwrap();
        assert_eq!(manifest["name"], "hangar-gate");
        assert_eq!(manifest["hooks"], "hooks.json");
        let path = exe(if cfg!(windows) {
            r"C:\Program Files\Hangar\hangar.exe"
        } else {
            "/opt/Hangar App/hangar"
        });
        let hooks: serde_json::Value = serde_json::from_str(&copilot_hooks(&path)).unwrap();
        assert_eq!(hooks["version"], 1);
        let entry = &hooks["hooks"]["preToolUse"][0];
        assert_eq!(entry["type"], "command");
        assert_eq!(entry["exec"], path.to_str().unwrap());
        assert_eq!(
            entry["args"],
            serde_json::json!(["gate-hook", "copilot", "pre-tool-use"])
        );
        assert_eq!(entry["matcher"], "edit|create|apply_patch");
        assert_eq!(entry["timeoutSec"], 660);
    }

    #[test]
    fn plugins_read_port_and_token_from_the_environment_only() {
        for (text, entry) in [
            (pi_extension(), "pi.on(\"tool_call\""),
            (opencode_plugin(), "\"tool.execute.before\""),
        ] {
            assert!(text.contains(entry), "{entry}");
            assert!(text.contains(&format!("const GATE_ENV = \"{HELPER_ENV}\"")));
            assert!(text.contains("process.env[GATE_ENV]"));
            assert!(text.contains(&format!(
                "const ROUTE = \"{}\"",
                crate::gate::ROUTE_PRE_TOOL_USE
            )));
            // Nothing to install, and no fetch: Node's sends Sec-Fetch-Mode, which the endpoint
            // refuses as a browser's request.
            for line in text.lines().filter(|l| l.starts_with("import ")) {
                assert!(line.contains("\"node:"), "{line}");
            }
            assert!(!text.contains("fetch("));
            // The imports come first, so the joined module parses.
            let first_code = text
                .lines()
                .find(|l| !l.starts_with("//") && !l.trim().is_empty())
                .unwrap();
            assert!(first_code.starts_with("import "), "{first_code}");
            assert!(text.is_ascii());
        }
        // Pi takes the default export; OpenCode calls every export, so each has exactly one.
        assert_eq!(pi_extension().matches("export ").count(), 1);
        assert_eq!(opencode_plugin().matches("export ").count(), 1);
    }

    #[test]
    fn opencode_config_names_the_plugin_as_a_file_url() {
        let (plugin, expected) = if cfg!(windows) {
            (
                r"C:\Users\Ann Lee\AppData\Local\Hangar\gate\sessions\1-2-0\hangar-gate.ts",
                "file:///C:/Users/Ann%20Lee/AppData/Local/Hangar/gate/sessions/1-2-0/hangar-gate.ts",
            )
        } else {
            (
                "/home/ann lee/.local/share/hangar/gate/sessions/1-2-0/hangar-gate.ts",
                "file:///home/ann%20lee/.local/share/hangar/gate/sessions/1-2-0/hangar-gate.ts",
            )
        };
        let json: serde_json::Value =
            serde_json::from_str(&opencode_config(Path::new(plugin)).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({ "plugin": [expected] }));
        assert_eq!(opencode_config(Path::new("relative.ts")), None);
    }

    #[test]
    fn droid_settings_run_the_helper_on_the_edit_tools() {
        let command = r#""C:\h\hangar.exe" gate-hook droid pre-tool-use"#;
        let json: serde_json::Value = serde_json::from_str(&droid_settings(command)).unwrap();
        let group = &json["hooks"]["PreToolUse"][0];
        assert_eq!(group["matcher"], "Edit|Create|ApplyPatch");
        let hook = &group["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert_eq!(hook["command"], command);
        assert_eq!(hook["timeout"], 660);
    }

    #[test]
    fn codex_value_is_one_toml_assignment_and_the_flag_comes_once() {
        let command = r"C:\h\hangar.exe gate-hook codex pre-tool-use";
        let value = codex_hooks_value(command);
        assert_eq!(
            value,
            "hooks.PreToolUse=[{matcher='apply_patch',hooks=[{type='command',\
             command='C:\\h\\hangar.exe gate-hook codex pre-tool-use',timeout=660}]}]"
        );
        // Codex parses the part after the first `=` as TOML.
        let (key, toml_value) = value.split_once('=').unwrap();
        assert_eq!(key, "hooks.PreToolUse");
        let parsed: toml::Table = toml::from_str(&format!("v = {toml_value}")).unwrap();
        let hook = &parsed["v"][0]["hooks"][0];
        assert_eq!(hook["command"].as_str(), Some(command));
        assert_eq!(hook["timeout"].as_integer(), Some(660));
        // Through npm's codex.cmd: nothing cmd.exe would interpret.
        assert!(crate::batch::argument_ok(&value));

        let none: Vec<String> = Vec::new();
        assert_eq!(
            codex_args(command, &none),
            [CODEX_TRUST_FLAG.to_owned(), "-c".to_owned(), value.clone()]
        );
        let own = vec!["--dangerously-bypass-hook-trust".to_owned()];
        assert_eq!(codex_args(command, &own), ["-c".to_owned(), value]);
    }
}
