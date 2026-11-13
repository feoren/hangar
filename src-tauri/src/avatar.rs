//! The signed-in person's profile picture (contract: docs/wiki/pages/components/hangar-account-api.md).
//!
//! `/me` names the picture (`avatar_url`, `avatar_etag`); the backend downloads it with the
//! access token, checks size and type, keeps one copy in the app's local data folder and hands the
//! window a `data:` URL. The window never sees the remote address (its CSP allows no remote
//! images) or a token. Sign-out removes the copy.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Far above a 256x256 WebP or PNG; anything larger is refused.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Folder in the app's local data folder.
pub const CACHE_DIR: &str = "account-avatar";
const META_FILE: &str = "avatar.json";
const IMAGE_FILE: &str = "avatar.img";
/// What the backend asks for and accepts.
pub const ACCEPT: &str = "image/webp, image/png, image/jpeg";

/// The checked type of an image: the `Content-Type` the server sent (parameters ignored), and
/// only when the bytes start like that type, so a mislabeled answer never reaches the window.
pub fn image_type(content_type: Option<&str>, bytes: &[u8]) -> Option<&'static str> {
    let essence = content_type?
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let (mime, fits) = match essence.as_str() {
        "image/png" => ("image/png", bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
        "image/jpeg" => ("image/jpeg", bytes.starts_with(&[0xFF, 0xD8, 0xFF])),
        "image/webp" => (
            "image/webp",
            bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        ),
        _ => return None,
    };
    (fits && bytes.len() <= MAX_BYTES).then_some(mime)
}

/// `data:<mime>;base64,...` for the window.
pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", crate::icon::base64(bytes))
}

/// An `avatar_etag` as the contract defines it: the picture's SHA-256 as 64 hex digits.
pub fn is_etag(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The value of an `ETag` header without quotes and weak prefix, when it is a valid etag.
pub fn etag_from_header(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix("W/").unwrap_or(value);
    let value = value.trim_matches('"');
    is_etag(value).then(|| value.to_ascii_lowercase())
}

/// The picture as cached, checked again when read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached {
    pub etag: Option<String>,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

impl Cached {
    pub fn data_url(&self) -> String {
        data_url(self.mime, &self.bytes)
    }
}

#[derive(Serialize, Deserialize)]
struct Meta {
    /// Server and account the picture belongs to; another one never reuses it.
    base: String,
    user: String,
    etag: Option<String>,
    mime: String,
}

/// One cached picture: the last signed-in person's.
#[derive(Debug, Clone)]
pub struct AvatarCache {
    dir: PathBuf,
}

impl AvatarCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The picture of `user` on `base`, if one is cached and still a valid image.
    pub fn load(&self, base: &str, user: &str) -> Option<Cached> {
        let meta = std::fs::read(self.dir.join(META_FILE)).ok()?;
        let meta: Meta = serde_json::from_slice(&meta).ok()?;
        if meta.base != base || meta.user != user {
            return None;
        }
        let bytes = std::fs::read(self.dir.join(IMAGE_FILE)).ok()?;
        let mime = image_type(Some(&meta.mime), &bytes)?;
        let etag = meta.etag.filter(|etag| is_etag(etag));
        Some(Cached { etag, mime, bytes })
    }

    /// Replaces the cached picture; the image first, so a crash in between leaves metadata that
    /// fails the check of the new bytes or names the old ones' etag (both read as a miss or a
    /// mismatch and lead to a new download).
    pub fn store(&self, base: &str, user: &str, picture: &Cached) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        crate::persist::write_atomic(&self.dir.join(IMAGE_FILE), &picture.bytes)?;
        let meta = Meta {
            base: base.to_owned(),
            user: user.to_owned(),
            etag: picture.etag.clone(),
            mime: picture.mime.to_owned(),
        };
        let text = serde_json::to_vec(&meta).map_err(std::io::Error::other)?;
        crate::persist::write_atomic(&self.dir.join(META_FILE), &text)
    }

    /// Removes the cached picture (sign-out, or the person removed it on the website).
    pub fn clear(&self) {
        for name in [META_FILE, IMAGE_FILE] {
            match std::fs::remove_file(self.dir.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => eprintln!("hangar: cached profile picture not removed: {error}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const WEBP: &[u8] = b"RIFF\x10\0\0\0WEBPVP8 ";
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
    const ETAG: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    #[test]
    fn only_png_jpeg_and_webp_that_look_like_it_pass() {
        assert_eq!(image_type(Some("image/png"), PNG), Some("image/png"));
        assert_eq!(
            image_type(Some("Image/WebP; charset=binary"), WEBP),
            Some("image/webp")
        );
        assert_eq!(image_type(Some("image/jpeg"), JPEG), Some("image/jpeg"));
        // The header decides the type; the bytes must agree with it.
        assert_eq!(image_type(Some("image/png"), WEBP), None);
        assert_eq!(image_type(None, PNG), None);
        for refused in [
            "image/svg+xml",
            "text/html",
            "image/gif",
            "application/json",
        ] {
            assert_eq!(image_type(Some(refused), PNG), None, "{refused}");
        }
        let mut large = PNG.to_vec();
        large.resize(MAX_BYTES + 1, 0);
        assert_eq!(image_type(Some("image/png"), &large), None);
        large.truncate(MAX_BYTES);
        assert_eq!(image_type(Some("image/png"), &large), Some("image/png"));
    }

    #[test]
    fn etags_are_sha256_hex() {
        assert!(is_etag(ETAG));
        assert!(!is_etag("abc"));
        assert!(!is_etag(&"g".repeat(64)));
        assert_eq!(
            etag_from_header(&format!("\"{ETAG}\"")).as_deref(),
            Some(ETAG)
        );
        assert_eq!(
            etag_from_header(&format!("W/\"{ETAG}\"")).as_deref(),
            Some(ETAG)
        );
        assert_eq!(etag_from_header("\"short\""), None);
    }

    #[test]
    fn data_urls_carry_the_checked_type() {
        assert_eq!(data_url("image/png", b"ab"), "data:image/png;base64,YWI=");
    }

    #[test]
    fn the_cache_belongs_to_one_person_on_one_server() {
        let temp = crate::test_support::TempDir::new("avatar-cache");
        let cache = AvatarCache::new(temp.path().join(CACHE_DIR));
        assert_eq!(cache.load("https://a.test/api/v1", "7"), None);
        let picture = Cached {
            etag: Some(ETAG.to_owned()),
            mime: "image/png",
            bytes: PNG.to_vec(),
        };
        cache.store("https://a.test/api/v1", "7", &picture).unwrap();
        assert_eq!(cache.load("https://a.test/api/v1", "7"), Some(picture));
        assert_eq!(cache.load("https://a.test/api/v1", "8"), None);
        assert_eq!(cache.load("https://b.test/api/v1", "7"), None);
        // A damaged image is a miss, not a broken picture.
        std::fs::write(cache.dir().join(IMAGE_FILE), b"not an image").unwrap();
        assert_eq!(cache.load("https://a.test/api/v1", "7"), None);
        cache.clear();
        assert!(!cache.dir().join(META_FILE).exists());
        cache.clear();
    }
}
