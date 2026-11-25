use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use hangar_team_crypto::recovery::RecoveryCode;

use super::*;
use crate::git::tests::{git, git_ok, init_repo};
use crate::test_support::TempDir;

const LABEL: &str = "other version";
const UNTITLED: &str = "Untitled";

/// In-memory keychain. Debug shows nothing.
#[derive(Default)]
struct MemoryStore(Mutex<HashMap<String, Zeroizing<String>>>);

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        Ok(lock(&self.0).get(account).cloned())
    }
    fn exists(&self, account: &str) -> Result<bool, SecretError> {
        Ok(lock(&self.0).contains_key(account))
    }
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        lock(&self.0).insert(account.to_owned(), Zeroizing::new(value.to_owned()));
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        lock(&self.0).remove(account);
        Ok(())
    }
}

fn store(dir: &Path) -> (ProjectNotes, Arc<MemoryStore>) {
    let keychain = Arc::new(MemoryStore::default());
    (
        ProjectNotes::new(&dir.join("app"), keychain.clone()),
        keychain,
    )
}

fn shared(path: &Path) -> NoteFolder {
    NoteFolder {
        path: Some(path.to_path_buf()),
        shared: true,
    }
}

fn private(path: &Path) -> NoteFolder {
    NoteFolder {
        path: Some(path.to_path_buf()),
        shared: false,
    }
}

fn plain(title: &str, updated: u64, by: &str, body: &str) -> Plain {
    Plain {
        meta: NoteMeta {
            v: 1,
            title: title.to_owned(),
            created: 1,
            updated,
            by: by.to_owned(),
        },
        body: Zeroizing::new(body.to_owned()),
    }
}

/// A repository with one commit, and the store with shared notes turned on. Returns the code.
fn enabled(g: &Git, dir: &Path) -> (PathBuf, ProjectNotes, String) {
    let repo = dir.join("repo");
    init_repo(g, &repo);
    fs::write(repo.join("readme.txt"), "x\n").unwrap();
    git_ok(g, &repo, &["add", "readme.txt"]);
    git_ok(g, &repo, &["commit", "-qm", "root"]);
    let (notes, _) = store(dir);
    assert_eq!(
        notes.status(&shared(&repo), LABEL, UNTITLED).unwrap(),
        SharedStatus::Off
    );
    let code = notes.prepare(&shared(&repo)).unwrap().to_string();
    (repo, notes, code)
}

