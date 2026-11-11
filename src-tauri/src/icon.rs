//! Program icons for the "New session" list.
//!
//! Windows: the first icon from the resources of the resolved file, 32×32 with alpha channel,
//! as a PNG data URL. Script shims (`.cmd`, `.bat`, `.ps1`, for example from npm) and files
//! without an icon of their own yield `None`; the frontend then shows its letter badge. Because
//! `PrivateExtractIconsW` only reads the file's resources, the generic Windows icon for
//! programs without an icon of their own never comes back in the first place.
//! macOS and Linux: always `None` for now (app bundles and `.desktop` files do not belong to
//! a program on the PATH; that would need a mapping of its own).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::agent_catalog::Agent;
use crate::filecache::{FileCache, FileStamp};

pub const SIZE: u32 = 32;

pub type IconCache = FileCache<FileStamp, Option<String>>;

/// profileId → data URL or `None`.
pub fn icons(installed: &[(Agent, PathBuf)], cache: &IconCache) -> HashMap<String, Option<String>> {
    installed
        .iter()
        .map(|(agent, program)| (agent.id().to_owned(), cached_icon(program, cache)))
        .collect()
}

fn cached_icon(program: &Path, cache: &IconCache) -> Option<String> {
    if is_script_shim(program) {
        return None;
    }
    let key = FileStamp::of(program)?;
    if let Some(icon) = cache.get(&key) {
        return icon;
    }
    let icon = program_icon(program);
    cache.insert(key, icon.clone());
    icon
}

/// Data URL of the program icon or `None`.
pub fn program_icon(program: &Path) -> Option<String> {
    if is_script_shim(program) {
        return None;
    }
    let png = icon_png(program)?;
    Some(format!("data:image/png;base64,{}", base64(&png)))
}

/// A script carries no icon; the interpreter's file would be the wrong one.
pub fn is_script_shim(program: &Path) -> bool {
    program
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ["cmd", "bat", "ps1"].contains(&ext.to_ascii_lowercase().as_str()))
}

/// PNG data of the program icon, [`SIZE`] × [`SIZE`] pixels.
#[cfg(windows)]
pub fn icon_png(program: &Path) -> Option<Vec<u8>> {
    // An execution alias (Store app) is an empty file; the icon lives in the package's executable.
    let file = crate::exec_alias::real_file(program);
    let (width, height, rgba) = windows::icon_rgba(&file, SIZE)?;
    encode_png(width, height, &rgba)
}

#[cfg(not(windows))]
pub fn icon_png(_program: &Path) -> Option<Vec<u8>> {
    None
}

#[cfg_attr(not(windows), allow(dead_code))]
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

