//! Claude Code sub-agent definitions that ship with Hangar (`src-tauri/bundled-agents/*.md`,
//! embedded at compile time). Hangar places each one as `~/.claude/agents/<name>.md` where no
//! definition of that name exists, keeps its own unchanged copies up to date and never touches a
//! file the user wrote or edited. The copies carry no marker: a manifest in Hangar's local data
//! folder keeps the SHA-256 of what Hangar last wrote per name. Contract:
//! docs/wiki/pages/syntheses/sub-agent-library.md, section "Bundled definitions".

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::agent_defs::{self, DefError};
use crate::agent_models;
use crate::no_links;

/// One definition shipped with Hangar.
#[derive(Debug, Clone, Copy)]
pub struct Bundled {
    pub name: &'static str,
    pub text: &'static str,
}

macro_rules! bundled {
    ($($name:literal),+ $(,)?) => {
        [$(Bundled {
            name: $name,
            text: include_str!(concat!("../bundled-agents/", $name, ".md")),
        }),+]
    };
}

/// Every bundled definition. Embedded so nothing is read from the installation folder at run
/// time; a test keeps each file parsable and its `name` equal to its file name.
pub const BUNDLED: [Bundled; 15] = bundled!(
    "hangar-backend",
    "hangar-comfyui",
    "hangar-contracts",
    "hangar-developer",
    "hangar-finance",
    "hangar-frontend",
    "hangar-manager",
    "hangar-marketing",
    "hangar-n8n",
    "hangar-researcher",
    "hangar-sales",
    "hangar-security",
    "hangar-social-media",
    "hangar-sysadmin",
    "hangar-tester",
);

/// The manifest's file name in the local app data folder.
pub const MANIFEST_FILE: &str = "bundled-agents.json";
/// Largest manifest read; fifteen entries take about a KiB.
const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// What Hangar knows of one bundled name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
enum Entry {
    /// Hangar wrote `<name>.md` with this content (SHA-256 hex) and it was unchanged when last
    /// looked at.
    Provided { sha256: String },
    /// Hangar's copy went missing: the user deleted it. Never created again while the switch
    /// stays on.
    Removed,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    agents: BTreeMap<String, Entry>,
}

/// A missing or damaged manifest is empty: every existing file then counts as the user's.
/// `None`: the manifest exists but cannot be read now; a run then changes nothing, so a passing
/// error never makes Hangar forget its copies.
fn load(path: &Path) -> Option<Manifest> {
    match crate::persist::read_limited(path, MAX_MANIFEST_BYTES) {
        Ok(Some(bytes)) => Some(serde_json::from_slice(&bytes).unwrap_or_default()),
        Ok(None) => Some(Manifest::default()),
        Err(error) => {
            eprintln!("hangar: bundled sub-agent manifest not read: {error}");
            None
        }
    }
}

fn save(path: &Path, manifest: &Manifest) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(manifest)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    crate::persist::write_atomic(path, &json)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One run at a time: the app start and the window can each start one, and both read and write
/// the manifest.
static RUN: Mutex<()> = Mutex::new(());

/// What one run changed, by name.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

/// The state of `<name>.md` in the global agents folder.
enum OnDisk {
    Missing,
    /// A regular file and its bytes.
    File(Vec<u8>),
    /// A link, a folder or a file too large: never Hangar's to change.
    Other,
    /// A plain file that cannot be read right now: left as it is, and as it was in the manifest.
    Unreadable,
}

fn on_disk(path: &Path) -> OnDisk {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return OnDisk::Missing,
        Err(_) => return OnDisk::Other,
        Ok(meta) if no_links::is_reparse_point(&meta) || !meta.is_file() => return OnDisk::Other,
        Ok(_) => {}
    }
    match crate::persist::read_regular_capped(path, agent_defs::MAX_FILE_BYTES) {
        Ok(Some(bytes)) => OnDisk::File(bytes),
        Ok(None) => OnDisk::Other,
        Err(_) => OnDisk::Unreadable,
    }
}

