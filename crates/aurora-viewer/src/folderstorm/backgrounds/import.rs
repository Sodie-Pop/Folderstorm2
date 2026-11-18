//! Pictures on disk: the copy made when one is chosen, and the decoding
//! for the texture (both off the event thread). Folderstorm (C++) copied
//! the chosen file into the account's own folder and kept only the copy's
//! name, so that the original may move or go; here the folder is the
//! settings folder's (per profile, as the layouts are).

use super::record::{EXTENSIONS, Picture, managed_name, tidy_name};
use std::path::{Path, PathBuf};

/// The largest file accepted.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// The largest side accepted, in pixels.
pub const MAX_SIDE: u32 = 8192;
/// The most pixels accepted.
pub const MAX_PIXELS: u64 = 32 * 1024 * 1024;
/// The largest side of the texture: a larger picture is reduced.
pub const TEXTURE_SIDE: u32 = 4096;

/// The folder the copies live in: `folderstorm_backgrounds` in the
/// settings folder (each `--profile` has its own).
pub fn folder() -> PathBuf {
    crate::settings::config_dir().join("folderstorm_backgrounds")
}

/// Why a picture was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// Over [`MAX_BYTES`], [`MAX_SIDE`] or [`MAX_PIXELS`].
    TooLarge,
    /// Not a picture the viewer decodes, or a damaged one.
    Unreadable,
    /// The file could not be read or the copy written.
    Disk,
}

/// What [`decode`] could not do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The copy is not in the folder any more.
    Missing,
    /// It is there but cannot be decoded, or is too large.
    Unreadable,
}

/// A picture decoded for the texture.
pub struct Decoded {
    pub image: egui::ColorImage,
    /// Its own size in pixels, before any reduction (Centre and Tile draw
    /// it at that size).
    pub size: [u32; 2],
}

/// A stable 64-bit hash of a file's bytes (FNV-1a): the copy's name, so
/// that the same picture chosen twice is one file (and how a backup checks
/// a copy it carries, `crate::folderstorm::backup::file`).
pub(crate) fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    // the decoded pixels of the largest picture accepted, with room
    limits.max_alloc = Some(MAX_PIXELS * 4 + 64 * 1024 * 1024);
    limits
}

fn extension(format: image::ImageFormat) -> Option<&'static str> {
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Bmp => "bmp",
        image::ImageFormat::Tga => "tga",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        _ => return None,
    };
    debug_assert!(EXTENSIONS.contains(&extension));
    Some(extension)
}

/// Copies the picture at `source` into `folder` and gives what a
/// background keeps of it. Checked first: its size on disk, its format
/// and size in pixels, and that it decodes. Runs on the file dialog's
/// thread.
pub fn import(source: &Path, folder: &Path) -> Result<Picture, ImportError> {
    let length = std::fs::metadata(source).map_err(|_| ImportError::Disk)?.len();
    if length > MAX_BYTES {
        return Err(ImportError::TooLarge);
    }
    let bytes = std::fs::read(source).map_err(|_| ImportError::Disk)?;
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    store(&bytes, &name, folder)
}

/// [`import`] of bytes already read (`name`: the original file's name).
pub fn store(bytes: &[u8], name: &str, folder: &Path) -> Result<Picture, ImportError> {
    let (_, extension) = checked(bytes)?;
    let file = format!("{:016x}.{extension}", hash(bytes));
    debug_assert!(managed_name(&file));
    let path = folder.join(&file);
    if !path.is_file() {
        std::fs::create_dir_all(folder).map_err(|_| ImportError::Disk)?;
        // written aside, then renamed: never half a picture under its name
        let part = folder.join(format!("{file}.part"));
        std::fs::write(&part, bytes).map_err(|_| ImportError::Disk)?;
        if std::fs::rename(&part, &path).is_err() {
            let _ = std::fs::remove_file(&part);
            return Err(ImportError::Disk);
        }
    }
    Ok(Picture {
        file,
        name: tidy_name(name),
    })
}

/// The checks of [`store`] on a picture's bytes (their size, the format,
/// the size in pixels), then the picture decoded within the limits, with
/// the extension its copy takes. Also what a desktop shortcut's picture
/// is read with (`desktop::style::import`) before it is reduced.
pub fn checked(bytes: &[u8]) -> Result<(image::DynamicImage, &'static str), ImportError> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ImportError::TooLarge);
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ImportError::Unreadable)?;
    let extension = reader.format().and_then(extension).ok_or(ImportError::Unreadable)?;
    let (width, height) = reader.into_dimensions().map_err(|_| ImportError::Unreadable)?;
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(ImportError::TooLarge);
    }
    if width == 0 || height == 0 {
        return Err(ImportError::Unreadable);
    }
    // decoded once, so that a damaged file is said now and not copied
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ImportError::Unreadable)?;
    reader.limits(limits());
    let picture = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => ImportError::TooLarge,
        _ => ImportError::Unreadable,
    })?;
    Ok((picture, extension))
}

