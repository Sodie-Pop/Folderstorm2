//! Folderstorm: the upload preview's worker (folderstorm/plans/mcp-uploads.md):
//! local paths checked, files read once, hashed and decoded with the
//! uploader's own codecs, and the plan's bytes encoded again when they are
//! sent. Runs off the frame thread; pure functions of the files given, so
//! the tests run it on a temporary folder.
//!
//! The path rules are Folderstorm (C++)'s `uploadLocalPath`: absolute,
//! UTF-8, no network or device path, and no symbolic link (nor, on
//! Windows, reparse point) anywhere from the root down, each component read
//! without following links; a folder is walked in name order and every
//! file in it must be supported (nothing is skipped silently).

use crate::imports::Source;
use aurora_assets::{import_animation, import_sound, import_texture};
use aurora_net::inventory::upload::Kind;
use folderstorm_mcp_bridge::protocol::uploads::{DEPTH_MAX, FILE_BYTES_MAX, FILES_MAX, PATH_BYTES_MAX, PLAN_BYTES_MAX};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// What a preview reads.
pub enum Input {
    /// 1 to 32 local files, each uploaded into the destination.
    Files(Vec<PathBuf>),
    /// One local folder: a new inventory folder of its name, its tree below.
    Directory(PathBuf),
    /// A picture the studio drew, as PNG bytes, under this item name.
    Picture { name: String, png: Vec<u8> },
}

/// One file read, hashed and decoded.
#[derive(Clone)]
pub struct Scan {
    /// The file's name on disk (never its path).
    pub file: String,
    /// The new item's name.
    pub name: String,
    /// The inventory folders below the destination, the root folder first
    /// (a folder upload); empty for files.
    pub folder: Vec<String>,
    pub bytes: u64,
    pub sha256: String,
    pub kind: Kind,
    pub source: Source,
    /// A texture's size once uploaded.
    pub size: (u32, u32),
    /// A sound's length, in seconds.
    pub seconds: Option<f32>,
}

/// What a preview read.
pub struct Scanned {
    /// A folder upload's root folder name.
    pub root: Option<String>,
    pub files: Vec<Scan>,
}

/// The rules a preview reads its files with.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// The largest side of a texture once uploaded (1024 or 2048).
    pub max_texture: u32,
    /// The longest sound, in seconds (the uploader's: 30 on Second Life).
    pub sound_limit: u32,
    /// The uploader's texture options (Preferences), the size aside.
    pub texture: import_texture::Options,
}

impl Rules {
    /// The texture options a file is sized and encoded with.
    pub fn texture_options(&self) -> import_texture::Options {
        import_texture::Options {
            max_width: self.max_texture,
            max_height: self.max_texture,
            ..self.texture
        }
    }
}

/// Whether a file name's extension is one a preview takes.
fn kind_of(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" | "jpg" | "jpeg" | "tga" | "bmp" => "texture",
        "wav" => "sound",
        "anim" | "bvh" => "animation",
        _ => return None,
    })
}

#[cfg(windows)]
fn reparse(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn reparse(_: &std::fs::Metadata) -> bool {
    false
}

fn linked(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink() || reparse(meta)
}

/// A local path as a preview takes it: absolute, UTF-8, no network or
/// device path, no link from the root down (`uploadLocalPath`).
pub fn check_path(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_str().ok_or("a path must be valid UTF-8")?;
    if text.is_empty() || text.len() > PATH_BYTES_MAX || text.contains('\0') {
        return Err("a path must be 1 to 4096 bytes".into());
    }
    if text.starts_with("\\\\") || text.starts_with("//") {
        return Err("network and device paths are not upload inputs".into());
    }
    if !path.is_absolute() {
        return Err("an absolute local path is required (on the machine running the viewer)".into());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("a path may not hold `..`".into());
    }
    // from the root down, so that a linked parent is refused before
    // anything goes through it
    let mut walked = PathBuf::new();
    for component in path.components() {
        walked.push(component);
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&walked).map_err(|_| "a path names something that does not exist".to_owned())?;
        if linked(&meta) {
            return Err("symbolic links are not upload inputs".into());
        }
    }
    Ok(walked)
}

