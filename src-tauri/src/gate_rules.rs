//! Gate rules of the agent gate (`gate.rs`): when the gate would ask the person about an
//! edit that collides with another session or a teammate, a rule the person set earlier answers
//! instead. A rule belongs to one project (the repository's git common folder, so every worktree
//! of it shares the rule) and names a file or a folder relative to the worktree's top. It never
//! reaches beyond its project: paths are relative, without `..`, and matched below the folder
//! only. A deny rule wins over an allow rule.
//!
//! Pure: parsing, matching and the rule book. The service (`gate_service.rs`) loads and saves the
//! file and applies the rules before it asks the window.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::SessionId;

/// File in Hangar's local data folder.
pub const FILE_NAME: &str = "gate-rules.json";
/// More rules than this are not kept; a file with more keeps the first ones.
pub const MAX_RULES: usize = 500;
/// Automatic answers kept for the window's log, newest first.
pub const LOG_LEN: usize = 50;
/// Larger rule files are ignored as a whole. Room for [`MAX_RULES`] rules at the longest project
/// and path (about 3 KiB each, pretty printed), so a full book always reads back.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PATH_LEN: usize = 1024;
const FILE_VERSION: u64 = 1;

/// What a rule answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleEffect {
    Allow,
    Deny,
}

/// What a rule covers: one file, or every file below a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleScope {
    File,
    Folder,
}

/// One gate rule (Settings lists and deletes them; command `gate_rules`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateRule {
    pub id: u64,
    /// The repository's key ([`crate::claims::WorktreeClaims::repo`], the git common folder).
    pub project: String,
    /// The project's folder name, for display.
    pub project_name: String,
    /// Relative to the worktree's top, `/` separators, as git prints it.
    pub path: String,
    pub scope: RuleScope,
    pub effect: RuleEffect,
    /// Unix time in milliseconds.
    pub created_at: u64,
}

/// One edit a rule answered (the gate snapshot's `log`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleAnswer {
    /// Unique per app run, increasing.
    pub id: u64,
    /// Unix time in milliseconds.
    pub at: u64,
    pub session_id: SessionId,
    /// The session's title when the rule answered.
    pub session: String,
    pub project_name: String,
    /// The edited file, relative to the worktree's top.
    pub path: String,
    pub rule_id: u64,
    pub rule_path: String,
    pub scope: RuleScope,
    pub effect: RuleEffect,
}

/// A rule path from the window or the file: `/` separators, no leading `./`, no trailing `/`.
/// `None` for an empty, absolute or escaping path (`..`), a drive or device prefix, control
/// characters or an overlong one: a rule never names anything outside its project.
pub fn normalize_path(path: &str) -> Option<String> {
    if path.len() > MAX_PATH_LEN || path.chars().any(char::is_control) {
        return None;
    }
    let text = path.replace('\\', "/");
    if text.starts_with('/') || text.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// The folder of a file path (`src/auth/a.ts` → `src/auth`); `None` for a file at the top, whose
/// folder would be the whole project.
pub fn folder_of(path: &str) -> Option<String> {
    let path = normalize_path(path)?;
    path.rsplit_once('/').map(|(folder, _)| folder.to_owned())
}

/// The project's display name from its key: the folder holding `.git`, else the last part.
pub fn project_name(project: &str) -> String {
    let trimmed = project.trim_end_matches('/');
    let mut parts = trimmed.rsplit('/');
    let last = parts.next().unwrap_or_default();
    if last.eq_ignore_ascii_case(".git") {
        if let Some(parent) = parts.next().filter(|p| !p.is_empty()) {
            return parent.to_owned();
        }
    }
    last.to_owned()
}

fn fold(text: &str, case_insensitive: bool) -> String {
    if case_insensitive {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

/// Whether paths compare without case on this platform (as [`crate::claims::file_key`]).
pub const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

/// Does `rule` cover the file `key` (a [`crate::claims::file_key`]) in `project`?
pub fn rule_matches(rule: &GateRule, project: &str, key: &str, case_insensitive: bool) -> bool {
    if fold(&rule.project, case_insensitive) != fold(project, case_insensitive) {
        return false;
    }
    let Some(file) = normalize_path(key) else {
        return false;
    };
    let file = fold(&file, case_insensitive);
    let path = fold(&rule.path, case_insensitive);
    match rule.scope {
        RuleScope::File => file == path,
        RuleScope::Folder => file
            .strip_prefix(path.as_str())
            .is_some_and(|rest| rest.starts_with('/')),
    }
}

/// The rule that answers an edit of `key` in `project`: any deny rule before any allow rule, and
/// within one effect the most specific (a file before a folder, a deeper folder first).
pub fn find<'a>(
    rules: &'a [GateRule],
    project: &str,
    key: &str,
    case_insensitive: bool,
) -> Option<&'a GateRule> {
    let rank = |rule: &GateRule| {
        (
            rule.effect == RuleEffect::Deny,
            rule.scope == RuleScope::File,
            rule.path.len(),
        )
    };
    rules
        .iter()
        .filter(|rule| rule_matches(rule, project, key, case_insensitive))
        .max_by(|a, b| rank(a).cmp(&rank(b)))
}

/// The rules of every project, as kept in [`FILE_NAME`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleBook {
    rules: Vec<GateRule>,
}