/// Decodes the copy `file` of `folder` for the texture, reduced to at most
/// `side` pixels a side (never more than [`TEXTURE_SIDE`]).
pub fn decode(folder: &Path, file: &str, side: u32) -> Result<Decoded, LoadError> {
    if !managed_name(file) {
        return Err(LoadError::Missing);
    }
    let path = folder.join(file);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LoadError::Missing),
        Err(_) => return Err(LoadError::Unreadable),
    };
    if bytes.len() as u64 > MAX_BYTES {
        return Err(LoadError::Unreadable);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| LoadError::Unreadable)?;
    reader.limits(limits());
    let picture = reader.decode().map_err(|_| LoadError::Unreadable)?;
    let size = [picture.width(), picture.height()];
    if size[0] == 0 || size[1] == 0 {
        return Err(LoadError::Unreadable);
    }
    let side = side.clamp(1, TEXTURE_SIDE);
    let picture = if size[0] > side || size[1] > side {
        picture.resize(side, side, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let rgba = picture.to_rgba8();
    let image = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    Ok(Decoded { image, size })
}

/// Deletes the copies in `folder` that no background refers to (`kept`):
/// only files of our own names, never anything else in the folder.
pub fn prune(folder: &Path, kept: &std::collections::BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = managed_name(&name) || name.strip_suffix(".part").is_some_and(managed_name);
        if ours && !kept.contains(&name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh, empty folder for one test.
    pub fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("folderstorm-backgrounds-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    /// A small PNG of `w` by `h`, red on the left half, blue on the right.
    pub fn png(w: u32, h: u32) -> Vec<u8> {
        let picture = image::RgbaImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let mut out = std::io::Cursor::new(Vec::new());
        picture.write_to(&mut out, image::ImageFormat::Png).expect("encoded");
        out.into_inner()
    }

    #[test]
    fn a_picture_is_copied_once_under_a_name_of_ours() {
        let dir = scratch("copy");
        let source = dir.join("My beach.png");
        std::fs::write(&source, png(40, 20)).expect("written");
        let into = dir.join("copies");
        let picture = import(&source, &into).expect("taken");
        assert!(managed_name(&picture.file) && picture.file.ends_with(".png"), "{}", picture.file);
        assert_eq!(picture.name, "My beach.png");
        // the original may go: the copy stays
        std::fs::remove_file(&source).expect("removed");
        assert!(into.join(&picture.file).is_file());
        // the same bytes again: the same file, no other
        let again = store(&png(40, 20), "copy.png", &into).expect("taken");
        assert_eq!(again.file, picture.file);
        assert_eq!(std::fs::read_dir(&into).expect("listed").count(), 1);
        let decoded = decode(&into, &picture.file, TEXTURE_SIDE).expect("decoded");
        assert_eq!((decoded.size, decoded.image.size), ([40, 20], [40, 20]));
        assert_eq!(decoded.image.pixels[0], egui::Color32::RED);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_not_a_picture_or_too_large_is_refused() {
        let dir = scratch("refused");
        assert_eq!(store(b"not a picture at all", "notes.txt", &dir), Err(ImportError::Unreadable));
        // a PNG cut in the middle: its header reads, its pixels do not
        let mut cut = png(64, 64);
        cut.truncate(cut.len() / 2);
        assert_eq!(store(&cut, "cut.png", &dir), Err(ImportError::Unreadable));
        // wider than the limit (its header is enough to tell)
        let wide = image::RgbaImage::new(MAX_SIDE + 1, 1);
        let mut bytes = std::io::Cursor::new(Vec::new());
        wide.write_to(&mut bytes, image::ImageFormat::Png).expect("encoded");
        assert_eq!(store(&bytes.into_inner(), "wide.png", &dir), Err(ImportError::TooLarge));
        assert_eq!(import(&dir.join("gone.png"), &dir), Err(ImportError::Disk));
        assert_eq!(std::fs::read_dir(&dir).expect("listed").count(), 0, "nothing copied");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_large_picture_is_reduced_its_own_size_kept() {
        let dir = scratch("reduced");
        let picture = store(&png(300, 100), "wide.png", &dir).expect("taken");
        let decoded = decode(&dir, &picture.file, 150).expect("decoded");
        assert_eq!(decoded.size, [300, 100]);
        assert_eq!(decoded.image.size, [150, 50]);
        assert_eq!(decode(&dir, "0123456789abcdef.png", 150).err(), Some(LoadError::Missing));
        assert_eq!(decode(&dir, "../x.png", 150).err(), Some(LoadError::Missing));
        std::fs::write(dir.join("0123456789abcdef.png"), b"damaged").expect("written");
        assert_eq!(decode(&dir, "0123456789abcdef.png", 150).err(), Some(LoadError::Unreadable));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_our_copies_that_nothing_refers_to_are_deleted() {
        let dir = scratch("prune");
        let kept = store(&png(10, 10), "a.png", &dir).expect("taken");
        let gone = store(&png(12, 10), "b.png", &dir).expect("taken");
        std::fs::write(dir.join("notes.txt"), b"mine").expect("written");
        std::fs::write(dir.join("0123456789abcdef.png.part"), b"half").expect("written");
        prune(&dir, &[kept.file.clone()].into());
        assert!(dir.join(&kept.file).is_file());
        assert!(!dir.join(&gone.file).exists());
        assert!(!dir.join("0123456789abcdef.png.part").exists());
        assert!(dir.join("notes.txt").is_file(), "not ours: left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
