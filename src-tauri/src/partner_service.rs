//! Partner projects and cross-project messages (ADR 0012), the service: loads and saves the
//! partner book, checks folders on disk, finds the sessions of a partner project and prepares a
//! message for pasting. The rules themselves live in [`crate::partners`].
//!
//! Delivery is split on purpose: this side decides who may send to whom and what text may be
//! pasted (partnership, target, cleaning, length, work log line); the window decides when (the
//! target paused, bracketed paste on, output quiet), because only it sees the terminal. A
//! message an agent sends later (ADR 0012, `hangar-send`) passes the same checks here.

use std::path::{Component, Path};

use serde::Serialize;

use crate::git::repo_place_of;
use crate::main_store::{MainStore, MainStoreError};
use crate::no_links::FolderPlace;
use crate::partners::{
    compose, in_project, NamedProject, PartnerBook, PartnerError, ProjectEnd, SessionPlace,
};
use crate::session::{SessionId, SessionInfo, SessionKind, Status};

#[derive(Debug, thiserror::Error)]
pub enum PartnerServiceError {
    #[error(transparent)]
    Partner(#[from] PartnerError),
    #[error(transparent)]
    Store(#[from] MainStoreError),
    #[error("Unknown session {0}")]
    UnknownSession(SessionId),
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl Serialize for PartnerServiceError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// The project a folder belongs to, as the project card shows it: the repository's main folder
/// (also for a linked worktree), else the folder itself.
pub fn project_root(folder: &Path) -> String {
    repo_place_of(folder, None).map_or_else(|| folder.to_string_lossy().into_owned(), |p| p.root)
}

/// A folder the person names as a partner: an existing local folder (no share, no device),
/// taken as its project root.
fn resolve_folder(folder: &str) -> Result<String, PartnerError> {
    let path = Path::new(folder.trim());
    // `..` and `.` would give one folder several spellings, and so several keys.
    if folder.trim().is_empty()
        || !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(PartnerError::InvalidFolder);
    }
    // Before anything touches the folder: a share is refused unasked (no_links.rs).
    match crate::no_links::folder_place(path) {
        FolderPlace::Local => {}
        FolderPlace::Remote => return Err(PartnerError::InvalidFolder),
        FolderPlace::NotAFolder => return Err(PartnerError::MissingFolder),
    }
    // Every link on the way leads to a local fixed drive (checked above), so resolving it touches
    // no host. A junction or symlink then gives the folder it points to, and a project linked
    // to itself through one is caught as such.
    let resolved = std::fs::canonicalize(path).map_err(|_| PartnerError::MissingFolder)?;
    Ok(project_root(Path::new(&crate::file_tree::display_path(
        &resolved,
    ))))
}

/// Links the project at `this_folder` with the one at `partner_folder` and stores the list.
pub fn link(
    store: &MainStore,
    this_folder: &str,
    this_name: &str,
    partner_folder: &str,
    partner_name: &str,
) -> Result<PartnerBook, PartnerServiceError> {
    let this = resolve_folder(this_folder)?;
    let partner = resolve_folder(partner_folder)?;
    let book = store.load_partners().link(
        ProjectEnd {
            folder: &this,
            name: this_name,
        },
        ProjectEnd {
            folder: &partner,
            name: partner_name,
        },
    )?;
    store.save_partners(&book)?;
    Ok(book)
}

/// Removes the pair of `a` and `b` and stores the list.
pub fn unlink(store: &MainStore, a: &str, b: &str) -> Result<PartnerBook, PartnerServiceError> {
    let book = store.load_partners().unlink(a, b)?;
    store.save_partners(&book)?;
    Ok(book)
}

/// A session in a partner project, as the window chooses the target from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartnerSession {
    pub id: SessionId,
    pub profile_id: String,
    /// The process runs; an ended session only tells which program last worked there.
    pub running: bool,
}

/// Where a message from a session to one of its partners can go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartnerTargets {
    /// The sending project's short name.
    pub sender: String,
    pub partner: NamedProject,
    /// The partner project's sessions (no update sessions), oldest first.
    pub sessions: Vec<PartnerSession>,
}

fn place_of(info: &SessionInfo, root: &dyn Fn(&Path) -> String) -> (String, String) {
    (root(&info.cwd), info.cwd.to_string_lossy().into_owned())
}

fn session(sessions: &[SessionInfo], id: SessionId) -> Result<&SessionInfo, PartnerServiceError> {
    sessions
        .iter()
        .find(|info| info.id == id)
        .ok_or(PartnerServiceError::UnknownSession(id))
}

/// The named project session `from` belongs to.
fn sender_project<'a>(
    book: &'a PartnerBook,
    sessions: &[SessionInfo],
    from: SessionId,
    root: &dyn Fn(&Path) -> String,
) -> Result<&'a NamedProject, PartnerServiceError> {
    let sender = session(sessions, from)?;
    if sender.kind == SessionKind::Update {
        return Err(PartnerError::NoProject.into());
    }
    let (root_text, cwd) = place_of(sender, root);
    book.project_of(&root_text, &cwd)
        .ok_or(PartnerError::NoProject.into())
}

