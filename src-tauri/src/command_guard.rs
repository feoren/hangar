//! Command guard of the agent gate (ADR 0010): before an agent in a team session runs a shell
//! command that matches one of the user's rules (a force push, a hard reset, deleting a folder
//! outside the project, ...), the gate asks the person, as it does for an edit of a file another
//! session is changing.
//!
//! Pure: the rules file (`command-guard.toml` in Hangar's config folder, written with
//! [`DEFAULT_RULES`] on first use), the built-in rules for what cannot be read or names Hangar's
//! own settings, and matching the program calls `command_split.rs` finds. Nothing here runs a
//! command, reads a file or touches a path the command names: paths are placed lexically.
//!
//! A safety net, not a sandbox. The command is text written by the model; anything that hides
//! what runs (a script file, a variable holding the program, an alias, base64 piped into a
//! shell) passes unrecognized.

use serde::{Deserialize, Serialize};

use crate::command_split::{join, split, Dialect};
use crate::git::key_within;

/// File in Hangar's config folder, next to `profiles.toml`.
pub const FILE_NAME: &str = "command-guard.toml";
/// The commented rules Hangar writes when the file does not exist yet.
pub const DEFAULT_RULES: &str = include_str!("command_guard.toml");
/// Longest command checked; a longer one is asked about with [`RULE_TOO_LONG`].
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;
/// A larger rules file is not read; the default rules apply.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Rules read from the file; the rest is reported.
pub const MAX_RULES: usize = 200;
const MAX_ID_LEN: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 300;
/// Entries of one list field of a rule.
const MAX_LIST: usize = 64;
/// Problems reported for one file.
const MAX_PROBLEMS: usize = 20;

// ---------------------------------------------------------------------------------------------
// Rules

/// One option pattern of a rule (see the comment block of [`DEFAULT_RULES`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Flag {
    /// `--force`: exact, or with `=value`.
    Long(String),
    /// `-f`: alone or inside a group of short options; case counts.
    Short(char),
    /// `-Recurse`, lowercase without the dash: PowerShell style, abbreviations count.
    Word(String),
    /// `/s`, lowercase without the slash: cmd style.
    Slash(String),
}

impl Flag {
    fn parse(text: &str) -> Option<Flag> {
        let plain = |rest: &str| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        if let Some(rest) = text.strip_prefix("--") {
            return plain(rest).then(|| Flag::Long(text.to_owned()));
        }
        if let Some(rest) = text.strip_prefix('-') {
            if !plain(rest) {
                return None;
            }
            let mut chars = rest.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) => Some(Flag::Short(c)),
                _ => Some(Flag::Word(rest.to_ascii_lowercase())),
            };
        }
        let rest = text.strip_prefix('/')?;
        (plain(rest) && rest.len() <= 3).then(|| Flag::Slash(rest.to_ascii_lowercase()))
    }

    /// Whether `option` (one argument that looks like an option) gives this flag.
    fn given_by(&self, option: &str) -> bool {
        match self {
            Flag::Long(long) => option
                .strip_prefix(long.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('=')),
            Flag::Short(c) => {
                let Some(group) = option.strip_prefix('-') else {
                    return false;
                };
                !group.starts_with('-')
                    && !group.is_empty()
                    && group.chars().all(|ch| ch.is_ascii_alphabetic())
                    && !powershell_word(group)
                    && group.contains(*c)
            }
            Flag::Word(word) => {
                let Some(rest) = option.strip_prefix('-') else {
                    return false;
                };
                if rest.starts_with('-') || rest.is_empty() {
                    return false;
                }
                let (name, value) = rest.split_once(':').unwrap_or((rest, ""));
                !name.is_empty()
                    && !value.eq_ignore_ascii_case("$false")
                    && word.starts_with(&name.to_ascii_lowercase())
            }
            Flag::Slash(switch) => option
                .split('/')
                .filter(|part| !part.is_empty())
                .any(|part| {
                    let name = part.split_once(':').map_or(part, |(name, _)| name);
                    name.eq_ignore_ascii_case(switch)
                }),
        }
    }
}

/// `Force`, `Verbose`, `Recurse`: a PowerShell parameter name, not a group of short options
/// (`-Rf`, `-rfv`). PowerShell's `rm` is Remove-Item, whose `-Force` holds an "r".
fn powershell_word(group: &str) -> bool {
    let mut chars = group.chars();
    group.len() >= 4
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.any(|c| c.is_ascii_lowercase())
}

/// One rule of the file, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardRule {
    pub id: String,
    pub description: String,
    programs: Vec<String>,
    subcommands: Vec<String>,
    value_options: Vec<String>,
    any: Vec<Flag>,
    all: Vec<Flag>,
    none: Vec<Flag>,
    /// Upper case word sequences.
    words: Vec<Vec<String>>,
    outside_project: bool,
}

/// What the window shows of a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSummary {
    pub id: String,
    pub description: String,
}

/// A rule or setting of the file that could not be used; the other rules still apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleProblem {
    /// Position of the rule in the file, from 1; `None` for the file as a whole.
    pub rule: Option<usize>,
    /// The rule's id, when it has a readable one.
    pub id: Option<String>,
    /// From 1, for a syntax error.
    pub line: Option<usize>,
    pub message: String,
}

/// The rules in use and what was wrong with the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuleSet {
    pub rules: Vec<GuardRule>,
    pub problems: Vec<RuleProblem>,
    /// The file could not be read as a whole, so the default rules apply.
    pub defaults: bool,
}

