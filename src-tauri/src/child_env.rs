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
    // A Hangar started inside a gated session must not hand that session's token on. A gated
    // session gets its own token afterwards, from the gate, never from here.
    out.push(Change::Remove(crate::gate::TOKEN_ENV));
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

pub fn apply(
    target: &mut impl Target,
    profile_env: &BTreeMap<String, String>,
    no_cwd_lookup: bool,
) {
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
