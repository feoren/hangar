//! Follows a growing JSONL file that another process keeps writing (the agent CLI).
//!
//! The file is opened per poll and closed again, with read, write and delete sharing on Windows,
//! so Hangar never keeps the CLI from appending, rotating or deleting it. A byte offset marks what
//! was read; an incomplete last line waits in a buffer until its newline arrives.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// On the first read only this much of the end of the file is read: the live view needs the
/// latest state, not the whole history of a long-running sub-agent.
pub const INITIAL_TAIL: u64 = 256 * 1024;
/// Longer lines (huge tool results) are dropped instead of buffered without bound.
pub const MAX_LINE: usize = 1024 * 1024;
/// At most this much is read per poll, so a burst cannot block the watcher for long; the rest
/// follows on the next polls.
pub const MAX_READ: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct Tail {
    /// `None` before the first read.
    offset: Option<u64>,
    partial: Vec<u8>,
    /// Discarding up to the next newline: the read started in the middle of a line, or the
    /// line grew past [`MAX_LINE`].
    skipping: bool,
    /// How much of the end of the file the first read (and every start over) takes.
    initial: u64,
    /// Counts the starts: the first read and every start over after the file shrank.
    generation: u32,
    /// The latest start began after the beginning of the file, so earlier lines were left out.
    skipped_head: bool,
    /// The last read stopped at [`MAX_READ`] before the end of the file.
    behind: bool,
}

impl Default for Tail {
    fn default() -> Self {
        Self::with_initial(INITIAL_TAIL)
    }
}

impl Tail {
    /// A tail whose first read takes the last `initial` bytes instead of [`INITIAL_TAIL`].
    pub fn with_initial(initial: u64) -> Self {
        Self {
            offset: None,
            partial: Vec::new(),
            skipping: false,
            initial,
            generation: 0,
            skipped_head: false,
            behind: false,
        }
    }

    /// The last read left bytes for the next one: a reader that wants everything reads again.
    pub fn behind(&self) -> bool {
        self.behind
    }

    /// Changes with every start: a reader that shows the lines replaces what it showed.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The latest start left out the beginning of the file.
    pub fn skipped_head(&self) -> bool {
        self.skipped_head
    }

    /// Complete lines written since the last call, without `\n` or `\r\n`.
    pub fn read_new(&mut self, path: &Path) -> io::Result<Vec<Vec<u8>>> {
        let mut file = open_shared(path)?;
        let len = file.metadata()?.len();
        let offset = match self.offset {
            // Shrunk: the file was truncated or replaced. Start over like a first read.
            Some(offset) if offset <= len => offset,
            _ => {
                self.partial.clear();
                let start = len.saturating_sub(self.initial);
                self.generation = self.generation.wrapping_add(1);
                self.skipped_head = start > 0;
                // Only a cut line is skipped: if the byte before `start` ends a line, the read
                // starts exactly at a line start.
                self.skipping = start > 0 && byte_at(&mut file, start - 1)? != b'\n';
                start
            }
        };
        let wanted = (len - offset).min(MAX_READ);
        let mut buffer = Vec::new();
        if wanted > 0 {
            file.seek(SeekFrom::Start(offset))?;
            file.take(wanted).read_to_end(&mut buffer)?;
        }
        let end = offset + buffer.len() as u64;
        self.offset = Some(end);
        self.behind = end < len;
        Ok(self.split(&buffer))
    }

    fn split(&mut self, mut data: &[u8]) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        while !data.is_empty() {
            let (segment, complete) = match data.iter().position(|b| *b == b'\n') {
                Some(end) => (&data[..end], true),
                None => (data, false),
            };
            data = &data[segment.len() + usize::from(complete)..];
            if self.skipping {
                if complete {
                    self.skipping = false;
                }
                continue;
            }
            if self.partial.len() + segment.len() > MAX_LINE {
                self.partial.clear();
                self.skipping = !complete;
                continue;
            }
            self.partial.extend_from_slice(segment);
            if complete {
                let mut line = std::mem::take(&mut self.partial);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if !line.is_empty() {
                    lines.push(line);
                }
            }
        }
        lines
    }
}

fn byte_at(file: &mut File, at: u64) -> io::Result<u8> {
    let mut byte = [0u8; 1];
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(&mut byte)?;
    Ok(byte[0])
}

