//! Integration tests with real programs, without a PTY: version query (ADR 0003) including
//! the time limit, and program icons.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use hangar_lib::icon;
use hangar_lib::manager::SessionManager;
use hangar_lib::profile::Profile;
use hangar_lib::resolve::resolve_program;
use hangar_lib::version::{self, VersionCache};

fn find(command: &str) -> PathBuf {
    let path = std::env::var_os("PATH");
    let pathext = std::env::var_os("PATHEXT");
    resolve_program(command, path.as_deref(), pathext.as_deref())
        .unwrap_or_else(|| panic!("{command} not on PATH"))
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| (*a).to_owned()).collect()
}

fn no_env() -> BTreeMap<String, String> {
    BTreeMap::new()
}

/// These tests check which version is read, not how fast a program starts. A cold Windows
/// PowerShell on a CI runner took longer than the app's `version::TIMEOUT` of 5 s; the timeout
/// itself has its own test.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(windows)]
#[test]
fn version_of_cmd_with_builtin_exception() {
    let cmd = find("cmd");
    let version = version::probe(
        &cmd,
        &version::version_args(None, &cmd),
        &no_env(),
        PROBE_TIMEOUT,
    )
    .unwrap();
    // "Microsoft Windows [Version 10.0.x.y]"
    assert!(version.starts_with("10.0."), "{version}");
}

#[cfg(windows)]
#[test]
fn version_of_windows_powershell_is_not_the_windows_version() {
    let powershell = find("powershell");
    let version = version::probe(
        &powershell,
        &version::version_args(None, &powershell),
        &no_env(),
        PROBE_TIMEOUT,
    )
    .unwrap();
    assert!(version.starts_with("5.1"), "{version}");
}

#[cfg(unix)]
#[test]
fn version_of_a_small_program() {
    let version = version::probe(
        &find("sh"),
        &args(&["-c", "echo tool 1.2.3"]),
        &no_env(),
        PROBE_TIMEOUT,
    );
    assert_eq!(version.as_deref(), Some("1.2.3"));
}

#[test]
fn stderr_counts_when_stdout_has_no_version() {
    #[cfg(windows)]
    let (program, list) = ("cmd", ["/d", "/c", "echo none & echo 4.5.6 1>&2 & exit 2"]);
    #[cfg(unix)]
    let (program, list) = ("sh", ["-c", "echo none; echo 4.5.6 >&2; exit 2"]);
    let version = version::probe(&find(program), &args(&list), &no_env(), PROBE_TIMEOUT);
    assert_eq!(version.as_deref(), Some("4.5.6"));
}

#[test]
fn profile_env_reaches_the_query() {
    #[cfg(windows)]
    let (program, list) = ("cmd", ["/d", "/c", "echo %HANGAR_TEST_VERSION%"]);
    #[cfg(unix)]
    let (program, list) = ("sh", ["-c", "echo $HANGAR_TEST_VERSION"]);
    let env = BTreeMap::from([("HANGAR_TEST_VERSION".to_owned(), "3.14.15".to_owned())]);
    let version = version::probe(&find(program), &args(&list), &env, PROBE_TIMEOUT);
    assert_eq!(version.as_deref(), Some("3.14.15"));
}

/// The child prints the version and then hangs in a grandchild that keeps the pipes open.
/// Only killing the whole tree delivers EOF; the partial output still counts.
#[test]
fn time_limit_kills_the_process_tree() {
    #[cfg(windows)]
    let (program, list) = (
        "cmd",
        ["/d", "/c", "echo 9.8.7 & ping -n 60 127.0.0.1 >nul"],
    );
    #[cfg(unix)]
    let (program, list) = ("sh", ["-c", "echo 9.8.7; sleep 60; echo never"]);
    let started = Instant::now();
    let version = version::probe(
        &find(program),
        &args(&list),
        &no_env(),
        Duration::from_secs(1),
    );
    let elapsed = started.elapsed();
    assert_eq!(version.as_deref(), Some("9.8.7"));
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    // Time limit plus grace period; if the grandchild kept holding the pipes, it would be 60 s.
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
}

