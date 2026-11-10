//! Reading the clipboard's image for a paste (paste_image.rs saves it).
//!
//! The clipboard is filled by any program, so its image is untrusted input: a few kilobytes of PNG
//! can claim 65535x65535 pixels at 16 bit, and a decoder that trusts the header allocates
//! gigabytes and aborts Hangar with every session in it. arboard 3.7 decodes on Windows with
//! `image`'s `Limits::no_limits()`, so on Windows Hangar reads the raw formats itself and checks
//! the size in the header before any pixel is decoded.

use std::io::Cursor;

use crate::paste_image::{check_dimensions, PasteImageError, RgbaImage, MAX_PIXELS};

/// Upper limit of the raw clipboard data: an uncompressed 32 bit bitmap at [`MAX_PIXELS`] plus
/// room for headers and a color profile. A PNG of an allowed image is always smaller.
pub const MAX_DATA_BYTES: usize = MAX_PIXELS * 4 + 1024 * 1024;
/// Allocation budget of the PNG decoder for everything besides the pixel buffer Hangar allocates
/// itself (text and profile chunks, a row of 16 bit samples).
const PNG_DECODER_BYTES: usize = MAX_PIXELS * 8;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
/// `BITMAPINFOHEADER`; larger headers (V3, V4, V5) start with the same fields.
const INFO_HEADER_BYTES: usize = 40;
/// From `BITMAPV3INFOHEADER` on, the header carries the red, green, blue and alpha masks itself.
const MASKS_HEADER_BYTES: usize = 56;
/// From `BITMAPV2INFOHEADER` on, the red, green and blue masks lie inside the header.
const RGB_MASKS_HEADER_BYTES: usize = 52;

/// The clipboard's image, `None` when it holds none (text, files or nothing).
///
/// Windows: the registered "PNG" format first (screenshot tools put it there, it keeps alpha),
/// then `CF_DIBV5`, then `CF_DIB`. Windows synthesizes the bitmap formats from any bitmap on the
/// clipboard, so without one of these there is no image; arboard would read the same two formats
/// and is not needed as a fallback.
#[cfg(windows)]
pub fn read() -> Result<Option<RgbaImage>, PasteImageError> {
    use clipboard_win::{formats, raw};
    use std::num::{NonZeroU32, NonZeroUsize};

    enum Kind {
        Png,
        Dib,
    }
    let clipboard_error =
        |error: clipboard_win::ErrorCode| PasteImageError::Clipboard(error.to_string());
    let (kind, data) = {
        // Closed again at the end of this block, before decoding: other programs wait while
        // Hangar holds the clipboard open.
        let _open = clipboard_win::Clipboard::new_attempts(10).map_err(clipboard_error)?;
        let png = raw::register_format("PNG")
            .map(NonZeroU32::get)
            .filter(|&id| raw::is_format_avail(id));
        let (kind, format) = match png {
            Some(id) => (Kind::Png, id),
            None if raw::is_format_avail(formats::CF_DIBV5) => (Kind::Dib, formats::CF_DIBV5),
            None if raw::is_format_avail(formats::CF_DIB) => (Kind::Dib, formats::CF_DIB),
            None => return Ok(None),
        };
        let size = raw::size(format)
            .map(NonZeroUsize::get)
            .ok_or_else(|| PasteImageError::Clipboard("image data unavailable".to_owned()))?;
        check_data_size(size)?;
        let mut data = Vec::new();
        raw::get_vec(format, &mut data).map_err(clipboard_error)?;
        (kind, data)
    };
    let image = match kind {
        Kind::Png => decode_png(&data)?,
        Kind::Dib => decode_dib(&data)?,
    };
    Ok(Some(image))
}

/// The clipboard's image, `None` when it holds none (text, files or nothing).
///
/// Elsewhere arboard decodes the image (X11 and Wayland PNG, macOS TIFF) through `image`'s
/// `ImageReader`, whose default `Limits` cap each allocation at 512 MiB (read in arboard 3.7.0,
/// `src/platform/linux/x11.rs`, `src/platform/linux/wayland.rs`, `src/platform/osx.rs`, and
/// image 0.25.10, `src/io/limits.rs` and `ImageReader::new`). A bomb therefore fails instead of
/// aborting; its pixel count is checked again before saving. The raw transfer from the other
/// program has no limit of Hangar's own there. [UNVERIFIED] on a real X11, Wayland or macOS
/// clipboard; Windows is the verified platform.
///
/// Must not run on the main thread: on Linux the clipboard libraries may deadlock there.
#[cfg(not(windows))]
pub fn read() -> Result<Option<RgbaImage>, PasteImageError> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| PasteImageError::Clipboard(error.to_string()))?;
    match clipboard.get_image() {
        Ok(image) => Ok(Some(RgbaImage {
            width: image.width,
            height: image.height,
            rgba: image.bytes.into_owned(),
        })),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(error) => Err(PasteImageError::Clipboard(error.to_string())),
    }
}

