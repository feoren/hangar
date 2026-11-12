//! App execution aliases of packaged Windows apps (Microsoft Store, MSIX), such as
//! `%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe` (sources: docs/wiki/pages/sources/app-execution-alias.md).
//!
//! The alias is an empty file with a reparse point of tag `IO_REPARSE_TAG_APPEXECLINK`. Windows
//! starts it like a program (CreateProcess follows it), but opening it as a file fails, so
//! everything that reads the program file (icon resources, signature, size and time stamp) has to
//! use the real executable inside the package, which the reparse data names.
//!
//! Layout of the reparse data, as libuv reads it (`src/win/fs.c`, `fs__readlink_handle`):
//! `ReparseTag: u32`, `ReparseDataLength: u16`, `Reserved: u16`, then `StringCount: u32` and that
//! many NUL-terminated UTF-16 strings: package family name, application user model ID, target
//! path (absolute), and on newer systems an application type.

use std::path::{Path, PathBuf};

/// `IO_REPARSE_TAG_APPEXECLINK` (windows-sys `Win32::System::SystemServices`).
pub const APPEXECLINK_TAG: u32 = 0x8000_001B;
/// Tag, data length, reserved, string count.
const HEADER_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecAlias {
    /// Package family name, such as `Microsoft.PowerShell_8wekyb3d8bbwe`.
    pub package: String,
    /// Application user model ID, such as `Microsoft.PowerShell_8wekyb3d8bbwe!App`.
    pub app_id: String,
    /// The executable inside the package, such as
    /// `C:\Program Files\WindowsApps\Microsoft.PowerShell_7.7.0.0_x64__8wekyb3d8bbwe\pwsh.exe`.
    pub target: PathBuf,
}

/// Reserved device names after Microsoft's "Naming Files, Paths, and Namespaces"
/// (learn.microsoft.com/windows/win32/fileio/naming-a-file): a path component with such a name,
/// also followed by an extension (`NUL.txt`), opens the device instead of a file.
const DEVICE_NAMES: &[&str] = &["CON", "PRN", "AUX", "NUL"];

fn is_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ')
        .to_uppercase();
    if DEVICE_NAMES.contains(&stem.as_str()) {
        return true;
    }
    ["COM", "LPT"].iter().any(|prefix| {
        let mut rest = stem.strip_prefix(prefix).unwrap_or_default().chars();
        // The superscript digits count as digits too, per the same page.
        matches!(
            (rest.next(), rest.next()),
            (Some('1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
        )
    })
}

/// An absolute path on a drive (`X:\...`) whose components are all plain names: none empty, no
/// `.` or `..`, no device name. Hangar reads what the alias names (icon, size, time), so it must
/// not climb out of the package or point at a device.
fn plain_target(target: &str) -> bool {
    let bytes = target.as_bytes();
    let drive =
        bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    drive
        && target[3..].split(['\\', '/']).all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && !is_device_name(component)
        })
}

/// Reads a `REPARSE_DATA_BUFFER` (as returned by `FSCTL_GET_REPARSE_POINT`). `None` for another
/// tag, a truncated buffer, fewer than three strings, empty strings or a target that is not a
/// plain absolute drive path ([`plain_target`]).
pub fn parse(buffer: &[u8]) -> Option<ExecAlias> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(buffer.get(at..at + 4)?.try_into().ok()?))
    };
    if word(0)? != APPEXECLINK_TAG {
        return None;
    }
    let data_len = usize::from(u16::from_le_bytes([*buffer.get(4)?, *buffer.get(5)?]));
    // The data length counts everything after the 8-byte header, the string count included; a
    // shorter buffer was cut off and could end inside the target path.
    let end = 8 + data_len;
    if buffer.len() < end || word(8)? < 3 {
        return None;
    }
    let (pairs, _) = buffer.get(HEADER_LEN..end)?.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    let mut strings = units.split(|unit| *unit == 0);
    let mut next = || -> Option<String> {
        let text = String::from_utf16(strings.next()?).ok()?;
        (!text.is_empty()).then_some(text)
    };
    let package = next()?;
    let app_id = next()?;
    let target = next()?;
    plain_target(&target).then(|| ExecAlias {
        package,
        app_id,
        target: PathBuf::from(target),
    })
}

/// The alias behind `path`, `None` for every ordinary file (and always outside Windows).
#[cfg(windows)]
pub fn read(path: &Path) -> Option<ExecAlias> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
    };
    use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    // Cheap check first: only empty files can be an alias, and a missing file is none.
    if std::fs::symlink_metadata(path).ok()?.len() != 0 {
        return None;
    }
    // The reparse point itself, not what it points to; attributes are enough to read it.
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut buffer = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize];
    let mut returned = 0u32;
    // SAFETY: The handle stays open for the call (`file` lives until the end of the function),
    // the output buffer is valid for its full length, which is passed as its size, no input
    // buffer is used, and the call is synchronous (no OVERLAPPED).
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_GET_REPARSE_POINT,
            std::ptr::null(),
            0,
            buffer.as_mut_ptr().cast(),
            MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    buffer.truncate(usize::try_from(returned).ok()?);
    // Only a target on a fixed local drive: a network or removable drive is not read from.
    parse(&buffer).filter(|alias| crate::gate::local_fixed_drive(&alias.target))
}