/// The files of `root` in name order, with their folders below it (the
/// root's own name first); links, special files and unsupported types
/// refuse the whole folder.
fn walk(root: &Path) -> Result<(String, Vec<(PathBuf, Vec<String>)>), String> {
    let meta = std::fs::symlink_metadata(root).map_err(|_| "the folder cannot be read".to_owned())?;
    if !meta.is_dir() || linked(&meta) {
        return Err("`directory` must be a real local folder".into());
    }
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the folder's name must be valid UTF-8")?
        .to_owned();
    let mut files = Vec::new();
    let mut pending = vec![(root.to_owned(), vec![name.clone()])];
    let mut visited = 0usize;
    while let Some((directory, folder)) = pending.pop() {
        let mut children: Vec<_> = std::fs::read_dir(&directory)
            .map_err(|_| "a folder cannot be read".to_owned())?
            .collect::<Result<_, _>>()
            .map_err(|_| "a folder cannot be read".to_owned())?;
        children.sort_by_key(|c| c.file_name().to_string_lossy().to_lowercase());
        for child in children {
            visited += 1;
            if visited > 10_000 {
                return Err("the folder holds too many entries".into());
            }
            let path = child.path();
            let shown = child.file_name().to_string_lossy().into_owned();
            let meta = std::fs::symlink_metadata(&path).map_err(|_| format!("{shown}: cannot be read"))?;
            if linked(&meta) {
                return Err(format!("{shown}: links are not upload inputs"));
            }
            if meta.is_dir() {
                if folder.len() >= DEPTH_MAX {
                    return Err(format!("the folder is deeper than {DEPTH_MAX} levels"));
                }
                let sub = child.file_name().to_str().ok_or("a folder's name must be valid UTF-8")?.to_owned();
                let mut below = folder.clone();
                below.push(sub);
                pending.push((path, below));
            } else if meta.is_file() {
                files.push((path, folder.clone()));
                if files.len() > FILES_MAX {
                    return Err(format!("the folder holds more than {FILES_MAX} files"));
                }
            } else {
                return Err(format!("{shown}: special files are not upload inputs"));
            }
        }
    }
    files.sort_by(|a, b| (&a.1, a.0.file_name()).cmp(&(&b.1, b.0.file_name())));
    Ok((name, files))
}

/// A file read once, within 64 MiB: its bytes, refused if it changed
/// while it was read.
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let before = std::fs::symlink_metadata(path).map_err(|_| "cannot be read".to_owned())?;
    if !before.is_file() || linked(&before) {
        return Err("not a regular file".into());
    }
    if before.len() == 0 || before.len() > FILE_BYTES_MAX {
        return Err("must hold 1 byte to 64 MiB".into());
    }
    let file = std::fs::File::open(path).map_err(|_| "cannot be opened".to_owned())?;
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0));
    file.take(FILE_BYTES_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot be read".to_owned())?;
    let after = std::fs::symlink_metadata(path).map_err(|_| "cannot be read".to_owned())?;
    if bytes.len() as u64 != before.len() || after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err("changed while it was read".into());
    }
    Ok(bytes)
}

/// An item name from a file's stem: the uploader's rule (1 to 63
/// characters, printable, no `|`).
pub fn item_name(stem: &str) -> Result<String, String> {
    let name = stem.trim();
    if name.is_empty() || name.chars().count() > 63 || !crate::text_validation::Policy::PrintableNoPipe.accepts(name) {
        return Err("an item name must be 1 to 63 printable characters without `|`".into());
    }
    Ok(name.to_owned())
}

/// The file decoded as the uploader decodes it.
fn decode(path_kind: &str, extension: &str, data: &[u8], rules: &Rules) -> Result<(Kind, Source, (u32, u32), Option<f32>), String> {
    match path_kind {
        "texture" => {
            let image = import_texture::decode(data).map_err(|e| e.to_string())?;
            let options = rules.texture_options();
            let size = import_texture::dimensions(image.width, image.height, options);
            Ok((Kind::Texture, Source::Texture(Arc::new(image)), size, None))
        }
        "sound" => {
            let sound = import_sound::decode_wav(data, crate::imports::sound::SOURCE_LIMIT).map_err(|e| e.to_string())?;
            let seconds = sound.duration();
            if seconds > rules.sound_limit as f32 + 0.0001 {
                return Err(format!(
                    "lasts {seconds:.1} s, longer than the {} s a sound may last (no splitting here: use the viewer's uploader)",
                    rules.sound_limit
                ));
            }
            Ok((Kind::Sound, Source::Sound(Arc::new(sound)), (0, 0), Some(seconds)))
        }
        _ if extension == "anim" => {
            let (animation, bytes) = aurora_assets::prepare_animation_upload(data).map_err(|e| e.to_string())?;
            Ok((
                Kind::Animation,
                Source::Animation {
                    animation: Arc::new(animation),
                    bytes: Arc::new(bytes),
                    bvh: None,
                },
                (0, 0),
                None,
            ))
        }
        _ => {
            let skeleton = aurora_assets::Skeleton::parse(include_bytes!("../../../assets/character/avatar_skeleton.xml"))
                .map_err(|e| e.to_string())?;
            let bvh = import_animation::parse_bvh(data, &skeleton).map_err(|e| e.to_string())?;
            Ok((
                Kind::Animation,
                Source::Animation {
                    bytes: Arc::new(Vec::new()),
                    bvh: Some(crate::imports::BvhInfo {
                        frames: bvh.frames,
                        frame_time: bvh.frame_time,
                    }),
                    animation: Arc::new(bvh.animation),
                },
                (0, 0),
                None,
            ))
        }
    }
}