pub fn check_data_size(bytes: usize) -> Result<(), PasteImageError> {
    if bytes > MAX_DATA_BYTES {
        return Err(PasteImageError::DataTooLarge {
            bytes,
            max: MAX_DATA_BYTES,
        });
    }
    Ok(())
}

/// Decodes a PNG to RGBA, after checking the size in its header.
pub fn decode_png(data: &[u8]) -> Result<RgbaImage, PasteImageError> {
    let (width, height) = png_dimensions(data)?;
    check_dimensions(width, height)?;
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(data),
        png::Limits {
            bytes: PNG_DECODER_BYTES,
        },
    );
    // Palette, low bit depths and tRNS become 8 bit gray or RGB, with alpha where there is one.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let decode_error = |error: png::DecodingError| PasteImageError::Malformed(error.to_string());
    let mut reader = decoder.read_info().map_err(decode_error)?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| PasteImageError::Malformed("PNG size overflows".to_owned()))?;
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer).map_err(decode_error)?;
    buffer.truncate(info.buffer_size());
    if info.bit_depth != png::BitDepth::Eight {
        return Err(PasteImageError::Unsupported(format!(
            "PNG with {:?} bit samples",
            info.bit_depth
        )));
    }
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => widen::<3>(&buffer, |[r, g, b]| [r, g, b, u8::MAX]),
        png::ColorType::GrayscaleAlpha => {
            widen::<2>(&buffer, |[gray, alpha]| [gray, gray, gray, alpha])
        }
        png::ColorType::Grayscale => widen::<1>(&buffer, |[gray]| [gray, gray, gray, u8::MAX]),
        png::ColorType::Indexed => {
            return Err(PasteImageError::Unsupported(
                "PNG with a palette".to_owned(),
            ));
        }
    };
    Ok(RgbaImage {
        width,
        height,
        rgba,
    })
}

