//! Agent profiles from the config file `profiles.toml`. New agents are added through a profile,
//! never through a code change.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::install_source::{Packages, SelfUpdate};
use crate::updates::{Latest, PlatformHint};

pub const FILE_NAME: &str = "profiles.toml";

#[cfg(windows)]
pub const DEFAULT_PROFILES: &str = include_str!("default_profiles_windows.toml");
#[cfg(not(windows))]
pub const DEFAULT_PROFILES: &str = include_str!("default_profiles_unix.toml");

/// Deliberately not `Serialize`: `env` may contain secrets. The frontend gets
/// [`crate::agent_catalog::ProfileView`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Program name or path. Never interpreted by a shell.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Names of variables whose values live in the OS keychain ([`crate::secrets`]) and are set
    /// when a session starts. Never values. Same style as `version_args`, hence snake_case.
    #[serde(default, rename = "secret_env")]
    pub secret_env: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    /// Arguments for the version query (ADR 0003). If the field is missing, the default from
    /// [`crate::version::version_args`] applies. Same name as in the catalog, hence snake_case.
    #[serde(default, rename = "version_args")]
    pub version_args: Option<Vec<String>>,
    /// Arguments for resuming the last conversation. They REPLACE `args` (rather than being appended),
    /// because some CLIs need a subcommand up front, such as `codex resume --last` or
    /// `goose session --resume`. If the field is missing or the list is empty, a resume starts
    /// normally with `args`. Same name as in the catalog, hence snake_case.
    #[serde(default, rename = "resume_args")]
    pub resume_args: Option<Vec<String>>,
    /// Arguments that give a fresh start a conversation id chosen by Hangar, with `{id}` as the
    /// placeholder (`["--session-id", "{id}"]`). APPENDED to `args`. See [`crate::cli_session`].
    #[serde(default, rename = "session_id_args")]
    pub session_id_args: Option<Vec<String>>,
    /// Arguments that resume exactly the conversation with the stored id (`["--resume", "{id}"]`).
    /// They REPLACE `args`, like `resume_args`, which still applies to sessions without a stored id.
    #[serde(default, rename = "resume_id_args")]
    pub resume_id_args: Option<Vec<String>>,
    /// Where the program keeps sub-agent transcripts that Hangar can show ([`crate::subagents`]).
    #[serde(default, rename = "sub_agents")]
    pub sub_agents: Option<crate::subagents::SubAgentKind>,
    /// Where the newest version is published (npm, PyPI or GitHub release), see [`Latest`].
    #[serde(default)]
    pub latest: Option<Latest>,
    /// Update command per platform; shown to the user and run without a shell on click
    /// ([`crate::updates::update_command`]).
    #[serde(default, rename = "update_hint")]
    pub update_hint: Option<PlatformHint>,
    /// Package name per installer, for the one-click update ([`crate::install_source`]).
    #[serde(default)]
    pub packages: Option<Packages>,
    /// The program's own update command ([`crate::install_source::SelfUpdate`]).
    #[serde(default, rename = "self_update")]
    pub self_update: Option<SelfUpdate>,
}

impl Profile {
    /// Arguments for resuming, if the profile has any. An empty list counts as none.
    pub fn resume_args(&self) -> Option<&[String]> {
        self.resume_args.as_deref().filter(|args| !args.is_empty())
    }

    /// `session_id_args`, if the profile has any. An empty list counts as none.
    pub fn session_id_args(&self) -> Option<&[String]> {
        self.session_id_args
            .as_deref()
            .filter(|args| !args.is_empty())
    }

    /// `resume_id_args`, if the profile has any. An empty list counts as none.
    pub fn resume_id_args(&self) -> Option<&[String]> {
        self.resume_id_args
            .as_deref()
            .filter(|args| !args.is_empty())
    }
}