#[test]
fn vault_json_round_trips_and_refuses_damage() {
    let vault = Vault {
        vault: VaultId::from_hex("00112233445566778899aabbccddeeff").unwrap(),
        mode: VaultMode::Solo {
            code_slot: "AQID".to_owned(),
        },
    };
    let text = vault_json(&vault);
    assert_eq!(
        text,
        "{\n  \"format\": 1,\n  \"vault\": \"00112233445566778899aabbccddeeff\",\n  \"mode\": \"solo\",\n  \"codeSlot\": \"AQID\"\n}\n"
    );
    assert_eq!(parse_vault(text.as_bytes()), VaultFile::Known(vault));
    let team = Vault {
        vault: VaultId::from_hex("00112233445566778899aabbccddeeff").unwrap(),
        mode: VaultMode::Team {
            team: "0011223344556677".to_owned(),
        },
    };
    assert_eq!(
        parse_vault(vault_json(&team).as_bytes()),
        VaultFile::Known(team)
    );
    for (text, want) in [
        (r#"{"format":2}"#, VaultFile::Newer),
        (
            r#"{"format":1,"vault":"00112233445566778899aabbccddeeff","mode":"group"}"#,
            VaultFile::Newer,
        ),
        (r#"{"format":0}"#, VaultFile::Damaged),
        ("not json", VaultFile::Damaged),
        (
            r#"{"format":1,"vault":"00","mode":"solo","codeSlot":"x"}"#,
            VaultFile::Damaged,
        ),
        (
            r#"{"format":1,"vault":"00112233445566778899aabbccddeeff","mode":"solo"}"#,
            VaultFile::Damaged,
        ),
        (
            r#"{"format":1,"vault":"00112233445566778899aabbccddeeff","mode":"team","team":"XYZ"}"#,
            VaultFile::Damaged,
        ),
    ] {
        assert_eq!(parse_vault(text.as_bytes()), want, "{text}");
    }
}

#[test]
fn plaintext_round_trips() {
    let note = plain("Plan", 5, "", "# Plan\nline\n");
    let bytes = encode_plain(&note).unwrap();
    assert!(bytes.starts_with(br#"{"v":1,"title":"Plan","created":1,"updated":5,"by":""}"#));
    assert_eq!(decode_plain(&bytes, UNTITLED).unwrap(), note);
    // An empty body, a title the app refuses, and a newer metadata version.
    let empty = plain("Empty", 1, "", "");
    assert_eq!(
        decode_plain(&encode_plain(&empty).unwrap(), UNTITLED).unwrap(),
        empty
    );
    let bad = br#"{"v":1,"title":"a\nb","created":1,"updated":1,"by":""}
x"#;
    assert_eq!(decode_plain(bad, UNTITLED).unwrap().meta.title, UNTITLED);
    let newer = br#"{"v":2,"title":"a","created":1,"updated":1,"by":""}
x"#;
    assert!(matches!(
        decode_plain(newer, UNTITLED),
        Err(ProjectNotesError::NewerFormat)
    ));
    assert!(matches!(
        decode_plain(b"no line", UNTITLED),
        Err(ProjectNotesError::Unreadable)
    ));
}

#[test]
fn conflicts_keep_both_versions() {
    let ours = plain("Plan", 10, "Ada", "ours");
    let theirs = plain("Plan", 20, "Bob", "theirs");
    assert_eq!(
        resolve(Some(&ours), Some(&theirs), LABEL),
        Some(Resolution::KeepBoth {
            keep_ours: false,
            copy_title: "Plan (other version, Ada)".to_owned()
        })
    );
    // A tie keeps ours; solo notes have no name.
    let solo = plain("Plan", 20, "", "solo");
    assert_eq!(
        resolve(Some(&solo), Some(&theirs), LABEL),
        Some(Resolution::KeepBoth {
            keep_ours: true,
            copy_title: "Plan (other version, Bob)".to_owned()
        })
    );
    assert_eq!(
        resolve(Some(&theirs), Some(&solo), LABEL),
        Some(Resolution::KeepBoth {
            keep_ours: true,
            copy_title: "Plan (other version)".to_owned()
        })
    );
    // Equal content needs no copy; a deleted side loses; nothing at all stays unresolved.
    let same = plain("Plan", 30, "Bob", "theirs");
    assert_eq!(
        resolve(Some(&same), Some(&theirs), LABEL),
        Some(Resolution::KeepOurs)
    );
    assert_eq!(
        resolve(Some(&ours), None, LABEL),
        Some(Resolution::KeepOurs)
    );
    assert_eq!(
        resolve(None, Some(&theirs), LABEL),
        Some(Resolution::KeepTheirs)
    );
    assert_eq!(resolve(None, None, LABEL), None);
    // Long titles stay within the limit.
    let long = "x".repeat(notes::MAX_TITLE_CHARS);
    let title = copy_title(&long, LABEL, "Ada");
    assert_eq!(title.chars().count(), notes::MAX_TITLE_CHARS);
    assert!(title.ends_with(" (other version, Ada)"));
}

#[test]
fn labels_are_checked() {
    assert_eq!(check_label(" second copy ").unwrap(), "second copy");
    for bad in ["", "a\nb", "a(b", "a,b", &"x".repeat(41)] {
        assert!(check_label(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn git_output_parsing() {
    let list = b"worktree C:/work/repo\nHEAD abc\nbranch refs/heads/main\n\nworktree C:/work/task\nHEAD def\n\n";
    assert_eq!(main_worktree(list), Some(git::native_path("C:/work/repo")));
    assert_eq!(main_worktree(b"worktree /srv/repo.git\nbare\n\n"), None);
    assert_eq!(main_worktree(b""), None);
    let status = b"UU .hangar/notes/0f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b.note\0?? .hangar/notes/vault.json\0R  .hangar/notes/b.note\0.hangar/notes/a.note\0";
    let entries = parse_status(status);
    assert_eq!(entries.len(), 3);
    assert!(entries[0].unmerged());
    assert!(!entries[1].unmerged());
    assert_eq!(
        note_path_id(&entries[0].path),
        Some("0f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b")
    );
    assert_eq!(note_path_id(".hangar/notes/vault.json"), None);
    assert_eq!(
        note_path_id(".hangar/notes/sub/0f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b.note"),
        None
    );
}

#[test]
fn outside_a_repository_there_is_no_shared_group() {
    let dir = TempDir::new("shared-notes-none");
    let (notes, _) = store(dir.path());
    let missing = dir.path().join("missing");
    assert_eq!(
        notes.status(&shared(&missing), LABEL, UNTITLED).unwrap(),
        SharedStatus::Unavailable
    );
    assert!(matches!(
        notes.prepare(&shared(&missing)),
        Err(ProjectNotesError::NotRepository)
    ));
}

#[test]
fn turn_on_write_lock_and_unlock() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-flow");
    let (repo, notes, code) = enabled(&g, dir.path());
    assert!(code.starts_with('N'));
    // A second dialog (or effects run twice) gets the same code, never a new one.
    assert_eq!(*notes.prepare(&shared(&repo)).unwrap(), code);
    assert_eq!(*notes.pending_code(&shared(&repo)).unwrap(), code);
    let status = notes.enable(&shared(&repo), LABEL, UNTITLED).unwrap();
    let SharedStatus::Unlocked {
        uncommitted,
        ignored,
        unreadable,
        resolved,
        ..
    } = status
    else {
        panic!("{status:?}");
    };
    // .gitattributes, README.md and vault.json, none staged.
    assert_eq!((uncommitted, ignored, unreadable), (3, false, 0));
    assert!(resolved.is_empty());
    let dir_notes = repo.join(".hangar").join("notes");
    assert_eq!(
        fs::read_to_string(dir_notes.join(".gitattributes")).unwrap(),
        "* -text -diff -merge\n"
    );
    assert!(matches!(
        parse_vault(&fs::read(dir_notes.join("vault.json")).unwrap()),
        VaultFile::Known(Vault {
            mode: VaultMode::Solo { .. },
            ..
        })
    ));
    assert!(matches!(
        notes.enable(&shared(&repo), LABEL, UNTITLED),
        Err(ProjectNotesError::NoSetup)
    ));

    let folder = shared(&repo);
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes
        .write_note(&folder, &id, "# Plan\nsecret text")
        .unwrap();
    let file = dir_notes.join(format!("{id}.note"));
    let bytes = fs::read(&file).unwrap();
    assert!(bytes.starts_with(b"hangar-note 1 "));
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("secret") && !text.contains("Plan"), "{text}");
    // No change, no write: the bytes stay the same.
    notes
        .write_note(&folder, &id, "# Plan\nsecret text")
        .unwrap();
    notes.rename_note(&folder, &id, "Plan").unwrap();
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert_eq!(
        notes.read_note(&folder, &id).unwrap(),
        "# Plan\nsecret text"
    );
    notes.rename_note(&folder, &id, "Roadmap").unwrap();
    let listed = notes.list_notes(&folder).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "Roadmap");

    // Another computer: no key in its keychain.
    let other_dir = dir.path().join("other");
    let (other, other_keychain) = store(&other_dir);
    assert_eq!(
        other.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Locked {
            reason: LockReason::NeedsCode
        }
    );
    assert!(matches!(
        other.list_notes(&folder),
        Err(ProjectNotesError::Locked)
    ));
    let wrong = NotesRecoveryCode::generate(&mut OsEntropy)
        .unwrap()
        .reveal();
    assert_eq!(
        other.unlock(&folder, &wrong, LABEL, UNTITLED).unwrap(),
        Unlock::WrongCode
    );
    let team = RecoveryCode::generate(&mut OsEntropy).unwrap().reveal();
    assert_eq!(
        other.unlock(&folder, &team, LABEL, UNTITLED).unwrap(),
        Unlock::TeamCode
    );
    let mut typo: Vec<char> = code.chars().collect();
    typo[12] = if typo[12] == '2' { '3' } else { '2' };
    let typo: String = typo.into_iter().collect();
    assert_eq!(
        other.unlock(&folder, &typo, LABEL, UNTITLED).unwrap(),
        Unlock::Typo
    );
    let status = other
        .unlock(&folder, &code.to_lowercase(), LABEL, UNTITLED)
        .unwrap();
    assert!(
        matches!(
            status,
            Unlock::Unlocked {
                status: SharedStatus::Unlocked { .. }
            }
        ),
        "{status:?}"
    );
    assert_eq!(
        other.read_note(&folder, &id).unwrap(),
        "# Plan\nsecret text"
    );
    let accounts: Vec<String> = lock(&other_keychain.0).keys().cloned().collect();
    assert_eq!(accounts.len(), 1);
    // A fixed message: keychain account names are never printed, not even in a failing test.
    assert!(
        accounts[0].starts_with("notes/"),
        "the keychain entry lies outside the notes namespace"
    );
    // The keychain now unlocks a fresh start of that computer.
    let again = ProjectNotes::new(&other_dir.join("app"), other_keychain);
    assert!(matches!(
        again.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked { .. }
    ));
}

#[test]
fn task_worktrees_share_the_main_worktree_notes() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-worktree");
    let (repo, notes, _) = enabled(&g, dir.path());
    notes.enable(&shared(&repo), LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&shared(&repo), "Main").unwrap();
    let task = dir.path().join("task");
    git_ok(
        &g,
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task",
            task.to_str().unwrap(),
        ],
    );
    let listed = notes.list_notes(&shared(&task)).unwrap();
    assert_eq!(
        listed.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        [id.as_str()]
    );
    let from_task = notes.create_note(&shared(&task), "From task").unwrap();
    assert!(repo
        .join(".hangar/notes")
        .join(format!("{from_task}.note"))
        .is_file());
    assert!(!task.join(".hangar").exists());
}

#[test]
fn trash_restore_and_move() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-trash");
    let (repo, notes, _) = enabled(&g, dir.path());
    notes.enable(&shared(&repo), LABEL, UNTITLED).unwrap();
    let folder = shared(&repo);
    let id = notes.create_note(&folder, "Gone").unwrap();
    notes.write_note(&folder, &id, "body").unwrap();
    notes.delete_note(&folder, &id).unwrap();
    assert!(notes.list_notes(&folder).unwrap().is_empty());
    let trash = notes.list_trash(&folder).unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].title, "Gone");
    assert_eq!(notes.restore_note(&folder, &id).unwrap(), id);
    assert_eq!(notes.read_note(&folder, &id).unwrap(), "body");
    assert!(notes.list_trash(&folder).unwrap().is_empty());

    // Private to shared and back; the source goes to its trash.
    let private_store = NotesStore::new(&dir.path().join("app"));
    let mine = private(&repo);
    let p = private_store.create_note(&mine, "Mine").unwrap();
    private_store.write_note(&mine, &p, "private text").unwrap();
    let moved = move_note(&private_store, &notes, &mine, &p, &folder).unwrap();
    assert_eq!(notes.read_note(&folder, &moved).unwrap(), "private text");
    assert!(private_store.list_notes(&mine).unwrap().is_empty());
    let back = move_note(&private_store, &notes, &folder, &moved, &mine).unwrap();
    assert_eq!(
        private_store.read_note(&mine, &back).unwrap(),
        "private text"
    );
    assert_eq!(notes.list_notes(&folder).unwrap().len(), 1);

    // Search finds the shared note, decrypted in memory.
    let hits = notes.search("BODY").unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.shared);
    assert_eq!(hits[0].1.id, id);
}

#[test]
fn ignored_folder_is_reported() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-ignored");
    let (repo, notes, _) = enabled(&g, dir.path());
    fs::write(repo.join(".gitignore"), ".hangar/\n").unwrap();
    let status = notes.enable(&shared(&repo), LABEL, UNTITLED).unwrap();
    assert!(
        matches!(status, SharedStatus::Unlocked { ignored: true, .. }),
        "{status:?}"
    );
}

#[test]
fn merge_conflicts_are_resolved_without_loss() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-conflict");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes.write_note(&folder, &id, "base").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes"]);
    git_ok(&g, &repo, &["checkout", "-qb", "other"]);
    notes.write_note(&folder, &id, "theirs, older").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "theirs"]);
    git_ok(&g, &repo, &["checkout", "-q", "main"]);
    std::thread::sleep(Duration::from_millis(5));
    notes.write_note(&folder, &id, "ours, newer").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "ours"]);
    let merge = g
        .run(
            &repo,
            [
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.invalid",
                "merge",
                "other",
            ],
            Instant::now() + Duration::from_secs(30),
            1 << 20,
        )
        .unwrap();
    assert!(!merge.success);

    let status = notes.status(&folder, LABEL, UNTITLED).unwrap();
    let SharedStatus::Unlocked { resolved, .. } = status else {
        panic!("{status:?}");
    };
    assert_eq!(
        resolved,
        [ResolvedConflict {
            title: "Plan".to_owned(),
            copy: Some("Plan (other version)".to_owned()),
        }]
    );
    assert_eq!(notes.read_note(&folder, &id).unwrap(), "ours, newer");
    let listed = notes.list_notes(&folder).unwrap();
    assert_eq!(listed.len(), 2);
    let copy = listed.iter().find(|n| n.id != id).unwrap();
    assert_eq!(copy.title, "Plan (other version)");
    assert_eq!(notes.read_note(&folder, &copy.id).unwrap(), "theirs, older");
    // Both files are staged, nothing is unmerged and nothing was committed.
    let output = g
        .run(
            &repo,
            ["status", "--porcelain=v1", "-z", "--", NOTES_PATH],
            Instant::now() + Duration::from_secs(30),
            1 << 20,
        )
        .unwrap();
    let entries = parse_status(&output.stdout);
    assert!(entries.iter().all(|e| !e.unmerged()), "{entries:?}");
    assert!(entries
        .iter()
        .any(|e| e.code == "A " && e.path.contains(&copy.id)));
    git_ok(&g, &repo, &["commit", "-qm", "merged"]);
}

