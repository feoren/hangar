//! What a working folder's own agent settings do to Hangar's hooks, read before a start.
//!
//! - **Claude Code:** a checked-in `.claude/settings.json` or `.claude/settings.local.json` can
//!   switch off the hooks Hangar passes with `--settings`: `disableAllHooks: true`, an
//!   `allowedHttpHookUrls` list without the endpoint's URL, or an `httpHookAllowedEnvVars` list
//!   without the token's variable (measured with 2.1.331,
//!   docs/wiki/pages/sources/claude-repo-settings-hooks-measurement.md). The gate then only seems
//!   active, and without the notice hooks a link message could press Enter into an open
//!   permission dialog.
//! - **Codex CLI:** Hangar's session hook runs only with `--dangerously-bypass-hook-trust`, which
//!   also runs every project hook unreviewed. A working folder with hooks of its own (`hooks.json`
//!   or a `[hooks]` table in `.codex/config.toml`) therefore gets no bypass and no gate.
//!
//! Read from the working folder up to its git root, plus the root checkout of a linked worktree
//! (Codex reads a linked worktree's project hooks there). More folders than a CLI reads only
//! make a hit more likely, the safe direction. Project files are read through
//! [`crate::unattended::read_settings`]: bounded, links not followed; one that is there but
//! cannot be read that way counts as a hit. The user's and the managed settings are read through
//! their links, still bounded.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::gate::{
    ROUTE_NOTIFY_ASKS, ROUTE_NOTIFY_CLEAR, ROUTE_PRE_TOOL_USE, ROUTE_USER_PROMPT_SUBMIT,
};

/// How many parent folders are searched for a git root.
const MAX_PARENTS: usize = 64;

/// How the files are reached: injected so the rules are tested without a real repository.
pub struct Disk<'a> {
    /// The file's text, `None` when it is missing, unreadable, a link or too big.
    pub read: &'a dyn Fn(&Path) -> Option<String>,
    /// Whether an entry of any kind is there, links included and not followed.
    pub exists: &'a dyn Fn(&Path) -> bool,
    /// The text of a file the user or an administrator keeps (user and managed settings), links
    /// followed: a dotfile manager often links `~/.claude/settings.json`. Still bounded and
    /// regular files only. `None` when it is missing, unreadable or too big.
    pub read_own: &'a dyn Fn(&Path) -> Option<String>,
}

impl Disk<'static> {
    /// The real disk.
    pub fn real() -> Self {
        Disk {
            read: &crate::unattended::read_settings,
            exists: &|path: &Path| std::fs::symlink_metadata(path).is_ok(),
            read_own: &read_followed,
        }
    }
}

/// [`Disk::read_own`] on the real disk: a regular file (after links) of at most
/// [`crate::unattended::MAX_SETTINGS_BYTES`], UTF-8, a leading byte order mark dropped.
fn read_followed(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let max = crate::unattended::MAX_SETTINGS_BYTES;
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).ok()? > max {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some(
        text.strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(text),
    )
}

/// The folders whose project settings count for a start in `cwd`: `cwd` and its parents up to
/// the first one holding `.git` (a folder or a linked worktree's file), and the root checkout a
/// linked worktree's `.git` file leads to. Without a git root, `cwd` alone.
pub fn project_dirs(cwd: &Path, disk: &Disk<'_>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut root = None;
    for dir in cwd.ancestors().take(MAX_PARENTS) {
        dirs.push(dir.to_path_buf());
        if (disk.exists)(&dir.join(".git")) {
            root = Some(dir);
            break;
        }
    }
    let Some(root) = root else {
        return vec![cwd.to_path_buf()];
    };
    if let Some(main) = root_checkout(root, disk) {
        if !dirs.contains(&main) {
            dirs.push(main);
        }
    }
    dirs
}

/// The root checkout of a linked worktree: its `.git` file says `gitdir: <main>/.git/worktrees/
/// <name>`. `None` for an ordinary checkout or any other shape.
fn root_checkout(root: &Path, disk: &Disk<'_>) -> Option<PathBuf> {
    let text = (disk.read)(&root.join(".git"))?;
    let gitdir = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))?
        .trim();
    let gitdir = lexical(&root.join(gitdir));
    let worktrees = gitdir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    let common = worktrees.parent()?;
    if common.file_name()? != ".git" {
        return None;
    }
    common.parent().map(Path::to_path_buf)
}

/// `path` with `.` and `..` taken out by the text alone (git may write a relative `gitdir`).
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The URLs Hangar's Claude Code hooks call on `port` (`gate::claude_settings`,
/// `gate::claude_notify_settings`).
fn hook_urls(port: u16) -> Vec<String> {
    [
        ROUTE_PRE_TOOL_USE,
        ROUTE_USER_PROMPT_SUBMIT,
        ROUTE_NOTIFY_ASKS,
        ROUTE_NOTIFY_CLEAR,
    ]
    .iter()
    .map(|route| format!("http://127.0.0.1:{port}{route}"))
    .collect()
}

