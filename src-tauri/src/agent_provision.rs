//! Hands the user's Claude Code sub-agent definitions (`~/.claude/agents/*.md`, global) to the
//! other installed agent CLIs whose format is measured: a copy per definition in the program's
//! own format and folder. Codex CLI (`~/.codex/agents/<name>.toml`, [`crate::codex_agents`]),
//! GitHub Copilot CLI, OpenCode 2.x and Gemini CLI (Markdown, [`crate::md_agents`]).
//! Contract: docs/wiki/pages/syntheses/sub-agent-library.md, "Provision to other programs".
//!
//! Rules:
//! - A copy carries a marker ([`crate::codex_agents::marker`], [`MdFormat::marker`]); Hangar
//!   changes and removes only files with an intact marker.
//! - A file of the user's under a definition's name (its file name or its `name`) is replaced:
//!   first set aside as `<name>.toml.bak` (`<name>.agent.md.bak`, `<name>.md.bak`; `.bak.2` and
//!   so on, never overwriting one), which the copy's marker names. When the definition goes, the
//!   copy goes and the backup takes its place again.
//! - Global folder: a missing copy is created. Project folders, Codex CLI only (start places and
//!   working folders of sessions, as the backend knows them): only files that already hold a
//!   definition's name (file name or `name`, never the marker's source) and that git does not
//!   track are replaced; no new file is created, other names stay untouched, and a link in
//!   `.codex` or `.codex/agents` skips the folder. The Markdown programs' project folders stay
//!   untouched: Copilot CLI ranks its user folder first anyway, and OpenCode's and Gemini CLI's
//!   project agents belong to the repository.
//! - Model and effort of a Codex CLI copy are kept when it is written again; those of a replaced
//!   file are taken over. The Markdown copies carry no model.
//! - Claude Code's `tools` and `model` are not copied: the other programs name their tools
//!   differently and Claude Code's model aliases mean nothing there.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::agent_defs::{self, Library, Scope};
use crate::agent_models::DefProgram;
use crate::codex_agents::{self, Copy};
use crate::md_agents::MdFormat;
use crate::no_links::{self, Links};

/// How one program's copies look and where they go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Codex,
    Md(MdFormat),
}

impl Format {
    fn of(program: DefProgram) -> Option<Self> {
        match program {
            DefProgram::Claude => None,
            DefProgram::Codex => Some(Self::Codex),
            other => MdFormat::of(other).map(Self::Md),
        }
    }

    /// The agents folder below `base` (the home folder or a project).
    fn dir(self, base: &Path) -> PathBuf {
        match self {
            Self::Codex => codex_agents::agents_dir(base),
            Self::Md(md) => md.agents_dir(base),
        }
    }

    fn dir_label(self) -> &'static str {
        match self {
            Self::Codex => ".codex/agents",
            Self::Md(md) => md.dir_label(),
        }
    }

    /// The ending of a copy's file name.
    fn suffix(self) -> &'static str {
        match self {
            Self::Codex => ".toml",
            Self::Md(md) => md.suffix(),
        }
    }

    /// Whether project folders get copies as well.
    fn projects(self) -> bool {
        self == Self::Codex
    }

    /// Whether the program knows an agent by the name inside the file only (Gemini CLI).
    fn names_inside(self) -> bool {
        matches!(self, Self::Md(md) if md.names_inside())
    }

    /// The agent a file stands for by its name; `None` for a file the program does not read.
    fn stem(self, file: &str) -> Option<&str> {
        match self {
            Self::Codex => Some(stem(file)),
            Self::Md(md) => md.stem(file),
        }
    }

    fn marker(self, text: &str) -> Option<codex_agents::Marker> {
        match self {
            Self::Codex => codex_agents::marker(text),
            Self::Md(md) => md.marker(text),
        }
    }

    fn marker_unchecked(self, text: &str) -> Option<codex_agents::Marker> {
        match self {
            Self::Codex => codex_agents::marker_unchecked(text),
            Self::Md(md) => md.marker_unchecked(text),
        }
    }
}

/// The oldest version whose sub-agent format was measured, for the programs that need one
/// (docs/wiki/pages/sources/sub-agent-formats-measurement.md). OpenCode 1.x reads another
/// folder layout that was never measured, so it gets no copies.
pub fn min_version(program: DefProgram) -> Option<&'static str> {
    match program {
        DefProgram::Copilot => Some("1.0.114"),
        DefProgram::Opencode => Some("2.3.2"),
        DefProgram::Gemini => Some("0.70.0"),
        DefProgram::Claude | DefProgram::Codex => None,
    }
}

/// Whether the installed `version` of `program` reaches [`min_version`]; an unknown version does
/// not, and a program without a floor always does.
pub fn version_reaches(program: DefProgram, version: Option<&str>) -> bool {
    use crate::agent_catalog::MinVersion;
    let Some(min) = min_version(program).and_then(MinVersion::parse) else {
        return true;
    };
    version
        .and_then(MinVersion::parse)
        .is_some_and(|installed| installed >= min)
}

/// Why a copy could not be written or a file was left alone, as the window words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    /// The file there cannot be read (too large, no UTF-8, access denied).
    Unreadable,
    /// Writing, setting aside or removing failed.
    Io,
    /// The agents folder lies on another computer or a device.
    OffMachine,
    /// Every backup name up to `.bak.100` is taken.
    NoBackupName,
    /// A project file git tracks holds the name: it stays and outranks the global copy there.
    Tracked,
    /// `.codex` or `.codex/agents` of a project is a link; the folder is skipped.
    Link,
    /// A file of the copy's file name holds another agent's name (Gemini CLI knows agents by
    /// `name`): it stays the user's, and no copy is written until it is renamed.
    NameTaken,
}

/// One file or folder a run left as it was, and why. Names only, no message text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub program: DefProgram,
    pub scope: Scope,
    pub folder: Option<PathBuf>,
    pub file: String,
    /// The definition it concerns, when known.
    pub name: Option<String>,
    pub kind: IssueKind,
}

/// What one run did; failures are reported, never fatal for the others.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Copies created or written again.
    pub written: Vec<PathBuf>,
    /// User files set aside before their copy took their place: (file, backup).
    pub replaced: Vec<(PathBuf, PathBuf)>,
    /// Copies removed (their definition is gone or the program is switched off).
    pub removed: Vec<PathBuf>,
    pub issues: Vec<Issue>,
}

/// One Claude Code definition as a copy needs it.
struct Source {
    name: String,
    description: String,
    instructions: String,
    /// Hangar's own unchanged bundled copy ([`crate::bundled_agents`]): the user never asked
    /// for it, so its copy only takes a free name in the global folder, never sets a user's file
    /// aside and never reaches a project.
    bundled: bool,
}

fn sources(home: &Path, bundled: &[String]) -> Vec<Source> {
    agent_defs::global_sources(home)
        .into_iter()
        .filter_map(|(name, path)| {
            let text = agent_defs::read_capped(&path).ok()?;
            let (description, instructions) = agent_defs::source_parts(&text)?;
            Some(Source {
                bundled: bundled.contains(&name),
                name,
                description,
                instructions,
            })
        })
        .collect()
}

/// One run at a time. The app start, the window and a switch in Settings can each start a run,
/// and two runs removing the same copy would delete the user's file the first one put back.
static RUN: Mutex<()> = Mutex::new(());

/// Held by every run and by a model change of a copy, which a run would otherwise write over.
pub(crate) fn run_lock() -> MutexGuard<'static, ()> {
    RUN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The issues of each program's newest run, for the listing.
static LAST: Mutex<BTreeMap<DefProgram, Vec<Issue>>> = Mutex::new(BTreeMap::new());

/// The issues of the newest run of every program.
pub fn last_issues() -> Vec<Issue> {
    LAST.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .flatten()
        .cloned()
        .collect()
}