/// What the service asks of the machine, stand-ins in tests.
pub struct Probe<'a> {
    /// A folder's project root ([`project_root`]).
    pub root: &'a dyn Fn(&Path) -> String,
    /// The profile's program is a shell ([`crate::partners::is_shell_command`]): a message pasted
    /// there would run as commands, so a shell session is never a target.
    pub is_shell: &'a dyn Fn(&str) -> bool,
}

/// The partner `partner` of session `from`'s project and its agent sessions (no update and no
/// shell sessions).
pub fn targets(
    book: &PartnerBook,
    sessions: &[SessionInfo],
    from: SessionId,
    partner: &str,
    probe: &Probe<'_>,
) -> Result<PartnerTargets, PartnerServiceError> {
    let sender = sender_project(book, sessions, from, probe.root)?;
    let target = book.partner(&sender.name, partner)?;
    let found = sessions
        .iter()
        .filter(|info| info.kind == SessionKind::Agent && !(probe.is_shell)(&info.profile_id))
        .filter(|info| {
            let (root_text, cwd) = place_of(info, probe.root);
            in_project(
                target,
                &SessionPlace {
                    root: &root_text,
                    cwd: &cwd,
                },
            )
        })
        .map(|info| PartnerSession {
            id: info.id,
            profile_id: info.profile_id.clone(),
            running: info.status != Status::Exited,
        })
        .collect();
    Ok(PartnerTargets {
        sender: sender.name.clone(),
        partner: target.clone(),
        sessions: found,
    })
}