impl RuleSet {
    pub fn summaries(&self) -> Vec<RuleSummary> {
        self.rules
            .iter()
            .map(|rule| RuleSummary {
                id: rule.id.clone(),
                description: rule.description.clone(),
            })
            .collect()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    description: String,
    program: Vec<String>,
    #[serde(default)]
    subcommand: Vec<String>,
    #[serde(default)]
    value_options: Vec<String>,
    #[serde(default)]
    any: Vec<String>,
    #[serde(default)]
    all: Vec<String>,
    #[serde(default)]
    none: Vec<String>,
    #[serde(default)]
    words: Vec<String>,
    #[serde(default)]
    outside_project: bool,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// Any whitespace or control character makes a pattern unusable: arguments never hold one where
/// a rule could mean it.
fn plain_text(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn check_rule(raw: RawRule) -> Result<GuardRule, String> {
    if raw.id.starts_with(RESERVED_PREFIX) {
        return Err(format!(
            "ids starting with {RESERVED_PREFIX:?} are reserved for Hangar's built-in rules"
        ));
    }
    if !valid_id(&raw.id) {
        return Err(format!(
            "id {:?} must be 1 to {MAX_ID_LEN} lowercase letters, digits, \"-\", \"_\" or \".\"",
            raw.id
        ));
    }
    let description = raw.description.trim().to_owned();
    if description.is_empty()
        || description.chars().count() > MAX_DESCRIPTION_CHARS
        || description.chars().any(char::is_control)
    {
        return Err(format!(
            "description must be 1 to {MAX_DESCRIPTION_CHARS} characters on one line"
        ));
    }
    let lists = [
        ("program", &raw.program),
        ("subcommand", &raw.subcommand),
        ("value_options", &raw.value_options),
        ("any", &raw.any),
        ("all", &raw.all),
        ("none", &raw.none),
        ("words", &raw.words),
    ];
    for (name, list) in lists {
        if list.len() > MAX_LIST {
            return Err(format!("{name} has more than {MAX_LIST} entries"));
        }
    }
    if raw.program.is_empty() {
        return Err("program needs at least one name".to_owned());
    }
    let mut programs = Vec::new();
    for program in &raw.program {
        if !plain_text(program) || program.contains(['/', '\\']) {
            return Err(format!(
                "program {program:?} must be a name without folder or spaces"
            ));
        }
        programs.push(program.to_lowercase());
    }
    for (name, list) in [
        ("subcommand", &raw.subcommand),
        ("value_options", &raw.value_options),
    ] {
        if let Some(bad) = list.iter().find(|entry| !plain_text(entry)) {
            return Err(format!("{name} entry {bad:?} must be one word"));
        }
    }
    let flags = |name: &str, list: &[String]| -> Result<Vec<Flag>, String> {
        list.iter()
            .map(|text| {
                Flag::parse(text).ok_or_else(|| {
                    format!("{name} entry {text:?} is not an option like \"--force\", \"-f\", \"-Recurse\" or \"/s\"")
                })
            })
            .collect()
    };
    let words = raw
        .words
        .iter()
        .map(|text| {
            let words = upper_words(text);
            if words.is_empty() {
                Err(format!("words entry {text:?} holds no word"))
            } else {
                Ok(words)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GuardRule {
        id: raw.id,
        description,
        programs,
        subcommands: raw.subcommand,
        value_options: raw.value_options,
        any: flags("any", &raw.any)?,
        all: flags("all", &raw.all)?,
        none: flags("none", &raw.none)?,
        words,
        outside_project: raw.outside_project,
    })
}

fn line_of(source: &str, offset: usize) -> usize {
    source
        .get(..offset)
        .map_or(0, |before| before.matches('\n').count())
        + 1
}

/// The rules of `source` (the file's text). A rule that does not read is reported and left out;
/// the others apply. A file that is not TOML at all is reported and the default rules apply, so a
/// typo never switches the guard off.
pub fn parse_rules(source: &str) -> RuleSet {
    let table: toml::Table = match toml::from_str(source) {
        Ok(table) => table,
        Err(error) => {
            let mut set = default_rules();
            set.defaults = true;
            set.problems.insert(
                0,
                RuleProblem {
                    rule: None,
                    id: None,
                    line: error.span().map(|span| line_of(source, span.start)),
                    message: error.message().trim().to_owned(),
                },
            );
            return set;
        }
    };
    let mut set = RuleSet::default();
    let report = |problems: &mut Vec<RuleProblem>, problem: RuleProblem| {
        if problems.len() < MAX_PROBLEMS {
            problems.push(problem);
        }
    };
    for key in table.keys() {
        if key != "version" && key != "rule" {
            report(
                &mut set.problems,
                RuleProblem {
                    rule: None,
                    id: None,
                    line: None,
                    message: format!("unknown setting `{key}`"),
                },
            );
        }
    }
    let entries = match table.get("rule") {
        None => Vec::new(),
        Some(toml::Value::Array(entries)) => entries.clone(),
        Some(_) => {
            report(
                &mut set.problems,
                RuleProblem {
                    rule: None,
                    id: None,
                    line: None,
                    message: "`rule` must be a list of [[rule]] tables".to_owned(),
                },
            );
            Vec::new()
        }
    };
    for (index, entry) in entries.into_iter().enumerate() {
        let id = entry
            .get("id")
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        let problem = |message: String| RuleProblem {
            rule: Some(index + 1),
            id: id.clone(),
            line: None,
            message,
        };
        if set.rules.len() == MAX_RULES {
            report(
                &mut set.problems,
                problem(format!("more than {MAX_RULES} rules; the rest is ignored")),
            );
            break;
        }
        let checked = entry
            .try_into::<RawRule>()
            .map_err(|error| error.message().trim().to_owned())
            .and_then(check_rule);
        match checked {
            Ok(rule) if set.rules.iter().any(|known| known.id == rule.id) => {
                report(
                    &mut set.problems,
                    problem(format!("id {:?} is used by an earlier rule", rule.id)),
                );
            }
            Ok(rule) => set.rules.push(rule),
            Err(message) => report(&mut set.problems, problem(message)),
        }
    }
    set
}

/// The rules Hangar ships; a test keeps them free of problems.
pub fn default_rules() -> RuleSet {
    // Checked first, so `parse_rules` never falls back to this function again.
    if toml::from_str::<toml::Table>(DEFAULT_RULES).is_err() {
        return RuleSet::default();
    }
    parse_rules(DEFAULT_RULES)
}

/// The program's name as rules compare it: without folder, lowercase, without `.exe`, `.com`,
/// `.cmd` or `.bat`.
pub fn program_name(word: &str) -> String {
    let name = word
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .to_lowercase();
    for suffix in [".exe", ".com", ".cmd", ".bat"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            if !stem.is_empty() {
                return stem.to_owned();
            }
        }
    }
    name
}

// ---------------------------------------------------------------------------------------------
// Paths

/// How paths read on the platform the agent's commands run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Drive letters, `\` as a separator, `%VAR%`, Git Bash's `/c/...`.
    pub windows: bool,
    /// Keys compare without case (as [`crate::git::path_key`]).
    pub fold_case: bool,
}

impl Style {
    pub const HOST: Style = Style {
        windows: cfg!(windows),
        fold_case: cfg!(any(windows, target_os = "macos")),
    };
}

/// Where a command runs, as [`crate::git::path_key`] keys.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Place {
    /// Hangar's own folders that hold settings or gate state (the config folder, the gate's
    /// folder in the local data folder): an argument inside one asks with [`RULE_HANGAR_CONFIG`].
    /// Never the whole local data folder: the team sessions' worktrees live there.
    pub protected: Vec<String>,
    /// Single protected files elsewhere (the remembered answers in the local data folder).
    pub protected_files: Vec<String>,
    /// Lowercase identifier-like folder names ([`protected_names`]) that give a protected folder
    /// away in a path Hangar cannot place.
    pub protected_names: Vec<String>,
    /// The hook's working folder.
    pub cwd: Option<String>,
    /// The session's project folder (its worktree's top); without one every path counts as
    /// outside.
    pub project: Option<String>,
    pub home: Option<String>,
}

/// Hangar's settings files: an argument naming one outside the session's project is asked about
/// wherever it points (a variable or wildcard may hide the folder). Inside the project a file of
/// that name is the project's own (Hangar's sources name them).
const PROTECTED_FILES: &[&str] = &[FILE_NAME, crate::gate_rules::FILE_NAME];

/// The names for [`Place::protected_names`]: the last part of each protected folder when it
/// looks like an app identifier (`com.example.hangar`); a plain word such as `hangar` would match
/// too much. Only compared against paths outside the session's project.
pub fn protected_names(folders: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for folder in folders {
        let last = folder
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_lowercase();
        if last.matches('.').count() >= 2 && !names.contains(&last) {
            names.push(last);
        }
    }
    names
}

/// A path an argument names, placed without touching the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    key: String,
    /// The part from the first wildcard on was cut off: the key's folder is not itself named.
    glob: bool,
}

/// Home folder spellings of the common shells, matched without case.
const HOME_NAMES: &[&str] = &[
    "$home",
    "${home}",
    "$env:userprofile",
    "${env:userprofile}",
    "$env:home",
    "%userprofile%",
];

/// The absolute key of `arg` from `cwd`, or `None` when it cannot be placed: a variable, a
/// substitution, another user's home, a drive relative path, no working folder for a relative
/// one.
fn place(arg: &str, cwd: Option<&str>, home: Option<&str>, style: Style) -> Option<Target> {
    let mut text = if style.windows {
        arg.replace('\\', "/")
    } else {
        arg.to_owned()
    };
    let lower = text.to_lowercase();
    let home_rest = if text == "~" || text.starts_with("~/") {
        Some(text[1..].to_owned())
    } else {
        HOME_NAMES.iter().find_map(|name| {
            let rest = lower.strip_prefix(name)?;
            if rest.is_empty() || rest.starts_with('/') {
                text.get(name.len()..).map(str::to_owned)
            } else {
                None
            }
        })
    };
    if let Some(rest) = home_rest {
        text = format!("{}{rest}", home?);
    } else if text.starts_with('~') {
        return None;
    }
    if text.contains(['$', '`']) || (style.windows && text.contains('%')) {
        return None;
    }
    let (prefix, rest) = split_root(&text, style)?;
    let joined = match prefix {
        Some(prefix) => (prefix, rest),
        None => {
            let base = cwd?;
            let (prefix, base_rest) = split_root(base, style)?;
            (prefix?, format!("{base_rest}/{rest}"))
        }
    };
    let (prefix, rest) = joined;
    let mut parts: Vec<&str> = Vec::new();
    let mut glob = false;
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part if part.contains(['*', '?', '[']) => {
                glob = true;
                break;
            }
            part => parts.push(part),
        }
    }
    let mut key = prefix;
    for part in parts {
        key.push('/');
        key.push_str(part);
    }
    if key.is_empty() {
        key.push('/');
    }
    if style.fold_case {
        key = key.to_lowercase();
    }
    Some(Target { key, glob })
}

/// An absolute path's root (`""` for `/`, `c:` for a drive, `//host` for a share) and the rest;
/// `Some((None, path))` for a relative path, `None` for a drive relative one (`c:x`).
fn split_root(text: &str, style: Style) -> Option<(Option<String>, String)> {
    if !style.windows {
        return Some(match text.strip_prefix('/') {
            Some(rest) => (Some(String::new()), rest.to_owned()),
            None => (None, text.to_owned()),
        });
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = text[..2].to_ascii_lowercase();
        let rest = &text[2..];
        return if rest.is_empty() || rest.starts_with('/') {
            Some((Some(drive), rest.to_owned()))
        } else {
            None
        };
    }
    if let Some(rest) = text.strip_prefix("//") {
        let (host, rest) = rest.split_once('/').unwrap_or((rest, ""));
        return Some((Some(format!("//{host}")), rest.to_owned()));
    }
    if let Some(rest) = text.strip_prefix('/') {
        // Git Bash and Cygwin name drives `/c/...`, `/cygdrive/c/...`; WSL `/mnt/c/...`.
        let rest_lower = rest.to_ascii_lowercase();
        let drive_part = ["cygdrive/", "mnt/"]
            .iter()
            .find_map(|lead| rest_lower.strip_prefix(lead).map(|_| &rest[lead.len()..]))
            .unwrap_or(rest);
        let (first, tail) = drive_part.split_once('/').unwrap_or((drive_part, ""));
        if first.len() == 1 && first.as_bytes()[0].is_ascii_alphabetic() {
            return Some((
                Some(format!("{}:", first.to_ascii_lowercase())),
                tail.to_owned(),
            ));
        }
        // Another root (Git Bash's own folder): never inside a project on a drive.
        return Some((Some(String::new()), rest.to_owned()));
    }
    Some((None, text.to_owned()))
}

