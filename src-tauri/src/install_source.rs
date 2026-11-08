//! Where an installed program came from, and which command updates exactly that copy
//! (contract: docs/wiki/pages/syntheses/program-updates.md, evidence:
//! docs/wiki/pages/sources/update-installers.md).
//!
//! Detection looks only at the resolved path and at files next to it; it never starts a process.
//! A bin folder alone never decides (`~/.local/bin` holds native Claude Code, uv tool and pipx
//! shims side by side): a match needs a second piece of evidence such as `node_modules/<package>`,
//! a uv receipt naming the path, or a `Cellar` segment in the real path.
//!
//! The update command is built only from catalog or profile data (validated package names) and
//! from absolute paths found on this machine. Tools such as npm or winget are looked up in known
//! folders and in absolute PATH entries only; a relative PATH entry would depend on the working
//! folder.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile::Profile;
use crate::updates::{has_shell_syntax, npm_name_ok, plain_name_ok, split_command, PlatformHint};

/// The uv receipt and the cargo list are small; nothing beyond this is read.
const MAX_METADATA_BYTES: usize = 1024 * 1024;
/// More copies than this are not listed.
const MAX_COPIES: usize = 16;
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

// --- Catalog data ---

/// Field `packages`: the package name of the program per installer. Only names verified against
/// the vendor docs; look-alikes (winget `Pressly.Goose`, npm `cursor-agent`) stay out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packages {
    #[serde(default)]
    pub npm: Option<String>,
    /// Only if it differs from `npm`; otherwise the npm name applies to pnpm as well.
    #[serde(default)]
    pub pnpm: Option<String>,
    #[serde(default)]
    pub winget: Option<String>,
    #[serde(default)]
    pub pypi: Option<String>,
    /// Homebrew formula.
    #[serde(default)]
    pub brew: Option<String>,
    /// Homebrew cask.
    #[serde(default)]
    pub cask: Option<String>,
    #[serde(default)]
    pub scoop: Option<String>,
}

pub fn winget_id_ok(id: &str) -> bool {
    id.len() <= 128
        && id.contains('.')
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && !id.ends_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

pub fn brew_name_ok(name: &str) -> bool {
    name.len() <= 100
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || matches!(b, b'-' | b'_' | b'.' | b'@' | b'+')
        })
}

fn scoop_name_ok(name: &str) -> bool {
    name.len() <= 100
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

impl Packages {
    pub fn validate(&self) -> Result<(), &'static str> {
        let check = |value: &Option<String>, ok: fn(&str) -> bool, problem| match value {
            Some(name) if !ok(name) => Err(problem),
            _ => Ok(()),
        };
        check(&self.npm, npm_name_ok, "packages: invalid npm package name")?;
        check(
            &self.pnpm,
            npm_name_ok,
            "packages: invalid pnpm package name",
        )?;
        check(&self.winget, winget_id_ok, "packages: invalid winget id")?;
        check(
            &self.pypi,
            plain_name_ok,
            "packages: invalid PyPI package name",
        )?;
        check(
            &self.brew,
            brew_name_ok,
            "packages: invalid Homebrew formula",
        )?;
        check(&self.cask, brew_name_ok, "packages: invalid Homebrew cask")?;
        check(
            &self.scoop,
            scoop_name_ok,
            "packages: invalid Scoop app name",
        )?;
        Ok(())
    }

    fn pnpm_name(&self) -> Option<&str> {
        self.pnpm.as_deref().or(self.npm.as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnlyOn {
    Windows,
    Unix,
}

/// Field `self_update`: the program updates itself with these arguments. `aware` means it knows
/// how it was installed and updates (or delegates) correctly for every channel; otherwise it is
/// used only for a native install.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfUpdate {
    pub args: Vec<String>,
    #[serde(default)]
    pub aware: bool,
    /// Only on this platform; missing means everywhere.
    #[serde(default)]
    pub only: Option<OnlyOn>,
}

impl SelfUpdate {
    pub fn applies_here(&self) -> bool {
        match self.only {
            None => true,
            Some(OnlyOn::Windows) => cfg!(windows),
            Some(OnlyOn::Unix) => !cfg!(windows),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.args.is_empty() {
            return Err("self_update needs at least one argument");
        }
        let bad = |arg: &String| {
            arg.trim().is_empty()
                || arg.chars().any(char::is_whitespace)
                || has_shell_syntax(arg).is_some()
        };
        if self.args.iter().any(bad) {
            return Err("self_update: arguments must be single words without shell syntax");
        }
        Ok(())
    }
}

// --- Probe ---

/// An entry under `...\CurrentVersion\Uninstall` (Windows "Apps & features").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallEntry {
    pub display_name: String,
    pub install_location: PathBuf,
    /// HKLM rather than HKCU.
    pub machine: bool,
}

/// Everything detection reads from the machine, so tests can supply a synthetic tree, a fixed
/// environment and fake uninstall entries.
pub trait Probe {
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// File content as text, `None` if missing, larger than `max` or not UTF-8.
    fn read_text(&self, path: &Path, max: usize) -> Option<String>;
    /// Real path with links resolved.
    fn canonical(&self, path: &Path) -> Option<PathBuf>;
    fn var(&self, name: &str) -> Option<OsString>;
    fn uninstall_entries(&self) -> Vec<UninstallEntry>;
}

/// The real machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProbe;

pub(crate) fn read_text_limited(path: &Path, max: usize) -> Option<String> {
    let bytes = crate::persist::read_limited(path, max).ok()??;
    String::from_utf8(bytes).ok()
}

impl Probe for SystemProbe {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
    fn read_text(&self, path: &Path, max: usize) -> Option<String> {
        read_text_limited(path, max)
    }
    fn canonical(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }
    fn var(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name).filter(|v| !v.is_empty())
    }
    fn uninstall_entries(&self) -> Vec<UninstallEntry> {
        registry::uninstall_entries()
    }
}

#[cfg(windows)]
mod registry {
    //! Reads the uninstall keys of HKLM (both registry views) and HKCU.

    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_EXPAND_SZ, REG_SZ,
    };

    use super::UninstallEntry;

    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    /// A machine rarely has more than a few hundred entries; this bounds a broken registry.
    const MAX_KEYS: u32 = 4096;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Closes the key on drop.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful RegOpenKeyExW and is closed exactly once.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    fn open(parent: HKEY, path: &str, flags: u32) -> Option<Key> {
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `path` is a NUL-terminated UTF-16 string that outlives the call, and `key` is a
        // valid out pointer.
        let status = unsafe { RegOpenKeyExW(parent, path.as_ptr(), 0, KEY_READ | flags, &mut key) };
        (status == ERROR_SUCCESS).then_some(Key(key))
    }

    fn string_value(key: &Key, name: &str) -> Option<String> {
        let name = wide(name);
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: size query with a null data pointer, as documented for RegQueryValueExW.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS || !(kind == REG_SZ || kind == REG_EXPAND_SZ) || size > 65536 {
            return None;
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut size = u32::try_from(buffer.len() * 2).ok()?;
        // SAFETY: `buffer` holds `size` bytes and lives across the call.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let text = String::from_utf16_lossy(&buffer[..len]);
        let text = text.trim().trim_matches('"').trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    fn subkeys(key: &Key) -> Vec<String> {
        let mut names = Vec::new();
        for index in 0..MAX_KEYS {
            let mut buffer = [0u16; 256];
            let mut len = buffer.len() as u32;
            // SAFETY: `buffer` holds `len` UTF-16 units; the optional out pointers are null.
            let status = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    buffer.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&buffer[..len as usize]));
        }
        names
    }

    pub fn uninstall_entries() -> Vec<UninstallEntry> {
        let roots = [
            (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, true),
            (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY, true),
            (HKEY_CURRENT_USER, 0, false),
        ];
        let mut entries = Vec::new();
        for (root, flags, machine) in roots {
            let Some(list) = open(root, UNINSTALL, flags) else {
                continue;
            };
            for name in subkeys(&list) {
                let Some(key) = open(list.0, &name, flags) else {
                    continue;
                };
                let Some(location) = string_value(&key, "InstallLocation") else {
                    continue;
                };
                entries.push(UninstallEntry {
                    display_name: string_value(&key, "DisplayName").unwrap_or(name),
                    install_location: PathBuf::from(location),
                    machine,
                });
            }
        }
        entries
    }
}

#[cfg(not(windows))]
mod registry {
    pub fn uninstall_entries() -> Vec<super::UninstallEntry> {
        Vec::new()
    }
}

// --- Paths ---

