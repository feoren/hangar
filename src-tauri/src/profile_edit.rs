//! Editing `profiles.toml` from Settings (contract: docs/wiki/pages/syntheses/agent-workflow-features.md,
//! "Profiles in Settings").
//!
//! The file is the user's: comments, order, layout and fields the form does not know (`icon`,
//! `latest`, `version_args`, ...) stay as they are. Only the `[[profile]]` with the given id is
//! changed, inserted or removed, through `toml_edit`. The result is checked with the same rules as
//! loading ([`crate::profile::parse`]) before anything is written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::profile::{self, Profile, ProfileError};

/// What the profile form sends.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileInput {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub secret_env: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("Invalid profile id '{0}': it must not be empty or contain spaces or '/'")]
    InvalidId(String),
    #[error("Profile id '{0}' differs from the existing '{1}' only in case; use the same spelling or another id")]
    IdCase(String, String),
    #[error("The working folder must be a full path, for example C:\\Users\\you\\project or /home/you/project")]
    RelativeCwd,
    #[error("profiles.toml cannot be read as TOML, fix it first: {0}")]
    Syntax(String),
    #[error("profiles.toml uses a layout Settings cannot edit: 'profile' must be a list of [[profile]] tables")]
    Layout,
    #[error("'{0}' is not one of your own profiles; built-in programs cannot be deleted")]
    NotUserProfile(String),
    #[error("The last profile in profiles.toml cannot be deleted")]
    LastProfile,
    #[error(transparent)]
    Invalid(#[from] ProfileError),
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl Serialize for EditError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Result of an edit: the new file text and the profiles as loading would read them.
#[derive(Debug)]
pub struct Edited {
    pub source: String,
    pub profiles: Vec<Profile>,
    /// `secret_env` names that are gone from the profile (changed or deleted); their keychain
    /// entries are no longer used.
    pub dropped_secrets: Vec<String>,
}

fn document(source: &str) -> Result<DocumentMut, EditError> {
    source
        .parse::<DocumentMut>()
        .map_err(|error| EditError::Syntax(error.message().trim().to_owned()))
}

fn profiles_mut(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables, EditError> {
    let item = doc
        .entry("profile")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
    item.as_array_of_tables_mut().ok_or(EditError::Layout)
}

fn table_id(table: &Table) -> Option<&str> {
    table.get("id").and_then(Item::as_str)
}

fn string_list(table: &Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Reads `profiles.toml` in `dir` (created from the template if it is missing), applies `change`
/// ([`upsert`] or [`delete`]) and writes the result atomically. Nothing is written when `change`
/// fails. The caller serializes concurrent edits.
pub fn edit_file(
    dir: &Path,
    change: impl FnOnce(&str) -> Result<Edited, EditError>,
) -> Result<Edited, EditError> {
    let path = profile::ensure_file(dir)?;
    let io = |source| ProfileError::Io {
        path: path.clone(),
        source,
    };
    let source = std::fs::read_to_string(&path).map_err(io)?;
    let edited = change(&source)?;
    crate::persist::write_atomic(&path, edited.source.as_bytes()).map_err(io)?;
    Ok(edited)
}

/// Inserts or replaces the profile `input.id`. An id from the catalog becomes a user profile that
/// replaces the catalog entry (see [`crate::agent_catalog::merge`]). `known_ids` are the ids of
/// all other entries (catalog, WSL): one that differs from `input.id` only in case is refused,
/// because the keychain would mix up their secrets. The file's own ids are checked by loading.
pub fn upsert(
    source: &str,
    input: &ProfileInput,
    known_ids: &[String],
) -> Result<Edited, EditError> {
    if !crate::secrets::is_valid_profile_id(&input.id) {
        return Err(EditError::InvalidId(input.id.clone()));
    }
    let folded = profile::fold_id(&input.id);
    if let Some(other) = known_ids
        .iter()
        .find(|known| **known != input.id && profile::fold_id(known) == folded)
    {
        return Err(EditError::IdCase(input.id.clone(), other.clone()));
    }
    // Relative would be resolved against Hangar's own working folder.
    if input
        .cwd
        .as_ref()
        .is_some_and(|cwd| !cwd.as_os_str().is_empty() && !cwd.is_absolute())
    {
        return Err(EditError::RelativeCwd);
    }
    let mut doc = document(source)?;
    let profiles = profiles_mut(&mut doc)?;
    let existing = profiles
        .iter_mut()
        .find(|table| table_id(table) == Some(input.id.as_str()));
    let previous_secrets = match existing {
        Some(table) => {
            let previous = string_list(table, "secret_env");
            fill(table, input);
            previous
        }
        None => {
            let mut table = Table::new();
            set_value(&mut table, "id", Value::from(input.id.as_str()));
            fill(&mut table, input);
            profiles.push(table);
            Vec::new()
        }
    };
    let source = doc.to_string();
    let profiles = profile::parse(&source)?;
    let dropped_secrets = previous_secrets
        .into_iter()
        .filter(|name| !input.secret_env.contains(name))
        .collect();
    Ok(Edited {
        source,
        profiles,
        dropped_secrets,
    })
}

/// Removes the user profile `id`. A catalog entry with the same id shows up again afterwards.
pub fn delete(source: &str, id: &str) -> Result<Edited, EditError> {
    let mut doc = document(source)?;
    let profiles = profiles_mut(&mut doc)?;
    let index = profiles
        .iter()
        .position(|table| table_id(table) == Some(id))
        .ok_or_else(|| EditError::NotUserProfile(id.to_owned()))?;
    if profiles.len() == 1 {
        return Err(EditError::LastProfile);
    }
    let removed = profiles.remove(index);
    let source = doc.to_string();
    let profiles = profile::parse(&source)?;
    Ok(Edited {
        source,
        profiles,
        dropped_secrets: string_list(&removed, "secret_env"),
    })
}

/// Writes the form fields into `table`. Optional fields that are empty are removed rather than
/// written empty, so the file stays as short as a hand-written one.
fn fill(table: &mut Table, input: &ProfileInput) {
    set_value(table, "name", Value::from(input.name.as_str()));
    set_value(table, "command", Value::from(input.command.as_str()));
    set_list(table, "args", &input.args);
    match &input.cwd {
        Some(cwd) if !cwd.as_os_str().is_empty() => {
            set_value(table, "cwd", Value::from(cwd.to_string_lossy().as_ref()));
        }
        _ => {
            table.remove("cwd");
        }
    }
    set_list(table, "secret_env", &input.secret_env);
    set_env(table, &input.env);
}

fn string_array(items: &[String]) -> Array {
    items.iter().map(String::as_str).collect()
}

fn set_list(table: &mut Table, key: &str, items: &[String]) {
    if items.is_empty() {
        table.remove(key);
    } else {
        set_value(table, key, Value::Array(string_array(items)));
    }
}

/// Replaces a value but keeps the comments around it; an unchanged value is left exactly as
/// written (quotes, spacing, line breaks inside a list).
fn set_value(table: &mut Table, key: &str, new: Value) {
    match table.get_mut(key).and_then(Item::as_value_mut) {
        Some(existing) if same_value(existing, &new) => {}
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = new;
            *existing.decor_mut() = decor;
        }
        None => {
            table.insert(key, Item::Value(new));
        }
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(a), Value::String(b)) => a.value() == b.value(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| same_value(a, b))
        }
        _ => false,
    }
}

