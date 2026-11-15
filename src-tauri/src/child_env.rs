//! The environment of every program Hangar starts: sessions in a PTY (`manager.rs`) and the
//! version query (`version.rs`). One rule for both, so a protection added for one cannot be
//! missing in the other.

use std::collections::BTreeMap;

use crate::manager::{INHERITED_SESSION_MARKERS, NO_CWD_IN_EXE_PATH};

/// One change to the inherited environment, in the order it must be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'a> {
    Set(&'a str, &'a str),
    Remove(&'a str),
}

/// Something a child environment can be written to.
pub trait Target {
    fn set(&mut self, key: &str, value: &str);
    fn remove(&mut self, key: &str);
}

impl Target for std::process::Command {
    fn set(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
    fn remove(&mut self, key: &str) {
        self.env_remove(key);
    }
}

impl Target for portable_pty::CommandBuilder {
    fn set(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
    fn remove(&mut self, key: &str) {
        self.env_remove(key);
    }
}

/// The changes, in order: terminal type, the calling agent session's markers and the gate token
/// ([`crate::gate::TOKEN_ENV`]) removed, on
/// Windows `NoDefaultCurrentDirectoryInExePath=1` when `no_cwd_lookup` asks for it, then the
/// profile's `env`. That comes after the protection so a profile can change it deliberately; an
/// empty value for the variable removes it (only its presence counts, not its value).
pub fn changes(profile_env: &BTreeMap<String, String>, no_cwd_lookup: bool) -> Vec<Change<'_>> {
    let inside_gate = [crate::gate::TOKEN_ENV, crate::hook_recipes::HELPER_ENV]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
    changes_with(profile_env, no_cwd_lookup, inside_gate)
}

/// [`changes`] with `inside_gate`: Hangar itself runs inside a gated session (it inherited a
/// gate token). Then every variable the gate sets ([`crate::hook_recipes::GATE_ENV_NAMES`]) is
/// removed, since they all came from that session's gate. Otherwise only the token variables
/// go: a `GEMINI_CLI_SYSTEM_SETTINGS_PATH` or trust variable without a token is the user's or an
/// administrator's own and stays for private sessions.
pub fn changes_with(
    profile_env: &BTreeMap<String, String>,
    no_cwd_lookup: bool,
    inside_gate: bool,
) -> Vec<Change<'_>> {
    let mut out = vec![
        // Agent CLIs expect a real terminal that supports color.
        Change::Set("TERM", "xterm-256color"),
        Change::Set("COLORTERM", "truecolor"),
    ];
    out.extend(
        INHERITED_SESSION_MARKERS
            .iter()
            .map(|key| Change::Remove(key)),
    );
    // A Hangar started inside a gated session must not hand that session's token, settings files
    // or folder trust on. A gated session gets its own afterwards, from the gate, never from here.
    out.push(Change::Remove(crate::gate::TOKEN_ENV));
    out.push(Change::Remove(crate::hook_recipes::HELPER_ENV));
    if inside_gate {
        out.extend(
            crate::hook_recipes::GATE_ENV_NAMES
                .iter()
                .filter(|name| {
                    **name != crate::gate::TOKEN_ENV && **name != crate::hook_recipes::HELPER_ENV
                })
                .map(|name| Change::Remove(name)),
        );
    }
    if cfg!(windows) && no_cwd_lookup {
        // Otherwise cmd.exe (and with it every `.cmd` launcher such as `codex.cmd`) looks for
        // `node` in the working folder first, where a foreign project could drop `node.cmd`.
        out.push(Change::Set(NO_CWD_IN_EXE_PATH, "1"));
    }
    for (key, value) in profile_env {
        if value.is_empty() && key.eq_ignore_ascii_case(NO_CWD_IN_EXE_PATH) {
            out.push(Change::Remove(key));
        } else {
            out.push(Change::Set(key, value));
        }
    }
    out
}

/// Prefix of the variables VS Code's git extension sets in its integrated terminal (the askpass
/// helper's IPC handle among them).
const VSCODE_GIT_PREFIX: &str = "VSCODE_GIT_";

