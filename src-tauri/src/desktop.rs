//! Opens folders and files with the program the operating system assigns to them.
//!
//! Callers pass paths the backend determined itself (notes folder, profiles.toml), with one
//! exception: a path a session printed and the user clicked in the terminal, which path_links.rs
//! checks first (local real paths only, only harmless file types open, the rest is revealed).
//! Web pages ([`open_url`]) are only http(s) addresses the backend checked: the account's pages,
//! a local dev server a session printed ([`local_dev_url`]) and an address clicked in the
//! terminal (`path_links::web_url`).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What gets opened. On macOS this decides on `open -t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Show in the file manager.
    Folder,
    /// Text file in the default editor. On macOS with `-t`, because `open` otherwise rejects a `.toml`
    /// without an associated program; Windows (Explorer) and Linux (`xdg-open`) pick the
    /// associated program or ask.
    TextFile,
}

/// Program and arguments, without a shell. Separate from [`open`] so it can be tested.
pub fn command_for(path: &Path, target: Target) -> (PathBuf, Vec<std::ffi::OsString>) {
    #[cfg(windows)]
    let program = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("explorer.exe");
    #[cfg(target_os = "macos")]
    let program = PathBuf::from("/usr/bin/open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = PathBuf::from("xdg-open");

    let mut args = Vec::new();
    if cfg!(target_os = "macos") && target == Target::TextFile {
        args.push("-t".into());
    }
    args.push(path.as_os_str().to_owned());
    (program, args)
}

pub fn open(path: &Path, target: Target) -> io::Result<()> {
    let (program, args) = command_for(path, target);
    let mut command = Command::new(program);
    command.args(args);
    spawn_detached(command)
}

/// Program and arguments that show `file` selected in the file manager. On Windows the single
/// argument is `/select,"<path>"` and must be passed raw ([`reveal`]): explorer.exe parses its
/// command line itself and does not understand the C runtime quoting of a whole argument. A
/// Windows path cannot contain `"`, so the quotes cannot be broken out of. Linux has no common
/// way to select a file; the folder opens instead.
pub fn reveal_command(file: &Path) -> (PathBuf, Vec<std::ffi::OsString>) {
    #[cfg(windows)]
    {
        let (program, _) = command_for(file, Target::Folder);
        let mut arg = std::ffi::OsString::from("/select,\"");
        arg.push(file.as_os_str());
        arg.push("\"");
        (program, vec![arg])
    }
    #[cfg(target_os = "macos")]
    {
        (
            PathBuf::from("/usr/bin/open"),
            vec!["-R".into(), file.as_os_str().to_owned()],
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let folder = file.parent().unwrap_or(file);
        command_for(folder, Target::Folder)
    }
}

/// Shows `file` selected in Explorer or Finder: paths from Hangar's own storage, and files or
/// bundles clicked in the terminal that are not plainly harmless to open (path_links.rs).
pub fn reveal(file: &Path) -> io::Result<()> {
    let (program, args) = reveal_command(file);
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        for arg in &args {
            command.raw_arg(arg);
        }
    }
    #[cfg(not(windows))]
    command.args(args);
    spawn_detached(command)
}

/// The address as text, if it is a plain http(s) address; nothing else is ever opened.
pub fn checked_url(url: &reqwest::Url) -> io::Result<&str> {
    if !matches!(url.scheme(), "https" | "http") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only plain http(s) addresses open in the browser",
        ));
    }
    Ok(url.as_str())
}

/// A dev server address from a session's output, if it is one: http or https, a loopback host
/// (`localhost`, `127.0.0.1`, `[::1]`), an explicit port, no user name or password. Anything else
/// is refused, so the frontend cannot use the command to open arbitrary pages or programs.
pub fn local_dev_url(text: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(text).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    // The parser has normalized the host: lower case, IPv4 in dotted form, IPv6 compressed.
    let loopback = matches!(url.host_str()?, "localhost" | "127.0.0.1" | "[::1]");
    // `port()` is `None` for the scheme's default port, which a dev server never prints alone.
    let port = url.port()?;
    (loopback && port != 0).then_some(url)
}

/// Program and arguments that open an http(s) address in the default browser, without a shell.
#[cfg(not(windows))]
pub fn url_command(url: &reqwest::Url) -> io::Result<(PathBuf, Vec<std::ffi::OsString>)> {
    let text = checked_url(url)?;
    let (program, _) = command_for(Path::new(""), Target::Folder);
    Ok((program, vec![text.into()]))
}

/// Opens the address in the default browser.
///
/// On Windows through `ShellExecuteW`, not explorer.exe: Explorer parses its own command line and
/// opened a folder window instead of the browser for addresses such as the sign-in page with
/// `?code=...`.
pub fn open_url(url: &reqwest::Url) -> io::Result<()> {
    #[cfg(windows)]
    {
        shell_open(checked_url(url)?)
    }
    #[cfg(not(windows))]
    {
        let (program, args) = url_command(url)?;
        let mut command = Command::new(program);
        command.args(args);
        spawn_detached(command)
    }
}