/// `env` as written before: an inline table stays inline, otherwise a `[profile.env]` table.
fn set_env(table: &mut Table, env: &BTreeMap<String, String>) {
    if env.is_empty() {
        table.remove("env");
        return;
    }
    let current: Option<BTreeMap<String, String>> = table.get("env").and_then(|item| {
        item.as_table_like().map(|t| {
            t.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.to_owned(), v.to_owned())))
                .collect()
        })
    });
    if current.as_ref() == Some(env) {
        return;
    }
    if table.get("env").is_some_and(Item::is_inline_table) {
        let inline: InlineTable = env
            .iter()
            .map(|(k, v)| (k.as_str(), Value::from(v.as_str())))
            .collect();
        set_value(table, "env", Value::InlineTable(inline));
        return;
    }
    let mut sub = Table::new();
    for (key, value) in env {
        sub.insert(key, toml_edit::value(value.as_str()));
    }
    match table.get_mut("env").and_then(Item::as_table_mut) {
        // Keeps the header's comments and position.
        Some(existing) => {
            existing.clear();
            for (key, item) in sub.iter() {
                existing.insert(key, item.clone());
            }
        }
        None => {
            table.insert("env", Item::Table(sub));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
# My profiles, keep this comment.

[[profile]]
id = \"pwsh\" # the shell
name = \"PowerShell 7\"
command = \"pwsh\"
args = [\"-NoLogo\"]
icon = \"PS\"
latest = { github = \"PowerShell/PowerShell\", tag_prefix = \"v\" }

# Agent with keys
[[profile]]
id = \"own\"
name = \"Own agent\"
command = \"own-agent\"
secret_env = [\"OWN_KEY\", \"OLD_KEY\"]
[profile.env]
# region comment
REGION = \"eu\"

[[profile]]
id = \"cmd\"
name = \"Command Prompt\"
command = \"cmd\"
";

    #[cfg(windows)]
    const ABSOLUTE_CWD: &str = "C:\\Users\\Test User\\work";
    #[cfg(not(windows))]
    const ABSOLUTE_CWD: &str = "/home/test user/work";

    fn input(id: &str) -> ProfileInput {
        ProfileInput {
            id: id.into(),
            name: "New".into(),
            command: "new-agent".into(),
            args: vec!["--fast".into(), "two words".into()],
            cwd: None,
            env: BTreeMap::new(),
            secret_env: Vec::new(),
        }
    }

    #[test]
    fn changing_a_profile_keeps_everything_else() {
        let mut change = input("pwsh");
        change.name = "PowerShell Seven".into();
        change.command = "pwsh".into();
        change.args = vec!["-NoLogo".into()];
        let edited = upsert(FILE, &change, &[]).unwrap();
        let expected = FILE.replace("PowerShell 7\"", "PowerShell Seven\"");
        assert_eq!(edited.source, expected);
        assert_eq!(edited.profiles[0].name, "PowerShell Seven");
        // Fields the form does not know stay.
        assert_eq!(edited.profiles[0].icon.as_deref(), Some("PS"));
        assert!(edited.profiles[0].latest.is_some());
        assert!(edited.dropped_secrets.is_empty());
    }

    #[test]
    fn unchanged_input_leaves_the_file_byte_for_byte() {
        let same = ProfileInput {
            id: "own".into(),
            name: "Own agent".into(),
            command: "own-agent".into(),
            args: Vec::new(),
            cwd: None,
            env: [("REGION".to_owned(), "eu".to_owned())].into(),
            secret_env: vec!["OWN_KEY".into(), "OLD_KEY".into()],
        };
        assert_eq!(upsert(FILE, &same, &[]).unwrap().source, FILE);
    }

    #[test]
    fn env_and_secrets_change_in_place() {
        let mut change = input("own");
        change.env = [
            ("REGION".to_owned(), "us".to_owned()),
            ("MODE".to_owned(), "x y".to_owned()),
        ]
        .into();
        change.secret_env = vec!["OWN_KEY".into()];
        change.cwd = Some(PathBuf::from(ABSOLUTE_CWD));
        let edited = upsert(FILE, &change, &[]).unwrap();
        assert_eq!(edited.dropped_secrets, ["OLD_KEY"]);
        let own = edited.profiles.iter().find(|p| p.id == "own").unwrap();
        assert_eq!(own.secret_env, ["OWN_KEY"]);
        assert_eq!(own.env.get("MODE").map(String::as_str), Some("x y"));
        assert_eq!(own.args, ["--fast", "two words"]);
        assert_eq!(own.cwd.as_deref(), Some(std::path::Path::new(ABSOLUTE_CWD)));
        assert!(edited
            .source
            .starts_with("# My profiles, keep this comment.\n"));
        assert!(edited
            .source
            .contains("# Agent with keys\n[[profile]]\nid = \"own\""));
        assert!(edited.source.contains("[profile.env]\n"));
        assert!(edited.source.contains("id = \"pwsh\" # the shell\n"));
        // The other profiles keep their order.
        let ids: Vec<&str> = edited.profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["pwsh", "own", "cmd"]);

        // Empty optional fields disappear from the file.
        let edited = upsert(&edited.source, &input("own"), &[]).unwrap();
        let own = edited.profiles.iter().find(|p| p.id == "own").unwrap();
        assert!(own.env.is_empty() && own.secret_env.is_empty() && own.cwd.is_none());
        assert!(!edited.source.contains("[profile.env]"));
        assert!(!edited.source.contains("secret_env"));
        assert_eq!(edited.dropped_secrets, ["OWN_KEY"]);
    }

    #[test]
    fn inline_env_stays_inline() {
        let file = "[[profile]]\nid = \"a\"\nname = \"A\"\ncommand = \"a\"\nenv = { X = \"1\" }\n";
        let mut change = input("a");
        change.env = [("X".to_owned(), "2".to_owned())].into();
        let edited = upsert(file, &change, &[]).unwrap();
        assert!(
            edited.source.contains("env = { X = \"2\" }"),
            "{}",
            edited.source
        );
    }

    #[test]
    fn new_profiles_are_appended_and_catalog_ids_become_overrides() {
        let mut change = input("claude");
        change.secret_env = vec!["ANTHROPIC_API_KEY".into()];
        change.env = [("DISABLE_TELEMETRY".to_owned(), "1".to_owned())].into();
        let edited = upsert(FILE, &change, &[]).unwrap();
        assert!(edited.source.starts_with(FILE));
        let ids: Vec<&str> = edited.profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["pwsh", "own", "cmd", "claude"]);
        let claude = &edited.profiles[3];
        assert_eq!(claude.secret_env, ["ANTHROPIC_API_KEY"]);
        // Reads back like a hand-written profile, and a second save changes nothing.
        assert_eq!(
            upsert(&edited.source, &change, &[]).unwrap().source,
            edited.source
        );
    }

    #[test]
    fn a_missing_file_section_is_created() {
        let edited = upsert("# only a comment\n", &input("x"), &[]).unwrap();
        assert!(
            edited.source.contains("# only a comment\n"),
            "{:?}",
            edited.source
        );
        assert_eq!(edited.profiles.len(), 1);
    }

    #[test]
    fn invalid_input_is_rejected_like_loading() {
        for id in ["", "a b", "a/b"] {
            assert!(matches!(
                upsert(FILE, &input(id), &[]),
                Err(EditError::InvalidId(_))
            ));
        }
        // Ids that differ only in case from another entry: in the file or in the catalog.
        assert!(matches!(
            upsert(FILE, &input("PWSH"), &[]),
            Err(EditError::Invalid(ProfileError::IdCase(..)))
        ));
        let catalog = ["claude".to_owned(), "wsl-ubuntu".to_owned()];
        assert!(matches!(
            upsert(FILE, &input("Claude"), &catalog),
            Err(EditError::IdCase(..))
        ));
        // The same spelling overrides the catalog entry.
        upsert(FILE, &input("claude"), &catalog).unwrap();
        // A working folder must be absolute; empty means none.
        for relative in ["work", "./work", "..\\work"] {
            let mut change = input("x");
            change.cwd = Some(PathBuf::from(relative));
            assert!(
                matches!(upsert(FILE, &change, &[]), Err(EditError::RelativeCwd)),
                "{relative}"
            );
        }
        let mut change = input("x");
        change.cwd = Some(PathBuf::new());
        upsert(FILE, &change, &[]).unwrap();
        let mut change = input("x");
        change.command = " ".into();
        assert!(matches!(
            upsert(FILE, &change, &[]),
            Err(EditError::Invalid(ProfileError::Empty { .. }))
        ));
        let mut change = input("x");
        change.secret_env = vec!["NOT VALID".into()];
        assert!(matches!(
            upsert(FILE, &change, &[]),
            Err(EditError::Invalid(ProfileError::Invalid { .. }))
        ));
        assert!(matches!(
            upsert("[[profile]\nid=", &input("x"), &[]),
            Err(EditError::Syntax(_))
        ));
        assert!(matches!(
            upsert("profile = 3\n", &input("x"), &[]),
            Err(EditError::Layout)
        ));
    }

    #[test]
    fn deleting_keeps_the_rest() {
        let edited = delete(FILE, "own").unwrap();
        assert_eq!(edited.dropped_secrets, ["OWN_KEY", "OLD_KEY"]);
        let ids: Vec<&str> = edited.profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["pwsh", "cmd"]);
        assert!(edited
            .source
            .starts_with("# My profiles, keep this comment.\n"));
        assert!(edited.source.contains("id = \"pwsh\" # the shell\n"));
        assert!(!edited.source.contains("REGION"));
        assert!(matches!(
            delete(FILE, "claude"),
            Err(EditError::NotUserProfile(_))
        ));
        let single = "[[profile]]\nid='a'\nname='A'\ncommand='a'\n";
        assert!(matches!(delete(single, "a"), Err(EditError::LastProfile)));
    }

    #[test]
    fn edits_reach_the_file_and_failures_leave_it_alone() {
        let tmp = crate::test_support::TempDir::new("profile-edit");
        let dir = tmp.path().join("config");
        // A missing file is created from the template first, then changed.
        let edited = edit_file(&dir, |source| upsert(source, &input("added"), &[])).unwrap();
        let written = std::fs::read_to_string(dir.join(profile::FILE_NAME)).unwrap();
        assert_eq!(written, edited.source);
        assert!(written.starts_with('#'), "template comments are kept");
        let loaded = profile::load_or_init(&dir).unwrap();
        assert_eq!(loaded.last().map(|p| p.id.as_str()), Some("added"));

        let mut broken = input("other");
        broken.command = String::new();
        assert!(edit_file(&dir, |source| upsert(source, &broken, &[])).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join(profile::FILE_NAME)).unwrap(),
            written
        );

        edit_file(&dir, |source| delete(source, "added")).unwrap();
        let loaded = profile::load_or_init(&dir).unwrap();
        assert!(loaded.iter().all(|p| p.id != "added"));
    }

    #[test]
    fn input_arrives_in_camel_case() {
        let input: ProfileInput = serde_json::from_value(serde_json::json!({
            "id": "x", "name": "X", "command": "x", "args": ["a"], "cwd": null,
            "env": { "A": "1" }, "secretEnv": ["KEY"]
        }))
        .unwrap();
        assert_eq!(input.secret_env, ["KEY"]);
        assert!(serde_json::from_value::<ProfileInput>(serde_json::json!({
            "id": "x", "name": "X", "command": "x", "icon": "y"
        }))
        .is_err());
    }
}