/// A non-empty `session_id_args` or `resume_id_args` must carry the id somewhere; without the
/// placeholder every session would share one conversation or none would get its own.
pub fn check_id_args(
    session_id_args: Option<&[String]>,
    resume_id_args: Option<&[String]>,
) -> Result<(), &'static str> {
    let lacks_placeholder = |args: Option<&[String]>| {
        args.is_some_and(|args| {
            !args.is_empty()
                && !args
                    .iter()
                    .any(|a| a.contains(crate::cli_session::PLACEHOLDER))
        })
    };
    if lacks_placeholder(session_id_args) || lacks_placeholder(resume_id_args) {
        return Err("session_id_args and resume_id_args need the placeholder {id}");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    #[serde(default, rename = "profile")]
    profiles: Vec<Profile>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("Profile file {path} cannot be read or written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Invalid profile file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Profile ID '{0}' appears more than once")]
    DuplicateId(String),
    /// The Windows Credential Manager ignores case in its target names, so both profiles would
    /// share (and delete) each other's secrets.
    #[error("Profile IDs '{0}' and '{1}' differ only in case; give one of them another ID")]
    IdCase(String, String),
    #[error("Profile '{profile}': field '{field}' must not be empty")]
    Empty {
        profile: String,
        field: &'static str,
    },
    #[error("The profile file contains no [[profile]]")]
    NoProfiles,
    #[error("Profile '{profile}': {problem}")]
    Invalid {
        profile: String,
        problem: &'static str,
    },
}

pub fn parse(source: &str) -> Result<Vec<Profile>, ProfileError> {
    let file: ProfileFile = toml::from_str(source)?;
    if file.profiles.is_empty() {
        return Err(ProfileError::NoProfiles);
    }
    let mut seen = HashSet::new();
    let mut folded: BTreeMap<String, &str> = BTreeMap::new();
    for profile in &file.profiles {
        let label = if profile.id.is_empty() {
            &profile.name
        } else {
            &profile.id
        };
        for (field, value) in [
            ("id", &profile.id),
            ("name", &profile.name),
            ("command", &profile.command),
        ] {
            if value.trim().is_empty() {
                return Err(ProfileError::Empty {
                    profile: label.clone(),
                    field,
                });
            }
        }
        if !seen.insert(profile.id.as_str()) {
            return Err(ProfileError::DuplicateId(profile.id.clone()));
        }
        if let Some(other) = folded.insert(fold_id(&profile.id), &profile.id) {
            return Err(ProfileError::IdCase(other.to_owned(), profile.id.clone()));
        }
        let update_fields = [
            profile.latest.as_ref().map(Latest::validate),
            profile.packages.as_ref().map(Packages::validate),
            profile.self_update.as_ref().map(SelfUpdate::validate),
        ];
        if let Some(Err(problem)) = update_fields.into_iter().flatten().find(Result::is_err) {
            return Err(ProfileError::Invalid {
                profile: label.clone(),
                problem,
            });
        }
        if let Err(problem) = check_id_args(
            profile.session_id_args.as_deref(),
            profile.resume_id_args.as_deref(),
        ) {
            return Err(ProfileError::Invalid {
                profile: label.clone(),
                problem,
            });
        }
        if let Err(problem) = check_environment(profile) {
            return Err(ProfileError::Invalid {
                profile: label.clone(),
                problem,
            });
        }
    }
    Ok(file.profiles)
}

/// Profile id as the keychain compares it: without case (Windows Credential Manager).
pub fn fold_id(id: &str) -> String {
    id.to_lowercase()
}

/// `env` names a process can carry, and `secret_env` names that are valid variables, appear
/// once and do not also have a plain value in `env`.
fn check_environment(profile: &Profile) -> Result<(), &'static str> {
    if profile
        .env
        .keys()
        .any(|key| key.is_empty() || key.contains(['=', '\0']))
    {
        return Err("env names must not be empty or contain '=' or NUL");
    }
    let mut seen = HashSet::new();
    for name in &profile.secret_env {
        if !crate::secrets::is_valid_name(name) {
            return Err("secret_env names may only contain letters, digits and _");
        }
        if !seen.insert(name.to_ascii_uppercase()) {
            return Err("a secret_env name appears more than once");
        }
        if profile.env.keys().any(|key| key.eq_ignore_ascii_case(name)) {
            return Err("a variable is listed in both env and secret_env");
        }
    }
    Ok(())
}