#[cfg(not(windows))]
pub fn read(_path: &Path) -> Option<ExecAlias> {
    None
}

/// The file to read for `program`: the package's executable behind an alias, else the program.
pub fn real_file(program: &Path) -> PathBuf {
    read(program).map_or_else(|| program.to_path_buf(), |alias| alias.target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reparse buffer as Windows writes it for an alias.
    fn buffer(tag: u32, strings: &[&str]) -> Vec<u8> {
        let mut data = (strings.len() as u32).to_le_bytes().to_vec();
        for text in strings {
            for unit in text.encode_utf16().chain([0]) {
                data.extend_from_slice(&unit.to_le_bytes());
            }
        }
        let mut out = tag.to_le_bytes().to_vec();
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&data);
        out
    }

    const TARGET: &str =
        r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.7.0.0_x64__8wekyb3d8bbwe\pwsh.exe";

    #[test]
    fn reads_package_app_and_target() {
        let alias = parse(&buffer(
            APPEXECLINK_TAG,
            &[
                "Microsoft.PowerShell_8wekyb3d8bbwe",
                "Microsoft.PowerShell_8wekyb3d8bbwe!App",
                TARGET,
                "0",
            ],
        ))
        .unwrap();
        assert_eq!(alias.package, "Microsoft.PowerShell_8wekyb3d8bbwe");
        assert_eq!(alias.app_id, "Microsoft.PowerShell_8wekyb3d8bbwe!App");
        assert_eq!(alias.target, PathBuf::from(TARGET));
        // Older systems write three strings.
        assert!(parse(&buffer(APPEXECLINK_TAG, &["p", "p!App", TARGET])).is_some());
    }

    #[test]
    fn rejects_anything_else() {
        // A symbolic link (IO_REPARSE_TAG_SYMLINK) is no alias.
        assert_eq!(parse(&buffer(0xA000_000C, &["p", "p!App", TARGET])), None);
        assert_eq!(parse(&buffer(APPEXECLINK_TAG, &["p", "p!App"])), None);
        assert_eq!(parse(&buffer(APPEXECLINK_TAG, &["p", "", TARGET])), None);
        assert_eq!(
            parse(&buffer(
                APPEXECLINK_TAG,
                &["p", "p!App", r"relative\pwsh.exe"]
            )),
            None
        );
        assert_eq!(
            parse(&buffer(
                APPEXECLINK_TAG,
                &["p", "p!App", r"\\server\share\x.exe"]
            )),
            None
        );
        let full = buffer(APPEXECLINK_TAG, &["p", "p!App", TARGET]);
        for cut in [0, 3, 8, 11, 40] {
            assert_eq!(parse(&full[..cut]), None, "{cut}");
        }
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn targets_are_plain_paths_without_devices_or_climbing() {
        for bad in [
            r"C:\Program Files\WindowsApps\..\..\Windows\System32\cmd.exe",
            r"C:\Program Files\WindowsApps\.\pwsh.exe",
            r"C:\Program Files\\pwsh.exe",
            r"C:\Program Files/WindowsApps/../x.exe",
            r"C:\x\CON",
            r"C:\x\nul.txt",
            r"C:\x\Aux.tar.gz",
            r"C:\x\COM1.exe",
            r"C:\x\lpt9",
            r"C:\x\COM¹.exe",
            r"C:\x\LPT³",
            r"C:\x\NUL .txt",
            r"C:\x\pwsh.exe\",
            r"C:\",
        ] {
            assert_eq!(
                parse(&buffer(APPEXECLINK_TAG, &["p", "p!App", bad])),
                None,
                "{bad}"
            );
        }
        // Names that only start like a device are fine.
        for good in [
            r"C:\x\CONSOLE.exe",
            r"C:\x\com10.exe",
            r"C:\x\nullable\a.exe",
            TARGET,
        ] {
            assert!(
                parse(&buffer(APPEXECLINK_TAG, &["p", "p!App", good])).is_some(),
                "{good}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn only_fixed_local_drives() {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let fixed = crate::gate::local_fixed_drive;
        assert!(fixed(Path::new(&format!(r"{system}\Windows\x.exe"))));
        // A drive letter without a drive behind it.
        let free = (b'D'..=b'Z')
            .map(char::from)
            .find(|letter| !Path::new(&format!(r"{letter}:\")).exists())
            .unwrap();
        assert!(!fixed(Path::new(&format!(r"{free}:\x\a.exe"))));
        assert!(!fixed(Path::new(r"\\server\share\a.exe")));
    }

    #[test]
    fn ordinary_files_are_no_alias() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join(".tmp");
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("exec alias empty.exe");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(read(&empty), None);
        assert_eq!(real_file(&empty), empty);
        std::fs::remove_file(&empty).unwrap();
        assert_eq!(read(&dir.join("missing.exe")), None);
        assert_eq!(read(Path::new(env!("CARGO_MANIFEST_DIR"))), None);
    }
}
