//! The figure of each sub-agent definition (`agent_defs.rs`): one of the built-in figures the
//! window bundles, or an image of the user's own. Global per agent name. Contract:
//! docs/wiki/pages/syntheses/sub-agent-library.md.
//!
//! The choice lives in `agent-figures.json` in the config folder; own images in the app's data
//! folder (`agent-figures/<sha256>.<ext>`), never in the user's `.claude` folder. Images are
//! checked by their bytes (PNG, GIF, WebP, JPEG; never SVG, which can carry script) and size,
//! again when read, and reach the window as `data:` URLs: its CSP allows no other images.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::agent_defs::valid_name;
use crate::paste_image::RgbaImage;

/// File in the config folder.
pub const FILE_NAME: &str = "agent-figures.json";
/// Folder in the app's data folder.
pub const IMAGE_DIR: &str = "agent-figures";
/// Largest own image; a small animated GIF fits easily.
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
/// Agents with a chosen figure; far more than anyone defines.
pub const MAX_ENTRIES: usize = 256;
/// Largest `agent-figures.json` read; 256 entries take a few KiB.
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// Own images handed to the window in one answer, in bytes; images past it are left out (the
/// window then shows the derived figure), so a large set never makes one huge answer.
pub const MAX_TOTAL_IMAGE_BYTES: usize = 32 * 1024 * 1024;
/// Longest side of a pasted clipboard image after scaling down.
pub const PASTE_SIDE: usize = 256;
const VERSION: u32 = 1;

/// The figures the window bundles (`src/assets/sub-agents/<id>.webp`); a test keeps both lists
/// equal.
pub const BUILTIN: &[&str] = &[
    "backend-01",
    "backend-02",
    "backend-03",
    "comfyui-01",
    "contracts-01",
    "contracts-02",
    "developer-01",
    "developer-02",
    "developer-03",
    "developer-04",
    "finance-01",
    "finance-02",
    "frontend-01",
    "frontend-02",
    "frontend-03",
    "frontend-04",
    "manager-01",
    "manager-02",
    "manager-03",
    "manager-04",
    "marketing-01",
    "marketing-02",
    "n8n-01",
    "n8n-02",
    "n8n-03",
    "researcher-01",
    "researcher-02",
    "researcher-03",
    "researcher-04",
    "sales-01",
    "sales-02",
    "security-01",
    "security-02",
    "security-03",
    "social-media-01",
    "social-media-02",
    "sysadmin-01",
    "sysadmin-02",
    "tester-01",
    "tester-02",
    "tester-03",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Stored {
    Builtin { id: String },
    Image { file: String },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FigureFile {
    version: u32,
    #[serde(default)]
    figures: BTreeMap<String, Stored>,
}

/// A chosen figure as the window gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FigureView {
    Builtin {
        id: String,
    },
    Image {
        #[serde(rename = "dataUrl")]
        data_url: String,
    },
}

/// What the window asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FigureInput {
    Builtin {
        id: String,
    },
    /// The image file's bytes as Base64.
    Image {
        data: String,
    },
    /// Back to the figure the window derives from the name.
    Default,
}

#[derive(Debug, thiserror::Error)]
pub enum FigureError {
    #[error("Invalid sub-agent name")]
    InvalidName,
    #[error("Unknown figure {0}")]
    UnknownFigure(String),
    #[error("Not a PNG, GIF, WebP or JPEG image")]
    NotAnImage,
    #[error("Image too large: at most {} MiB", MAX_IMAGE_BYTES / 1024 / 1024)]
    TooLarge,
    #[error("Too many figures: at most {MAX_ENTRIES}")]
    TooMany,
    #[error("The clipboard holds no image")]
    NoImage,
    #[error("Clipboard image: {0}")]
    Clipboard(String),
    #[error("{path} cannot be written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl FigureError {
    /// Stable code the window words itself (`backendError.ts`), so it never matches English text.
    fn code(&self) -> Option<&'static str> {
        match self {
            Self::InvalidName => Some("invalid_name"),
            Self::UnknownFigure(_) => Some("unknown_figure"),
            Self::NotAnImage => Some("not_an_image"),
            Self::TooLarge => Some("too_large"),
            Self::TooMany => Some("too_many"),
            Self::NoImage => Some("no_image"),
            Self::Clipboard(_) | Self::Io { .. } | Self::Task(_) => None,
        }
    }
}