/// Comparison form of a path: `/` as separator, no verbatim prefix, no trailing separator,
/// lower case on Windows (case-insensitive file systems).
pub fn key(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .unwrap_or_else(|| text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned());
    let mut text = text.replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// The part of `path` below `dir`, in comparison form, or `None` if it is not inside.
fn below(path: &Path, dir: &Path) -> Option<String> {
    let path = key(path);
    let dir = key(dir);
    if dir.is_empty() {
        return None;
    }
    path.strip_prefix(&dir)?
        .strip_prefix('/')
        .map(str::to_owned)
}

fn first_segment(rest: &str) -> &str {
    rest.split('/').next().unwrap_or_default()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

fn stem_lower(path: &Path) -> String {
    path.file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_lowercase()
}

/// Known install folders, computed once per detection from the probe's environment.
struct Locations {
    home: Option<PathBuf>,
    appdata: Option<PathBuf>,
    local_appdata: Option<PathBuf>,
    program_files: Vec<PathBuf>,
    program_data: Option<PathBuf>,
    system_root: Option<PathBuf>,
    pnpm_home: Option<PathBuf>,
    bun: Option<PathBuf>,
    scoop: Vec<(PathBuf, bool)>,
    choco: Option<PathBuf>,
    uv_tools: Option<PathBuf>,
    pipx_homes: Vec<PathBuf>,
    pipx_bin: Option<PathBuf>,
    cargo: Option<PathBuf>,
}

impl Locations {
    fn new(probe: &dyn Probe) -> Self {
        let var = |name: &str| probe.var(name).map(PathBuf::from);
        let home = if cfg!(windows) {
            var("USERPROFILE").or_else(|| var("HOME"))
        } else {
            var("HOME")
        };
        let at_home = |rest: &str| home.as_ref().map(|h| h.join(rest));
        let appdata = var("APPDATA");
        let local_appdata = var("LOCALAPPDATA");
        let program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|n| var(n))
            .collect();
        let program_data = var("ProgramData").or_else(|| var("ALLUSERSPROFILE"));
        let system_root = var("SystemRoot").or_else(|| var("windir"));
        let pnpm_home = var("PNPM_HOME").or_else(|| {
            if cfg!(windows) {
                local_appdata.as_ref().map(|d| d.join("pnpm"))
            } else if cfg!(target_os = "macos") {
                at_home("Library/pnpm")
            } else {
                at_home(".local/share/pnpm")
            }
        });
        let bun = var("BUN_INSTALL").or_else(|| at_home(".bun"));
        let mut scoop = Vec::new();
        if let Some(user) = var("SCOOP").or_else(|| at_home("scoop")) {
            scoop.push((user, false));
        }
        if let Some(global) =
            var("SCOOP_GLOBAL").or_else(|| program_data.as_ref().map(|d| d.join("scoop")))
        {
            scoop.push((global, true));
        }
        let choco = var("ChocolateyInstall")
            .or_else(|| program_data.as_ref().map(|d| d.join("chocolatey")));
        let uv_tools = var("UV_TOOL_DIR").or_else(|| {
            if cfg!(windows) {
                appdata.as_ref().map(|d| d.join("uv").join("tools"))
            } else {
                var("XDG_DATA_HOME")
                    .map(|d| d.join("uv").join("tools"))
                    .or_else(|| at_home(".local/share/uv/tools"))
            }
        });
        let mut pipx_homes: Vec<PathBuf> = var("PIPX_HOME").into_iter().collect();
        pipx_homes.extend(
            [".local/pipx", ".local/share/pipx", "pipx"]
                .iter()
                .filter_map(|rest| at_home(rest)),
        );
        let pipx_bin = var("PIPX_BIN_DIR").or_else(|| at_home(".local/bin"));
        let cargo = var("CARGO_HOME").or_else(|| at_home(".cargo"));
        Self {
            home,
            appdata,
            local_appdata,
            program_files,
            program_data,
            system_root,
            pnpm_home,
            bun,
            scoop,
            choco,
            uv_tools,
            pipx_homes,
            pipx_bin,
            cargo,
        }
    }

    fn local_bin(&self) -> Option<PathBuf> {
        self.home.as_ref().map(|h| h.join(".local").join("bin"))
    }

    /// winget folders for portable packages: (Packages, Links, machine scope).
    fn winget(&self) -> Vec<(PathBuf, PathBuf, bool)> {
        let mut dirs = Vec::new();
        if let Some(local) = &self.local_appdata {
            let base = local.join("Microsoft").join("WinGet");
            dirs.push((base.join("Packages"), base.join("Links"), false));
        }
        for program_files in &self.program_files {
            let base = program_files.join("WinGet");
            dirs.push((base.join("Packages"), base.join("Links"), true));
        }
        dirs
    }

    fn windows_apps(&self) -> Option<PathBuf> {
        self.local_appdata
            .as_ref()
            .map(|d| d.join("Microsoft").join("WindowsApps"))
    }

    /// Machine-wide folders a normal user cannot write to.
    fn scope_of(&self, path: &Path) -> Scope {
        if cfg!(windows) {
            let machine = self
                .program_files
                .iter()
                .chain(self.program_data.iter())
                .any(|dir| below(path, dir).is_some());
            if machine {
                return Scope::Machine;
            }
            return Scope::User;
        }
        let path = key(path);
        if (path.starts_with("/usr/") && !path.starts_with("/usr/local/"))
            || path.starts_with("/snap/")
        {
            Scope::Machine
        } else {
            Scope::User
        }
    }

    /// Folders that hold program copies besides the PATH, searched for second copies.
    fn known_bins(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        dirs.extend(self.appdata.iter().map(|d| d.join("npm")));
        dirs.extend(self.local_bin());
        for (_, links, _) in self.winget() {
            dirs.push(links);
        }
        dirs.extend(self.bun.iter().map(|d| d.join("bin")));
        dirs.extend(self.cargo.iter().map(|d| d.join("bin")));
        dirs.extend(self.pnpm_home.iter().cloned());
        dirs.extend(self.scoop.iter().map(|(d, _)| d.join("shims")));
        dirs.extend(
            self.local_appdata
                .iter()
                .map(|d| d.join("Programs").join("OpenAI").join("Codex").join("bin")),
        );
        if !cfg!(windows) {
            dirs.extend(
                [
                    "/opt/homebrew/bin",
                    "/usr/local/bin",
                    "/home/linuxbrew/.linuxbrew/bin",
                ]
                .iter()
                .map(PathBuf::from),
            );
        }
        dirs
    }
}

// --- Detection ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Installer {
    Npm,
    Pnpm,
    Bun,
    /// winget portable package (`WinGet\Packages`, `WinGet\Links`).
    Winget,
    /// A setup program (MSI or EXE) registered under the Windows uninstall keys.
    Registered,
    Scoop,
    Choco,
    Uv,
    Pipx,
    Brew,
    BrewCask,
    Cargo,
    /// The vendor's own installer, which also brings the self-updater.
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    User,
    /// Installed for all users; updating it needs administrator rights.
    Machine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub installer: Installer,
    pub scope: Scope,
    /// Prefix or package folder: npm prefix, Homebrew prefix, winget package folder and so on.
    pub root: PathBuf,
    /// Package name the location itself names (winget folder, Cellar formula, uv tool folder),
    /// checked against the catalog so a look-alike package is never updated by mistake.
    pub package: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Known(Install),
    /// More than one installer claims the program (for example a uv tool and a pipx venv
    /// both linked into `~/.local/bin`).
    Ambiguous(Vec<Install>),
    /// A launcher of another program (the VS Code `copilot` shim), not an install.
    Foreign,
    Unknown,
}

/// Launchers that carry an agent's name but belong to another program; they count as not
/// installed.
pub fn is_foreign_launcher(path: &Path) -> bool {
    let path = key(path).to_lowercase();
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", &path));
    let in_dir = |tails: &[&str]| tails.iter().any(|tail| dir.ends_with(tail));
    let vs_code = in_dir(&["microsoft vs code/bin", "microsoft vs code insiders/bin"]);
    // `bash.exe` in the Windows folders starts WSL, not a bash of its own.
    let wsl_bash = (file == "bash.exe" || file == "bash")
        && in_dir(&[
            "windows/system32",
            "windows/sysnative",
            "microsoft/windowsapps",
        ]);
    path.split('/').any(|segment| segment == "copilot-shim") || vs_code || wsl_bash
}

/// `bash.exe` of Git for Windows (`<root>\bin\bash.exe` or `<root>\usr\bin\bash.exe` next to
/// `<root>\cmd\git.exe`): returns that `git.exe`, whose version is the one of the install.
pub fn git_for_windows(program: &Path, probe: &dyn Probe) -> Option<PathBuf> {
    if stem_lower(program) != "bash" {
        return None;
    }
    let bin = program.parent()?;
    if !bin
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|n| n.eq_ignore_ascii_case("bin"))
    {
        return None;
    }
    let mut roots = vec![bin.parent()?];
    let parent = bin.parent()?;
    if parent
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|n| n.eq_ignore_ascii_case("usr"))
    {
        roots.insert(0, parent.parent()?);
    }
    roots
        .into_iter()
        .map(|root| root.join("cmd").join("git.exe"))
        .find(|git| probe.is_file(git))
}

/// One rule of [`detect`]: the program path and its real path, the catalog packages, the probe
/// and the known folders.
type Matcher = fn(&[&Path], &Packages, &dyn Probe, &Locations) -> Option<Install>;

/// Which installer put `program` there. `packages` names the catalog's packages; a match by
/// package (npm, uv) needs it, a match by location (winget, Homebrew) checks it later.
pub fn detect(program: &Path, packages: &Packages, probe: &dyn Probe) -> Detection {
    if is_foreign_launcher(program) {
        return Detection::Foreign;
    }
    let loc = Locations::new(probe);
    let real = probe
        .canonical(program)
        .map(|p| PathBuf::from(key_path(&p)))
        .unwrap_or_else(|| program.to_path_buf());
    let paths = [program, real.as_path()];
    let matchers: [Matcher; 10] = [
        match_pnpm,
        match_npm,
        match_bun,
        match_winget,
        match_scoop,
        match_choco,
        match_uv,
        match_pipx,
        match_brew,
        match_cargo,
    ];
    let mut found: Vec<Install> = Vec::new();
    for matcher in matchers {
        if let Some(install) = matcher(&paths, packages, probe, &loc) {
            if found.iter().all(|f| f.installer != install.installer) {
                found.push(install);
            }
        }
    }
    // pnpm keeps its packages below its own home, which also looks like an npm prefix.
    if found.iter().any(|f| f.installer == Installer::Pnpm) {
        found.retain(|f| f.installer != Installer::Npm);
    }
    match found.len() {
        0 => match_native(&paths, &loc, probe)
            .or_else(|| match_registered(&paths, probe, &loc))
            .map_or(Detection::Unknown, Detection::Known),
        1 => Detection::Known(found.remove(0)),
        _ => Detection::Ambiguous(found),
    }
}