/// Whether `target` lies strictly inside `project` and not in its `.git`. A wildcard right in
/// the project folder (`./*`) names its contents, which counts as inside.
fn inside(target: &Target, project: &str, fold_case: bool) -> bool {
    let project = if fold_case {
        project.to_lowercase()
    } else {
        project.to_owned()
    };
    if !key_within(&target.key, &project) {
        return false;
    }
    if target.key == project {
        return target.glob;
    }
    let rest = &target.key[project.len()..];
    let first = rest.trim_start_matches('/').split('/').next().unwrap_or("");
    !first.eq_ignore_ascii_case(".git")
}

// ---------------------------------------------------------------------------------------------
// Matching

/// Built-in rules that are not in the file and cannot be switched off there: what Hangar cannot
/// read is asked about rather than let through.
pub const RULE_TOO_COMPLEX: &str = "hangar-too-complex";
pub const RULE_STDIN_SCRIPT: &str = "hangar-stdin-script";
pub const RULE_TOO_LONG: &str = "hangar-too-long";
/// Edits and commands that name Hangar's own settings: the guard's rules, the remembered answers.
pub const RULE_HANGAR_CONFIG: &str = "hangar-config";
/// Ids with this start belong to Hangar's built-in rules; a rule of the file may not use one.
pub const RESERVED_PREFIX: &str = "hangar-";

/// The description of a built-in rule (English, for the agent; the window shows its own text).
pub fn builtin_description(id: &str) -> Option<&'static str> {
    Some(match id {
        RULE_TOO_COMPLEX => {
            "the command is too complex for Hangar to check: nested too deep, too many commands, \
             or a quote or substitution left open"
        }
        RULE_STDIN_SCRIPT => {
            "a shell reads its script from a pipe, a here-string or stdin, which Hangar cannot see"
        }
        RULE_TOO_LONG => {
            "the command is longer than 64 KiB or holds a NUL character, so Hangar cannot check it"
        }
        RULE_HANGAR_CONFIG => {
            "the command or edit touches Hangar's own settings: its config folder, \
             command-guard.toml or gate-rules.json"
        }
        _ => return None,
    })
}

/// Whether a rule is one Hangar ships with its wording unchanged: a built-in rule, or a default
/// rule of [`DEFAULT_RULES`] whose description the user did not edit. The window shows such a
/// rule in its own language and a user's rule verbatim.
pub fn is_builtin(id: &str, description: &str) -> bool {
    static DEFAULTS: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    builtin_description(id).is_some()
        || DEFAULTS
            .get_or_init(|| {
                default_rules()
                    .rules
                    .into_iter()
                    .map(|rule| (rule.id, rule.description))
                    .collect()
            })
            .iter()
            .any(|(known, text)| known == id && text == description)
}

/// Whether "always allow" may be remembered for the rule: never for Hangar's own settings, or an
/// answer would switch the guard off for good.
pub fn rememberable(id: &str) -> bool {
    id != RULE_HANGAR_CONFIG
}

/// A rule that matched the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub rule_id: String,
    pub description: String,
    /// The words of the program call that matched, joined; empty for a rule about the whole
    /// command.
    pub call: String,
}

impl Hit {
    /// The hit of a built-in rule.
    pub fn builtin(id: &str, call: String) -> Hit {
        Hit {
            rule_id: id.to_owned(),
            description: builtin_description(id).unwrap_or_default().to_owned(),
            call,
        }
    }
}

/// A command switch in cmd style (`/s`, `/q`, `/a:h`): only for rules that compare such.
fn slash_switch(arg: &str) -> bool {
    arg.strip_prefix('/').is_some_and(|rest| {
        let name = rest.split_once(':').map_or(rest, |(name, _)| name);
        (1..=3).contains(&name.len())
            && name
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == '?' || c == '/')
    })
}

fn upper_words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_uppercase)
        .collect()
}

/// One element of a wildcard pattern.
enum GlobToken {
    Star,
    One(Option<char>),
    /// `[abc]`, `[a-z]`, `[!x]` or `[^x]`: the ranges and whether they are negated.
    Class(Vec<(char, char)>, bool),
}

impl GlobToken {
    fn accepts(&self, c: char) -> bool {
        match self {
            GlobToken::Star => true,
            GlobToken::One(None) => true,
            GlobToken::One(Some(wanted)) => *wanted == c,
            GlobToken::Class(ranges, negated) => {
                ranges.iter().any(|(low, high)| (*low..=*high).contains(&c)) != *negated
            }
        }
    }
}

/// `*`, `?` and `[...]` of a pattern; an unclosed `[` is a plain character.
fn glob_tokens(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => tokens.push(GlobToken::Star),
            '?' => tokens.push(GlobToken::One(None)),
            '[' if chars[i + 1..].contains(&']') => {
                let mut j = i + 1;
                let negated = matches!(chars.get(j), Some('!' | '^'));
                if negated {
                    j += 1;
                }
                let mut ranges = Vec::new();
                // A `]` right after the opening bracket belongs to the class.
                let mut first = true;
                while j < chars.len() && (chars[j] != ']' || first) {
                    first = false;
                    if chars.get(j + 1) == Some(&'-') && chars.get(j + 2).is_some_and(|c| *c != ']')
                    {
                        ranges.push((chars[j], chars[j + 2]));
                        j += 3;
                    } else {
                        ranges.push((chars[j], chars[j]));
                        j += 1;
                    }
                }
                if j >= chars.len() {
                    tokens.push(GlobToken::One(Some('[')));
                } else {
                    tokens.push(GlobToken::Class(ranges, negated));
                    i = j;
                }
            }
            c => tokens.push(GlobToken::One(Some(c))),
        }
        i += 1;
    }
    tokens
}

/// Whether `name` matches the wildcard `pattern` (`*`, `?`, `[...]`).
fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern = glob_tokens(pattern);
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while n < name.len() {
        if p < pattern.len()
            && !matches!(pattern[p], GlobToken::Star)
            && pattern[p].accepts(name[n])
        {
            p += 1;
            n += 1;
        } else if p < pattern.len() && matches!(pattern[p], GlobToken::Star) {
            star = Some(p);
            mark = n;
            p += 1;
        } else if let Some(s) = star {
            p = s + 1;
            mark += 1;
            n = mark;
        } else {
            return false;
        }
    }
    pattern[p..]
        .iter()
        .all(|token| matches!(token, GlobToken::Star))
}

/// Whether a path component is a wildcard pattern.
fn is_wildcard(component: &str) -> bool {
    component.contains(['*', '?']) || (component.contains('[') && component.contains(']'))
}

/// Letters a wildcard component spells out itself, outside a plain extension (`.json`) and
/// outside bracket classes: `*.json` has none, `command-g*.toml` nine.
fn own_letters(component: &str) -> usize {
    let body = match component.rsplit_once('.') {
        Some((base, ext)) if !ext.is_empty() && !is_wildcard(ext) => base,
        _ => component,
    };
    let mut count = 0;
    let mut in_class = false;
    for c in body.chars() {
        match c {
            '[' => in_class = true,
            ']' if in_class => in_class = false,
            '*' | '?' => {}
            _ if in_class => {}
            _ => count += 1,
        }
    }
    count
}

/// Most variants a brace expansion in one option word is taken apart into.
const MAX_BRACE_VARIANTS: usize = 64;
/// Most braces an expansion is tried for. Each brace is one level of [`brace_variants`]'
/// recursion, so this also bounds the stack an agent's command can make it use; a word with
/// more falls back to the prefix rule of [`Flag::given_by_word`].
const MAX_BRACE_GROUPS: usize = 8;

