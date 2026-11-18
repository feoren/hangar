//! The login shell's `PATH` (ADR 0013) with a real shell: `/bin/sh` as a login shell reads
//! `$HOME/.profile`, which here prints noise and adds a folder, as a user's dotfiles would.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use hangar_lib::login_path::{query, run, COMMAND, TIMEOUT};

/// Test folder under `<repo>/tests/.tmp/`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/.tmp")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_real_login_shell_reports_the_path_its_profile_sets() {
    let home = TempDir::new("login-path-home");
    let marker = home.0.join("hangar-marker-bin");
    std::fs::write(
        home.0.join(".profile"),
        format!(
            "echo 'profile greeting'\nPATH=\"{}:$PATH\"\nexport PATH\n",
            marker.display()
        ),
    )
    .unwrap();
    let mut command = Command::new("/bin/sh");
    command
        .args(["-ilc", COMMAND])
        .env("HOME", &home.0)
        // An interactive POSIX shell also reads the file $ENV names; none here.
        .env_remove("ENV");
    let path = run(command, TIMEOUT).unwrap();
    let first = std::env::split_paths(&path).next();
    assert_eq!(first, Some(marker));
}

#[test]
fn the_users_own_login_shell_answers() {
    let path = query(Path::new("/bin/sh"), TIMEOUT).unwrap();
    assert!(
        std::env::split_paths(&path).any(|dir| dir.is_absolute()),
        "{path:?}"
    );
}
