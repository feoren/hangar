//! Claude Code sub-agent definitions: Markdown files with a frontmatter (`name`, `description`,
//! optional `model`, `color`, `tools`) in `~/.claude/agents/` (global) and
//! `<project>/.claude/agents/` (project). Hangar lists them, adds new global ones from a pasted
//! text and starts Claude Code as one of them (`--agent`). Contract:
//! docs/wiki/pages/syntheses/sub-agent-library.md.
//!
//! The frontmatter is read leniently, the way the user's files are written in practice: a
//! description like `Backend: logic, APIs` holds a `: ` that strict YAML refuses. A broken file
//! becomes a [`DefProblem`] in the listing, never an error of the whole listing.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::ser::SerializeStruct as _;
use serde::{Serialize, Serializer};

use crate::no_links::{self, Links};

/// Larger files are not read: the user's own definitions are a few KiB.
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// Files read per agents folder; more is reported once as [`Problem::TooMany`].
pub const MAX_FILES: usize = 256;
/// Project folders one listing reads.
pub const MAX_FOLDERS: usize = 32;
/// Longest name taken.
pub const MAX_NAME_LEN: usize = 64;
/// Longest description handed to the window, in characters.
const MAX_DESCRIPTION_CHARS: usize = 2000;
/// Frontmatter fields that give an agent started with `--agent` more than a prompt: its own
/// hooks and MCP servers (Claude Code skips both when the definition's folder is not trusted)
/// and a permission mode (ignored where it would widen the inherited one). Read from the
/// 2.1.325 binary, docs/wiki/pages/sources/claude-agent-flag-measurement.md. The window names
/// them for project definitions, which come with a repository.
pub const POWER_FIELDS: [&str; 3] = ["hooks", "mcpServers", "permissionMode"];
/// Longest `model` or `color` value kept.
const MAX_SHORT_VALUE: usize = 64;

/// `<base>/.claude/agents`, the folder Claude Code reads sub-agents from (the home folder for
/// the global ones, a project folder for that project's).
pub fn agents_dir(base: &Path) -> PathBuf {
    base.join(".claude").join("agents")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// `~/.claude/agents`: in every project.
    Global,
    /// `<project>/.claude/agents`: in that project only.
    Project,
}

/// One readable definition, as the window shows it. The prompt below the frontmatter stays in
/// the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub color: Option<String>,
    pub tools: Vec<String>,
    /// Fields of [`POWER_FIELDS`] the frontmatter sets, in that order.
    pub extras: Vec<String>,
    pub scope: Scope,
    /// The project folder of a project definition, as the window named it; `None` when global.
    pub folder: Option<PathBuf>,
    /// The file's name in its agents folder.
    pub file: String,
}

/// Why a file in an agents folder is not listed as a definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Problem {
    TooLarge,
    NotText,
    NoFrontmatter,
    MissingName,
    /// The name breaks [`valid_name`].
    InvalidName,
    MissingDescription,
    /// A second file with a name already taken in the same folder.
    Duplicate,
    /// A symlink, junction or other reparse point; links are not followed.
    Link,
    Unreadable,
    /// More than [`MAX_FILES`] files; the rest is not read.
    TooMany,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefProblem {
    pub scope: Scope,
    pub folder: Option<PathBuf>,
    pub file: String,
    pub problem: Problem,
}

/// Every definition found, global first, then the project folders in the order named; each
/// folder's files in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub defs: Vec<AgentDef>,
    pub problems: Vec<DefProblem>,
}

/// A name as Claude Code's documentation describes it: lowercase letters and hyphens (digits
/// taken as well), starting and ending with a letter or digit, at most [`MAX_NAME_LEN`]. The
/// same rule decides what `--agent` may get, so the name never needs quoting on a command line
/// or in a cmd.exe launcher.
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let edge =
        |byte: Option<&u8>| byte.is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    !bytes.is_empty()
        && bytes.len() <= MAX_NAME_LEN
        && edge(bytes.first())
        && edge(bytes.last())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A frontmatter value: one text, or a list (`[a, b]` or `- a` lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
}

/// The fields between the leading `---` line and the next one; `None` without both. Keys must
/// start a line; indented lines continue the field above. The first of two equal keys wins.
pub fn frontmatter(text: &str) -> Option<BTreeMap<String, Value>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut body = Vec::new();
    for line in lines {
        if line.trim_end() == "---" {
            return Some(fields(&body));
        }
        body.push(line);
    }
    None
}