/// A git command that may fail (a merge or rebase that stops on a conflict); its success.
fn git_try(g: &Git, repo: &Path, args: &[&str]) -> bool {
    let mut all = vec![
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.editor=true",
    ];
    all.extend_from_slice(args);
    g.run(
        repo,
        &all,
        Instant::now() + Duration::from_secs(30),
        1 << 20,
    )
    .unwrap()
    .success
}

fn unmerged_paths(g: &Git, repo: &Path) -> Vec<StatusEntry> {
    let output = g
        .run(
            repo,
            ["status", "--porcelain=v1", "-z", "--", NOTES_PATH],
            Instant::now() + Duration::from_secs(30),
            1 << 20,
        )
        .unwrap();
    parse_status(&output.stdout)
        .into_iter()
        .filter(StatusEntry::unmerged)
        .collect()
}

#[test]
fn a_rebase_keeps_the_newer_version_although_its_stages_are_swapped() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-rebase");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes.write_note(&folder, &id, "base").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes"]);
    // main changes the note first (older), the task branch later (newer).
    notes.write_note(&folder, &id, "main, older").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "main"]);
    git_ok(&g, &repo, &["checkout", "-q", "-b", "task", "HEAD~1"]);
    std::thread::sleep(Duration::from_millis(5));
    notes.write_note(&folder, &id, "task, newer").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "task"]);
    // Rebasing the task onto main: stage 2 is main (older), stage 3 the task commit (newer).
    assert!(!git_try(&g, &repo, &["rebase", "main"]));
    assert_eq!(unmerged_paths(&g, &repo).len(), 1);

    let status = notes.status(&folder, LABEL, UNTITLED).unwrap();
    let SharedStatus::Unlocked { resolved, .. } = status else {
        panic!("{status:?}");
    };
    assert_eq!(resolved.len(), 1, "{resolved:?}");
    assert_eq!(notes.read_note(&folder, &id).unwrap(), "task, newer");
    let listed = notes.list_notes(&folder).unwrap();
    let copy = listed.iter().find(|n| n.id != id).expect("copy");
    assert_eq!(notes.read_note(&folder, &copy.id).unwrap(), "main, older");
    assert!(unmerged_paths(&g, &repo).is_empty());
    assert!(git_try(&g, &repo, &["rebase", "--continue"]));
}

#[test]
fn a_note_deleted_on_one_side_comes_back_with_the_other_sides_text() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-modify-delete");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes.write_note(&folder, &id, "base").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes"]);
    git_ok(&g, &repo, &["checkout", "-qb", "other"]);
    notes.write_note(&folder, &id, "kept on other").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "edit"]);
    git_ok(&g, &repo, &["checkout", "-q", "main"]);
    notes.delete_note(&folder, &id).unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "delete"]);
    // main deleted it (ours), other changed it (theirs): DU.
    assert!(!git_try(&g, &repo, &["merge", "other"]));
    let conflicts = unmerged_paths(&g, &repo);
    assert_eq!(
        conflicts
            .iter()
            .map(|e| e.code.as_str())
            .collect::<Vec<_>>(),
        ["DU"]
    );

    let status = notes.status(&folder, LABEL, UNTITLED).unwrap();
    let SharedStatus::Unlocked { resolved, .. } = status else {
        panic!("{status:?}");
    };
    assert_eq!(
        resolved,
        [ResolvedConflict {
            title: "Plan".to_owned(),
            copy: None
        }]
    );
    assert_eq!(notes.read_note(&folder, &id).unwrap(), "kept on other");
    assert_eq!(notes.list_notes(&folder).unwrap().len(), 1);
    assert!(unmerged_paths(&g, &repo).is_empty());
}

#[test]
fn without_the_notes_folder_the_status_is_off() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-off");
    let repo = dir.path().join("repo");
    init_repo(&g, &repo);
    let (notes, _) = store(dir.path());
    assert_eq!(
        notes.status(&shared(&repo), LABEL, UNTITLED).unwrap(),
        SharedStatus::Off
    );
    // A folder without vault.json is off as well.
    fs::create_dir_all(repo.join(".hangar").join("notes")).unwrap();
    assert_eq!(
        notes.status(&shared(&repo), LABEL, UNTITLED).unwrap(),
        SharedStatus::Off
    );
}

#[test]
fn two_branches_that_turned_notes_on_end_in_one_vault() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-two-vaults");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    git_ok(&g, &repo, &["branch", "other"]);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let on_main = notes.create_note(&folder, "Main").unwrap();
    notes.write_note(&folder, &on_main, "from main").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes on main"]);
    git_ok(&g, &repo, &["checkout", "-q", "other"]);
    // The same computer turns notes on again on the other branch: a second vault.
    notes.prepare(&folder).unwrap();
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let on_other = notes.create_note(&folder, "Other").unwrap();
    notes.write_note(&folder, &on_other, "from other").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes on other"]);
    git_ok(&g, &repo, &["checkout", "-q", "main"]);
    assert!(!git_try(&g, &repo, &["merge", "other"]));
    let conflicts = unmerged_paths(&g, &repo);
    assert_eq!(
        conflicts
            .iter()
            .map(|e| e.code.as_str())
            .collect::<Vec<_>>(),
        ["AA"]
    );

    // This computer holds both key rings: every note opens in the vault that stays.
    let status = notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert!(
        matches!(status, SharedStatus::Unlocked { unreadable: 0, .. }),
        "{status:?}"
    );
    assert!(unmerged_paths(&g, &repo).is_empty());
    assert_eq!(notes.read_note(&folder, &on_main).unwrap(), "from main");
    assert_eq!(notes.read_note(&folder, &on_other).unwrap(), "from other");

    // Another computer still needs the code of the vault that stayed.
    let (other, _) = store(&dir.path().join("second"));
    assert!(matches!(
        other.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Locked {
            reason: LockReason::NeedsCode
        }
    ));
}

#[test]
fn a_worktree_of_a_bare_clone_keeps_its_own_notes() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-bare");
    let (repo, notes, _) = enabled(&g, dir.path());
    let bare = dir.path().join("bare.git");
    git_ok(
        &g,
        dir.path(),
        &[
            "clone",
            "-q",
            "--bare",
            repo.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let task = dir.path().join("bare-task");
    git_ok(
        &g,
        &bare,
        &["worktree", "add", "-q", task.to_str().unwrap(), "main"],
    );
    let folder = shared(&task);
    notes.prepare(&folder).unwrap();
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    assert!(task.join(".hangar/notes/vault.json").is_file());
    assert!(!bare.join(".hangar").exists());
}

/// A keychain whose writes fail.
#[derive(Default)]
struct ReadOnlyStore;

impl SecretStore for ReadOnlyStore {
    fn get(&self, _: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        Ok(None)
    }
    fn exists(&self, _: &str) -> Result<bool, SecretError> {
        Ok(false)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
        Err(SecretError::Denied("test".to_owned()))
    }
    fn delete(&self, _: &str) -> Result<(), SecretError> {
        Ok(())
    }
}

/// On `other`, from the root commit, a hand-written `vault.json`; merged into main where notes
/// are on. Returns the unmerged entries.
fn vault_conflict(g: &Git, repo: &Path, foreign: &str) -> Vec<StatusEntry> {
    git_ok(g, repo, &["add", "-A"]);
    git_ok(g, repo, &["commit", "-qm", "notes on main"]);
    git_ok(g, repo, &["checkout", "-q", "-b", "other", "HEAD~1"]);
    fs::create_dir_all(repo.join(".hangar/notes")).unwrap();
    fs::write(repo.join(".hangar/notes/vault.json"), foreign).unwrap();
    git_ok(g, repo, &["add", "-A"]);
    git_ok(g, repo, &["commit", "-qm", "foreign vault"]);
    git_ok(g, repo, &["checkout", "-q", "main"]);
    assert!(!git_try(g, repo, &["merge", "other"]));
    unmerged_paths(g, repo)
}

#[test]
fn a_foreign_vault_never_wins_by_its_id() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-foreign-vault");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let ours = fs::read(repo.join(".hangar/notes/vault.json")).unwrap();
    // The smallest possible id, without a key on this computer.
    let foreign = "{\n  \"format\": 1,\n  \"vault\": \"00000000000000000000000000000000\",\n  \"mode\": \"solo\",\n  \"codeSlot\": \"AQID\"\n}\n";
    let conflicts = vault_conflict(&g, &repo, foreign);
    assert_eq!(conflicts.len(), 1);
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert_eq!(
        fs::read(repo.join(".hangar/notes/vault.json")).unwrap(),
        ours
    );
    assert_eq!(unmerged_paths(&g, &repo).len(), 1, "the conflict stays");
}

