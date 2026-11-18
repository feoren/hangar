//! The environment of every program Hangar starts: sessions in a PTY (`manager.rs`) and the
//! version query (`version.rs`) get [`apply`]; helper programs (git in `git.rs`, the file manager
//! and opened files in `desktop.rs`) get [`scrub`], the same removals without the terminal and
//! profile settings. One rule for all, so a protection added for one cannot be missing in the
//! other. Programs Windows starts for Hangar through `ShellExecuteW` (`desktop.rs`) inherit
//! Hangar's own environment with no way to change it per call, so [`scrub_own_environment`]
//! takes the same removals out of Hangar's own process once at startup. Started from an AppImage
//! on Linux, both also undo what the image changed for Hangar itself ([`appimage_fixes`]); Hangar's
//! own process keeps those, its bundled GTK needs them.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

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
    changes_with(profile_env, no_cwd_lookup, inside_gate())
}

/// Whether Hangar started inside a gated session (it inherited a gate token), as its environment
/// said on the first call. [`scrub_own_environment`] asks first, before it removes the token, so
/// the answer stays what Hangar inherited.
fn inside_gate() -> bool {
    static INSIDE_GATE: OnceLock<bool> = OnceLock::new();
    *INSIDE_GATE.get_or_init(|| {
        [crate::gate::TOKEN_ENV, crate::hook_recipes::HELPER_ENV]
            .iter()
            .any(|name| std::env::var_os(name).is_some())
    })
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
    // Likewise the calling session's send token: a session gets its own from the send client.
    out.push(Change::Remove(crate::partners::SEND_ENV));
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

/// What the AppImage runtime sets for the program inside it: the image file, its mount point,
/// the name it was started by and the working folder at the start.
pub const APPIMAGE_RUNTIME: [&str; 4] = ["APPIMAGE", "APPDIR", "ARGV0", "OWD"];

/// What one variable of a child becomes: removed (`None`) or set to the value.
pub type Fix = (String, Option<String>);

/// The changes a child of Hangar running as an AppImage needs (ADR 0013). Inside the image,
/// AppRun and linuxdeploy's GTK hook point library, GTK, GIO and data search paths into the
/// image's mount point (`GTK_PATH`, `GDK_PIXBUF_MODULE_FILE`, `GSETTINGS_SCHEMA_DIR`,
/// `XDG_DATA_DIRS` and the like) and force `GDK_BACKEND=x11`. A shell, an agent or the file
/// manager would load Hangar's bundled modules, or none once the image is unmounted. So:
///
/// - the runtime's own variables ([`APPIMAGE_RUNTIME`]) go;
/// - in every other variable whose value names the mount point, the `:`-separated entries inside
///   it go; a variable with nothing left goes;
/// - `GDK_BACKEND=x11` and an empty `GTK_THEME` go: the hook sets both unconditionally, so inside
///   the image they never are the user's own.
///
/// Nothing changes unless both `APPIMAGE` and an absolute `APPDIR` other than `/` are set and
/// Hangar's own executable `exe` lies inside `APPDIR`: only then did this image set them. An
/// `APPDIR=/usr` inherited from some other program's environment must not strip `/usr/bin` from
/// every child's `PATH`.
pub fn appimage_fixes<'a>(
    inherited: impl IntoIterator<Item = (&'a str, &'a str)>,
    exe: Option<&Path>,
) -> Vec<Fix> {
    let inherited: Vec<(&str, &str)> = inherited.into_iter().collect();
    let value_of = |name: &str| {
        inherited
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    };
    let Some(appdir) = value_of("APPDIR").map(|dir| dir.trim_end_matches('/')) else {
        return Vec::new();
    };
    // `/` trims to "" and fails here too: every path would count as inside the image.
    if value_of("APPIMAGE").is_none() || !appdir.starts_with('/') {
        return Vec::new();
    }
    if !exe.is_some_and(|exe| exe.starts_with(appdir)) {
        return Vec::new();
    }
    let inside = |entry: &str| {
        entry
            .strip_prefix(appdir)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    let mut fixes = Vec::new();
    for (name, value) in &inherited {
        if APPIMAGE_RUNTIME.contains(name)
            || (*name == "GDK_BACKEND" && *value == "x11")
            || (*name == "GTK_THEME" && value.is_empty())
        {
            fixes.push(((*name).to_owned(), None));
        } else if value.contains(appdir) {
            let kept: Vec<&str> = value
                .split(':')
                .filter(|entry| !entry.is_empty() && !inside(entry))
                .collect();
            let fixed = (!kept.is_empty()).then(|| kept.join(":"));
            if fixed.as_deref() != Some(*value) {
                fixes.push(((*name).to_owned(), fixed));
            }
        }
    }
    fixes
}

/// [`appimage_fixes`] for Hangar's own environment; only Linux has AppImages.
fn own_appimage_fixes(inherited: &[(String, String)]) -> Vec<Fix> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let exe = std::env::current_exe().ok();
    appimage_fixes(
        inherited.iter().map(|(n, v)| (n.as_str(), v.as_str())),
        exe.as_deref(),
    )
}