/// The words a POSIX shell's brace expansion makes of `word` (`--{force,x}` gives `--force` and
/// `--x`); `None` when it holds a brace this cannot expand (a sequence `{a..z}`, an unclosed
/// one, too many variants).
fn brace_variants(word: &str) -> Option<Vec<String>> {
    let Some(open) = word.find('{') else {
        return Some(vec![word.to_owned()]);
    };
    if word.matches('{').count() > MAX_BRACE_GROUPS {
        return None;
    }
    let mut depth = 0usize;
    let mut close = None;
    let mut commas = Vec::new();
    for (index, c) in word[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + index);
                    break;
                }
            }
            ',' if depth == 1 => commas.push(open + index),
            _ => {}
        }
    }
    let close = close?;
    if commas.is_empty() {
        return None;
    }
    let (prefix, suffix) = (&word[..open], &word[close + 1..]);
    let mut bounds = vec![open];
    bounds.extend(&commas);
    bounds.push(close);
    let mut out = Vec::new();
    for pair in bounds.windows(2) {
        let part = &word[pair[0] + 1..pair[1]];
        for variant in brace_variants(&format!("{prefix}{part}{suffix}"))? {
            out.push(variant);
            if out.len() > MAX_BRACE_VARIANTS {
                return None;
            }
        }
    }
    Some(out)
}

impl Flag {
    /// The pattern as written in the rules file.
    fn text(&self) -> String {
        match self {
            Flag::Long(long) => long.clone(),
            Flag::Short(c) => format!("-{c}"),
            Flag::Word(word) => format!("-{word}"),
            Flag::Slash(switch) => format!("/{switch}"),
        }
    }

    /// [`Flag::given_by`] through a brace expansion: any variant counts, and an option word
    /// whose braces cannot be expanded counts when the part before them fits the pattern.
    fn given_by_word(&self, option: &str) -> bool {
        if !option.contains('{') {
            return self.given_by(option);
        }
        match brace_variants(option) {
            Some(variants) => variants.iter().any(|variant| self.given_by(variant)),
            None => {
                let prefix = option.split('{').next().unwrap_or_default();
                let pattern = self.text();
                pattern.starts_with(prefix)
                    || pattern
                        .to_ascii_lowercase()
                        .starts_with(&prefix.to_ascii_lowercase())
            }
        }
    }
}

impl GuardRule {
    fn matches(&self, call: &[String], cwd: Option<&str>, place_of: &Place, style: Style) -> bool {
        let Some(first) = call.first() else {
            return false;
        };
        let program = program_name(first);
        if !self.programs.iter().any(|p| glob_match(p, &program)) {
            return false;
        }
        let slash = self
            .any
            .iter()
            .chain(&self.all)
            .chain(&self.none)
            .any(|flag| matches!(flag, Flag::Slash(_)));
        let mut options: Vec<&str> = Vec::new();
        let mut positionals: Vec<&str> = Vec::new();
        let mut ended = false;
        let mut args = call[1..].iter();
        while let Some(arg) = args.next() {
            if ended {
                positionals.push(arg);
            } else if arg == "--" {
                ended = true;
            } else if arg.starts_with('-') && arg.len() > 1 {
                options.push(arg);
                if self.value_options.iter().any(|v| v == arg) {
                    args.next();
                }
            } else if slash && slash_switch(arg) {
                options.push(arg);
            } else {
                positionals.push(arg);
            }
        }
        if !self.subcommands.is_empty() {
            match positionals.first() {
                Some(sub) if self.subcommands.iter().any(|s| s == sub) => {
                    positionals.remove(0);
                }
                _ => return false,
            }
        }
        let given = |flag: &Flag| options.iter().any(|option| flag.given_by_word(option));
        if !self.any.is_empty() && !self.any.iter().any(given) {
            return false;
        }
        if !self.all.iter().all(given) || self.none.iter().any(given) {
            return false;
        }
        if !self.words.is_empty() {
            let found = call[1..].iter().any(|arg| {
                let words = upper_words(arg);
                self.words.iter().any(|wanted| {
                    words
                        .windows(wanted.len())
                        .any(|window| window == wanted.as_slice())
                })
            });
            if !found {
                return false;
            }
        }
        if self.outside_project {
            let outside = positionals.iter().any(|arg| {
                match (
                    place(arg, cwd, place_of.home.as_deref(), style),
                    &place_of.project,
                ) {
                    (Some(target), Some(project)) => !inside(&target, project, style.fold_case),
                    _ => true,
                }
            });
            if !outside {
                return false;
            }
        }
        true
    }
}

/// Programs that change the working folder for the commands after them.
const CHANGE_FOLDER: &[&str] = &["cd", "chdir", "pushd", "set-location", "sl"];
/// Programs that go back to an earlier working folder, which the lexer does not track.
const FOLDER_BACK: &[&str] = &["popd", "pop-location"];

/// The folders apps keep their settings and data in, as a command names them before Hangar can
/// place them (lowercase, `/` separators).
const APP_FOLDERS: &[&str] = &[
    "$appdata",
    "${appdata}",
    "%appdata%",
    "$env:appdata",
    "${env:appdata}",
    "$localappdata",
    "${localappdata}",
    "%localappdata%",
    "$env:localappdata",
    "${env:localappdata}",
    "$xdg_config_home",
    "${xdg_config_home}",
    "$xdg_data_home",
    "${xdg_data_home}",
    "~/.config",
    "$home/.config",
    "~/.local/share",
    "$home/.local/share",
    "~/library/application support",
    "$home/library/application support",
];

/// Whether `lower` (an argument, lowercase) puts a wildcard right below one of [`APP_FOLDERS`]
/// (`$env:APPDATA\c*`): it may reach Hangar's folder there whatever letter it starts with.
fn wildcard_below_app_folders(lower: &str) -> bool {
    let text = lower.replace('\\', "/");
    APP_FOLDERS.iter().any(|folder| {
        text.strip_prefix(folder)
            .and_then(|rest| rest.strip_prefix('/'))
            .and_then(|rest| rest.split('/').next())
            .is_some_and(is_wildcard)
    })
}

/// The key of the folder holding `key`; `None` for a root.
fn parent_key(key: &str) -> Option<&str> {
    let (parent, _) = key.rsplit_once('/')?;
    Some(if parent.is_empty() { "/" } else { parent })
}

/// Whether a path component written by the agent may stand for `name`: equal, a wildcard
/// pattern that matches it (`com.f*`), or a Windows 8.3 short name of it (`COMFEO~1`,
/// `COMMAN~1.TOM`: the first characters of the name without dots and spaces).
fn component_names(component: &str, name: &str) -> bool {
    // `*.json` or `com.*` name half the disk; a pattern needs letters of its own, outside a plain
    // extension, to point at one of Hangar's names (`command-g*.toml`, `com.f*`).
    if component == name
        || (is_wildcard(component) && own_letters(component) >= 5 && glob_match(component, name))
    {
        return true;
    }
    let Some((stem, rest)) = component.split_once('~') else {
        return false;
    };
    let digits = rest.split('.').next().unwrap_or_default();
    if stem.is_empty() || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let base = name.rsplit_once('.').map_or(name, |(base, _)| base);
    let squeezed: String = base.chars().filter(|c| *c != '.' && *c != ' ').collect();
    let whole: String = name.chars().filter(|c| *c != '.' && *c != ' ').collect();
    squeezed.starts_with(stem) || whole.starts_with(stem)
}

/// Whether `arg` names one of Hangar's protected folders or files. A path inside the session's
/// project never counts by name, only by where it lies: Hangar's own worktrees live in its data
/// folder and its sources name the settings files.
fn protected_path(arg: &str, cwd: Option<&str>, place_of: &Place, style: Style) -> bool {
    let fold = |text: &str| {
        if style.fold_case {
            text.to_lowercase()
        } else {
            text.to_owned()
        }
    };
    let target = place(arg, cwd, place_of.home.as_deref(), style);
    if let Some(target) = &target {
        let in_folder = place_of
            .protected
            .iter()
            .any(|folder| key_within(&target.key, &fold(folder)));
        let is_file = place_of
            .protected_files
            .iter()
            .any(|file| target.key == fold(file));
        if in_folder || is_file {
            return true;
        }
        if let Some(project) = &place_of.project {
            if inside(target, project, style.fold_case) {
                return false;
            }
        }
    }
    // A wildcard right below the folder that holds Hangar's own (`~/.config/c*`).
    if let Some(target) = &target {
        if target.glob
            && place_of
                .protected
                .iter()
                .any(|folder| parent_key(&fold(folder)) == Some(target.key.as_str()))
        {
            return true;
        }
    }
    // Outside the project or not placeable: names count, also through wildcards and short names,
    // and the identifier anywhere in the text (a mangled or variable-built path).
    let lower = arg.to_lowercase();
    if wildcard_below_app_folders(&lower) {
        return true;
    }
    if place_of
        .protected_names
        .iter()
        .any(|name| lower.contains(name.as_str()))
    {
        return true;
    }
    lower.split(['/', '\\']).any(|component| {
        PROTECTED_FILES
            .iter()
            .map(|name| name.to_string())
            .chain(place_of.protected_names.iter().cloned())
            .any(|name| component_names(component, &name))
    })
}

/// Whether an edit of `path` (as the agent named it, relative to `cwd`) touches Hangar's own
/// settings ([`RULE_HANGAR_CONFIG`]).
pub fn edit_touches_hangar(path: &str, place_of: &Place, style: Style) -> bool {
    protected_path(path, place_of.cwd.as_deref(), place_of, style)
}