/// Names among `inherited` that belong to an editor Hangar was started from (VS Code and its
/// forks): its git credential helper and IPC handles. A session would otherwise ask that editor
/// for git credentials, or reach into it, and the editor need not even run any more. Exact names
/// such as `VSCODE_IPC_HOOK_CLI` are in [`INHERITED_SESSION_MARKERS`]; this covers the prefix
/// and `GIT_ASKPASS` or `SSH_ASKPASS` when it points to an editor's askpass helper (the user's
/// own helper stays).
pub fn ide_variables<'a>(inherited: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    inherited
        .into_iter()
        .filter(|(name, value)| {
            let upper = name.to_ascii_uppercase();
            upper.starts_with(VSCODE_GIT_PREFIX)
                || (matches!(upper.as_str(), "GIT_ASKPASS" | "SSH_ASKPASS")
                    && is_editor_askpass(value))
        })
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// An editor's askpass helper: the script of VS Code's built-in git extension in any of its
/// installations (`…/resources/app/extensions/git/dist/askpass.sh`, also in Cursor, VSCodium and
/// the like), and the scripts JetBrains IDEs write into their temporary folder
/// (`intellij-git-askpass-local.sh`, `intellij-ssh-askpass-local.sh`, `.bat` on Windows).
fn is_editor_askpass(value: &str) -> bool {
    let normalized = value.replace('\\', "/").to_ascii_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or_default();
    normalized.contains("/extensions/git/dist/askpass")
        || file.starts_with("intellij-git-askpass")
        || file.starts_with("intellij-ssh-askpass")
}