impl RuleBook {
    /// Reads the file tolerantly: entries that do not parse or name an unsafe path are skipped,
    /// unknown fields ignored, a broken file is an empty book. Ids repeat never: a repeated id
    /// drops the later entry.
    pub fn parse(bytes: &[u8]) -> Self {
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            return Self::default();
        };
        let Some(entries) = value.get("rules").and_then(Value::as_array) else {
            return Self::default();
        };
        let mut rules: Vec<GateRule> = Vec::new();
        for entry in entries {
            if rules.len() == MAX_RULES {
                break;
            }
            let Some(rule) = parse_rule(entry) else {
                continue;
            };
            if rules.iter().any(|r| r.id == rule.id) {
                continue;
            }
            rules.push(rule);
        }
        Self { rules }
    }

    pub fn to_json(&self) -> Vec<u8> {
        let value = serde_json::json!({ "version": FILE_VERSION, "rules": self.rules });
        serde_json::to_vec_pretty(&value).unwrap_or_default()
    }

    pub fn rules(&self) -> &[GateRule] {
        &self.rules
    }

    /// Adds a rule, or changes the effect of the rule for the same project, path and scope.
    /// `None` when the path is unsafe, the project empty or the book full.
    pub fn add(
        &mut self,
        project: &str,
        path: &str,
        scope: RuleScope,
        effect: RuleEffect,
        now_millis: u64,
    ) -> Option<GateRule> {
        let path = normalize_path(path)?;
        // As `parse_rule` checks it: a rule that would not read back is not added.
        if project.is_empty()
            || project.len() > MAX_PATH_LEN
            || project.chars().any(char::is_control)
        {
            return None;
        }
        let same = |rule: &GateRule| {
            rule.scope == scope
                && fold(&rule.project, CASE_INSENSITIVE) == fold(project, CASE_INSENSITIVE)
                && fold(&rule.path, CASE_INSENSITIVE) == fold(&path, CASE_INSENSITIVE)
        };
        if let Some(rule) = self.rules.iter_mut().find(|rule| same(rule)) {
            rule.effect = effect;
            rule.created_at = now_millis;
            return Some(rule.clone());
        }
        if self.rules.len() >= MAX_RULES {
            return None;
        }
        let id = self.rules.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        let rule = GateRule {
            id,
            project: project.to_owned(),
            project_name: project_name(project),
            path,
            scope,
            effect,
            created_at: now_millis,
        };
        self.rules.push(rule.clone());
        Some(rule)
    }

    /// `true` when the rule existed.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.rules.len();
        self.rules.retain(|rule| rule.id != id);
        self.rules.len() != before
    }

    /// [`find`] with this platform's case rule.
    pub fn find(&self, project: &str, key: &str) -> Option<&GateRule> {
        find(&self.rules, project, key, CASE_INSENSITIVE)
    }
}

