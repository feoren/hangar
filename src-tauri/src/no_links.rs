//! Looking at a path without a lookup ever leaving this machine. A symlink or junction in any
//! segment (`mklink /D C:\w \\host\share`, or one to a mapped drive) can lead to another
//! computer, and a mere lookup there contacts it (and hands it the user's credentials on
//! Windows). The segments are checked from the root down, so no lookup ever passes a link that
//! has not been checked, and `canonicalize` may run only after a walk has passed.
//!
//! Two modes ([`Links`]): `Refuse` takes no link and no reparse point at all (the gate, which
//! also lists folders, and listing a cloud placeholder folder may fetch it from the cloud);
//! `FollowLocal` follows symlinks and junctions whose target, read with `read_link` (which does
//! not touch the target), lies on a local fixed drive, at most [`MAX_LINKS`] of them. On Windows
//! only name surrogate reparse points count as links there; others (OneDrive placeholders,
//! deduplicated files) name no other path and are plain files and folders.
//!
//! A link swapped in between the walk and the lookup after it is not caught; that takes a
//! process already running on this machine, which could contact the share itself.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Links one walk follows; more is a loop or nothing a printed path needs.
pub const MAX_LINKS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Links {
    /// Any link or reparse point on the way refuses the path.
    Refuse,
    /// Links to local targets are followed.
    FollowLocal,
}

#[derive(Debug, thiserror::Error)]
pub enum NoLinksError {
    #[error("{0} is a link")]
    Link(PathBuf),
    #[error("{0} leads off this machine")]
    Remote(PathBuf),
    #[error("more than {MAX_LINKS} links on the way")]
    TooManyLinks,
    #[error("{path} cannot be read: {source}")]
    Unreadable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// A symlink, or on Windows any reparse point (junctions, mount points and cloud placeholders
/// too, which `FileType::is_symlink` may not report).
pub fn is_reparse_point(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        if meta_attributes(meta) & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// `FILE_ATTRIBUTE_REPARSE_POINT` of the Windows SDK.
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[cfg(windows)]
fn meta_attributes(meta: &fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt as _;
    meta.file_attributes()
}

/// Whether the entry at `path` (not followed) names another path: a symlink, or on Windows a
/// reparse point with a name surrogate tag (symlink, junction, mount point). The tag is read
/// from the parent folder's listing, which opens neither the entry nor what it points to.
fn is_link(path: &Path, meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        if meta_attributes(meta) & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            // Unknown tag: counted as a link, which is the stricter reading.
            return reparse_tag(path).is_none_or(is_name_surrogate);
        }
    }
    #[cfg(not(windows))]
    let _ = path;
    meta.file_type().is_symlink()
}

/// `IsReparseTagNameSurrogate` of the Windows SDK (winnt.h): bit 29 of the tag.
#[cfg(windows)]
fn is_name_surrogate(tag: u32) -> bool {
    tag & 0x2000_0000 != 0
}

/// The reparse tag of the entry at `path`: `dwReserved0` of `FindFirstFileW`, documented to hold
/// it when the entry has `FILE_ATTRIBUTE_REPARSE_POINT`.
#[cfg(windows)]
fn reparse_tag(path: &Path) -> Option<u32> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, WIN32_FIND_DATAW};
    let text = path.as_os_str();
    // FindFirstFileW takes wildcards; such a name is no entry to ask about.
    if text
        .encode_wide()
        .any(|c| c == u16::from(b'*') || c == u16::from(b'?'))
    {
        return None;
    }
    let mut wide: Vec<u16> = text.encode_wide().collect();
    // Past MAX_PATH only the verbatim form works. Not always: it would keep trailing dots and
    // spaces that Windows otherwise drops, and then miss the entry.
    if wide.len() >= 260 && path.is_absolute() && !text.as_encoded_bytes().starts_with(br"\\") {
        wide.splice(0..0, r"\\?\".encode_utf16());
    }
    wide.push(0);
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `wide` is NUL-terminated and outlives the call; `data` is a valid, writable
    // WIN32_FIND_DATAW. The handle is closed below when the call returned one.
    let handle = unsafe { FindFirstFileW(wide.as_ptr(), &mut data) };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: `handle` is a search handle FindFirstFileW returned and is not used afterwards.
    unsafe { FindClose(handle) };
    (data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0).then_some(data.dwReserved0)
}

