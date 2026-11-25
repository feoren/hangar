//! Team vaults (ADR 0006, phase 2: "Phase 2 design", sections 6 to 9).
//!
//! A team vault's `vault.json` names its team by a hint ([`crypto::team_hint`]). The key ring
//! comes from the team's worker ([`TeamSide::teams`], the `notes-keys` record) and from a
//! keychain cache of its newest [`CACHED_TEAM_KEYS`] keys (account `team/<team key>`), which
//! also keeps the save rule across restarts: the epoch under which the ring's newest key was
//! confirmed and whether saving was refused when the log was last known.
//!
//! **Save rule.** Saving is refused while the verified log names a removal whose rotation has
//! not landed, or knows a newer epoch than the one the ring's newest key was confirmed under:
//! that key may be in the hands of whoever left. Without a verified log in this run (after a
//! restart, before the first connection) the last decision stored with the cache holds. On a
//! relay of version 1 the ring cannot be renewed, so once the log moves past it saving stays
//! refused.

use std::path::Path;

use hangar_team_crypto::notes::{self as crypto, KeyRing, VaultId};
use hangar_team_crypto::OsEntropy;
use hangar_team_protocol::ids::{Epoch, TeamId};
use serde::Serialize;
use zeroize::Zeroizing;

use super::{
    check_notes_dir, io_error, vault_json, Open, Project, ProjectNotes, ProjectNotesError,
    SharedStatus, Vault, VaultFile, VaultMode, ATTRIBUTES, ATTRIBUTES_FILE, README, README_FILE,
    VAULT_FILE,
};
use crate::git::{self, Git};
use crate::lock;
use crate::notes::NoteFolder;
use crate::persist::write_atomic;
use crate::team::TeamError;
use crate::team_service::NotesTeam;

/// Keys of a team ring the keychain keeps: 12 x 82 units plus the header stay below the Windows
/// credential store's 1280 (`secrets::MAX_VALUE_UNITS`).
pub const CACHED_TEAM_KEYS: usize = 12;
const CACHE_VERSION: &str = "t1";

/// What shared notes need from the team side; `TeamService` in the app, a fake in tests.
pub trait TeamSide: Send + Sync {
    /// The teams of the current server with sharing on.
    fn teams(&self) -> Vec<NotesTeam>;
    /// Turns shared notes on for `project`, or brings a solo ring in; returns once the relay
    /// stored the ring. Blocks the calling (background) thread.
    fn enable(&self, project: &str, adopt: Option<KeyRing>) -> Result<(), TeamError>;
    /// Links the clone at `folder` with root commit `root` to `project`.
    fn link(&self, project: &str, root: &str, folder: &str) -> Result<(), TeamError>;
    /// The person's display name for the teammates (`by` of a note), empty without one.
    fn person(&self) -> String;
}

/// The team a working folder's shared notes belong to, for the window's team lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamNotes {
    /// The website's project id of the team.
    pub project: String,
    /// This device may turn shared notes on or share a solo vault (owner or admin).
    pub manager: bool,
    /// The team server speaks protocol version 2 (`None`: not connected, not known).
    pub speaks: Option<bool>,
    /// The team links a clone of this repository whose main worktree is this one.
    pub linked: bool,
    /// The team has a key ring on the relay (`None`: not known).
    pub exists: Option<bool>,
}

/// The keychain cache of a team ring: `t1 <fresh epoch, 0 for none> <stale 0|1> v1 <keys>`.
#[derive(Debug, Clone)]
pub(super) struct TeamCache {
    pub fresh: Option<Epoch>,
    pub stale: bool,
    pub ring: KeyRing,
}

pub(super) enum CacheEntry {
    Nothing,
    Cache(TeamCache),
    /// Unreadable or of a newer format: never overwritten.
    Unusable,
}

pub(super) fn parse_cache(text: &str) -> Option<TeamCache> {
    let mut parts = text.splitn(4, ' ');
    if parts.next()? != CACHE_VERSION {
        return None;
    }
    let fresh: u32 = parts.next()?.parse().ok()?;
    let stale = match parts.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let ring = KeyRing::from_keychain(parts.next()?).ok()?;
    Some(TeamCache {
        fresh: (fresh > 0).then_some(Epoch(fresh)),
        stale,
        ring,
    })
}

pub(super) fn cache_text(cache: &TeamCache) -> Zeroizing<String> {
    let ring = cache.ring.newest_keys(CACHED_TEAM_KEYS).to_keychain();
    Zeroizing::new(format!(
        "{CACHE_VERSION} {} {} {}",
        cache.fresh.map_or(0, |e| e.0),
        u8::from(cache.stale),
        ring.as_str()
    ))
}