#[test]
fn a_newer_vault_is_never_overwritten() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-newer-vault");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    vault_conflict(&g, &repo, "{\"format\": 2}\n");
    // Ours stays in the working tree, theirs in the index; nothing is staged over it.
    let before = fs::read(repo.join(".hangar/notes/vault.json")).unwrap();
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert_eq!(
        fs::read(repo.join(".hangar/notes/vault.json")).unwrap(),
        before
    );
    assert_eq!(unmerged_paths(&g, &repo).len(), 1);
}

#[test]
fn expected_stages_follow_the_xy_code() {
    assert_eq!(expected_stages("UU"), Some((true, true)));
    assert_eq!(expected_stages("AA"), Some((true, true)));
    assert_eq!(expected_stages("UD"), Some((true, false)));
    assert_eq!(expected_stages("AU"), Some((true, false)));
    assert_eq!(expected_stages("DU"), Some((false, true)));
    assert_eq!(expected_stages("UA"), Some((false, true)));
    assert_eq!(expected_stages("DD"), None);
    assert_eq!(expected_stages(" M"), None);
}

#[test]
fn a_note_both_sides_renamed_away_is_dropped_from_the_index() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-dd");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes.write_note(&folder, &id, "text").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes"]);
    let from = format!("{NOTES_PATH}/{id}.note");
    let a = format!("{NOTES_PATH}/00000000-0000-4000-8000-00000000000a.note");
    let b = format!("{NOTES_PATH}/00000000-0000-4000-8000-00000000000b.note");
    git_ok(&g, &repo, &["checkout", "-qb", "other"]);
    git_ok(&g, &repo, &["mv", &from, &b]);
    git_ok(&g, &repo, &["commit", "-qm", "b"]);
    git_ok(&g, &repo, &["checkout", "-q", "main"]);
    git_ok(&g, &repo, &["mv", &from, &a]);
    git_ok(&g, &repo, &["commit", "-qm", "a"]);
    assert!(!git_try(&g, &repo, &["merge", "other"]));
    let codes: Vec<String> = unmerged_paths(&g, &repo)
        .into_iter()
        .filter(|e| e.path == from)
        .map(|e| e.code)
        .collect();
    assert_eq!(codes, ["DD"]);
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert!(unmerged_paths(&g, &repo).iter().all(|e| e.path != from));
}

#[test]
fn the_recovery_code_is_not_saved_inside_a_repository() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-code-path");
    let (repo, _, _) = enabled(&g, dir.path());
    assert!(!code_path_allowed(&repo, &repo.join("code.txt")));
    assert!(!code_path_allowed(
        &repo,
        &repo.join(".git").join("code.txt")
    ));
    // Outside this repository but inside another work tree (the test folder lies in Hangar's).
    assert!(!code_path_allowed(&repo, &dir.path().join("code.txt")));
    assert!(!code_path_allowed(
        &repo,
        &dir.path().join("missing").join("code.txt")
    ));
}

#[test]
fn a_failing_keychain_keeps_the_setup() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-keychain-fails");
    let (repo, _, _) = enabled(&g, dir.path());
    let notes = ProjectNotes::new(&dir.path().join("app2"), Arc::new(ReadOnlyStore));
    let folder = shared(&repo);
    let code = notes.prepare(&folder).unwrap().to_string();
    assert!(matches!(
        notes.enable(&folder, LABEL, UNTITLED),
        Err(ProjectNotesError::Keychain(_))
    ));
    assert!(!repo.join(".hangar").exists());
    // The same code is still the one a retry seals.
    assert_eq!(*notes.pending_code(&folder).unwrap(), code);
    notes.cancel(&folder).unwrap();
    assert!(matches!(
        notes.pending_code(&folder),
        Err(ProjectNotesError::NoSetup)
    ));
}

#[test]
fn an_unreadable_keychain_entry_is_never_overwritten() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-keychain-newer");
    let (repo, notes, code) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let (other, keychain) = store(&dir.path().join("second"));
    let VaultFile::Known(vault) =
        parse_vault(&fs::read(repo.join(".hangar/notes/vault.json")).unwrap())
    else {
        panic!("vault");
    };
    let account = keychain_account(&vault.vault);
    keychain
        .set(&account, "v2 written-by-a-later-hangar")
        .unwrap();
    let outcome = other.unlock(&folder, &code, LABEL, UNTITLED).unwrap();
    assert!(
        matches!(
            outcome,
            Unlock::Unlocked {
                status: SharedStatus::Unlocked { .. }
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(
        keychain.get(&account).unwrap().unwrap().as_str(),
        "v2 written-by-a-later-hangar"
    );
}

#[test]
fn a_linked_notes_folder_is_refused() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-link");
    let (repo, notes, _) = enabled(&g, dir.path());
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(elsewhere.join("notes")).unwrap();
    let link = repo.join(".hangar");
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&elsewhere)
        .output()
        .is_ok_and(|o| o.status.success());
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&elsewhere, &link).is_ok();
    if !made {
        eprintln!("no link could be made, skipping");
        return;
    }
    assert!(matches!(
        notes.status(&shared(&repo), LABEL, UNTITLED),
        Err(ProjectNotesError::UnsafeFolder)
    ));
    assert!(matches!(
        notes.enable(&shared(&repo), LABEL, UNTITLED),
        Err(ProjectNotesError::UnsafeFolder)
    ));
    assert!(!elsewhere.join("notes").join("vault.json").exists());
}

#[test]
fn files_that_do_not_open_are_counted_once_per_change() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-unreadable");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let junk = repo
        .join(".hangar/notes")
        .join("00000000-0000-4000-8000-000000000001.note");
    fs::write(&junk, "hangar-note 1 0011223344556677\nAAAA\n").unwrap();
    for _ in 0..2 {
        let status = notes.status(&folder, LABEL, UNTITLED).unwrap();
        assert!(
            matches!(status, SharedStatus::Unlocked { unreadable: 1, .. }),
            "{status:?}"
        );
    }
    let cached = lock(&notes.state).meta.get(&junk).cloned();
    assert!(matches!(cached, Some(CachedMeta { meta: None, .. })));
}

#[test]
fn a_folder_with_a_git_entry_is_refused_even_when_git_finds_no_repository() {
    let dir = TempDir::new("shared-notes-dot-git");
    let (_, _) = store(dir.path());
    let broken = dir.path().join("broken");
    fs::create_dir_all(broken.join("sub")).unwrap();
    // rev-parse fails on this: the gitdir does not exist.
    fs::write(broken.join(".git"), "gitdir: ./does-not-exist\n").unwrap();
    let root = dir.path().join("elsewhere");
    fs::create_dir_all(&root).unwrap();
    assert!(!code_path_allowed(&root, &broken.join("code.txt")));
    assert!(!code_path_allowed(
        &root,
        &broken.join("sub").join("code.txt")
    ));
}

/// The vault and ring of an enabled repository.
fn vault_and_ring(notes: &ProjectNotes, repo: &Path) -> (Vault, KeyRing) {
    let VaultFile::Known(vault) =
        parse_vault(&fs::read(repo.join(".hangar/notes/vault.json")).unwrap())
    else {
        panic!("vault");
    };
    let ring = notes.ring(&vault.vault).unwrap();
    (vault, ring)
}

#[test]
fn a_note_becomes_readable_once_its_key_arrives() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-cache-key");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let (vault, ring) = vault_and_ring(&notes, &repo);
    // A note written with a key this computer does not have yet.
    let mut bigger = ring.clone();
    bigger.add_generated(&mut OsEntropy).unwrap();
    let id = "00000000-0000-4000-8000-0000000000c1";
    let plain = plain("Later", 5, "", "body");
    let file = crypto::seal_note(
        &bigger,
        &vault.vault,
        id,
        &encode_plain(&plain).unwrap(),
        &mut OsEntropy,
    )
    .unwrap();
    fs::write(repo.join(".hangar/notes").join(format!("{id}.note")), file).unwrap();
    assert!(matches!(
        notes.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked { unreadable: 1, .. }
    ));
    // The ring grows (as on unlock): the cached "does not open" must not hide the note.
    lock(&notes.state).rings.insert(vault.vault, bigger);
    assert!(matches!(
        notes.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked { unreadable: 0, .. }
    ));
    assert!(notes
        .list_notes(&folder)
        .unwrap()
        .iter()
        .any(|n| n.id == id));
}