pub fn apply(
    target: &mut impl Target,
    profile_env: &BTreeMap<String, String>,
    no_cwd_lookup: bool,
) {
    // Names and values that are not UTF-8 are no editor's variables.
    let inherited: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    // First, so a profile can still set one of them on purpose.
    for name in ide_variables(inherited.iter().map(|(n, v)| (n.as_str(), v.as_str()))) {
        target.remove(&name);
    }
    for change in changes(profile_env, no_cwd_lookup) {
        match change {
            Change::Set(key, value) => target.set(key, value),
            Change::Remove(key) => target.remove(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn markers_go_and_the_profile_comes_last() {
        let profile = env(&[("CLAUDECODE", "1"), ("X", "y")]);
        let all = changes(&profile, false);
        let removed = all.iter().position(|c| *c == Change::Remove("CLAUDECODE"));
        let set = all
            .iter()
            .position(|c| *c == Change::Set("CLAUDECODE", "1"));
        // A profile may set a marker again on purpose.
        assert!(removed < set, "{all:?}");
        assert!(all.contains(&Change::Set("TERM", "xterm-256color")));
        assert!(all.contains(&Change::Set("X", "y")));
        assert!(!all
            .iter()
            .any(|c| matches!(c, Change::Set(NO_CWD_IN_EXE_PATH, _))));
    }

    #[test]
    fn the_gate_token_of_a_calling_session_never_reaches_a_child() {
        let none = BTreeMap::new();
        for no_cwd_lookup in [false, true] {
            let all = changes(&none, no_cwd_lookup);
            assert!(
                all.contains(&Change::Remove(crate::gate::TOKEN_ENV)),
                "{all:?}"
            );
        }
    }

    #[test]
    fn inside_a_gated_session_every_gate_variable_goes() {
        let none = BTreeMap::new();
        let inside = changes_with(&none, false, true);
        for name in crate::hook_recipes::GATE_ENV_NAMES {
            assert!(inside.contains(&Change::Remove(name)), "{name}: {inside:?}");
        }
        // Outside one, only the token variables: settings and trust are the user's own.
        let outside = changes_with(&none, false, false);
        assert!(outside.contains(&Change::Remove(crate::gate::TOKEN_ENV)));
        assert!(outside.contains(&Change::Remove(crate::hook_recipes::HELPER_ENV)));
        assert!(!outside.contains(&Change::Remove(crate::hook_recipes::GEMINI_SETTINGS_ENV)));
    }

    #[test]
    fn editor_variables_are_found() {
        let inherited = [
            ("VSCODE_GIT_IPC_HANDLE", "\\\\.\\pipe\\vscode-git-marker"),
            ("vscode_git_askpass_main", "marker"),
            (
                "GIT_ASKPASS",
                r"C:\Users\x\AppData\Local\Programs\Microsoft VS Code\resources\app\extensions\git\dist\askpass.sh",
            ),
            ("PATH", "/usr/bin"),
            ("VSCODE_PID", "1"),
        ];
        assert_eq!(
            ide_variables(inherited),
            [
                "VSCODE_GIT_IPC_HANDLE",
                "vscode_git_askpass_main",
                "GIT_ASKPASS"
            ]
        );
        // The user's own credential helper stays.
        let own = [("GIT_ASKPASS", "/usr/lib/ssh/x11-ssh-askpass")];
        assert!(ide_variables(own).is_empty());
        let cursor = [(
            "GIT_ASKPASS",
            "/Applications/Cursor.app/Contents/Resources/app/extensions/git/dist/askpass.sh",
        )];
        assert_eq!(ide_variables(cursor), ["GIT_ASKPASS"]);
    }

    #[test]
    fn jetbrains_askpass_helpers_are_found() {
        let inherited = [
            (
                "GIT_ASKPASS",
                r"C:\Users\x\AppData\Local\JetBrains\IntelliJIdea2025.2\tmp\intellij-git-askpass-local.bat",
            ),
            (
                "SSH_ASKPASS",
                "/home/x/.cache/JetBrains/PyCharm2025.2/tmp/intellij-ssh-askpass-local.sh",
            ),
            (
                "Git_AskPass",
                "/Applications/Cursor.app/x/extensions/git/dist/askpass.sh",
            ),
        ];
        assert_eq!(
            ide_variables(inherited),
            ["GIT_ASKPASS", "SSH_ASKPASS", "Git_AskPass"]
        );
        // A helper of the user's own with a similar word in its folder stays.
        let own = [("SSH_ASKPASS", "/opt/intellij-tools/bin/my-askpass")];
        assert!(ide_variables(own).is_empty());
    }

    #[test]
    fn exact_editor_variables_are_removed_like_session_markers() {
        let none = BTreeMap::new();
        let all = changes(&none, false);
        for name in ["VSCODE_IPC_HOOK_CLI", "CLAUDE_CODE_SSE_PORT"] {
            assert!(all.contains(&Change::Remove(name)), "{name}: {all:?}");
        }
    }

    #[derive(Default)]
    struct Recorded(Vec<String>);

    impl Target for Recorded {
        fn set(&mut self, key: &str, value: &str) {
            self.0.push(format!("set {key}={value}"));
        }
        fn remove(&mut self, key: &str) {
            self.0.push(format!("remove {key}"));
        }
    }

    #[test]
    fn apply_removes_editor_variables_before_the_profile() {
        // The variable is only read here; no other test depends on it.
        std::env::set_var("VSCODE_GIT_HANGAR_TEST_MARKER", "1");
        let profile = env(&[("VSCODE_GIT_HANGAR_TEST_MARKER", "again")]);
        let mut target = Recorded::default();
        apply(&mut target, &profile, false);
        std::env::remove_var("VSCODE_GIT_HANGAR_TEST_MARKER");
        let removed = target
            .0
            .iter()
            .position(|c| c == "remove VSCODE_GIT_HANGAR_TEST_MARKER");
        let set = target
            .0
            .iter()
            .position(|c| c == "set VSCODE_GIT_HANGAR_TEST_MARKER=again");
        assert!(removed.is_some() && removed < set, "{:?}", target.0);
    }

    #[test]
    fn no_cwd_lookup_on_windows_with_opt_out() {
        let none = BTreeMap::new();
        let guarded = changes(&none, true);
        assert_eq!(
            guarded.contains(&Change::Set(NO_CWD_IN_EXE_PATH, "1")),
            cfg!(windows)
        );
        let opted_out = env(&[("nodefaultcurrentdirectoryinexepath", "")]);
        let all = changes(&opted_out, true);
        assert_eq!(
            all.last(),
            Some(&Change::Remove("nodefaultcurrentdirectoryinexepath"))
        );
    }
}