/// The first line of the file without its line break, `None` when it is empty, not finished yet
/// or longer than `max` bytes (then it is not worth showing and must not be buffered).
pub fn first_line(path: &Path, max: usize) -> io::Result<Option<Vec<u8>>> {
    let file = open_shared(path)?;
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let mut reader = io::BufReader::new(file.take(limit));
    let mut line = Vec::new();
    io::BufRead::read_until(&mut reader, b'\n', &mut line)?;
    if line.last() != Some(&b'\n') {
        return Ok(None);
    }
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok((!line.is_empty() && line.len() <= max).then_some(line))
}

/// Read-only, shared with writers and deleters; never follows the file beyond this call.
///
/// A link in the last path part is never followed (`FILE_FLAG_OPEN_REPARSE_POINT` on Windows
/// opens the link itself, `O_NOFOLLOW` on Unix refuses it), and the opened handle must be a
/// regular file; a FIFO opens without blocking (`O_NONBLOCK`) and is then refused.
///
/// Limits: the folders above the file are checked once per poll ([`super::claude`]), not on
/// this open, so a folder swapped for a link in between is still followed; a hard link to a file
/// elsewhere cannot be told apart from the file. Both need someone who can already write to the
/// user's Claude folder, that is the same user, who could read the target anyway.
pub fn open_shared(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        options.share_mode(0x1 | 0x2 | 0x4);
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NONBLOCK: opening a FIFO for reading would otherwise wait for a writer and hang
        // the watcher; on a regular file it changes nothing.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::test_support::TempDir;

    fn append(path: &Path, data: &[u8]) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(data).unwrap();
    }

    fn text(lines: Vec<Vec<u8>>) -> Vec<String> {
        lines
            .into_iter()
            .map(|l| String::from_utf8(l).unwrap())
            .collect()
    }

    #[test]
    fn partial_lines_wait_for_their_newline() {
        let dir = TempDir::new("tail-partial");
        let path = dir.path().join("a.jsonl");
        append(&path, b"one\r\ntw");
        let mut tail = Tail::default();
        assert_eq!(text(tail.read_new(&path).unwrap()), ["one"]);
        assert!(tail.read_new(&path).unwrap().is_empty());
        append(&path, b"o\n\nthree\n");
        assert_eq!(text(tail.read_new(&path).unwrap()), ["two", "three"]);
    }

    #[test]
    fn first_read_starts_near_the_end_at_a_line_start() {
        let dir = TempDir::new("tail-initial");
        let path = dir.path().join("a.jsonl");
        let filler = format!("{}\n", "x".repeat(1023));
        let mut content = String::new();
        while (content.len() as u64) < INITIAL_TAIL * 2 {
            content.push_str(&filler);
        }
        content.push_str("last\n");
        append(&path, content.as_bytes());
        let lines = text(Tail::default().read_new(&path).unwrap());
        assert_eq!(lines.last().map(String::as_str), Some("last"));
        // No cut-off first line, and not the whole file.
        assert!(lines[..lines.len() - 1].iter().all(|l| l.len() == 1023));
        assert!((lines.len() as u64) < INITIAL_TAIL / 1024 + 2);
    }

    #[test]
    fn first_read_exactly_at_a_line_start_keeps_that_line() {
        let dir = TempDir::new("tail-boundary");
        let path = dir.path().join("a.jsonl");
        let head = "h".repeat(99);
        let body = "b".repeat(usize::try_from(INITIAL_TAIL).unwrap() - 1);
        append(&path, format!("{head}\n{body}\n").as_bytes());
        // The tail window starts right after the first newline.
        assert_eq!(text(Tail::default().read_new(&path).unwrap()), [body]);
    }

    #[test]
    fn huge_lines_are_dropped_and_reading_goes_on() {
        let dir = TempDir::new("tail-huge");
        let path = dir.path().join("a.jsonl");
        let mut tail = Tail::default();
        append(&path, b"before\n");
        assert_eq!(text(tail.read_new(&path).unwrap()), ["before"]);
        // Arrives over several polls, as a CLI flushes it.
        let chunk = vec![b'y'; MAX_LINE / 2 + 1];
        for _ in 0..3 {
            append(&path, &chunk);
            assert!(tail.read_new(&path).unwrap().is_empty());
        }
        append(&path, b"tail of huge\nafter\n");
        assert_eq!(text(tail.read_new(&path).unwrap()), ["after"]);
    }

    #[test]
    fn shrinking_file_starts_over() {
        let dir = TempDir::new("tail-shrink");
        let path = dir.path().join("a.jsonl");
        append(&path, b"aaaaaaaaaa\nbbbbbbbbbb\n");
        let mut tail = Tail::default();
        assert_eq!(tail.read_new(&path).unwrap().len(), 2);
        std::fs::write(&path, b"new\n").unwrap();
        assert_eq!(text(tail.read_new(&path).unwrap()), ["new"]);
        append(&path, b"next\n");
        assert_eq!(text(tail.read_new(&path).unwrap()), ["next"]);
    }

    #[test]
    fn generation_and_skipped_head_follow_the_starts() {
        let dir = TempDir::new("tail-generation");
        let path = dir.path().join("a.jsonl");
        append(&path, b"first line\nsecond line\n");
        let mut whole = Tail::with_initial(1024);
        assert_eq!(whole.generation(), 0);
        assert_eq!(whole.read_new(&path).unwrap().len(), 2);
        assert_eq!(whole.generation(), 1);
        assert!(!whole.skipped_head());
        append(&path, b"third\n");
        assert_eq!(text(whole.read_new(&path).unwrap()), ["third"]);
        assert_eq!(whole.generation(), 1);
        std::fs::write(&path, b"x\n").unwrap();
        assert_eq!(text(whole.read_new(&path).unwrap()), ["x"]);
        assert_eq!(whole.generation(), 2);

        std::fs::write(&path, b"first line\nsecond line\n").unwrap();
        let mut short = Tail::with_initial(12);
        assert_eq!(text(short.read_new(&path).unwrap()), ["second line"]);
        assert!(short.skipped_head());
    }

    #[test]
    fn a_whole_file_beyond_one_read_is_read_in_steps() {
        let dir = TempDir::new("tail-behind");
        let path = dir.path().join("a.jsonl");
        let line = format!("{}\n", "x".repeat(1023));
        let count = usize::try_from(MAX_READ / 1024).unwrap() * 2 + 3;
        append(&path, line.repeat(count).as_bytes());
        let mut tail = Tail::with_initial(u64::MAX);
        let mut read = 0;
        let mut steps = 0;
        loop {
            read += tail.read_new(&path).unwrap().len();
            steps += 1;
            if !tail.behind() {
                break;
            }
        }
        assert_eq!(read, count);
        assert_eq!(steps, 3);
        assert!(!tail.skipped_head());
    }

    #[test]
    fn first_line_is_bounded() {
        let dir = TempDir::new("tail-first");
        let path = dir.path().join("a.jsonl");
        append(&path, b"head\r\nrest\n");
        assert_eq!(first_line(&path, 64).unwrap(), Some(b"head".to_vec()));
        assert_eq!(first_line(&path, 3).unwrap(), None);
        let open = dir.path().join("b.jsonl");
        append(&open, b"no newline yet");
        assert_eq!(first_line(&open, 64).unwrap(), None);
    }

    #[test]
    fn folders_and_links_are_never_opened() {
        let dir = TempDir::new("tail-links");
        assert!(open_shared(dir.path()).is_err(), "a folder must not open");
        let target = dir.path().join("outside.jsonl");
        append(&target, b"OUTSIDE-MARKER\n");
        let link = dir.path().join("agent-a1.jsonl");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target, &link);
        // File symbolic links need Developer Mode or an elevated process on Windows.
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target, &link);
        match made {
            Ok(()) => {
                assert!(open_shared(&link).is_err(), "a link must not be followed");
                assert!(Tail::default().read_new(&link).is_err());
                assert_eq!(first_line(&link, 64).ok().flatten(), None);
            }
            Err(error) => eprintln!("symbolic link not possible here, link case skipped: {error}"),
        }
        assert!(open_shared(&target).is_ok());
    }

    /// A FIFO under a transcript's name is refused at once instead of waiting for a writer.
    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_blocking() {
        use std::os::unix::ffi::OsStrExt;
        let dir = TempDir::new("tail-fifo");
        let fifo = dir.path().join("agent-a1.jsonl");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `name` is a valid NUL-terminated path that lives for the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        assert!(open_shared(&fifo).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn file_stays_writable_and_deletable_while_followed() {
        let dir = TempDir::new("tail-share");
        let path = dir.path().join("a.jsonl");
        append(&path, b"x\n");
        let held = open_shared(&path).unwrap();
        append(&path, b"y\n");
        std::fs::remove_file(&path).unwrap();
        drop(held);
        assert!(Tail::default().read_new(&path).is_err());
    }
}