#[test]
fn a_cached_note_does_not_survive_a_vault_change() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-cache-vault");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&folder, "Plan").unwrap();
    assert_eq!(notes.list_notes(&folder).unwrap().len(), 1);
    let (vault, ring) = vault_and_ring(&notes, &repo);
    // vault.json now names another vault, whose ring this computer holds (the same keys).
    let other = Vault {
        vault: VaultId::generate(&mut OsEntropy).unwrap(),
        ..vault
    };
    fs::write(repo.join(".hangar/notes/vault.json"), vault_json(&other)).unwrap();
    lock(&notes.state).rings.insert(other.vault, ring);
    // The note is bound to the old vault: it no longer opens, whatever the cache says.
    assert!(notes.list_notes(&folder).unwrap().is_empty());
    assert!(matches!(
        notes.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked { unreadable: 1, .. }
    ));
    assert!(notes.read_note(&folder, &id).is_err());
}

/// A hook that writes `marker` when it runs; git runs hooks through `sh` (Git for Windows brings
/// its own).
fn marker_hook(repo: &Path, name: &str, marker: &Path) {
    let marker = marker.to_string_lossy().replace('\\', "/");
    let hook = repo.join(".git/hooks").join(name);
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, format!("#!/bin/sh\necho hit > '{marker}'\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn resolving_conflicts_runs_no_repository_hook() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("shared-notes-no-hooks");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes.write_note(&folder, &id, "base").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes"]);
    git_ok(&g, &repo, &["checkout", "-qb", "other"]);
    notes.write_note(&folder, &id, "theirs").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "theirs"]);
    git_ok(&g, &repo, &["checkout", "-q", "main"]);
    std::thread::sleep(Duration::from_millis(5));
    notes.write_note(&folder, &id, "ours").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "ours"]);
    assert!(!git_try(&g, &repo, &["merge", "other"]));
    assert_eq!(unmerged_paths(&g, &repo).len(), 1);

    let marker = dir.path().join("hook-ran");
    marker_hook(&repo, "post-index-change", &marker);
    let status = notes.status(&folder, LABEL, UNTITLED).unwrap();
    let SharedStatus::Unlocked { resolved, .. } = status else {
        panic!("{status:?}");
    };
    assert_eq!(resolved.len(), 1, "{resolved:?}");
    assert!(unmerged_paths(&g, &repo).is_empty(), "nothing was staged");
    assert!(!marker.exists(), "a repository hook ran while staging");

    // The fixture is sound: git with the repository's hooks runs the marker hook.
    fs::write(repo.join("scratch.txt"), "x\n").unwrap();
    git_ok(&g.with_repo_hooks(), &repo, &["add", "scratch.txt"]);
    assert!(
        marker.exists(),
        "git did not run the hook; the test is void"
    );
}

/// The panel window names the folder: a share or device path is refused before git runs or any
/// lookup reaches its host.
#[test]
fn share_folders_are_refused_before_they_are_touched() {
    let tmp = TempDir::new("project-notes-share");
    let (notes, _) = store(tmp.path());
    let started = std::time::Instant::now();
    #[cfg(windows)]
    let shares = [
        r"\\hangar-test.invalid\share\project",
        "//hangar-test.invalid/share/project",
        r"\\?\UNC\hangar-test.invalid\share",
    ];
    #[cfg(not(windows))]
    let shares: [&str; 0] = [];
    for share in shares {
        let folder = NoteFolder {
            path: Some(PathBuf::from(share)),
            shared: true,
        };
        assert!(
            matches!(
                ProjectNotes::folder_path(&folder),
                Err(ProjectNotesError::NotRepository)
            ),
            "{share}"
        );
        assert!(matches!(
            notes.list_notes(&folder),
            Err(ProjectNotesError::NotRepository)
        ));
        assert!(matches!(
            notes.status(&folder, LABEL, UNTITLED),
            Err(ProjectNotesError::NotRepository)
        ));
    }
    let relative = NoteFolder {
        path: Some(PathBuf::from("relative")),
        shared: true,
    };
    assert!(ProjectNotes::folder_path(&relative).is_err());
    // A name lookup or SMB connection attempt would take seconds.
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// Note files are read only as regular files, checked on the open handle: a link is never
/// followed (it could lead to a share or a device) and a FIFO never blocks.
#[test]
fn note_files_are_read_only_as_regular_files() {
    let dir = TempDir::new("project-notes-regular");
    let note = dir.path().join("a.note");
    fs::write(&note, "content").unwrap();
    assert_eq!(read_file(&note).unwrap().as_deref(), Some(&b"content"[..]));
    assert!(stamp(&note).is_some());
    assert!(read_file(&dir.path().join("missing.note"))
        .unwrap()
        .is_none());
    assert!(stamp(dir.path()).is_none(), "a folder has no note stamp");
    assert!(matches!(
        read_file(dir.path()),
        Err(ProjectNotesError::Unreadable) | Err(ProjectNotesError::Io { .. })
    ));

    let link = dir.path().join("link.note");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&note, &link);
    // File symbolic links need Developer Mode or an elevated process on Windows.
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&note, &link);
    match made {
        Ok(()) => {
            assert!(
                matches!(read_file(&link), Err(ProjectNotesError::Unreadable)),
                "a link was followed"
            );
            assert!(stamp(&link).is_none(), "a link was followed");
        }
        Err(error) => eprintln!("symbolic link not possible here, link case skipped: {error}"),
    }
}