fn is_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn indented(line: &str) -> bool {
    line.starts_with([' ', '\t'])
}

fn fields(lines: &[&str]) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || indented(line) {
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if !is_key(key) {
            continue;
        }
        let start = index;
        while index < lines.len() && (indented(lines[index]) || lines[index].trim().is_empty()) {
            index += 1;
        }
        let value = field_value(rest.trim(), &lines[start..index]);
        out.entry(key.to_owned()).or_insert(value);
    }
    out
}

fn field_value(first: &str, more: &[&str]) -> Value {
    if first.starts_with(['|', '>']) {
        return Value::Text(block(first.starts_with('|'), more));
    }
    let more: Vec<&str> = more
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if first.is_empty() && !more.is_empty() && more.iter().all(|line| line.starts_with('-')) {
        let items = more
            .iter()
            .map(|line| unquote(line.trim_start_matches('-').trim()))
            .filter(|item| !item.is_empty())
            .collect();
        return Value::List(items);
    }
    let joined = std::iter::once(first)
        .chain(more)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(inner) = joined
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        let items = inner
            .split(',')
            .map(|item| unquote(item.trim()))
            .filter(|item| !item.is_empty())
            .collect();
        return Value::List(items);
    }
    Value::Text(unquote(&joined))
}

/// A `|` (lines kept) or `>` (lines folded) block, without its common indentation.
fn block(literal: bool, lines: &[&str]) -> String {
    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let lines: Vec<&str> = lines
        .iter()
        .map(|line| line.get(indent..).unwrap_or("").trim_end())
        .collect();
    let text = if literal {
        lines.join("\n")
    } else {
        let mut out = String::new();
        for line in lines {
            if line.is_empty() {
                out.push('\n');
            } else {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(line);
            }
        }
        out
    };
    text.trim().to_owned()
}

/// A quoted value without its quotes (`\"` and friends in double quotes, `''` in single quotes);
/// a plain value without a trailing ` # comment`.
fn unquote(value: &str) -> String {
    if let Some(rest) = value.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => break,
                },
                other => out.push(other),
            }
        }
        return out;
    }
    if let Some(rest) = value.strip_prefix('\'') {
        let mut out = String::new();
        let mut chars = rest.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    out.push('\'');
                } else {
                    break;
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    let plain = value.find(" #").map_or(value, |at| &value[..at]);
    plain.trim().to_owned()
}

/// What a definition's frontmatter says, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub color: Option<String>,
    pub tools: Vec<String>,
    /// Fields of [`POWER_FIELDS`] the frontmatter sets, in that order.
    pub extras: Vec<String>,
}

fn text_field(fields: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    match fields.get(key)? {
        Value::Text(text) => Some(text.trim().to_owned()).filter(|text| !text.is_empty()),
        Value::List(_) => None,
    }
}

/// A short value (`model`, `color`) from plain characters only, or nothing.
fn short_field(fields: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    text_field(fields, key).filter(|value| {
        value.len() <= MAX_SHORT_VALUE
            && value.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '[' | ']' | '#')
            })
    })
}

/// Reads one definition's text.
pub fn parse_def(text: &str) -> Result<Parsed, Problem> {
    let fields = frontmatter(text).ok_or(Problem::NoFrontmatter)?;
    let name = text_field(&fields, "name").ok_or(Problem::MissingName)?;
    if !valid_name(&name) {
        return Err(Problem::InvalidName);
    }
    let description = text_field(&fields, "description").ok_or(Problem::MissingDescription)?;
    let description = match description.char_indices().nth(MAX_DESCRIPTION_CHARS) {
        Some((end, _)) => format!("{}…", &description[..end]),
        None => description,
    };
    let tools = match fields.get("tools") {
        Some(Value::Text(text)) => text
            .split(',')
            .map(|tool| tool.trim().to_owned())
            .filter(|tool| !tool.is_empty())
            .collect(),
        Some(Value::List(items)) => items.clone(),
        None => Vec::new(),
    };
    Ok(Parsed {
        name,
        description,
        model: short_field(&fields, "model"),
        color: short_field(&fields, "color").map(|color| color.to_ascii_lowercase()),
        tools,
        extras: POWER_FIELDS
            .iter()
            .filter(|key| fields.contains_key(**key))
            .map(|key| (*key).to_owned())
            .collect(),
    })
}