fn team_account(key: &str) -> String {
    format!("team/{key}")
}

/// The ring a team vault opens with and whether saving is refused.
pub(super) struct TeamRing {
    pub ring: Option<KeyRing>,
    pub stale: bool,
    /// This run verified the team's log, so `stale` reflects the log, not a stored decision.
    pub verified: bool,
}

/// Why a team vault does not open.
pub(super) enum TeamLock {
    /// No team of this person carries the hint.
    NotMember,
    /// The team is this person's, but the clone is not linked to it.
    NotLinked,
    /// Linked, but no key here yet; `old_server`: the relay speaks version 1.
    Waiting { old_server: bool },
}

impl ProjectNotes {
    /// The team side, once the app set it.
    pub fn set_team(&self, team: std::sync::Arc<dyn TeamSide>) {
        *lock(&self.team) = Some(team);
    }

    fn team_side(&self) -> Option<std::sync::Arc<dyn TeamSide>> {
        lock(&self.team).clone()
    }

    fn teams(&self) -> Vec<NotesTeam> {
        self.team_side()
            .map(|side| side.teams())
            .unwrap_or_default()
    }

    /// The team whose hint `vault.json` names.
    fn team_for_hint(&self, hint: &str) -> Option<NotesTeam> {
        self.teams().into_iter().find(|team| {
            TeamId::new(team.project.as_str()).is_ok_and(|id| crypto::team_hint(&id) == hint)
        })
    }

    /// Whether `team` links a clone of this repository whose main worktree is `project.root`.
    fn links(&self, team: &NotesTeam, project: &Project, root: Option<&str>) -> bool {
        let Some(root) = root else {
            return false;
        };
        team.linked.iter().any(|link| {
            link.root == root
                && link.folder.as_deref().is_some_and(|folder| {
                    let path = Path::new(folder);
                    path.is_absolute()
                        && !crate::manager::is_network_or_device(path)
                        && self
                            .locate(path)
                            .ok()
                            .flatten()
                            .is_some_and(|linked| same_dir(&linked.root, &project.root))
                })
        })
    }

    /// The repository's root commit, its identity in the team protocol.
    fn root_commit(project: &Project) -> Option<String> {
        let git = Git::locate().ok()?;
        crate::claims_service::root_commit(&git, &project.root)
            .ok()
            .flatten()
    }

    /// The team that links this clone, for a folder without a team vault.
    fn linked_team(&self, project: &Project) -> Option<NotesTeam> {
        let teams = self.teams();
        if teams.iter().all(|team| team.linked.is_empty()) {
            return None;
        }
        let root = Self::root_commit(project);
        teams
            .into_iter()
            .find(|team| self.links(team, project, root.as_deref()))
    }

    fn read_cache(&self, key: &str) -> CacheEntry {
        match self.keychain.get(&team_account(key)) {
            Ok(None) => CacheEntry::Nothing,
            Ok(Some(text)) => match parse_cache(&text) {
                Some(cache) => CacheEntry::Cache(cache),
                None => CacheEntry::Unusable,
            },
            Err(error) => {
                eprintln!("hangar: team notes keys not read from the keychain: {error}");
                CacheEntry::Unusable
            }
        }
    }