/// `\\?\C:\x` from `canonicalize` back to `C:\x`, so it compares and displays like PATH entries.
fn key_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

fn install(installer: Installer, scope: Scope, root: PathBuf, package: Option<String>) -> Install {
    Install {
        installer,
        scope,
        root,
        package,
    }
}

/// Root of a real path up to (not including) the segment `marker`, plus the segment after it.
fn split_at_segment(path: &Path, marker: &str) -> Option<(PathBuf, String)> {
    let parts: Vec<Component<'_>> = path.components().collect();
    let index = parts.iter().position(|c| {
        matches!(c, Component::Normal(name) if name.to_str().is_some_and(|n| n.eq_ignore_ascii_case(marker)))
    })?;
    let root: PathBuf = parts[..index].iter().collect();
    let next = match parts.get(index + 1)? {
        Component::Normal(name) => name.to_str()?.to_owned(),
        _ => return None,
    };
    Some((root, next))
}

fn match_npm(
    paths: &[&Path],
    pk: &Packages,
    probe: &dyn Probe,
    loc: &Locations,
) -> Option<Install> {
    for path in paths {
        let dir = path.parent()?;
        if let Some(name) = &pk.npm {
            // Windows layout: shims and node_modules share the prefix folder.
            if probe.is_dir(&dir.join("node_modules").join(name)) {
                return Some(install(
                    Installer::Npm,
                    loc.scope_of(dir),
                    dir.to_path_buf(),
                    None,
                ));
            }
            // Unix layout: <prefix>/bin/<command> and <prefix>/lib/node_modules/<package>.
            if dir.file_name().and_then(OsStr::to_str) == Some("bin") {
                if let Some(prefix) = dir.parent() {
                    if probe.is_dir(&prefix.join("lib").join("node_modules").join(name)) {
                        return Some(install(
                            Installer::Npm,
                            loc.scope_of(prefix),
                            prefix.to_path_buf(),
                            None,
                        ));
                    }
                }
            }
            // Real path inside the package: .../lib/node_modules/<package>/...
            let text = key(path);
            let marker = format!("/node_modules/{}/", key(Path::new(name)));
            if let Some(at) = text.find(&marker) {
                let prefix = text[..at].trim_end_matches("/lib");
                let prefix = PathBuf::from(prefix);
                return Some(install(Installer::Npm, loc.scope_of(&prefix), prefix, None));
            }
        }
    }
    None
}

fn match_pnpm(
    paths: &[&Path],
    pk: &Packages,
    probe: &dyn Probe,
    loc: &Locations,
) -> Option<Install> {
    let home = loc.pnpm_home.as_ref()?;
    for path in paths {
        let in_home = path.parent().is_some_and(|dir| same_dir(dir, home));
        let in_store = below(path, &home.join("global")).is_some();
        if in_home || in_store {
            // The package must be there, otherwise the shim belongs to something else.
            let has_package = pk.pnpm_name().is_some_and(|name| {
                probe.is_dir(
                    &home
                        .join("global")
                        .join("5")
                        .join("node_modules")
                        .join(name),
                ) || below(path, &home.join("global")).is_some_and(|rest| {
                    rest.contains(&format!("/node_modules/{}/", key(Path::new(name))))
                })
            });
            if has_package || pk.pnpm_name().is_none() {
                return Some(install(Installer::Pnpm, Scope::User, home.clone(), None));
            }
        }
    }
    None
}

fn match_bun(paths: &[&Path], _: &Packages, _: &dyn Probe, loc: &Locations) -> Option<Install> {
    let bun = loc.bun.as_ref()?;
    paths
        .iter()
        .any(|path| {
            path.parent()
                .is_some_and(|dir| same_dir(dir, &bun.join("bin")))
                || below(path, &bun.join("install").join("global")).is_some()
        })
        .then(|| install(Installer::Bun, Scope::User, bun.clone(), None))
}

fn match_winget(paths: &[&Path], _: &Packages, _: &dyn Probe, loc: &Locations) -> Option<Install> {
    for (packages, links, machine) in loc.winget() {
        let scope = if machine { Scope::Machine } else { Scope::User };
        for path in paths {
            if let Some(rest) = below(path, &packages) {
                let folder = first_segment(&rest);
                // `<Id>_<Source>`, such as `Anthropic.ClaudeCode_Microsoft.Winget.Source_8wekyb3d8bbwe`;
                // the comparison form is lower case on Windows, so the id is compared without case.
                let id = folder.split_once('_').map_or(folder, |(id, _)| id);
                return Some(install(
                    Installer::Winget,
                    scope,
                    packages.join(folder),
                    Some(id.to_owned()),
                ));
            }
            if below(path, &links).is_some() {
                return Some(install(Installer::Winget, scope, links.clone(), None));
            }
        }
    }
    None
}

fn match_scoop(paths: &[&Path], _: &Packages, _: &dyn Probe, loc: &Locations) -> Option<Install> {
    for (root, global) in &loc.scoop {
        let scope = if *global { Scope::Machine } else { Scope::User };
        for path in paths {
            if let Some(rest) = below(path, &root.join("apps")) {
                let app = first_segment(&rest).to_owned();
                return Some(install(Installer::Scoop, scope, root.clone(), Some(app)));
            }
            if below(path, &root.join("shims")).is_some() {
                return Some(install(Installer::Scoop, scope, root.clone(), None));
            }
        }
    }
    None
}

fn match_choco(paths: &[&Path], _: &Packages, _: &dyn Probe, loc: &Locations) -> Option<Install> {
    let root = loc.choco.as_ref()?;
    paths
        .iter()
        .any(|path| {
            below(path, &root.join("bin")).is_some() || below(path, &root.join("lib")).is_some()
        })
        .then(|| install(Installer::Choco, Scope::Machine, root.clone(), None))
}

fn match_uv(paths: &[&Path], pk: &Packages, probe: &dyn Probe, loc: &Locations) -> Option<Install> {
    let tools = loc.uv_tools.as_ref()?;
    for path in paths {
        if let Some(rest) = below(path, tools) {
            let tool = first_segment(&rest).to_owned();
            return Some(install(
                Installer::Uv,
                Scope::User,
                tools.join(&tool),
                Some(tool),
            ));
        }
    }
    // The shim in the bin folder: the receipt of the catalog's package must name this path.
    let name = pk.pypi.as_ref()?;
    let receipt = probe.read_text(
        &tools.join(name).join("uv-receipt.toml"),
        MAX_METADATA_BYTES,
    )?;
    let receipt = key(Path::new(&receipt.replace("\\\\", "/")));
    let named = paths.iter().any(|path| receipt.contains(&key(path)));
    named.then(|| {
        install(
            Installer::Uv,
            Scope::User,
            tools.join(name),
            Some(name.clone()),
        )
    })
}

fn match_pipx(
    paths: &[&Path],
    pk: &Packages,
    probe: &dyn Probe,
    loc: &Locations,
) -> Option<Install> {
    for home in &loc.pipx_homes {
        let venvs = home.join("venvs");
        for path in paths {
            if let Some(rest) = below(path, &venvs) {
                let venv = first_segment(&rest).to_owned();
                return Some(install(
                    Installer::Pipx,
                    Scope::User,
                    venvs.join(&venv),
                    Some(venv),
                ));
            }
        }
        let Some(name) = &pk.pypi else { continue };
        let metadata = venvs.join(name).join("pipx_metadata.json");
        let linked = loc.pipx_bin.as_ref().is_some_and(|bin| {
            paths
                .iter()
                .any(|p| p.parent().is_some_and(|dir| same_dir(dir, bin)))
        });
        if linked && probe.is_file(&metadata) {
            return Some(install(
                Installer::Pipx,
                Scope::User,
                venvs.join(name),
                Some(name.clone()),
            ));
        }
    }
    None
}

fn match_brew(paths: &[&Path], _: &Packages, _: &dyn Probe, loc: &Locations) -> Option<Install> {
    for path in paths {
        for (marker, installer) in [
            ("Cellar", Installer::Brew),
            ("Caskroom", Installer::BrewCask),
        ] {
            if let Some((root, package)) = split_at_segment(path, marker) {
                return Some(install(installer, loc.scope_of(&root), root, Some(package)));
            }
        }
    }
    None
}

fn match_cargo(
    paths: &[&Path],
    _: &Packages,
    probe: &dyn Probe,
    loc: &Locations,
) -> Option<Install> {
    let cargo = loc.cargo.as_ref()?;
    let path = paths.first()?;
    if !path
        .parent()
        .is_some_and(|dir| same_dir(dir, &cargo.join("bin")))
    {
        return None;
    }
    let list = probe.read_text(&cargo.join(".crates2.json"), MAX_METADATA_BYTES)?;
    let json: serde_json::Value = serde_json::from_str(&list).ok()?;
    let stem = stem_lower(path);
    let installs = json.get("installs")?.as_object()?;
    let (crate_id, _) = installs.iter().find(|(_, entry)| {
        entry
            .get("bins")
            .and_then(|b| b.as_array())
            .is_some_and(|bins| {
                bins.iter()
                    .filter_map(|b| b.as_str())
                    .any(|bin| stem_lower(Path::new(bin)) == stem)
            })
    })?;
    let name = crate_id.split(' ').next().map(str::to_owned);
    Some(install(Installer::Cargo, Scope::User, cargo.clone(), name))
}