#[cfg(unix)]
#[test]
fn a_fifo_note_never_blocks() {
    use std::os::unix::ffi::OsStrExt;
    let dir = TempDir::new("project-notes-fifo");
    let fifo = dir.path().join("b.note");
    let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `name` is a valid NUL-terminated path that lives for the call.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let path = fifo.clone();
    std::thread::spawn(move || {
        let _ = tx.send(read_file(&path).is_err() && stamp(&path).is_none());
    });
    assert!(rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the read blocked"));
}

// ---------------------------------------------------------------------------------------------
// Team vaults (ADR 0006, phase 2)
// ---------------------------------------------------------------------------------------------

use crate::team::{LinkedRepo, TeamError};
use crate::team_engine::NotesKeysView;
use crate::team_service::NotesTeam;
use hangar_team_protocol::ids::Epoch;

/// The team side as the notes see it, with one team `p1`; `enable` stores a ring as the relay
/// would (the adopted one plus a new key, or a new one).
struct FakeTeam {
    team: Mutex<NotesTeam>,
    refuse: Mutex<Option<TeamError>>,
}

impl FakeTeam {
    fn new(view: Option<NotesKeysView>) -> Arc<Self> {
        Arc::new(Self {
            team: Mutex::new(NotesTeam {
                project: "p1".to_owned(),
                key: "k1".to_owned(),
                linked: Vec::new(),
                view,
            }),
            refuse: Mutex::new(None),
        })
    }

    fn view(&self, change: impl FnOnce(&mut NotesKeysView)) {
        let mut team = lock(&self.team);
        let view = team.view.get_or_insert_with(NotesKeysView::default);
        change(view);
    }

    fn unview(&self) {
        lock(&self.team).view = None;
    }

    fn link_repo(&self, repo: &Path) {
        let g = git().unwrap();
        let root = crate::claims_service::root_commit(&g, repo)
            .unwrap()
            .unwrap();
        lock(&self.team).linked.push(LinkedRepo {
            root,
            folder: Some(repo.to_string_lossy().into_owned()),
            name: "repo".to_owned(),
        });
    }
}

impl TeamSide for FakeTeam {
    fn teams(&self) -> Vec<NotesTeam> {
        vec![lock(&self.team).clone()]
    }
    fn enable(&self, project: &str, adopt: Option<KeyRing>) -> Result<(), TeamError> {
        assert_eq!(project, "p1");
        if let Some(error) = lock(&self.refuse).take() {
            return Err(error);
        }
        let mut ring = match adopt {
            Some(mut solo) => {
                solo.add_generated(&mut OsEntropy).unwrap();
                solo
            }
            None => KeyRing::generate(&mut OsEntropy).unwrap(),
        };
        self.view(|v| {
            if let Some(team) = &v.ring {
                ring.merge(team).unwrap();
            }
            v.ring = Some(ring);
            v.fresh = v.log_epoch;
            v.exists = Some(true);
        });
        Ok(())
    }
    fn link(&self, project: &str, root: &str, folder: &str) -> Result<(), TeamError> {
        assert_eq!(project, "p1");
        lock(&self.team).linked.push(LinkedRepo {
            root: root.to_owned(),
            folder: Some(folder.to_owned()),
            name: "repo".to_owned(),
        });
        Ok(())
    }
    fn person(&self) -> String {
        "Alice".to_owned()
    }
}

fn manager_view(epoch: u32) -> NotesKeysView {
    NotesKeysView {
        speaks: Some(true),
        exists: Some(false),
        manager: true,
        log_epoch: Some(Epoch(epoch)),
        ..NotesKeysView::default()
    }
}

fn committed_repo(g: &Git, dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    init_repo(g, &repo);
    fs::write(repo.join("readme.txt"), "x\n").unwrap();
    git_ok(g, &repo, &["add", "readme.txt"]);
    git_ok(g, &repo, &["commit", "-qm", "root"]);
    repo
}

type TeamVault = (PathBuf, ProjectNotes, Arc<MemoryStore>, Arc<FakeTeam>);

/// A repository whose team vault is turned on through the fake team.
fn team_vault(g: &Git, dir: &Path) -> TeamVault {
    let repo = committed_repo(g, dir);
    let (notes, keychain) = store(dir);
    let team = FakeTeam::new(Some(manager_view(1)));
    notes.set_team(team.clone());
    team.link_repo(&repo);
    let status = notes.enable_team(&shared(&repo), LABEL, UNTITLED).unwrap();
    assert!(
        matches!(
            status,
            SharedStatus::Unlocked {
                renewing: false,
                ..
            }
        ),
        "{status:?}"
    );
    (repo, notes, keychain, team)
}

#[test]
fn a_team_turns_notes_on_without_a_code_and_a_linked_clone_reads_them() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-on");
    let repo = committed_repo(&g, dir.path());
    let (notes, _) = store(dir.path());
    let team = FakeTeam::new(Some(manager_view(1)));
    notes.set_team(team.clone());
    let folder = shared(&repo);
    // Not linked: no team offered, turning on is refused.
    assert_eq!(notes.team_notes(&folder).unwrap(), None);
    assert!(matches!(
        notes.enable_team(&folder, LABEL, UNTITLED),
        Err(ProjectNotesError::NotLinked)
    ));
    team.link_repo(&repo);
    let offered = notes.team_notes(&folder).unwrap().unwrap();
    assert!(offered.linked && offered.manager);
    assert_eq!(offered.speaks, Some(true));
    notes.enable_team(&folder, LABEL, UNTITLED).unwrap();
    let text = fs::read_to_string(repo.join(".hangar/notes/vault.json")).unwrap();
    let VaultFile::Known(Vault {
        mode: VaultMode::Team { team: hint },
        ..
    }) = parse_vault(text.as_bytes())
    else {
        panic!("{text}");
    };
    assert_eq!(
        hint,
        crypto::team_hint(&hangar_team_protocol::ids::TeamId::new("p1").unwrap())
    );
    assert!(!text.contains("codeSlot"));
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes.write_note(&folder, &id, "# Plan\nteam text").unwrap();
    let file = fs::read_to_string(repo.join(format!(".hangar/notes/{id}.note"))).unwrap();
    assert!(!file.contains("team text"));

    // A second computer of the team: not linked yet, then linked from the notes.
    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    let (second, _) = store(&other);
    let theirs = FakeTeam::new(lock(&team.team).view.clone());
    second.set_team(theirs);
    assert_eq!(
        second.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Locked {
            reason: LockReason::Link
        }
    );
    second.link_team(&folder, LABEL, UNTITLED).unwrap();
    assert_eq!(
        second.read_note(&folder, &id).unwrap().as_str(),
        "# Plan\nteam text"
    );

    // Someone outside the team, and a linked clone that has no key yet.
    let strange_dir = dir.path().join("stranger");
    fs::create_dir_all(&strange_dir).unwrap();
    let (stranger, _) = store(&strange_dir);
    let nobody = FakeTeam::new(None);
    lock(&nobody.team).project = "p2".to_owned();
    stranger.set_team(nobody);
    assert_eq!(
        stranger.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Locked {
            reason: LockReason::Team
        }
    );
    let wait_dir = dir.path().join("waiting");
    fs::create_dir_all(&wait_dir).unwrap();
    let (waiting, _) = store(&wait_dir);
    let empty = FakeTeam::new(Some(manager_view(1)));
    empty.link_repo(&repo);
    waiting.set_team(empty.clone());
    assert_eq!(
        waiting.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Locked {
            reason: LockReason::Waiting
        }
    );
    empty.view(|v| v.speaks = Some(false));
    assert_eq!(
        waiting.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Locked {
            reason: LockReason::OldServer
        }
    );
}

/// Without this rule the newest key, which whoever left still has, would keep sealing notes.
#[test]
fn saving_waits_while_the_log_is_ahead_of_the_rings_newest_key_also_after_a_restart() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-renew");
    let (repo, notes, keychain, team) = team_vault(&g, dir.path());
    let folder = shared(&repo);
    let id = notes.create_note(&folder, "Plan").unwrap();

    // A member left and the log rotated: the ring's newest key is from epoch 1.
    team.view(|v| v.log_epoch = Some(Epoch(2)));
    assert!(matches!(
        notes.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked { renewing: true, .. }
    ));
    assert!(matches!(
        notes.write_note(&folder, &id, "after the removal"),
        Err(ProjectNotesError::Renewing)
    ));
    assert!(matches!(
        notes.create_note(&folder, "New"),
        Err(ProjectNotesError::Renewing)
    ));
    // Reading goes on.
    assert_eq!(notes.read_note(&folder, &id).unwrap().as_str(), "");

    // A removal whose rotation has not landed: refused as well.
    team.view(|v| {
        v.log_epoch = Some(Epoch(1));
        v.urgent = true;
    });
    assert!(matches!(
        notes.write_note(&folder, &id, "x"),
        Err(ProjectNotesError::Renewing)
    ));

    // The log moved on again; then a restart before the first connection: no log, no ring
    // from the team, the stored decision holds.
    team.view(|v| {
        v.urgent = false;
        v.log_epoch = Some(Epoch(2));
    });
    assert!(notes.write_note(&folder, &id, "y").is_err());
    let restarted = ProjectNotes::new(&dir.path().join("app"), keychain.clone());
    let ring = lock(&team.team).view.clone().unwrap().ring.unwrap();
    team.unview();
    restarted.set_team(team.clone());
    assert!(matches!(
        restarted.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked { renewing: true, .. }
    ));
    assert!(matches!(
        restarted.write_note(&folder, &id, "z"),
        Err(ProjectNotesError::Renewing)
    ));

    // A relay of version 1: the log is verified, the ring cannot follow it; still refused.
    team.view(|v| {
        v.speaks = Some(false);
        v.log_epoch = Some(Epoch(2));
    });
    assert!(matches!(
        restarted.write_note(&folder, &id, "z"),
        Err(ProjectNotesError::Renewing)
    ));

    // An owner or admin renewed the ring under epoch 2: saving again.
    team.view(|v| {
        v.speaks = Some(true);
        let mut renewed = ring.clone();
        renewed.add_generated(&mut OsEntropy).unwrap();
        v.ring = Some(renewed);
        v.fresh = Some(Epoch(2));
    });
    restarted.write_note(&folder, &id, "renewed").unwrap();
    assert_eq!(
        restarted.read_note(&folder, &id).unwrap().as_str(),
        "renewed"
    );
    let cached = lock(&keychain.0)["team/k1"].to_string();
    assert!(cached.starts_with("t1 2 0 v1 "), "{cached}");
}

#[test]
fn a_solo_vault_becomes_the_teams_only_after_the_relay_stored_its_keys() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-solo");
    let (repo, notes, _) = enabled(&g, dir.path());
    notes.enable(&shared(&repo), LABEL, UNTITLED).unwrap();
    let folder = shared(&repo);
    let id = notes.create_note(&folder, "Old").unwrap();
    notes.write_note(&folder, &id, "solo text").unwrap();
    let team = FakeTeam::new(Some(manager_view(1)));
    notes.set_team(team.clone());
    team.link_repo(&repo);
    let vault_path = repo.join(".hangar/notes/vault.json");
    let before = fs::read_to_string(&vault_path).unwrap();
    *lock(&team.refuse) = Some(TeamError::Busy);
    assert!(notes.to_team(&folder, LABEL, UNTITLED).is_err());
    assert_eq!(fs::read_to_string(&vault_path).unwrap(), before);

    notes.to_team(&folder, LABEL, UNTITLED).unwrap();
    let after = fs::read_to_string(&vault_path).unwrap();
    assert!(after.contains("\"mode\": \"team\"") && !after.contains("codeSlot"));
    // The note written before still opens, now with the team's ring.
    assert_eq!(notes.read_note(&folder, &id).unwrap().as_str(), "solo text");
    let ring = lock(&team.team).view.clone().unwrap().ring.unwrap();
    assert_eq!(ring.len(), 2);
    // The solo key was the owner's alone: its notes move to the team's newest key, unmarked.
    assert_eq!(header_key(&repo, &id), ring.newest().id());
    assert!(!old_key_of(&notes, &folder, &id));
}

