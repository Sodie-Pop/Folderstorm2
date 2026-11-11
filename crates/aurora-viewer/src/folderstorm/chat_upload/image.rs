//! What is checked and changed before an image leaves the viewer.
//!
//! - Only PNG, JPEG, GIF and WebP, recognised by their content, not by the
//!   file's name; their header must give a size within `MAX_PIXELS`.
//! - At most the size limit of Preferences, checked before the file is read.
//! - A file keeps its own bytes (animation, quality), without the metadata
//!   that can tell where and with what a picture was taken: JPEG's EXIF
//!   (except its orientation), XMP, IPTC and comments; PNG's text, EXIF and
//!   time chunks; WebP's EXIF and XMP chunks. GIF is sent as it is.
//! - A pasted picture becomes a new PNG, with nothing but its pixels.

use std::io::Read;

/// Largest picture, in pixels (16 megapixels, Folderstorm (C++)'s limit).
pub const MAX_PIXELS: u64 = 16_000_000;
/// Side of the preview of the confirmation.
pub const THUMB_SIDE: u32 = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Gif => "gif",
            Format::Webp => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Jpeg => "image/jpeg",
            Format::Gif => "image/gif",
            Format::Webp => "image/webp",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Gif => "GIF",
            Format::Webp => "WebP",
        }
    }

    fn image_format(self) -> ::image::ImageFormat {
        match self {
            Format::Png => ::image::ImageFormat::Png,
            Format::Jpeg => ::image::ImageFormat::Jpeg,
            Format::Gif => ::image::ImageFormat::Gif,
            Format::Webp => ::image::ImageFormat::WebP,
        }
    }
}

/// The format a file's first bytes announce.
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else {
        None
    }
}

/// Why an image is not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Larger than the limit (its size, the limit, in bytes).
    TooLarge(u64, u64),
    /// Not one of the four formats.
    Unsupported,
    /// Announced as one of them, but its structure is broken.
    Corrupt,
    /// Over `MAX_PIXELS`.
    TooManyPixels(u32, u32),
    /// The file could not be read.
    Unreadable,
}

/// An image ready to send.
#[derive(Clone)]
pub struct Prepared {
    pub bytes: Vec<u8>,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    /// Metadata was found and left out.
    pub stripped: bool,
    /// A small preview for the confirmation.
    pub thumb: Option<egui::ColorImage>,
}

/// Reads and prepares a file, at most `max` bytes. Only a regular file
/// is opened: opening a named pipe (FIFO) or a device can block for good.
pub fn prepare_file(path: &std::path::Path, max: u64) -> Result<Prepared, Refusal> {
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(Refusal::Unreadable);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // swapped for a pipe between the look and the opening: the opening
    // still does not wait (no effect on a regular file's reads)
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| Refusal::Unreadable)?;
    let meta = file.metadata().map_err(|_| Refusal::Unreadable)?;
    if !meta.is_file() {
        return Err(Refusal::Unreadable);
    }
    let len = meta.len();
    if len > max {
        return Err(Refusal::TooLarge(len, max));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(max + 1).read_to_end(&mut bytes).map_err(|_| Refusal::Unreadable)?;
    prepare_bytes(bytes, max)
}

/// Checks a file's bytes and leaves out its metadata.
pub fn prepare_bytes(bytes: Vec<u8>, max: u64) -> Result<Prepared, Refusal> {
    if bytes.len() as u64 > max {
        return Err(Refusal::TooLarge(bytes.len() as u64, max));
    }
    let format = sniff(&bytes).ok_or(Refusal::Unsupported)?;
    let (width, height) = dimensions(&bytes, format)?;
    let (bytes, stripped) = match format {
        Format::Jpeg => strip_jpeg(&bytes)?,
        Format::Png => strip_png(&bytes)?,
        Format::Webp => strip_webp(&bytes)?,
        Format::Gif => (bytes, false),
    };
    let thumb = Some(thumbnail(&bytes, format).ok_or(Refusal::Corrupt)?);
    Ok(Prepared {
        bytes,
        format,
        width,
        height,
        stripped,
        thumb,
    })
}

/// A pasted picture as a new PNG.
pub fn prepare_pasted(image: &egui::ColorImage, max: u64) -> Result<Prepared, Refusal> {
    let [w, h] = image.size;
    let (width, height) = (
        u32::try_from(w).map_err(|_| Refusal::Corrupt)?,
        u32::try_from(h).map_err(|_| Refusal::Corrupt)?,
    );
    check_pixels(width, height)?;
    if width == 0 || height == 0 || image.pixels.len() != w * h {
        return Err(Refusal::Corrupt);
    }
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let mut bytes = Vec::new();
    {
        use ::image::ImageEncoder;
        ::image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&rgba, width, height, ::image::ExtendedColorType::Rgba8)
            .map_err(|_| Refusal::Corrupt)?;
    }
    if bytes.len() as u64 > max {
        return Err(Refusal::TooLarge(bytes.len() as u64, max));
    }
    let thumb = thumbnail(&bytes, Format::Png);
    Ok(Prepared {
        bytes,
        format: Format::Png,
        width,
        height,
        stripped: false,
        thumb,
    })
}