/// Vendor installers that bring their own updater, recognized by their fixed folders.
fn match_native(paths: &[&Path], loc: &Locations, probe: &dyn Probe) -> Option<Install> {
    let program = paths.first()?;
    let stem = stem_lower(program);
    let dir = program.parent()?;
    let native = match stem.as_str() {
        // Native Claude Code: launcher in ~/.local/bin, versions in ~/.local/share/claude.
        "claude" => {
            loc.local_bin().is_some_and(|bin| same_dir(dir, &bin))
                && loc
                    .home
                    .as_ref()
                    .is_some_and(|h| probe.is_dir(&h.join(".local").join("share").join("claude")))
        }
        // Native Codex for Windows (measurement 2026-11-08).
        "codex" => loc.local_appdata.as_ref().is_some_and(|d| {
            below(program, &d.join("Programs").join("OpenAI").join("Codex")).is_some()
        }),
        _ => false,
    };
    native.then(|| install(Installer::Native, Scope::User, dir.to_path_buf(), None))
}

/// A setup program registered under the uninstall keys. MSI packages (PowerShell 7) often leave
/// `InstallLocation` empty; a program below Program Files then counts as such an install too,
/// because only setup programs write there. Whether winget knows it is winget's to decide.
fn match_registered(paths: &[&Path], probe: &dyn Probe, loc: &Locations) -> Option<Install> {
    if !cfg!(windows) {
        return None;
    }
    let by_entry = match_uninstall_entry(paths, probe, loc);
    by_entry.or_else(|| {
        loc.program_files.iter().find_map(|program_files| {
            let rest = paths.iter().find_map(|p| below(p, program_files))?;
            // At least `<vendor or product>\<file>`; a file directly in Program Files is no install.
            let (folder, _) = rest.split_once('/')?;
            Some(install(
                Installer::Registered,
                Scope::Machine,
                program_files.join(folder),
                None,
            ))
        })
    })
}

fn match_uninstall_entry(paths: &[&Path], probe: &dyn Probe, loc: &Locations) -> Option<Install> {
    let entries = probe.uninstall_entries();
    // Entries that claim a whole drive, the Windows folder or a shared root (measured: one with
    // `C:\Windows\System32\`) say nothing about a single program.
    let shared: Vec<String> = loc
        .program_files
        .iter()
        .chain(loc.program_data.iter())
        .chain(loc.home.iter())
        .map(|dir| key(dir))
        .collect();
    let specific = |location: &Path| {
        let location = key(location);
        let deep =
            location.trim_end_matches('/').matches('/').count() >= 1 && !location.ends_with(':');
        let in_windows = loc.system_root.as_ref().is_some_and(|root| {
            location == key(root) || below(Path::new(&location), root).is_some()
        });
        deep && !in_windows && !shared.contains(&location)
    };
    // The longest matching location wins, so `C:\Program Files\Git` beats `C:\Program Files`.
    let best = entries
        .iter()
        .filter(|e| specific(&e.install_location))
        .filter(|e| {
            paths
                .iter()
                .any(|p| below(p, &e.install_location).is_some())
        })
        .max_by_key(|e| key(&e.install_location).len())?;
    let scope = if best.machine || loc.scope_of(&best.install_location) == Scope::Machine {
        Scope::Machine
    } else {
        Scope::User
    };
    Some(install(
        Installer::Registered,
        scope,
        best.install_location.clone(),
        None,
    ))
}

// --- Copies on this machine ---

