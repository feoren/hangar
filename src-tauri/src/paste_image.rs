//! Pasting a screenshot into a session: the clipboard holds an image but no text, so the image is
//! saved as a PNG file and the window pastes the file's path, which agent CLIs attach.
//!
//! The files live in the app's cache folder (`pastes/`) rather than the system temp folder: the
//! path must stay valid while the agent works with it, and system cleaners empty the temp folder
//! on their own schedule. Hangar removes its own files after [`MAX_AGE`] instead. Reading and
//! decoding the clipboard lives in `clipboard_image.rs`.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Serialize, Serializer};

use crate::persist::{create_private_dir, create_private_file};

/// Folder below the app's cache folder.
pub const FOLDER: &str = "pastes";
/// Pasted images older than this are removed.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Upper limit in pixels: three 4K screens side by side fit, an 8K screen too.
pub const MAX_PIXELS: usize = 40_000_000;
const PREFIX: &str = "paste-";
const EXTENSION: &str = ".png";
/// A clash of the random suffix is practically impossible; a few retries cover it anyway.
const NAME_ATTEMPTS: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum PasteImageError {
    #[error("Cache folder unknown: {0}")]
    CacheDir(String),
    #[error("Clipboard cannot be read: {0}")]
    Clipboard(String),
    #[error("Clipboard data too large: {bytes} bytes, at most {max} allowed")]
    DataTooLarge { bytes: usize, max: usize },
    #[error("Image too large: {width}x{height} pixels, at most {max} pixels allowed")]
    TooLarge {
        width: usize,
        height: usize,
        max: usize,
    },
    #[error("Clipboard image is malformed: {0}")]
    Malformed(String),
    #[error("Clipboard image format not supported: {0}")]
    Unsupported(String),
    #[error("PNG encoding failed: {0}")]
    Encode(#[from] png::EncodingError),
    #[error("File {path} cannot be written: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Background task aborted: {0}")]
    Task(String),
}

/// Errors reach the window as text; none contains image data.
impl Serialize for PasteImageError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// What a paste of the clipboard's image did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PasteOutcome {
    /// Saved; the window pastes this path.
    Saved { path: String },
    /// The clipboard holds no image.
    NoImage,
    /// Another paste is still reading or saving; this one is dropped without a word.
    Busy,
}

/// An image as 8 bit RGBA rows, top row first.
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Lets one image paste run at a time. A held Ctrl+V repeats the paste event many times a second,
/// and each paste would decode and encode a whole screen in parallel.
#[derive(Default)]
pub struct PasteGate(Arc<AtomicBool>);

/// Held while a paste runs; dropping it opens the gate again, also on an error or a panic.
pub struct PasteTicket(Arc<AtomicBool>);

impl PasteGate {
    /// `None` while another paste holds its ticket.
    pub fn try_start(&self) -> Option<PasteTicket> {
        let busy = self.0.swap(true, Ordering::AcqRel);
        (!busy).then(|| PasteTicket(Arc::clone(&self.0)))
    }
}

impl Drop for PasteTicket {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Rejects an image above [`MAX_PIXELS`] or without pixels. Decoders call this with the size from
/// the header, before they allocate anything for the pixels.
pub fn check_dimensions(width: usize, height: usize) -> Result<(), PasteImageError> {
    if width == 0 || height == 0 {
        return Err(PasteImageError::Malformed(format!(
            "{width}x{height} pixels"
        )));
    }
    if width.saturating_mul(height) > MAX_PIXELS {
        return Err(PasteImageError::TooLarge {
            width,
            height,
            max: MAX_PIXELS,
        });
    }
    Ok(())
}

/// Saves `image` as a new PNG file in `dir` and returns its path. Removes old pastes first. On
/// Unix the folder is only for the owner (0700) and so is each file (0600): a screenshot may show
/// anything.
pub fn save(dir: &Path, mut image: RgbaImage, now: SystemTime) -> Result<PathBuf, PasteImageError> {
    check(&image)?;
    make_opaque_if_blank(&mut image.rgba);
    let encoded = encode(&image)?;
    create_private_dir(dir).map_err(|source| io_error(dir, source))?;
    cleanup(dir, now, MAX_AGE);
    let mut last = None;
    for _ in 0..NAME_ATTEMPTS {
        let path = dir.join(file_name(now, &random_suffix()));
        match create_private_file(&path) {
            Ok(mut file) => {
                let written = file.write_all(&encoded).and_then(|()| file.sync_all());
                if let Err(source) = written {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(io_error(&path, source));
                }
                return Ok(path);
            }
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                last = Some((path, source));
            }
            Err(source) => return Err(io_error(&path, source)),
        }
    }
    let (path, source) = last.unwrap_or_else(|| {
        (
            dir.to_path_buf(),
            std::io::Error::from(ErrorKind::AlreadyExists),
        )
    });
    Err(io_error(&path, source))
}

