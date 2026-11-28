//! Hangar's own status line for agent CLIs that run a command for theirs (ADR 0023): Claude Code
//! (`statusLine` in `--settings`) and Qwen Code (`ui.statusLine` in the session's system
//! settings) run `hangar status-line <claude|qwen>`, which reads the program's JSON on stdin and
//! prints two lines: where the session works (model, account, folder, branch, repository, pull
//! request) and how it is doing (context, cache, changed lines, cost, time, limits), in the look
//! of the owner's own Claude Code status line: emoji labels, `|` between segments. Codex CLI
//! has no command line of its own; it gets its built-in items in Hangar's order
//! ([`codex_args`]).
//!
//! The renderer never fails loudly: input that is not a JSON object prints nothing, a segment
//! without data is left out, and every text from the input or from git is cleaned of control
//! and direction characters before it reaches the terminal. When the program passes the
//! terminal width in `COLUMNS`, the least important segments go first until a line fits.
//!
//! Data that is not in the input comes from `git status` (through [`crate::git::Git`]: no hooks,
//! no filters, no network, a time limit) and, for Claude Code, the display name and plan of the
//! signed-in account from `~/.claude.json` (name, e-mail and plan, never any key). Both are cached per
//! session for [`CACHE_MAX_AGE`] in the folder named by [`CACHE_ENV`], which Hangar sets to a
//! folder of its own; files older than [`CACHE_PRUNE`] are removed. Nothing is ever logged.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// First argument of the Hangar executable that runs the renderer instead of the app.
pub const SUBCOMMAND: &str = "status-line";
/// The folder for the per-session cache, set by Hangar in the agent's environment.
pub const CACHE_ENV: &str = "HANGAR_STATUS_DIR";
/// Name of that folder inside Hangar's local data folder.
pub const CACHE_DIR: &str = "status-line";
/// How often the program runs the command without an event, so countdowns stay current.
pub const REFRESH_SECS: u64 = 30;
/// How long one git reading is reused.
pub const CACHE_MAX_AGE: Duration = Duration::from_secs(5);
/// Cache files untouched for this long belong to sessions that ended.
pub const CACHE_PRUNE: Duration = Duration::from_secs(24 * 60 * 60);
/// Context share in percent from which `/compact` is suggested.
pub const COMPACT_AT: f64 = 80.0;
/// Codex CLI's built-in status items in Hangar's order (ids from codex-rs 0.172.0,
/// `tui/src/bottom_pane/status_line_setup.rs`): where first, then how.
pub const CODEX_ITEMS: &[&str] = &[
    "model-with-reasoning",
    "current-dir",
    "git-branch",
    "branch-changes",
    "pull-request-number",
    "context-used",
    "used-tokens",
    "five-hour-limit",
    "weekly-limit",
];
/// Codex's config key for its status items.
pub const CODEX_KEY: &str = "tui.status_line";

/// Status input larger than this is not a status line's.
const MAX_INPUT: u64 = 1024 * 1024;
/// How long the renderer waits for its input; the programs close stdin at once.
const INPUT_WAIT: Duration = Duration::from_secs(2);
/// All git calls of one reading together; Qwen Code stops the command after 5 s.
const GIT_BUDGET: Duration = Duration::from_secs(2);
/// `git status` output kept at most.
const MAX_STATUS_BYTES: usize = 4 * 1024 * 1024;
/// `~/.claude.json` read at most; it also holds the project history.
const MAX_ACCOUNT_BYTES: usize = 32 * 1024 * 1024;
/// A cache file read at most.
const MAX_CACHE_BYTES: usize = 64 * 1024;
/// The cost rate is noise in the first minutes.
const RATE_AFTER_MS: u64 = 10 * 60 * 1000;
/// How long a prompt cache miss and its cause stay visible, in seconds.
const MISS_SHOW_FOR: f64 = 600.0;
/// Miss causes kept at most.
const MAX_MISS_CAUSES: usize = 8;
/// Columns Claude Code and Qwen Code keep around the line.
const EDGE_COLUMNS: usize = 4;

const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";
const GREY: &str = "\x1b[90m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BLINK: &str = "\x1b[5m";
const NOBLINK: &str = "\x1b[25m";
const RESET: &str = "\x1b[0m";

/// Which program's input the renderer reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Claude,
    Qwen,
}

impl Source {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Source::Claude),
            "qwen" => Some(Source::Qwen),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Source::Claude => "claude",
            Source::Qwen => "qwen",
        }
    }
}

/// A pull request as Claude Code names it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub url: Option<String>,
    pub review_state: Option<String>,
    /// `mr` for a GitLab merge request.
    pub merge_request: bool,
}

/// One usage limit window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limit {
    pub used: f64,
    /// Unix time in seconds.
    pub resets_at: Option<f64>,
}

/// The prompt cache: hit ratio from 0 to 1, whether it is warm and until when.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cache {
    pub ratio: Option<f64>,
    pub warm: bool,
    pub expires_at: Option<f64>,
    /// Unix time in seconds of the last miss, and Claude Code's names for its causes.
    pub miss_at: Option<f64>,
    pub miss_causes: Vec<String>,
}

/// Everything one rendering shows, normalized from either program's input. `None` or zero
/// leaves the segment out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    pub model: Option<String>,
    pub vim: Option<String>,
    pub agent: Option<String>,
    pub effort: Option<String>,
    pub fast: bool,
    pub version: Option<String>,
    pub account: Option<Account>,
    pub session_name: Option<String>,
    pub dir: Option<String>,
    pub worktree: Option<String>,
    pub git: Option<GitState>,
    /// `https://host/owner/name` and the name.
    pub repo: Option<(String, String)>,
    pub pr: Option<PullRequest>,
    pub context_percent: Option<f64>,
    pub context_used: Option<u64>,
    pub context_size: Option<u64>,
    pub cache: Option<Cache>,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub api_ms: Option<u64>,
    pub tokens: Option<u64>,
    pub five_hour: Option<Limit>,
    pub seven_day: Option<Limit>,
    pub spend: Option<Limit>,
}

/// The signed-in account's display name, e-mail and plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    pub plan: Option<String>,
}

/// One `git status` reading of a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitState {
    pub branch: Option<String>,
    /// Short commit id of a detached head.
    pub oid: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
    pub conflicts: u32,
}

// ---------------------------------------------------------------------------------------------
// Input

fn at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, key| value.get(key))
}

fn text_at(value: &Value, path: &[&str]) -> Option<String> {
    at(value, path)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
}

/// A finite, non-negative number.
fn number_at(value: &Value, path: &[&str]) -> Option<f64> {
    at(value, path)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
}

fn count_at(value: &Value, path: &[&str]) -> Option<u64> {
    // Saturating float to integer conversion; the value is finite and non-negative.
    number_at(value, path).map(|n| n as u64)
}

fn limit_at(value: &Value, path: &[&str]) -> Option<Limit> {
    let window = at(value, path)?;
    Some(Limit {
        used: number_at(window, &["used_percentage"])?,
        resets_at: number_at(window, &["resets_at"]),
    })
}

/// The facts the program's own input carries.
pub fn facts_from(source: Source, input: &Value) -> Facts {
    let mut facts = Facts {
        model: text_at(input, &["model", "display_name"]),
        vim: text_at(input, &["vim", "mode"]),
        version: text_at(input, &["version"]),
        dir: text_at(input, &["workspace", "current_dir"]).or_else(|| text_at(input, &["cwd"])),
        worktree: text_at(input, &["worktree", "name"]),
        context_percent: number_at(input, &["context_window", "used_percentage"]),
        context_used: count_at(input, &["context_window", "total_input_tokens"]),
        context_size: count_at(input, &["context_window", "context_window_size"]),
        ..Facts::default()
    };
    match source {
        Source::Claude => claude_facts(input, &mut facts),
        Source::Qwen => qwen_facts(input, &mut facts),
    }
    facts
}

