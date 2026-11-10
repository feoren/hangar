//! Is a program file a real Windows executable before Hangar starts it unattended?
//!
//! CreateProcessW treats a `.exe` without the `MZ` signature as a 16-bit DOS program. 64-bit
//! Windows cannot run those and shows a modal system dialog ("Unsupported 16-Bit Application")
//! instead of just failing. npm packages such as `opencode-ai` ship a text placeholder named
//! `opencode.exe` that a postinstall script replaces; when that script did not run, every
//! version query at startup raised the dialog. npm shims (`opencode.cmd`) start the `.exe` through
//! cmd.exe, so the files a shim calls are checked as well.
//!
//! On Unix a file without a valid format fails with `ENOEXEC` and no dialog; everything counts as
//! launchable there.

use std::path::{Path, PathBuf};

/// Scripts larger than this are not npm shims; they are not inspected.
#[cfg_attr(not(windows), allow(dead_code))]
const SHIM_LIMIT: u64 = 64 * 1024;

/// `false` if starting `program` would make Windows show the 16-bit dialog: the program itself or
/// an existing `.exe`/`.com` named by its npm shim lacks the `MZ` signature. Missing files count
/// as launchable; they fail without a dialog.
pub fn is_launchable(program: &Path) -> bool {
    if cfg!(windows) {
        windows_launchable(program)
    } else {
        true
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_launchable(program: &Path) -> bool {
    if is_image(program) {
        return has_mz_signature(program);
    }
    if has_ext(program, &["cmd", "bat"]) {
        return shim_targets(program)
            .iter()
            .filter(|target| is_image(target) && target.is_file())
            .all(|target| has_mz_signature(target));
    }
    true
}

fn has_ext(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn is_image(path: &Path) -> bool {
    has_ext(path, &["exe", "com"])
}

/// A file that cannot be read counts as valid: the check only prevents the dialog, it does not
/// decide whether a program is installed.
fn has_mz_signature(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return true;
    };
    let mut head = Vec::with_capacity(2);
    match file.take(2).read_to_end(&mut head) {
        Ok(_) => head == b"MZ",
        Err(_) => true,
    }
}

/// Files an npm shim (cmd-shim) calls: every quoted `"%dp0%\…"` or `"%~dp0\…"` path, resolved
/// against the shim's folder.
fn shim_targets(shim: &Path) -> Vec<PathBuf> {
    let Some(dir) = shim.parent() else {
        return Vec::new();
    };
    let Ok(meta) = std::fs::metadata(shim) else {
        return Vec::new();
    };
    if meta.len() > SHIM_LIMIT {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(shim) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    text.split('"')
        // Odd parts lie inside quotes.
        .skip(1)
        .step_by(2)
        .filter_map(|quoted| {
            ["%dp0%", "%~dp0"]
                .iter()
                .find_map(|prefix| quoted.strip_prefix(prefix))
        })
        .map(|rest| {
            let relative = rest.trim_start_matches(['\\', '/']);
            relative
                .split(['\\', '/'])
                .filter(|part| !part.is_empty())
                .fold(dir.to_path_buf(), |path, part| path.join(part))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use std::fs;

    /// The placeholder `opencode-ai` ships before its postinstall script runs (shortened).
    const PLACEHOLDER: &[u8] =
        b"#!/bin/sh\necho \"Error: opencode-ai's postinstall script was not run.\"\nexit 1\n";

    /// npm's cmd-shim output for a native binary, as found next to `opencode.cmd`.
    const NPM_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n\
        :start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\
        \"%dp0%\\node_modules\\opencode-ai\\bin\\opencode.exe\"   %*\r\n";

    fn shim_dir(name: &str, exe: &[u8]) -> (TempDir, PathBuf) {
        let dir = TempDir::new(name);
        let bin = dir
            .path()
            .join("node_modules")
            .join("opencode-ai")
            .join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("opencode.exe"), exe).unwrap();
        let shim = dir.path().join("opencode.cmd");
        fs::write(&shim, NPM_SHIM).unwrap();
        (dir, shim)
    }

    #[test]
    fn shim_targets_resolve_against_the_shim_folder() {
        let (dir, shim) = shim_dir("exe-image-targets", b"MZ");
        let expected = dir
            .path()
            .join("node_modules")
            .join("opencode-ai")
            .join("bin")
            .join("opencode.exe");
        assert_eq!(shim_targets(&shim), [expected]);
    }

    #[test]
    fn exe_without_mz_is_rejected() {
        let dir = TempDir::new("exe-image-plain");
        let fake = dir.path().join("fake.exe");
        fs::write(&fake, PLACEHOLDER).unwrap();
        let real = dir.path().join("real.exe");
        fs::write(&real, b"MZ\x90\x00").unwrap();
        assert!(!windows_launchable(&fake));
        assert!(windows_launchable(&real));
    }

    #[test]
    fn npm_shim_to_placeholder_is_rejected() {
        let (_dir, shim) = shim_dir("exe-image-shim-bad", PLACEHOLDER);
        assert!(!windows_launchable(&shim));
    }

    #[test]
    fn npm_shim_to_real_binary_is_accepted() {
        let (_dir, shim) = shim_dir("exe-image-shim-ok", b"MZ\x90\x00");
        assert!(windows_launchable(&shim));
    }

    #[test]
    fn missing_or_other_files_count_as_launchable() {
        let dir = TempDir::new("exe-image-other");
        assert!(windows_launchable(&dir.path().join("missing.exe")));
        let script = dir.path().join("tool.ps1");
        fs::write(&script, PLACEHOLDER).unwrap();
        assert!(windows_launchable(&script));
        // A shim whose target does not exist fails in cmd.exe without a dialog.
        let shim = dir.path().join("gone.cmd");
        fs::write(&shim, NPM_SHIM).unwrap();
        assert!(windows_launchable(&shim));
    }
}