/// Removes pasted images in `dir` older than `max_age`; other files stay. Returns how many went.
/// Errors are ignored: a file another program holds open is tried again next time.
pub fn cleanup(dir: &Path, now: SystemTime, max_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let is_paste = name
            .to_str()
            .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(EXTENSION));
        if !is_paste {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let old = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > max_age);
        if old && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// `paste-20261110-142233-1a2b3c4d.png`: UTC time, so names sort by time, and a random suffix.
pub fn file_name(now: SystemTime, suffix: &str) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let days = i64::try_from(seconds / 86_400).unwrap_or_default();
    let (year, month, day) = civil_from_days(days);
    let rest = seconds % 86_400;
    format!(
        "{PREFIX}{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{suffix}{EXTENSION}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

fn random_suffix() -> String {
    let mut hex = uuid::Uuid::new_v4().simple().to_string();
    hex.truncate(8);
    hex
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (H. Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Both are in range by construction (1..=31 and 1..=12).
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

fn check(image: &RgbaImage) -> Result<(), PasteImageError> {
    check_dimensions(image.width, image.height)?;
    if image.rgba.len() != image.width * image.height * 4 {
        return Err(PasteImageError::Malformed(format!(
            "{}x{} pixels but {} bytes",
            image.width,
            image.height,
            image.rgba.len()
        )));
    }
    Ok(())
}

/// Some programs put a bitmap on the clipboard whose alpha channel is all zero although the
/// picture is opaque; saved as it is, the screenshot would be invisible. A picture that is fully
/// transparent carries nothing, so treating it as opaque loses nothing.
fn make_opaque_if_blank(rgba: &mut [u8]) {
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    if pixels.iter().all(|[_, _, _, alpha]| *alpha == 0) {
        for [_, _, _, alpha] in pixels {
            *alpha = u8::MAX;
        }
    }
}

fn encode(image: &RgbaImage) -> Result<Vec<u8>, PasteImageError> {
    let too_large = || PasteImageError::TooLarge {
        width: image.width,
        height: image.height,
        max: MAX_PIXELS,
    };
    let width = u32::try_from(image.width).map_err(|_| too_large())?;
    let height = u32::try_from(image.height).map_err(|_| too_large())?;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Screenshots compress well even at the fast level; the default takes noticeably longer
        // for a large screen, and the person waits for the path to appear.
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&image.rgba)?;
        writer.finish()?;
    }
    Ok(out)
}

fn io_error(path: &Path, source: std::io::Error) -> PasteImageError {
    PasteImageError::Io {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn image(width: usize, height: usize, alpha: u8) -> RgbaImage {
        let mut rgba = Vec::with_capacity(width * height * 4);
        for index in 0..width * height {
            let value = u8::try_from(index % 256).unwrap();
            rgba.extend_from_slice(&[value, 255 - value, 7, alpha]);
        }
        RgbaImage {
            width,
            height,
            rgba,
        }
    }

    fn decode(path: &Path) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    #[test]
    fn names_sort_by_utc_time_and_carry_the_suffix() {
        assert_eq!(file_name(at(0), "ab12"), "paste-19700101-000000-ab12.png");
        // 2026-11-10 14:22:33 UTC
        assert_eq!(
            file_name(at(1_794_320_553), "1a2b3c4d"),
            "paste-20261110-142233-1a2b3c4d.png"
        );
        // Leap day.
        assert_eq!(
            file_name(at(951_782_400), "x"),
            "paste-20000229-000000-x.png"
        );
    }

    #[test]
    fn random_suffixes_are_short_hex_and_differ() {
        let first = random_suffix();
        assert_eq!(first.len(), 8);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, random_suffix());
    }

    #[test]
    fn saves_a_png_with_the_same_pixels() {
        let dir = TempDir::new("paste-image-save");
        let original = image(5, 3, 200);
        let expected = original.rgba.clone();
        let path = save(dir.path(), original, at(1_794_320_553)).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("paste-20261110-142233-"), "{name}");
        assert!(name.ends_with(".png"));
        assert_eq!(decode(&path), (5, 3, expected));
    }

    #[test]
    fn two_pastes_in_the_same_second_get_two_files() {
        let dir = TempDir::new("paste-image-twice");
        let first = save(dir.path(), image(2, 2, 255), at(100)).unwrap();
        let second = save(dir.path(), image(2, 2, 255), at(100)).unwrap();
        assert_ne!(first, second);
        assert!(first.is_file() && second.is_file());
    }

    #[test]
    fn a_blank_alpha_channel_becomes_opaque() {
        let dir = TempDir::new("paste-image-alpha");
        let path = save(dir.path(), image(2, 1, 0), at(100)).unwrap();
        let (_, _, pixels) = decode(&path);
        assert!(pixels.iter().skip(3).step_by(4).all(|&alpha| alpha == 255));
        // Real transparency stays.
        let mut mixed = image(2, 1, 0);
        mixed.rgba[7] = 128;
        let path = save(dir.path(), mixed, at(100)).unwrap();
        let (_, _, pixels) = decode(&path);
        assert_eq!((pixels[3], pixels[7]), (0, 128));
    }

    #[test]
    fn rejects_oversized_and_malformed_images_without_writing() {
        let dir = TempDir::new("paste-image-reject");
        let huge = RgbaImage {
            width: MAX_PIXELS + 1,
            height: 1,
            rgba: Vec::new(),
        };
        assert!(matches!(
            save(dir.path(), huge, at(100)),
            Err(PasteImageError::TooLarge { .. })
        ));
        let short = RgbaImage {
            width: 2,
            height: 2,
            rgba: vec![0; 15],
        };
        assert!(matches!(
            save(dir.path(), short, at(100)),
            Err(PasteImageError::Malformed(_))
        ));
        let empty = RgbaImage {
            width: 0,
            height: 4,
            rgba: Vec::new(),
        };
        assert!(matches!(
            save(dir.path(), empty, at(100)),
            Err(PasteImageError::Malformed(_))
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cleanup_removes_only_old_pastes() {
        let dir = TempDir::new("paste-image-cleanup");
        let old = dir.path().join("paste-20200101-000000-old.png");
        let fresh = dir.path().join("paste-20200101-000000-new.png");
        let foreign = dir.path().join("notes.png");
        for path in [&old, &fresh, &foreign] {
            fs::write(path, b"x").unwrap();
        }
        let folder = dir.path().join("paste-folder.png");
        fs::create_dir(&folder).unwrap();
        let written = fs::metadata(&old).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(written - Duration::from_secs(8 * 24 * 60 * 60))
            .unwrap();
        assert_eq!(cleanup(dir.path(), written, MAX_AGE), 1);
        assert!(!old.exists());
        assert!(fresh.exists());
        assert!(foreign.exists());
        assert!(folder.exists());
        // A missing folder is not an error.
        assert_eq!(cleanup(&dir.path().join("missing"), written, MAX_AGE), 0);
    }

    #[test]
    fn saving_cleans_up_old_pastes() {
        let dir = TempDir::new("paste-image-save-cleanup");
        let first = save(dir.path(), image(1, 1, 255), at(1_000)).unwrap();
        let written = fs::metadata(&first).unwrap().modified().unwrap();
        let later = written + MAX_AGE + Duration::from_secs(1);
        let second = save(dir.path(), image(1, 1, 255), later).unwrap();
        assert!(!first.exists());
        assert!(second.exists());
    }

    #[test]
    fn only_one_paste_runs_at_a_time() {
        let gate = PasteGate::default();
        let ticket = gate.try_start().unwrap();
        assert!(gate.try_start().is_none());
        drop(ticket);
        assert!(gate.try_start().is_some());
    }

    #[test]
    fn outcomes_reach_the_window_tagged() {
        let saved = PasteOutcome::Saved {
            path: "C:\\x.png".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&saved).unwrap(),
            r#"{"kind":"saved","path":"C:\\x.png"}"#
        );
        assert_eq!(
            serde_json::to_string(&PasteOutcome::NoImage).unwrap(),
            r#"{"kind":"noImage"}"#
        );
        assert_eq!(
            serde_json::to_string(&PasteOutcome::Busy).unwrap(),
            r#"{"kind":"busy"}"#
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions_only_for_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("paste-image-mode");
        let pastes = dir.path().join(FOLDER);
        let path = save(&pastes, image(1, 1, 255), at(100)).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&pastes), 0o700);
    }
}