/// `text` matches `pattern`, where `*` stands for any run of characters (the wildcard of
/// `allowedHttpHookUrls`).
fn glob_matches(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else {
        return text.is_empty();
    };
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        // No `*`: the whole text must be the pattern.
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// The settings keys that can switch off or limit Hangar's http hooks.
const LIMITING_KEYS: [&str; 4] = [
    "disableAllHooks",
    "allowManagedHooksOnly",
    "allowedHttpHookUrls",
    "httpHookAllowedEnvVars",
];

/// Whether one Claude Code settings file switches off or limits Hangar's http hooks on `port`
/// whose token travels in one of `env_vars`. A value of an unexpected type counts as a hit: what
/// Claude Code makes of it is not known. So does text that is no strict JSON (comments, a
/// trailing comma) but names one of the keys: a more lenient reader might still take it.
pub fn claude_switches_hooks_off(text: &str, port: u16, env_vars: &[&str]) -> bool {
    let settings = match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(settings)) => settings,
        Ok(_) => return false,
        Err(_) => return LIMITING_KEYS.iter().any(|key| text.contains(key)),
    };
    let truthy = |key: &str| {
        settings
            .get(key)
            .is_some_and(|value| value != &Value::Bool(false))
    };
    if truthy("disableAllHooks") || truthy("allowManagedHooksOnly") {
        return true;
    }
    if let Some(allowed) = settings.get("allowedHttpHookUrls") {
        let Some(patterns) = allowed.as_array() else {
            return true;
        };
        let patterns: Vec<&str> = patterns.iter().filter_map(Value::as_str).collect();
        let all_allowed = hook_urls(port)
            .iter()
            .all(|url| patterns.iter().any(|pattern| glob_matches(pattern, url)));
        if !all_allowed {
            return true;
        }
    }
    if let Some(allowed) = settings.get("httpHookAllowedEnvVars") {
        let Some(names) = allowed.as_array() else {
            return true;
        };
        if !env_vars
            .iter()
            .all(|var| names.iter().any(|name| name.as_str() == Some(var)))
        {
            return true;
        }
    }
    false
}

/// Whether a Claude Code start in `cwd` may lose Hangar's hooks to a settings file: the project
/// files of [`project_dirs`] and the `extra` files (the user's and the managed settings). A
/// project file that is there but cannot be read (a link, too big, no UTF-8) counts as a hit:
/// Claude Code follows the link Hangar does not, so what it reads there is not known.
pub fn claude_hooks_off(
    cwd: &Path,
    extra: &[PathBuf],
    port: u16,
    env_vars: &[&str],
    disk: &Disk<'_>,
) -> bool {
    let off = |text: String| claude_switches_hooks_off(&text, port, env_vars);
    let project = project_dirs(cwd, disk).iter().any(|dir| {
        let folder = dir.join(".claude");
        [
            folder.join("settings.json"),
            folder.join("settings.local.json"),
        ]
        .iter()
        .any(|file| match (disk.read)(file) {
            Some(text) => off(text),
            None => (disk.exists)(file),
        })
    });
    project
        || extra
            .iter()
            .filter_map(|file| (disk.read_own)(file))
            .any(off)
}