/// Width and height from the IHDR chunk, which the PNG format requires right after the
/// signature.
fn png_dimensions(data: &[u8]) -> Result<(usize, usize), PasteImageError> {
    let malformed = || PasteImageError::Malformed("no PNG header".to_owned());
    if data.len() < 24 || !data.starts_with(PNG_SIGNATURE) || &data[12..16] != b"IHDR" {
        return Err(malformed());
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    let width = usize::try_from(width).map_err(|_| malformed())?;
    let height = usize::try_from(height).map_err(|_| malformed())?;
    Ok((width, height))
}

fn widen<const N: usize>(samples: &[u8], pixel: impl Fn([u8; N]) -> [u8; 4]) -> Vec<u8> {
    let (pixels, _) = samples.as_chunks::<N>();
    pixels.iter().flat_map(|&samples| pixel(samples)).collect()
}

/// Channel masks of a 32 bit bitmap; a zero mask means the channel is missing.
#[derive(Clone, Copy)]
struct Masks {
    red: u32,
    green: u32,
    blue: u32,
    alpha: u32,
}

/// Decodes a `CF_DIB` or `CF_DIBV5` (a bitmap info header, optional masks and colors, then the
/// rows) to RGBA, after checking the size in its header. Screenshots arrive as 24 or 32 bit;
/// palettes, 16 bit and compressed bitmaps are refused rather than decoded by a general decoder.
pub fn decode_dib(data: &[u8]) -> Result<RgbaImage, PasteImageError> {
    let header_bytes = usize::try_from(read_u32(data, 0)?).unwrap_or(usize::MAX);
    if header_bytes < INFO_HEADER_BYTES || header_bytes > data.len() {
        return Err(PasteImageError::Malformed(format!(
            "bitmap header of {header_bytes} bytes"
        )));
    }
    let width = read_i32(data, 4)?;
    let height = read_i32(data, 8)?;
    let bit_count = read_u16(data, 14)?;
    let compression = read_u32(data, 16)?;
    let colors_used = read_u32(data, 32)?;
    let columns = usize::try_from(width).unwrap_or(0);
    let rows = usize::try_from(height.unsigned_abs()).unwrap_or(0);
    check_dimensions(columns, rows)?;

    let header_masks = header_bytes >= MASKS_HEADER_BYTES;
    let (masks, mut offset) = match (bit_count, compression) {
        (24, BI_RGB) => (None, header_bytes),
        // The fourth byte is reserved unless a V3 or later header names an alpha mask.
        (32, BI_RGB) => {
            let alpha = if header_masks { read_u32(data, 52)? } else { 0 };
            let masks = Masks {
                red: 0x00ff_0000,
                green: 0x0000_ff00,
                blue: 0x0000_00ff,
                alpha,
            };
            (Some(masks), header_bytes)
        }
        (32, BI_BITFIELDS) => {
            // A plain info header is followed by three masks; later headers hold four.
            let masks = Masks {
                red: read_u32(data, 40)?,
                green: read_u32(data, 44)?,
                blue: read_u32(data, 48)?,
                alpha: if header_masks { read_u32(data, 52)? } else { 0 },
            };
            let offset = if header_bytes >= RGB_MASKS_HEADER_BYTES {
                header_bytes
            } else {
                header_bytes + 12
            };
            (Some(masks), offset)
        }
        _ => {
            return Err(PasteImageError::Unsupported(format!(
                "bitmap with {bit_count} bit and compression {compression}"
            )));
        }
    };
    // channel() scales by the number of mask bits; a mask with gaps would not fit that.
    if let Some(m) = masks {
        if ![m.red, m.green, m.blue, m.alpha]
            .into_iter()
            .all(contiguous)
        {
            return Err(PasteImageError::Unsupported(
                "bitmap with non-contiguous channel masks".to_owned(),
            ));
        }
    }
    let overflow = || PasteImageError::Malformed("bitmap size overflows".to_owned());
    let colors = usize::try_from(colors_used).map_err(|_| overflow())?;
    offset = colors
        .checked_mul(4)
        .and_then(|bytes| offset.checked_add(bytes))
        .ok_or_else(overflow)?;
    let bytes_per_pixel = usize::from(bit_count / 8);
    let row_bytes = columns * bytes_per_pixel;
    let stride = row_bytes.next_multiple_of(4);
    let end = stride
        .checked_mul(rows)
        .and_then(|bytes| bytes.checked_add(offset))
        .ok_or_else(overflow)?;
    if data.len() < end {
        return Err(PasteImageError::Malformed(format!(
            "bitmap needs {end} bytes, has {}",
            data.len()
        )));
    }

    let mut rgba = vec![0; columns * rows * 4];
    let (target_rows, _) = rgba.as_chunks_mut::<4>();
    for (row, target) in target_rows.chunks_exact_mut(columns).enumerate() {
        // A positive height means the bottom row comes first.
        let source_row = if height > 0 { rows - 1 - row } else { row };
        let start = offset + source_row * stride;
        let source = &data[start..start + row_bytes];
        match masks {
            None => {
                let (pixels, _) = source.as_chunks::<3>();
                for (pixel, [blue, green, red]) in target.iter_mut().zip(pixels) {
                    *pixel = [*red, *green, *blue, u8::MAX];
                }
            }
            Some(masks) => {
                let (pixels, _) = source.as_chunks::<4>();
                for (pixel, bytes) in target.iter_mut().zip(pixels) {
                    let value = u32::from_le_bytes(*bytes);
                    *pixel = [
                        channel(value, masks.red, 0),
                        channel(value, masks.green, 0),
                        channel(value, masks.blue, 0),
                        channel(value, masks.alpha, u8::MAX),
                    ];
                }
            }
        }
    }
    Ok(RgbaImage {
        width: columns,
        height: rows,
        rgba,
    })
}

/// One channel of a 32 bit pixel, scaled to 8 bits; `missing` when the mask is zero.
fn channel(value: u32, mask: u32, missing: u8) -> u8 {
    if mask == 0 {
        return missing;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).count_ones();
    // u64, so the scaling cannot overflow even for a mask decode_dib would reject.
    let sample = u64::from((value & mask) >> shift);
    let scaled = if bits >= 8 {
        sample >> (bits - 8)
    } else {
        sample * 255 / ((1u64 << bits) - 1)
    };
    u8::try_from(scaled).unwrap_or(u8::MAX)
}

/// The set bits of `mask` form one run (or there are none).
fn contiguous(mask: u32) -> bool {
    if mask == 0 {
        return true;
    }
    let run = mask >> mask.trailing_zeros();
    run & run.wrapping_add(1) == 0
}

fn read_bytes<const N: usize>(data: &[u8], at: usize) -> Result<[u8; N], PasteImageError> {
    data.get(at..at + N)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| PasteImageError::Malformed("bitmap header cut short".to_owned()))
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, PasteImageError> {
    read_bytes(data, at).map(u16::from_le_bytes)
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, PasteImageError> {
    read_bytes(data, at).map(u32::from_le_bytes)
}

fn read_i32(data: &[u8], at: usize) -> Result<i32, PasteImageError> {
    read_bytes(data, at).map(i32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of only signature, IHDR (with a dummy CRC), an empty IDAT and IEND: what an attacker
    /// needs to claim a huge image.
    fn png_header_only(width: u32, height: u32, depth: u8, color: u8) -> Vec<u8> {
        let mut data = PNG_SIGNATURE.to_vec();
        data.extend_from_slice(&13u32.to_be_bytes());
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[depth, color, 0, 0, 0]);
        data.extend_from_slice(&[0; 4]);
        for chunk in [b"IDAT", b"IEND"] {
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(chunk);
            data.extend_from_slice(&[0; 4]);
        }
        data
    }

    fn encode_png(width: u32, height: u32, color: png::ColorType, samples: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(samples).unwrap();
        writer.finish().unwrap();
        out
    }

    /// A bitmap info header of `header_bytes` (40 or 124) with the given fields.
    fn dib_header(
        header_bytes: u32,
        width: i32,
        height: i32,
        bits: u16,
        compression: u32,
    ) -> Vec<u8> {
        let mut data = vec![0; usize::try_from(header_bytes).unwrap()];
        data[0..4].copy_from_slice(&header_bytes.to_le_bytes());
        data[4..8].copy_from_slice(&width.to_le_bytes());
        data[8..12].copy_from_slice(&height.to_le_bytes());
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&bits.to_le_bytes());
        data[16..20].copy_from_slice(&compression.to_le_bytes());
        data
    }

    #[test]
    fn a_png_claiming_a_huge_image_is_refused_before_decoding() {
        let bomb = png_header_only(50_000, 50_000, 16, 6);
        assert!(matches!(
            decode_png(&bomb),
            Err(PasteImageError::TooLarge {
                width: 50_000,
                height: 50_000,
                ..
            })
        ));
        let widest = png_header_only(u32::MAX, u32::MAX, 16, 6);
        assert!(matches!(
            decode_png(&widest),
            Err(PasteImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn data_that_is_no_png_is_malformed() {
        assert!(matches!(
            decode_png(b"GIF89a"),
            Err(PasteImageError::Malformed(_))
        ));
        assert!(matches!(
            decode_png(&png_header_only(0, 4, 8, 6)),
            Err(PasteImageError::Malformed(_))
        ));
        // Header fine, data missing: the decoder's error, no panic.
        assert!(matches!(
            decode_png(&png_header_only(4, 4, 8, 6)),
            Err(PasteImageError::Malformed(_))
        ));
    }

    #[test]
    fn pngs_of_every_color_type_become_rgba() {
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
        let image = decode_png(&encode_png(2, 1, png::ColorType::Rgba, &rgba)).unwrap();
        assert_eq!(
            (image.width, image.height, image.rgba),
            (2, 1, rgba.to_vec())
        );
        let image =
            decode_png(&encode_png(2, 1, png::ColorType::Rgb, &[1, 2, 3, 4, 5, 6])).unwrap();
        assert_eq!(image.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
        let image = decode_png(&encode_png(2, 1, png::ColorType::Grayscale, &[9, 10])).unwrap();
        assert_eq!(image.rgba, [9, 9, 9, 255, 10, 10, 10, 255]);
        let image =
            decode_png(&encode_png(1, 1, png::ColorType::GrayscaleAlpha, &[9, 128])).unwrap();
        assert_eq!(image.rgba, [9, 9, 9, 128]);
    }

    #[test]
    fn a_bitmap_claiming_a_huge_image_is_refused_before_decoding() {
        let bomb = dib_header(40, 50_000, 50_000, 32, BI_RGB);
        assert!(matches!(
            decode_dib(&bomb),
            Err(PasteImageError::TooLarge {
                width: 50_000,
                height: 50_000,
                ..
            })
        ));
        // Top-down bitmaps have a negative height.
        let top_down = dib_header(124, 50_000, i32::MIN, 24, BI_RGB);
        assert!(matches!(
            decode_dib(&top_down),
            Err(PasteImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn broken_or_unusual_bitmaps_are_refused() {
        assert!(matches!(
            decode_dib(&[40, 0, 0]),
            Err(PasteImageError::Malformed(_))
        ));
        assert!(matches!(
            decode_dib(&dib_header(40, -2, 2, 24, BI_RGB)),
            Err(PasteImageError::Malformed(_))
        ));
        // Header promises 2x2 pixels, no rows follow.
        assert!(matches!(
            decode_dib(&dib_header(40, 2, 2, 24, BI_RGB)),
            Err(PasteImageError::Malformed(_))
        ));
        assert!(matches!(
            decode_dib(&dib_header(40, 2, 2, 8, BI_RGB)),
            Err(PasteImageError::Unsupported(_))
        ));
        // Run length encoded.
        assert!(matches!(
            decode_dib(&dib_header(40, 2, 2, 24, 1)),
            Err(PasteImageError::Unsupported(_))
        ));
    }

    #[test]
    fn a_24_bit_bitmap_is_read_bottom_up_with_padded_rows() {
        let mut data = dib_header(40, 2, 2, 24, BI_RGB);
        // Bottom row first, BGR, each row padded from 6 to 8 bytes.
        data.extend_from_slice(&[3, 2, 1, 6, 5, 4, 0, 0]);
        data.extend_from_slice(&[9, 8, 7, 12, 11, 10, 0, 0]);
        let image = decode_dib(&data).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(
            image.rgba,
            [7, 8, 9, 255, 10, 11, 12, 255, 1, 2, 3, 255, 4, 5, 6, 255]
        );
    }

    #[test]
    fn a_32_bit_bitmap_uses_its_masks_and_ignores_the_reserved_byte() {
        // BI_RGB with a plain header: the fourth byte is reserved, the pixel stays opaque.
        let mut data = dib_header(40, 1, -1, 32, BI_RGB);
        data.extend_from_slice(&[3, 2, 1, 77]);
        assert_eq!(decode_dib(&data).unwrap().rgba, [1, 2, 3, 255]);
        // BI_BITFIELDS after a plain header: three masks follow it (here RGB in reverse order).
        let mut data = dib_header(40, 1, 1, 32, BI_BITFIELDS);
        for mask in [0x0000_00ffu32, 0x0000_ff00, 0x00ff_0000] {
            data.extend_from_slice(&mask.to_le_bytes());
        }
        data.extend_from_slice(&[1, 2, 3, 0]);
        assert_eq!(decode_dib(&data).unwrap().rgba, [1, 2, 3, 255]);
        // A V5 header with an alpha mask keeps the alpha.
        let mut data = dib_header(124, 1, 1, 32, BI_BITFIELDS);
        for (at, mask) in [
            (40, 0x00ff_0000u32),
            (44, 0x0000_ff00),
            (48, 0x0000_00ff),
            (52, 0xff00_0000),
        ] {
            data[at..at + 4].copy_from_slice(&mask.to_le_bytes());
        }
        data.extend_from_slice(&[3, 2, 1, 128]);
        assert_eq!(decode_dib(&data).unwrap().rgba, [1, 2, 3, 128]);
    }

    #[test]
    fn narrow_masks_are_scaled_to_8_bits() {
        assert_eq!(channel(0b1_1111, 0b1_1111, 0), 255);
        assert_eq!(channel(0, 0b1_1111, 0), 0);
        assert_eq!(channel(0xffff_0000, 0xffff_0000, 0), 255);
        assert_eq!(channel(123, 0, 9), 9);
        // A mask with gaps must not overflow, even though decode_dib refuses it first.
        let _ = channel(u32::MAX, 0x8000_0001, 0);
    }

    #[test]
    fn masks_with_gaps_are_refused() {
        assert!(contiguous(0));
        assert!(contiguous(0x00ff_0000));
        assert!(!contiguous(0x8000_0001));
        let mut data = dib_header(40, 1, 1, 32, BI_BITFIELDS);
        for mask in [0x8000_0001u32, 0x0000_ff00, 0x00ff_0000] {
            data.extend_from_slice(&mask.to_le_bytes());
        }
        data.extend_from_slice(&[1, 2, 3, 0]);
        assert!(matches!(
            decode_dib(&data),
            Err(PasteImageError::Unsupported(_))
        ));
    }

    #[test]
    fn a_v2_header_holds_its_masks_inside() {
        let mut data = dib_header(52, 1, 1, 32, BI_BITFIELDS);
        for (at, mask) in [(40, 0x00ff_0000u32), (44, 0x0000_ff00), (48, 0x0000_00ff)] {
            data[at..at + 4].copy_from_slice(&mask.to_le_bytes());
        }
        data.extend_from_slice(&[3, 2, 1, 0]);
        assert_eq!(decode_dib(&data).unwrap().rgba, [1, 2, 3, 255]);
    }

    #[test]
    fn oversized_clipboard_data_is_refused() {
        assert!(check_data_size(MAX_DATA_BYTES).is_ok());
        assert!(matches!(
            check_data_size(MAX_DATA_BYTES + 1),
            Err(PasteImageError::DataTooLarge { .. })
        ));
    }
}