/// Whether a link target lies on this machine, judged by its text alone: on Windows a path on a
/// fixed local drive (no UNC, no device or volume path, no mapped or removable drive); elsewhere
/// an absolute path that is not `//host/...`.
fn local_target(target: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        match target.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => fixed_drive(letter),
                _ => false,
            },
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        let text = target.as_os_str().as_encoded_bytes();
        target.is_absolute() && !text.starts_with(b"//")
    }
}

/// `GetDriveTypeW` says `DRIVE_FIXED` for the drive with this letter.
#[cfg(windows)]
pub(crate) fn fixed_drive(letter: u8) -> bool {
    /// `DRIVE_FIXED` of `GetDriveTypeW`.
    const DRIVE_FIXED: u32 = 3;
    let root: Vec<u16> = format!("{}:\\", char::from(letter))
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call, which only reads
    // it.
    let kind = unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) };
    kind == DRIVE_FIXED
}

/// `\\?\C:\x` as `C:\x`: junction targets come verbatim, and a verbatim path takes `..` and `/`
/// literally, which the walk below resolves itself.
fn plain_form(target: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if let Some(Component::Prefix(prefix)) = target.components().next() {
            if let Prefix::VerbatimDisk(letter) = prefix.kind() {
                let rest: PathBuf = target.components().skip(1).collect();
                let mut plain = PathBuf::from(format!("{}:\\", char::from(letter)));
                plain.push(rest.strip_prefix("\\").unwrap_or(&rest));
                return plain;
            }
        }
    }
    target
}

/// `..` and `.` resolved by the text alone, as Win32 does before it opens anything: `C:\link\..\x`
/// is `C:\x` and never touches `link`. Unix resolves `..` after following links instead, which
/// the walk does itself.
#[cfg(windows)]
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The metadata of `path` (the last segment followed too in `FollowLocal`) once the walk from the
/// root down found every segment acceptable for `links`. A drive root reports no reparse point
/// of its own; a mount point below it does. `.` is skipped and `..` goes back one checked
/// segment (on Windows by the text first, like Win32), so it never climbs out of a link target
/// unchecked.
pub fn metadata(path: &Path, links: Links) -> Result<fs::Metadata, NoLinksError> {
    let mut current = path.to_owned();
    let mut followed = 0;
    'walk: loop {
        #[cfg(windows)]
        {
            current = lexical(&current);
        }
        let parts: Vec<Component> = current.components().collect();
        let mut walked = PathBuf::new();
        let mut last = None;
        for (index, part) in parts.iter().enumerate() {
            match part {
                Component::Prefix(_) => {
                    walked.push(part);
                    continue;
                }
                Component::CurDir => continue,
                Component::ParentDir => {
                    walked.pop();
                    last = None;
                    continue;
                }
                Component::RootDir | Component::Normal(_) => walked.push(part),
            }
            let meta =
                fs::symlink_metadata(&walked).map_err(|source| NoLinksError::Unreadable {
                    path: walked.clone(),
                    source,
                })?;
            let link = match links {
                Links::Refuse if is_reparse_point(&meta) => {
                    return Err(NoLinksError::Link(walked));
                }
                Links::Refuse => false,
                Links::FollowLocal => is_link(&walked, &meta),
            };
            if !link {
                last = Some(meta);
                continue;
            }
            followed += 1;
            if followed > MAX_LINKS {
                return Err(NoLinksError::TooManyLinks);
            }
            let target = fs::read_link(&walked).map_err(|source| NoLinksError::Unreadable {
                path: walked.clone(),
                source,
            })?;
            // Relative targets count from the link's folder; a rooted one without a drive keeps
            // the drive (`PathBuf::push`).
            let mut resolved = walked.parent().map(Path::to_owned).unwrap_or_default();
            resolved.push(target);
            let resolved = plain_form(resolved);
            if !local_target(&resolved) {
                return Err(NoLinksError::Remote(walked));
            }
            // Walk again from the root: the target's own folders may be links as well.
            current = parts[index + 1..].iter().fold(resolved, |mut next, rest| {
                next.push(rest);
                next
            });
            continue 'walk;
        }
        if let Some(meta) = last {
            return Ok(meta);
        }
        // Empty, or ending in `..`: look at what the walk arrived at, through the same rules.
        if walked.as_os_str().is_empty() || walked == current {
            return Err(NoLinksError::Unreadable {
                path: path.to_owned(),
                source: io::Error::new(io::ErrorKind::InvalidInput, "no segment to look at"),
            });
        }
        current = walked;
    }
}

/// Whether `path` is a folder reached without any link or reparse point on the way.
pub fn plain_folder(path: &Path) -> bool {
    metadata(path, Links::Refuse).is_ok_and(|meta| meta.is_dir())
}