/// The text to paste into session `to` for a message from session `from` to `partner`: checked
/// again at the moment of delivery (the two are partners, `to` runs in the partner project) and
/// cleaned and headed ([`compose`]). The caller logs the delivery.
pub fn prepare(
    book: &PartnerBook,
    sessions: &[SessionInfo],
    from: SessionId,
    to: SessionId,
    partner: &str,
    text: &str,
    probe: &Probe<'_>,
) -> Result<String, PartnerServiceError> {
    let found = targets(book, sessions, from, partner, probe)?;
    if !found.sessions.iter().any(|s| s.id == to && s.running) {
        return Err(PartnerError::WrongTarget.into());
    }
    Ok(compose(&found.sender, text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[cfg(windows)]
    const BASE: &str = r"C:\work";
    #[cfg(not(windows))]
    const BASE: &str = "/work";

    fn folder(name: &str) -> String {
        Path::new(BASE).join(name).to_string_lossy().into_owned()
    }

    fn info(id: SessionId, cwd: &str) -> SessionInfo {
        let mut info = SessionInfo::new(id, "claude".into(), "t".into(), PathBuf::from(cwd), 0);
        info.status = Status::Running;
        info
    }

    fn book() -> PartnerBook {
        PartnerBook::default()
            .link(
                ProjectEnd {
                    folder: &folder("app"),
                    name: "app",
                },
                ProjectEnd {
                    folder: &folder("web"),
                    name: "web",
                },
            )
            .unwrap()
    }

    /// Every folder is its own root, except worktrees under `trees`, which belong to `web`.
    fn root(path: &Path) -> String {
        let text = path.to_string_lossy().into_owned();
        if text.starts_with(&folder("trees")) {
            folder("web")
        } else {
            text
        }
    }

    fn is_shell(profile: &str) -> bool {
        profile == "pwsh"
    }

    const PROBE: Probe<'static> = Probe {
        root: &root,
        is_shell: &is_shell,
    };

    #[test]
    fn shell_sessions_are_never_targets() {
        let mut shell = info(2, &folder("web"));
        shell.profile_id = "pwsh".into();
        let sessions = vec![info(1, &folder("app")), shell];
        assert!(targets(&book(), &sessions, 1, "web", &PROBE)
            .unwrap()
            .sessions
            .is_empty());
        assert!(matches!(
            prepare(&book(), &sessions, 1, 2, "web", "dir", &PROBE),
            Err(PartnerServiceError::Partner(PartnerError::WrongTarget))
        ));
        // A shell session may still send: only the target must be an agent.
        let mut back = sessions.clone();
        back.push(info(3, &folder("app")));
        back[0].profile_id = "pwsh".into();
        let from_shell = targets(&book(), &back, 1, "web", &PROBE).unwrap();
        assert_eq!(from_shell.sender, "app");
    }

    #[test]
    fn targets_are_the_partner_projects_agent_sessions() {
        let mut ended = info(4, &folder("web"));
        ended.status = Status::Exited;
        let mut update = info(5, &folder("web"));
        update.kind = SessionKind::Update;
        let sessions = vec![
            info(1, &folder("app")),
            info(2, &Path::new(&folder("web")).join("src").to_string_lossy()),
            info(3, &Path::new(&folder("trees")).join("w1").to_string_lossy()),
            ended,
            update,
            info(6, &folder("other")),
        ];
        let found = targets(&book(), &sessions, 1, "web", &PROBE).unwrap();
        assert_eq!(found.sender, "app");
        assert_eq!(found.partner.name, "web");
        assert_eq!(
            found
                .sessions
                .iter()
                .map(|s| (s.id, s.running))
                .collect::<Vec<_>>(),
            [(2, true), (3, true), (4, false)]
        );
        // And back: the pair holds both ways.
        let back = targets(&book(), &sessions, 2, "app", &PROBE).unwrap();
        assert_eq!(back.sender, "web");
        assert_eq!(back.sessions.len(), 1);
    }

    #[test]
    fn only_partners_and_known_sessions_count() {
        let sessions = vec![info(1, &folder("app")), info(6, &folder("other"))];
        assert!(matches!(
            targets(&book(), &sessions, 1, "relay", &PROBE),
            Err(PartnerServiceError::Partner(PartnerError::NotAPartner(_)))
        ));
        assert!(matches!(
            targets(&book(), &sessions, 6, "web", &PROBE),
            Err(PartnerServiceError::Partner(PartnerError::NoProject))
        ));
        assert!(matches!(
            targets(&book(), &sessions, 9, "web", &PROBE),
            Err(PartnerServiceError::UnknownSession(9))
        ));
    }

    #[test]
    fn delivery_checks_the_target_and_cleans_the_text() {
        let mut ended = info(4, &folder("web"));
        ended.status = Status::Exited;
        let sessions = vec![info(1, &folder("app")), info(2, &folder("web")), ended];
        assert_eq!(
            prepare(
                &book(),
                &sessions,
                1,
                2,
                "web",
                "go\u{1b}[201~\r\non",
                &PROBE
            )
            .unwrap(),
            "[Message from project \"app\"]\ngo[201~\non"
        );
        for to in [1, 4, 7] {
            assert!(
                matches!(
                    prepare(&book(), &sessions, 1, to, "web", "x", &PROBE),
                    Err(PartnerServiceError::Partner(PartnerError::WrongTarget))
                ),
                "{to}"
            );
        }
        assert!(matches!(
            prepare(&book(), &sessions, 1, 2, "web", " \u{7} ", &PROBE),
            Err(PartnerServiceError::Partner(PartnerError::EmptyMessage))
        ));
    }

    #[test]
    fn linking_needs_existing_local_folders() {
        let tmp = crate::test_support::TempDir::new("partner-link");
        let store = MainStore::new(tmp.path().join("store"));
        let app = tmp.path().join("app");
        let web = tmp.path().join("web");
        // Each its own repository: the test folder lies inside Hangar's own, whose root both
        // would otherwise resolve to.
        std::fs::create_dir_all(app.join(".git")).unwrap();
        let (app_text, web_text) = (app.to_string_lossy(), web.to_string_lossy());
        assert!(matches!(
            link(&store, &app_text, "app", &web_text, "web"),
            Err(PartnerServiceError::Partner(PartnerError::MissingFolder))
        ));
        assert!(matches!(
            link(&store, &app_text, "app", "relative/web", "web"),
            Err(PartnerServiceError::Partner(PartnerError::InvalidFolder))
        ));
        std::fs::create_dir_all(web.join(".git")).unwrap();
        let book = link(&store, &app_text, "app", &web_text, "web").unwrap();
        assert_eq!(store.load_partners(), book);
        assert_eq!(
            unlink(&store, "web", "app").unwrap(),
            PartnerBook::default()
        );
        assert_eq!(store.load_partners(), PartnerBook::default());
    }

    #[test]
    fn dotted_paths_are_refused_and_links_resolve_to_their_folder() {
        let tmp = crate::test_support::TempDir::new("partner-link-resolve");
        let store = MainStore::new(tmp.path().join("store"));
        let app = tmp.path().join("app");
        std::fs::create_dir_all(app.join(".git")).unwrap();
        let app_text = app.to_string_lossy();
        let dotted = app.join("..").join("app");
        assert!(matches!(
            link(&store, &app_text, "app", &dotted.to_string_lossy(), "web"),
            Err(PartnerServiceError::Partner(PartnerError::InvalidFolder))
        ));
        // A junction (Windows) or symlink to the project itself is the project itself.
        let alias = tmp.path().join("alias");
        crate::no_links::tests::folder_link(&alias, &app);
        assert!(matches!(
            link(&store, &app_text, "app", &alias.to_string_lossy(), "web"),
            Err(PartnerServiceError::Partner(PartnerError::SelfPartner))
        ));
        assert_eq!(store.load_partners(), PartnerBook::default());
    }
}