/// The names a global definition holds already: every file name (without `.md`, in lower case)
/// and every readable definition's `name`.
fn taken_names(home: &Path) -> HashSet<String> {
    let dir = agent_defs::agents_dir(home);
    let mut taken: HashSet<String> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter_map(|file| {
                    let lower = file.to_lowercase();
                    lower.strip_suffix(".md").map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    taken.extend(
        agent_defs::global_sources(home)
            .into_iter()
            .map(|(name, _)| name),
    );
    taken
}

/// [`provide`] with the switch from `store`. Nothing happens while no agent program that reads
/// the definitions is installed: Settings shows the switch only then, and Hangar creates no
/// `~/.claude` on a machine without one.
pub fn provide_for(
    home: &Path,
    installed: &[agent_models::DefProgram],
    store: &crate::main_store::MainStore,
) -> Changes {
    if installed.is_empty() {
        return Changes::default();
    }
    provide(home, &store.bundled_manifest(), || {
        !store.load_sub_agents().bundled_off
    })
}

/// [`provide_from`] with the definitions shipped in this build. `enabled` reads the switch once
/// the run holds the lock, so a run started before the switch changed cannot undo the run the
/// change started.
pub fn provide(home: &Path, manifest: &Path, enabled: impl FnOnce() -> bool) -> Changes {
    let _run = RUN.lock().unwrap_or_else(PoisonError::into_inner);
    run(home, manifest, enabled(), &BUNDLED)
}

/// Brings Hangar's copies of `catalog` in `~/.claude/agents` up to date.
///
/// Switched on: a name no global definition holds (by file name or `name`) gets `<name>.md`,
/// never replacing a file; an unchanged copy of Hangar's gets the new text; a changed one is the
/// user's from then on and leaves the manifest; a missing one is remembered as removed and not
/// created again. Switched off: unchanged copies are deleted, edited ones stay, and the manifest
/// forgets every name, so switching on again creates whatever is missing.
pub fn provide_from(
    home: &Path,
    manifest_path: &Path,
    enabled: bool,
    catalog: &[Bundled],
) -> Changes {
    let _run = RUN.lock().unwrap_or_else(PoisonError::into_inner);
    run(home, manifest_path, enabled, catalog)
}

/// [`provide_from`] for a caller that holds [`RUN`].
fn run(home: &Path, manifest_path: &Path, enabled: bool, catalog: &[Bundled]) -> Changes {
    let Some(before) = load(manifest_path) else {
        return Changes::default();
    };
    let mut manifest = Manifest {
        agents: before
            .agents
            .iter()
            .filter(|(name, _)| catalog.iter().any(|bundled| bundled.name == *name))
            .map(|(name, entry)| (name.clone(), entry.clone()))
            .collect(),
    };
    let mut changes = Changes::default();
    let dir = agent_defs::agents_dir(home);
    let mut taken: Option<HashSet<String>> = None;
    for bundled in catalog {
        let name = bundled.name;
        let bytes = agent_defs::file_bytes(bundled.text);
        let path = dir.join(format!("{name}.md"));
        let entry = manifest.agents.get(name).cloned();
        recover(&dir, name, entry.as_ref());
        let result = if enabled {
            match entry {
                Some(Entry::Removed) => Ok(()),
                Some(Entry::Provided { sha256: written }) => match on_disk(&path) {
                    OnDisk::Missing => {
                        manifest.agents.insert(name.to_owned(), Entry::Removed);
                        Ok(())
                    }
                    OnDisk::File(current) if sha256(&current) == written => {
                        if current == bytes {
                            Ok(())
                        } else {
                            replace(&path, name, &written, &bytes).map(|outcome| match outcome {
                                Replaced::Written => {
                                    let sha256 = sha256(&bytes);
                                    manifest
                                        .agents
                                        .insert(name.to_owned(), Entry::Provided { sha256 });
                                    changes.updated.push(name.to_owned());
                                }
                                // Changed or taken over meanwhile: the user's from now on.
                                Replaced::NotOurs => {
                                    manifest.agents.remove(name);
                                }
                                Replaced::Missing => {
                                    manifest.agents.insert(name.to_owned(), Entry::Removed);
                                }
                            })
                        }
                    }
                    OnDisk::Unreadable => Ok(()),
                    // Edited by the user, or no longer a plain file: theirs from now on.
                    OnDisk::File(_) | OnDisk::Other => {
                        manifest.agents.remove(name);
                        Ok(())
                    }
                },
                None => {
                    let taken = taken.get_or_insert_with(|| taken_names(home));
                    if taken.contains(name) {
                        Ok(())
                    } else {
                        create(home, name, &bytes).map(|created| {
                            if created {
                                let sha256 = sha256(&bytes);
                                manifest
                                    .agents
                                    .insert(name.to_owned(), Entry::Provided { sha256 });
                                changes.created.push(name.to_owned());
                                taken.insert(name.to_owned());
                            }
                        })
                    }
                }
            }
        } else {
            match (entry, on_disk(&path)) {
                (Some(Entry::Provided { sha256: written }), OnDisk::File(current))
                    if sha256(&current) == written =>
                {
                    // Forgotten only once deleted: a copy that could not be deleted stays
                    // Hangar's, for the next run.
                    take_if_unchanged(&path, &written).and_then(|taken| match taken {
                        Taken::Moved(temp) => fs::remove_file(&temp).map(|()| {
                            manifest.agents.remove(name);
                            changes.removed.push(name.to_owned());
                        }),
                        Taken::NotOurs | Taken::Missing => {
                            manifest.agents.remove(name);
                            Ok(())
                        }
                    })
                }
                (Some(Entry::Provided { .. }), OnDisk::Unreadable) => Ok(()),
                _ => {
                    manifest.agents.remove(name);
                    Ok(())
                }
            }
        };
        if let Err(error) = result {
            eprintln!("hangar: bundled sub-agent {name} left alone: {error}");
        }
    }
    if manifest != before {
        if let Err(error) = save(manifest_path, &manifest) {
            eprintln!("hangar: bundled sub-agent manifest not saved: {error}");
        }
    }
    changes
}

/// Suffix of the temporary name [`take_if_unchanged`] gives a copy.
const TAKEN_SUFFIX: &str = ".old";

/// Copies [`take_if_unchanged`] moved aside and a crash or failure left there
/// (`.<name>.md.<pid>.<n>.old`): put back when the name is free, deleted when they hold Hangar's
/// unchanged copy (`entry`), else left alone, as they may hold the user's text.
fn recover(dir: &Path, name: &str, entry: Option<&Entry>) {
    let prefix = format!(".{name}.md.");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let leftovers: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|item| {
            item.file_name()
                .to_str()
                .is_some_and(|file| file.starts_with(&prefix) && file.ends_with(TAKEN_SUFFIX))
        })
        .map(|item| item.path())
        .collect();
    let path = dir.join(format!("{name}.md"));
    for temp in leftovers {
        let OnDisk::File(bytes) = on_disk(&temp) else {
            continue;
        };
        let result = if matches!(on_disk(&path), OnDisk::Missing) {
            put_back(&temp, &path)
        } else if matches!(entry, Some(Entry::Provided { sha256: written }) if sha256(&bytes) == *written)
        {
            fs::remove_file(&temp)
        } else {
            Ok(())
        };
        if let Err(error) = result {
            eprintln!("hangar: bundled sub-agent {name}: leftover not recovered: {error}");
        }
    }
}

/// Where [`take_if_unchanged`] left a copy.
enum Taken {
    /// Moved to this temporary name in the same folder; its bytes are Hangar's copy.
    Moved(PathBuf),
    /// Changed since it was read (or no plain file): back under its own name, untouched.
    NotOurs,
    /// Gone before it could be moved.
    Missing,
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Moves `path` to a temporary name beside it first and checks the moved file, so a change the
/// user makes between the check and the deletion or replacement can never be lost: what is
/// checked is exactly what goes. A file that fails the check is put back without replacing one
/// that took its name meanwhile.
fn take_if_unchanged(path: &Path, written: &str) -> io::Result<Taken> {
    let (Some(dir), Some(file)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no file name"));
    };
    let temp = dir.join(format!(
        ".{}.{}.{}{TAKEN_SUFFIX}",
        file.to_string_lossy(),
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    match fs::rename(path, &temp) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Taken::Missing),
        Err(error) => return Err(error),
    }
    if matches!(on_disk(&temp), OnDisk::File(bytes) if sha256(&bytes) == written) {
        return Ok(Taken::Moved(temp));
    }
    put_back(&temp, path)?;
    Ok(Taken::NotOurs)
}

/// `temp` back under `path`, never replacing a file that took the name meanwhile; then the
/// temporary name is gone. When the name is taken, the moved file stays under `temp`.
fn put_back(temp: &Path, path: &Path) -> io::Result<()> {
    match fs::hard_link(temp, path) {
        Ok(()) => fs::remove_file(temp),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} was taken meanwhile; kept as {}",
                path.display(),
                temp.display()
            ),
        )),
        // No hard links here: a plain rename, checked just before.
        Err(_) if !path.exists() => fs::rename(temp, path),
        Err(error) => Err(error),
    }
}