/// Standard Base64 with padding (RFC 4648). Written by hand because only this place needs it.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
    };

    /// Largest accepted edge length; guards against absurd buffer sizes.
    const MAX_EDGE: i32 = 256;

    struct Icon(HICON);
    impl Drop for Icon {
        fn drop(&mut self) {
            // SAFETY: The handle comes from PrivateExtractIconsW and is released only here.
            unsafe { DestroyIcon(self.0) };
        }
    }

    struct Bitmap(HBITMAP);
    impl Drop for Bitmap {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: GetIconInfo creates copies of the bitmaps that the caller must delete;
                // they are not selected into any DC.
                unsafe { DeleteObject(self.0) };
            }
        }
    }

    struct MemoryDc(HDC);
    impl Drop for MemoryDc {
        fn drop(&mut self) {
            // SAFETY: The DC comes from CreateCompatibleDC and is deleted only here.
            unsafe { DeleteDC(self.0) };
        }
    }

    /// First icon of the file at `size` × `size`, as RGBA without premultiplication.
    pub fn icon_rgba(program: &Path, size: u32) -> Option<(u32, u32, Vec<u8>)> {
        let edge = i32::try_from(size).ok()?;
        let wide: Vec<u16> = program
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut handle: HICON = ptr::null_mut();
        let mut id = 0u32;
        // SAFETY: `wide` is null-terminated and lives for the whole call; `handle` and `id`
        // have room for exactly the one requested icon (nicons = 1).
        let count = unsafe {
            PrivateExtractIconsW(wide.as_ptr(), 0, edge, edge, &mut handle, &mut id, 1, 0)
        };
        // 0: no icon in the file; u32::MAX: file not found or not readable.
        if count == 0 || count == u32::MAX || handle.is_null() {
            return None;
        }
        let icon = Icon(handle);

        // SAFETY: An all-zero bit pattern is valid for this POD struct.
        let mut info: ICONINFO = unsafe { std::mem::zeroed() };
        // SAFETY: `icon.0` is a valid icon, `info` a writable output buffer.
        if unsafe { GetIconInfo(icon.0, &mut info) } == 0 {
            return None;
        }
        let color = Bitmap(info.hbmColor);
        let mask = Bitmap(info.hbmMask);
        // Monochrome icons (mask only) are practically extinct today; no badge fallback needed.
        if color.0.is_null() {
            return None;
        }

        // SAFETY: An all-zero bit pattern is valid for BITMAP.
        let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };
        // SAFETY: Size and pointer describe exactly `bitmap`.
        let written = unsafe {
            GetObjectW(
                color.0,
                std::mem::size_of::<BITMAP>() as i32,
                (&mut bitmap as *mut BITMAP).cast::<c_void>(),
            )
        };
        let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
        if written == 0 || !(1..=MAX_EDGE).contains(&width) || !(1..=MAX_EDGE).contains(&height) {
            return None;
        }

        // SAFETY: Null means a memory DC compatible with the screen.
        let dc = MemoryDc(unsafe { CreateCompatibleDC(ptr::null_mut()) });
        if dc.0.is_null() {
            return None;
        }
        let mut pixels = read_bgra(&dc, &color, width, height)?;
        // Older icons have no alpha channel; then the AND mask determines transparency.
        if pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            let mask_pixels = read_bgra(&dc, &mask, width, height)?;
            for (pixel, mask) in pixels
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(mask_pixels.as_chunks::<4>().0)
            {
                pixel[3] = if mask[0] == 0 { 255 } else { 0 };
            }
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Some((width.unsigned_abs(), height.unsigned_abs(), pixels))
    }

    /// Bitmap as 32-bit BGRA, top to bottom.
    fn read_bgra(dc: &MemoryDc, bitmap: &Bitmap, width: i32, height: i32) -> Option<Vec<u8>> {
        if bitmap.0.is_null() {
            return None;
        }
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative: rows top to bottom, as PNG expects them.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = height.unsigned_abs();
        let mut pixels = vec![0u8; (width.unsigned_abs() * lines * 4) as usize];
        // SAFETY: The buffer holds width × height × 4 bytes, exactly what the 32-bit format
        // requires without row padding. `header` describes this format and lives for the whole
        // call. The bitmap is not selected into any DC, as GetDIBits requires.
        let copied = unsafe {
            GetDIBits(
                dc.0,
                bitmap.0,
                0,
                lines,
                pixels.as_mut_ptr().cast::<c_void>(),
                &mut header,
                DIB_RGB_COLORS,
            )
        };
        (copied == height).then_some(pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_test_vectors_rfc4648() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in cases {
            assert_eq!(base64(input.as_bytes()), expected, "{input:?}");
        }
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn script_shims_have_no_icon() {
        for name in ["codex.cmd", "x.BAT", "y.ps1", r"C:\npm\gemini.CMD"] {
            assert!(is_script_shim(Path::new(name)), "{name}");
            assert_eq!(program_icon(Path::new(name)), None);
        }
        for name in ["claude.exe", "claude", "my.cmd.exe"] {
            assert!(!is_script_shim(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn png_encoding_has_signature_and_size() {
        let rgba = vec![0x80u8; 2 * 3 * 4];
        let png = encode_png(2, 3, &rgba).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR: width and height as big-endian from byte 16.
        assert_eq!(&png[16..24], &[0, 0, 0, 2, 0, 0, 0, 3]);
        // A wrong buffer size is an error, not a panic.
        assert_eq!(encode_png(2, 2, &rgba), None);
    }

    #[test]
    fn missing_file_yields_no_icon() {
        assert_eq!(program_icon(Path::new("does-not-exist.exe")), None);
        let cache = IconCache::default();
        assert_eq!(cached_icon(Path::new("does-not-exist.exe"), &cache), None);
    }
}