fn path_exts(probe: &dyn Probe) -> Vec<String> {
    if !cfg!(windows) {
        return vec![String::new()];
    }
    let pathext = probe
        .var("PATHEXT")
        .and_then(|v| v.into_string().ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PATHEXT.to_owned());
    pathext
        .split(';')
        .map(str::trim)
        .filter(|e| e.starts_with('.'))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Absolute PATH entries in order; relative and empty entries are skipped.
fn absolute_path_dirs(probe: &dyn Probe) -> Vec<PathBuf> {
    let Some(path) = probe.var("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .collect()
}

fn find_in(dir: &Path, name: &str, exts: &[String], probe: &dyn Probe) -> Option<PathBuf> {
    exts.iter().find_map(|ext| {
        let candidate = dir.join(format!("{name}{ext}"));
        probe.is_file(&candidate).then_some(candidate)
    })
}

/// An executable named `name` in the preferred folders first, then in absolute PATH entries.
pub fn find_tool(name: &str, preferred: &[PathBuf], probe: &dyn Probe) -> Option<PathBuf> {
    let exts = path_exts(probe);
    preferred
        .iter()
        .filter(|dir| dir.is_absolute())
        .cloned()
        .chain(absolute_path_dirs(probe))
        .find_map(|dir| find_in(&dir, name, &exts, probe))
}

/// Every copy of the program's commands on this machine: PATH order first, then known install
/// folders; foreign launchers and the same file twice are left out. The first one is the copy
/// a session starts.
pub fn copies(candidates: &[String], probe: &dyn Probe) -> Vec<PathBuf> {
    let loc = Locations::new(probe);
    let exts = path_exts(probe);
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for dir in absolute_path_dirs(probe)
        .into_iter()
        .chain(loc.known_bins())
    {
        for command in candidates {
            let Some(path) = find_in(&dir, command, &exts, probe) else {
                continue;
            };
            if is_foreign_launcher(&path) {
                continue;
            }
            let real = probe
                .canonical(&path)
                .map_or_else(|| key(&path), |p| key(&p));
            if seen.insert(real) && found.len() < MAX_COPIES {
                found.push(path);
            }
        }
    }
    found
}

/// The uv that runs `uv tool upgrade`. Only a uv the user installed for their own use counts:
/// one in the folders of uv's standalone installer (`UV_INSTALL_DIR`, `XDG_BIN_HOME`,
/// `~/.local/bin`) or of cargo, or one on PATH that a package manager installed as the package
/// `uv` (winget, Scoop, Homebrew, pipx, cargo). A uv bundled inside another application's
/// folder (measured 2026-11-08: first on PATH) is never run; its version and setup belong to
/// that application, and it may be older than the uv that wrote the tool's receipt.
fn find_uv(loc: &Locations, probe: &dyn Probe) -> Option<PathBuf> {
    let exts = path_exts(probe);
    let var = |name: &str| probe.var(name).map(PathBuf::from);
    let own = var("UV_INSTALL_DIR")
        .into_iter()
        .chain(var("XDG_BIN_HOME"))
        .chain(loc.local_bin())
        .chain(loc.cargo.iter().map(|c| c.join("bin")))
        .filter(|dir| dir.is_absolute())
        .find_map(|dir| find_in(&dir, "uv", &exts, probe));
    if own.is_some() {
        return own;
    }
    let uv = Packages {
        pypi: Some("uv".into()),
        brew: Some("uv".into()),
        winget: Some("astral-sh.uv".into()),
        scoop: Some("uv".into()),
        ..Packages::default()
    };
    let installed_as_uv = |candidate: &PathBuf| match detect(candidate, &uv, probe) {
        Detection::Known(install) => {
            matches!(
                install.installer,
                Installer::Winget
                    | Installer::Scoop
                    | Installer::Brew
                    | Installer::Pipx
                    | Installer::Cargo
            ) && install.package.as_deref().is_none_or(|p| {
                p.eq_ignore_ascii_case("uv") || p.eq_ignore_ascii_case("astral-sh.uv")
            })
        }
        _ => false,
    };
    absolute_path_dirs(probe)
        .into_iter()
        .filter_map(|dir| find_in(&dir, "uv", &exts, probe))
        .find(installed_as_uv)
}

// --- Plan ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanKind {
    /// `create_update_session` runs `command`.
    Run,
    /// `command` is shown for copying only.
    Copy,
    None,
}

/// Why there is no run plan. Stable codes; the frontend translates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// The profile's `update_hint` cannot be split into a plain argv.
    InvalidHint,
    NotInstalled,
    ForeignLauncher,
    Ambiguous,
    UnknownInstaller,
    /// The installer is known, but the catalog has no package name for it.
    NoPackage,
    /// The install folder names another package than the catalog.
    PackageMismatch,
    /// Hangar does not update this installer (bun, Chocolatey, cargo).
    Unsupported,
    /// npm, winget, uv, ... was not found in a known folder or an absolute PATH entry.
    ToolMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Warning {
    /// Another copy of the program exists; the update may hit one that is not started.
    SecondCopy,
    /// The program runs in a session; on Windows locked files can make the update fail.
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Run { program: String, args: Vec<String> },
    Copy(String),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    pub action: Action,
    pub reason: Option<Reason>,
    pub needs_admin: bool,
    pub installer: Option<Installer>,
    pub warnings: Vec<Warning>,
    /// All copies found, the started one first.
    pub copies: Vec<PathBuf>,
}

impl UpdatePlan {
    pub fn kind(&self) -> PlanKind {
        match self.action {
            Action::Run { .. } => PlanKind::Run,
            Action::Copy(_) => PlanKind::Copy,
            Action::None => PlanKind::None,
        }
    }

    /// Display text: the command that runs, or the text for copying.
    pub fn command(&self) -> Option<String> {
        match &self.action {
            Action::Run { program, args } => Some(display_command(program, args)),
            Action::Copy(text) => Some(text.clone()),
            Action::None => None,
        }
    }
}

/// Command line for display only (never run through a shell): parts with spaces in quotes.
pub fn display_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(|part| {
            if part.contains(' ') {
                format!("\"{part}\"")
            } else {
                part.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the plan is made from.
pub struct PlanInput<'a> {
    pub profile: &'a Profile,
    /// Install command of the catalog, offered for copying when nothing can run.
    pub install_hint: Option<&'a str>,
    /// The program a session starts; `None` if not installed.
    pub program: Option<&'a Path>,
    /// Command names of the entry, for the search of second copies.
    pub candidates: &'a [String],
}

fn run(program: &Path, args: Vec<String>) -> Action {
    Action::Run {
        program: program.to_string_lossy().into_owned(),
        args,
    }
}

/// Order: the profile's `update_hint` (an explicit override), then an installation-aware
/// self-updater, then the installer's command with the catalog's package name, else nothing to
/// run (copy text or none, with a reason).
pub fn plan(input: &PlanInput<'_>, probe: &dyn Probe) -> UpdatePlan {
    let profile = input.profile;
    let packages = profile.packages.clone().unwrap_or_default();
    let detection = input
        .program
        .map_or(Detection::Unknown, |p| detect(p, &packages, probe));
    let copies = if input.program.is_some() {
        copies(input.candidates, probe)
    } else {
        Vec::new()
    };
    let mut warnings = Vec::new();
    if copies.len() > 1 {
        warnings.push(Warning::SecondCopy);
    }
    let (installer, needs_admin) = match &detection {
        Detection::Known(i) => (Some(i.installer), i.scope == Scope::Machine),
        _ => (None, false),
    };
    let copy_install = || {
        input
            .install_hint
            .map_or(Action::None, |hint| Action::Copy(hint.to_owned()))
    };
    let (action, reason) = 'decide: {
        if let Some(hint) = profile
            .update_hint
            .as_ref()
            .and_then(PlatformHint::for_platform)
        {
            break 'decide match split_command(hint) {
                Ok(mut argv) => {
                    let program = argv.remove(0);
                    (
                        Action::Run {
                            program,
                            args: argv,
                        },
                        None,
                    )
                }
                Err(_) => (Action::Copy(hint.clone()), Some(Reason::InvalidHint)),
            };
        }
        let Some(program) = input.program else {
            break 'decide (copy_install(), Some(Reason::NotInstalled));
        };
        if detection == Detection::Foreign {
            break 'decide (copy_install(), Some(Reason::ForeignLauncher));
        }
        let self_update = profile
            .self_update
            .as_ref()
            .filter(|s| s.applies_here() && s.validate().is_ok());
        if let Some(update) = self_update {
            let native =
                matches!(&detection, Detection::Known(i) if i.installer == Installer::Native);
            if update.aware || native {
                break 'decide (run(program, update.args.clone()), None);
            }
        }
        match &detection {
            Detection::Known(install) => match template(install, &packages, probe) {
                Ok(action) => (action, None),
                Err((reason, Some(text))) => (Action::Copy(text), Some(reason)),
                Err((reason, None)) => (copy_install(), Some(reason)),
            },
            Detection::Ambiguous(_) => (copy_install(), Some(Reason::Ambiguous)),
            Detection::Foreign | Detection::Unknown => {
                (copy_install(), Some(Reason::UnknownInstaller))
            }
        }
    };
    UpdatePlan {
        action,
        reason,
        needs_admin,
        installer,
        warnings,
        copies,
    }
}

type TemplateError = (Reason, Option<String>);

/// The installer's update command for the catalog package. On a missing tool, the same
/// command with the bare tool name comes back as copy text.
fn template(install: &Install, pk: &Packages, probe: &dyn Probe) -> Result<Action, TemplateError> {
    let loc = Locations::new(probe);
    let need = |name: &Option<String>, ok: fn(&str) -> bool| -> Result<String, TemplateError> {
        match name {
            Some(name) if ok(name) => Ok(name.clone()),
            _ => Err((Reason::NoPackage, None)),
        }
    };
    let matches_location = |name: &str| {
        install
            .package
            .as_deref()
            .is_none_or(|found| found.eq_ignore_ascii_case(name))
    };
    let tool = |name: &str, preferred: &[PathBuf], args: &[String]| match find_tool(
        name, preferred, probe,
    ) {
        Some(path) => Ok(run(&path, args.to_vec())),
        None => Err((Reason::ToolMissing, Some(display_command(name, args)))),
    };
    let winget = |id: &str| {
        let args: Vec<String> = ["upgrade", "--id", id, "--exact", "--source", "winget"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let preferred: Vec<PathBuf> = loc.windows_apps().into_iter().collect();
        tool("winget", &preferred, &args)
    };
    match install.installer {
        Installer::Npm => {
            let name = need(&pk.npm, npm_name_ok)?;
            let prefix = install.root.to_string_lossy().into_owned();
            // `--prefix` keeps the update on the prefix that holds the started copy, even if the
            // npm found belongs to another Node.js installation.
            let args = vec![
                "install".to_owned(),
                "-g".to_owned(),
                "--prefix".to_owned(),
                prefix,
                format!("{name}@latest"),
            ];
            let preferred = [install.root.clone(), install.root.join("bin")];
            tool("npm", &preferred, &args)
        }
        Installer::Pnpm => {
            let name = pk
                .pnpm_name()
                .filter(|n| npm_name_ok(n))
                .ok_or((Reason::NoPackage, None))?;
            let args = vec!["add".to_owned(), "-g".to_owned(), format!("{name}@latest")];
            tool("pnpm", std::slice::from_ref(&install.root), &args)
        }
        Installer::Winget => {
            let id = need(&pk.winget, winget_id_ok)?;
            if !matches_location(&id) {
                return Err((Reason::PackageMismatch, None));
            }
            winget(&id)
        }
        Installer::Registered => {
            let id = need(&pk.winget, winget_id_ok)?;
            winget(&id)
        }
        Installer::Scoop => {
            let app = need(&pk.scoop, scoop_name_ok)?;
            if !matches_location(&app) {
                return Err((Reason::PackageMismatch, None));
            }
            tool(
                "scoop",
                &[install.root.join("shims")],
                &["update".to_owned(), app],
            )
        }
        Installer::Uv => {
            let name = need(&pk.pypi, plain_name_ok)?;
            if !matches_location(&name) {
                return Err((Reason::PackageMismatch, None));
            }
            let args = vec!["tool".to_owned(), "upgrade".to_owned(), name];
            match find_uv(&loc, probe) {
                Some(uv) => Ok(run(&uv, args)),
                None => Err((Reason::ToolMissing, Some(display_command("uv", &args)))),
            }
        }
        Installer::Pipx => {
            let name = need(&pk.pypi, plain_name_ok)?;
            if !matches_location(&name) {
                return Err((Reason::PackageMismatch, None));
            }
            let preferred: Vec<PathBuf> = loc.pipx_bin.iter().cloned().collect();
            tool("pipx", &preferred, &["upgrade".to_owned(), name])
        }
        Installer::Brew | Installer::BrewCask => {
            let cask = install.installer == Installer::BrewCask;
            let name = if cask {
                need(&pk.cask, brew_name_ok)?
            } else {
                need(&pk.brew, brew_name_ok)?
            };
            if !matches_location(&name) {
                return Err((Reason::PackageMismatch, None));
            }
            let mut args = vec!["upgrade".to_owned()];
            if cask {
                args.push("--cask".to_owned());
            }
            args.push(name);
            // The brew of the prefix that holds the Cellar, never another one from PATH.
            let brew = install.root.join("bin").join("brew");
            if probe.is_file(&brew) {
                Ok(run(&brew, args))
            } else {
                Err((Reason::ToolMissing, Some(display_command("brew", &args))))
            }
        }
        Installer::Bun | Installer::Choco | Installer::Cargo | Installer::Native => {
            Err((Reason::Unsupported, None))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use super::*;
    use crate::test_support::TempDir;

    /// Real files in a synthetic tree, a fixed environment and fake uninstall entries.
    struct TreeProbe {
        vars: HashMap<String, OsString>,
        uninstall: Vec<UninstallEntry>,
    }

    impl TreeProbe {
        fn new(root: &Path) -> Self {
            let mut vars = HashMap::new();
            let mut set = |name: &str, rest: &str| {
                vars.insert(name.to_owned(), root.join(rest).into_os_string());
            };
            set("HOME", "home");
            set("USERPROFILE", "home");
            set("APPDATA", "home/AppData/Roaming");
            set("LOCALAPPDATA", "home/AppData/Local");
            set("ProgramFiles", "Program Files");
            set("ProgramData", "ProgramData");
            set("XDG_DATA_HOME", "home/.local/share");
            vars.insert("PATHEXT".into(), ".COM;.EXE;.BAT;.CMD".into());
            vars.insert("PATH".into(), OsString::new());
            Self {
                vars,
                uninstall: Vec::new(),
            }
        }

        fn path(mut self, dirs: &[PathBuf]) -> Self {
            let mut all: Vec<PathBuf> = dirs.to_vec();
            // A relative entry is never used for tools.
            all.push(PathBuf::from("relative-bin"));
            self.vars
                .insert("PATH".into(), std::env::join_paths(all).unwrap());
            self
        }
    }

    impl Probe for TreeProbe {
        fn is_file(&self, path: &Path) -> bool {
            path.is_file()
        }
        fn is_dir(&self, path: &Path) -> bool {
            path.is_dir()
        }
        fn read_text(&self, path: &Path, max: usize) -> Option<String> {
            read_text_limited(path, max)
        }
        fn canonical(&self, path: &Path) -> Option<PathBuf> {
            fs::canonicalize(path).ok()
        }
        fn var(&self, name: &str) -> Option<OsString> {
            self.vars.get(name).cloned()
        }
        fn uninstall_entries(&self) -> Vec<UninstallEntry> {
            self.uninstall.clone()
        }
    }

    /// Program file name as PATH lookup finds it on this platform.
    fn exe(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        }
    }

    fn touch(path: &Path) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
        path.to_path_buf()
    }

    fn packages(toml_src: &str) -> Packages {
        #[derive(Deserialize)]
        struct Wrap {
            packages: Packages,
        }
        toml::from_str::<Wrap>(toml_src).unwrap().packages
    }

    fn profile_with(packages: Option<Packages>, self_update: Option<SelfUpdate>) -> Profile {
        let mut profile = crate::profile::parse("[[profile]]\nid='x'\nname='X'\ncommand='x'\n")
            .unwrap()
            .remove(0);
        profile.packages = packages;
        profile.self_update = self_update;
        profile
    }

    fn plan_for(profile: &Profile, program: &Path, probe: &TreeProbe) -> UpdatePlan {
        let candidates = [stem_lower(program)];
        plan(
            &PlanInput {
                profile,
                install_hint: Some("vendor install"),
                program: Some(program),
                candidates: &candidates,
            },
            probe,
        )
    }

    fn known(detection: Detection) -> Install {
        match detection {
            Detection::Known(install) => install,
            other => panic!("expected Known, got {other:?}"),
        }
    }

    #[test]
    fn package_names_are_validated() {
        let ok = packages(
            r#"packages = { npm = "@openai/codex", winget = "OpenAI.Codex", pypi = "aider-chat",
               brew = "gemini-cli", cask = "claude-code", scoop = "pwsh" }"#,
        );
        assert!(ok.validate().is_ok());
        for bad in [
            r#"packages = { npm = "../x" }"#,
            r#"packages = { npm = "Upper" }"#,
            r#"packages = { winget = "NoDot" }"#,
            r#"packages = { winget = "A.B; rm" }"#,
            r#"packages = { winget = "A.B --override x" }"#,
            r#"packages = { pypi = "a/b" }"#,
            r#"packages = { brew = "Upper" }"#,
            r#"packages = { brew = "tap/formula" }"#,
            r#"packages = { cask = "-x" }"#,
            r#"packages = { scoop = "a b" }"#,
        ] {
            assert!(packages(bad).validate().is_err(), "{bad}");
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Wrap {
            packages: Packages,
        }
        assert!(toml::from_str::<Wrap>(r#"packages = { apt = "x" }"#).is_err());

        let update = |src: &str| {
            #[derive(Deserialize)]
            struct Wrap {
                self_update: SelfUpdate,
            }
            toml::from_str::<Wrap>(src).unwrap().self_update
        };
        assert!(
            update(r#"self_update = { args = ["update"], aware = true }"#)
                .validate()
                .is_ok()
        );
        for bad in [
            r#"self_update = { args = [] }"#,
            r#"self_update = { args = ["up date"] }"#,
            r#"self_update = { args = ["update;x"] }"#,
            r#"self_update = { args = [""] }"#,
        ] {
            assert!(update(bad).validate().is_err(), "{bad}");
        }
        let only = update(r#"self_update = { args = ["upgrade"], only = "windows" }"#);
        assert_eq!(only.applies_here(), cfg!(windows));
    }

    #[test]
    fn npm_prefix_with_package_runs_npm_of_that_prefix() {
        let t = TempDir::new("source-npm");
        let prefix = t.path().join("home/AppData/Roaming/npm");
        let shim = if cfg!(windows) {
            touch(&prefix.join("codex.cmd"))
        } else {
            touch(&prefix.join("bin/codex"))
        };
        let npm = if cfg!(windows) {
            touch(&prefix.join("npm.cmd"))
        } else {
            touch(&prefix.join("bin/npm"))
        };
        let modules = if cfg!(windows) {
            prefix.join("node_modules")
        } else {
            prefix.join("lib/node_modules")
        };
        fs::create_dir_all(modules.join("@openai/codex")).unwrap();
        let probe = TreeProbe::new(t.path()).path(&[shim.parent().unwrap().to_path_buf()]);
        let pk = packages(r#"packages = { npm = "@openai/codex" }"#);
        let found = known(detect(&shim, &pk, &probe));
        assert_eq!(found.installer, Installer::Npm);
        assert_eq!(found.scope, Scope::User);
        assert_eq!(key(&found.root), key(&prefix));

        let plan = plan_for(&profile_with(Some(pk), None), &shim, &probe);
        assert_eq!(plan.kind(), PlanKind::Run);
        assert!(!plan.needs_admin);
        assert_eq!(plan.installer, Some(Installer::Npm));
        let Action::Run { program, args } = &plan.action else {
            panic!("{plan:?}")
        };
        assert_eq!(key(Path::new(program)), key(&npm));
        assert!(Path::new(program).is_absolute());
        assert_eq!(
            args,
            &[
                "install".to_owned(),
                "-g".into(),
                "--prefix".into(),
                prefix.to_string_lossy().into_owned(),
                "@openai/codex@latest".into()
            ]
        );
        // Without the package next to the shim, the npm folder alone decides nothing.
        let other = packages(r#"packages = { npm = "@google/gemini-cli" }"#);
        assert_eq!(detect(&shim, &other, &probe), Detection::Unknown);
    }

    #[test]
    fn npm_missing_falls_back_to_copy_text() {
        let t = TempDir::new("source-npm-missing");
        let prefix = t.path().join("prefix");
        let shim = touch(&prefix.join(exe("gemini")));
        fs::create_dir_all(prefix.join("node_modules/@google/gemini-cli")).unwrap();
        fs::create_dir_all(prefix.join("lib/node_modules/@google/gemini-cli")).unwrap();
        // An npm only reachable through a relative PATH entry does not count.
        let probe = TreeProbe::new(t.path()).path(&[]);
        let pk = packages(r#"packages = { npm = "@google/gemini-cli" }"#);
        let plan = plan_for(&profile_with(Some(pk), None), &shim, &probe);
        assert_eq!(plan.kind(), PlanKind::Copy);
        assert_eq!(plan.reason, Some(Reason::ToolMissing));
        let text = plan.command().unwrap();
        assert!(text.starts_with("npm install -g --prefix "), "{text}");
        assert!(text.ends_with("@google/gemini-cli@latest"), "{text}");
    }

    #[test]
    fn winget_portable_and_mismatch() {
        let t = TempDir::new("source-winget");
        let packages_dir = t
            .path()
            .join("home/AppData/Local/Microsoft/WinGet/Packages");
        let program = touch(
            &packages_dir
                .join("Anthropic.ClaudeCode_Microsoft.Winget.Source_8wekyb3d8bbwe")
                .join(exe("claude")),
        );
        let winget = touch(
            &t.path()
                .join("home/AppData/Local/Microsoft/WindowsApps")
                .join(exe("winget")),
        );
        let probe = TreeProbe::new(t.path());
        let found = known(detect(&program, &Packages::default(), &probe));
        assert_eq!(found.installer, Installer::Winget);
        assert!(found
            .package
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("Anthropic.ClaudeCode")));

        let pk = packages(r#"packages = { winget = "Anthropic.ClaudeCode" }"#);
        let plan = plan_for(&profile_with(Some(pk), None), &program, &probe);
        let Action::Run {
            program: tool,
            args,
        } = &plan.action
        else {
            panic!("{plan:?}")
        };
        assert_eq!(key(Path::new(tool)), key(&winget));
        assert_eq!(
            args,
            &[
                "upgrade",
                "--id",
                "Anthropic.ClaudeCode",
                "--exact",
                "--source",
                "winget"
            ]
        );
        // A catalog id that is not the installed package: never update the look-alike.
        let wrong = packages(r#"packages = { winget = "Pressly.Goose" }"#);
        let plan = plan_for(&profile_with(Some(wrong), None), &program, &probe);
        assert_eq!(plan.reason, Some(Reason::PackageMismatch));
        assert_eq!(plan.kind(), PlanKind::Copy);
        assert_eq!(plan.command().as_deref(), Some("vendor install"));
        // No winget id in the catalog.
        let plan = plan_for(&profile_with(None, None), &program, &probe);
        assert_eq!(plan.reason, Some(Reason::NoPackage));
    }

    #[test]
    fn machine_scope_needs_admin() {
        let t = TempDir::new("source-machine");
        let root = t.path().join("Program Files/PowerShell/7");
        let program = touch(&root.join(exe("pwsh")));
        touch(
            &t.path()
                .join("home/AppData/Local/Microsoft/WindowsApps")
                .join(exe("winget")),
        );
        let mut probe = TreeProbe::new(t.path());
        probe.uninstall.push(UninstallEntry {
            display_name: "PowerShell 7-x64".into(),
            install_location: root.clone(),
            machine: true,
        });
        let pk = packages(r#"packages = { winget = "Microsoft.PowerShell" }"#);
        let plan = plan_for(&profile_with(Some(pk.clone()), None), &program, &probe);
        if cfg!(windows) {
            assert_eq!(plan.installer, Some(Installer::Registered));
            assert_eq!(plan.kind(), PlanKind::Run);
            assert!(plan.needs_admin);
            assert!(plan.command().unwrap().contains("Microsoft.PowerShell"));
            // MSI entries often have no InstallLocation (measured for PowerShell 7.6.9): the
            // folder below Program Files decides then.
            probe.uninstall.clear();
            let found = known(detect(&program, &pk, &probe));
            assert_eq!(
                (found.installer, found.scope),
                (Installer::Registered, Scope::Machine)
            );
            assert_eq!(
                key(&found.root),
                key(&t.path().join("Program Files/PowerShell"))
            );
            // A file directly in Program Files is no install.
            let loose = touch(&t.path().join("Program Files").join(exe("loose")));
            assert_eq!(detect(&loose, &pk, &probe), Detection::Unknown);
        } else {
            // Uninstall keys only exist on Windows.
            assert_eq!(plan.reason, Some(Reason::UnknownInstaller));
        }
        // Machine-wide npm prefix: the update needs administrator rights as well.
        let prefix = if cfg!(windows) {
            t.path().join("Program Files/nodejs")
        } else {
            PathBuf::from("/usr/lib/node")
        };
        let probe = TreeProbe::new(t.path());
        let loc = Locations::new(&probe);
        assert_eq!(loc.scope_of(&prefix.join("x")), Scope::Machine);
        assert_eq!(
            loc.scope_of(&t.path().join("home/AppData/Roaming/npm/x")),
            Scope::User
        );
    }

    #[cfg(windows)]
    #[test]
    fn broad_uninstall_locations_are_ignored() {
        let t = TempDir::new("source-broad");
        let windows = t.path().join("Windows");
        let program = touch(&windows.join("System32").join("cmd.exe"));
        let mut probe = TreeProbe::new(t.path());
        probe
            .vars
            .insert("SystemRoot".into(), windows.clone().into_os_string());
        for location in [
            windows.join("System32"),
            windows.clone(),
            t.path().join("Program Files"),
            PathBuf::from(r"C:\"),
        ] {
            probe.uninstall.push(UninstallEntry {
                display_name: String::new(),
                install_location: location,
                machine: true,
            });
        }
        assert_eq!(
            detect(&program, &Packages::default(), &probe),
            Detection::Unknown
        );
        // A specific folder still counts.
        let tool = touch(&t.path().join("Tools/Vendor").join(exe("tool")));
        probe.uninstall.push(UninstallEntry {
            display_name: "Tool".into(),
            install_location: t.path().join("Tools/Vendor"),
            machine: false,
        });
        let found = known(detect(&tool, &Packages::default(), &probe));
        assert_eq!(
            (found.installer, found.scope),
            (Installer::Registered, Scope::User)
        );
    }

    #[test]
    fn uv_receipt_must_name_the_path() {
        let t = TempDir::new("source-uv");
        let bin = t.path().join("home/.local/bin");
        let program = touch(&bin.join(exe("aider")));
        let tools = if cfg!(windows) {
            t.path().join("home/AppData/Roaming/uv/tools")
        } else {
            t.path().join("home/.local/share/uv/tools")
        };
        let receipt = tools.join("aider-chat/uv-receipt.toml");
        let uv = touch(&bin.join(exe("uv")));
        let pk = packages(r#"packages = { pypi = "aider-chat" }"#);
        let probe = TreeProbe::new(t.path());

        // A receipt for another path: the bin folder alone decides nothing.
        fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        fs::write(
            &receipt,
            "[tool]\nentrypoints = [{ install-path = \"/elsewhere/aider\" }]\n",
        )
        .unwrap();
        assert_eq!(detect(&program, &pk, &probe), Detection::Unknown);

        let named = program.to_string_lossy().replace('\\', "/");
        fs::write(
            &receipt,
            format!("[tool]\nentrypoints = [{{ name = \"aider\", install-path = \"{named}\" }}]\n"),
        )
        .unwrap();
        let found = known(detect(&program, &pk, &probe));
        assert_eq!(found.installer, Installer::Uv);
        let plan = plan_for(&profile_with(Some(pk), None), &program, &probe);
        let Action::Run {
            program: tool,
            args,
        } = &plan.action
        else {
            panic!("{plan:?}")
        };
        assert_eq!(key(Path::new(tool)), key(&uv));
        assert_eq!(args, &["tool", "upgrade", "aider-chat"]);
    }

    #[test]
    fn uv_bundled_by_another_app_is_never_run() {
        let t = TempDir::new("source-uv-owner");
        let bin = t.path().join("home/.local/bin");
        let program = touch(&bin.join(exe("aider")));
        let tools = if cfg!(windows) {
            t.path().join("home/AppData/Roaming/uv/tools")
        } else {
            t.path().join("home/.local/share/uv/tools")
        };
        let named = program.to_string_lossy().replace('\\', "/");
        let receipt = tools.join("aider-chat/uv-receipt.toml");
        fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        fs::write(&receipt, format!("install-path = \"{named}\"\n")).unwrap();
        // Measured layout: the only uv on PATH lives in another application's folder.
        let bundled = t.path().join("home/AppData/Local/other-app/bin");
        touch(&bundled.join(exe("uv")));
        let pk = packages(r#"packages = { pypi = "aider-chat" }"#);
        let probe = TreeProbe::new(t.path()).path(std::slice::from_ref(&bundled));
        let plan = plan_for(&profile_with(Some(pk.clone()), None), &program, &probe);
        assert_eq!(plan.installer, Some(Installer::Uv));
        assert_eq!(plan.reason, Some(Reason::ToolMissing), "{plan:?}");
        assert_eq!(
            plan.command().as_deref(),
            Some("uv tool upgrade aider-chat")
        );

        // A uv that winget installed as the package uv counts, even behind the bundled one.
        let links = t.path().join("home/AppData/Local/Microsoft/WinGet/Links");
        let winget_uv = touch(&links.join(exe("uv")));
        let probe = TreeProbe::new(t.path()).path(&[bundled, links]);
        let plan = plan_for(&profile_with(Some(pk), None), &program, &probe);
        let Action::Run { program: tool, .. } = &plan.action else {
            panic!("{plan:?}")
        };
        assert_eq!(key(Path::new(tool)), key(&winget_uv));
    }

    #[test]
    fn uv_and_pipx_together_are_ambiguous() {
        let t = TempDir::new("source-ambiguous");
        let bin = t.path().join("home/.local/bin");
        let program = touch(&bin.join(exe("vibe")));
        let tools = if cfg!(windows) {
            t.path().join("home/AppData/Roaming/uv/tools")
        } else {
            t.path().join("home/.local/share/uv/tools")
        };
        let named = program.to_string_lossy().replace('\\', "/");
        let receipt = tools.join("mistral-vibe/uv-receipt.toml");
        fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        fs::write(&receipt, format!("install-path = \"{named}\"\n")).unwrap();
        touch(
            &t.path()
                .join("home/.local/pipx/venvs/mistral-vibe/pipx_metadata.json"),
        );
        let pk = packages(r#"packages = { pypi = "mistral-vibe" }"#);
        let probe = TreeProbe::new(t.path());
        match detect(&program, &pk, &probe) {
            Detection::Ambiguous(found) => {
                let kinds: Vec<Installer> = found.iter().map(|f| f.installer).collect();
                assert_eq!(kinds, [Installer::Uv, Installer::Pipx]);
            }
            other => panic!("{other:?}"),
        }
        let plan = plan_for(&profile_with(Some(pk), None), &program, &probe);
        assert_eq!(plan.reason, Some(Reason::Ambiguous));
        assert_eq!(plan.kind(), PlanKind::Copy);
    }

    #[test]
    fn pipx_venv_and_scoop_and_choco_and_bun() {
        let t = TempDir::new("source-misc");
        let probe = TreeProbe::new(t.path());
        let pk = packages(r#"packages = { pypi = "aider-chat", scoop = "aider" }"#);

        let venv = touch(
            &t.path()
                .join("home/.local/pipx/venvs/aider-chat/bin")
                .join(exe("aider")),
        );
        let found = known(detect(&venv, &pk, &probe));
        assert_eq!(found.installer, Installer::Pipx);
        assert_eq!(found.package.as_deref(), Some("aider-chat"));

        let scoop = touch(
            &t.path()
                .join("home/scoop/apps/aider/current")
                .join(exe("aider")),
        );
        let found = known(detect(&scoop, &pk, &probe));
        assert_eq!(found.installer, Installer::Scoop);
        touch(&t.path().join("home/scoop/shims").join(if cfg!(windows) {
            "scoop.cmd"
        } else {
            "scoop"
        }));
        let plan = plan_for(&profile_with(Some(pk.clone()), None), &scoop, &probe);
        assert_eq!(plan.kind(), PlanKind::Run, "{plan:?}");
        assert!(plan.command().unwrap().ends_with("update aider"));

        let choco = touch(
            &t.path()
                .join("ProgramData/chocolatey/bin")
                .join(exe("aider")),
        );
        let found = known(detect(&choco, &pk, &probe));
        assert_eq!(
            (found.installer, found.scope),
            (Installer::Choco, Scope::Machine)
        );
        let plan = plan_for(&profile_with(Some(pk.clone()), None), &choco, &probe);
        assert_eq!(plan.reason, Some(Reason::Unsupported));
        assert!(plan.needs_admin);

        let bun = touch(&t.path().join("home/.bun/bin").join(exe("aider")));
        assert_eq!(known(detect(&bun, &pk, &probe)).installer, Installer::Bun);
    }

    #[test]
    fn homebrew_cellar_and_caskroom() {
        let t = TempDir::new("source-brew");
        let prefix = t.path().join("homebrew");
        let probe = TreeProbe::new(t.path());
        let formula = touch(
            &prefix
                .join("Cellar/gemini-cli/0.9.0/bin")
                .join(exe("gemini")),
        );
        let cask = touch(&prefix.join("Caskroom/codex/0.167.0").join(exe("codex")));
        let pk = packages(r#"packages = { brew = "gemini-cli", cask = "codex" }"#);
        let found = known(detect(&formula, &pk, &probe));
        assert_eq!(found.installer, Installer::Brew);
        assert_eq!(key(&found.root), key(&prefix));
        assert_eq!(found.package.as_deref(), Some("gemini-cli"));
        assert_eq!(
            known(detect(&cask, &pk, &probe)).installer,
            Installer::BrewCask
        );

        // Without the prefix's own brew there is nothing to run.
        let plan = plan_for(&profile_with(Some(pk.clone()), None), &formula, &probe);
        assert_eq!(plan.reason, Some(Reason::ToolMissing));
        assert_eq!(plan.command().as_deref(), Some("brew upgrade gemini-cli"));
        let brew = touch(&prefix.join("bin/brew"));
        let plan = plan_for(&profile_with(Some(pk.clone()), None), &cask, &probe);
        let Action::Run { program, args } = &plan.action else {
            panic!("{plan:?}")
        };
        assert_eq!(key(Path::new(program)), key(&brew));
        assert_eq!(args, &["upgrade", "--cask", "codex"]);
        // A Cellar formula other than the catalog's.
        let other = packages(r#"packages = { brew = "gemini" }"#);
        let plan = plan_for(&profile_with(Some(other), None), &formula, &probe);
        assert_eq!(plan.reason, Some(Reason::PackageMismatch));
    }

    #[test]
    fn cargo_crates_list() {
        let t = TempDir::new("source-cargo");
        let program = touch(&t.path().join("home/.cargo/bin").join(exe("forge")));
        fs::write(
            t.path().join("home/.cargo/.crates2.json"),
            r#"{"installs":{"forge 0.1.0 (registry+https://github.com/rust-lang/crates.io-index)":{"bins":["forge.exe","forge"]}}}"#,
        )
        .unwrap();
        let probe = TreeProbe::new(t.path());
        let found = known(detect(&program, &Packages::default(), &probe));
        assert_eq!(found.installer, Installer::Cargo);
        assert_eq!(found.package.as_deref(), Some("forge"));
        let plan = plan_for(&profile_with(None, None), &program, &probe);
        assert_eq!(plan.reason, Some(Reason::Unsupported));
    }

    #[test]
    fn foreign_launcher_is_not_an_install() {
        let t = TempDir::new("source-foreign");
        let shim = touch(
            &t.path()
                .join("home/AppData/Local/Programs/Microsoft VS Code/bin/copilot-shim")
                .join(exe("copilot")),
        );
        let probe = TreeProbe::new(t.path()).path(&[shim.parent().unwrap().to_path_buf()]);
        assert!(is_foreign_launcher(&shim));
        assert!(is_foreign_launcher(Path::new(
            "/Applications/Microsoft VS Code/bin/code"
        )));
        // bash.exe in the Windows folders starts WSL; Git Bash is a real one.
        for wsl in [
            r"C:\Windows\System32\bash.exe",
            r"C:\Users\X\AppData\Local\Microsoft\WindowsApps\bash.exe",
            "c:/windows/sysnative/BASH.EXE",
        ] {
            assert!(is_foreign_launcher(Path::new(wsl)), "{wsl}");
        }
        assert!(!is_foreign_launcher(Path::new(
            r"C:\Program Files\Git\usr\bin\bash.exe"
        )));
        assert_eq!(
            detect(&shim, &Packages::default(), &probe),
            Detection::Foreign
        );
        let plan = plan_for(&profile_with(None, None), &shim, &probe);
        assert_eq!(plan.reason, Some(Reason::ForeignLauncher));
        // The shim is no copy either.
        assert!(copies(&["copilot".to_owned()], &probe).is_empty());
        assert!(!is_foreign_launcher(
            &t.path().join("home/.local/bin").join(exe("copilot"))
        ));
    }

    #[test]
    fn self_update_order_and_override() {
        let t = TempDir::new("source-self");
        let bin = t.path().join("home/.local/bin");
        let program = touch(&bin.join(exe("claude")));
        let probe = TreeProbe::new(t.path());
        let aware = SelfUpdate {
            args: vec!["update".into()],
            aware: true,
            only: None,
        };
        // Aware self-updater wins over an unknown installer.
        let plan = plan_for(&profile_with(None, Some(aware.clone())), &program, &probe);
        assert_eq!(
            plan.action,
            Action::Run {
                program: program.to_string_lossy().into_owned(),
                args: vec!["update".into()]
            }
        );
        // Not aware: only for the native install.
        let native_only = SelfUpdate {
            aware: false,
            ..aware.clone()
        };
        let plan = plan_for(
            &profile_with(None, Some(native_only.clone())),
            &program,
            &probe,
        );
        assert_eq!(plan.reason, Some(Reason::UnknownInstaller));
        fs::create_dir_all(t.path().join("home/.local/share/claude/versions")).unwrap();
        let plan = plan_for(&profile_with(None, Some(native_only)), &program, &probe);
        assert_eq!(plan.installer, Some(Installer::Native));
        assert_eq!(plan.kind(), PlanKind::Run);

        // The profile's update_hint is an override and wins over everything.
        let mut profile = profile_with(None, Some(aware));
        profile.update_hint = Some(PlatformHint {
            windows: Some("winget upgrade Anthropic.ClaudeCode".into()),
            unix: Some("brew upgrade claude-code".into()),
        });
        let plan = plan_for(&profile, &program, &probe);
        let Action::Run { program: tool, .. } = &plan.action else {
            panic!("{plan:?}")
        };
        assert!(tool == "winget" || tool == "brew");
        profile.update_hint = Some(PlatformHint {
            windows: Some("irm x | iex".into()),
            unix: Some("curl x | sh".into()),
        });
        let plan = plan_for(&profile, &program, &probe);
        assert_eq!(plan.reason, Some(Reason::InvalidHint));
        assert_eq!(plan.kind(), PlanKind::Copy);
    }

    #[test]
    fn not_installed_offers_the_install_command() {
        let t = TempDir::new("source-none");
        let probe = TreeProbe::new(t.path());
        let profile = profile_with(None, None);
        let plan = plan(
            &PlanInput {
                profile: &profile,
                install_hint: None,
                program: None,
                candidates: &[],
            },
            &probe,
        );
        assert_eq!(plan.kind(), PlanKind::None);
        assert_eq!(plan.reason, Some(Reason::NotInstalled));
        assert_eq!(plan.command(), None);
    }

    #[test]
    fn second_copy_is_a_warning() {
        let t = TempDir::new("source-copies");
        let first = t.path().join("first");
        let second = t.path().join("home/.local/bin");
        let a = touch(&first.join(exe("codex")));
        let b = touch(&second.join(exe("codex")));
        // Not a program on Windows: PATHEXT has no empty extension.
        touch(&first.join("codex"));
        let probe = TreeProbe::new(t.path()).path(&[first.clone(), first.clone()]);
        let found = copies(&["codex".to_owned()], &probe);
        assert_eq!(found, [a.clone(), b]);
        let plan = plan_for(&profile_with(None, None), &a, &probe);
        assert_eq!(plan.warnings, [Warning::SecondCopy]);
        assert_eq!(plan.copies.len(), 2);
    }

    #[test]
    fn git_for_windows_bash() {
        let t = TempDir::new("source-git");
        let root = t.path().join("Program Files/Git");
        let git = touch(&root.join("cmd/git.exe"));
        let usr_bash = touch(&root.join("usr/bin/bash.exe"));
        let bin_bash = touch(&root.join("bin/bash.exe"));
        let probe = TreeProbe::new(t.path());
        assert_eq!(git_for_windows(&usr_bash, &probe), Some(git.clone()));
        assert_eq!(git_for_windows(&bin_bash, &probe), Some(git));
        let lone = touch(&t.path().join("other/bin/bash.exe"));
        assert_eq!(git_for_windows(&lone, &probe), None);
        assert_eq!(git_for_windows(&root.join("cmd/git.exe"), &probe), None);
    }

    #[test]
    fn display_quotes_parts_with_spaces() {
        assert_eq!(
            display_command(
                r"C:\Program Files\nodejs\npm.cmd",
                &["install".into(), "x@latest".into()]
            ),
            r#""C:\Program Files\nodejs\npm.cmd" install x@latest"#
        );
    }

    /// Reads this machine only; nothing is started or changed: `cargo test -- --ignored live_detection`.
    #[test]
    #[ignore]
    fn live_detection_on_this_machine() {
        let mut agents: Vec<crate::agent_catalog::Agent> = crate::agent_catalog::builtin()
            .unwrap()
            .into_iter()
            .map(crate::agent_catalog::Agent::from_catalog)
            .collect();
        // Git Bash and PowerShell 7 as user profiles would start them.
        for (id, command) in [("bash", "bash"), ("pwsh", "pwsh")] {
            let source = format!(
                "[[profile]]
id='{id}'
name='{id}'
command='{command}'
"
            );
            let profile = crate::profile::parse(&source).unwrap().remove(0);
            agents.push(crate::agent_catalog::Agent::from_user(profile));
        }
        let path = std::env::var_os("PATH");
        let pathext = std::env::var_os("PATHEXT");
        for mut agent in agents {
            let Some(program) = agent
                .resolve(path.as_deref(), pathext.as_deref())
                .map(|r| r.program)
            else {
                continue;
            };
            crate::updates::enrich(&mut agent, &program, &SystemProbe);
            let plan = plan(
                &PlanInput {
                    profile: &agent.profile,
                    install_hint: agent.install_hint.as_deref(),
                    program: Some(&program),
                    candidates: &agent.candidates,
                },
                &SystemProbe,
            );
            println!(
                "{}: {} -> {:?} {:?} admin={} {:?} copies={:?}",
                agent.id(),
                program.display(),
                plan.installer,
                plan.command(),
                plan.needs_admin,
                plan.reason,
                plan.copies
            );
        }
    }
}