fn hash(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Why `path`'s type is refused, naming the file.
fn unsupported(path: &Path) -> String {
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dae")) {
        format!("{file}: a Collada file goes to inventory_upload_mesh_review")
    } else {
        format!("{file}: unsupported type (PNG, JPEG, TGA, BMP, WAV, ANIM or BVH)")
    }
}

/// One local file read, hashed and decoded; errors name the file.
fn scan_file(path: &Path, folder: Vec<String>, rules: &Rules) -> Result<Scan, String> {
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    let fail = |e: String| format!("{file}: {e}");
    let kind = kind_of(path).ok_or_else(|| unsupported(path))?;
    let name = item_name(path.file_stem().and_then(|s| s.to_str()).unwrap_or_default()).map_err(fail)?;
    let data = read(path).map_err(fail)?;
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let (kind, source, size, seconds) = decode(kind, &extension, &data, rules).map_err(fail)?;
    Ok(Scan {
        file,
        name,
        folder,
        bytes: data.len() as u64,
        sha256: hash(&data),
        kind,
        source,
        size,
        seconds,
    })
}

/// Read what a preview asks for, with `rules`.
pub fn run(input: Input, rules: &Rules) -> Result<Scanned, String> {
    let (root, listed) = match input {
        Input::Picture { name, png } => {
            if png.len() as u64 > FILE_BYTES_MAX {
                return Err("the picture is too large".into());
            }
            let image = import_texture::decode(&png).map_err(|e| e.to_string())?;
            let size = import_texture::dimensions(image.width, image.height, rules.texture_options());
            return Ok(Scanned {
                root: None,
                files: vec![Scan {
                    file: "snapshot.png".into(),
                    name,
                    folder: Vec::new(),
                    bytes: png.len() as u64,
                    sha256: hash(&png),
                    kind: Kind::Snapshot,
                    source: Source::Texture(Arc::new(image)),
                    size,
                    seconds: None,
                }],
            });
        }
        Input::Files(paths) => {
            if paths.is_empty() || paths.len() > FILES_MAX {
                return Err(format!("give 1 to {FILES_MAX} files"));
            }
            let mut checked = Vec::with_capacity(paths.len());
            for path in &paths {
                let path = check_path(path)?;
                if checked.contains(&path) {
                    return Err("the same file is given twice".into());
                }
                checked.push(path);
            }
            (None, checked.into_iter().map(|p| (p, Vec::new())).collect::<Vec<_>>())
        }
        Input::Directory(path) => {
            let path = check_path(&path)?;
            let (name, files) = walk(&path)?;
            item_name(&name).map_err(|e| format!("the folder's name: {e}"))?;
            if files.is_empty() {
                return Err("the folder holds no file".into());
            }
            (Some(name), files)
        }
    };
    // every type first: an unsupported file refuses before anything is read
    if let Some((path, _)) = listed.iter().find(|(path, _)| kind_of(path).is_none()) {
        return Err(unsupported(path));
    }
    let mut files = Vec::with_capacity(listed.len());
    let mut total = 0u64;
    for (path, folder) in listed {
        let scan = scan_file(&path, folder, rules)?;
        total += scan.bytes;
        if total > PLAN_BYTES_MAX {
            return Err("the files hold more than 128 MiB together".into());
        }
        files.push(scan);
    }
    Ok(Scanned { root, files })
}

/// A plan's file encoded for its upload, as the uploader encodes it
/// (`imports::files::encode`, without its sound edits).
pub fn encode(source: &Source, options: import_texture::Options) -> Result<Vec<u8>, String> {
    let result = match source {
        Source::Texture(pixels) => import_texture::prepare(pixels, options).and_then(|image| import_texture::encode(&image, options)),
        Source::Sound(sound) => {
            return crate::imports::sound::Options::default()
                .render(sound)?
                .encode()
                .map_err(|e| e.to_string());
        }
        Source::Animation { bytes, animation, bvh } => {
            if bvh.is_some() {
                import_animation::encode(animation)
            } else {
                aurora_assets::rewrite_animation_upload(bytes, animation)
            }
        }
        Source::Material { .. } | Source::Mesh(_) => return Err("not an automatic upload".into()),
    };
    result.map_err(|e| e.to_string())
}