/// The text of one file, at most [`MAX_FILE_BYTES`]; a link swapped in after the listing is not
/// followed either ([`crate::persist::read_regular_capped`]).
fn read_text(path: &Path) -> Result<String, Problem> {
    let bytes = crate::persist::read_regular_capped(path, MAX_FILE_BYTES)
        .map_err(|_| Problem::Unreadable)?
        .ok_or(Problem::TooLarge)?;
    String::from_utf8(bytes).map_err(|_| Problem::NotText)
}

fn is_markdown(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// One readable definition and the path it was read from.
struct Found {
    def: AgentDef,
    path: PathBuf,
}

/// Reads one agents folder. A missing folder, one off this machine or one reached through a link
/// to another computer adds nothing; a link inside it is a [`Problem::Link`].
fn scan_dir(dir: &Path, scope: Scope, folder: Option<&Path>, out: &mut Library) -> Vec<Found> {
    if no_links::is_network_or_device(dir) {
        return Vec::new();
    }
    let is_dir = no_links::metadata(dir, Links::FollowLocal).is_ok_and(|meta| meta.is_dir());
    let Some(entries) = is_dir.then(|| fs::read_dir(dir).ok()).flatten() else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_markdown(name))
        .collect();
    names.sort();
    let problem = |out: &mut Library, file: &str, problem: Problem| {
        out.problems.push(DefProblem {
            scope,
            folder: folder.map(Path::to_owned),
            file: file.to_owned(),
            problem,
        });
    };
    if names.len() > MAX_FILES {
        let first_skipped = names[MAX_FILES].clone();
        problem(out, &first_skipped, Problem::TooMany);
        names.truncate(MAX_FILES);
    }
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for file in names {
        let path = dir.join(&file);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            problem(out, &file, Problem::Unreadable);
            continue;
        };
        if no_links::is_reparse_point(&meta) {
            problem(out, &file, Problem::Link);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let parsed = read_text(&path).and_then(|text| parse_def(&text));
        match parsed {
            Err(reason) => problem(out, &file, reason),
            Ok(parsed) if !seen.insert(parsed.name.clone()) => {
                problem(out, &file, Problem::Duplicate);
            }
            Ok(parsed) => {
                let def = AgentDef {
                    name: parsed.name,
                    description: parsed.description,
                    model: parsed.model,
                    color: parsed.color,
                    tools: parsed.tools,
                    extras: parsed.extras,
                    scope,
                    folder: folder.map(Path::to_owned),
                    file,
                };
                out.defs.push(def.clone());
                found.push(Found { def, path });
            }
        }
    }
    found
}

/// The project folders worth reading: absolute, each once, not the home folder (whose agents
/// folder is the global one), at most [`MAX_FOLDERS`].
fn project_folders<'a>(home: &Path, folders: &'a [PathBuf]) -> Vec<&'a Path> {
    let mut seen = HashSet::new();
    folders
        .iter()
        .map(PathBuf::as_path)
        .filter(|folder| folder.is_absolute() && *folder != home)
        .filter(|folder| seen.insert(folder.to_path_buf()))
        .take(MAX_FOLDERS)
        .collect()
}

/// The global definitions and those of the named project folders.
pub fn library(home: &Path, folders: &[PathBuf]) -> Library {
    let mut out = Library::default();
    scan_dir(&agents_dir(home), Scope::Global, None, &mut out);
    for folder in project_folders(home, folders) {
        scan_dir(&agents_dir(folder), Scope::Project, Some(folder), &mut out);
    }
    out
}

#[derive(Debug, thiserror::Error)]
pub enum DefError {
    #[error("The text is too long: {0} bytes, at most {MAX_FILE_BYTES} allowed")]
    TooLarge(usize),
    #[error("The text is no sub-agent definition ({0:?})")]
    Invalid(Problem),
    #[error("A sub-agent named {0} exists already")]
    Exists(String),
    #[error("{0} is a device name on Windows and cannot be a file name")]
    Reserved(String),
    #[error("No sub-agent named {0}")]
    NotFound(String),
    #[error("{0} is a link; Hangar does not write through links")]
    Link(PathBuf),
    #[error("{0} lies off this machine")]
    Remote(PathBuf),
    #[error("{path} cannot be written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl DefError {
    fn kind(&self) -> &'static str {
        match self {
            Self::TooLarge(_) => "tooLarge",
            Self::Invalid(_) => "invalid",
            Self::Exists(_) => "exists",
            Self::Reserved(_) => "reserved",
            Self::NotFound(_) => "notFound",
            Self::Link(_) | Self::Remote(_) | Self::Io { .. } | Self::Task(_) => "io",
        }
    }
}