#[test]
fn the_keychain_keeps_only_the_newest_team_keys() {
    let mut ring = KeyRing::generate(&mut OsEntropy).unwrap();
    for _ in 1..crypto::MAX_KEYS {
        ring.add_generated(&mut OsEntropy).unwrap();
    }
    let text = team::cache_text(&team::TeamCache {
        fresh: Some(Epoch(254)),
        stale: true,
        ring: ring.clone(),
    });
    assert!(text.encode_utf16().count() <= crate::secrets::MAX_VALUE_UNITS);
    let cache = team::parse_cache(&text).unwrap();
    assert_eq!(cache.ring.len(), CACHED_TEAM_KEYS);
    assert_eq!(cache.ring.newest().id(), ring.newest().id());
    assert_eq!(cache.fresh, Some(Epoch(254)));
    assert!(cache.stale);
    assert!(team::parse_cache("t2 1 0 v1").is_none());
    assert!(team::parse_cache("v1 0123456789abcdef:00").is_none());
}

fn header_key(repo: &Path, id: &str) -> crypto::KeyId {
    let bytes = fs::read(repo.join(".hangar/notes").join(format!("{id}.note"))).unwrap();
    crypto::note_header(&bytes).unwrap().key
}

fn team_ring_of(team: &FakeTeam) -> KeyRing {
    lock(&team.team).view.clone().unwrap().ring.unwrap()
}

/// What a holder of `ring` can write without Hangar: any key of the ring, any `by`, any time.
fn plant(repo: &Path, ring: &KeyRing, id: &str, plain: &Plain) {
    let VaultFile::Known(vault) =
        parse_vault(&fs::read(repo.join(".hangar/notes/vault.json")).unwrap())
    else {
        panic!("vault");
    };
    let file = crypto::seal_note(
        ring,
        &vault.vault,
        id,
        &encode_plain(plain).unwrap(),
        &mut OsEntropy,
    )
    .unwrap();
    fs::write(repo.join(".hangar/notes").join(format!("{id}.note")), file).unwrap();
}

fn old_key_of(notes: &ProjectNotes, folder: &NoteFolder, id: &str) -> bool {
    notes
        .list_notes(folder)
        .unwrap()
        .into_iter()
        .find(|n| n.id == id)
        .unwrap()
        .old_key
}

/// The keychain caches only the newest keys; merging the team's whole ring into that cache must
/// not make an old key the one new notes are sealed with.
#[test]
fn writes_use_the_newest_key_when_the_ring_is_longer_than_the_keychain_cache() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-cache-order");
    let (repo, notes, _, team) = team_vault(&g, dir.path());
    let folder = shared(&repo);
    let mut ring = team_ring_of(&team);
    for _ in 0..CACHED_TEAM_KEYS {
        ring.add_generated(&mut OsEntropy).unwrap();
    }
    team.view(|v| v.ring = Some(ring.clone()));
    // The first call caches the newest keys, the next ones merge the team's ring into them.
    notes.status(&shared(&repo), LABEL, UNTITLED).unwrap();
    let id = notes.create_note(&folder, "Plan").unwrap();
    assert_eq!(header_key(&repo, &id), ring.newest().id());
    notes.write_note(&folder, &id, "text").unwrap();
    assert_eq!(header_key(&repo, &id), ring.newest().id());
}

/// A removed member keeps the old keys and may still be able to push to the repository: notes
/// this computer knew move to the new key, anything else under an old key stays marked.
#[test]
fn after_a_removal_known_notes_move_to_the_new_key_and_other_old_key_notes_are_marked() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-reseal");
    let (repo, notes, keychain, team) = team_vault(&g, dir.path());
    let folder = shared(&repo);
    let kept = notes.create_note(&folder, "Kept").unwrap();
    notes.write_note(&folder, &kept, "known text").unwrap();
    let seen = notes.create_note(&folder, "Seen").unwrap();
    notes.write_note(&folder, &seen, "seen text").unwrap();
    let before = notes.list_notes(&folder).unwrap();
    assert!(before.iter().all(|n| !n.old_key));
    let kept_updated = before.iter().find(|n| n.id == kept).unwrap().updated_at;
    let old = team_ring_of(&team);

    // A member was removed: the log rotated to epoch 2 and an owner added a key.
    let mut renewed = old.clone();
    renewed.add_generated(&mut OsEntropy).unwrap();
    team.view(|v| {
        v.log_epoch = Some(Epoch(2));
        v.fresh = Some(Epoch(2));
        v.ring = Some(renewed.clone());
    });
    // Before any member's Hangar re-encrypts, the removed member writes with the old key.
    let planted = "00000000-0000-4000-8000-0000000000d1";
    let late = u64::MAX / 2;
    plant(
        &repo,
        &old,
        planted,
        &plain("Planted", late, "Alice", "marker one"),
    );
    plant(
        &repo,
        &old,
        &seen,
        &plain("Seen", late, "Alice", "marker two"),
    );

    // After a restart: what this computer had seen is remembered outside the repository.
    let restarted = ProjectNotes::new(&dir.path().join("app"), keychain.clone());
    restarted.set_team(team.clone());
    assert!(matches!(
        restarted.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked {
            renewing: false,
            unreadable: 0,
            ..
        }
    ));
    assert_eq!(header_key(&repo, &kept), renewed.newest().id());
    assert_eq!(
        restarted.read_note(&folder, &kept).unwrap().as_str(),
        "known text"
    );
    let after = restarted.list_notes(&folder).unwrap();
    let kept_info = after.iter().find(|n| n.id == kept).unwrap();
    assert!(!kept_info.old_key);
    assert_eq!(kept_info.updated_at, kept_updated);
    // Never re-encrypted, so their old key keeps showing.
    assert_eq!(header_key(&repo, planted), old.newest().id());
    assert_eq!(header_key(&repo, &seen), old.newest().id());
    assert!(old_key_of(&restarted, &folder, planted));
    assert!(old_key_of(&restarted, &folder, &seen));
    // A second status changes nothing more.
    let planted_path = repo.join(".hangar/notes").join(format!("{planted}.note"));
    let planted_file = fs::read(&planted_path).unwrap();
    restarted.status(&folder, LABEL, UNTITLED).unwrap();
    assert_eq!(fs::read(&planted_path).unwrap(), planted_file);

    // The person's own edit is written with the new key; the mark goes.
    restarted.write_note(&folder, planted, "checked").unwrap();
    assert_eq!(header_key(&repo, planted), renewed.newest().id());
    assert!(!old_key_of(&restarted, &folder, planted));
}

/// While the newest key may still be known to whoever left, nothing is re-encrypted with it, and
/// a run without a verified log trusts nothing it sees.
#[test]
fn nothing_is_resealed_while_renewing_and_unverified_runs_trust_nothing() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-reseal-wait");
    let (repo, notes, _, team) = team_vault(&g, dir.path());
    let folder = shared(&repo);
    let old = team_ring_of(&team);
    // Written by someone else, seen first in a run that has not verified the log.
    let foreign = "00000000-0000-4000-8000-0000000000d2";
    plant(
        &repo,
        &old,
        foreign,
        &plain("Foreign", 5, "Bob", "marker three"),
    );
    team.view(|v| v.log_epoch = None);
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert!(!old_key_of(&notes, &folder, foreign));

    // A removal whose new key is not there yet: saving waits, nothing moves.
    team.view(|v| v.log_epoch = Some(Epoch(2)));
    assert!(matches!(
        notes.status(&folder, LABEL, UNTITLED).unwrap(),
        SharedStatus::Unlocked { renewing: true, .. }
    ));
    assert_eq!(header_key(&repo, foreign), old.newest().id());

    // The key arrives: the note was never seen in a verified run, so it stays as it is.
    let mut renewed = old.clone();
    renewed.add_generated(&mut OsEntropy).unwrap();
    team.view(|v| {
        v.fresh = Some(Epoch(2));
        v.ring = Some(renewed.clone());
    });
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert_eq!(header_key(&repo, foreign), old.newest().id());
    assert!(old_key_of(&notes, &folder, foreign));
}