/// Path of `profiles.toml` in `dir`. If it is missing, it is created from the commented template
/// ([`DEFAULT_PROFILES`]); an existing file stays untouched.
pub fn ensure_file(dir: &Path) -> Result<PathBuf, ProfileError> {
    let path = dir.join(FILE_NAME);
    if !path.exists() {
        let io_error = |source| ProfileError::Io {
            path: path.clone(),
            source,
        };
        std::fs::create_dir_all(dir).map_err(io_error)?;
        std::fs::write(&path, DEFAULT_PROFILES).map_err(io_error)?;
    }
    Ok(path)
}

/// Loads `profiles.toml` from `dir` and creates the default profiles on first start.
pub fn load_or_init(dir: &Path) -> Result<Vec<Profile>, ProfileError> {
    let path = ensure_file(dir)?;
    let source = std::fs::read_to_string(&path).map_err(|source| ProfileError::Io {
        path: path.clone(),
        source,
    })?;
    parse(&source)
}

/// Reload error, prepared so the window can show it in an understandable way: for
/// syntax and field errors the location in the file comes with it (from 1, column in characters).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct ProfileProblem {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl ProfileProblem {
    /// `source` is the file content that was read; only with it can the location of a
    /// TOML error (byte range) be converted into line and column.
    pub fn new(error: &ProfileError, source: Option<&str>) -> Self {
        if let ProfileError::Parse(parse) = error {
            let position = parse
                .span()
                .zip(source)
                .map(|(span, source)| line_column(source, span.start));
            return Self {
                message: parse.message().trim().to_owned(),
                line: position.map(|(line, _)| line),
                column: position.map(|(_, column)| column),
            };
        }
        Self {
            message: error.to_string(),
            line: None,
            column: None,
        }
    }
}

/// Line and column (both from 1) for the byte offset `offset`. An offset in the middle of a
/// character or past the end counts up to the nearest valid boundary before it.
fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(source.len());
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let before = &source[..end];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = before[line_start..].chars().count() + 1;
    (line, column)
}