/// `{ kind, message, problem? }`: the window words `invalid` and `exists` itself and asks before
/// replacing; the other kinds show the message.
impl Serialize for DefError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("DefError", 3)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        let problem = match self {
            Self::Invalid(problem) => Some(*problem),
            _ => None,
        };
        state.serialize_field("problem", &problem)?;
        state.end()
    }
}

/// The global agents folder, created if missing; refused when it lies off this machine.
fn global_dir(home: &Path) -> Result<PathBuf, DefError> {
    let dir = agents_dir(home);
    if no_links::is_network_or_device(&dir) {
        return Err(DefError::Remote(dir));
    }
    fs::create_dir_all(&dir).map_err(|source| DefError::Io {
        path: dir.clone(),
        source,
    })?;
    match no_links::metadata(&dir, Links::FollowLocal) {
        Ok(meta) if meta.is_dir() => Ok(dir),
        Ok(_) => Err(DefError::Link(dir)),
        Err(no_links::NoLinksError::Remote(path)) => Err(DefError::Remote(path)),
        Err(error) => Err(DefError::Io {
            path: dir,
            source: io::Error::other(error.to_string()),
        }),
    }
}

/// The global definition named `name` and its file.
fn find_global(home: &Path, name: &str) -> Option<Found> {
    let mut ignored = Library::default();
    scan_dir(&agents_dir(home), Scope::Global, None, &mut ignored)
        .into_iter()
        .find(|found| found.def.name == name)
}

fn refuse_link(path: &Path) -> Result<(), DefError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if no_links::is_reparse_point(&meta) => Err(DefError::Link(path.to_owned())),
        _ => Ok(()),
    }
}

/// Saves `text` as a global definition: `<name>.md` in `~/.claude/agents`, written atomically.
/// A definition of that name, or a file `<name>.md`, is only replaced with `replace`; then the
/// existing definition's own file is the one written.
pub fn create(home: &Path, text: &str, replace: bool) -> Result<AgentDef, DefError> {
    if text.len() > MAX_FILE_BYTES {
        return Err(DefError::TooLarge(text.len()));
    }
    let parsed = parse_def(text).map_err(DefError::Invalid)?;
    if is_device_name(&parsed.name) {
        return Err(DefError::Reserved(parsed.name));
    }
    let dir = global_dir(home)?;
    let mut bytes = text.replace("\r\n", "\n").into_bytes();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    let file = match find_global(home, &parsed.name) {
        // Only a definition of that name is ever replaced, in its own file.
        Some(_) if !replace => return Err(DefError::Exists(parsed.name)),
        Some(found) => {
            refuse_link(&found.path)?;
            crate::persist::write_atomic(&found.path, &bytes).map_err(|source| DefError::Io {
                path: found.path.clone(),
                source,
            })?;
            found.def.file
        }
        // A file that merely holds the name (broken, another definition, another case) stays:
        // the new definition takes the first free name and never overwrites.
        None => write_new(&dir, &parsed.name, &bytes)?,
    };
    Ok(AgentDef {
        name: parsed.name,
        description: parsed.description,
        model: parsed.model,
        color: parsed.color,
        tools: parsed.tools,
        extras: parsed.extras,
        scope: Scope::Global,
        folder: None,
        file,
    })
}

/// Windows device names (`con`, `nul`, `com1` and so on), which cannot be file names there.
/// Refused only when a definition is created: an existing one keeps working.
pub fn is_device_name(name: &str) -> bool {
    matches!(name, "con" | "nul" | "aux" | "prn")
        || ["com", "lpt"].iter().any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| matches!(rest.as_bytes(), [b'1'..=b'9']))
        })
}

/// Tried file names per definition: `<name>.md`, `<name>-2.md` up to this many.
const NAME_ATTEMPTS: usize = 100;