#[test]
fn unknown_program_yields_no_version() {
    let missing = std::env::temp_dir().join("hangar-does-not-exist-4711");
    assert_eq!(
        version::probe(&missing, &args(&["--version"]), &no_env(), PROBE_TIMEOUT),
        None
    );
}

fn profile(id: &str, command: &str, version_args: Option<&[&str]>) -> Profile {
    Profile {
        id: id.into(),
        name: id.into(),
        command: command.into(),
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
        secret_env: Vec::new(),
        icon: None,
        version_args: version_args.map(args),
        resume_args: None,
        session_id_args: None,
        resume_id_args: None,
        sub_agents: None,
        latest: None,
        update_hint: None,
        packages: None,
        self_update: None,
    }
}

#[test]
fn detect_only_installed_and_with_cache() {
    #[cfg(windows)]
    let (program, list) = ("cmd", ["/d", "/c", "echo 7.7.7"]);
    #[cfg(unix)]
    let (program, list) = ("sh", ["-c", "echo 7.7.7"]);
    let manager = SessionManager::new(
        vec![
            profile("present", program, Some(&list)),
            profile("missing", "hangar-does-not-exist-4711", None),
        ],
        Vec::new(),
        std::env::temp_dir(),
    );
    let installed = manager.installed();
    let cache = VersionCache::default();
    let versions = version::detect(&installed, &cache);
    assert_eq!(versions.len(), 1);
    assert_eq!(versions["present"].as_deref(), Some("7.7.7"));

    // The second call comes from the cache, without starting the program.
    let started = Instant::now();
    assert_eq!(version::detect(&installed, &cache), versions);
    assert!(started.elapsed() < Duration::from_millis(50));
}

/// A launcher whose file stamp stays the same while the program behind it changes (npm swaps
/// the package, not `gemini.cmd`; an agent updates itself): only `redetect` sees the new version.
#[test]
fn redetect_bypasses_the_remembered_version() {
    #[cfg(windows)]
    let (program, list) = ("cmd", ["/d", "/c", "echo %HANGAR_TEST_VERSION%"]);
    #[cfg(unix)]
    let (program, list) = ("sh", ["-c", "echo $HANGAR_TEST_VERSION"]);
    let mut present = profile("present", program, Some(&list));
    present
        .env
        .insert("HANGAR_TEST_VERSION".into(), "7.7.7".into());
    let manager = SessionManager::new(vec![present], Vec::new(), std::env::temp_dir());
    let mut installed = manager.installed();
    let cache = VersionCache::default();
    let version_of = |found: HashMap<String, Option<String>>| found["present"].clone();
    assert_eq!(
        version_of(version::detect(&installed, &cache)).as_deref(),
        Some("7.7.7")
    );

    for (agent, _) in &mut installed {
        agent
            .profile
            .env
            .insert("HANGAR_TEST_VERSION".into(), "8.8.8".into());
    }
    assert_eq!(
        version_of(version::detect(&installed, &cache)).as_deref(),
        Some("7.7.7")
    );
    assert_eq!(
        version_of(version::redetect(&installed, &cache)).as_deref(),
        Some("8.8.8")
    );
    // The new answer is remembered for the plain query as well.
    assert_eq!(
        version_of(version::detect(&installed, &cache)).as_deref(),
        Some("8.8.8")
    );
}

#[cfg(windows)]
#[test]
fn icon_of_cmd_is_png_32x32() {
    let cmd = find("cmd");
    let url = icon::program_icon(&cmd).unwrap();
    // Base64 of the PNG signature 89 50 4E 47 0D 0A 1A 0A.
    assert!(
        url.starts_with("data:image/png;base64,iVBORw0KGgo"),
        "{url:.60}"
    );
    let png = icon::icon_png(&cmd).unwrap();
    assert_eq!(&png[16..24], &[0, 0, 0, 32, 0, 0, 0, 32]);
}

#[cfg(windows)]
#[test]
fn icon_of_pwsh_if_present() {
    let path = std::env::var_os("PATH");
    let pathext = std::env::var_os("PATHEXT");
    let Some(pwsh) = resolve_program("pwsh", path.as_deref(), pathext.as_deref()) else {
        return;
    };
    let png = icon::icon_png(&pwsh).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[16..24], &[0, 0, 0, 32, 0, 0, 0, 32]);
}