    /// The team's ring (the worker's and the cache's) and the save rule (module docs). Writes
    /// the cache when what it should hold changed.
    pub(super) fn team_ring(&self, team: &NotesTeam) -> TeamRing {
        let entry = self.read_cache(&team.key);
        let cached = match &entry {
            CacheEntry::Cache(cache) => Some(cache.clone()),
            _ => None,
        };
        let view = team.view.as_ref();
        // The team's ring comes first and the cache is merged into it, never the other way:
        // merging puts the keys a ring lacks behind its own, so a cache of the newest keys with
        // the team's whole ring merged in would end in the team's oldest key, and new notes
        // would be sealed with it. Cached keys the team's ring lacks are newer than it (an old
        // record shown again) and rightly go behind.
        let ring = match (view.and_then(|v| v.ring.as_ref()), &cached) {
            (Some(live), Some(cache)) => {
                let mut ring = live.clone();
                if ring.merge(&cache.ring).is_err() {
                    // A cached key with other bytes than the team's: the team wins.
                    ring = live.clone();
                }
                Some(ring)
            }
            (Some(live), None) => Some(live.clone()),
            (None, cache) => cache.as_ref().map(|c| c.ring.clone()),
        };
        let fresh = match (
            cached.as_ref().and_then(|c| c.fresh),
            view.and_then(|v| v.fresh),
        ) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let stale = match view.and_then(|v| v.log_epoch) {
            Some(log) => view.is_some_and(|v| v.urgent) || fresh.is_none_or(|f| f < log),
            None => cached.as_ref().is_some_and(|c| c.stale),
        };
        if let (Some(ring), false) = (&ring, matches!(entry, CacheEntry::Unusable)) {
            let next = TeamCache {
                fresh,
                stale,
                ring: ring.clone(),
            };
            let text = cache_text(&next);
            let same = cached.as_ref().is_some_and(|c| *cache_text(c) == *text);
            if !same {
                if let Err(error) = self.keychain.set(&team_account(&team.key), &text) {
                    eprintln!("hangar: team notes keys not cached: {error}");
                }
            }
        }
        TeamRing {
            ring,
            stale,
            verified: view.is_some_and(|v| v.log_epoch.is_some()),
        }
    }

    /// The team vault's ring, or why it does not open.
    pub(super) fn open_team(
        &self,
        project: &Project,
        hint: &str,
    ) -> Result<(TeamRing, NotesTeam), TeamLock> {
        let Some(team) = self.team_for_hint(hint) else {
            return Err(TeamLock::NotMember);
        };
        if !self.links(&team, project, Self::root_commit(project).as_deref()) {
            return Err(TeamLock::NotLinked);
        }
        let access = self.team_ring(&team);
        if access.ring.is_none() {
            let old_server = team.view.as_ref().and_then(|v| v.speaks) == Some(false);
            return Err(TeamLock::Waiting { old_server });
        }
        Ok((access, team))
    }

    /// The display name a team note carries as its last editor.
    pub(super) fn person(&self) -> String {
        self.team_side()
            .map(|side| side.person())
            .unwrap_or_default()
    }

    // ----- Commands ------------------------------------------------------------------------

    /// The team this folder's shared notes belong to (a team vault's, else the team that links
    /// the clone); `None` without one.
    pub fn team_notes(&self, folder: &NoteFolder) -> Result<Option<TeamNotes>, ProjectNotesError> {
        let Some(project) = self.locate(Self::folder_path(folder)?)? else {
            return Ok(None);
        };
        let hinted = match Self::read_vault(&project)? {
            Some(VaultFile::Known(Vault {
                mode: VaultMode::Team { team },
                ..
            })) => Some(team),
            _ => None,
        };
        let root = Self::root_commit(&project);
        let team = match &hinted {
            Some(hint) => self.team_for_hint(hint),
            None => self.linked_team(&project),
        };
        Ok(team.map(|team| TeamNotes {
            linked: self.links(&team, &project, root.as_deref()),
            manager: team.view.as_ref().is_some_and(|v| v.manager),
            speaks: team.view.as_ref().and_then(|v| v.speaks),
            exists: team.view.as_ref().and_then(|v| v.exists),
            project: team.project,
        }))
    }

    /// Links this clone's main worktree to the team its vault names ("Link this folder").
    pub fn link_team(
        &self,
        folder: &NoteFolder,
        label: &str,
        untitled: &str,
    ) -> Result<SharedStatus, ProjectNotesError> {
        let project = self.project(folder)?;
        let Some(VaultFile::Known(Vault {
            mode: VaultMode::Team { team: hint },
            ..
        })) = Self::read_vault(&project)?
        else {
            return Err(ProjectNotesError::NotOn);
        };
        let team = self
            .team_for_hint(&hint)
            .ok_or(ProjectNotesError::Team(TeamError::UnknownProject))?;
        let root = Self::root_commit(&project).ok_or(ProjectNotesError::NotRepository)?;
        let side = self
            .team_side()
            .ok_or(ProjectNotesError::Team(TeamError::UnknownProject))?;
        side.link(&team.project, &root, &project.root.to_string_lossy())?;
        self.status(folder, label, untitled)
    }