/// The programs Hangar hands definitions to.
pub const TARGETS: [DefProgram; 4] = [
    DefProgram::Codex,
    DefProgram::Copilot,
    DefProgram::Opencode,
    DefProgram::Gemini,
];

/// Brings the copies of `program` up to date: in the global folder and in the agents folders of
/// `folders` that exist. `enabled = false` removes Hangar's copies and restores the backups.
pub fn sync(home: &Path, folders: &[PathBuf], program: DefProgram, enabled: bool) -> Summary {
    sync_with(home, folders, program, enabled, &[])
}

/// [`sync`] where the definitions named in `bundled` are Hangar's unchanged bundled copies
/// ([`Source::bundled`]).
pub fn sync_with(
    home: &Path,
    folders: &[PathBuf],
    program: DefProgram,
    enabled: bool,
    bundled: &[String],
) -> Summary {
    let mut summary = Summary::default();
    let Some(format) = Format::of(program) else {
        return summary;
    };
    let _run = run_lock();
    let sources = if enabled {
        sources(home, bundled)
    } else {
        Vec::new()
    };
    let global = Place {
        program,
        format,
        dir: format.dir(home),
        scope: Scope::Global,
        folder: None,
    };
    let global_ready = sources.is_empty()
        || match ensure_dir(&global.dir) {
            Ok(()) => true,
            Err(kind) => {
                global.issue(&mut summary, format.dir_label(), None, kind);
                false
            }
        };
    if global_ready {
        sync_dir(&global, &sources, &mut summary);
    }
    let projects = if format.projects() {
        agent_defs::project_folders(home, folders)
    } else {
        Vec::new()
    };
    for folder in projects {
        let place = Place {
            program,
            format,
            dir: format.dir(folder),
            scope: Scope::Project,
            folder: Some(folder),
        };
        match project_dir(folder) {
            ProjectDir::Missing => {}
            ProjectDir::Link => place.issue(&mut summary, ".codex", None, IssueKind::Link),
            ProjectDir::Ready => sync_dir(&place, &sources, &mut summary),
        }
    }
    for issue in &summary.issues {
        eprintln!(
            "hangar: sub-agent copy left alone: {} ({:?})",
            issue.file, issue.kind
        );
    }
    LAST.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(program, summary.issues.clone());
    summary
}

/// [`sync`] for every target that is installed: on unless switched off (`off`). A program that
/// is not installed gets no folder and keeps whatever is there.
pub fn sync_installed(
    home: &Path,
    folders: &[PathBuf],
    installed: &[DefProgram],
    off: &BTreeSet<DefProgram>,
    bundled: &[String],
) -> Vec<Summary> {
    targets(installed, off)
        .into_iter()
        .map(|target| sync_with(home, folders, target.program, target.enabled, bundled))
        .collect()
}

/// The folders of `requested` that the backend knows itself (`known`: the sessions' working
/// folders and the stored start places), compared the way the file system does. The window can
/// narrow the folders a run touches, never widen them. Each match goes on as the known path, not
/// as requested: the comparison folds case by Unicode rules, so a requested spelling could match
/// and still name another folder.
pub fn permitted(requested: &[PathBuf], known: &[PathBuf]) -> Vec<PathBuf> {
    let known: HashMap<String, &PathBuf> = known
        .iter()
        .map(|path| (crate::git::path_key(path), path))
        .collect();
    requested
        .iter()
        .filter_map(|path| {
            known
                .get(&crate::git::path_key(path))
                .map(|&path| path.clone())
        })
        .collect()
}

/// The global agents folder of a program, created if missing; refused when it lies off this
/// machine or is no folder.
fn ensure_dir(dir: &Path) -> Result<(), IssueKind> {
    if no_links::is_network_or_device(dir) {
        return Err(IssueKind::OffMachine);
    }
    fs::create_dir_all(dir).map_err(|_| IssueKind::Io)?;
    match no_links::metadata(dir, Links::FollowLocal) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(IssueKind::Io),
        Err(no_links::NoLinksError::Remote(_)) => Err(IssueKind::OffMachine),
        Err(_) => Err(IssueKind::Io),
    }
}

enum ProjectDir {
    Missing,
    Link,
    Ready,
}

/// `<folder>/.codex/agents`, checked from the project folder down: neither part may be a link
/// (a repository could point it anywhere), and a project off this machine is skipped.
fn project_dir(folder: &Path) -> ProjectDir {
    if no_links::is_network_or_device(folder) {
        return ProjectDir::Missing;
    }
    let codex = folder.join(".codex");
    for part in [codex.clone(), codex.join("agents")] {
        match fs::symlink_metadata(&part) {
            Ok(meta) if no_links::is_reparse_point(&meta) => return ProjectDir::Link,
            Ok(meta) if meta.is_dir() => {}
            _ => return ProjectDir::Missing,
        }
    }
    ProjectDir::Ready
}

/// Whether git tracks `path` in the repository of `folder`. No repository: not tracked. A git
/// that cannot be asked, or an answer that is neither yes nor no, counts as tracked: a
/// repository's file is then left alone.
fn tracked(folder: &Path, path: &Path) -> bool {
    if crate::git::repo_place_of(folder, None).is_none() {
        return false;
    }
    let Ok(relative) = path.strip_prefix(folder) else {
        return true;
    };
    let Ok(git) = crate::git::Git::locate() else {
        return true;
    };
    let args: [&OsStr; 4] = [
        OsStr::new("ls-files"),
        OsStr::new("--error-unmatch"),
        OsStr::new("--"),
        relative.as_os_str(),
    ];
    let deadline = Instant::now() + Duration::from_secs(10);
    match git.run(folder, args, deadline, 64 * 1024) {
        Ok(output) => output.code != Some(1),
        Err(_) => true,
    }
}

/// One agents folder of a run.
struct Place<'a> {
    program: DefProgram,
    format: Format,
    dir: PathBuf,
    scope: Scope,
    folder: Option<&'a Path>,
}

impl Place<'_> {
    fn issue(&self, summary: &mut Summary, file: &str, name: Option<&str>, kind: IssueKind) {
        summary.issues.push(Issue {
            program: self.program,
            scope: self.scope,
            folder: self.folder.map(Path::to_owned),
            file: file.to_owned(),
            name: name.map(str::to_owned),
            kind,
        });
    }

    /// A project file git tracks belongs to the repository.
    fn tracked(&self, path: &Path) -> bool {
        self.folder.is_some_and(|folder| tracked(folder, path))
    }
}

/// A file of an agents folder and what it holds.
struct Entry {
    file: String,
    path: PathBuf,
    /// The agent name it holds, when it is a readable definition.
    name: Option<String>,
    marker: Option<codex_agents::Marker>,
}

fn stem(file: &str) -> &str {
    file.strip_suffix(".toml")
        .or_else(|| file.strip_suffix(".TOML"))
        .unwrap_or(file)
}

/// Every file of the folder the program reads that is no link, with what it holds; broken files
/// count by their file name.
fn entries(place: &Place<'_>) -> Vec<Entry> {
    match place.format {
        Format::Codex => codex_entries(place),
        Format::Md(md) => md_entries(place, md),
    }
}

/// Files of a Markdown agents folder read at most.
const MAX_MD_FILES: usize = 512;

/// Every file of a Markdown agents folder the program reads, with the agent it stands for. A
/// file that cannot be read still counts by its file name.
fn md_entries(place: &Place<'_>, md: MdFormat) -> Vec<Entry> {
    if no_links::is_network_or_device(&place.dir) {
        return Vec::new();
    }
    let Ok(listing) = fs::read_dir(&place.dir) else {
        return Vec::new();
    };
    let mut files: Vec<String> = listing
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|file| md.stem(file).is_some())
        .collect();
    files.sort();
    files.truncate(MAX_MD_FILES);
    files
        .into_iter()
        .filter_map(|file| {
            let path = place.dir.join(&file);
            let meta = fs::symlink_metadata(&path).ok()?;
            if no_links::is_reparse_point(&meta) || !meta.is_file() {
                return None;
            }
            let text = agent_defs::read_capped(&path).ok();
            // An unreadable file holds no name; it still counts by its file name below.
            let name = text.as_deref().and_then(|text| md.name_of(&file, text));
            let marker = text.as_deref().and_then(|text| md.marker(text));
            Some(Entry {
                file,
                path,
                name,
                marker,
            })
        })
        .collect()
}