#[test]
fn a_conflict_side_under_an_old_key_is_never_re_encrypted() {
    let ours = plain("Plan", 10, "Ada", "ours");
    let theirs = plain("Plan", 20, "Bob", "theirs");
    // Theirs has the old key and the later time: kept as it is, ours becomes the copy.
    assert_eq!(
        resolve_marked(Some(&ours), Some(&theirs), LABEL, (false, true)),
        Some(Resolution::KeepBoth {
            keep_ours: false,
            copy_title: "Plan (other version, Ada)".to_owned()
        })
    );
    // Ours would be the copy, but it has the old key: it stays under the id instead.
    assert_eq!(
        resolve_marked(Some(&ours), Some(&theirs), LABEL, (true, false)),
        Some(Resolution::KeepBoth {
            keep_ours: true,
            copy_title: "Plan (other version, Bob)".to_owned()
        })
    );
    // Both old and different: no copy without a new encryption; left to Git.
    assert_eq!(
        resolve_marked(Some(&ours), Some(&theirs), LABEL, (true, true)),
        None
    );
    // Equal content keeps the side with the newest key.
    let same = plain("Plan", 30, "Bob", "theirs");
    assert_eq!(
        resolve_marked(Some(&same), Some(&theirs), LABEL, (true, false)),
        Some(Resolution::KeepTheirs)
    );
    assert_eq!(
        resolve_marked(Some(&same), Some(&theirs), LABEL, (false, true)),
        Some(Resolution::KeepOurs)
    );
}

/// A conflict copy holds the other side's text: merged in while the log is not verified, it may
/// be a removed member's version under the key they still hold, so it is not counted as known
/// and stays marked after the next removal instead of being re-encrypted.
#[test]
fn a_conflict_copy_from_an_unverified_run_is_not_re_encrypted_later() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-copy-unverified");
    let (repo, notes, _, team) = team_vault(&g, dir.path());
    let folder = shared(&repo);
    let id = notes.create_note(&folder, "Plan").unwrap();
    notes.write_note(&folder, &id, "base").unwrap();
    git_ok(&g, &repo, &["add", "-A"]);
    git_ok(&g, &repo, &["commit", "-qm", "notes"]);
    let old = team_ring_of(&team);
    git_ok(&g, &repo, &["checkout", "-qb", "other"]);
    plant(&repo, &old, &id, &plain("Plan", 5, "Bob", "marker four"));
    git_ok(&g, &repo, &["commit", "-qam", "theirs"]);
    git_ok(&g, &repo, &["checkout", "-q", "main"]);
    notes.write_note(&folder, &id, "ours, newer").unwrap();
    git_ok(&g, &repo, &["commit", "-qam", "ours"]);
    assert!(!git_try(&g, &repo, &["merge", "other"]));

    // This run has not verified the log: the copy is written, but not learned.
    team.view(|v| v.log_epoch = None);
    let status = notes.status(&folder, LABEL, UNTITLED).unwrap();
    let SharedStatus::Unlocked { resolved, .. } = status else {
        panic!("{status:?}");
    };
    assert_eq!(resolved.len(), 1);
    let copy = notes
        .list_notes(&folder)
        .unwrap()
        .into_iter()
        .find(|n| n.id != id)
        .unwrap()
        .id;

    let mut renewed = old.clone();
    renewed.add_generated(&mut OsEntropy).unwrap();
    team.view(|v| {
        v.log_epoch = Some(Epoch(2));
        v.fresh = Some(Epoch(2));
        v.ring = Some(renewed.clone());
    });
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    // Our own version was known and moves; the copy keeps its old key and its mark.
    assert_eq!(header_key(&repo, &id), renewed.newest().id());
    assert_eq!(header_key(&repo, &copy), old.newest().id());
    assert!(old_key_of(&notes, &folder, &copy));
}

/// Restoring under a new id is a new encryption: for a planted note under an old key that would
/// hide its key, so it is refused and the note that came back stays as it is.
#[test]
fn a_planted_old_key_note_is_not_restored_under_a_new_id() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-restore-old");
    let (repo, notes, _, team) = team_vault(&g, dir.path());
    let folder = shared(&repo);
    let old = team_ring_of(&team);
    let mut renewed = old.clone();
    renewed.add_generated(&mut OsEntropy).unwrap();
    team.view(|v| {
        v.log_epoch = Some(Epoch(2));
        v.fresh = Some(Epoch(2));
        v.ring = Some(renewed.clone());
    });
    let planted = "00000000-0000-4000-8000-0000000000d3";
    plant(
        &repo,
        &old,
        planted,
        &plain("Planted", 5, "Alice", "marker five"),
    );
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert!(old_key_of(&notes, &folder, planted));
    notes.delete_note(&folder, planted).unwrap();
    // A pull brings the same id back.
    plant(
        &repo,
        &old,
        planted,
        &plain("Planted", 6, "Alice", "marker six"),
    );
    let path = repo.join(".hangar/notes").join(format!("{planted}.note"));
    let back = fs::read(&path).unwrap();
    assert!(matches!(
        notes.restore_note(&folder, planted),
        Err(ProjectNotesError::RestoreOldKey)
    ));
    assert_eq!(fs::read(&path).unwrap(), back);
    assert_eq!(notes.list_notes(&folder).unwrap().len(), 1);
    // Still in the trash: once the other note is gone, it comes back under its own id, marked.
    fs::remove_file(&path).unwrap();
    assert_eq!(notes.restore_note(&folder, planted).unwrap(), planted);
    assert!(old_key_of(&notes, &folder, planted));
}

#[test]
fn moving_between_vaults_learns_only_what_could_be_known() {
    use MoveRule::{Known, Skip, Unknown};
    // Solo source: its owner's notes, known.
    assert_eq!(move_rule(false, false, false, false), Known);
    // Team source under its newest key: known only in a verified run without a pending renewal.
    assert_eq!(move_rule(true, true, true, false), Known);
    assert_eq!(move_rule(true, true, false, false), Unknown);
    // Team source under an old key: moved only if known before, else left behind.
    assert_eq!(move_rule(true, false, true, true), Known);
    assert_eq!(move_rule(true, false, true, false), Skip);
    assert_eq!(move_rule(true, false, false, false), Skip);
    // Known before stays known whatever the run knows.
    assert_eq!(move_rule(true, true, false, true), Known);
}

/// A team vault that was solo still has its solo ring in the keychain, and that ring's newest
/// key is an old one in the team's ring. A `vault.json` conflict that moves such a vault's notes
/// into the winner must judge them by the team's ring, so a note planted under the solo key stays
/// where it is instead of coming out as a new encryption in the other vault.
#[test]
fn a_vault_conflict_judges_a_former_solo_vault_by_the_teams_newest_key() {
    let Some(g) = git() else { return };
    let dir = TempDir::new("team-notes-vault-move");
    let (repo, notes, _) = enabled(&g, dir.path());
    let folder = shared(&repo);
    notes.enable(&folder, LABEL, UNTITLED).unwrap();
    let team = FakeTeam::new(Some(manager_view(1)));
    notes.set_team(team.clone());
    team.link_repo(&repo);
    notes.to_team(&folder, LABEL, UNTITLED).unwrap();
    let VaultFile::Known(ours) =
        parse_vault(&fs::read(repo.join(".hangar/notes/vault.json")).unwrap())
    else {
        panic!("vault");
    };
    let solo = notes.ring(&ours.vault).unwrap();
    assert_ne!(solo.newest().id(), team_ring_of(&team).newest().id());
    let planted = "00000000-0000-4000-8000-0000000000d3";
    plant(
        &repo,
        &solo,
        planted,
        &plain("Planted", 5, "Bob", "marker five"),
    );
    let planted_path = repo.join(".hangar/notes").join(format!("{planted}.note"));
    let planted_file = fs::read(&planted_path).unwrap();

    // A solo vault with the smallest id wins the conflict; this computer holds its ring.
    let foreign_id = VaultId::from_hex("00000000000000000000000000000001").unwrap();
    let foreign_ring = KeyRing::generate(&mut OsEntropy).unwrap();
    notes
        .keychain
        .set(&keychain_account(&foreign_id), &foreign_ring.to_keychain())
        .unwrap();
    let foreign = "{\n  \"format\": 1,\n  \"vault\": \"00000000000000000000000000000001\",\n  \"mode\": \"solo\",\n  \"codeSlot\": \"AQID\"\n}\n";
    vault_conflict(&g, &repo, foreign);
    notes.status(&folder, LABEL, UNTITLED).unwrap();
    assert!(fs::read_to_string(repo.join(".hangar/notes/vault.json"))
        .unwrap()
        .contains("00000000000000000000000000000001"));
    assert_eq!(fs::read(&planted_path).unwrap(), planted_file);
}