fn check_pixels(width: u32, height: u32) -> Result<(), Refusal> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Refusal::TooManyPixels(width, height));
    }
    Ok(())
}

/// The size the header gives, read without decoding the picture.
fn dimensions(bytes: &[u8], format: Format) -> Result<(u32, u32), Refusal> {
    let reader = ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format.image_format());
    let (width, height) = reader.into_dimensions().map_err(|_| Refusal::Corrupt)?;
    if width == 0 || height == 0 {
        return Err(Refusal::Corrupt);
    }
    check_pixels(width, height)?;
    Ok((width, height))
}

/// The first frame, at most `THUMB_SIDE` on its long side; none when the
/// decoder refuses the picture (its header alone looked right).
fn thumbnail(bytes: &[u8], format: Format) -> Option<egui::ColorImage> {
    let mut reader = ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format.image_format());
    let mut limits = ::image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 4 + 64 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader.decode().ok()?;
    let small = picture.thumbnail(THUMB_SIDE, THUMB_SIDE).to_rgba8();
    let size = [small.width() as usize, small.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, small.as_raw()))
}

// ---- JPEG

/// A JPEG without its APP1 (EXIF, XMP), APP3 to APP13 and APP15 segments
/// (IPTC, Photoshop, maker data, MPF's index of extra pictures) and
/// comments. JFIF (APP0, without its thumbnail), the colour profile (APP2
/// `ICC_PROFILE`) and Adobe's colour transform (APP14) stay: the picture
/// looks the same. The EXIF orientation is put back alone, so that a
/// phone's photo is not turned.
///
/// The whole file is walked, not only its header: between the scans of a
/// progressive picture only what decoding needs stays (tables, restart
/// interval, scans; no APPn, no comment), and nothing after the end of the
/// picture (EOI) is kept: a phone appends there a second picture with its
/// own EXIF (MPF) or a motion photo's video with its location. A file cut
/// short inside its picture data is closed with an EOI.
pub fn strip_jpeg(bytes: &[u8]) -> Result<(Vec<u8>, bool), Refusal> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(Refusal::Corrupt);
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&[0xff, 0xd8]);
    let mut at = 2;
    let mut stripped = false;
    let mut orientation = None;
    // a scan was met: what follows its header is entropy-coded data
    let mut scanned = false;
    loop {
        // fill bytes before a marker are allowed
        while bytes.get(at) == Some(&0xff) && bytes.get(at + 1) == Some(&0xff) {
            at += 1;
        }
        let (Some(&0xff), Some(&marker)) = (bytes.get(at), bytes.get(at + 1)) else {
            return Err(Refusal::Corrupt);
        };
        // markers without a length
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            out.extend_from_slice(&bytes[at..at + 2]);
            at += 2;
            continue;
        }
        if marker == 0xd9 {
            out.extend_from_slice(&[0xff, 0xd9]);
            // whatever follows the picture is not part of it
            stripped |= at + 2 < bytes.len();
            break;
        }
        if marker == 0xd8 {
            // a second start of image before the end of the first
            return Err(Refusal::Corrupt);
        }
        let len = match (bytes.get(at + 2), bytes.get(at + 3)) {
            (Some(&hi), Some(&lo)) => usize::from(u16::from_be_bytes([hi, lo])),
            _ => return Err(Refusal::Corrupt),
        };
        let end = at + 2 + len;
        if len < 2 || end > bytes.len() {
            return Err(Refusal::Corrupt);
        }
        let body = &bytes[at + 4..end];
        if marker == 0xda {
            if !scanned {
                put_orientation(&mut out, orientation.take());
                scanned = true;
            }
            out.extend_from_slice(&bytes[at..end]);
            at = end;
            // the scan's data, to the next marker that is neither a
            // stuffed 0xFF (FF 00) nor a restart (FF D0 to FF D7)
            let start = at;
            loop {
                let Some(found) = bytes[at..].iter().position(|&b| b == 0xff) else {
                    at = bytes.len();
                    break;
                };
                at += found;
                match bytes.get(at + 1) {
                    Some(0x00 | 0xd0..=0xd7) => at += 2,
                    Some(_) => break,
                    None => {
                        at = bytes.len();
                        break;
                    }
                }
            }
            out.extend_from_slice(&bytes[start..at.min(bytes.len())]);
            if at >= bytes.len() {
                // cut short: the picture is closed where it stops
                if out.last() == Some(&0xff) {
                    out.pop();
                }
                out.extend_from_slice(&[0xff, 0xd9]);
                break;
            }
            continue;
        }
        let keep = match marker {
            // nothing but decoding after the first scan
            0xe0..=0xef | 0xfe if scanned => Keep::No,
            0xe0 => jfif(body),
            0xee => Keep::Yes,
            0xe2 if body.starts_with(b"ICC_PROFILE\0") => Keep::Yes,
            0xe1 => {
                if body.starts_with(b"Exif\0\0") {
                    orientation = orientation.or_else(|| exif_orientation(&body[6..]));
                }
                Keep::No
            }
            0xe2..=0xed | 0xef | 0xfe => Keep::No,
            _ => Keep::Yes,
        };
        match keep {
            Keep::No => stripped = true,
            Keep::Yes | Keep::Rewritten(_) => {
                // the orientation goes right after JFIF, before the frame
                if marker != 0xe0 {
                    put_orientation(&mut out, orientation.take());
                }
                if let Keep::Rewritten(segment) = keep {
                    out.extend_from_slice(&segment);
                    stripped = true;
                } else {
                    out.extend_from_slice(&bytes[at..end]);
                }
            }
        }
        at = end;
    }
    Ok((out, stripped))
}