/// Every `.toml` file of a Codex CLI agents folder that is no link.
fn codex_entries(place: &Place<'_>) -> Vec<Entry> {
    let mut library = Library::default();
    let read = agent_defs::scan_codex_dir(&place.dir, place.scope, place.folder, &mut library);
    let mut out: Vec<Entry> = read
        .into_iter()
        .map(|read| Entry {
            file: read.file,
            path: read.path,
            name: Some(read.parsed.name),
            marker: read.parsed.managed,
        })
        .collect();
    // Broken files and second files of a name are still the user's files under that name.
    for problem in library.problems {
        if problem.problem == agent_defs::Problem::Link {
            continue;
        }
        let path = place.dir.join(&problem.file);
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
            out.push(Entry {
                file: problem.file,
                path,
                name: None,
                marker: None,
            });
        }
    }
    out
}

fn sync_dir(place: &Place<'_>, sources: &[Source], summary: &mut Summary) {
    let entries = entries(place);
    let wanted: HashSet<&str> = sources.iter().map(|source| source.name.as_str()).collect();
    let mut handled = HashSet::new();
    for source in sources {
        if source.bundled && place.scope != Scope::Global {
            continue;
        }
        let by_stem = |entry: &Entry| {
            place
                .format
                .stem(&entry.file)
                .is_some_and(|stem| stem.eq_ignore_ascii_case(&source.name))
        };
        let by_marker = |entry: &Entry| {
            entry
                .marker
                .as_ref()
                .is_some_and(|marker| marker.source == source.name)
        };
        let matching: Vec<&Entry> = entries
            .iter()
            .filter(|entry| {
                // Gemini CLI knows an agent only by its `name`: a file of the definition's file
                // name counts only while it holds no valid name of its own.
                let stem_counts = !place.format.names_inside() || entry.name.is_none();
                let by_name = entry.name.as_deref() == Some(source.name.as_str())
                    || (stem_counts && by_stem(entry));
                // In a project only what the file is called counts: a repository could forge a
                // marker to steer a copy into any of its files.
                by_name || (place.scope == Scope::Global && by_marker(entry))
            })
            .collect();
        if matching.is_empty() {
            // The copy's file name is held by another agent of the user's: left alone, named.
            let taken = entries.iter().find(|entry| {
                place.format.names_inside()
                    && by_stem(entry)
                    && entry.name.is_some()
                    && !by_marker(entry)
            });
            match taken {
                // Hangar's own bundled agent never troubles the user over a taken name.
                Some(_) if source.bundled => {}
                Some(entry) => {
                    place.issue(
                        summary,
                        &entry.file,
                        Some(&source.name),
                        IssueKind::NameTaken,
                    );
                }
                None if place.scope == Scope::Global => create(place, source, summary),
                None => {}
            }
            continue;
        }
        for entry in matching {
            if handled.insert(entry.path.clone()) {
                update(place, entry, source, summary);
            }
        }
    }
    // Copies whose definition is gone: removed, the backup back in place.
    for entry in &entries {
        let Some(marker) = &entry.marker else {
            continue;
        };
        if wanted.contains(marker.source.as_str()) || handled.contains(&entry.path) {
            continue;
        }
        if place.tracked(&entry.path) {
            continue;
        }
        remove(place, entry, marker, summary);
    }
}