fn apply_fixes(target: &mut impl Target, fixes: Vec<Fix>) {
    for (name, value) in fixes {
        match value {
            Some(value) => target.set(&name, &value),
            None => target.remove(&name),
        }
    }
}

/// Hangar's own environment as `(name, value)` pairs; names and values that are not UTF-8 are no
/// editor's variables.
fn inherited() -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

pub fn apply(
    target: &mut impl Target,
    profile_env: &BTreeMap<String, String>,
    no_cwd_lookup: bool,
) {
    let inherited = inherited();
    // First, so a profile can still set one of them on purpose.
    for name in ide_variables(inherited.iter().map(|(n, v)| (n.as_str(), v.as_str()))) {
        target.remove(&name);
    }
    apply_fixes(target, own_appimage_fixes(&inherited));
    for change in changes(profile_env, no_cwd_lookup) {
        match change {
            Change::Set(key, value) => target.set(key, value),
            Change::Remove(key) => target.remove(key),
        }
    }
}

/// The names a helper program must not inherit: the calling agent session's markers, the gate's
/// token and helper variables (every gate variable when Hangar itself runs in a gated session)
/// and an editor's git and IPC variables among `inherited`. A repository's hooks and helpers, or
/// a program a clicked file opens in, would otherwise reach that session's gate or editor.
pub fn removals<'a>(
    inherited: impl IntoIterator<Item = (&'a str, &'a str)>,
    inside_gate: bool,
) -> Vec<String> {
    let mut names = ide_variables(inherited);
    names.extend(
        changes_with(&BTreeMap::new(), false, inside_gate)
            .into_iter()
            .filter_map(|change| match change {
                Change::Remove(name) => Some(name.to_owned()),
                Change::Set(..) => None,
            }),
    );
    names
}

/// Removes [`removals`] from a helper program's environment and undoes an AppImage's changes
/// ([`appimage_fixes`]). Sets nothing else: helpers need neither a terminal type nor a
/// profile's variables.
pub fn scrub(target: &mut impl Target) {
    let inherited = inherited();
    for name in removals(
        inherited.iter().map(|(n, v)| (n.as_str(), v.as_str())),
        inside_gate(),
    ) {
        target.remove(&name);
    }
    apply_fixes(target, own_appimage_fixes(&inherited));
}