/// Keeps the temporary files of concurrent creations in one process apart.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` as a new file `<name>.md` (or `<name>-2.md` and so on, the first name no file
/// holds in any case) and returns its name. Never replaces a file: the text goes to a temporary
/// file first, which is then hard linked to its name, a step that fails when the name is taken;
/// where the file system has no hard links, the file is created with `create_new`.
fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<String, DefError> {
    let io_error = |path: &Path| {
        let path = path.to_owned();
        move |source| DefError::Io { path, source }
    };
    let taken: HashSet<String> = fs::read_dir(dir)
        .map_err(io_error(dir))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .map(|file| file.to_lowercase())
        .collect();
    let candidates = (1..=NAME_ATTEMPTS)
        .map(|n| match n {
            1 => format!("{name}.md"),
            n => format!("{name}-{n}.md"),
        })
        .filter(|file| !taken.contains(file));
    let temp = dir.join(format!(
        ".{name}.{}.{}.new",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = crate::persist::create_private_file(&temp)?;
        io::Write::write_all(&mut file, bytes)?;
        file.sync_all()
    })();
    if let Err(source) = written {
        let _ = fs::remove_file(&temp);
        return Err(DefError::Io { path: temp, source });
    }
    let mut result = Err(DefError::Exists(name.to_owned()));
    for file in candidates {
        let path = dir.join(&file);
        match link_new(&temp, &path, bytes) {
            Ok(()) => {
                result = Ok(file);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                result = Err(DefError::Io { path, source });
                break;
            }
        }
    }
    let _ = fs::remove_file(&temp);
    result
}

/// `temp`'s content under `path`, failing with `AlreadyExists` when `path` exists.
fn link_new(temp: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::hard_link(temp, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
        // No hard links here (FAT, some network drives): create the file itself, still never
        // replacing one.
        Err(_) => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            let written = io::Write::write_all(&mut file, bytes).and_then(|()| file.sync_all());
            if written.is_err() {
                drop(file);
                let _ = fs::remove_file(path);
            }
            written
        }
    }
}

/// Deletes the global definition named `name` (its file). Project definitions belong to their
/// repository and are never deleted from Hangar.
pub fn delete(home: &Path, name: &str) -> Result<(), DefError> {
    let found = find_global(home, name).ok_or_else(|| DefError::NotFound(name.to_owned()))?;
    refuse_link(&found.path)?;
    fs::remove_file(&found.path).map_err(|source| DefError::Io {
        path: found.path,
        source,
    })
}