/// The shell dialects a tool's command is read in. A tool whose shell is not known (`Shell`, an
/// extension on Windows) is read in all three and every hit counts.
pub fn dialects_of(tool: &str) -> &'static [Dialect] {
    match tool {
        "PowerShell" => &[Dialect::PowerShell],
        "Shell" => &[Dialect::Posix, Dialect::PowerShell, Dialect::Cmd],
        _ => &[Dialect::Posix],
    }
}

/// Every rule that matches `command`, each once, in the order found: the file's rules per
/// program call, and the built-in rules for what cannot be read ([`RULE_TOO_COMPLEX`],
/// [`RULE_STDIN_SCRIPT`]) or names Hangar's own settings ([`RULE_HANGAR_CONFIG`]). A `cd`
/// moves the working folder for the calls after it; `cd -`, a bare `cd` and `popd` leave it
/// unknown.
pub fn check(
    rules: &[GuardRule],
    command: &str,
    dialects: &[Dialect],
    place_of: &Place,
    style: Style,
) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();
    let mut add = |hit: Hit| {
        if !hits.iter().any(|known| known.rule_id == hit.rule_id) {
            hits.push(hit);
        }
    };
    for dialect in dialects {
        let analysis = split(command, *dialect);
        if analysis.truncated || analysis.unclosed {
            add(Hit::builtin(RULE_TOO_COMPLEX, String::new()));
        }
        if analysis.stdin_script {
            add(Hit::builtin(RULE_STDIN_SCRIPT, String::new()));
        }
        let mut cwd = place_of.cwd.clone();
        let mut redirects = analysis.redirects.iter().peekable();
        for (index, call) in analysis.calls.iter().enumerate() {
            while let Some((_, target)) = redirects.next_if(|(at, _)| *at <= index) {
                if protected_path(target, cwd.as_deref(), place_of, style) {
                    add(Hit::builtin(RULE_HANGAR_CONFIG, format!("> {target}")));
                }
            }
            let program = call.first().map(|word| program_name(word));
            let program = program.as_deref().unwrap_or_default();
            if CHANGE_FOLDER.contains(&program) {
                let target = call.iter().skip(1).find(|arg| !arg.starts_with('-'));
                cwd = target.and_then(|arg| {
                    place(arg, cwd.as_deref(), place_of.home.as_deref(), style).map(|t| t.key)
                });
                continue;
            }
            if FOLDER_BACK.contains(&program) {
                cwd = None;
                continue;
            }
            let shown_call = join(call);
            if call
                .iter()
                .skip(1)
                .any(|arg| protected_path(arg, cwd.as_deref(), place_of, style))
            {
                add(Hit::builtin(RULE_HANGAR_CONFIG, shown_call.clone()));
            }
            // `g$@it`, `--for${x}ce`: parameters that are empty in an agent's shell vanish.
            let variant = without_empty_parameters(call);
            for rule in rules {
                let matched = rule.matches(call, cwd.as_deref(), place_of, style)
                    || variant.as_ref().is_some_and(|variant| {
                        rule.matches(variant, cwd.as_deref(), place_of, style)
                    });
                if matched {
                    add(Hit {
                        rule_id: rule.id.clone(),
                        description: rule.description.clone(),
                        call: shown_call.clone(),
                    });
                }
            }
        }
        for (_, target) in redirects {
            if protected_path(target, cwd.as_deref(), place_of, style) {
                add(Hit::builtin(RULE_HANGAR_CONFIG, format!("> {target}")));
            }
        }
    }
    hits
}

/// The call with `$@`, `$*` and `${...}` removed from its words, which a shell expands to
/// nothing when unset; `None` when no word has one.
fn without_empty_parameters(call: &[String]) -> Option<Vec<String>> {
    let strip = |word: &str| {
        let mut out = String::new();
        let mut rest = word;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            if let Some(tail) = after.strip_prefix(['@', '*']) {
                rest = tail;
            } else if let Some(inner) = after.strip_prefix('{') {
                match inner.find('}') {
                    Some(close) => rest = &inner[close + 1..],
                    None => {
                        out.push_str(&rest[at..]);
                        rest = "";
                    }
                }
            } else {
                out.push('$');
                rest = after;
            }
        }
        out.push_str(rest);
        out
    };
    if !call
        .iter()
        .any(|word| word.contains("$@") || word.contains("$*") || word.contains("${"))
    {
        return None;
    }
    Some(call.iter().map(|word| strip(word)).collect())
}