/// Errors reach the window as text, `[code] ` first when they have one; none contains image data.
impl Serialize for FigureError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.code() {
            Some(code) => serializer.serialize_str(&format!("[{code}] {self}")),
            None => serializer.serialize_str(&self.to_string()),
        }
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> FigureError + '_ {
    move |source| FigureError::Io {
        path: path.to_owned(),
        source,
    }
}

fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "jpg",
    }
}

/// An own image's file name: its SHA-256 and the extension of its checked type. Nothing else is
/// ever read from the folder, so a name in the JSON file cannot point anywhere else.
fn image_file_ok(file: &str) -> bool {
    let Some((hash, ext)) = file.split_once('.') else {
        return false;
    };
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && ["png", "gif", "webp", "jpg"].contains(&ext)
}

/// The checked image, or why not.
pub fn checked_image(bytes: &[u8]) -> Result<&'static str, FigureError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(FigureError::TooLarge);
    }
    crate::avatar::sniff(bytes).ok_or(FigureError::NotAnImage)
}

/// Scales `image` down so its longer side is at most `side` (box filter, premultiplied alpha so
/// transparent pixels do not darken the edges); a smaller image stays as it is.
pub fn fit(image: RgbaImage, side: usize) -> RgbaImage {
    let longer = image.width.max(image.height);
    if longer <= side || side == 0 {
        return image;
    }
    let width = (image.width * side / longer).max(1);
    let height = (image.height * side / longer).max(1);
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let (y0, y1) = (
            y * image.height / height,
            ((y + 1) * image.height / height).max(y * image.height / height + 1),
        );
        for x in 0..width {
            let (x0, x1) = (
                x * image.width / width,
                ((x + 1) * image.width / width).max(x * image.width / width + 1),
            );
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = (sy * image.width + sx) * 4;
                    let alpha = u64::from(image.rgba[at + 3]);
                    for (channel, total) in sum.iter_mut().take(3).enumerate() {
                        *total += u64::from(image.rgba[at + channel]) * alpha;
                    }
                    sum[3] += alpha;
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            for total in &sum[..3] {
                let value = total.checked_div(sum[3]).unwrap_or(0);
                rgba.push(u8::try_from(value.min(255)).unwrap_or(u8::MAX));
            }
            rgba.push(u8::try_from((sum[3] / count).min(255)).unwrap_or(u8::MAX));
        }
    }
    RgbaImage {
        width,
        height,
        rgba,
    }
}

/// The figures file and the folder of own images. One change at a time.
pub struct FigureStore {
    file: PathBuf,
    images: PathBuf,
    lock: Mutex<()>,
}

impl FigureStore {
    pub fn new(config_dir: &Path, data_dir: &Path) -> Self {
        Self {
            file: config_dir.join(FILE_NAME),
            images: data_dir.join(IMAGE_DIR),
            lock: Mutex::new(()),
        }
    }

    /// The stored choices; a missing or unreadable file is an empty one, entries with an invalid
    /// name or figure are left out.
    fn load(&self) -> FigureFile {
        let parsed: Option<FigureFile> =
            crate::persist::read_regular_capped(&self.file, MAX_FILE_BYTES)
                .ok()
                .flatten()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let mut file = parsed
            .filter(|file| file.version == VERSION)
            .unwrap_or_default();
        file.figures.retain(|name, stored| {
            valid_name(name)
                && match stored {
                    Stored::Builtin { id } => BUILTIN.contains(&id.as_str()),
                    Stored::Image { file } => image_file_ok(file),
                }
        });
        file
    }

    fn save(&self, file: &FigureFile) -> Result<(), FigureError> {
        let text = serde_json::to_vec_pretty(&FigureFile {
            version: VERSION,
            figures: file.figures.clone(),
        })
        .map_err(|error| io_error(&self.file)(io::Error::other(error)))?;
        crate::persist::write_atomic(&self.file, &text).map_err(io_error(&self.file))
    }