/// Opens a file in its default program. On Windows through `ShellExecuteW` like [`open_url`]:
/// explorer.exe parses its own command line and misreads paths with commas. Callers pass only
/// real paths of harmless file types here (path_links.rs, `opens_directly`).
pub fn open_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let text = path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "the path is not valid Unicode")
        })?;
        shell_open(text)
    }
    #[cfg(not(windows))]
    {
        let (program, args) = command_for(path, Target::Folder);
        let mut command = Command::new(program);
        command.args(args);
        spawn_detached(command)
    }
}

/// Opens a folder in the file manager. On Windows through `ShellExecuteW` ("open" on a folder
/// opens Explorer there), so the path never goes through explorer.exe's own command line parsing,
/// which splits at commas.
pub fn open_folder(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let text = path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "the path is not valid Unicode")
        })?;
        shell_open(text)
    }
    #[cfg(not(windows))]
    {
        open(path, Target::Folder)
    }
}

/// UTF-16 with the terminating zero the Win32 API expects.
#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn shell_open(address: &str) -> io::Result<()> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = wide("open");
    let file = wide(address);
    // SAFETY: both buffers are zero-terminated UTF-16 that live until the call returns; the
    // window handle, parameters and directory may be null according to the ShellExecuteW
    // documentation.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success; smaller ones are error codes.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "the default program did not open (ShellExecute {})",
            result as isize
        )))
    }
}

fn spawn_detached(mut command: Command) -> io::Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap it so no zombie remains on Unix; explorer.exe exits right away anyway.
    std::thread::Builder::new()
        .name("open-wait".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_dev_servers_with_a_port_open() {
        for ok in [
            "http://localhost:5173/",
            "http://LOCALHOST:3000/app?x=1",
            "https://127.0.0.1:8443",
            "http://[::1]:8080/",
            "http://[0:0:0:0:0:0:0:1]:8080/",
        ] {
            assert!(local_dev_url(ok).is_some(), "{ok}");
        }
        for refused in [
            "http://localhost/",
            "http://localhost:80/",
            "http://localhost:0/",
            "http://example.com:3000/",
            "http://localhost.example.com:3000/",
            "http://127.0.0.2:3000/",
            "http://0.0.0.0:3000/",
            "http://user:secret@localhost:3000/",
            "http://user@localhost:3000/",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "ftp://localhost:21/",
            "localhost:3000",
            "",
        ] {
            assert!(local_dev_url(refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn path_is_a_single_argument_without_shell() {
        let path = Path::new("Folder with spaces & symbols/profiles.toml");
        let (program, args) = command_for(path, Target::TextFile);
        assert_eq!(args.last().map(|a| a.as_os_str()), Some(path.as_os_str()));
        let name = program.file_name().and_then(|n| n.to_str()).unwrap_or("");
        #[cfg(windows)]
        assert_eq!(name, "explorer.exe");
        #[cfg(target_os = "macos")]
        {
            assert_eq!(name, "open");
            assert_eq!(args.first().and_then(|a| a.to_str()), Some("-t"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(name, "xdg-open");
        if !cfg!(target_os = "macos") {
            assert_eq!(args.len(), 1);
        }
    }

    #[test]
    fn reveal_selects_the_file() {
        let file = if cfg!(windows) {
            Path::new(r"C:\Users\Test User\notes\a & b.md")
        } else {
            Path::new("/home/test user/notes/a & b.md")
        };
        let (program, args) = reveal_command(file);
        let name = program.file_name().and_then(|n| n.to_str()).unwrap_or("");
        #[cfg(windows)]
        {
            assert_eq!(name, "explorer.exe");
            assert_eq!(
                args,
                [std::ffi::OsString::from(
                    r#"/select,"C:\Users\Test User\notes\a & b.md""#
                )]
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(name, "open");
            assert_eq!(args.len(), 2);
            assert_eq!(args[1], file.as_os_str());
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert_eq!(name, "xdg-open");
            assert_eq!(args, [file.parent().unwrap().as_os_str().to_owned()]);
        }
    }

    #[test]
    fn only_http_addresses_open() {
        let url = reqwest::Url::parse("https://example.test/device?code=BCDF-GHJK").unwrap();
        assert_eq!(checked_url(&url).unwrap(), url.as_str());
        let comma = reqwest::Url::parse("https://example.test/a,b").unwrap();
        assert!(checked_url(&comma).is_ok());
        for refused in ["file:///C:/Windows/notepad.exe", "ftp://example.test/x"] {
            let url = reqwest::Url::parse(refused).unwrap();
            assert!(checked_url(&url).is_err(), "{refused}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn urls_open_as_one_argument() {
        let url = reqwest::Url::parse("https://example.test/device?user_code=BCDF-GHJK").unwrap();
        let (program, args) = url_command(&url).unwrap();
        assert_eq!(args, [std::ffi::OsString::from(url.as_str())]);
        let name = program.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert!(["open", "xdg-open"].contains(&name), "{name}");
    }

    #[cfg(windows)]
    #[test]
    fn addresses_reach_shell_execute_zero_terminated() {
        let text = wide("https://example.test/device?code=BCDF-GHJK");
        assert_eq!(text.last(), Some(&0));
        assert_eq!(text.iter().filter(|&&unit| unit == 0).count(), 1);
    }

    #[test]
    fn folder_without_editor_flag() {
        let (_, args) = command_for(Path::new("x"), Target::Folder);
        assert_eq!(args.len(), 1);
    }
}