/// How [`replace`] ended.
enum Replaced {
    Written,
    NotOurs,
    Missing,
}

/// Hangar's unchanged copy at `path` replaced by `bytes`: moved aside and checked
/// ([`take_if_unchanged`]), then the new text created under the name, never over a file that
/// took it meanwhile.
fn replace(path: &Path, name: &str, written: &str, bytes: &[u8]) -> io::Result<Replaced> {
    let temp = match take_if_unchanged(path, written)? {
        Taken::Moved(temp) => temp,
        Taken::NotOurs => return Ok(Replaced::NotOurs),
        Taken::Missing => return Ok(Replaced::Missing),
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    match agent_defs::write_exact(dir, name, bytes) {
        Ok(true) => fs::remove_file(&temp).map(|()| Replaced::Written),
        // Someone else's file took the name: it stays, Hangar's old copy goes.
        Ok(false) => fs::remove_file(&temp).map(|()| Replaced::NotOurs),
        Err(error) => {
            put_back(&temp, path)?;
            Err(def_io(error))
        }
    }
}

/// `<name>.md` written as a new file; `Ok(false)` when a file of that name appeared meanwhile.
fn create(home: &Path, name: &str, bytes: &[u8]) -> io::Result<bool> {
    let dir = agent_defs::global_dir(home).map_err(def_io)?;
    agent_defs::write_exact(&dir, name, bytes).map_err(def_io)
}

fn def_io(error: DefError) -> io::Error {
    match error {
        DefError::Io { source, .. } => source,
        other => io::Error::other(other.to_string()),
    }
}

/// The names whose global file is Hangar's unchanged copy, for the "From Hangar" note and the
/// copies in other programs.
pub fn unchanged(home: &Path, manifest_path: &Path) -> Vec<String> {
    unchanged_from(home, manifest_path, &BUNDLED)
}

/// [`unchanged`] for `catalog`. Paths come from the catalog's names only, never from the
/// manifest's keys, which are merely looked up.
fn unchanged_from(home: &Path, manifest_path: &Path, catalog: &[Bundled]) -> Vec<String> {
    let Some(manifest) = load(manifest_path) else {
        return Vec::new();
    };
    let dir = agent_defs::agents_dir(home);
    catalog
        .iter()
        .filter(|bundled| match manifest.agents.get(bundled.name) {
            Some(Entry::Provided { sha256: written }) => matches!(
                on_disk(&dir.join(format!("{}.md", bundled.name))),
                OnDisk::File(current) if sha256(&current) == *written
            ),
            Some(Entry::Removed) | None => false,
        })
        .map(|bundled| bundled.name.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const ALPHA: &str =
        "---\nname: alpha\ndescription: First bundled agent.\n---\n\nYou are alpha.\n";
    const BETA: &str =
        "---\nname: beta\ndescription: Second bundled agent.\n---\n\nYou are beta.\n";
    const CATALOG: [Bundled; 2] = [
        Bundled {
            name: "alpha",
            text: ALPHA,
        },
        Bundled {
            name: "beta",
            text: BETA,
        },
    ];

    struct Setup {
        _temp: TempDir,
        home: std::path::PathBuf,
        manifest: std::path::PathBuf,
    }

    impl Setup {
        fn new(name: &str) -> Self {
            let temp = TempDir::new(name);
            let home = temp.path().join("home");
            let manifest = temp.path().join("data").join(MANIFEST_FILE);
            Self {
                _temp: temp,
                home,
                manifest,
            }
        }

        fn file(&self, name: &str) -> std::path::PathBuf {
            agent_defs::agents_dir(&self.home).join(name)
        }

        fn run(&self, enabled: bool, catalog: &[Bundled]) -> Changes {
            provide_from(&self.home, &self.manifest, enabled, catalog)
        }

        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.file(name)).unwrap()
        }

        fn unchanged(&self) -> Vec<String> {
            unchanged_from(&self.home, &self.manifest, &CATALOG)
        }

        /// Files whose name starts with a dot: temporary files a run left behind.
        fn leftovers(&self) -> Vec<String> {
            fs::read_dir(agent_defs::agents_dir(&self.home))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .filter(|name| name.starts_with('.'))
                .collect()
        }
    }

    #[test]
    fn every_bundled_definition_parses_under_its_file_name() {
        let mut names = HashSet::new();
        for bundled in &BUNDLED {
            let parsed = agent_defs::parse_def(bundled.text)
                .unwrap_or_else(|problem| panic!("{}: {problem:?}", bundled.name));
            assert_eq!(parsed.name, bundled.name);
            assert!(agent_defs::valid_name(bundled.name));
            assert!(!agent_defs::is_device_name(bundled.name));
            assert!(bundled.text.len() <= agent_defs::MAX_FILE_BYTES);
            assert!(names.insert(bundled.name), "{} twice", bundled.name);
        }
    }

    #[test]
    fn missing_definitions_are_created_once() {
        let setup = Setup::new("bundled-create");
        let changes = setup.run(true, &CATALOG);
        assert_eq!(changes.created, ["alpha", "beta"]);
        assert_eq!(setup.read("alpha.md"), ALPHA);
        // Nothing to do the second time.
        assert_eq!(setup.run(true, &CATALOG), Changes::default());
        let mut names = setup.unchanged();
        names.sort();
        assert_eq!(names, ["alpha", "beta"]);
    }

    #[test]
    fn a_file_of_the_name_is_never_touched() {
        let setup = Setup::new("bundled-foreign");
        let dir = agent_defs::agents_dir(&setup.home);
        fs::create_dir_all(&dir).unwrap();
        // The user's own file under the file name, another case of it, and the name held by a
        // definition in a file of another name.
        let own = "---\nname: alpha\ndescription: Mine.\n---\nMine.\n";
        fs::write(dir.join("alpha.md"), own).unwrap();
        fs::write(
            dir.join("my-beta.md"),
            "---\nname: beta\ndescription: Mine too.\n---\n",
        )
        .unwrap();
        let changes = setup.run(true, &CATALOG);
        assert!(changes.created.is_empty(), "{changes:?}");
        assert_eq!(setup.read("alpha.md"), own);
        assert!(!setup.file("beta.md").exists());
        assert!(setup.unchanged().is_empty());
        // Not Hangar's, so switching off leaves it too.
        assert!(setup.run(false, &CATALOG).removed.is_empty());
        assert_eq!(setup.read("alpha.md"), own);

        let broken = Setup::new("bundled-broken");
        let dir = agent_defs::agents_dir(&broken.home);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ALPHA.md"), "not a definition").unwrap();
        assert_eq!(broken.run(true, &CATALOG).created, ["beta"]);
        assert_eq!(
            fs::read_to_string(dir.join("ALPHA.md")).unwrap(),
            "not a definition"
        );
    }

    #[test]
    fn an_unchanged_copy_follows_a_new_version() {
        let setup = Setup::new("bundled-update");
        setup.run(true, &CATALOG);
        let newer = ALPHA.replace("You are alpha.", "You are alpha, version two.");
        let catalog = [
            Bundled {
                name: "alpha",
                // Leaked once per test run; a `&'static str` is what the catalog holds.
                text: Box::leak(newer.clone().into_boxed_str()),
            },
            CATALOG[1],
        ];
        let changes = setup.run(true, &catalog);
        assert_eq!(changes.updated, ["alpha"]);
        assert_eq!(setup.read("alpha.md"), newer);
        assert!(setup.unchanged().contains(&"alpha".to_owned()));
    }

    #[test]
    fn an_edited_copy_belongs_to_the_user() {
        let setup = Setup::new("bundled-edited");
        setup.run(true, &CATALOG);
        let edited = ALPHA.replace("You are alpha.", "You are my alpha.");
        fs::write(setup.file("alpha.md"), &edited).unwrap();
        let newer = ALPHA.replace("You are alpha.", "You are alpha, version two.");
        let catalog = [
            Bundled {
                name: "alpha",
                text: Box::leak(newer.into_boxed_str()),
            },
            CATALOG[1],
        ];
        assert_eq!(setup.run(true, &catalog), Changes::default());
        assert_eq!(setup.read("alpha.md"), edited);
        assert_eq!(setup.unchanged(), ["beta"]);
        // Even if the user later restores Hangar's text, it stays theirs.
        fs::write(setup.file("alpha.md"), ALPHA).unwrap();
        assert_eq!(setup.run(true, &catalog), Changes::default());
        assert_eq!(setup.read("alpha.md"), ALPHA);
        // Switching off keeps it.
        assert_eq!(setup.run(false, &catalog).removed, ["beta"]);
        assert_eq!(setup.read("alpha.md"), ALPHA);
    }

    #[test]
    fn a_deleted_copy_is_not_created_again() {
        let setup = Setup::new("bundled-deleted");
        setup.run(true, &CATALOG);
        fs::remove_file(setup.file("alpha.md")).unwrap();
        assert_eq!(setup.run(true, &CATALOG), Changes::default());
        assert!(!setup.file("alpha.md").exists());
        assert_eq!(setup.run(true, &CATALOG), Changes::default());
        assert!(!setup.file("alpha.md").exists());
        let manifest = load(&setup.manifest).unwrap();
        assert_eq!(manifest.agents.get("alpha"), Some(&Entry::Removed));
    }

    #[test]
    fn the_switch_removes_unchanged_copies_and_brings_them_back() {
        let setup = Setup::new("bundled-switch");
        setup.run(true, &CATALOG);
        fs::remove_file(setup.file("beta.md")).unwrap();
        setup.run(true, &CATALOG);
        let edited = ALPHA.replace("You are alpha.", "Edited.");
        fs::write(setup.file("alpha.md"), &edited).unwrap();

        // Off: Hangar's unchanged copies go (none left here), edited ones stay.
        assert_eq!(setup.run(false, &CATALOG), Changes::default());
        assert_eq!(setup.read("alpha.md"), edited);
        assert_eq!(load(&setup.manifest), Some(Manifest::default()));

        // On again: the deleted one comes back, the user's file stays.
        assert_eq!(setup.run(true, &CATALOG).created, ["beta"]);
        assert_eq!(setup.read("alpha.md"), edited);
        assert_eq!(setup.read("beta.md"), BETA);

        // Off removes the unchanged copy.
        assert_eq!(setup.run(false, &CATALOG).removed, ["beta"]);
        assert!(!setup.file("beta.md").exists());
        // Off and nothing of Hangar's: nothing happens, nothing is created.
        assert_eq!(setup.run(false, &CATALOG), Changes::default());
        assert!(!setup.file("beta.md").exists());
    }

    #[test]
    fn an_unreadable_manifest_changes_nothing() {
        let setup = Setup::new("bundled-manifest-unreadable");
        setup.run(true, &CATALOG);
        // A manifest that exists but cannot be read (here a folder) is not taken as empty.
        fs::remove_file(&setup.manifest).unwrap();
        fs::create_dir(&setup.manifest).unwrap();
        assert_eq!(setup.run(false, &CATALOG), Changes::default());
        assert_eq!(setup.read("alpha.md"), ALPHA);
        fs::remove_file(setup.file("beta.md")).unwrap();
        assert_eq!(setup.run(true, &CATALOG), Changes::default());
        assert!(!setup.file("beta.md").exists());
        assert!(setup.unchanged().is_empty());
    }

    #[test]
    fn the_stored_switch_and_the_installed_programs_decide() {
        use crate::agent_models::DefProgram;
        use crate::main_store::{MainStore, SubAgentSettings};
        let setup = Setup::new("bundled-store");
        let store = MainStore::new(setup.manifest.parent().unwrap().to_path_buf());
        // No agent program: not even the folder is created.
        assert_eq!(provide_for(&setup.home, &[], &store), Changes::default());
        assert!(!agent_defs::agents_dir(&setup.home).exists());
        // On by default.
        let created = provide_for(&setup.home, &[DefProgram::Codex], &store).created;
        assert_eq!(created.len(), BUNDLED.len());
        assert_eq!(store.bundled_manifest(), setup.manifest);
        // Off as stored: the unchanged copies go.
        store
            .save_sub_agents(SubAgentSettings {
                bundled_off: true,
                ..SubAgentSettings::default()
            })
            .unwrap();
        let removed = provide_for(&setup.home, &[DefProgram::Claude], &store).removed;
        assert_eq!(removed.len(), BUNDLED.len());
    }

    /// A copy another program holds open without sharing can be neither read nor deleted: it
    /// stays Hangar's until it can be.
    #[cfg(windows)]
    #[test]
    fn a_locked_copy_stays_hangars() {
        use std::os::windows::fs::OpenOptionsExt;
        let setup = Setup::new("bundled-locked");
        setup.run(true, &CATALOG);
        let newer = ALPHA.replace("You are alpha.", "You are alpha, version two.");
        let catalog = [
            Bundled {
                name: "alpha",
                text: Box::leak(newer.clone().into_boxed_str()),
            },
            CATALOG[1],
        ];
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(setup.file("alpha.md"))
            .unwrap();
        assert_eq!(setup.run(true, &catalog), Changes::default());
        assert_eq!(setup.run(false, &catalog).removed, ["beta"]);
        drop(lock);
        assert_eq!(setup.read("alpha.md"), ALPHA);
        // Still Hangar's: the next run brings it up to date (and creates beta again, switched
        // on anew), or removes it.
        let changes = setup.run(true, &catalog);
        assert_eq!(
            (changes.updated, changes.created),
            (vec!["alpha".to_owned()], vec!["beta".to_owned()])
        );
        assert_eq!(setup.read("alpha.md"), newer);
        assert_eq!(setup.run(false, &catalog).removed, ["alpha", "beta"]);
        assert!(!setup.file("alpha.md").exists());
    }

    #[test]
    fn a_copy_is_checked_after_it_was_moved_aside() {
        let setup = Setup::new("bundled-take");
        setup.run(true, &CATALOG);
        let path = setup.file("alpha.md");
        let written = sha256(ALPHA.as_bytes());

        // Changed: moved back, byte for byte, nothing left over.
        let edited = ALPHA.replace("You are alpha.", "Mine now.");
        fs::write(&path, &edited).unwrap();
        assert!(matches!(
            take_if_unchanged(&path, &written).unwrap(),
            Taken::NotOurs
        ));
        assert_eq!(setup.read("alpha.md"), edited);
        assert!(setup.leftovers().is_empty(), "{:?}", setup.leftovers());

        // Unchanged: what was checked is what is moved; the name is free.
        fs::write(&path, ALPHA).unwrap();
        let Taken::Moved(temp) = take_if_unchanged(&path, &written).unwrap() else {
            panic!("not moved");
        };
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(&temp).unwrap(), ALPHA);

        // A file that took the name meanwhile is never replaced by putting one back.
        fs::write(&path, "new file of the user").unwrap();
        let error = put_back(&temp, &path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(setup.read("alpha.md"), "new file of the user");
        assert_eq!(fs::read_to_string(&temp).unwrap(), ALPHA);

        assert!(matches!(
            take_if_unchanged(&setup.file("gone.md"), &written).unwrap(),
            Taken::Missing
        ));
    }

    #[test]
    fn replacing_and_removing_leave_no_temporary_files() {
        let setup = Setup::new("bundled-temp");
        setup.run(true, &CATALOG);
        let newer = ALPHA.replace("You are alpha.", "You are alpha, version two.");
        let catalog = [
            Bundled {
                name: "alpha",
                text: Box::leak(newer.clone().into_boxed_str()),
            },
            CATALOG[1],
        ];
        assert_eq!(setup.run(true, &catalog).updated, ["alpha"]);
        assert_eq!(setup.read("alpha.md"), newer);
        assert_eq!(setup.run(false, &catalog).removed, ["alpha", "beta"]);
        assert!(setup.leftovers().is_empty(), "{:?}", setup.leftovers());
    }

    #[test]
    fn manifest_keys_never_name_a_path() {
        let setup = Setup::new("bundled-evil-key");
        let dir = agent_defs::agents_dir(&setup.home);
        fs::create_dir_all(&dir).unwrap();
        // A file outside the agents folder that a key `../evil` would reach, with its hash.
        let evil = setup.home.join(".claude").join("evil.md");
        fs::write(&evil, ALPHA).unwrap();
        let hash = sha256(ALPHA.as_bytes());
        let json = format!(
            r#"{{"agents":{{"../evil":{{"state":"provided","sha256":"{hash}"}},"alpha":{{"state":"provided","sha256":"{hash}"}}}}}}"#
        );
        fs::create_dir_all(setup.manifest.parent().unwrap()).unwrap();
        fs::write(&setup.manifest, json).unwrap();
        fs::write(dir.join("alpha.md"), ALPHA).unwrap();
        assert_eq!(setup.unchanged(), ["alpha"]);
        assert_eq!(
            unchanged(&setup.home, &setup.manifest),
            Vec::<String>::new()
        );
        // A run drops the key and never touches the file.
        setup.run(false, &CATALOG);
        assert_eq!(fs::read_to_string(&evil).unwrap(), ALPHA);
        assert!(!load(&setup.manifest)
            .unwrap()
            .agents
            .contains_key("../evil"));
    }

    /// A crash between moving a copy aside and finishing leaves `.<name>.md.<pid>.<n>.old`: the
    /// next run puts it back where the name is free, and deletes it only when it is Hangar's.
    #[test]
    fn a_copy_left_aside_by_a_crash_is_recovered() {
        let setup = Setup::new("bundled-leftover");
        setup.run(true, &CATALOG);
        let dir = agent_defs::agents_dir(&setup.home);
        // Crash after the move: the name is free, the copy waits beside it.
        let aside = dir.join(".alpha.md.1.0.old");
        fs::rename(setup.file("alpha.md"), &aside).unwrap();
        // Hangar's copy that was already replaced: only the leftover remains.
        let stale = dir.join(".beta.md.1.1.old");
        fs::write(&stale, BETA).unwrap();
        // The user's text left aside while a new file took the name: kept.
        let mine = "---
name: beta
description: Mine.
---
Mine.
";
        let kept = dir.join(".beta.md.1.2.old");
        fs::write(&kept, mine).unwrap();

        assert_eq!(setup.run(true, &CATALOG), Changes::default());
        assert_eq!(setup.read("alpha.md"), ALPHA);
        assert!(!aside.exists());
        assert!(!stale.exists());
        assert_eq!(fs::read_to_string(&kept).unwrap(), mine);
        // Still Hangar's copies, not taken for deleted ones.
        let mut names = unchanged_from(&setup.home, &setup.manifest, &CATALOG);
        names.sort();
        assert_eq!(names, ["alpha", "beta"]);
    }

    #[test]
    fn crlf_and_a_damaged_manifest_are_handled() {
        let setup = Setup::new("bundled-crlf");
        let crlf = ALPHA.replace('\n', "\r\n");
        let catalog = [Bundled {
            name: "alpha",
            text: Box::leak(crlf.into_boxed_str()),
        }];
        setup.run(true, &catalog);
        assert_eq!(setup.read("alpha.md"), ALPHA);
        // A damaged manifest forgets Hangar's copies: they then count as the user's.
        fs::write(&setup.manifest, "{ not json").unwrap();
        assert!(setup.run(false, &catalog).removed.is_empty());
        assert_eq!(setup.read("alpha.md"), ALPHA);
    }
}