    /// Every chosen figure, own images read and checked again; a missing or damaged image is left
    /// out (the window then shows the derived figure).
    pub fn views(&self) -> BTreeMap<String, FigureView> {
        let _guard = crate::lock(&self.lock);
        let mut budget = MAX_TOTAL_IMAGE_BYTES;
        self.load()
            .figures
            .into_iter()
            .filter_map(|(name, stored)| {
                let view = match stored {
                    Stored::Builtin { id } => FigureView::Builtin { id },
                    Stored::Image { file } => {
                        // Never more than the limit from disk, and no link followed.
                        let path = self.images.join(&file);
                        let bytes = crate::persist::read_regular_capped(&path, MAX_IMAGE_BYTES)
                            .ok()
                            .flatten()?;
                        budget = budget.checked_sub(bytes.len())?;
                        let mime = checked_image(&bytes).ok()?;
                        if extension(mime) != file.rsplit('.').next()? {
                            return None;
                        }
                        FigureView::Image {
                            data_url: crate::avatar::data_url(mime, &bytes),
                        }
                    }
                };
                Some((name, view))
            })
            .collect()
    }

    /// Chooses the figure of `name` and returns all of them, as [`Self::views`].
    pub fn set(
        &self,
        name: &str,
        input: FigureInput,
    ) -> Result<BTreeMap<String, FigureView>, FigureError> {
        if !valid_name(name) {
            return Err(FigureError::InvalidName);
        }
        let bytes = match &input {
            FigureInput::Builtin { id } if !BUILTIN.contains(&id.as_str()) => {
                return Err(FigureError::UnknownFigure(id.clone()));
            }
            // Base64 grows by a third; one symbol more than that is too large already.
            FigureInput::Image { data } => Some(
                crate::icon::base64_decode(data, MAX_IMAGE_BYTES.div_ceil(3) * 4 + 4)
                    .ok_or(FigureError::TooLarge)?,
            ),
            FigureInput::Builtin { .. } | FigureInput::Default => None,
        };
        {
            // Stored and assigned under one lock: another change's clean-up could otherwise
            // remove the new image file before it is assigned.
            let _guard = crate::lock(&self.lock);
            let stored = match (input, bytes) {
                (FigureInput::Builtin { id }, _) => Some(Stored::Builtin { id }),
                (_, Some(bytes)) => Some(self.store_image(&bytes)?),
                _ => None,
            };
            self.assign(name, stored)?;
        }
        Ok(self.views())
    }

    /// [`Self::set`] with an image from the clipboard, scaled down to [`PASTE_SIDE`] and saved as
    /// PNG.
    pub fn set_pasted(
        &self,
        name: &str,
        image: RgbaImage,
    ) -> Result<BTreeMap<String, FigureView>, FigureError> {
        if !valid_name(name) {
            return Err(FigureError::InvalidName);
        }
        let image = fit(image, PASTE_SIDE);
        let png = crate::paste_image::encode(&image)
            .map_err(|error| FigureError::Clipboard(error.to_string()))?;
        {
            let _guard = crate::lock(&self.lock);
            let stored = self.store_image(&png)?;
            self.assign(name, Some(stored))?;
        }
        Ok(self.views())
    }

    fn store_image(&self, bytes: &[u8]) -> Result<Stored, FigureError> {
        let mime = checked_image(bytes)?;
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let file = format!("{hash}.{}", extension(mime));
        let path = self.images.join(&file);
        if !path.is_file() {
            crate::persist::write_atomic(&path, bytes).map_err(io_error(&path))?;
        }
        Ok(Stored::Image { file })
    }

    /// Writes the choice; the caller holds `lock` and checked `name`.
    fn assign(&self, name: &str, stored: Option<Stored>) -> Result<(), FigureError> {
        let mut file = self.load();
        match stored {
            Some(stored) => {
                if !file.figures.contains_key(name) && file.figures.len() >= MAX_ENTRIES {
                    return Err(FigureError::TooMany);
                }
                file.figures.insert(name.to_owned(), stored);
            }
            None => {
                file.figures.remove(name);
            }
        }
        self.save(&file)?;
        self.remove_unused(&file);
        Ok(())
    }