fn parse_rule(entry: &Value) -> Option<GateRule> {
    let id = entry.get("id")?.as_u64().filter(|id| *id > 0)?;
    let project = entry.get("project")?.as_str()?.to_owned();
    if project.is_empty() || project.len() > MAX_PATH_LEN || project.chars().any(char::is_control) {
        return None;
    }
    let path = normalize_path(entry.get("path")?.as_str()?)?;
    let scope = serde_json::from_value(entry.get("scope")?.clone()).ok()?;
    let effect = serde_json::from_value(entry.get("effect")?.clone()).ok()?;
    let created_at = entry.get("createdAt").and_then(Value::as_u64).unwrap_or(0);
    Some(GateRule {
        id,
        project_name: project_name(&project),
        project,
        path,
        scope,
        effect,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: u64, path: &str, scope: RuleScope, effect: RuleEffect) -> GateRule {
        GateRule {
            id,
            project: "c:/code/app/.git".into(),
            project_name: "app".into(),
            path: path.into(),
            scope,
            effect,
            created_at: 0,
        }
    }

    const PROJECT: &str = "c:/code/app/.git";

    #[test]
    fn a_file_rule_covers_exactly_that_file() {
        let r = rule(1, "src/a.ts", RuleScope::File, RuleEffect::Allow);
        assert!(rule_matches(&r, PROJECT, "src/a.ts", false));
        assert!(!rule_matches(&r, PROJECT, "src/a.tsx", false));
        assert!(!rule_matches(&r, PROJECT, "src/a.ts/b", false));
        assert!(!rule_matches(&r, PROJECT, "a.ts", false));
    }

    #[test]
    fn a_folder_rule_covers_files_below_it_only() {
        let r = rule(1, "src/auth", RuleScope::Folder, RuleEffect::Allow);
        assert!(rule_matches(&r, PROJECT, "src/auth/login.ts", false));
        assert!(rule_matches(&r, PROJECT, "src/auth/deep/x.ts", false));
        // A sibling with the same start is not inside the folder.
        assert!(!rule_matches(&r, PROJECT, "src/authority.ts", false));
        assert!(!rule_matches(&r, PROJECT, "src/auth", false));
        assert!(!rule_matches(&r, PROJECT, "src/other/a.ts", false));
    }

    #[test]
    fn rules_never_reach_another_project() {
        let r = rule(1, "src", RuleScope::Folder, RuleEffect::Allow);
        assert!(!rule_matches(&r, "c:/code/other/.git", "src/a.ts", false));
        assert!(!rule_matches(&r, "c:/code/app2/.git", "src/a.ts", false));
        // An escaping key never matches.
        assert!(!rule_matches(&r, PROJECT, "src/../../other/a.ts", false));
    }

    #[test]
    fn case_counts_only_where_the_file_system_ignores_it() {
        let r = rule(1, "Src/Auth", RuleScope::Folder, RuleEffect::Deny);
        let mut upper = r.clone();
        upper.project = "C:/Code/App/.git".into();
        // Windows and macOS: one file, whatever the case.
        assert!(rule_matches(&upper, PROJECT, "src/auth/login.ts", true));
        assert!(rule_matches(&r, PROJECT, "SRC/AUTH/x.ts", true));
        // Linux: different files.
        assert!(!rule_matches(&r, PROJECT, "src/auth/login.ts", false));
        assert!(!rule_matches(&upper, PROJECT, "Src/Auth/login.ts", false));
    }

    #[test]
    fn deny_wins_over_allow_and_the_most_specific_rule_is_named() {
        let rules = vec![
            rule(1, "src", RuleScope::Folder, RuleEffect::Allow),
            rule(2, "src/auth/login.ts", RuleScope::File, RuleEffect::Allow),
            rule(3, "src/auth", RuleScope::Folder, RuleEffect::Deny),
        ];
        let found = find(&rules, PROJECT, "src/auth/login.ts", false).unwrap();
        assert_eq!(found.id, 3, "a deny folder rule beats an allow file rule");
        let found = find(&rules, PROJECT, "src/main.ts", false).unwrap();
        assert_eq!(found.id, 1);
        let narrower = vec![
            rule(1, "src", RuleScope::Folder, RuleEffect::Allow),
            rule(2, "src/ui", RuleScope::Folder, RuleEffect::Allow),
            rule(3, "src/ui/a.ts", RuleScope::File, RuleEffect::Allow),
        ];
        assert_eq!(
            find(&narrower, PROJECT, "src/ui/a.ts", false).unwrap().id,
            3
        );
        assert_eq!(
            find(&narrower, PROJECT, "src/ui/b.ts", false).unwrap().id,
            2
        );
        assert!(find(&narrower, PROJECT, "docs/x.md", false).is_none());
    }

    #[test]
    fn paths_stay_inside_the_project() {
        assert_eq!(
            normalize_path(r"src\auth\a.ts").as_deref(),
            Some("src/auth/a.ts")
        );
        assert_eq!(normalize_path("./src//a.ts/").as_deref(), Some("src/a.ts"));
        for bad in [
            "",
            "/",
            ".",
            "/etc/passwd",
            r"C:\x\a.ts",
            "c:a.ts",
            r"\\server\share\a",
            "../a.ts",
            "src/../../a.ts",
            "a\u{0}b",
            "a\nb",
        ] {
            assert_eq!(normalize_path(bad), None, "{bad:?}");
        }
        assert_eq!(normalize_path(&"a/".repeat(600)), None);
        assert_eq!(folder_of("src/auth/a.ts").as_deref(), Some("src/auth"));
        assert_eq!(folder_of("README.md"), None);
    }

    #[test]
    fn project_names_come_from_the_folder_holding_git() {
        assert_eq!(project_name("c:/code/hangar/.git"), "hangar");
        assert_eq!(project_name("/srv/repos/tool.git"), "tool.git");
        assert_eq!(project_name("/home/me/app/.git/"), "app");
    }

    #[test]
    fn the_book_adds_replaces_and_removes() {
        let mut book = RuleBook::default();
        let first = book
            .add(PROJECT, "src/a.ts", RuleScope::File, RuleEffect::Allow, 10)
            .unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(first.project_name, "app");
        // The same file again changes the effect instead of adding a second rule.
        let again = book
            .add(PROJECT, r"src\a.ts", RuleScope::File, RuleEffect::Deny, 20)
            .unwrap();
        assert_eq!(again.id, 1);
        assert_eq!(book.rules().len(), 1);
        assert_eq!(book.rules()[0].effect, RuleEffect::Deny);
        // The folder is a rule of its own.
        let folder = book
            .add(PROJECT, "src", RuleScope::Folder, RuleEffect::Allow, 30)
            .unwrap();
        assert_eq!(folder.id, 2);
        assert!(book
            .add(PROJECT, "../x", RuleScope::File, RuleEffect::Allow, 0)
            .is_none());
        assert!(book
            .add("", "x", RuleScope::File, RuleEffect::Allow, 0)
            .is_none());
        assert!(book
            .add(
                &"p".repeat(1025),
                "x",
                RuleScope::File,
                RuleEffect::Allow,
                0
            )
            .is_none());
        assert_eq!(book.find(PROJECT, "src/a.ts").unwrap().id, 1);
        assert!(book.remove(1));
        assert!(!book.remove(1));
        assert_eq!(book.find(PROJECT, "src/a.ts").unwrap().id, 2);
    }

    #[test]
    fn a_full_book_at_the_longest_paths_still_reads_back() {
        let mut book = RuleBook::default();
        let project = "p".repeat(MAX_PATH_LEN);
        for n in 0..MAX_RULES {
            let path = format!("{n:04}/{}", "x".repeat(MAX_PATH_LEN - 5));
            assert!(book
                .add(&project, &path, RuleScope::File, RuleEffect::Deny, u64::MAX)
                .is_some());
        }
        let bytes = book.to_json();
        assert!(bytes.len() <= MAX_FILE_BYTES, "{}", bytes.len());
        assert_eq!(RuleBook::parse(&bytes).rules().len(), MAX_RULES);
    }

    #[test]
    fn the_file_round_trips_and_bad_entries_are_skipped() {
        let mut book = RuleBook::default();
        book.add(PROJECT, "src/a.ts", RuleScope::File, RuleEffect::Allow, 5);
        book.add(PROJECT, "docs", RuleScope::Folder, RuleEffect::Deny, 6);
        let parsed = RuleBook::parse(&book.to_json());
        assert_eq!(parsed, book);

        let text = r#"{"version": 9, "future": true, "rules": [
            {"id": 1, "project": "c:/code/app/.git", "path": "src/a.ts", "scope": "file", "effect": "allow", "createdAt": 1, "note": "kept"},
            {"id": 2, "project": "c:/code/app/.git", "path": "../escape", "scope": "file", "effect": "allow"},
            {"id": 3, "project": "c:/code/app/.git", "path": "src", "scope": "everything", "effect": "allow"},
            {"id": 4, "project": "", "path": "src", "scope": "folder", "effect": "allow"},
            {"id": 1, "project": "c:/code/app/.git", "path": "dup", "scope": "file", "effect": "deny"},
            {"id": 5, "project": "c:/code/app/.git", "path": "lib", "scope": "folder", "effect": "deny"},
            "garbage", 7, null
        ]}"#;
        let parsed = RuleBook::parse(text.as_bytes());
        let ids: Vec<u64> = parsed.rules().iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1, 5]);
        assert_eq!(RuleBook::parse(b"not json").rules().len(), 0);
        assert_eq!(RuleBook::parse(b"{\"rules\": 3}").rules().len(), 0);
    }
}