/// The command as the window shows it: line breaks and tabs kept, any other control character
/// as a visible `<U+XXXX>` mark (an escape sequence could hide or rewrite what the person reads),
/// cut at `max` characters. The second value counts the characters left out.
pub fn shown_counted(command: &str, max: usize) -> (String, usize) {
    let mut out = String::new();
    let mut hidden = 0;
    for (count, c) in command.chars().enumerate() {
        if count >= max {
            hidden += 1;
            continue;
        }
        if (c.is_control() && c != '\n' && c != '\t') || crate::text::is_invisible(c) {
            out.push_str(&format!("<U+{:04X}>", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    (out, hidden)
}

/// The first `max` characters of `command` as they are, and how many were left out.
pub fn cut(command: &str, max: usize) -> (String, usize) {
    let kept: String = command.chars().take(max).collect();
    let hidden = command.chars().count() - kept.chars().count();
    (kept, hidden)
}

/// [`shown_counted`] with a `…` where text was left out.
pub fn shown(command: &str, max: usize) -> String {
    let (mut text, hidden) = shown_counted(command, max);
    if hidden > 0 {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIX: Style = Style {
        windows: false,
        fold_case: false,
    };
    const WINDOWS: Style = Style {
        windows: true,
        fold_case: true,
    };

    fn unix_place() -> Place {
        Place {
            cwd: Some("/home/me/app".into()),
            project: Some("/home/me/app".into()),
            home: Some("/home/me".into()),
            protected: vec![
                "/home/me/.config/com.example.hangar".into(),
                "/home/me/.local/share/com.example.hangar/gate".into(),
            ],
            protected_files: vec!["/home/me/.local/share/com.example.hangar/gate-rules.json".into()],
            protected_names: protected_names(&["/home/me/.config/com.example.hangar"]),
        }
    }

    fn windows_place() -> Place {
        Place {
            cwd: Some("c:/code/app".into()),
            project: Some("c:/code/app".into()),
            home: Some("c:/users/me".into()),
            protected: vec!["c:/users/me/appdata/roaming/com.example.hangar".into()],
            protected_files: Vec::new(),
            protected_names: protected_names(&["c:/users/me/appdata/roaming/com.example.hangar"]),
        }
    }

    fn defaults() -> Vec<GuardRule> {
        let set = default_rules();
        assert!(set.problems.is_empty(), "{:?}", set.problems);
        set.rules
    }

    fn ids(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|hit| hit.rule_id.as_str()).collect()
    }

    /// The rule ids `command` hits as Claude Code's `Bash` sends it.
    fn hits(command: &str) -> Vec<String> {
        check(&defaults(), command, &[Dialect::Posix], &unix_place(), UNIX)
            .into_iter()
            .map(|hit| hit.rule_id)
            .collect()
    }

    fn hits_in(command: &str, tool: &str) -> Vec<String> {
        check(
            &defaults(),
            command,
            dialects_of(tool),
            &windows_place(),
            WINDOWS,
        )
        .into_iter()
        .map(|hit| hit.rule_id)
        .collect()
    }

    fn assert_hits(rule: &str, commands: &[&str]) {
        for command in commands {
            let found = hits(command);
            assert!(found.iter().any(|id| id == rule), "{command}: {found:?}");
        }
    }

    fn assert_passes(commands: &[&str]) {
        for command in commands {
            assert_eq!(hits(command), Vec::<String>::new(), "{command}");
        }
    }

    #[test]
    fn the_default_rules_read_without_problems() {
        let set = default_rules();
        assert!(set.problems.is_empty(), "{:?}", set.problems);
        assert!(!set.defaults);
        let ids: Vec<&str> = set.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "git-push-force",
                "git-reset-hard",
                "git-clean-force",
                "rm-outside-project",
                "remove-item-outside-project",
                "del-outside-project",
                "sql-drop",
                "format-disk",
            ]
        );
        assert!(DEFAULT_RULES.starts_with("# Hangar"));
    }

    #[test]
    fn force_pushes_are_caught_but_not_safe_pushes() {
        assert_hits(
            "git-push-force",
            &[
                "git push --force",
                "git push -f origin main",
                "git push origin main --force",
                "git push -uf origin main",
                "git -C ../other push --force",
                "/usr/bin/git push --force",
                r#""C:\Program Files\Git\cmd\git.exe" push -f"#,
                // Git Bash drops every backslash outside quotes.
                r"git push \--force",
                r"g\it push --force",
                "git push $'--force'",
                "git push $'--for\\x63e'",
                // Brace expansion: each variant counts, or the part before an unexpandable one.
                "git push --{force,x}",
                "git push --for{c,}e",
                "git push --{a..z}",
            ],
        );
        assert_passes(&[
            "git push",
            "git push origin main",
            "git push --force-with-lease",
            "git push --force-if-includes --force-with-lease=main",
            "git status --force",
            "echo git push --force",
            "git log -f",
            "git push --{tags,verbose}",
        ]);
    }

    #[test]
    fn hard_resets_and_forced_cleans_are_caught() {
        assert_hits(
            "git-reset-hard",
            &[
                "git reset --hard",
                "git reset --hard HEAD~1",
                "git -c a=b reset --hard",
            ],
        );
        assert_passes(&["git reset --soft HEAD~1", "git reset", "git reset --mixed"]);
        assert_hits(
            "git-clean-force",
            &[
                "git clean -fdx",
                "git clean -f",
                "git clean -xdf",
                "git clean --force -d",
            ],
        );
        assert_passes(&[
            "git clean -n",
            "git clean -nfd",
            "git clean --dry-run -f",
            "git clean",
        ]);
    }

    #[test]
    fn recursive_deletes_count_only_outside_the_project() {
        assert_hits(
            "rm-outside-project",
            &[
                "rm -rf /",
                "rm -rf /*",
                "rm -rf ~",
                "rm -rf ~/",
                "rm -rf $HOME",
                "rm -rf \"$HOME/projects\"",
                "rm -fr ../other-project",
                "rm -r -f /etc",
                "rm --recursive --force /var/lib",
                "rm -rf .",
                "rm -rf .git",
                "rm -rf ./.git/objects",
                "rm -rf \"$BUILD_DIR\"",
                "rm -rf $(pwd)",
                "rm -rf node_modules /tmp/x",
                "rm -rf ~other",
                "cd / && rm -rf home",
                "cd .. ; rm -rf app",
                // The working folder is not known after these.
                "cd /tmp && cd /home/me/app && cd - && rm -rf build",
                "cd && rm -rf build",
                "pushd /tmp; popd; rm -rf build",
            ],
        );
        assert_passes(&[
            "rm -rf ./node_modules",
            "rm -rf node_modules dist",
            "rm -rf /home/me/app/target",
            "rm -rf ./*",
            "rm -rf src/../build",
            "rm -f /tmp/one-file",
            "rm /etc/hosts.bak",
            "cd src && rm -rf generated",
            "rmdir /tmp/x",
        ]);
    }

    #[test]
    fn powershell_words_are_no_group_of_short_options() {
        // PowerShell's `rm` is Remove-Item: -Force and -Verbose hold an "r" but mean no recursion.
        for command in [r"rm -Force C:\Users\me\x.txt", r"rm -Verbose C:\temp\a.log"] {
            assert_eq!(
                hits_in(command, "PowerShell"),
                Vec::<String>::new(),
                "{command}"
            );
        }
        assert_eq!(
            hits_in(r"rm -Recurse -Force C:\Users\me", "PowerShell"),
            ["rm-outside-project"]
        );
        // A real group of short options still counts.
        assert_hits("rm-outside-project", &["rm -Rf /etc", "rm -rfv /etc"]);
    }

    #[test]
    fn powershell_and_cmd_deletes_count_only_outside_the_project() {
        for command in [
            r"Remove-Item -Recurse -Force C:\",
            r"Remove-Item C:\Users\me -Recurse",
            r"remove-item -r -fo $env:USERPROFILE",
            r"ri -Recurse D:\data",
            r"Remove-Item -Path \\server\share\x -Recurse",
            r"Remove-Item -Recurse:$true ~",
        ] {
            assert_eq!(
                hits_in(command, "PowerShell"),
                ["remove-item-outside-project"],
                "{command}"
            );
        }
        for command in [
            r"cmd /c del /s /q C:\Windows\Temp",
            r"cmd /c rd /s /q %USERPROFILE%",
            r"cmd /c rmdir /S /Q D:\",
            r"cmd /c del /s/q ..\..\x",
        ] {
            assert_eq!(
                hits_in(command, "PowerShell"),
                ["del-outside-project"],
                "{command}"
            );
        }
        for command in [
            r"Remove-Item -Recurse -Force .\node_modules",
            r"Remove-Item -Recurse C:\code\app\dist",
            r"Remove-Item -Recurse:$false C:\",
            r"Remove-Item C:\temp\one.txt",
            r"cmd /c del /s /q build",
            r"cmd /c del /q C:\temp\x.txt",
            r"cmd /c rd /s /q C:\Code\App\out",
        ] {
            assert_eq!(
                hits_in(command, "PowerShell"),
                Vec::<String>::new(),
                "{command}"
            );
        }
        // Git Bash and WSL spell drives as folders.
        for command in ["rm -rf /c/Users/me", "rm -rf /mnt/d/backup"] {
            assert_eq!(
                hits_in(command, "Bash"),
                ["rm-outside-project"],
                "{command}"
            );
        }
        assert_eq!(
            hits_in("rm -rf /c/code/app/target", "Bash"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn each_dialect_reads_its_own_quotes_and_escapes() {
        // A quoted path with a trailing backslash ends the string in PowerShell, so the delete
        // behind it is seen.
        let trailing =
            r#"Get-ChildItem "C:\code\app\"; Remove-Item -Recurse C:\Users\me\x; echo "done""#;
        assert_eq!(
            hits_in(trailing, "PowerShell"),
            ["remove-item-outside-project"]
        );
        assert!(hits_in(&format!("pwsh -Command '{trailing}'"), "Bash")
            .contains(&"remove-item-outside-project".to_owned()));
        // In bash the same quote stays open: too complex, asked about.
        assert!(hits(trailing).contains(&RULE_TOO_COMPLEX.to_owned()));
        // A backtick inside an option is PowerShell's escape.
        assert_eq!(
            hits_in("git push --for`ce", "PowerShell"),
            ["git-push-force"]
        );
        // cmd removes carets; `/r` runs like `/c`; pwsh's -cwa takes a script.
        for command in [
            "cmd /c git push --for^ce",
            "cmd /r git push --force",
            "pwsh -cwa 'git push --force'",
            "pwsh -CommandWithArgs 'git push --force' x",
        ] {
            assert!(
                hits(command).contains(&"git-push-force".to_owned()),
                "{command}"
            );
        }
        // An extension on Windows sends `Shell`: every dialect is read and every hit counts.
        let both = hits_in("git push --for`ce; git reset --hard", "Shell");
        assert!(both.contains(&"git-push-force".to_owned()), "{both:?}");
        assert!(both.contains(&"git-reset-hard".to_owned()), "{both:?}");
    }

    #[test]
    fn every_matching_rule_is_reported_once() {
        let found = check(
            &defaults(),
            "git reset --hard && rm -rf ~ && git reset --hard HEAD~1",
            &[Dialect::Posix],
            &unix_place(),
            UNIX,
        );
        assert_eq!(ids(&found), ["git-reset-hard", "rm-outside-project"]);
        assert_eq!(found[0].call, "git reset --hard");
        assert_eq!(found[1].call, "rm -rf ~");
    }

    #[test]
    fn what_cannot_be_read_is_asked_about() {
        let mut deep = "git push --force".to_owned();
        for _ in 0..7 {
            deep = format!("sh -c {}", join(&[deep]));
        }
        assert_eq!(hits(&deep), [RULE_TOO_COMPLEX]);
        let many = format!("{}git push --force", "true; ".repeat(512));
        assert_eq!(hits(&many), [RULE_TOO_COMPLEX]);
        for command in [
            "bash <<< 'git push --force'",
            "echo 'git push --force' | sh",
            "pwsh -Command -",
        ] {
            assert!(
                hits(command).contains(&RULE_STDIN_SCRIPT.to_owned()),
                "{command}"
            );
        }
        assert_eq!(hits("echo 'open"), [RULE_TOO_COMPLEX]);
    }

    #[test]
    fn hangars_own_settings_are_always_asked_about() {
        for command in [
            "cat ~/.config/com.example.hangar/command-guard.toml",
            "echo x > /home/me/.config/com.example.hangar/other.toml",
            "rm ../gate-rules.json",
            "cp x ../../.config/COM.EXAMPLE.HANGAR/profiles.toml",
            "Set-Content $HOME/x/Command-Guard.TOML x",
            "cp evil.toml \"$XDG_CONFIG_HOME\"/*/command-guard.toml",
        ] {
            assert!(
                hits(command).contains(&RULE_HANGAR_CONFIG.to_owned()),
                "{command}: {:?}",
                hits(command)
            );
        }
        // The project's own files of that name (Hangar's sources) are not Hangar's settings.
        assert_passes(&[
            "cat README.md",
            "ls ~/.config",
            "rg gate-rules.json src",
            "cat docs/command-guard.toml",
        ]);
        let place = unix_place();
        assert!(edit_touches_hangar(
            "/home/me/.config/com.example.hangar/command-guard.toml",
            &place,
            UNIX
        ));
        assert!(edit_touches_hangar("../x/gate-rules.json", &place, UNIX));
        assert!(!edit_touches_hangar("src/main.rs", &place, UNIX));
        assert!(!edit_touches_hangar(
            "src-tauri/gate-rules.json",
            &place,
            UNIX
        ));
        assert!(!rememberable(RULE_HANGAR_CONFIG));
        assert!(rememberable("git-push-force"));
    }

    #[test]
    fn hangars_own_worktrees_are_ordinary_projects() {
        let data = "/home/me/.local/share/com.example.hangar";
        let place = Place {
            cwd: Some(format!("{data}/worktrees/x")),
            project: Some(format!("{data}/worktrees/x")),
            ..unix_place()
        };
        let found = |command: &str| {
            check(&defaults(), command, &[Dialect::Posix], &place, UNIX)
                .into_iter()
                .map(|hit| hit.rule_id)
                .collect::<Vec<_>>()
        };
        for command in [
            "git status",
            "ls src",
            "cat src/main.rs",
            "cargo test > out.log",
        ] {
            assert_eq!(found(command), Vec::<String>::new(), "{command}");
        }
        assert!(!edit_touches_hangar(
            &format!("{data}/worktrees/x/src/main.rs"),
            &place,
            UNIX
        ));
        assert!(!edit_touches_hangar("src/main.rs", &place, UNIX));
        // The gate's own state and the remembered answers stay protected.
        for command in [
            format!("rm {data}/gate-rules.json"),
            "rm -f ../../gate-rules.json".to_owned(),
            format!("ls {data}/gate/sessions"),
            "echo {} > /home/me/.config/com.example.hangar/command-guard.toml".to_owned(),
        ] {
            assert!(
                found(&command).contains(&RULE_HANGAR_CONFIG.to_owned()),
                "{command}"
            );
        }
        assert!(edit_touches_hangar(
            "/home/me/.config/com.example.hangar/command-guard.toml",
            &place,
            UNIX
        ));
    }

    #[test]
    fn redirections_wildcards_and_short_names_reach_the_settings() {
        for command in [
            "echo x > ~/.config/com.example.hangar/command-guard.toml",
            "cd ~/.config/com.example.hangar && echo x >> command-guard.toml",
            "cp x ~/.config/com.f*/command-g*.toml",
            "cp x $XDG_CONFIG_HOME/com.example.h?ngar/x",
        ] {
            assert!(
                hits(command).contains(&RULE_HANGAR_CONFIG.to_owned()),
                "{command}"
            );
        }
        for command in [
            r"copy x C:\Users\me\AppData\Roaming\COMEXA~1\command-guard.toml",
            r"Set-Content C:\Users\me\AppData\Roaming\com.example.hangar\COMMAN~1.TOM x",
        ] {
            assert!(
                hits_in(command, "PowerShell").contains(&RULE_HANGAR_CONFIG.to_owned()),
                "{command}"
            );
        }
        assert_passes(&["echo x > notes.txt", "ls ~/*", "cp x ~/.config/other/x"]);
    }

    #[test]
    fn common_wildcards_outside_the_project_are_not_hangars_settings() {
        assert_passes(&[
            "ls /tmp/*.json",
            "cat ../other/*.toml",
            "ls ~/Library/com.*",
            "ls ~/.config/other/*.json",
        ]);
        for command in [
            "rm ../x/command-g*.toml",
            "rm /tmp/[c]ommand-guard.toml",
            "ls ~/.config/c*",
            "ls ~/.config/com.ex[a-z]mple.hangar",
            "cp x \"$HOME/Library/Application Support/c*\"",
        ] {
            assert!(
                hits(command).contains(&RULE_HANGAR_CONFIG.to_owned()),
                "{command}"
            );
        }
        for command in [
            r"Clear-Content $env:APPDATA\c*\[c]ommand-guard.toml",
            r"Remove-Item -Recurse $env:APPDATA\c*",
            r"Remove-Item -Recurse %LOCALAPPDATA%\c?m*",
        ] {
            assert!(
                hits_in(command, "PowerShell").contains(&RULE_HANGAR_CONFIG.to_owned()),
                "{command}"
            );
        }
        assert!(glob_match("[c]ommand", "command"));
        assert!(glob_match("[!x]y", "ay"));
        assert!(!glob_match("[!a]y", "ay"));
        assert!(glob_match("[a-c]?", "bz"));
        assert!(glob_match("[a", "[a"));
        assert_eq!(own_letters("*.json"), 0);
        assert_eq!(own_letters("command-g*.toml"), 9);
        assert_eq!(own_letters("com.*"), 4);
        assert_eq!(own_letters("[c]ommand-guard.toml"), 12);
    }

    #[test]
    fn a_literal_in_parentheses_is_placed_like_a_word() {
        assert_eq!(
            hits_in(r#"Remove-Item -Recurse (".\build")"#, "PowerShell"),
            Vec::<String>::new()
        );
        assert_eq!(
            hits_in(r#"Remove-Item -Recurse ("C:\Users\me\x")"#, "PowerShell"),
            ["remove-item-outside-project"]
        );
    }

    #[test]
    fn empty_special_parameters_do_not_hide_a_rule() {
        assert_hits(
            "git-push-force",
            &[
                "g$@it push --force",
                "git push --for$@ce",
                "git push --for${x}ce",
                "git push --f$*orce",
            ],
        );
    }

    #[test]
    fn a_huge_brace_expansion_does_not_exhaust_a_small_stack() {
        let command = format!("git push --{}", "{a,b}".repeat(12_000));
        assert!(command.len() < MAX_COMMAND_BYTES);
        let found = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || hits(&command))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(found, ["git-push-force"]);
    }

    #[test]
    fn powershell_parenthesized_and_wsl_arguments_are_read() {
        for command in [
            r#"Remove-Item -Recurse ("C:\Users\me\x")"#,
            "Remove-Item -Recurse (Join-Path $HOME x)",
        ] {
            assert_eq!(
                hits_in(command, "PowerShell"),
                ["remove-item-outside-project"],
                "{command}"
            );
        }
        assert_hits(
            "git-push-force",
            &[
                "wsl 'git push --force'",
                "bash -o pipefail -c 'git push --force'",
            ],
        );
    }

    #[test]
    fn built_in_wording_is_told_apart_from_the_users() {
        let push = &defaults()[0];
        assert!(is_builtin(&push.id, &push.description));
        assert!(!is_builtin(&push.id, "my own words"));
        assert!(!is_builtin("my-rule", &push.description));
        assert!(is_builtin(RULE_TOO_COMPLEX, ""));
        assert!(builtin_description(RULE_HANGAR_CONFIG).is_some());
    }

    #[test]
    fn sql_drops_through_database_clients_are_caught() {
        assert_hits(
            "sql-drop",
            &[
                "mysql -u root -e 'DROP DATABASE shop'",
                "mariadb --execute=\"drop table users\"",
                "psql -c 'TRUNCATE orders' mydb",
                "psql --command \"Drop Schema public cascade\"",
                "sqlite3 app.db 'DROP TABLE t'",
                "sqlcmd -Q \"DROP DATABASE x\"",
            ],
        );
        assert_passes(&[
            "psql -c 'SELECT 1'",
            "mysql -e 'SHOW TABLES'",
            "echo 'DROP TABLE x'",
            "sqlite3 drop_table.db .tables",
            "psql -c 'DROP INDEX i'",
        ]);
    }

    #[test]
    fn disk_tools_are_caught() {
        assert_hits(
            "format-disk",
            &[
                "format D: /q",
                "diskpart -s x",
                "mkfs.ext4 /dev/sdb1",
                "sudo mkfs -t ext4 /dev/sdb",
            ],
        );
        assert_eq!(
            hits_in("Format-Volume -DriveLetter D", "PowerShell"),
            ["format-disk"]
        );
        assert_passes(&["pnpm format", "cargo fmt", "git format-patch -1"]);
    }

    #[test]
    fn wrappers_and_chains_are_looked_through() {
        assert_hits(
            "git-push-force",
            &[
                "sudo git push --force",
                "sudo -u deploy git push -f",
                "env GIT_TRACE=1 git push --force",
                "env -i PATH=/bin git push --force",
                "GIT_TRACE=1 git push --force",
                "sh -c 'git push --force'",
                "bash -lc \"git push --force\"",
                "bash -c \"cd /tmp && git push -f\"",
                "pwsh -Command git push --force",
                "pwsh -NoProfile -c \"git push --force\"",
                "powershell.exe -ExecutionPolicy Bypass -Command \"git push -f\"",
                "powershell git push --force",
                "cmd /c git push --force",
                "cmd.exe /d /s /c \"git push --force\"",
                "npm test && git push --force",
                "git fetch; git push --force",
                "false || git push --force",
                "cat x | git push --force",
                "echo start &\ngit push --force",
                "nohup git push --force > log 2>&1 &",
                "time nice -n 5 git push --force",
                "timeout 60 git push --force",
                "eval 'git push --force'",
                "Invoke-Expression 'git push --force'",
                "iex \"git push -f\"",
                "wsl -e git push --force",
                "wsl -d Ubuntu -- git push --force",
                "echo $(git push --force)",
                "echo \"`git push --force`\"",
                "(git push --force)",
                "{ git push --force; }",
                "if true; then git push --force; fi",
                "git push \\\n  --force",
                "sh -c \"bash -c 'git push --force'\"",
                "pwsh -EncodedCommand ZwBpAHQAIABwAHUAcwBoACAALQAtAGYAbwByAGMAZQA=",
            ],
        );
        assert_passes(&[
            "echo 'git push --force'",
            "# git push --force",
            "git commit -m 'git push --force later'",
            "bash script.sh --force",
            "pwsh -File push.ps1 -Force",
        ]);
    }

    #[test]
    fn an_invalid_rule_is_reported_and_the_others_still_work() {
        let source = r#"
version = 1
colour = "red"

[[rule]]
id = "push"
description = "force push"
program = ["git"]
subcommand = ["push"]
any = ["--force"]

[[rule]]
id = "typo"
description = "a field with a typo"
program = ["git"]
subcomand = ["reset"]

[[rule]]
id = "Bad Id"
description = "x"
program = ["git"]

[[rule]]
id = "flags"
description = "a pattern that is no option"
program = ["rm"]
any = ["force"]

[[rule]]
id = "push"
description = "the same id again"
program = ["git"]

[[rule]]
id = "folder"
description = "a program with a folder"
program = ["/bin/rm"]

[[rule]]
description = "no id"
program = ["x"]

[[rule]]
id = "hangar-mine"
description = "a reserved id"
program = ["x"]
"#;
        let set = parse_rules(source);
        assert!(!set.defaults);
        let ids: Vec<&str> = set.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["push"]);
        let problems: Vec<(Option<usize>, Option<&str>)> = set
            .problems
            .iter()
            .map(|p| (p.rule, p.id.as_deref()))
            .collect();
        assert_eq!(
            problems,
            [
                (None, None),
                (Some(2), Some("typo")),
                (Some(3), Some("Bad Id")),
                (Some(4), Some("flags")),
                (Some(5), Some("push")),
                (Some(6), Some("folder")),
                (Some(7), None),
                (Some(8), Some("hangar-mine")),
            ],
            "{:?}",
            set.problems
        );
        assert!(set.problems[0].message.contains("colour"));
        assert!(
            set.problems[1].message.contains("subcomand"),
            "{:?}",
            set.problems[1]
        );
        assert!(set.problems[4].message.contains("earlier"));
        assert!(set.problems[7].message.contains("reserved"));
        let place = unix_place();
        assert_eq!(
            ids_of(check(
                &set.rules,
                "git push --force",
                &[Dialect::Posix],
                &place,
                UNIX
            )),
            ["push"]
        );
        assert!(check(
            &set.rules,
            "git reset --hard",
            &[Dialect::Posix],
            &place,
            UNIX
        )
        .is_empty());
    }

    fn ids_of(hits: Vec<Hit>) -> Vec<String> {
        hits.into_iter().map(|hit| hit.rule_id).collect()
    }

    #[test]
    fn a_broken_file_keeps_the_default_rules() {
        let set = parse_rules("[[rule]\nid = 'x'\n");
        assert!(set.defaults);
        assert_eq!(set.rules, default_rules().rules);
        assert_eq!(set.problems.len(), 1);
        assert_eq!(set.problems[0].line, Some(1), "{:?}", set.problems);
        assert!(!set.problems[0].message.contains('\n'));
        assert!(parse_rules("").rules.is_empty());
        assert!(parse_rules("# all rules removed\nversion = 1\n")
            .problems
            .is_empty());
    }

    #[test]
    fn flags_compare_as_their_style_says() {
        let flag = |text: &str| Flag::parse(text).unwrap();
        assert!(flag("--force").given_by("--force"));
        assert!(flag("--force").given_by("--force=yes"));
        assert!(!flag("--force").given_by("--force-with-lease"));
        assert!(flag("-f").given_by("-xfd"));
        assert!(!flag("-f").given_by("-F"));
        assert!(!flag("-f").given_by("--fix"));
        assert!(!flag("-r").given_by("-Force"));
        assert!(!flag("-r").given_by("-Verbose"));
        assert!(flag("-R").given_by("-Rf"));
        assert!(flag("-Recurse").given_by("-r"));
        assert!(flag("-Recurse").given_by("-RECURSE"));
        assert!(flag("-Recurse").given_by("-Recurse:$true"));
        assert!(!flag("-Recurse").given_by("-Recurse:$false"));
        assert!(!flag("-Recurse").given_by("-Force"));
        assert!(flag("/s").given_by("/S"));
        assert!(flag("/s").given_by("/q/s"));
        assert!(!flag("/s").given_by("/sx"));
        assert!(flag("--force").given_by_word("--{force,x}"));
        assert!(flag("--force").given_by_word("--f{o..z}"));
        assert!(!flag("--force").given_by_word("--{tags,x}"));
        for bad in ["force", "-", "--", "-a b", "/toolong", ""] {
            assert_eq!(Flag::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn paths_are_placed_without_touching_the_disk() {
        let key = |arg: &str, style: Style, cwd: &str| {
            place(
                arg,
                Some(cwd),
                Some(if style.windows {
                    "c:/users/me"
                } else {
                    "/home/me"
                }),
                style,
            )
            .map(|t| t.key)
        };
        assert_eq!(key("a/../b", UNIX, "/x").as_deref(), Some("/x/b"));
        assert_eq!(key("../../../..", UNIX, "/x").as_deref(), Some("/"));
        assert_eq!(key("~/x", UNIX, "/").as_deref(), Some("/home/me/x"));
        assert_eq!(key("$VAR/x", UNIX, "/"), None);
        assert_eq!(key("~bob", UNIX, "/"), None);
        assert_eq!(
            key(r"D:\Data\", WINDOWS, "c:/x").as_deref(),
            Some("d:/data")
        );
        assert_eq!(key(r"C:\", WINDOWS, "c:/x").as_deref(), Some("c:"));
        assert_eq!(key("c:relative", WINDOWS, "c:/x"), None);
        assert_eq!(
            key(r"\\host\share\x", WINDOWS, "c:/x").as_deref(),
            Some("//host/share/x")
        );
        assert_eq!(key("/c/Code", WINDOWS, "c:/x").as_deref(), Some("c:/code"));
        assert_eq!(key("%TEMP%", WINDOWS, "c:/x"), None);
        assert_eq!(
            key("%USERPROFILE%", WINDOWS, "c:/x").as_deref(),
            Some("c:/users/me")
        );
        assert_eq!(
            key("sub", WINDOWS, "C:/Code/App").as_deref(),
            Some("c:/code/app/sub")
        );
        assert_eq!(place("x", None, None, UNIX), None);
    }

    #[test]
    fn without_a_project_every_delete_counts_as_outside() {
        let place = Place {
            cwd: Some("/w".into()),
            ..Place::default()
        };
        assert!(!check(&defaults(), "rm -rf build", &[Dialect::Posix], &place, UNIX).is_empty());
    }

    #[test]
    fn the_shown_command_keeps_lines_and_marks_controls() {
        assert_eq!(shown("a\n\tb", 100), "a\n\tb");
        assert_eq!(shown("a\u{1b}[2Jb", 100), "a<U+001B>[2Jb");
        assert_eq!(shown("a\u{202e}b", 100), "a<U+202E>b");
        assert_eq!(shown("abcdef", 3), "abc…");
        assert_eq!(shown_counted("abcdef", 3), ("abc".to_owned(), 3));
        assert_eq!(cut("a\u{1b}bcd", 3), ("a\u{1b}b".to_owned(), 2));
    }

    #[test]
    fn program_names_lose_folder_case_and_launcher_suffix() {
        assert_eq!(program_name(r"C:\Git\bin\GIT.EXE"), "git");
        assert_eq!(program_name("/usr/bin/mkfs.ext4"), "mkfs.ext4");
        assert_eq!(program_name("npm.cmd"), "npm");
        assert_eq!(program_name(".exe"), ".exe");
        assert!(glob_match("mkfs.*", "mkfs.ext4"));
        assert!(!glob_match("mkfs.*", "mkfs"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "aXbYc"));
        assert!(!glob_match("a*b", "aXbY"));
    }
}