    /// Turns shared notes on for the team that links this clone: the relay stores the team's
    /// ring first (or the team has one already), then the files are written. No code.
    pub fn enable_team(
        &self,
        folder: &NoteFolder,
        label: &str,
        untitled: &str,
    ) -> Result<SharedStatus, ProjectNotesError> {
        let project = self.project(folder)?;
        if Self::read_vault(&project)?.is_some() {
            return Err(ProjectNotesError::AlreadyOn);
        }
        let team = self
            .linked_team(&project)
            .ok_or(ProjectNotesError::NotLinked)?;
        let side = self.team_side().ok_or(ProjectNotesError::NotLinked)?;
        side.enable(&team.project, None)?;
        let team = self
            .team_for_project(&team.project)
            .ok_or(ProjectNotesError::NotLinked)?;
        if self.team_ring(&team).ring.is_none() {
            return Err(ProjectNotesError::Team(TeamError::NotesNotReady));
        }
        let id = TeamId::new(team.project.as_str())
            .map_err(|_| ProjectNotesError::Team(TeamError::UnknownProject))?;
        let vault = Vault {
            vault: VaultId::generate(&mut OsEntropy).map_err(crypto::NotesCryptoError::from)?,
            mode: VaultMode::Team {
                team: crypto::team_hint(&id),
            },
        };
        self.write_vault_files(&project, &vault, true)?;
        self.status(folder, label, untitled)
    }

    /// Shares an unlocked solo vault with the team that links this clone: the relay stores the
    /// team's ring with the solo keys and a new key first; only then `vault.json` becomes a team
    /// vault without its code slot. The solo keychain entry stays (the code still opens what was
    /// written before).
    pub fn to_team(
        &self,
        folder: &NoteFolder,
        label: &str,
        untitled: &str,
    ) -> Result<SharedStatus, ProjectNotesError> {
        let open = self.open(folder)?;
        if !matches!(open.vault.mode, VaultMode::Solo { .. }) {
            return Err(ProjectNotesError::AlreadyOn);
        }
        // Only the owner holds a solo key: every note it opens is known, so the status after the
        // switch moves them all to the team's newest key instead of marking them.
        self.scan_known(&open)?;
        let team = self
            .linked_team(&open.project)
            .ok_or(ProjectNotesError::NotLinked)?;
        let side = self.team_side().ok_or(ProjectNotesError::NotLinked)?;
        side.enable(&team.project, Some(open.ring.clone()))?;
        let team = self
            .team_for_project(&team.project)
            .ok_or(ProjectNotesError::NotLinked)?;
        let ring = self.team_ring(&team).ring;
        if !ring.is_some_and(|ring| ring.contains_all(&open.ring)) {
            return Err(ProjectNotesError::Team(TeamError::NotesNotReady));
        }
        let id = TeamId::new(team.project.as_str())
            .map_err(|_| ProjectNotesError::Team(TeamError::UnknownProject))?;
        let vault = Vault {
            vault: open.vault.vault,
            mode: VaultMode::Team {
                team: crypto::team_hint(&id),
            },
        };
        self.write_vault_files(&open.project, &vault, false)?;
        self.status(folder, label, untitled)
    }

    fn team_for_project(&self, project: &str) -> Option<NotesTeam> {
        self.teams().into_iter().find(|t| t.project == project)
    }

    /// Writes `.gitattributes` and `README.md` where missing and `vault.json`: a new one only
    /// where none exists (`new`), else replacing the solo one.
    fn write_vault_files(
        &self,
        project: &Project,
        vault: &Vault,
        new: bool,
    ) -> Result<(), ProjectNotesError> {
        let _guard = lock(&self.state);
        std::fs::create_dir_all(&project.dir).map_err(io_error(&project.dir))?;
        check_notes_dir(&project.root)?;
        for (name, content) in [(ATTRIBUTES_FILE, ATTRIBUTES), (README_FILE, README)] {
            let path = project.dir.join(name);
            if !path.exists() {
                write_atomic(&path, content.as_bytes()).map_err(io_error(&path))?;
            }
        }
        let path = project.dir.join(VAULT_FILE);
        if new {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::AlreadyExists => ProjectNotesError::AlreadyOn,
                    _ => io_error(&path)(error),
                })?;
            file.write_all(vault_json(vault).as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(io_error(&path))?;
        } else {
            write_atomic(&path, vault_json(vault).as_bytes()).map_err(io_error(&path))?;
        }
        Ok(())
    }
}

/// Whether two folders are the same, by their canonical paths (case and links resolved).
fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => git::path_key(a) == git::path_key(b),
    }
}

impl Open {
    /// Saving is refused while the team's newest key may be known to whoever left.
    pub(super) fn check_writable(&self) -> Result<(), ProjectNotesError> {
        if self.stale {
            Err(ProjectNotesError::Renewing)
        } else {
            Ok(())
        }
    }
}