/// Whether a Codex start in `cwd` would find project hooks: a `.codex/hooks.json` of any kind,
/// or a `.codex/config.toml` with a `hooks` table. A config file that cannot be read (too big,
/// a link) or parsed counts when it may hold one: Codex might still read it.
pub fn codex_project_hooks(cwd: &Path, disk: &Disk<'_>) -> bool {
    project_dirs(cwd, disk).iter().any(|dir| {
        let codex = dir.join(".codex");
        if (disk.exists)(&codex.join("hooks.json")) {
            return true;
        }
        let config = codex.join("config.toml");
        match (disk.read)(&config) {
            Some(text) => match text.parse::<toml::Table>() {
                Ok(table) => table.contains_key("hooks"),
                Err(_) => text.contains("hooks"),
            },
            None => (disk.exists)(&config),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::gate::{NOTIFY_TOKEN_ENV, TOKEN_ENV};

    const PORT: u16 = 4711;
    const VARS: [&str; 2] = [TOKEN_ENV, NOTIFY_TOKEN_ENV];

    /// Files by path; `links` are entries Hangar's bounded reader refuses (a link, too big), which
    /// only the reader for the user's own files follows.
    #[derive(Default)]
    struct Files(BTreeMap<PathBuf, String>, BTreeSet<PathBuf>);

    impl Files {
        fn with(mut self, path: &str, text: &str) -> Self {
            self.0.insert(PathBuf::from(path), text.to_owned());
            self
        }

        fn linked(mut self, path: &str, text: &str) -> Self {
            self.1.insert(PathBuf::from(path));
            self.with(path, text)
        }

        fn read(&self, path: &Path) -> Option<String> {
            if self.1.contains(path) {
                return None;
            }
            self.0.get(path).cloned()
        }

        fn claude_off(&self, cwd: &str) -> bool {
            let read = |path: &Path| self.read(path);
            let exists = |path: &Path| self.0.contains_key(path);
            let read_own = |path: &Path| self.0.get(path).cloned();
            let disk = Disk {
                read: &read,
                exists: &exists,
                read_own: &read_own,
            };
            let extra = [PathBuf::from("/home/me/.claude/settings.json")];
            claude_hooks_off(Path::new(cwd), &extra, PORT, &VARS, &disk)
        }

        fn codex_hooks(&self, cwd: &str) -> bool {
            let read = |path: &Path| self.read(path);
            let exists = |path: &Path| self.0.contains_key(path);
            let read_own = |path: &Path| self.0.get(path).cloned();
            let disk = Disk {
                read: &read,
                exists: &exists,
                read_own: &read_own,
            };
            codex_project_hooks(Path::new(cwd), &disk)
        }
    }

    #[test]
    fn settings_that_switch_the_hooks_off_are_found() {
        let off = |text: &str| claude_switches_hooks_off(text, PORT, &VARS);
        assert!(off(r#"{"disableAllHooks":true}"#));
        assert!(off(r#"{"disableAllHooks":"yes"}"#));
        assert!(!off(r#"{"disableAllHooks":false}"#));
        assert!(off(r#"{"allowManagedHooksOnly":true}"#));
        assert!(off(r#"{"allowedHttpHookUrls":[]}"#));
        assert!(off(
            r#"{"allowedHttpHookUrls":["https://example.invalid/*"]}"#
        ));
        assert!(off(r#"{"allowedHttpHookUrls":"http://127.0.0.1:*"}"#));
        // Only some of Hangar's routes allowed: the notice hooks would be off.
        assert!(off(
            r#"{"allowedHttpHookUrls":["http://127.0.0.1:4711/v1/pre-tool-use"]}"#
        ));
        assert!(!off(r#"{"allowedHttpHookUrls":["http://127.0.0.1:*"]}"#));
        assert!(!off(
            r#"{"allowedHttpHookUrls":["http://127.0.0.1:4711/v1/*"]}"#
        ));
        assert!(off(
            r#"{"allowedHttpHookUrls":["http://127.0.0.1:4712/*"]}"#
        ));
        assert!(off(r#"{"httpHookAllowedEnvVars":[]}"#));
        assert!(off(r#"{"httpHookAllowedEnvVars":["HANGAR_HOOK_TOKEN"]}"#));
        assert!(!off(
            r#"{"httpHookAllowedEnvVars":["HANGAR_HOOK_TOKEN","HANGAR_NOTIFY_TOKEN"]}"#
        ));
        // A private session's notice hooks need their own variable only.
        assert!(!claude_switches_hooks_off(
            r#"{"httpHookAllowedEnvVars":["HANGAR_NOTIFY_TOKEN"]}"#,
            PORT,
            &[NOTIFY_TOKEN_ENV]
        ));
        assert!(!off(r#"{"permissions":{"allow":["Read"]},"hooks":{}}"#));
        assert!(!off("{ not json"));
        assert!(!off("[]"));
        // No strict JSON, but naming a key: a lenient reader might still take it.
        assert!(off("{
  // local
  \"disableAllHooks\": true,
}"));
        assert!(!off("{
  // local
  \"model\": \"x\",
}"));
    }

    /// Claude Code follows a link Hangar's bounded reader refuses, so a project file Hangar cannot
    /// read counts; the user's own file is read through its link (dotfile managers link it).
    #[test]
    fn an_unreadable_project_file_counts_and_the_users_own_file_is_followed() {
        let git = "/work/repo/.git";
        let model = r#"{"model":"x"}"#;
        assert!(Files::default()
            .with(git, "")
            .linked("/work/repo/.claude/settings.json", model)
            .claude_off("/work/repo"));
        assert!(!Files::default()
            .with(git, "")
            .linked("/home/me/.claude/settings.json", model)
            .claude_off("/work/repo"));
        assert!(Files::default()
            .with(git, "")
            .linked(
                "/home/me/.claude/settings.json",
                r#"{"disableAllHooks":true}"#
            )
            .claude_off("/work/repo"));
    }

    #[test]
    fn the_users_own_file_is_read_bounded_and_only_as_a_regular_file() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(".tmp")
            .join("repo-hooks-own");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(&file, "\u{feff}{\"model\":\"x\"}").unwrap();
        assert_eq!(read_followed(&file).as_deref(), Some(r#"{"model":"x"}"#));
        assert_eq!(read_followed(&dir), None);
        assert_eq!(read_followed(&dir.join("missing.json")), None);
        let max = usize::try_from(crate::unattended::MAX_SETTINGS_BYTES).unwrap();
        std::fs::write(&file, vec![b' '; max + 1]).unwrap();
        assert_eq!(read_followed(&file), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_wildcard_matches_any_run_of_characters() {
        assert!(glob_matches("a*c", "abbbc"));
        assert!(glob_matches("*", ""));
        assert!(glob_matches("a*b*c", "a-b-c"));
        assert!(!glob_matches("a*b*c", "a-c"));
        assert!(glob_matches("abc", "abc"));
        assert!(!glob_matches("abc", "abcd"));
        assert!(!glob_matches("a*c", "ab"));
    }

    #[test]
    fn claude_files_are_read_from_the_folder_up_to_the_git_root() {
        let off = r#"{"disableAllHooks":true}"#;
        for file in [
            "/work/repo/app/.claude/settings.json",
            "/work/repo/app/.claude/settings.local.json",
            "/work/repo/.claude/settings.json",
            "/home/me/.claude/settings.json",
        ] {
            let files = Files::default().with("/work/repo/.git", "").with(file, off);
            assert!(files.claude_off("/work/repo/app"), "{file}");
        }
        // Above the git root nothing counts.
        let above = Files::default()
            .with("/work/repo/.git", "")
            .with("/work/.claude/settings.json", off);
        assert!(!above.claude_off("/work/repo/app"));
        // Without a git root only the working folder.
        let parent = Files::default().with("/work/repo/.claude/settings.json", off);
        assert!(!parent.claude_off("/work/repo/app"));
        assert!(!Files::default()
            .with("/work/repo/.git", "")
            .with("/work/repo/.claude/settings.json", r#"{"model":"x"}"#)
            .claude_off("/work/repo"));
    }

    #[test]
    fn codex_project_hooks_are_found_in_the_worktree_and_its_root_checkout() {
        let git = "/work/repo/.git";
        assert!(Files::default()
            .with(git, "")
            .with("/work/repo/.codex/hooks.json", "{}")
            .codex_hooks("/work/repo/app"));
        assert!(Files::default()
            .with(git, "")
            .with(
                "/work/repo/app/.codex/config.toml",
                "[[hooks.PreToolUse]]\nmatcher = \"x\"\n"
            )
            .codex_hooks("/work/repo/app"));
        assert!(!Files::default()
            .with(git, "")
            .with(
                "/work/repo/.codex/config.toml",
                "approval_policy = \"on-request\"\n"
            )
            .codex_hooks("/work/repo/app"));
        // Unparsable but naming hooks counts; unparsable without them does not.
        assert!(Files::default()
            .with(git, "")
            .with("/work/repo/.codex/config.toml", "hooks = [")
            .codex_hooks("/work/repo"));
        assert!(!Files::default()
            .with(git, "")
            .with("/work/repo/.codex/config.toml", "model = [")
            .codex_hooks("/work/repo"));
        assert!(!Files::default().with(git, "").codex_hooks("/work/repo"));

        // A linked worktree: its `.git` file leads to the root checkout, whose hooks Codex reads.
        let linked = Files::default()
            .with(
                "/work/wt/feature/.git",
                "gitdir: /work/repo/.git/worktrees/feature\n",
            )
            .with("/work/repo/.codex/hooks.json", "{}");
        assert!(linked.codex_hooks("/work/wt/feature"));
        let other_shape = Files::default()
            .with("/work/wt/feature/.git", "gitdir: /elsewhere/modules/x\n")
            .with("/elsewhere/.codex/hooks.json", "{}");
        assert!(!other_shape.codex_hooks("/work/wt/feature"));
    }

    #[test]
    fn a_relative_gitdir_is_taken_from_the_worktree() {
        let files = Files::default()
            .with(
                "/work/repo/.claude/wt/a/.git",
                "gitdir: ../../../.git/worktrees/a",
            )
            .with("/work/repo/.codex/hooks.json", "{}");
        let read = |path: &Path| files.read(path);
        let exists = |path: &Path| files.0.contains_key(path);
        let read_own = |path: &Path| files.0.get(path).cloned();
        let disk = Disk {
            read: &read,
            exists: &exists,
            read_own: &read_own,
        };
        let cwd = Path::new("/work/repo/.claude/wt/a");
        assert_eq!(
            project_dirs(cwd, &disk).last(),
            Some(&PathBuf::from("/work/repo"))
        );
        assert!(codex_project_hooks(cwd, &disk));
    }
}