    /// Own images no agent uses any more; best effort.
    fn remove_unused(&self, file: &FigureFile) {
        let Ok(entries) = fs::read_dir(&self.images) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let used = file
                .figures
                .values()
                .any(|stored| matches!(stored, Stored::Image { file } if *file == name));
            if image_file_ok(&name) && !used {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const GIF: &[u8] = b"GIF89a\x01\0\x01\0";

    fn store(temp: &TempDir) -> FigureStore {
        FigureStore::new(&temp.path().join("config"), &temp.path().join("data"))
    }

    #[test]
    fn builtin_ids_match_the_bundled_files() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("src")
            .join("assets")
            .join("sub-agents");
        let mut bundled: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".webp").map(str::to_owned)
            })
            .collect();
        bundled.sort();
        assert_eq!(bundled, BUILTIN);
    }

    #[test]
    fn builtin_figures_are_chosen_and_reset() {
        let temp = TempDir::new("figures-builtin");
        let store = store(&temp);
        assert!(store.views().is_empty());
        let views = store
            .set(
                "backend",
                FigureInput::Builtin {
                    id: "tester-01".into(),
                },
            )
            .unwrap();
        assert_eq!(
            views.get("backend"),
            Some(&FigureView::Builtin {
                id: "tester-01".into()
            })
        );
        assert!(matches!(
            store.set("backend", FigureInput::Builtin { id: "../x".into() }),
            Err(FigureError::UnknownFigure(_))
        ));
        assert!(matches!(
            store.set("Bad Name", FigureInput::Default),
            Err(FigureError::InvalidName)
        ));
        assert!(store
            .set("backend", FigureInput::Default)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn own_images_are_checked_stored_and_cleaned_up() {
        let temp = TempDir::new("figures-image");
        let store = store(&temp);
        let data = crate::icon::base64(GIF);
        let views = store.set("tester", FigureInput::Image { data }).unwrap();
        assert_eq!(
            views.get("tester"),
            Some(&FigureView::Image {
                data_url: crate::avatar::data_url("image/gif", GIF)
            })
        );
        let images = temp.path().join("data").join(IMAGE_DIR);
        assert_eq!(fs::read_dir(&images).unwrap().count(), 1);

        // An SVG or any other text is refused.
        let svg = crate::icon::base64(b"<svg xmlns='http://www.w3.org/2000/svg'/>");
        assert!(matches!(
            store.set("tester", FigureInput::Image { data: svg }),
            Err(FigureError::NotAnImage)
        ));
        let large = crate::icon::base64(&[0u8; MAX_IMAGE_BYTES + 1]);
        assert!(matches!(
            store.set("tester", FigureInput::Image { data: large }),
            Err(FigureError::TooLarge)
        ));

        // A new image replaces the old one, whose file goes.
        let data = crate::icon::base64(PNG);
        store.set("tester", FigureInput::Image { data }).unwrap();
        let left: Vec<String> = fs::read_dir(&images)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(left.len(), 1);
        assert!(left[0].ends_with(".png"));

        // A damaged image is left out instead of reaching the window.
        fs::write(images.join(&left[0]), b"<svg/>").unwrap();
        assert!(store.views().is_empty());
    }

    #[test]
    fn oversized_files_and_answers_are_cut() {
        let temp = TempDir::new("figures-limits");
        let store = store(&temp);
        let config = temp.path().join("config");
        fs::create_dir_all(&config).unwrap();
        // A figures file above its limit reads as empty.
        let mut big = String::from(
            r#"{"version":1,"figures":{"a":{"kind":"builtin","id":"tester-01"}},"x":""#,
        );
        big.push_str(&"y".repeat(MAX_FILE_BYTES));
        big.push_str("\"}");
        fs::write(config.join(FILE_NAME), big).unwrap();
        assert!(store.views().is_empty());
        fs::remove_file(config.join(FILE_NAME)).unwrap();

        // An image file grown past the limit on disk is left out.
        let data = crate::icon::base64(GIF);
        store.set("tester", FigureInput::Image { data }).unwrap();
        let images = temp.path().join("data").join(IMAGE_DIR);
        let file = fs::read_dir(&images)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut grown = GIF.to_vec();
        grown.resize(MAX_IMAGE_BYTES + 1, 0);
        fs::write(&file, grown).unwrap();
        assert!(store.views().is_empty());

        // More images than one answer takes: the rest falls back to the derived figure.
        let count = MAX_TOTAL_IMAGE_BYTES / MAX_IMAGE_BYTES + 2;
        for index in 0..count {
            let mut image = GIF.to_vec();
            image.resize(MAX_IMAGE_BYTES - 16, u8::try_from(index).unwrap());
            let data = crate::icon::base64(&image);
            store
                .set(&format!("agent-{index}"), FigureInput::Image { data })
                .unwrap();
        }
        let views = store.views();
        assert!(views.len() < count);
        assert!(views.len() >= MAX_TOTAL_IMAGE_BYTES / MAX_IMAGE_BYTES);
    }

    #[test]
    fn the_file_cannot_point_outside_the_image_folder() {
        let temp = TempDir::new("figures-file");
        let store = store(&temp);
        fs::create_dir_all(temp.path().join("config")).unwrap();
        fs::write(
            temp.path().join("config").join(FILE_NAME),
            r#"{"version":1,"figures":{"a":{"kind":"image","file":"../../secret.png"},"b":{"kind":"builtin","id":"manager-02"}}}"#,
        )
        .unwrap();
        let views = store.views();
        assert_eq!(views.len(), 1);
        assert!(views.contains_key("b"));
        fs::write(temp.path().join("config").join(FILE_NAME), "not json").unwrap();
        assert!(store.views().is_empty());
    }

    #[test]
    fn pasted_images_are_scaled_down_and_saved_as_png() {
        let temp = TempDir::new("figures-paste");
        let store = store(&temp);
        let image = RgbaImage {
            width: 1024,
            height: 512,
            rgba: vec![255; 1024 * 512 * 4],
        };
        let views = store.set_pasted("manager", image).unwrap();
        let Some(FigureView::Image { data_url }) = views.get("manager") else {
            panic!("no image");
        };
        assert!(data_url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn an_invalid_name_stores_no_image() {
        let temp = TempDir::new("figures-invalid-name");
        let store = store(&temp);
        let data = crate::icon::base64(PNG);
        assert!(matches!(
            store.set("Bad Name", FigureInput::Image { data }),
            Err(FigureError::InvalidName)
        ));
        assert!(!temp.path().join("data").join(IMAGE_DIR).exists());
    }

    #[test]
    fn errors_carry_a_stable_code_for_the_window() {
        let text = |error: FigureError| serde_json::to_value(error).unwrap();
        assert_eq!(
            text(FigureError::NoImage),
            "[no_image] The clipboard holds no image"
        );
        assert_eq!(
            text(FigureError::NotAnImage),
            "[not_an_image] Not a PNG, GIF, WebP or JPEG image"
        );
        assert!(text(FigureError::TooLarge)
            .as_str()
            .unwrap()
            .starts_with("[too_large] "));
        assert_eq!(
            text(FigureError::Clipboard("busy".into())),
            "Clipboard image: busy"
        );
    }

    #[test]
    fn fitting_keeps_the_aspect_and_alpha() {
        let mut rgba = Vec::new();
        for index in 0..16 {
            // Left half opaque red, right half transparent.
            let opaque = index % 4 < 2;
            rgba.extend_from_slice(if opaque {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
        let small = fit(
            RgbaImage {
                width: 4,
                height: 4,
                rgba,
            },
            2,
        );
        assert_eq!((small.width, small.height), (2, 2));
        assert_eq!(&small.rgba[..8], &[255, 0, 0, 255, 0, 0, 0, 0]);
        let same = fit(
            RgbaImage {
                width: 3,
                height: 1,
                rgba: vec![1; 12],
            },
            256,
        );
        assert_eq!(same.width, 3);
    }
}