/// The file of the definition named `name`: global without `folder`, else that project's.
pub fn def_path(home: &Path, name: &str, folder: Option<&Path>) -> Result<PathBuf, DefError> {
    let mut ignored = Library::default();
    let found = match folder {
        None => scan_dir(&agents_dir(home), Scope::Global, None, &mut ignored),
        Some(folder) if folder.is_absolute() => scan_dir(
            &agents_dir(folder),
            Scope::Project,
            Some(folder),
            &mut ignored,
        ),
        Some(_) => Vec::new(),
    };
    found
        .into_iter()
        .find(|found| found.def.name == name)
        .map(|found| found.path)
        .ok_or_else(|| DefError::NotFound(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const BACKEND: &str = "---\nname: backend\ndescription: Backend work: logic, APIs and jobs. Use PROACTIVELY.\nmodel: opus\ncolor: Green\n---\n\nYou are the backend engineer.\n";

    #[test]
    fn names_follow_claude_codes_rule() {
        for good in ["backend", "code-reviewer", "a", "agent-2", "x9"] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "",
            "Backend",
            "-lead",
            "trail-",
            "with space",
            "under_score",
            "dot.name",
            "über",
            "a\"b",
            "%PATH%",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(valid_name(&"a".repeat(MAX_NAME_LEN)));
        assert!(!valid_name(&"a".repeat(MAX_NAME_LEN + 1)));
    }

    #[test]
    fn the_frontmatter_is_read_leniently() {
        let parsed = parse_def(BACKEND).unwrap();
        assert_eq!(parsed.name, "backend");
        // A ": " inside a plain value stays part of it.
        assert_eq!(
            parsed.description,
            "Backend work: logic, APIs and jobs. Use PROACTIVELY."
        );
        assert_eq!(parsed.model.as_deref(), Some("opus"));
        assert_eq!(parsed.color.as_deref(), Some("green"));
        assert!(parsed.tools.is_empty());

        let crlf = BACKEND.replace('\n', "\r\n");
        assert_eq!(
            parse_def(&format!("\u{feff}{crlf}")).unwrap().name,
            "backend"
        );
    }

    #[test]
    fn tools_quotes_blocks_and_lists_are_understood() {
        let text = "---\nname: 'researcher'\ndescription: >\n  Finds facts\n  with sources.\ntools: Read, Grep,  Glob\n# a comment\nmodel: \"sonnet\" \n---\nBody";
        let parsed = parse_def(text).unwrap();
        assert_eq!(parsed.name, "researcher");
        assert_eq!(parsed.description, "Finds facts with sources.");
        assert_eq!(parsed.tools, ["Read", "Grep", "Glob"]);
        assert_eq!(parsed.model.as_deref(), Some("sonnet"));

        let listed = "---\nname: tester\ndescription: |\n  Line one\n  Line two\ntools:\n  - Read\n  - \"Bash\"\n---\n";
        let parsed = parse_def(listed).unwrap();
        assert_eq!(parsed.description, "Line one\nLine two");
        assert_eq!(parsed.tools, ["Read", "Bash"]);

        let inline =
            "---\nname: x\ndescription: \"Say \\\"hi\\\"\" # note\ntools: [Read, 'Edit']\n---\n";
        let parsed = parse_def(inline).unwrap();
        assert_eq!(parsed.description, "Say \"hi\"");
        assert_eq!(parsed.tools, ["Read", "Edit"]);

        let continued =
            "---\nname: y\ndescription: First part\n  second part # not a key: here\n---\n";
        assert_eq!(
            parse_def(continued).unwrap().description,
            "First part second part"
        );
    }

    #[test]
    fn fields_beyond_the_prompt_are_named() {
        let text = "---\nname: x\ndescription: d\nhooks:\n  PreToolUse:\n    - matcher: Bash\nmcpServers:\n  - files\npermissionMode: acceptEdits\n---\n";
        assert_eq!(
            parse_def(text).unwrap().extras,
            ["hooks", "mcpServers", "permissionMode"]
        );
        assert!(parse_def(BACKEND).unwrap().extras.is_empty());
    }

    #[test]
    fn broken_definitions_name_their_problem() {
        assert_eq!(parse_def("no frontmatter"), Err(Problem::NoFrontmatter));
        assert_eq!(
            parse_def("---\nname: x\ndescription: open"),
            Err(Problem::NoFrontmatter)
        );
        assert_eq!(
            parse_def("---\ndescription: d\n---\n"),
            Err(Problem::MissingName)
        );
        assert_eq!(
            parse_def("---\nname: Big Name\ndescription: d\n---\n"),
            Err(Problem::InvalidName)
        );
        assert_eq!(
            parse_def("---\nname: x\ndescription:\n---\n"),
            Err(Problem::MissingDescription)
        );
        // A model value with odd characters is dropped, not taken.
        let odd = parse_def("---\nname: x\ndescription: d\nmodel: a b; c\n---\n").unwrap();
        assert_eq!(odd.model, None);
    }

    #[test]
    fn long_descriptions_are_cut_for_the_window() {
        let text = format!("---\nname: x\ndescription: {}\n---\n", "ä".repeat(3000));
        let description = parse_def(&text).unwrap().description;
        assert_eq!(description.chars().count(), MAX_DESCRIPTION_CHARS + 1);
        assert!(description.ends_with('…'));
    }

    fn write(dir: &Path, name: &str, text: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), text).unwrap();
    }

    #[test]
    fn the_library_lists_global_then_project_definitions() {
        let temp = TempDir::new("agent-defs-library");
        let home = temp.path().join("home");
        let project = temp.path().join("my project");
        let global = agents_dir(&home);
        write(&global, "backend.md", BACKEND);
        write(&global, "broken.md", "just text");
        write(&global, "notes.txt", "ignored");
        write(
            &global,
            "copy.md",
            &BACKEND.replace("You are", "Another file, same name:"),
        );
        write(
            &agents_dir(&project),
            "reviewer.md",
            "---\nname: reviewer\ndescription: Reviews.\n---\n",
        );
        fs::write(global.join("huge.md"), vec![b'a'; MAX_FILE_BYTES + 1]).unwrap();
        fs::write(global.join("binary.md"), [0xff, 0xfe, 0x00]).unwrap();

        let missing = temp.path().join("missing");
        let library = library(
            &home,
            &[
                project.clone(),
                project.clone(),
                missing,
                home.clone(),
                PathBuf::from("relative"),
            ],
        );
        let names: Vec<(&str, Scope)> = library
            .defs
            .iter()
            .map(|def| (def.name.as_str(), def.scope))
            .collect();
        assert_eq!(
            names,
            [("backend", Scope::Global), ("reviewer", Scope::Project)]
        );
        assert_eq!(library.defs[1].folder.as_deref(), Some(project.as_path()));
        assert_eq!(library.defs[0].file, "backend.md");
        let problems: Vec<(&str, Problem)> = library
            .problems
            .iter()
            .map(|problem| (problem.file.as_str(), problem.problem))
            .collect();
        // "backend.md" sorts first, so it holds the name and "copy.md" is the duplicate.
        assert!(problems.contains(&("copy.md", Problem::Duplicate)));
        assert!(problems.contains(&("binary.md", Problem::NotText)));
        assert!(problems.contains(&("broken.md", Problem::NoFrontmatter)));
        assert!(problems.contains(&("huge.md", Problem::TooLarge)));
        assert_eq!(
            problems
                .iter()
                .filter(|(_, problem)| *problem == Problem::Duplicate)
                .count(),
            1
        );
    }

    #[test]
    fn too_many_files_are_reported_once() {
        let temp = TempDir::new("agent-defs-many");
        let dir = agents_dir(temp.path());
        fs::create_dir_all(&dir).unwrap();
        for index in 0..=MAX_FILES {
            fs::write(dir.join(format!("a{index:04}.md")), "x").unwrap();
        }
        let library = library(temp.path(), &[]);
        let too_many = library
            .problems
            .iter()
            .filter(|problem| problem.problem == Problem::TooMany)
            .count();
        assert_eq!(too_many, 1);
        assert_eq!(library.problems.len(), MAX_FILES + 1);
    }

    #[test]
    fn creating_writes_once_and_replaces_only_when_asked() {
        let temp = TempDir::new("agent-defs-create");
        let home = temp.path();
        let made = create(home, BACKEND, false).unwrap();
        assert_eq!(made.name, "backend");
        assert_eq!(made.scope, Scope::Global);
        let path = agents_dir(home).join("backend.md");
        assert_eq!(fs::read_to_string(&path).unwrap(), BACKEND);

        let changed = BACKEND.replace("opus", "sonnet");
        let refused = create(home, &changed, false).unwrap_err();
        assert!(matches!(refused, DefError::Exists(ref name) if name == "backend"));
        assert_eq!(fs::read_to_string(&path).unwrap(), BACKEND);
        create(home, &changed, true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), changed);

        // Replacing keeps the existing file of that name, whatever it is called.
        fs::rename(&path, agents_dir(home).join("server.md")).unwrap();
        let again = create(home, BACKEND, true).unwrap();
        assert_eq!(again.file, "server.md");
        assert!(!path.exists());

        // CRLF becomes LF and a missing last line break is added.
        let pasted = "---\r\nname: writer\r\ndescription: Writes.\r\n---\r\nText";
        create(home, pasted, false).unwrap();
        assert_eq!(
            fs::read_to_string(agents_dir(home).join("writer.md")).unwrap(),
            "---\nname: writer\ndescription: Writes.\n---\nText\n"
        );
    }

    #[test]
    fn creating_refuses_what_is_no_definition() {
        let temp = TempDir::new("agent-defs-invalid");
        let error = create(temp.path(), "hello", false).unwrap_err();
        assert!(matches!(error, DefError::Invalid(Problem::NoFrontmatter)));
        let error = create(temp.path(), &"x".repeat(MAX_FILE_BYTES + 1), false).unwrap_err();
        assert!(matches!(error, DefError::TooLarge(_)));
        for device in ["con", "nul", "com1", "lpt9"] {
            let text = BACKEND.replace("name: backend", &format!("name: {device}"));
            assert!(matches!(
                create(temp.path(), &text, true),
                Err(DefError::Reserved(_))
            ));
        }
        assert!(!is_device_name("com10") && !is_device_name("console"));
    }

    #[test]
    fn a_file_that_only_holds_the_name_is_kept_and_a_free_name_taken() {
        let temp = TempDir::new("agent-defs-free-name");
        let dir = agents_dir(temp.path());
        // Broken, and another case of the name: neither is a definition named "backend".
        write(&dir, "backend.md", "broken");
        write(
            &dir,
            "BACKEND-2.md",
            "---\nname: other\ndescription: d\n---\n",
        );
        let made = create(temp.path(), BACKEND, false).unwrap();
        assert_eq!(made.file, "backend-3.md");
        assert_eq!(
            fs::read_to_string(dir.join("backend.md")).unwrap(),
            "broken"
        );
        assert_eq!(
            fs::read_to_string(dir.join("backend-3.md")).unwrap(),
            BACKEND
        );
        // Now a definition of that name exists: asked first, then replaced in its own file.
        assert!(matches!(
            create(temp.path(), BACKEND, false),
            Err(DefError::Exists(_))
        ));
        let changed = BACKEND.replace("opus", "haiku");
        assert_eq!(
            create(temp.path(), &changed, true).unwrap().file,
            "backend-3.md"
        );
        assert_eq!(
            fs::read_to_string(dir.join("backend-3.md")).unwrap(),
            changed
        );
        // No temporary file stays behind.
        let left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_new_file_never_replaces_one() {
        let temp = TempDir::new("agent-defs-create-new");
        let source = temp.path().join("source");
        let target = temp.path().join("target.md");
        fs::write(&source, "new").unwrap();
        fs::write(&target, "old").unwrap();
        let error = link_new(&source, &target, b"new").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
        let fresh = temp.path().join("fresh.md");
        link_new(&source, &fresh, b"new").unwrap();
        assert_eq!(fs::read_to_string(&fresh).unwrap(), "new");
        // NTFS ignores case: another spelling of a taken name is taken as well.
        #[cfg(windows)]
        {
            let error = link_new(&source, &temp.path().join("TARGET.md"), b"new").unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read_to_string(&target).unwrap(), "old");
        }
    }

    #[test]
    fn errors_reach_the_window_with_a_kind() {
        let json = serde_json::to_value(DefError::Invalid(Problem::MissingName)).unwrap();
        assert_eq!(json["kind"], "invalid");
        assert_eq!(json["problem"], "missingName");
        let json = serde_json::to_value(DefError::Exists("x".into())).unwrap();
        assert_eq!(json["kind"], "exists");
        assert_eq!(json["problem"], serde_json::Value::Null);
    }

    #[test]
    fn deleting_and_finding_work_by_name() {
        let temp = TempDir::new("agent-defs-delete");
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        create(&home, BACKEND, false).unwrap();
        write(
            &agents_dir(&project),
            "r.md",
            "---\nname: reviewer\ndescription: Reviews.\n---\n",
        );
        assert_eq!(
            def_path(&home, "backend", None).unwrap(),
            agents_dir(&home).join("backend.md")
        );
        assert_eq!(
            def_path(&home, "reviewer", Some(&project)).unwrap(),
            agents_dir(&project).join("r.md")
        );
        assert!(def_path(&home, "reviewer", None).is_err());
        assert!(def_path(&home, "reviewer", Some(Path::new("relative"))).is_err());
        assert!(matches!(
            delete(&home, "reviewer"),
            Err(DefError::NotFound(_))
        ));
        delete(&home, "backend").unwrap();
        assert!(library(&home, &[]).defs.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn links_are_not_followed() {
        let temp = TempDir::new("agent-defs-link");
        let home = temp.path().join("home");
        let elsewhere = temp.path().join("elsewhere.md");
        fs::write(&elsewhere, BACKEND).unwrap();
        let dir = agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join("backend.md")).unwrap();
        let library = library(&home, &[]);
        assert!(library.defs.is_empty());
        assert_eq!(library.problems[0].problem, Problem::Link);
        // The link is no definition: a new file is made next to it, the link's target stays.
        let changed = BACKEND.replace("opus", "haiku");
        assert_eq!(create(&home, &changed, true).unwrap().file, "backend-2.md");
        assert_eq!(fs::read_to_string(&elsewhere).unwrap(), BACKEND);
    }
}