/// Windows network shares (`\\host\share`, `\\?\UNC\...`) and device or verbatim namespaces
/// (`\\.\...`, `\\?\...` other than a drive), by the text alone. Checked before a folder is
/// touched at all: even asking a share whether a folder exists hands the user's credentials to
/// its host.
pub fn is_network_or_device(path: &Path) -> bool {
    use std::path::Prefix;
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix)) if matches!(
            prefix.kind(),
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::DeviceNS(_) | Prefix::Verbatim(_)
        )
    )
}

/// Where a folder the user or a window named lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderPlace {
    /// A folder on this machine, also through local links, or on a mapped network drive named
    /// directly (Windows authenticated against its host when the user mapped it, so asking it
    /// hands over nothing new).
    Local,
    /// A share or device path, or a link on the way that leads off the fixed local drives (or
    /// too many links). Nothing there was touched.
    Remote,
    /// Relative, missing, unreadable or not a folder.
    NotAFolder,
}

/// [`FolderPlace`] of `path`, without a lookup ever reaching a host the user did not choose: the
/// text first (share, device, relative), then a walk from the root down that follows only links
/// to local fixed drives ([`Links::FollowLocal`]). `Path::is_dir` alone would follow a symlink or
/// junction to a share and authenticate against its host. A path on a mapped drive (`Z:\work`)
/// stays allowed: the user mapped that drive, and sessions there worked before.
pub fn folder_place(path: &Path) -> FolderPlace {
    if is_network_or_device(path) {
        return FolderPlace::Remote;
    }
    if !path.is_absolute() {
        return FolderPlace::NotAFolder;
    }
    match metadata(path, Links::FollowLocal) {
        Ok(meta) if meta.is_dir() => FolderPlace::Local,
        Ok(_) | Err(NoLinksError::Unreadable { .. }) | Err(NoLinksError::Link(_)) => {
            FolderPlace::NotAFolder
        }
        Err(NoLinksError::Remote(_)) | Err(NoLinksError::TooManyLinks) => FolderPlace::Remote,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::TempDir;

    /// A folder link to `target`: a junction on Windows (needs no privilege, unlike a symlink).
    pub(crate) fn folder_link(link: &Path, target: &Path) {
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(made.status.success(), "{made:?}");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn strict_mode_refuses_every_link_from_the_root_down() {
        let tmp = TempDir::new("no-links-strict");
        let target = tmp.path().join("target");
        fs::create_dir_all(target.join("inner")).unwrap();
        fs::write(target.join("marker.txt"), "marker").unwrap();
        let link = tmp.path().join("linked");
        folder_link(&link, &target);

        let strict = |path: &Path| metadata(path, Links::Refuse);
        assert!(strict(&target.join("marker.txt")).unwrap().is_file());
        assert!(plain_folder(&target.join("inner")));
        assert!(!plain_folder(&target.join("marker.txt")));
        for through in [
            link.clone(),
            link.join("inner"),
            link.join("marker.txt"),
            link.join("missing").join("deeper"),
        ] {
            let refused = strict(&through);
            assert!(
                matches!(&refused, Err(NoLinksError::Link(at)) if *at == link),
                "{}: {refused:?}",
                through.display()
            );
            assert!(!plain_folder(&through));
        }
        assert!(matches!(
            strict(&target.join("missing.txt")),
            Err(NoLinksError::Unreadable { .. })
        ));
        assert!(strict(Path::new("")).is_err());
    }

    #[test]
    fn local_links_are_followed_and_checked_again() {
        let tmp = TempDir::new("no-links-follow");
        let target = tmp.path().join("target");
        fs::create_dir_all(target.join("inner")).unwrap();
        fs::write(target.join("marker.txt"), "marker").unwrap();
        let link = tmp.path().join("linked");
        folder_link(&link, &target);
        let follow = |path: &Path| metadata(path, Links::FollowLocal);
        assert!(follow(&link.join("marker.txt")).unwrap().is_file());
        assert!(follow(&link.join("inner")).unwrap().is_dir());
        assert!(follow(&link).unwrap().is_dir());
        // `..` after the link: the same file either way.
        assert!(follow(&link.join("inner").join("..").join("marker.txt"))
            .unwrap()
            .is_file());
        assert!(matches!(
            follow(&link.join("missing.txt")),
            Err(NoLinksError::Unreadable { .. })
        ));
    }

    #[test]
    fn long_link_chains_are_refused() {
        let tmp = TempDir::new("no-links-chain");
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("marker.txt"), "marker").unwrap();
        // link0 -> target, link1 -> link0, ...
        let mut previous = target.clone();
        let mut chain = Vec::new();
        for hop in 0..=MAX_LINKS {
            let link = tmp.path().join(format!("link{hop}"));
            folder_link(&link, &previous);
            chain.push(link.clone());
            previous = link;
        }
        let within = &chain[MAX_LINKS - 1];
        assert!(metadata(&within.join("marker.txt"), Links::FollowLocal).is_ok());
        let beyond = &chain[MAX_LINKS];
        assert!(beyond.join("marker.txt").is_file(), "the chain works");
        assert!(matches!(
            metadata(&beyond.join("marker.txt"), Links::FollowLocal),
            Err(NoLinksError::TooManyLinks)
        ));
    }

    #[test]
    fn link_targets_off_this_machine_are_judged_by_their_text() {
        #[cfg(windows)]
        {
            assert!(!local_target(Path::new(r"\\nonexistent.invalid\share\x")));
            assert!(!local_target(Path::new(
                r"\\?\UNC\nonexistent.invalid\share"
            )));
            assert!(!local_target(Path::new(
                r"\\?\Volume{00000000-0000-0000-0000-000000000000}\"
            )));
            assert!(!local_target(Path::new(r"\\.\pipe\x")));
            assert!(!local_target(Path::new(r"relative\x")));
            let here = std::env::current_dir().unwrap();
            assert!(local_target(&here), "{}", here.display());
            assert_eq!(
                plain_form(PathBuf::from(r"\\?\C:\a\b")),
                PathBuf::from(r"C:\a\b")
            );
            assert!(is_name_surrogate(0xA000_000C), "symlink");
            assert!(is_name_surrogate(0xA000_0003), "junction");
            assert!(!is_name_surrogate(0x9000_401A), "OneDrive placeholder");
            assert!(!is_name_surrogate(0x8000_0013), "deduplicated file");
        }
        #[cfg(unix)]
        {
            assert!(!local_target(Path::new("//nonexistent.invalid/share")));
            assert!(!local_target(Path::new("relative/x")));
            assert!(local_target(Path::new("/private/tmp")));
        }
    }

    /// A link whose target is a network path is refused before anything is looked up there;
    /// creating it does not resolve the target either.
    #[test]
    fn links_to_network_paths_are_refused_without_connecting() {
        let tmp = TempDir::new("no-links-remote");
        let link = tmp.path().join("share");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(r"\\nonexistent.invalid\share", &link);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink("//nonexistent.invalid/share", &link);
        if let Err(error) = made {
            // Symlinks need developer mode or a privilege on Windows, and a junction cannot
            // point to a UNC path; the text rule is tested above regardless.
            eprintln!("no symlink allowed here ({error}), skipping");
            return;
        }
        let started = std::time::Instant::now();
        let refused = metadata(&link.join("file.txt"), Links::FollowLocal);
        assert!(
            matches!(&refused, Err(NoLinksError::Remote(at)) if *at == link),
            "{refused:?}"
        );
        assert!(matches!(
            metadata(&link, Links::FollowLocal),
            Err(NoLinksError::Remote(_))
        ));
        // A name lookup or SMB connection attempt would take seconds.
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(folder_place(&link), FolderPlace::Remote);
        assert_eq!(folder_place(&link.join("project")), FolderPlace::Remote);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn folders_are_placed_without_leaving_the_machine() {
        let tmp = TempDir::new("no-links-place");
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("marker.txt"), "marker").unwrap();
        let link = tmp.path().join("linked");
        folder_link(&link, &target);
        assert_eq!(folder_place(&target), FolderPlace::Local);
        assert_eq!(folder_place(&link), FolderPlace::Local);
        assert_eq!(
            folder_place(&target.join("marker.txt")),
            FolderPlace::NotAFolder
        );
        assert_eq!(
            folder_place(&target.join("missing")),
            FolderPlace::NotAFolder
        );
        assert_eq!(folder_place(Path::new("relative")), FolderPlace::NotAFolder);
        let started = std::time::Instant::now();
        #[cfg(windows)]
        for remote in [
            r"\\hangar-test.invalid\share\project",
            "//hangar-test.invalid/share/project",
            r"\\?\UNC\hangar-test.invalid\share",
            r"\\.\PhysicalDrive0",
        ] {
            assert_eq!(
                folder_place(Path::new(remote)),
                FolderPlace::Remote,
                "{remote}"
            );
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
}