fn copy_text(
    format: Format,
    source: &Source,
    model: Option<&str>,
    effort: Option<&str>,
    backup: Option<&str>,
) -> String {
    match format {
        Format::Codex => codex_agents::render(&Copy {
            name: &source.name,
            description: &source.description,
            instructions: &source.instructions,
            model,
            effort,
            backup,
        }),
        Format::Md(md) => md.render(
            &source.name,
            &source.description,
            &source.instructions,
            backup,
        ),
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!(
        ".{name}.{}.{}.new",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// A new copy `<name>.toml` (`<name>.agent.md`, `<name>.md`), never replacing a file that
/// appeared meanwhile.
fn create(place: &Place<'_>, source: &Source, summary: &mut Summary) {
    let file = format!("{}{}", source.name, place.format.suffix());
    let path = place.dir.join(&file);
    let text = copy_text(place.format, source, None, None, None);
    let temp = temp_path(&place.dir, &source.name);
    let result = crate::persist::write_atomic(&temp, text.as_bytes())
        .and_then(|()| agent_defs::create_beside(&temp, &path, text.as_bytes()));
    let _ = fs::remove_file(&temp);
    match result {
        Ok(()) => summary.written.push(path),
        Err(_) => place.issue(summary, &file, Some(&source.name), IssueKind::Io),
    }
}

/// Backups tried per file.
const BACKUP_ATTEMPTS: usize = 100;

/// The first free `<name><suffix>.bak`, `<name><suffix>.bak.2` and so on (named after the
/// definition, so the marker can always name it; no program reads a `.bak` file): the file at
/// `path` is hard linked there (or copied where hard links do not exist), so its bytes stay
/// whatever happens next.
fn set_aside(path: &Path, name: &str, suffix: &str, bytes: &[u8]) -> Result<String, IssueKind> {
    let dir = path.parent().unwrap_or(Path::new("."));
    for n in 1..=BACKUP_ATTEMPTS {
        let backup = match n {
            1 => format!("{name}{suffix}.bak"),
            n => format!("{name}{suffix}.bak.{n}"),
        };
        match agent_defs::create_beside(path, &dir.join(&backup), bytes) {
            Ok(()) => return Ok(backup),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(IssueKind::Io),
        }
    }
    Err(IssueKind::NoBackupName)
}

/// Writes the copy of `source` over `entry`: again when it is Hangar's own, after setting the
/// user's file aside otherwise.
fn update(place: &Place<'_>, entry: &Entry, source: &Source, summary: &mut Summary) {
    let name = Some(source.name.as_str());
    if place.tracked(&entry.path) {
        place.issue(summary, &entry.file, name, IssueKind::Tracked);
        return;
    }
    let Ok(text) = agent_defs::read_capped(&entry.path) else {
        place.issue(summary, &entry.file, name, IssueKind::Unreadable);
        return;
    };
    // Read again: the file may have changed since the folder was listed. Only Codex CLI copies
    // carry a model.
    let parsed = match place.format {
        Format::Codex => codex_agents::parse_def(&text).ok(),
        Format::Md(_) => None,
    };
    let model = parsed
        .as_ref()
        .and_then(|parsed| parsed.model.as_deref())
        .filter(|model| crate::agent_models::valid_model(model));
    let effort = parsed
        .as_ref()
        .and_then(|parsed| parsed.effort.as_deref())
        .filter(|effort| crate::agent_models::valid_effort(effort));
    let format = place.format;
    let own = format
        .marker(&text)
        .filter(|marker| marker.source == source.name);
    let next = match own {
        Some(marker) => copy_text(format, source, model, effort, marker.backup.as_deref()),
        // The user's file of a name Hangar brought along on its own stays theirs.
        None if source.bundled => return,
        None => {
            let backup =
                match set_aside(&entry.path, &source.name, format.suffix(), text.as_bytes()) {
                    Ok(backup) => backup,
                    Err(kind) => {
                        place.issue(summary, &entry.file, name, kind);
                        return;
                    }
                };
            summary
                .replaced
                .push((entry.path.clone(), place.dir.join(&backup)));
            // A copy edited by hand keeps naming the user's original, which then comes back
            // when the copy goes; the edited text stays as the new backup.
            let original = format
                .marker_unchecked(&text)
                .filter(|marker| marker.source == source.name)
                .and_then(|marker| marker.backup)
                .filter(|original| place.dir.join(original).is_file());
            copy_text(
                format,
                source,
                model,
                effort,
                Some(original.as_deref().unwrap_or(&backup)),
            )
        }
    };
    if next == text {
        return;
    }
    match crate::persist::write_atomic(&entry.path, next.as_bytes()) {
        Ok(()) => summary.written.push(entry.path.clone()),
        Err(_) => place.issue(summary, &entry.file, name, IssueKind::Io),
    }
}

/// Removes a copy and puts the user's file set aside for it back in its place.
fn remove(place: &Place<'_>, entry: &Entry, marker: &codex_agents::Marker, summary: &mut Summary) {
    let name = Some(marker.source.as_str());
    // Read again: a copy edited since the folder was listed is the user's file now.
    let still_own = agent_defs::read_capped(&entry.path)
        .ok()
        .and_then(|text| place.format.marker(&text))
        .is_some_and(|fresh| fresh == *marker);
    if !still_own {
        return;
    }
    if fs::remove_file(&entry.path).is_err() {
        place.issue(summary, &entry.file, name, IssueKind::Io);
        return;
    }
    summary.removed.push(entry.path.clone());
    let Some(backup) = &marker.backup else { return };
    let backup_path = place.dir.join(backup);
    let plain = fs::symlink_metadata(&backup_path).is_ok_and(|meta| meta.is_file());
    if plain && !entry.path.exists() && fs::rename(&backup_path, &entry.path).is_err() {
        place.issue(summary, backup, name, IssueKind::Io);
    }
}

/// The copy of each definition per program, for the window: which programs Hangar hands
/// definitions to on this machine, and whether that is on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetView {
    pub program: DefProgram,
    pub enabled: bool,
}

/// One of Hangar's copies in a Markdown program's global folder, for the window: which file and
/// which backups of the user's files lie beside it. Names only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MdCopy {
    pub program: DefProgram,
    /// The definition the copy was made from.
    pub name: String,
    pub file: String,
    /// The backup its marker names: the user's file it replaced.
    pub backup: Option<String>,
    /// Every backup of its definition beside it, sorted.
    pub backups: Vec<String>,
}

/// Backups listed per copy at most.
const MAX_BACKUPS_LISTED: usize = 16;

/// Hangar's copies (intact marker) in the global folders of the Markdown programs among
/// `programs`, with their backups. Codex CLI's copies come with the library's `codexDefs`.
pub fn md_copies(home: &Path, programs: &[DefProgram]) -> Vec<MdCopy> {
    let mut out = Vec::new();
    for &program in programs {
        let Some(Format::Md(md)) = Format::of(program) else {
            continue;
        };
        let place = Place {
            program,
            format: Format::Md(md),
            dir: md.agents_dir(home),
            scope: Scope::Global,
            folder: None,
        };
        let entries = md_entries(&place, md);
        if entries.iter().all(|entry| entry.marker.is_none()) {
            continue;
        }
        let files: Vec<String> = fs::read_dir(&place.dir)
            .map(|listing| {
                listing
                    .filter_map(Result::ok)
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        for entry in entries {
            let Some(marker) = entry.marker else {
                continue;
            };
            let mut backups: Vec<String> = files
                .iter()
                .filter(|file| md.backup_name_of(&marker.source, file))
                .take(MAX_BACKUPS_LISTED)
                .cloned()
                .collect();
            backups.sort();
            out.push(MdCopy {
                program,
                name: marker.source,
                file: entry.file,
                backup: marker.backup,
                backups,
            });
        }
    }
    out
}

/// The targets that are installed, with their switch (`off`: switched off in Settings).
pub fn targets(installed: &[DefProgram], off: &BTreeSet<DefProgram>) -> Vec<TargetView> {
    TARGETS
        .iter()
        .filter(|program| installed.contains(program))
        .map(|program| TargetView {
            program: *program,
            enabled: !off.contains(program),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const BACKEND: &str = "---\nname: backend\ndescription: Backend work: logic, APIs\nmodel: opus\ntools: Read, Bash\n---\n\nYou are the backend engineer.\nUse \"quotes\" and C:\\paths.\n";
    const MANAGER: &str = "---\nname: manager\ndescription: Reviews everything\n---\nBe strict.\n";
    const FOREIGN: &str = "name = \"manager\"\ndescription = \"My own manager\"\nmodel = \"gpt-5\"\nmodel_reasoning_effort = \"high\"\ndeveloper_instructions = \"\"\"\nMine.\n\"\"\"\n";

    fn setup(name: &str) -> (TempDir, PathBuf) {
        let temp = TempDir::new(name);
        let home = temp.path().join("home");
        let claude = agent_defs::agents_dir(&home);
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("backend.md"), BACKEND).unwrap();
        fs::write(claude.join("manager.md"), MANAGER).unwrap();
        (temp, home)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn copies_are_created_in_the_global_folder() {
        let (_temp, home) = setup("provision-create");
        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        let dir = codex_agents::agents_dir(&home);
        let text = read(&dir.join("backend.toml"));
        let table: toml::Table = text.parse().unwrap();
        assert_eq!(table["name"].as_str(), Some("backend"));
        assert_eq!(
            table["description"].as_str(),
            Some("Backend work: logic, APIs")
        );
        assert_eq!(
            table["developer_instructions"].as_str(),
            Some("You are the backend engineer.\nUse \"quotes\" and C:\\paths.")
        );
        // Claude Code's model alias and tools mean nothing to Codex CLI.
        assert!(!table.contains_key("model") && !table.contains_key("tools"));
        assert!(codex_agents::marker(&text).is_some());
        // A second run writes nothing.
        assert!(sync(&home, &[], DefProgram::Codex, true).written.is_empty());
    }

    #[test]
    fn a_users_file_is_set_aside_and_replaced() {
        let (_temp, home) = setup("provision-replace");
        let dir = codex_agents::agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manager.toml"), FOREIGN).unwrap();
        fs::write(dir.join("manager.toml.bak"), "older backup").unwrap();
        let other = "name = \"writer\"\ndescription = \"d\"\ndeveloper_instructions = \"x\"\n";
        fs::write(dir.join("writer.toml"), other).unwrap();

        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert_eq!(
            summary.replaced,
            [(dir.join("manager.toml"), dir.join("manager.toml.bak.2"))]
        );
        // The user's file stays byte for byte in the next free backup; the older one stays too.
        assert_eq!(read(&dir.join("manager.toml.bak.2")), FOREIGN);
        assert_eq!(read(&dir.join("manager.toml.bak")), "older backup");
        // Other names stay untouched.
        assert_eq!(read(&dir.join("writer.toml")), other);
        let copy = read(&dir.join("manager.toml"));
        let marker = codex_agents::marker(&copy).unwrap();
        assert_eq!(marker.backup.as_deref(), Some("manager.toml.bak.2"));
        // The user's model choice moves over to the copy.
        let parsed = codex_agents::parse_def(&copy).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("gpt-5"));
        assert_eq!(parsed.effort.as_deref(), Some("high"));
        assert_eq!(parsed.description, "Reviews everything");

        // Now it is Hangar's: the next run neither sets anything aside nor writes.
        let again = sync(&home, &[], DefProgram::Codex, true);
        assert!(again.replaced.is_empty() && again.written.is_empty());
    }

    #[test]
    fn a_copy_edited_by_hand_is_set_aside_again() {
        let (_temp, home) = setup("provision-edited");
        sync(&home, &[], DefProgram::Codex, true);
        let path = codex_agents::agents_dir(&home).join("backend.toml");
        let edited = read(&path).replace("backend engineer", "edited engineer");
        fs::write(&path, &edited).unwrap();
        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert_eq!(summary.replaced.len(), 1);
        assert_eq!(read(&path.with_file_name("backend.toml.bak")), edited);
        assert!(read(&path).contains("backend engineer"));
    }

    #[test]
    fn project_folders_only_get_files_they_have() {
        let (temp, home) = setup("provision-project");
        let project = temp.path().join("my project");
        let dir = codex_agents::agents_dir(&project);
        fs::create_dir_all(&dir).unwrap();
        // A file under another name that holds a definition's name counts as well.
        let own = FOREIGN.replace("\"manager\"", "\"backend\"");
        fs::write(dir.join("lead.toml"), &own).unwrap();
        let other = "name = \"planner\"\ndescription = \"d\"\ndeveloper_instructions = \"x\"\n";
        fs::write(dir.join("planner.toml"), other).unwrap();
        let empty = temp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();

        let summary = sync(
            &home,
            &[project.clone(), empty.clone()],
            DefProgram::Codex,
            true,
        );
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert_eq!(read(&dir.join("backend.toml.bak")), own);
        assert!(codex_agents::marker(&read(&dir.join("lead.toml"))).is_some());
        // No new project file for "manager", none in a folder without agents.
        assert!(!dir.join("manager.toml").exists());
        assert!(!codex_agents::agents_dir(&empty).exists());
        assert_eq!(read(&dir.join("planner.toml")), other);
    }

    #[test]
    fn copies_go_with_their_definition_and_backups_return() {
        let (_temp, home) = setup("provision-remove");
        let dir = codex_agents::agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manager.toml"), FOREIGN).unwrap();
        sync(&home, &[], DefProgram::Codex, true);
        fs::remove_file(agent_defs::agents_dir(&home).join("manager.md")).unwrap();
        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert_eq!(summary.removed, [dir.join("manager.toml")]);
        assert_eq!(read(&dir.join("manager.toml")), FOREIGN);
        assert!(!dir.join("manager.toml.bak").exists());
        assert!(dir.join("backend.toml").exists());

        // Switched off: every copy goes, the user's files stay.
        let off = sync(&home, &[], DefProgram::Codex, false);
        assert_eq!(off.removed, [dir.join("backend.toml")]);
        assert_eq!(read(&dir.join("manager.toml")), FOREIGN);
    }

    #[test]
    fn runs_at_the_same_time_never_lose_a_users_file() {
        let (_temp, home) = setup("provision-parallel");
        let dir = codex_agents::agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manager.toml"), FOREIGN).unwrap();
        sync(&home, &[], DefProgram::Codex, true);
        // Switched off from several places at once: the backup comes back exactly once.
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| sync(&home, &[], DefProgram::Codex, false));
            }
        });
        assert_eq!(read(&dir.join("manager.toml")), FOREIGN);
        assert!(!dir.join("manager.toml.bak").exists());
        assert!(!dir.join("backend.toml").exists());
    }

    #[test]
    fn an_edited_copy_keeps_the_original_backup() {
        let (_temp, home) = setup("provision-edited-original");
        let dir = codex_agents::agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("manager.toml");
        fs::write(&path, FOREIGN).unwrap();
        sync(&home, &[], DefProgram::Codex, true);
        let edited = read(&path).replace("Be strict.", "Be kind.");
        fs::write(&path, &edited).unwrap();
        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert_eq!(
            summary.replaced,
            [(path.clone(), dir.join("manager.toml.bak.2"))]
        );
        let marker = codex_agents::marker(&read(&path)).unwrap();
        assert_eq!(marker.backup.as_deref(), Some("manager.toml.bak"));
        // The listing names both backups.
        let library = agent_defs::library(&home, &[]);
        let copy = library
            .codex_defs
            .iter()
            .find(|d| d.name == "manager")
            .unwrap();
        assert_eq!(copy.backups, ["manager.toml.bak", "manager.toml.bak.2"]);
        // The definition goes: the original comes back, the edited text stays as a backup.
        fs::remove_file(agent_defs::agents_dir(&home).join("manager.md")).unwrap();
        sync(&home, &[], DefProgram::Codex, true);
        assert_eq!(read(&path), FOREIGN);
        assert_eq!(read(&dir.join("manager.toml.bak.2")), edited);
    }

    #[test]
    fn backups_are_named_after_the_definition() {
        let (_temp, home) = setup("provision-backup-name");
        let claude = agent_defs::agents_dir(&home);
        fs::write(
            claude.join("code-reviewer.md"),
            "---\nname: code-reviewer\ndescription: Reviews\n---\nReview.\n",
        )
        .unwrap();
        let dir = codex_agents::agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        let own = FOREIGN.replace("\"manager\"", "\"code-reviewer\"");
        let path = dir.join("code reviewer.toml");
        fs::write(&path, &own).unwrap();
        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert_eq!(read(&dir.join("code-reviewer.toml.bak")), own);
        let marker = codex_agents::marker(&read(&path)).unwrap();
        assert_eq!(marker.backup.as_deref(), Some("code-reviewer.toml.bak"));
        // Hangar's own now: no second backup, no endless chain.
        let again = sync(&home, &[], DefProgram::Codex, true);
        assert!(again.replaced.is_empty(), "{:?}", again.replaced);
        assert!(!dir.join("code-reviewer.toml.bak.2").exists());
    }

    #[test]
    fn an_unreadable_file_is_reported_and_kept() {
        let (_temp, home) = setup("provision-unreadable");
        let dir = codex_agents::agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        let bytes = [0xff, 0xfe, b'x'];
        fs::write(dir.join("manager.toml"), bytes).unwrap();
        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert_eq!(
            summary.issues,
            [Issue {
                program: DefProgram::Codex,
                scope: Scope::Global,
                folder: None,
                file: "manager.toml".into(),
                name: Some("manager".into()),
                kind: IssueKind::Unreadable,
            }]
        );
        assert_eq!(fs::read(dir.join("manager.toml")).unwrap(), bytes);
        let json = serde_json::to_value(&summary.issues[0]).unwrap();
        assert_eq!(json["kind"], "unreadable");
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
    }

    #[test]
    fn tracked_project_files_stay_and_untracked_ones_are_replaced() {
        let (temp, home) = setup("provision-tracked");
        let project = temp.path().join("repo");
        let dir = codex_agents::agents_dir(&project);
        fs::create_dir_all(&dir).unwrap();
        let tracked_text = FOREIGN.replace("\"manager\"", "\"backend\"");
        fs::write(dir.join("backend.toml"), &tracked_text).unwrap();
        fs::write(dir.join("manager.toml"), FOREIGN).unwrap();
        git(&project, &["init", "-q"]);
        git(&project, &["add", ".codex/agents/backend.toml"]);

        let summary = sync(
            &home,
            std::slice::from_ref(&project),
            DefProgram::Codex,
            true,
        );
        assert_eq!(read(&dir.join("backend.toml")), tracked_text);
        assert!(!dir.join("backend.toml.bak").exists());
        let tracked: Vec<_> = summary
            .issues
            .iter()
            .filter(|issue| issue.kind == IssueKind::Tracked)
            .collect();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].name.as_deref(), Some("backend"));
        assert_eq!(tracked[0].folder.as_deref(), Some(project.as_path()));
        // The untracked one is replaced after a backup.
        assert_eq!(read(&dir.join("manager.toml.bak")), FOREIGN);
        assert!(codex_agents::marker(&read(&dir.join("manager.toml"))).is_some());
    }

    #[test]
    fn a_forged_marker_steers_nothing_in_a_project() {
        use sha2::{Digest as _, Sha256};
        let (temp, home) = setup("provision-forged");
        let project = temp.path().join("project");
        let dir = codex_agents::agents_dir(&project);
        fs::create_dir_all(&dir).unwrap();
        let rest = "name = \"planner\"\ndescription = \"d\"\ndeveloper_instructions = \"x\"\n";
        let hash: String = Sha256::digest(rest.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let forged =
            format!("# Managed by Hangar from ~/.claude/agents/backend.md. sha256:{hash}\n{rest}");
        assert!(codex_agents::marker(&forged).is_some());
        fs::write(dir.join("planner.toml"), &forged).unwrap();
        let summary = sync(
            &home,
            std::slice::from_ref(&project),
            DefProgram::Codex,
            true,
        );
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert_eq!(read(&dir.join("planner.toml")), forged);
    }

    #[test]
    fn only_folders_the_backend_knows_are_touched() {
        let known = [PathBuf::from("/work/hangar"), PathBuf::from("/work/site")];
        let asked = [
            PathBuf::from("/work/hangar/"),
            PathBuf::from("/etc"),
            PathBuf::from("/work/site"),
        ];
        assert_eq!(
            permitted(&asked, &known),
            [PathBuf::from("/work/hangar"), PathBuf::from("/work/site")]
        );
        assert!(permitted(&asked, &[]).is_empty());
    }

    /// The comparison folds case, so a requested spelling can match a known folder and still
    /// name another one: the KELVIN SIGN lowercases to `k`, yet NTFS keeps `WorK` and `work`
    /// apart (as a case-sensitive folder keeps `Work` and `work`). Only the known path goes on.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_permitted_folder_is_handed_on_as_the_backend_knows_it() {
        let known = [PathBuf::from("C:/Work/Hangar")];
        let asked = [
            PathBuf::from("c:/WORK/hangar/"),
            PathBuf::from("C:/Wor\u{212A}/Hangar"),
        ];
        assert_eq!(
            permitted(&asked, &known),
            [
                PathBuf::from("C:/Work/Hangar"),
                PathBuf::from("C:/Work/Hangar")
            ]
        );
    }

    /// A link to a folder: a junction on Windows (no privilege needed), a symlink elsewhere.
    fn folder_link(link: &Path, to: &Path) {
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(to)
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "mklink /J failed");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(to, link).unwrap();
    }

    #[test]
    fn a_linked_project_agents_folder_is_skipped() {
        let (temp, home) = setup("provision-link");
        let elsewhere = temp.path().join("elsewhere");
        fs::create_dir_all(elsewhere.join("agents")).unwrap();
        fs::write(elsewhere.join("agents").join("manager.toml"), FOREIGN).unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        folder_link(&project.join(".codex"), &elsewhere);
        let summary = sync(
            &home,
            std::slice::from_ref(&project),
            DefProgram::Codex,
            true,
        );
        assert_eq!(summary.issues.len(), 1);
        assert_eq!(summary.issues[0].kind, IssueKind::Link);
        assert_eq!(
            read(&elsewhere.join("agents").join("manager.toml")),
            FOREIGN
        );
        assert!(!elsewhere.join("agents").join("manager.toml.bak").exists());

        // The same for a link one level down, `.codex/agents`.
        let inner = temp.path().join("inner");
        fs::create_dir_all(inner.join(".codex")).unwrap();
        folder_link(
            &inner.join(".codex").join("agents"),
            &elsewhere.join("agents"),
        );
        let summary = sync(&home, std::slice::from_ref(&inner), DefProgram::Codex, true);
        assert_eq!(summary.issues.len(), 1);
        assert_eq!(summary.issues[0].kind, IssueKind::Link);
        assert_eq!(
            read(&elsewhere.join("agents").join("manager.toml")),
            FOREIGN
        );
    }

    #[test]
    fn a_model_change_never_writes_through_a_linked_project_folder() {
        let (temp, home) = setup("provision-link-model");
        let elsewhere = temp.path().join("elsewhere");
        fs::create_dir_all(elsewhere.join("agents")).unwrap();
        fs::write(elsewhere.join("agents").join("manager.toml"), FOREIGN).unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        folder_link(&project.join(".codex"), &elsewhere);
        let choice = crate::agent_models::ModelChoice {
            model: Some("o3".into()),
            effort: None,
        };
        let result = agent_defs::set_model(
            &home,
            DefProgram::Codex,
            "manager",
            Some(&project),
            &choice,
            true,
        );
        assert!(
            matches!(result, Err(agent_defs::DefError::Link(_))),
            "{result:?}"
        );
        assert_eq!(
            read(&elsewhere.join("agents").join("manager.toml")),
            FOREIGN
        );
    }

    #[test]
    fn a_copy_edited_while_a_run_lists_the_folder_is_kept() {
        let (_temp, home) = setup("provision-fresh-remove");
        sync(&home, &[], DefProgram::Codex, true);
        let dir = codex_agents::agents_dir(&home);
        let path = dir.join("backend.toml");
        let listed = entries(&Place {
            program: DefProgram::Codex,
            format: Format::Codex,
            dir: dir.clone(),
            scope: Scope::Global,
            folder: None,
        });
        let entry = listed.iter().find(|e| e.path == path).unwrap();
        let marker = entry.marker.clone().unwrap();
        let edited = read(&path).replace("backend engineer", "my engineer");
        fs::write(&path, &edited).unwrap();
        let mut summary = Summary::default();
        let place = Place {
            program: DefProgram::Codex,
            format: Format::Codex,
            dir,
            scope: Scope::Global,
            folder: None,
        };
        remove(&place, entry, &marker, &mut summary);
        assert!(summary.removed.is_empty());
        assert_eq!(read(&path), edited);
    }

    #[test]
    fn a_bundled_copy_never_displaces_a_users_file() {
        let (temp, home) = setup("provision-bundled");
        let claude = agent_defs::agents_dir(&home);
        let tester = "---
name: hangar-tester
description: Tests
---
Test it.
";
        let developer = "---
name: hangar-developer
description: Builds
---
Build it.
";
        fs::write(claude.join("hangar-tester.md"), tester).unwrap();
        fs::write(claude.join("hangar-developer.md"), developer).unwrap();
        let global = codex_agents::agents_dir(&home);
        fs::create_dir_all(&global).unwrap();
        let own = FOREIGN.replace("\"manager\"", "\"hangar-tester\"");
        fs::write(global.join("hangar-tester.toml"), &own).unwrap();
        // An untracked project file of the name, which a user's own definition would replace.
        let project = temp.path().join("project");
        let project_dir = codex_agents::agents_dir(&project);
        fs::create_dir_all(&project_dir).unwrap();
        let project_own = FOREIGN.replace("\"manager\"", "\"hangar-developer\"");
        fs::write(project_dir.join("hangar-developer.toml"), &project_own).unwrap();

        let bundled = ["hangar-tester".to_owned(), "hangar-developer".to_owned()];
        let summary = sync_with(
            &home,
            std::slice::from_ref(&project),
            DefProgram::Codex,
            true,
            &bundled,
        );
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert!(summary.replaced.is_empty(), "{:?}", summary.replaced);
        // The user's own file stays byte for byte, and no backup appears.
        assert_eq!(read(&global.join("hangar-tester.toml")), own);
        assert!(!global.join("hangar-tester.toml.bak").exists());
        // A free name gets its copy; the project is left alone.
        assert!(global.join("hangar-developer.toml").is_file());
        assert_eq!(
            read(&project_dir.join("hangar-developer.toml")),
            project_own
        );
        assert!(!project_dir.join("hangar-developer.toml.bak").exists());
        // The user's definitions still replace as before.
        assert!(global.join("backend.toml").is_file());

        // Hangar's own copy of a bundled name is kept up to date.
        let changed = developer.replace("Build it.", "Build it well.");
        fs::write(claude.join("hangar-developer.md"), &changed).unwrap();
        sync_with(&home, &[], DefProgram::Codex, true, &bundled);
        assert!(read(&global.join("hangar-developer.toml")).contains("Build it well."));
    }

    #[test]
    fn nothing_is_created_without_definitions() {
        let temp = TempDir::new("provision-nothing");
        let home = temp.path().join("home");
        let summary = sync(&home, &[], DefProgram::Codex, true);
        assert_eq!(summary, Summary::default());
        assert!(!codex_agents::agents_dir(&home).exists());
        assert!(sync(&home, &[], DefProgram::Claude, true)
            .written
            .is_empty());
    }

    #[test]
    fn targets_are_the_installed_ones() {
        let off = BTreeSet::from([DefProgram::Codex]);
        assert!(targets(&[DefProgram::Claude], &BTreeSet::new()).is_empty());
        assert_eq!(
            targets(&[DefProgram::Codex], &off),
            [TargetView {
                program: DefProgram::Codex,
                enabled: false
            }]
        );
    }

    const MD_PROGRAMS: [(DefProgram, MdFormat); 3] = [
        (DefProgram::Copilot, MdFormat::Copilot),
        (DefProgram::Opencode, MdFormat::Opencode),
        (DefProgram::Gemini, MdFormat::Gemini),
    ];

    /// A user's own Markdown agent for `name`, in the program's shape.
    fn foreign_md(name: &str) -> String {
        format!("---\nname: {name}\ndescription: Mine\n---\nMy own prompt.\n")
    }

    #[test]
    fn markdown_copies_are_created_in_each_programs_global_folder() {
        for (program, md) in MD_PROGRAMS {
            let (_temp, home) = setup(&format!("provision-md-create-{program:?}"));
            let summary = sync(&home, &[], program, true);
            assert!(
                summary.issues.is_empty(),
                "{program:?}: {:?}",
                summary.issues
            );
            let dir = md.agents_dir(&home);
            let path = dir.join(format!("backend{}", md.suffix()));
            assert!(summary.written.contains(&path), "{program:?}");
            let text = read(&path);
            let marker = md.marker(&text).unwrap();
            assert_eq!(marker.source, "backend");
            assert_eq!(marker.backup, None);
            assert!(text.contains("description: \"Backend work: logic, APIs\"\n"));
            assert!(text.ends_with(
                "---\n\nYou are the backend engineer.\nUse \"quotes\" and C:\\paths.\n"
            ));
            // Claude Code's model alias and tools mean nothing there.
            let fields = agent_defs::frontmatter(&text).unwrap();
            assert!(!fields.contains_key("model") && !fields.contains_key("tools"));
            assert!(dir.join(format!("manager{}", md.suffix())).is_file());
            // A second run writes nothing; no Codex CLI folder appears.
            assert!(sync(&home, &[], program, true).written.is_empty());
            assert!(!codex_agents::agents_dir(&home).exists());
        }
    }

    #[test]
    fn markdown_folders_are_the_measured_paths() {
        let (_temp, home) = setup("provision-md-paths");
        for (program, _) in MD_PROGRAMS {
            sync(&home, &[], program, true);
        }
        assert!(home.join(".copilot/agents/backend.agent.md").is_file());
        assert!(home.join(".config/opencode/agents/backend.md").is_file());
        assert!(home.join(".gemini/agents/backend.md").is_file());
        let opencode = read(&home.join(".config/opencode/agents/backend.md"));
        assert!(opencode.contains("\nmode: subagent\n"));
    }

    #[test]
    fn a_users_markdown_file_is_set_aside_and_comes_back() {
        for (program, md) in MD_PROGRAMS {
            let (_temp, home) = setup(&format!("provision-md-replace-{program:?}"));
            let dir = md.agents_dir(&home);
            fs::create_dir_all(&dir).unwrap();
            let file = format!("manager{}", md.suffix());
            let own = foreign_md("manager");
            fs::write(dir.join(&file), &own).unwrap();
            let other = foreign_md("writer");
            fs::write(dir.join(format!("writer{}", md.suffix())), &other).unwrap();

            let summary = sync(&home, &[], program, true);
            assert!(
                summary.issues.is_empty(),
                "{program:?}: {:?}",
                summary.issues
            );
            let backup = format!("{file}.bak");
            assert_eq!(summary.replaced, [(dir.join(&file), dir.join(&backup))]);
            assert_eq!(read(&dir.join(&backup)), own);
            let marker = md.marker(&read(&dir.join(&file))).unwrap();
            assert_eq!(marker.backup.as_deref(), Some(backup.as_str()));
            assert_eq!(read(&dir.join(format!("writer{}", md.suffix()))), other);
            // Hangar's own now: nothing set aside or written again.
            let again = sync(&home, &[], program, true);
            assert!(again.replaced.is_empty() && again.written.is_empty());

            // The definition goes: the copy goes and the user's file is back.
            fs::remove_file(agent_defs::agents_dir(&home).join("manager.md")).unwrap();
            let summary = sync(&home, &[], program, true);
            assert_eq!(summary.removed, [dir.join(&file)]);
            assert_eq!(read(&dir.join(&file)), own);
            assert!(!dir.join(&backup).exists());

            // Switched off: every copy goes, the user's files stay.
            let off = sync(&home, &[], program, false);
            assert_eq!(off.removed, [dir.join(format!("backend{}", md.suffix()))]);
            assert_eq!(read(&dir.join(&file)), own);
            assert_eq!(read(&dir.join(format!("writer{}", md.suffix()))), other);
        }
    }

    #[test]
    fn an_edited_markdown_copy_is_the_users_again() {
        let (_temp, home) = setup("provision-md-edited");
        let md = MdFormat::Gemini;
        sync(&home, &[], DefProgram::Gemini, true);
        let path = md.agents_dir(&home).join("backend.md");
        let edited = read(&path).replace("backend engineer", "edited engineer");
        fs::write(&path, &edited).unwrap();
        let summary = sync(&home, &[], DefProgram::Gemini, true);
        assert_eq!(summary.replaced.len(), 1);
        assert_eq!(read(&path.with_file_name("backend.md.bak")), edited);
        assert!(read(&path).contains("backend engineer"));
        // Switched off: the edited text comes back.
        sync(&home, &[], DefProgram::Gemini, false);
        assert_eq!(read(&path), edited);
    }

    #[test]
    fn copilot_counts_a_plain_md_file_as_the_same_agent() {
        let (_temp, home) = setup("provision-md-copilot-plain");
        let dir = MdFormat::Copilot.agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        let own = foreign_md("whatever");
        fs::write(dir.join("manager.md"), &own).unwrap();
        let summary = sync(&home, &[], DefProgram::Copilot, true);
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        // Copilot CLI drops `.md` as it drops `.agent.md`: the file is that agent and is
        // replaced in place, so no second agent of the name appears.
        assert_eq!(read(&dir.join("manager.agent.md.bak")), own);
        assert!(MdFormat::Copilot
            .marker(&read(&dir.join("manager.md")))
            .is_some());
        assert!(!dir.join("manager.agent.md").exists());
    }

    #[test]
    fn gemini_matches_by_the_name_inside_the_file() {
        let (_temp, home) = setup("provision-md-gemini-name");
        let dir = MdFormat::Gemini.agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        // Gemini CLI reads `name`, not the file name: this file is the agent "manager".
        let own = foreign_md("manager");
        fs::write(dir.join("lead.md"), &own).unwrap();
        // A file Gemini CLI skips (leading underscore) is left alone, whatever it holds.
        let draft = foreign_md("backend");
        fs::write(dir.join("_draft.md"), &draft).unwrap();
        let summary = sync(&home, &[], DefProgram::Gemini, true);
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert_eq!(read(&dir.join("manager.md.bak")), own);
        assert!(MdFormat::Gemini
            .marker(&read(&dir.join("lead.md")))
            .is_some());
        assert!(!dir.join("manager.md").exists());
        assert_eq!(read(&dir.join("_draft.md")), draft);
        assert!(dir.join("backend.md").is_file());
    }

    #[test]
    fn gemini_leaves_a_file_of_the_name_that_holds_another_agent() {
        let (_temp, home) = setup("provision-md-gemini-taken");
        let dir = MdFormat::Gemini.agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        // The file is called "manager" but is the user's agent "lead" in Gemini CLI.
        let lead = foreign_md("lead");
        fs::write(dir.join("manager.md"), &lead).unwrap();
        let summary = sync(&home, &[], DefProgram::Gemini, true);
        assert_eq!(fs::read(dir.join("manager.md")).unwrap(), lead.as_bytes());
        assert!(summary.replaced.is_empty(), "{:?}", summary.replaced);
        assert!(!dir.join("manager.md.bak").exists());
        assert_eq!(
            summary.issues,
            [Issue {
                program: DefProgram::Gemini,
                scope: Scope::Global,
                folder: None,
                file: "manager.md".into(),
                name: Some("manager".into()),
                kind: IssueKind::NameTaken,
            }]
        );
        let json = serde_json::to_value(&summary.issues[0]).unwrap();
        assert_eq!(json["kind"], "nameTaken");
        // The other definition gets its copy as usual.
        assert!(dir.join("backend.md").is_file());
        // Renamed by the user: the name is free and the copy comes.
        fs::rename(dir.join("manager.md"), dir.join("lead.md")).unwrap();
        let summary = sync(&home, &[], DefProgram::Gemini, true);
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert!(MdFormat::Gemini
            .marker(&read(&dir.join("manager.md")))
            .is_some());
        assert_eq!(read(&dir.join("lead.md")), lead);
    }

    #[test]
    fn gemini_still_replaces_a_file_of_the_name_without_a_valid_name() {
        let (_temp, home) = setup("provision-md-gemini-nameless");
        let dir = MdFormat::Gemini.agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        // No frontmatter, and a name Gemini CLI would refuse: neither is another agent.
        let plain = "Just notes.\n";
        let bad = foreign_md("Lead Agent");
        fs::write(dir.join("manager.md"), plain).unwrap();
        fs::write(dir.join("backend.md"), &bad).unwrap();
        let summary = sync(&home, &[], DefProgram::Gemini, true);
        assert!(summary.issues.is_empty(), "{:?}", summary.issues);
        assert_eq!(read(&dir.join("manager.md.bak")), plain);
        assert_eq!(read(&dir.join("backend.md.bak")), bad);
        assert!(MdFormat::Gemini
            .marker(&read(&dir.join("manager.md")))
            .is_some());
    }

    #[test]
    fn the_listing_names_markdown_copies_and_their_backups() {
        let (_temp, home) = setup("provision-md-listing");
        let dir = MdFormat::Gemini.agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        let own = foreign_md("manager");
        fs::write(dir.join("manager.md"), &own).unwrap();
        fs::write(dir.join("manager.md.bak"), "older backup").unwrap();
        sync(&home, &[], DefProgram::Gemini, true);
        sync(&home, &[], DefProgram::Copilot, true);
        let mut copies = md_copies(
            &home,
            &[DefProgram::Codex, DefProgram::Gemini, DefProgram::Opencode],
        );
        copies.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            copies,
            [
                MdCopy {
                    program: DefProgram::Gemini,
                    name: "backend".into(),
                    file: "backend.md".into(),
                    backup: None,
                    backups: Vec::new(),
                },
                MdCopy {
                    program: DefProgram::Gemini,
                    name: "manager".into(),
                    file: "manager.md".into(),
                    backup: Some("manager.md.bak.2".into()),
                    backups: vec!["manager.md.bak".into(), "manager.md.bak.2".into()],
                },
            ]
        );
        // Only the named programs are read; a user's own file is no copy.
        assert_eq!(md_copies(&home, &[DefProgram::Copilot]).len(), 2);
        assert!(md_copies(&home, &[DefProgram::Opencode]).is_empty());
        let json = serde_json::to_value(&copies[1]).unwrap();
        assert_eq!(json["program"], "gemini");
        assert_eq!(json["backups"][0], "manager.md.bak");
    }

    #[test]
    fn markdown_folders_follow_the_link_rules_of_codex() {
        let (temp, home) = setup("provision-md-links");
        let md = MdFormat::Gemini;
        let dir = md.agents_dir(&home);
        // A global folder linked to a local one is followed, as for Codex CLI (dotfiles).
        let real = temp.path().join("dotfiles-agents");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(dir.parent().unwrap()).unwrap();
        folder_link(&dir, &real);
        // A link named like a copy is no file of the program's: never written through.
        let elsewhere = temp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("keep.txt"), "mine").unwrap();
        folder_link(&real.join("manager.md"), &elsewhere);

        let summary = sync(&home, &[], DefProgram::Gemini, true);
        assert!(md.marker(&read(&real.join("backend.md"))).is_some());
        assert_eq!(summary.issues.len(), 1, "{:?}", summary.issues);
        assert_eq!(summary.issues[0].kind, IssueKind::Io);
        assert_eq!(read(&elsewhere.join("keep.txt")), "mine");
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 1);

        // Switched off: the copy goes, the link and what it points to stay.
        let off = sync(&home, &[], DefProgram::Gemini, false);
        assert_eq!(off.removed, [dir.join("backend.md")]);
        assert!(fs::symlink_metadata(real.join("manager.md")).is_ok());
        assert_eq!(read(&elsewhere.join("keep.txt")), "mine");
    }

    #[test]
    fn markdown_programs_never_touch_project_folders() {
        let (temp, home) = setup("provision-md-project");
        let project = temp.path().join("project");
        for (program, md) in MD_PROGRAMS {
            let dir = md.agents_dir(&project);
            fs::create_dir_all(&dir).unwrap();
            let file = dir.join(format!("manager{}", md.suffix()));
            fs::write(&file, foreign_md("manager")).unwrap();
            let summary = sync(&home, std::slice::from_ref(&project), program, true);
            assert!(
                summary.issues.is_empty(),
                "{program:?}: {:?}",
                summary.issues
            );
            assert_eq!(read(&file), foreign_md("manager"));
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "{program:?}");
        }
    }

    #[test]
    fn a_markdown_copy_of_a_bundled_name_never_displaces_a_users_file() {
        let (_temp, home) = setup("provision-md-bundled");
        let dir = MdFormat::Opencode.agents_dir(&home);
        fs::create_dir_all(&dir).unwrap();
        let own = foreign_md("manager");
        fs::write(dir.join("manager.md"), &own).unwrap();
        let summary = sync_with(
            &home,
            &[],
            DefProgram::Opencode,
            true,
            &["manager".to_owned()],
        );
        assert!(summary.replaced.is_empty(), "{:?}", summary.replaced);
        assert_eq!(read(&dir.join("manager.md")), own);
        assert!(!dir.join("manager.md.bak").exists());
    }

    #[test]
    fn markdown_targets_need_their_measured_version() {
        for (program, floor) in [
            (DefProgram::Copilot, "1.0.114"),
            (DefProgram::Opencode, "2.3.2"),
            (DefProgram::Gemini, "0.70.0"),
        ] {
            assert_eq!(min_version(program), Some(floor));
            assert!(version_reaches(program, Some(floor)));
            assert!(!version_reaches(program, None), "{program:?}");
            assert!(!version_reaches(program, Some("garbage")), "{program:?}");
        }
        assert!(version_reaches(DefProgram::Opencode, Some("v2.4.0")));
        // OpenCode 1.x reads another folder layout, never measured.
        assert!(!version_reaches(DefProgram::Opencode, Some("1.19.5")));
        assert!(!version_reaches(DefProgram::Gemini, Some("0.69.9")));
        // Claude Code and Codex CLI need no version.
        assert!(version_reaches(DefProgram::Codex, None));
        assert!(version_reaches(DefProgram::Claude, None));
        let all = [
            DefProgram::Claude,
            DefProgram::Codex,
            DefProgram::Copilot,
            DefProgram::Opencode,
            DefProgram::Gemini,
        ];
        let listed: Vec<DefProgram> = targets(&all, &BTreeSet::new())
            .into_iter()
            .map(|target| target.program)
            .collect();
        assert_eq!(listed, TARGETS);
    }
}