fn claude_facts(input: &Value, facts: &mut Facts) {
    // Model, context, cost and time always show, as in the owner's line.
    if facts.model.is_none() {
        facts.model = Some("Claude".to_owned());
    }
    if facts.context_percent.is_none() {
        facts.context_percent = Some(0.0);
    }
    facts.agent = text_at(input, &["agent", "name"]);
    facts.effort = text_at(input, &["effort", "level"]);
    facts.fast = at(input, &["fast_mode"]).and_then(Value::as_bool) == Some(true);
    facts.session_name = text_at(input, &["session_name"]);
    if facts.worktree.is_none() {
        facts.worktree = text_at(input, &["workspace", "git_worktree"]);
    }
    let part = |key: &str| text_at(input, &["workspace", "repo", key]).filter(|t| url_part(t));
    if let (Some(host), Some(owner), Some(name)) = (part("host"), part("owner"), part("name")) {
        facts.repo = Some((format!("https://{host}/{owner}/{name}"), name));
    }
    if let Some(number) = count_at(input, &["pr", "number"]).filter(|n| *n > 0) {
        facts.pr = Some(PullRequest {
            number,
            url: text_at(input, &["pr", "url"]).filter(|url| safe_url(url)),
            review_state: text_at(input, &["pr", "review_state"]),
            merge_request: text_at(input, &["pr", "kind"]).as_deref() == Some("mr"),
        });
    }
    if at(input, &["prompt_cache", "caching_observed"]).and_then(Value::as_bool) == Some(true) {
        facts.cache = Some(Cache {
            ratio: number_at(input, &["prompt_cache", "hit_ratio"]).map(|r| r.min(1.0)),
            warm: at(input, &["prompt_cache", "warm"]).and_then(Value::as_bool) == Some(true),
            expires_at: number_at(input, &["prompt_cache", "expires_at"]),
            miss_at: number_at(input, &["prompt_cache", "last_miss_at"]),
            miss_causes: at(input, &["prompt_cache", "last_miss_cause", "causes"])
                .and_then(Value::as_array)
                .map(|causes| {
                    causes
                        .iter()
                        .filter_map(Value::as_str)
                        .take(MAX_MISS_CAUSES)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    facts.lines_added = count_at(input, &["cost", "total_lines_added"]).unwrap_or(0);
    facts.lines_removed = count_at(input, &["cost", "total_lines_removed"]).unwrap_or(0);
    facts.cost_usd = number_at(input, &["cost", "total_cost_usd"]).or(Some(0.0));
    facts.duration_ms = count_at(input, &["cost", "total_duration_ms"]).or(Some(0));
    facts.api_ms = count_at(input, &["cost", "total_api_duration_ms"]);
    facts.five_hour = limit_at(input, &["rate_limits", "five_hour"]);
    facts.seven_day = limit_at(input, &["rate_limits", "seven_day"]);
    facts.spend = limit_at(input, &["rate_limits", "spend_limit"]);
}

fn qwen_facts(input: &Value, facts: &mut Facts) {
    facts.lines_added = count_at(input, &["metrics", "files", "total_lines_added"]).unwrap_or(0);
    facts.lines_removed =
        count_at(input, &["metrics", "files", "total_lines_removed"]).unwrap_or(0);
    // Qwen names the branch itself; git's reading replaces it when there is one.
    if let Some(branch) = text_at(input, &["git", "branch"]) {
        facts.git = Some(GitState {
            branch: Some(branch),
            ..GitState::default()
        });
    }
    let Some(models) = at(input, &["metrics", "models"]).and_then(Value::as_object) else {
        return;
    };
    let sum = |key: &str| -> u64 {
        models
            .values()
            .filter_map(|model| count_at(model, &["tokens", key]))
            .fold(0u64, u64::saturating_add)
    };
    let (total, prompt, cached) = (sum("total"), sum("prompt"), sum("cached"));
    if total > 0 {
        facts.tokens = Some(total);
    }
    if prompt > 0 && cached > 0 {
        facts.cache = Some(Cache {
            ratio: Some((cached as f64 / prompt as f64).min(1.0)),
            warm: true,
            ..Cache::default()
        });
    }
}

/// A host, owner or repository name that can stand in a URL as it is.
fn url_part(text: &str) -> bool {
    text.len() <= 200
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// An `https` URL of printable ASCII without blanks: nothing that could end the link's escape.
fn safe_url(url: &str) -> bool {
    url.len() <= 500 && url.starts_with("https://") && url.chars().all(|c| c.is_ascii_graphic())
}

// ---------------------------------------------------------------------------------------------
// Git and account

/// `git status --porcelain=v2 --branch`: branch, ahead and behind, and the file counts.
pub fn parse_porcelain(output: &[u8]) -> GitState {
    let mut state = GitState::default();
    for line in String::from_utf8_lossy(output).lines() {
        let mut parts = line.splitn(3, ' ');
        let tag = parts.next().unwrap_or_default();
        match tag {
            "u" => state.conflicts = state.conflicts.saturating_add(1),
            "1" | "2" => {
                let xy = parts.next().unwrap_or_default().as_bytes();
                if xy.first().is_some_and(|x| *x != b'.') {
                    state.staged = state.staged.saturating_add(1);
                }
                if xy.get(1).is_some_and(|y| *y != b'.') {
                    state.modified = state.modified.saturating_add(1);
                }
            }
            "?" => state.untracked = state.untracked.saturating_add(1),
            "#" => {
                let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
                    continue;
                };
                match key {
                    "branch.head" if value != "(detached)" => {
                        state.branch = Some(value.to_owned());
                    }
                    "branch.oid" if value != "(initial)" => {
                        state.oid = Some(value.chars().take(7).collect());
                    }
                    "branch.ab" => {
                        let mut counts = value.split(' ');
                        let parse = |part: Option<&str>, sign: char| {
                            part.and_then(|p| p.strip_prefix(sign))
                                .and_then(|n| n.parse().ok())
                                .unwrap_or(0)
                        };
                        state.ahead = parse(counts.next(), '+');
                        state.behind = parse(counts.next(), '-');
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    state
}

/// One reading through Hangar's hardened git runner; `None` outside a repository or on any
/// failure.
fn read_git(dir: &Path) -> Option<GitState> {
    use crate::git;
    let runner = git::Git::locate().ok()?;
    git::check_folder(dir).ok()?;
    let deadline = Instant::now() + GIT_BUDGET;
    let root = git::toplevel(&runner, dir, deadline).ok()??;
    let neutral = git::filter_overrides(&runner, &root, deadline).ok()?;
    let output = runner
        .run(
            &root,
            neutral.iter().map(String::as_str).chain([
                "status",
                "--porcelain=v2",
                "--branch",
                "--untracked-files=normal",
                "--ignore-submodules=all",
            ]),
            deadline,
            MAX_STATUS_BYTES,
        )
        .ok()?;
    output.success.then(|| parse_porcelain(&output.stdout))
}

/// `default_claude_max_5x` gives `Max 5x`, `claude_pro` gives `Pro`.
fn plan_name(tier: &str) -> Option<String> {
    let (_, plan) = tier.split_once("claude_")?;
    let words: Vec<String> = plan
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) if word.chars().all(char::is_alphabetic) => {
                    first.to_uppercase().chain(chars).collect()
                }
                _ => word.to_owned(),
            }
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Display name, e-mail and plan from Claude Code's `oauthAccount`; no key, nothing else.
pub fn parse_account(bytes: &[u8]) -> Option<Account> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let account = value.get("oauthAccount")?;
    let email = text_at(account, &["emailAddress"]);
    let name = text_at(account, &["displayName"])
        .or_else(|| text_at(account, &["fullName"]))
        .or_else(|| {
            email
                .as_deref()
                .and_then(|email| email.split('@').next())
                .filter(|local| !local.trim().is_empty())
                .map(str::to_owned)
        })?;
    let plan = text_at(account, &["organizationRateLimitTier"])
        .or_else(|| text_at(account, &["organizationType"]))
        .and_then(|tier| plan_name(&tier));
    Some(Account { name, email, plan })
}

/// Claude Code's global file: `CLAUDE_CONFIG_DIR/.claude.json` or `~/.claude.json`.
fn account_file() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join(".claude.json"));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(PathBuf::from(home).join(".claude.json"))
}

fn read_account() -> Option<Account> {
    let bytes = crate::persist::read_limited(&account_file()?, MAX_ACCOUNT_BYTES).ok()??;
    parse_account(&bytes)
}

/// Reads git and the account for a folder; tests stand in for both.
type Reader<'a> = dyn Fn(&Path, Source) -> (Option<GitState>, Option<Account>) + 'a;

/// What one session's cache file holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Cached {
    dir: String,
    git: Option<GitState>,
    account: Option<Account>,
}

/// A file name from the session id: letters, digits, `-` and `_` only, bounded.
fn cache_name(session: &str) -> Option<String> {
    let name: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(80)
        .collect();
    (!name.is_empty()).then(|| format!("{name}.json"))
}

/// Removes cache files nobody touched for [`CACHE_PRUNE`].
pub fn prune(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > CACHE_PRUNE);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Git state and account for `facts`, from the cache while it is fresh. `read` stands for git
/// and the account file, so tests need neither.
fn enrich_with(
    facts: &mut Facts,
    source: Source,
    session: Option<&str>,
    cache_dir: Option<&Path>,
    read: &Reader<'_>,
) {
    let Some(dir) = facts.dir.clone().filter(|dir| Path::new(dir).is_absolute()) else {
        return;
    };
    let file = cache_dir
        .zip(session.and_then(cache_name))
        .map(|(folder, name)| folder.join(name));
    let fresh = file.as_ref().and_then(|file| {
        let age = std::fs::metadata(file)
            .ok()?
            .modified()
            .ok()?
            .elapsed()
            .ok()?;
        let bytes = crate::persist::read_limited(file, MAX_CACHE_BYTES).ok()??;
        let cached: Cached = serde_json::from_slice(&bytes).ok()?;
        (age <= CACHE_MAX_AGE && cached.dir == dir).then_some(cached)
    });
    let cached = match fresh {
        Some(cached) => cached,
        None => {
            if let Some(folder) = cache_dir {
                prune(folder, SystemTime::now());
            }
            let (git, account) = read(Path::new(&dir), source);
            let cached = Cached { dir, git, account };
            if let (Some(file), Ok(text)) = (&file, serde_json::to_vec(&cached)) {
                if file
                    .parent()
                    .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
                {
                    let _ = crate::persist::write_atomic(file, &text);
                }
            }
            cached
        }
    };
    if cached.git.is_some() {
        facts.git = cached.git;
    }
    facts.account = cached.account;
}

fn read_real(dir: &Path, source: Source) -> (Option<GitState>, Option<Account>) {
    let account = match source {
        Source::Claude => read_account(),
        Source::Qwen => None,
    };
    (read_git(dir), account)
}

// ---------------------------------------------------------------------------------------------
// Rendering

/// A character that must never reach the terminal from input: controls (C0, DEL, C1) and the
/// characters that reorder or hide text.
fn unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}')
        || c == '\u{FEFF}'
}

/// `text` with unsafe characters as `?`, cut to `max` characters with an ellipsis.
pub fn clean(text: &str, max: usize) -> String {
    let safe: Vec<char> = text
        .trim()
        .chars()
        .map(|c| if unsafe_char(c) { '?' } else { c })
        .collect();
    if safe.len() <= max {
        return safe.into_iter().collect();
    }
    let mut cut: String = safe.into_iter().take(max.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

/// Columns a character takes in a terminal: 0 for combining marks and joiners, 2 for wide East
/// Asian characters and emoji, 1 otherwise.
fn char_width(c: char) -> usize {
    let code = u32::from(c);
    match code {
        0x0300..=0x036F
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x200B..=0x200F
        | 0x20D0..=0x20FF
        | 0xFE00..=0xFE0E
        | 0xFE20..=0xFE2F => 0,
        // Emoji presentation widens the character before it.
        0xFE0F => 1,
        0x1100..=0x115F
        | 0x231A..=0x231B
        | 0x23E9..=0x23EC
        | 0x23F0
        | 0x23F3
        | 0x25FD..=0x25FE
        | 0x2614..=0x2615
        | 0x26A1
        | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE
        | 0x26C4..=0x26C5
        | 0x26D4
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x2753..=0x2755
        | 0x2757
        | 0x2795..=0x2797
        | 0x2B1B..=0x2B1C
        | 0x2B50
        | 0x2B55
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F900..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Visible width of a rendered text: SGR colors and OSC 8 links take no column.
pub fn display_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            width += char_width(c);
            continue;
        }
        match chars.next() {
            // CSI: up to the final byte.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: up to BEL or ST.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    width
}

fn link(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x07{text}\x1b]8;;\x07")
}

fn level_color(percent: f64) -> &'static str {
    if percent >= 90.0 {
        RED
    } else if percent >= 70.0 {
        YELLOW
    } else {
        GREEN
    }
}

/// `152340` gives `152k`, `1000000` gives `1M`.
pub fn short_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        let text = format!("{:.1}", n as f64 / 1_000_000.0);
        format!("{}M", text.trim_end_matches('0').trim_end_matches('.'))
    } else if n >= 1000 {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

/// Time until `epoch` as `3d4h`, `2h13m` or `12m`; empty once it passed.
pub fn until(epoch: f64, now: f64) -> Option<String> {
    let secs = (epoch - now).floor();
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    let secs = secs as u64;
    let (days, hours, mins) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    Some(if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else {
        format!("{mins}m")
    })
}

/// Segments with a priority (0 is the most important). A glued segment belongs to the one before
/// it: joined with a blank and dropped together with it.
#[derive(Default)]
struct Line {
    items: Vec<(u8, String, bool)>,
}

impl Line {
    fn add(&mut self, prio: u8, text: String) {
        self.items.push((prio, text, false));
    }

    fn glue(&mut self, prio: u8, text: String) {
        self.items.push((prio, text, true));
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (index, (_, text, glue)) in self.items.iter().enumerate() {
            if index > 0 {
                out.push_str(if *glue { " " } else { " | " });
            }
            out.push_str(text);
        }
        out
    }

    /// Drops the least important segments until the line fits `max` columns; a tie loses the
    /// rightmost one. One segment always stays.
    fn fit(mut self, max: usize) -> String {
        while self.items.len() > 1 && display_width(&self.render()) > max {
            let Some(index) = (0..self.items.len()).max_by_key(|i| (self.items[*i].0, *i)) else {
                break;
            };
            let mut end = index + 1;
            if !self.items[index].2 {
                while end < self.items.len() && self.items[end].2 {
                    end += 1;
                }
            }
            if index == 0 {
                if let Some(next) = self.items.get_mut(end) {
                    next.2 = false;
                }
            }
            self.items.drain(index..end);
        }
        self.render()
    }
}

fn effort_color(level: &str) -> &'static str {
    match level {
        "low" => GREY,
        "medium" => GREEN,
        "high" => YELLOW,
        "xhigh" => MAGENTA,
        "max" => RED,
        _ => GREY,
    }
}

fn where_line(facts: &Facts, links: bool) -> Line {
    let mut line = Line::default();
    if let Some(model) = &facts.model {
        line.add(0, format!("{CYAN}[{}]{RESET}", clean(model, 32)));
    }
    // Mode, agent, effort and version belong to the model; without one they lead the line.
    let glue = |line: &mut Line, prio: u8, text: String| {
        if line.items.is_empty() {
            line.add(prio, text);
        } else {
            line.glue(prio, text);
        }
    };
    if let Some(vim) = &facts.vim {
        let (color, short) = match vim.as_str() {
            "NORMAL" => (GREEN, "NOR".to_owned()),
            "INSERT" => (CYAN, "INS".to_owned()),
            "VISUAL" => (MAGENTA, "VIS".to_owned()),
            "VISUAL LINE" => (MAGENTA, "V-L".to_owned()),
            other => (GREY, clean(other, 8)),
        };
        glue(&mut line, 1, format!("{color}{short}{RESET}"));
    }
    if let Some(agent) = &facts.agent {
        glue(
            &mut line,
            2,
            format!("{MAGENTA}\u{1f916} {}{RESET}", clean(agent, 20)),
        );
    }
    if let Some(effort) = &facts.effort {
        let effort = clean(effort, 10);
        glue(
            &mut line,
            3,
            format!("{}{effort}{RESET}", effort_color(&effort)),
        );
    }
    if facts.fast {
        glue(&mut line, 4, format!("{MAGENTA}\u{21af}fast{RESET}"));
    }
    if let Some(version) = &facts.version {
        glue(
            &mut line,
            9,
            format!("{GREY}v{}{RESET}", clean(version, 16)),
        );
    }
    if let Some(account) = &facts.account {
        line.add(5, format!("\u{1f464} {}", clean(&account.name, 20)));
        if let Some(email) = &account.email {
            line.glue(5, format!("{GREY}<{}>{RESET}", clean(email, 40)));
        }
        if let Some(plan) = &account.plan {
            line.glue(8, format!("{GREY}{}{RESET}", clean(plan, 16)));
        }
    }
    if let Some(name) = &facts.session_name {
        line.add(6, format!("\u{1f3f7}\u{fe0f} {}", clean(name, 28)));
    }
    if let Some(dir) = &facts.dir {
        let trimmed = dir.trim_end_matches(['/', '\\']);
        let base = trimmed
            .rsplit(['/', '\\'])
            .next()
            .filter(|base| !base.is_empty())
            .unwrap_or(trimmed);
        let base = clean(base, 32);
        line.add(1, format!("\u{1f4c1} {base}"));
        if let Some(worktree) = facts.worktree.as_deref().map(|w| clean(w, 24)) {
            if worktree != base {
                line.glue(5, format!("{GREY}\u{2387} {worktree}{RESET}"));
            }
        }
    }
    if let Some(git) = &facts.git {
        let head = match (&git.branch, &git.oid) {
            (Some(branch), _) => Some(clean(branch, 40)),
            (None, Some(oid)) => Some(format!("{GREY}@{}{RESET}", clean(oid, 7))),
            (None, None) => None,
        };
        if let Some(head) = head {
            let mut text = format!("\u{1f33f} {head}");
            for (count, color, mark) in [
                (git.ahead, CYAN, '\u{2191}'),
                (git.behind, CYAN, '\u{2193}'),
                (git.staged, GREEN, '+'),
                (git.modified, YELLOW, '~'),
                (git.untracked, GREY, '?'),
                (git.conflicts, RED, '!'),
            ] {
                if count > 0 {
                    text.push_str(&format!(" {color}{mark}{count}{RESET}"));
                }
            }
            line.add(2, text);
        }
    }
    if let Some((url, name)) = &facts.repo {
        let text = format!("\u{1f517} {}", clean(name, 32));
        line.add(7, if links { link(url, &text) } else { text });
    }
    if let Some(pr) = &facts.pr {
        let (color, mark) = match pr.review_state.as_deref() {
            Some("approved") => (GREEN, " \u{2713}"),
            Some("changes_requested") => (RED, " \u{2717}"),
            Some("pending") => (YELLOW, " \u{2026}"),
            Some("draft") => (GREY, " \u{270e}"),
            _ => (GREY, ""),
        };
        let sign = if pr.merge_request { '!' } else { '#' };
        let text = format!("{color}\u{21c4} {sign}{}{mark}{RESET}", pr.number);
        line.add(
            4,
            match (&pr.url, links) {
                (Some(url), true) => link(url, &text),
                _ => text,
            },
        );
    }
    line
}

/// Claude Code's names for the causes of a cache miss, shortened as in the owner's line.
fn miss_names(causes: &[String]) -> String {
    let mut names: Vec<String> = Vec::new();
    for cause in causes {
        let name = match cause.as_str() {
            "tools_changed" => "Tools".to_owned(),
            "system_prompt_changed" => "Prompt".to_owned(),
            "ttl_expired_5m" | "ttl_expired_1h" => "TTL".to_owned(),
            "likely_server_side" => "Server".to_owned(),
            other => clean(other, 24),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.join("+")
}

/// The word for a cold prompt cache in the window's language (two letter code); English for
/// any other.
pub fn cold_word(language: &str) -> &'static str {
    match language {
        "de" => "kalt",
        "el" => "\u{03ba}\u{03c1}\u{03cd}\u{03b1}",
        "es" => "fr\u{ed}a",
        "fr" => "froid",
        "it" => "freddo",
        "ja" | "zh" => "\u{51b7}",
        "pl" => "zimny",
        "pt" => "frio",
        "ru" => "\u{0445}\u{043e}\u{043b}\u{043e}\u{0434}",
        "tr" => "so\u{11f}uk",
        _ => "cold",
    }
}

/// The user's interface language as a two letter code; Hangar's window starts in it too.
fn ui_language() -> String {
    #[cfg(windows)]
    {
        // SAFETY: takes no arguments and only returns the language id.
        let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        let code = match id & 0x3ff {
            0x04 => "zh",
            0x07 => "de",
            0x08 => "el",
            0x0a => "es",
            0x0c => "fr",
            0x10 => "it",
            0x11 => "ja",
            0x15 => "pl",
            0x16 => "pt",
            0x19 => "ru",
            0x1f => "tr",
            _ => "en",
        };
        code.to_owned()
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .map(|value| {
                value
                    .chars()
                    .take(2)
                    .collect::<String>()
                    .to_ascii_lowercase()
            })
            .unwrap_or_else(|| "en".to_owned())
    }
}

fn how_line(facts: &Facts, now: f64, cold: &str) -> Line {
    let mut line = Line::default();
    if let Some(percent) = facts.context_percent {
        let percent = percent.min(100.0);
        let filled = (percent / 10.0).round().clamp(0.0, 10.0) as usize;
        let color = level_color(percent);
        line.add(
            0,
            format!(
                "{color}{}{GREY}{}{RESET} {color}{percent:.0}%{RESET}",
                "\u{2588}".repeat(filled),
                "\u{2591}".repeat(10 - filled)
            ),
        );
        if let (Some(used), Some(size)) = (facts.context_used, facts.context_size) {
            if size > 0 {
                line.glue(
                    6,
                    format!(
                        "{GREY}({}/{}){RESET}",
                        short_tokens(used),
                        short_tokens(size)
                    ),
                );
            }
        }
        if percent >= COMPACT_AT {
            line.glue(0, format!("{RED}{BLINK}\u{26a0} /compact{NOBLINK}{RESET}"));
        }
    }
    if let Some(cache) = &facts.cache {
        match cache.ratio {
            Some(ratio) => line.add(4, format!("\u{26a1} {:.0}%", ratio * 100.0)),
            None => line.add(4, "\u{26a1}".to_owned()),
        }
        if !cache.warm {
            line.glue(4, format!("{RED}{cold}{RESET}"));
        } else if let Some(left) = cache.expires_at.and_then(|at| until(at, now)) {
            line.glue(8, format!("{GREY}{left}{RESET}"));
        }
        let recent = cache.miss_at.is_some_and(|at| now - at < MISS_SHOW_FOR);
        if recent && !cache.miss_causes.is_empty() {
            line.glue(
                7,
                format!(
                    "{YELLOW}\u{26a0} Miss: {}{RESET}",
                    miss_names(&cache.miss_causes)
                ),
            );
        }
    }
    if facts.lines_added > 0 || facts.lines_removed > 0 {
        line.add(
            6,
            format!(
                "\u{270f}\u{fe0f} {GREEN}+{}{RESET} {RED}-{}{RESET}",
                facts.lines_added, facts.lines_removed
            ),
        );
    }
    if let Some(tokens) = facts.tokens {
        line.add(5, format!("\u{03a3} {}", short_tokens(tokens)));
    }
    if let Some(cost) = facts.cost_usd {
        line.add(1, format!("{YELLOW}\u{1f4b0} ${cost:.2}{RESET}"));
        if let Some(ms) = facts
            .duration_ms
            .filter(|ms| *ms >= RATE_AFTER_MS && cost > 0.0)
        {
            let rate = cost / (ms as f64 / 3_600_000.0);
            line.glue(5, format!("{GREY}(${rate:.2}/h){RESET}"));
        }
    }
    if let Some(ms) = facts.duration_ms {
        let secs = ms / 1000;
        let (hours, mins, rest) = (secs / 3600, secs % 3600 / 60, secs % 60);
        line.add(
            3,
            if hours > 0 {
                format!("\u{23f1}\u{fe0f} {hours}h {mins:02}m")
            } else {
                format!("\u{23f1}\u{fe0f} {mins}m {rest:02}s")
            },
        );
        if let Some(api) = facts.api_ms.filter(|api| *api > 0 && ms >= 60_000) {
            let share = (api.saturating_mul(100) / ms).min(100);
            line.glue(8, format!("{GREY}(API {share}%){RESET}"));
        }
    }
    for (prio, label, limit) in [
        (1, "\u{1f50b} 5h", &facts.five_hour),
        (2, "\u{1f4c5} 7d", &facts.seven_day),
        (2, "\u{1f4b3}", &facts.spend),
    ] {
        let Some(limit) = limit else {
            continue;
        };
        let used = limit.used.min(100.0);
        line.add(
            prio,
            format!("{label} {}{used:.0}%{RESET}", level_color(used)),
        );
        if let Some(left) = limit.resets_at.and_then(|at| until(at, now)) {
            line.glue(prio + 4, format!("{GREY}\u{21bb} {left}{RESET}"));
        }
    }
    line
}

/// The two lines; `width` (the terminal's columns) makes them fit, `links` turns the repository
/// and the pull request into OSC 8 links. A line without any segment is left out.
pub fn render(facts: &Facts, now: f64, width: Option<usize>, links: bool) -> String {
    render_in(facts, now, width, links, cold_word("en"))
}

/// [`render`] with the word for a cold cache in the window's language.
pub fn render_in(facts: &Facts, now: f64, width: Option<usize>, links: bool, cold: &str) -> String {
    let lines = [where_line(facts, links), how_line(facts, now, cold)];
    let max = width.map(|w| w.saturating_sub(EDGE_COLUMNS).max(10));
    let rendered: Vec<String> = lines
        .into_iter()
        .filter(|line| !line.items.is_empty())
        .map(|line| match max {
            Some(max) => line.fit(max),
            None => line.render(),
        })
        .collect();
    if rendered.is_empty() {
        String::new()
    } else {
        format!("{}\n", rendered.join("\n"))
    }
}

// ---------------------------------------------------------------------------------------------
// Entry

fn read_input(input: impl Read + Send + 'static) -> Option<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("status-line-stdin".to_owned())
        .spawn(move || {
            let mut bytes = Vec::new();
            let read = input
                .take(MAX_INPUT + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = tx.send(read);
        })
        .ok()?;
    let bytes = rx.recv_timeout(INPUT_WAIT).ok()?.ok()?;
    (bytes.len() as u64 <= MAX_INPUT).then_some(bytes)
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Everything after reading: `None` for input that is not a JSON object.
pub fn run(
    source: Source,
    input: &[u8],
    width: Option<usize>,
    cache_dir: Option<&Path>,
    read: &Reader<'_>,
) -> Option<String> {
    let value: Value = serde_json::from_slice(input).ok()?;
    if !value.is_object() {
        return None;
    }
    let mut facts = facts_from(source, &value);
    let session = text_at(&value, &["session_id"]);
    enrich_with(&mut facts, source, session.as_deref(), cache_dir, read);
    Some(render_in(
        &facts,
        now_secs(),
        width,
        source == Source::Claude,
        cold_word(&ui_language()),
    ))
}

/// Entry from `main.rs`: `hangar status-line <claude|qwen>`. Always exits 0; a failure prints
/// nothing, so the program shows an empty line rather than an error.
pub fn main(args: &[OsString]) -> i32 {
    let Some(source) = args
        .first()
        .and_then(|a| a.to_str())
        .and_then(Source::parse)
    else {
        return 0;
    };
    let Some(input) = read_input(io::stdin()) else {
        return 0;
    };
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse::<usize>().ok())
        .filter(|c| *c > 0);
    let cache = std::env::var_os(CACHE_ENV)
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute());
    if let Some(text) = run(source, &input, width, cache.as_deref(), &read_real) {
        let mut out = io::stdout().lock();
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
    0
}

// ---------------------------------------------------------------------------------------------
// Launch

/// Which shell reads the status command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandShell {
    /// Claude Code on Windows: Git Bash, or PowerShell without it (statusline docs). No quoting
    /// reads the same in both, so the path must stand bare.
    BashOrPowerShell,
    /// Qwen Code on Windows: cmd.exe (status line docs).
    Cmd,
    /// `sh` or `bash` elsewhere.
    Posix,
}

impl CommandShell {
    pub fn for_source(source: Source) -> Self {
        match (cfg!(windows), source) {
            (true, Source::Claude) => CommandShell::BashOrPowerShell,
            (true, Source::Qwen) => CommandShell::Cmd,
            (false, _) => CommandShell::Posix,
        }
    }
}

/// The command line that runs `<exe> status-line <source>` in `shell`; `None` when the path
/// cannot stand there unchanged. Windows paths go with forward slashes: Git Bash would eat the
/// backslashes, and cmd.exe and PowerShell take both.
pub fn command_line(exe: &Path, source: Source, shell: CommandShell) -> Option<String> {
    let path = exe.to_str()?;
    if path.is_empty() || !exe.is_absolute() {
        return None;
    }
    let path = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    let plain = |c: char| c.is_alphanumeric() || matches!(c, '/' | ':' | '.' | '_' | '-' | '~');
    let program = match shell {
        CommandShell::BashOrPowerShell if path.chars().all(plain) => path,
        CommandShell::BashOrPowerShell => return None,
        CommandShell::Cmd | CommandShell::Posix if path.chars().all(|c| plain(c) || c == ' ') => {
            if path.contains(' ') {
                format!("\"{path}\"")
            } else {
                path
            }
        }
        CommandShell::Cmd | CommandShell::Posix => return None,
    };
    Some(format!("{program} {SUBCOMMAND} {}", source.name()))
}

/// The executable's path in a form without blanks where Windows keeps a short (8.3) name for
/// it; the path itself otherwise.
pub fn short_path(exe: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut buffer = vec![0u16; 1024];
        // SAFETY: `wide` is NUL-terminated and outlives the call; `buffer` is writable for its
        // whole length, which is passed as the size, and the call writes at most that many.
        let written = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetShortPathNameW(
                wide.as_ptr(),
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(0),
            )
        };
        match usize::try_from(written) {
            Ok(len) if len > 0 && len < buffer.len() => {
                buffer.truncate(len);
                PathBuf::from(OsString::from_wide(&buffer))
            }
            _ => exe.to_path_buf(),
        }
    }
    #[cfg(not(windows))]
    {
        exe.to_path_buf()
    }
}

/// [`command_line`] for this platform's shell, with the short path where the long one has a
/// blank Claude Code's shells cannot both read.
pub fn command_for(exe: &Path, source: Source) -> Option<String> {
    let shell = CommandShell::for_source(source);
    command_line(exe, source, shell).or_else(|| match shell {
        CommandShell::BashOrPowerShell => command_line(&short_path(exe), source, shell),
        _ => None,
    })
}

/// Claude Code's `statusLine` setting.
pub fn claude_setting(command: &str) -> Value {
    serde_json::json!({
        "type": "command",
        "command": command,
        "padding": 0,
        "refreshInterval": REFRESH_SECS,
        // The line shows the mode itself.
        "hideVimModeIndicator": true,
    })
}

/// Qwen Code's `ui.statusLine` setting: Hangar's colors, and its own context indicator off,
/// since the line shows the context.
pub fn qwen_setting(command: &str) -> Value {
    serde_json::json!({
        "type": "command",
        "command": command,
        "refreshInterval": REFRESH_SECS,
        "respectUserColors": true,
        "hideContextIndicator": true,
    })
}

/// `-c tui.status_line=[...]` for a Codex start, before its arguments; nothing when the
/// profile's arguments set the key themselves. TOML literal strings carry no `"`, so the value
/// also passes a cmd.exe launcher (npm's `codex.cmd`).
pub fn codex_args(user_args: &[String]) -> Vec<String> {
    let own = crate::agent_catalog::codex_overrides(user_args).any(|value| {
        crate::agent_catalog::config_key(value).is_some_and(|key| {
            key == CODEX_KEY || key == "tui" || key.starts_with("tui.status_line.")
        })
    });
    if own {
        return Vec::new();
    }
    let items: Vec<String> = CODEX_ITEMS.iter().map(|item| format!("'{item}'")).collect();
    vec![
        "-c".to_owned(),
        format!("{CODEX_KEY}=[{}]", items.join(",")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    for c in chars.by_ref() {
                        if c == '\x07' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn no_reads(_: &Path, _: Source) -> (Option<GitState>, Option<Account>) {
        (None, None)
    }

    fn full_claude() -> Value {
        serde_json::json!({
            "session_id": "abc-123",
            "version": "2.1.332",
            "model": { "display_name": "Opus 4.7" },
            "effort": { "level": "high" },
            "vim": { "mode": "INSERT" },
            "workspace": {
                "current_dir": if cfg!(windows) { "C:\\work\\hangar" } else { "/work/hangar" },
                "repo": { "host": "github.com", "owner": "acme", "name": "hangar" }
            },
            "worktree": { "name": "feature-x" },
            "pr": { "number": 42, "url": "https://github.com/acme/hangar/pull/42",
                    "review_state": "approved" },
            "context_window": { "used_percentage": 85, "total_input_tokens": 170000,
                                "context_window_size": 200000 },
            "prompt_cache": { "caching_observed": true, "hit_ratio": 0.92, "warm": true,
                              "expires_at": 4_000_000_000.0 },
            "cost": { "total_cost_usd": 2.5, "total_duration_ms": 3_600_000,
                      "total_api_duration_ms": 1_800_000, "total_lines_added": 120,
                      "total_lines_removed": 30 },
            "rate_limits": { "five_hour": { "used_percentage": 30, "resets_at": 4_000_000_000.0 },
                             "seven_day": { "used_percentage": 12 } }
        })
    }

    fn git_state() -> GitState {
        GitState {
            branch: Some("main".into()),
            oid: Some("abc1234".into()),
            ahead: 1,
            behind: 2,
            staged: 3,
            modified: 4,
            untracked: 5,
            conflicts: 0,
        }
    }

    #[test]
    fn porcelain_gives_branch_counts_and_detached_heads() {
        let out =
            b"# branch.oid 0123456789abcdef\n# branch.head main\n# branch.upstream origin/main\n\
            # branch.ab +2 -1\n1 M. N... 100644 100644 100644 a b src/a.rs\n\
            1 .M N... 100644 100644 100644 a b src/b.rs\n1 MM N... 100644 100644 100644 a b c\n\
            u UU N... 1 2 3 4 a b c d e\n? new.txt\n? other.txt\n";
        let state = parse_porcelain(out);
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!(state.oid.as_deref(), Some("0123456"));
        assert_eq!((state.ahead, state.behind), (2, 1));
        assert_eq!(
            (
                state.staged,
                state.modified,
                state.untracked,
                state.conflicts
            ),
            (2, 2, 2, 1)
        );
        let detached = parse_porcelain(b"# branch.oid fedcba9876\n# branch.head (detached)\n");
        assert_eq!(detached.branch, None);
        assert_eq!(detached.oid.as_deref(), Some("fedcba9"));
        assert_eq!(
            parse_porcelain(b"# branch.oid (initial)\n# branch.head main\n").oid,
            None
        );
        assert_eq!(
            parse_porcelain(b"garbage\n# branch.ab x y\n"),
            GitState::default()
        );
    }

    #[test]
    fn every_segment_shows_with_full_input() {
        let value = full_claude();
        let mut facts = facts_from(Source::Claude, &value);
        facts.git = Some(git_state());
        facts.account = Some(Account {
            name: "Ann".into(),
            email: Some("ann@example.invalid".into()),
            plan: Some("Max 5x".into()),
        });
        let text = render(&facts, 3_999_990_000.0, None, true);
        let plain = strip(&text);
        let lines: Vec<&str> = plain.lines().collect();
        assert_eq!(lines.len(), 2, "{plain}");
        assert_eq!(
            lines[0],
            "[Opus 4.7] INS high v2.1.332 | 👤 Ann <ann@example.invalid> Max 5x | 📁 hangar \
             ⎇ feature-x | 🌿 main ↑1 ↓2 +3 ~4 ?5 | 🔗 hangar | ⇄ #42 ✓"
        );
        assert!(lines[1].starts_with("\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2591} 85% (170k/200k) \u{26a0} /compact"), "{}", lines[1]);
        for part in [
            " | ⚡ 92% 2h46m",
            " | ✏️ +120 -30",
            " | 💰 $2.50 ($2.50/h)",
            " | ⏱️ 1h 00m (API 50%)",
            " | 🔋 5h 30% ↻ 2h46m",
            " | 📅 7d 12%",
        ] {
            assert!(lines[1].contains(part), "{part} missing in {}", lines[1]);
        }
        // Repository and pull request are links for Claude Code.
        assert!(text.contains("\x1b]8;;https://github.com/acme/hangar\x07"));
        assert!(text.contains("\x1b]8;;https://github.com/acme/hangar/pull/42\x07"));
        assert!(!render(&facts, 0.0, None, false).contains("\x1b]8;;"));
    }

    #[test]
    fn segments_without_data_are_left_out() {
        let facts = facts_from(
            Source::Claude,
            &serde_json::json!({ "model": { "display_name": "Sonnet" } }),
        );
        // Model, context, cost and time always show for Claude Code.
        assert_eq!(
            strip(&render(&facts, 0.0, None, true)),
            "[Sonnet]\n░░░░░░░░░░ 0% | 💰 $0.00 | ⏱️ 0m 00s\n"
        );
        // Nothing at all: no line.
        assert_eq!(render(&Facts::default(), 0.0, None, true), "");
        let facts = facts_from(
            Source::Claude,
            &serde_json::json!({ "cost": { "total_cost_usd": 0.5 } }),
        );
        assert_eq!(
            strip(&render(&facts, 0.0, None, true)),
            "[Claude]\n░░░░░░░░░░ 0% | 💰 $0.50 | ⏱️ 0m 00s\n"
        );
        // No rate before ten minutes, no API share before one, no countdown once passed.
        let facts = facts_from(
            Source::Claude,
            &serde_json::json!({ "cost": { "total_cost_usd": 1.0, "total_duration_ms": 30_000,
                "total_api_duration_ms": 10_000 },
                "rate_limits": { "five_hour": { "used_percentage": 5, "resets_at": 100 } } }),
        );
        assert_eq!(
            strip(&render(&facts, 200.0, None, true)),
            "[Claude]\n░░░░░░░░░░ 0% | 💰 $1.00 | ⏱️ 0m 30s | 🔋 5h 5%\n"
        );
    }

    #[test]
    fn narrow_terminals_drop_unimportant_segments_in_a_fixed_order() {
        let mut facts = facts_from(Source::Claude, &full_claude());
        facts.git = Some(git_state());
        let wide = strip(&render(&facts, 3_999_990_000.0, Some(400), true));
        let wide_first = wide.lines().next().unwrap().to_owned();
        assert!(wide_first.contains("v2.1.332"));
        // A bit narrower: the version (priority 9) goes first, everything else stays.
        let width = display_width(&wide_first) + EDGE_COLUMNS - 1;
        let narrower = strip(&render(&facts, 3_999_990_000.0, Some(width), true));
        let first = narrower.lines().next().unwrap();
        assert!(!first.contains("v2.1.332"), "{first}");
        assert!(first.contains("🔗 hangar"), "{first}");
        // Very narrow: the most important ones stay, every line fits.
        let narrow = render(&facts, 3_999_990_000.0, Some(30), true);
        for line in narrow.lines() {
            assert!(
                display_width(line) <= 26 || !line.contains(" | "),
                "{}",
                strip(line)
            );
        }
        let plain = strip(&narrow);
        let mut lines = plain.lines();
        assert!(lines.next().unwrap().starts_with("[Opus 4.7]"));
        assert!(lines.next().unwrap().contains("85%"));
    }

    #[test]
    fn a_dropped_first_segment_hands_its_place_on() {
        let mut line = Line::default();
        line.add(9, "aaaa".into());
        line.glue(9, "bbbb".into());
        line.add(0, "cccc".into());
        line.glue(1, "dd".into());
        assert_eq!(strip(&line.fit(7)), "cccc dd");
    }

    #[test]
    fn broken_input_prints_nothing() {
        for input in [&b"not json"[..], b"", b"[1,2]", b"\"text\"", b"{\"model\":"] {
            assert_eq!(run(Source::Claude, input, None, None, &no_reads), None);
        }
        // Wrong types are left out, never a panic.
        let odd = br#"{"model":{"display_name":5},"context_window":{"used_percentage":"x"},
            "cost":{"total_cost_usd":-3,"total_duration_ms":1e300},"pr":{"number":-1},
            "rate_limits":{"five_hour":{"used_percentage":null}}}"#;
        let text = run(Source::Claude, odd, Some(1), None, &no_reads).unwrap();
        assert!(!text.contains("$-"));
        assert_eq!(
            run(Source::Qwen, b"{}", Some(80), None, &no_reads),
            Some(String::new())
        );
    }

    #[test]
    fn control_characters_in_names_are_neutralized() {
        let value = serde_json::json!({
            "model": { "display_name": "Opus\u{1b}[2J" },
            "workspace": { "current_dir": "/w/evil\u{7}dir\u{202e}txt" },
            "session_name": "a\u{9b}b",
            "pr": { "number": 3, "url": "https://x/\u{1b}]8;;evil" },
            "workspace_repo": {}
        });
        let mut facts = facts_from(Source::Claude, &value);
        facts.git = Some(GitState {
            branch: Some("feat\u{1b}]0;title\u{7}".into()),
            ..GitState::default()
        });
        let text = render(&facts, 0.0, None, true);
        // Only Hangar's own SGR sequences remain; no BEL, no OSC, no CSI from the input.
        assert!(!text.contains('\u{7}'));
        assert!(!text.contains("\x1b]"));
        assert!(!text.contains("\x1b[2J"));
        assert!(!text.contains('\u{202e}'));
        assert!(!text.contains('\u{9b}'));
        let plain = strip(&text);
        assert!(plain.contains("Opus?[2J"), "{plain}");
        assert!(plain.contains("evil?dir?txt"), "{plain}");
        assert!(plain.contains("feat?]0;title?"), "{plain}");
        // The unsafe URL is dropped, the number stays.
        assert!(plain.contains("#3"));
        assert_eq!(clean("  abcdefgh  ", 5), "abcd\u{2026}");
    }

    #[test]
    fn widths_count_colors_links_and_wide_characters_right() {
        assert_eq!(display_width("\x1b[31mab\x1b[0m"), 2);
        assert_eq!(display_width(&link("https://x", "abc")), 3);
        assert_eq!(display_width("日本"), 4);
        assert_eq!(display_width("e\u{301}"), 1);
        assert_eq!(short_tokens(152_340), "152k");
        assert_eq!(short_tokens(1_000_000), "1M");
        assert_eq!(short_tokens(1_500_000), "1.5M");
        assert_eq!(short_tokens(999), "999");
        assert_eq!(until(100.0 + 90_000.0, 100.0).as_deref(), Some("1d1h"));
        assert_eq!(until(100.0 + 7_980.0, 100.0).as_deref(), Some("2h13m"));
        assert_eq!(until(100.0 + 720.0, 100.0).as_deref(), Some("12m"));
        assert_eq!(until(50.0, 100.0), None);
    }

    #[test]
    fn qwen_input_is_read_in_its_own_shape() {
        let value = serde_json::json!({
            "session_id": "q1",
            "version": "0.30.0",
            "model": { "display_name": "qwen3-coder" },
            "context_window": { "used_percentage": 40, "total_input_tokens": 40000,
                                "context_window_size": 100000 },
            "workspace": { "current_dir": "/w/app" },
            "git": { "branch": "dev" },
            "metrics": {
                "models": {
                    "a": { "tokens": { "prompt": 1000, "completion": 10, "total": 1010, "cached": 500 } },
                    "b": { "tokens": { "prompt": 1000, "total": 1000, "cached": 300 } }
                },
                "files": { "total_lines_added": 7, "total_lines_removed": 2 }
            },
            "vim": { "mode": "NORMAL" }
        });
        let facts = facts_from(Source::Qwen, &value);
        assert_eq!(facts.tokens, Some(2010));
        assert_eq!(
            facts.git.as_ref().and_then(|g| g.branch.as_deref()),
            Some("dev")
        );
        let plain = strip(&render(&facts, 0.0, None, false));
        assert!(
            plain.starts_with("[qwen3-coder] NOR v0.30.0 | 📁 app | 🌿 dev\n"),
            "{plain}"
        );
        assert!(plain.contains("40% (40k/100k)"));
        assert!(plain.contains("⚡ 40%"));
        assert!(plain.contains("+7 -2"));
        assert!(plain.contains("\u{03a3} 2k"));
        assert!(!plain.contains('$'));
    }

    #[test]
    fn the_account_shows_name_email_and_plan_only() {
        let file = br#"{"oauthAccount":{"emailAddress":"a@example.invalid","displayName":"Ann",
            "organizationRateLimitTier":"default_claude_max_5x"},"primaryApiKey":"HANGAR-MARKER"}"#;
        let account = parse_account(file).unwrap();
        assert_eq!(
            account,
            Account {
                name: "Ann".into(),
                email: Some("a@example.invalid".into()),
                plan: Some("Max 5x".into())
            }
        );
        assert!(!format!("{account:?}").contains("HANGAR-MARKER"));
        assert_eq!(plan_name("claude_pro").as_deref(), Some("Pro"));
        assert_eq!(plan_name("enterprise"), None);
        // Without a name the e-mail's local part stands in, as in the owner's line.
        assert_eq!(
            parse_account(br#"{"oauthAccount":{"emailAddress":"x@example.invalid"}}"#)
                .map(|account| account.name),
            Some("x".to_owned())
        );
        assert_eq!(parse_account(br#"{"oauthAccount":{}}"#), None);
        assert_eq!(parse_account(b"{}"), None);
    }

    #[test]
    fn a_cold_cache_and_a_recent_miss_show_with_their_cause() {
        let now = 1_000_000.0;
        let value = serde_json::json!({
            "prompt_cache": { "caching_observed": true, "hit_ratio": 0.5, "warm": false,
                "last_miss_at": now - 60.0,
                "last_miss_cause": { "causes": ["tools_changed", "ttl_expired_5m",
                                                "ttl_expired_1h", "odd\u{7}cause"] } }
        });
        let facts = facts_from(Source::Claude, &value);
        let plain = strip(&render_in(&facts, now, None, true, cold_word("de")));
        assert!(
            plain.contains(" | ⚡ 50% kalt ⚠ Miss: Tools+TTL+odd?cause"),
            "{plain}"
        );
        // Ten minutes later the miss is gone; English stands in for other languages.
        let later = strip(&render_in(&facts, now + 600.0, None, true, cold_word("xx")));
        assert!(later.contains("⚡ 50% cold"), "{later}");
        assert!(!later.contains("Miss"), "{later}");
        assert_eq!(cold_word("fr"), "froid");
    }

    #[test]
    fn git_readings_are_cached_per_session_and_folder() {
        let tmp = crate::test_support::TempDir::new("status-line-cache");
        let cache = tmp.path().join("cache");
        let dir = if cfg!(windows) {
            "C:\\w\\app"
        } else {
            "/w/app"
        };
        let input =
            serde_json::json!({ "session_id": "s/../1", "workspace": { "current_dir": dir } })
                .to_string();
        let calls = std::cell::Cell::new(0);
        let read = |_: &Path, _: Source| {
            calls.set(calls.get() + 1);
            (Some(git_state()), None)
        };
        for _ in 0..3 {
            let text = run(Source::Claude, input.as_bytes(), None, Some(&cache), &read).unwrap();
            assert!(strip(&text).contains("🌿 main"));
        }
        assert_eq!(calls.get(), 1);
        // The file name keeps only safe characters.
        assert!(cache.join("s1.json").is_file());
        // Another folder reads again.
        let other = serde_json::json!({ "session_id": "s/../1",
            "workspace": { "current_dir": if cfg!(windows) { "C:\\w\\b" } else { "/w/b" } } })
        .to_string();
        run(Source::Claude, other.as_bytes(), None, Some(&cache), &read).unwrap();
        assert_eq!(calls.get(), 2);
        // Without a cache folder every rendering reads.
        run(Source::Claude, input.as_bytes(), None, None, &read).unwrap();
        assert_eq!(calls.get(), 3);
        // A relative folder is never read.
        let relative = br#"{"session_id":"x","workspace":{"current_dir":"rel"}}"#;
        run(Source::Claude, relative, None, Some(&cache), &read).unwrap();
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn old_cache_files_are_pruned() {
        let tmp = crate::test_support::TempDir::new("status-line-prune");
        let file = tmp.path().join("old.json");
        std::fs::write(&file, "{}").unwrap();
        prune(tmp.path(), SystemTime::now());
        assert!(file.exists());
        prune(
            tmp.path(),
            SystemTime::now() + CACHE_PRUNE + Duration::from_secs(60),
        );
        assert!(!file.exists());
    }

    #[test]
    fn command_lines_fit_their_shell() {
        let (exe, spaced) = if cfg!(windows) {
            (
                PathBuf::from(r"C:\Users\Ann\AppData\Local\Hangar\hangar.exe"),
                PathBuf::from(r"C:\Program Files\Hangar\hangar.exe"),
            )
        } else {
            (
                PathBuf::from("/opt/hangar/hangar"),
                PathBuf::from("/opt/my apps/hangar"),
            )
        };
        let fwd = |p: &Path| p.to_str().unwrap().replace('\\', "/");
        assert_eq!(
            command_line(&exe, Source::Claude, CommandShell::BashOrPowerShell).unwrap(),
            format!("{} status-line claude", fwd(&exe))
        );
        // Neither Git Bash nor PowerShell reads one quoting the same way.
        assert_eq!(
            command_line(&spaced, Source::Claude, CommandShell::BashOrPowerShell),
            None
        );
        assert_eq!(
            command_line(&spaced, Source::Qwen, CommandShell::Cmd).unwrap(),
            format!("\"{}\" status-line qwen", fwd(&spaced))
        );
        assert_eq!(
            command_line(&spaced, Source::Claude, CommandShell::Posix).unwrap(),
            format!("\"{}\" status-line claude", fwd(&spaced))
        );
        for bad in [
            "/opt/a$b/hangar",
            "/opt/a\"b/hangar",
            "/opt/(x)/hangar",
            "relative/hangar",
        ] {
            for shell in [
                CommandShell::Cmd,
                CommandShell::Posix,
                CommandShell::BashOrPowerShell,
            ] {
                assert_eq!(
                    command_line(Path::new(bad), Source::Qwen, shell),
                    None,
                    "{bad}"
                );
            }
        }
    }

    #[test]
    fn settings_values_carry_the_command() {
        let claude = claude_setting("x status-line claude");
        assert_eq!(claude["type"], "command");
        assert_eq!(claude["command"], "x status-line claude");
        assert_eq!(claude["refreshInterval"], REFRESH_SECS);
        assert_eq!(claude["hideVimModeIndicator"], true);
        let qwen = qwen_setting("x status-line qwen");
        assert_eq!(qwen["respectUserColors"], true);
        assert_eq!(qwen["hideContextIndicator"], true);
    }

    #[test]
    fn codex_gets_its_items_unless_the_profile_sets_them() {
        let args = codex_args(&[]);
        assert_eq!(args[0], "-c");
        assert_eq!(
            args[1],
            "tui.status_line=['model-with-reasoning','current-dir','git-branch','branch-changes',\
             'pull-request-number','context-used','used-tokens','five-hour-limit','weekly-limit']"
        );
        assert!(crate::batch::argument_ok(&args[1]));
        let own =
            |list: &[&str]| codex_args(&list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert!(own(&["-c", "tui.status_line=[]"]).is_empty());
        assert!(own(&["--config=tui.status_line=['model-name']"]).is_empty());
        assert!(own(&["-c", "tui={status_line=[]}"]).is_empty());
        assert_eq!(own(&["-c", "model=gpt"]).len(), 2);
    }
}