/// What becomes of a JPEG segment.
enum Keep {
    Yes,
    No,
    /// Kept in a shorter form.
    Rewritten(Vec<u8>),
}

/// JFIF's APP0 stays, without the thumbnail it may carry; JFXX (a
/// thumbnail alone) and any other APP0 go.
fn jfif(body: &[u8]) -> Keep {
    // "JFIF\0", version, units, densities, thumbnail width and height
    const HEADER: usize = 14;
    if !body.starts_with(b"JFIF\0") || body.len() < HEADER {
        return Keep::No;
    }
    if body.len() == HEADER && body[12] == 0 && body[13] == 0 {
        return Keep::Yes;
    }
    let mut segment = vec![0xff, 0xe0, 0, (HEADER + 2) as u8];
    segment.extend_from_slice(&body[..12]);
    segment.extend_from_slice(&[0, 0]);
    Keep::Rewritten(segment)
}

/// Writes an EXIF segment holding only the orientation (when not upright).
fn put_orientation(out: &mut Vec<u8>, orientation: Option<u16>) {
    let Some(value) = orientation.filter(|v| (2..=8).contains(v)) else {
        return;
    };
    let mut tiff = Vec::with_capacity(26);
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&value.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next directory
    let len = (2 + 6 + tiff.len()) as u16;
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
}

/// The Orientation tag of an EXIF block's first directory.
pub fn exif_orientation(tiff: &[u8]) -> Option<u16> {
    let little = match tiff.get(..4)? {
        b"II*\0" => true,
        b"MM\0*" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b = tiff.get(at..at + 2)?;
        Some(if little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b = tiff.get(at..at + 4)?;
        let b = [b[0], b[1], b[2], b[3]];
        Some(if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    };
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let count = usize::from(u16_at(ifd)?);
    (0..count.min(256)).find_map(|n| {
        let entry = ifd + 2 + n * 12;
        (u16_at(entry)? == 0x0112 && u16_at(entry + 2)? == 3).then(|| u16_at(entry + 8))?
    })
}

// ---- PNG

/// Chunks that only describe the picture: text, EXIF, time.
const PNG_DROPPED: [&[u8; 4]; 5] = [b"tEXt", b"zTXt", b"iTXt", b"eXIf", b"tIME"];

pub fn strip_png(bytes: &[u8]) -> Result<(Vec<u8>, bool), Refusal> {
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..8]);
    let mut at = 8;
    let mut stripped = false;
    loop {
        let header = bytes.get(at..at + 8).ok_or(Refusal::Corrupt)?;
        let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let kind = &header[4..8];
        let end = at.checked_add(12).and_then(|n| n.checked_add(len)).ok_or(Refusal::Corrupt)?;
        if end > bytes.len() {
            return Err(Refusal::Corrupt);
        }
        if PNG_DROPPED.iter().any(|d| d.as_slice() == kind) {
            stripped = true;
        } else {
            out.extend_from_slice(&bytes[at..end]);
        }
        at = end;
        if kind == b"IEND" {
            return Ok((out, stripped));
        }
    }
}

// ---- WebP

pub fn strip_webp(bytes: &[u8]) -> Result<(Vec<u8>, bool), Refusal> {
    let mut chunks = Vec::with_capacity(bytes.len());
    let mut at = 12;
    let mut stripped = false;
    let declared = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let end_of_file = declared.checked_add(8).ok_or(Refusal::Corrupt)?.min(bytes.len());
    while at + 8 <= end_of_file {
        let kind = &bytes[at..at + 4];
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let padded = len.checked_add(len & 1).ok_or(Refusal::Corrupt)?;
        let end = (at + 8).checked_add(padded).ok_or(Refusal::Corrupt)?;
        if at + 8 + len > end_of_file {
            return Err(Refusal::Corrupt);
        }
        let end = end.min(end_of_file);
        if kind == b"EXIF" || kind == b"XMP " {
            stripped = true;
        } else {
            let from = chunks.len();
            chunks.extend_from_slice(&bytes[at..end]);
            if kind == b"VP8X" && len >= 1 {
                // the flags no longer announce what was left out
                chunks[from + 8] &= !(0x08 | 0x04);
            }
            if len & 1 == 1 && end - at < 8 + padded {
                chunks.push(0);
            }
        }
        at = end;
    }
    if chunks.is_empty() {
        return Err(Refusal::Corrupt);
    }
    let mut out = Vec::with_capacity(chunks.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&u32::try_from(chunks.len() + 4).map_err(|_| Refusal::Corrupt)?.to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&chunks);
    Ok((out, stripped))
}