/// Every execution alias in the user's WindowsApps folder (Store apps such as PowerShell, winget)
/// names an executable inside its package that can be read, so icon, stamp and version work.
#[cfg(windows)]
#[test]
fn execution_aliases_lead_to_their_package_executable() {
    use hangar_lib::exec_alias;
    use hangar_lib::filecache::FileStamp;

    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let folder = PathBuf::from(local).join("Microsoft").join("WindowsApps");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return;
    };
    let mut seen = 0;
    for entry in entries.flatten() {
        let alias_path = entry.path();
        let Some(alias) = exec_alias::read(&alias_path) else {
            continue;
        };
        seen += 1;
        assert!(alias.app_id.starts_with(&alias.package), "{alias:?}");
        // Some aliases point into C:\Windows\SystemApps; all of them name an existing file.
        if !alias.target.is_file() {
            continue;
        }
        let stamp = FileStamp::of(&alias_path).unwrap();
        assert_eq!(stamp.path, alias_path);
        assert_eq!(
            stamp.len,
            std::fs::metadata(&alias.target).unwrap().len(),
            "{alias:?}"
        );
    }
    // A machine without Store apps has nothing to check; one with them must find some.
    if folder.join("winget.exe").exists() {
        assert!(seen > 0);
    }
}

/// The real Store PowerShell gets no winget or administrator offer: the Store updates it.
#[cfg(windows)]
#[test]
fn store_powershell_is_left_to_the_store() {
    use hangar_lib::install_source::{self, Installer, PlanInput, PlanKind, Reason, SystemProbe};

    let path = std::env::var_os("PATH");
    let pathext = std::env::var_os("PATHEXT");
    let Some(pwsh) = resolve_program("pwsh", path.as_deref(), pathext.as_deref()) else {
        return;
    };
    if hangar_lib::exec_alias::read(&pwsh).is_none() {
        return;
    }
    let mut pwsh_profile = profile("pwsh", "pwsh", None);
    let (latest, packages) = hangar_lib::updates::builtin_for_command("pwsh").unwrap();
    pwsh_profile.latest = Some(latest);
    pwsh_profile.packages = Some(packages);
    let plan = install_source::plan(
        &PlanInput {
            profile: &pwsh_profile,
            install_hint: Some("winget install Microsoft.PowerShell"),
            program: Some(&pwsh),
            candidates: &["pwsh".to_owned()],
        },
        &SystemProbe,
    );
    assert_eq!(plan.installer, Some(Installer::Store), "{plan:?}");
    assert_eq!(plan.kind(), PlanKind::None);
    assert_eq!(plan.reason, Some(Reason::StoreManaged));
    assert!(!plan.needs_admin);
}

/// A Store PowerShell starts through its alias, also for the version query.
#[cfg(windows)]
#[test]
fn version_through_an_execution_alias() {
    let path = std::env::var_os("PATH");
    let pathext = std::env::var_os("PATHEXT");
    let Some(pwsh) = resolve_program("pwsh", path.as_deref(), pathext.as_deref()) else {
        return;
    };
    if hangar_lib::exec_alias::read(&pwsh).is_none() {
        return;
    }
    let version = version::probe(
        &pwsh,
        &version::version_args(None, &pwsh),
        &no_env(),
        PROBE_TIMEOUT,
    );
    assert!(version.is_some_and(|v| v.starts_with('7')));
}

#[test]
fn script_shim_yields_no_icon() {
    let dir = std::env::temp_dir().join(format!("hangar-shim-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shim = dir.join("agent.cmd");
    std::fs::write(&shim, b"@echo off\r\nnode agent.js %*\r\n").unwrap();
    assert_eq!(icon::program_icon(&shim), None);

    let manager = SessionManager::new(
        vec![profile("shim", shim.to_str().unwrap(), None)],
        Vec::new(),
        std::env::temp_dir(),
    );
    let icons = icon::icons(&manager.installed(), &icon::IconCache::default());
    std::fs::remove_dir_all(&dir).unwrap();
    // The search only checks whether the file exists, so the entry counts as installed everywhere.
    assert_eq!(icons.get("shim"), Some(&None));
}