/// Rereads `profiles.toml` without restarting the app. If the file is missing, the template is
/// created as on first start. On an error the caller's previous state remains valid.
pub fn reload(dir: &Path) -> Result<Vec<Profile>, ProfileProblem> {
    let path = ensure_file(dir).map_err(|error| ProfileProblem::new(&error, None))?;
    let source = std::fs::read_to_string(&path)
        .map_err(|source| ProfileProblem::new(&ProfileError::Io { path, source }, None))?;
    parse(&source).map_err(|error| ProfileProblem::new(&error, Some(&source)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profiles_of_both_platforms_are_valid() {
        for source in [
            include_str!("default_profiles_windows.toml"),
            include_str!("default_profiles_unix.toml"),
        ] {
            let profiles = parse(source).unwrap();
            assert!(!profiles.is_empty());
            // Agents come from the catalog; if they were listed here, they would override it forever.
            let catalog = crate::agent_catalog::builtin().unwrap();
            for profile in &profiles {
                assert!(
                    catalog.iter().all(|a| a.id != profile.id),
                    "default profile '{}' hides a catalog agent",
                    profile.id
                );
            }
        }
        let windows = parse(include_str!("default_profiles_windows.toml")).unwrap();
        let ids: Vec<&str> = windows.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["pwsh", "powershell", "cmd"]);
    }

    #[test]
    fn full_profile() {
        let profiles = parse(
            r#"
            [[profile]]
            id = "x"
            name = "X"
            command = "C:\\Program Files\\x\\x.exe"
            args = ["--a", "b c"]
            cwd = "C:\\Users\\Test User"
            icon = "x"
            [profile.env]
            FOO = "bar"
            "#,
        )
        .unwrap();
        let p = &profiles[0];
        assert_eq!(p.command, r"C:\Program Files\x\x.exe");
        assert_eq!(p.args, vec!["--a", "b c"]);
        assert_eq!(p.cwd.as_deref(), Some(Path::new(r"C:\Users\Test User")));
        assert_eq!(p.env.get("FOO").map(String::as_str), Some("bar"));
    }

    #[test]
    fn minimal_profile_has_defaults() {
        let p = &parse("[[profile]]\nid='a'\nname='A'\ncommand='a'\n").unwrap()[0];
        assert!(p.args.is_empty());
        assert!(p.env.is_empty());
        assert_eq!(p.cwd, None);
        assert_eq!(p.icon, None);
    }

    #[test]
    fn resume_args_replace_args_and_empty_means_none() {
        let src = "[[profile]]\nid='g'\nname='G'\ncommand='goose'\nargs=['session']\nresume_args=['session','--resume']\n\
                   [[profile]]\nid='e'\nname='E'\ncommand='e'\nresume_args=[]\n";
        let profiles = parse(src).unwrap();
        assert_eq!(profiles[0].args, vec!["session"]);
        assert_eq!(
            profiles[0].resume_args(),
            Some(&["session".to_owned(), "--resume".to_owned()][..])
        );
        assert_eq!(profiles[1].resume_args(), None);
        let minimal = &parse("[[profile]]\nid='a'\nname='A'\ncommand='a'\n").unwrap()[0];
        assert_eq!(minimal.resume_args(), None);
        // camelCase would be a typo and gets caught by deny_unknown_fields.
        let camel = "[[profile]]\nid='a'\nname='A'\ncommand='a'\nresumeArgs=['x']\n";
        assert!(matches!(parse(camel), Err(ProfileError::Parse(_))));
    }

    #[test]
    fn id_args_and_sub_agents() {
        let src = "[[profile]]\nid='c'\nname='C'\ncommand='claude'\n\
                   session_id_args=['--session-id','{id}']\nresume_id_args=['--resume','{id}']\n\
                   sub_agents='claude'\n";
        let p = &parse(src).unwrap()[0];
        assert_eq!(p.session_id_args().map(<[String]>::len), Some(2));
        assert_eq!(p.resume_id_args().map(<[String]>::len), Some(2));
        assert_eq!(p.sub_agents, Some(crate::subagents::SubAgentKind::Claude));
        let base = "[[profile]]\nid='c'\nname='C'\ncommand='claude'\n";
        let empty = &parse(&format!("{base}session_id_args=[]\n")).unwrap()[0];
        assert_eq!(empty.session_id_args(), None);
        for bad in [
            "session_id_args=['--session-id']\n",
            "resume_id_args=['--resume','id']\n",
        ] {
            assert!(
                matches!(
                    parse(&format!("{base}{bad}")),
                    Err(ProfileError::Invalid { .. })
                ),
                "{bad}"
            );
        }
        assert!(matches!(
            parse(&format!("{base}sub_agents='codex'\n")),
            Err(ProfileError::Parse(_))
        ));
    }

    #[test]
    fn latest_and_update_hint() {
        let src = "[[profile]]\nid='p'\nname='P'\ncommand='pwsh'\n\
                   latest={ github='PowerShell/PowerShell', tag_prefix='v' }\n\
                   update_hint={ windows='winget upgrade Microsoft.PowerShell' }\n";
        let p = &parse(src).unwrap()[0];
        assert!(p.latest.as_ref().unwrap().validate().is_ok());
        assert_eq!(
            p.update_hint.as_ref().unwrap().windows.as_deref(),
            Some("winget upgrade Microsoft.PowerShell")
        );
        let bad = "[[profile]]\nid='p'\nname='P'\ncommand='x'\nlatest={ npm='../x' }\n";
        assert!(matches!(parse(bad), Err(ProfileError::Invalid { .. })));
        let typo = "[[profile]]\nid='p'\nname='P'\ncommand='x'\nupdate_hint={ win='x' }\n";
        assert!(matches!(parse(typo), Err(ProfileError::Parse(_))));
    }

    #[test]
    fn packages_and_self_update() {
        let src = "[[profile]]\nid='p'\nname='P'\ncommand='x'\n\
                   packages={ npm='@scope/x', winget='Vendor.X' }\n\
                   self_update={ args=['update'], aware=true }\n";
        let p = &parse(src).unwrap()[0];
        assert_eq!(
            p.packages.as_ref().unwrap().winget.as_deref(),
            Some("Vendor.X")
        );
        assert!(p.self_update.as_ref().unwrap().aware);
        for bad in [
            "packages={ winget='x & y' }",
            "packages={ npm='../x' }",
            "self_update={ args=[] }",
            "self_update={ args=['a|b'] }",
        ] {
            let src = format!("[[profile]]\nid='p'\nname='P'\ncommand='x'\n{bad}\n");
            assert!(
                matches!(parse(&src), Err(ProfileError::Invalid { .. })),
                "{bad}"
            );
        }
    }

    #[test]
    fn duplicate_id() {
        let src = "[[profile]]\nid='a'\nname='A'\ncommand='a'\n[[profile]]\nid='a'\nname='B'\ncommand='b'\n";
        assert!(matches!(parse(src), Err(ProfileError::DuplicateId(id)) if id == "a"));
    }

    #[test]
    fn ids_differing_only_in_case_are_refused() {
        let src = "[[profile]]\nid='Work'\nname='A'\ncommand='a'\n[[profile]]\nid='work'\nname='B'\ncommand='b'\n";
        assert!(
            matches!(parse(src), Err(ProfileError::IdCase(a, b)) if a == "Work" && b == "work")
        );
        let umlaut = "[[profile]]\nid='Ärger'\nname='A'\ncommand='a'\n[[profile]]\nid='ärger'\nname='B'\ncommand='b'\n";
        assert!(matches!(parse(umlaut), Err(ProfileError::IdCase(..))));
        let distinct = "[[profile]]\nid='work'\nname='A'\ncommand='a'\n[[profile]]\nid='work2'\nname='B'\ncommand='b'\n";
        assert_eq!(parse(distinct).unwrap().len(), 2);
    }

    #[test]
    fn empty_command() {
        let src = "[[profile]]\nid='a'\nname='A'\ncommand='  '\n";
        assert!(matches!(
            parse(src),
            Err(ProfileError::Empty {
                field: "command",
                ..
            })
        ));
    }

    #[test]
    fn unknown_field_is_error() {
        let src = "[[profile]]\nid='a'\nname='A'\ncommand='a'\nargv=['x']\n";
        assert!(matches!(parse(src), Err(ProfileError::Parse(_))));
    }

    #[test]
    fn no_profiles() {
        assert!(matches!(parse(""), Err(ProfileError::NoProfiles)));
    }

    /// Not created yet, so tests can check that the file appears.
    struct TempDir(PathBuf, #[allow(dead_code)] crate::test_support::TempDir);

    impl TempDir {
        fn new(name: &str) -> Self {
            let outer = crate::test_support::TempDir::new(&format!("profile-{name}"));
            Self(outer.path().join("config"), outer)
        }
    }

    #[test]
    fn reload_reads_changed_file() {
        let dir = TempDir::new("reload");
        std::fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join(FILE_NAME);
        std::fs::write(&path, "[[profile]]\nid='a'\nname='A'\ncommand='a'\n").unwrap();
        assert_eq!(reload(&dir.0).unwrap()[0].id, "a");
        std::fs::write(
            &path,
            "[[profile]]\nid='a'\nname='A'\ncommand='a'\n[[profile]]\nid='b'\nname='B'\ncommand='b'\n",
        )
        .unwrap();
        let ids: Vec<String> = reload(&dir.0).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn reload_creates_missing_file_from_template() {
        let dir = TempDir::new("reload-new");
        let profiles = reload(&dir.0).unwrap();
        assert_eq!(profiles, parse(DEFAULT_PROFILES).unwrap());
        let written = std::fs::read_to_string(dir.0.join(FILE_NAME)).unwrap();
        assert_eq!(written, DEFAULT_PROFILES);
        // The template explains the fields and reloading.
        assert!(written.starts_with('#'));
        assert!(written.contains("Reload profiles"));
    }

    #[test]
    fn ensure_file_overwrites_nothing() {
        let dir = TempDir::new("ensure");
        std::fs::create_dir_all(&dir.0).unwrap();
        std::fs::write(dir.0.join(FILE_NAME), "# own\n").unwrap();
        let path = ensure_file(&dir.0).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "# own\n");
    }

    #[test]
    fn syntax_error_names_line_and_column() {
        let dir = TempDir::new("reload-syntax");
        std::fs::create_dir_all(&dir.0).unwrap();
        std::fs::write(
            dir.0.join(FILE_NAME),
            "# Comment\n[[profile]]\nid = 'a'\nname = 'Ä'\ncommand = \"a\n",
        )
        .unwrap();
        let problem = reload(&dir.0).unwrap_err();
        assert_eq!(problem.line, Some(5), "{problem:?}");
        assert!(problem.column.is_some(), "{problem:?}");
        assert!(!problem.message.is_empty());
        // Only the message, not toml's multi-line excerpt with line numbers.
        assert!(!problem.message.contains('\n'), "{problem:?}");
    }

    #[test]
    fn unknown_field_names_its_line() {
        let src = "[[profile]]\nid='a'\nname='A'\ncommand='a'\n\n[[profile]]\nid='b'\nname='B'\ncommand='b'\nargv=['x']\n";
        let problem = ProfileProblem::new(&parse(src).unwrap_err(), Some(src));
        assert_eq!(problem.line, Some(10), "{problem:?}");
        assert!(problem.message.contains("argv"), "{problem:?}");
    }

    #[test]
    fn semantic_errors_have_no_location() {
        let src = "[[profile]]\nid='a'\nname='A'\ncommand='a'\n[[profile]]\nid='a'\nname='B'\ncommand='b'\n";
        let problem = ProfileProblem::new(&parse(src).unwrap_err(), Some(src));
        assert_eq!(problem.line, None);
        assert!(problem.message.contains("'a'"), "{problem:?}");
    }

    #[test]
    fn line_and_column_count_characters() {
        assert_eq!(line_column("", 0), (1, 1));
        assert_eq!(line_column("ab\ncd", 4), (2, 2));
        assert_eq!(line_column("äb", 2), (1, 2));
        // In the middle of a character and past the end: up to the boundary before it.
        assert_eq!(line_column("ä", 1), (1, 1));
        assert_eq!(line_column("a\n", 99), (2, 1));
    }

    #[test]
    fn load_or_init_creates_file() {
        let tmp = TempDir::new("init");
        let dir = tmp.0.clone();
        let profiles = load_or_init(&dir).unwrap();
        assert!(!profiles.is_empty());
        assert!(dir.join(FILE_NAME).is_file());
        // The second call reads the existing file instead of overwriting it.
        std::fs::write(
            dir.join(FILE_NAME),
            "[[profile]]\nid='z'\nname='Z'\ncommand='z'\n",
        )
        .unwrap();
        assert_eq!(load_or_init(&dir).unwrap()[0].id, "z");
    }

    #[test]
    fn secret_env_names_are_checked() {
        let base = "[[profile]]\nid='a'\nname='A'\ncommand='a'\n";
        let p = &parse(&format!("{base}secret_env=['ANTHROPIC_API_KEY','X_1']\n")).unwrap()[0];
        assert_eq!(p.secret_env, ["ANTHROPIC_API_KEY", "X_1"]);
        for bad in [
            "secret_env=['A B']\n",
            "secret_env=['1A']\n",
            "secret_env=['A','a']\n",
            "secret_env=['TOKEN']\nenv={ TOKEN='x' }\n",
            "env={ 'A=B'='x' }\n",
            "env={ ''='x' }\n",
        ] {
            assert!(
                matches!(
                    parse(&format!("{base}{bad}")),
                    Err(ProfileError::Invalid { .. })
                ),
                "{bad}"
            );
        }
        // camelCase is a typo here as elsewhere in the file.
        assert!(matches!(
            parse(&format!("{base}secretEnv=['A']\n")),
            Err(ProfileError::Parse(_))
        ));
    }
}