/// Removes [`removals`] from Hangar's own process environment. Called once at the start of
/// [`crate::run`], while the process has a single thread: changing the environment while another
/// thread reads it is undefined behavior on Unix (`setenv`/`getenv` are not thread safe), which
/// is why edition 2024 marks `remove_var` unsafe. Afterwards a program Windows' shell starts
/// for Hangar (an opened file, page or folder) inherits nothing of a calling agent session, its
/// gate or an editor, like every program Hangar starts itself.
pub fn scrub_own_environment() {
    let inside_gate = inside_gate();
    let inherited = inherited();
    for name in removals(
        inherited.iter().map(|(n, v)| (n.as_str(), v.as_str())),
        inside_gate,
    ) {
        std::env::remove_var(name);
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
            assert!(
                all.contains(&Change::Remove(crate::partners::SEND_ENV)),
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

    /// Held by every test here that changes or reads the process environment: the startup scrub
    /// removes every `VSCODE_GIT_` variable, the markers of the other tests included.
    static PROCESS_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn startup_takes_the_removals_out_of_hangars_own_environment() {
        let _env = crate::lock(&PROCESS_ENV);
        // Names only this test uses; an unrelated variable stays.
        std::env::set_var("VSCODE_GIT_HANGAR_STARTUP_MARKER", "1");
        std::env::set_var("HANGAR_STARTUP_KEEP_MARKER", "1");
        let gated = inside_gate();
        scrub_own_environment();
        assert!(std::env::var_os("VSCODE_GIT_HANGAR_STARTUP_MARKER").is_none());
        assert_eq!(
            std::env::var_os("HANGAR_STARTUP_KEEP_MARKER").as_deref(),
            Some(std::ffi::OsStr::new("1"))
        );
        std::env::remove_var("HANGAR_STARTUP_KEEP_MARKER");
        for name in crate::manager::INHERITED_SESSION_MARKERS
            .iter()
            .chain([&crate::gate::TOKEN_ENV, &crate::hook_recipes::HELPER_ENV])
        {
            assert!(std::env::var_os(name).is_none(), "{name} stayed");
        }
        // What Hangar inherited stays remembered after its token is gone.
        assert_eq!(inside_gate(), gated);
    }

    #[test]
    fn apply_removes_editor_variables_before_the_profile() {
        // The variable is only read here; the startup scrub test waits for the lock.
        let _env = crate::lock(&PROCESS_ENV);
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
    fn helpers_lose_markers_gate_and_editor_variables() {
        let inherited = [
            ("VSCODE_GIT_IPC_HANDLE", "marker"),
            ("PATH", "/usr/bin"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ];
        let outside = removals(inherited, false);
        for name in [
            "VSCODE_GIT_IPC_HANDLE",
            "CLAUDECODE",
            "CLAUDE_CODE_SESSION_ID",
            "VSCODE_IPC_HOOK_CLI",
            crate::gate::TOKEN_ENV,
            crate::hook_recipes::HELPER_ENV,
        ] {
            assert!(outside.iter().any(|n| n == name), "{name}: {outside:?}");
        }
        // The user's own settings stay; nothing is set.
        for kept in ["PATH", "CLAUDE_CODE_USE_BEDROCK", "TERM"] {
            assert!(!outside.iter().any(|n| n == kept), "{kept}");
        }
        let inside = removals(inherited, true);
        for name in crate::hook_recipes::GATE_ENV_NAMES {
            assert!(inside.iter().any(|n| n == name), "{name}: {inside:?}");
        }
    }

    #[test]
    fn scrub_only_removes() {
        let mut target = Recorded::default();
        scrub(&mut target);
        assert!(
            target.0.iter().all(|c| c.starts_with("remove ")),
            "{:?}",
            target.0
        );
        assert!(target
            .0
            .contains(&format!("remove {}", crate::gate::TOKEN_ENV)));
    }

    /// The environment AppRun and linuxdeploy's GTK hook leave behind for Hangar, plus the
    /// user's own variables.
    const APPIMAGE_ENV: [(&str, &str); 13] = [
        ("APPIMAGE", "/home/x/Apps/Hangar.AppImage"),
        ("APPDIR", "/tmp/.mount_HangarAb12"),
        ("ARGV0", "./Hangar.AppImage"),
        ("OWD", "/home/x"),
        ("GDK_BACKEND", "x11"),
        ("GTK_THEME", ""),
        (
            "GDK_PIXBUF_MODULE_FILE",
            "/tmp/.mount_HangarAb12/usr/lib/gdk-pixbuf-2.0/loaders.cache",
        ),
        (
            "XDG_DATA_DIRS",
            "/tmp/.mount_HangarAb12/usr/share:/usr/share:/usr/local/share",
        ),
        (
            "PATH",
            "/tmp/.mount_HangarAb12/usr/bin:/home/x/.local/bin:/usr/bin",
        ),
        ("LD_LIBRARY_PATH", "/tmp/.mount_HangarAb12/usr/lib"),
        // A folder whose name merely starts like the mount point is not inside it.
        ("MY_TOOLS", "/tmp/.mount_HangarAb12-other/bin"),
        ("HOME", "/home/x"),
        ("SHELL", "/bin/zsh"),
    ];

    /// Hangar's executable inside the mounted image of [`APPIMAGE_ENV`].
    fn image_exe() -> Option<&'static Path> {
        Some(Path::new("/tmp/.mount_HangarAb12/usr/bin/hangar"))
    }

    fn fix_of<'a>(fixes: &'a [Fix], name: &str) -> Option<&'a Option<String>> {
        fixes.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    #[test]
    fn appimage_variables_do_not_reach_children() {
        let fixes = appimage_fixes(APPIMAGE_ENV, image_exe());
        for name in APPIMAGE_RUNTIME
            .iter()
            .chain(&["GDK_BACKEND", "GTK_THEME", "GDK_PIXBUF_MODULE_FILE"])
            .chain(&["LD_LIBRARY_PATH"])
        {
            assert_eq!(fix_of(&fixes, name), Some(&None), "{name}: {fixes:?}");
        }
        assert_eq!(
            fix_of(&fixes, "PATH"),
            Some(&Some("/home/x/.local/bin:/usr/bin".to_owned()))
        );
        assert_eq!(
            fix_of(&fixes, "XDG_DATA_DIRS"),
            Some(&Some("/usr/share:/usr/local/share".to_owned()))
        );
        for kept in ["MY_TOOLS", "HOME", "SHELL"] {
            assert_eq!(fix_of(&fixes, kept), None, "{kept}: {fixes:?}");
        }
    }

    #[test]
    fn outside_an_appimage_nothing_changes() {
        // Without APPIMAGE, a user's own APPDIR or GDK_BACKEND stays.
        let without_image: Vec<_> = APPIMAGE_ENV
            .iter()
            .copied()
            .filter(|(name, _)| *name != "APPIMAGE")
            .collect();
        assert!(appimage_fixes(without_image, image_exe()).is_empty());
        // A mount point of `/` would put every path inside the image.
        let anywhere = Some(Path::new("/usr/bin/hangar"));
        assert!(
            appimage_fixes([("APPIMAGE", "/x.AppImage"), ("APPDIR", "/")], anywhere).is_empty()
        );
        assert!(
            appimage_fixes([("APPIMAGE", "/x.AppImage"), ("APPDIR", "rel")], anywhere).is_empty()
        );
    }

    #[test]
    fn variables_of_an_image_hangar_does_not_run_from_stay() {
        // Hangar installed from the .deb, started by a program that runs as an AppImage itself:
        // its APPDIR is not Hangar's, and with APPDIR=/usr the fixes would strip /usr/bin.
        let deb = Some(Path::new("/usr/bin/hangar"));
        assert!(appimage_fixes(APPIMAGE_ENV, deb).is_empty());
        let foreign = [
            ("APPIMAGE", "/home/x/Other.AppImage"),
            ("APPDIR", "/usr"),
            ("PATH", "/usr/local/bin:/usr/bin"),
        ];
        assert!(appimage_fixes(foreign, Some(Path::new("/opt/hangar/hangar"))).is_empty());
        assert!(appimage_fixes(APPIMAGE_ENV, None).is_empty());
        // A folder that merely starts like the mount point does not count either.
        let beside = Some(Path::new("/tmp/.mount_HangarAb12-other/usr/bin/hangar"));
        assert!(appimage_fixes(APPIMAGE_ENV, beside).is_empty());
    }

    #[test]
    fn appimage_fixes_are_written_to_the_child() {
        let mut target = Recorded::default();
        apply_fixes(&mut target, appimage_fixes(APPIMAGE_ENV, image_exe()));
        assert!(target.0.contains(&"remove APPDIR".to_owned()));
        assert!(target
            .0
            .contains(&"set PATH=/home/x/.local/bin:/usr/bin".to_owned()));
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
