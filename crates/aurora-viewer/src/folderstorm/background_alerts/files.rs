//! The local sound files: a WAV or OGG file the user chooses is checked
//! (size, a sound that decodes, its length) and copied into the main
//! profile's folder, `background-alert-sounds/`, where every character's
//! viewer finds it. A choice names the copy, never a path elsewhere: the
//! original can move or go, the alert stays.

use aurora_audio::SoundClip;
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "background-alert-sounds";
/// Folderstorm (C++)'s limits: 4 MiB and 10 seconds.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_SECONDS: f64 = 10.0;
/// The kinds of file taken.
pub const EXTENSIONS: [&str; 2] = ["wav", "ogg"];
/// A copy's name, at most this many characters before its extension.
const MAX_STEM: usize = 48;

/// Why a file was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    /// Not a `.wav` or `.ogg` file.
    Kind,
    TooLarge,
    /// It could not be read (gone, locked, no permission).
    Unreadable,
    /// It is not a sound this viewer decodes.
    NotSound,
    TooLong,
    /// The copy could not be written.
    Copy,
}

impl FileError {
    pub fn text(self) -> String {
        use aurora_i18n::tr;
        match self {
            Self::Kind => tr!("fs-alerts-file-kind").to_owned(),
            Self::TooLarge => tr!("fs-alerts-file-too-large", mib = (MAX_BYTES / (1024 * 1024)) as i64),
            Self::Unreadable => tr!("fs-alerts-file-unreadable").to_owned(),
            Self::NotSound => tr!("fs-alerts-file-not-sound").to_owned(),
            Self::TooLong => tr!("fs-alerts-file-too-long", seconds = MAX_SECONDS as i64),
            Self::Copy => tr!("fs-alerts-file-copy").to_owned(),
        }
    }
}

/// The sounds' folder.
pub fn folder() -> &'static Path {
    static FOLDER_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER_PATH.get_or_init(|| crate::folderstorm_characters::profile::main_config_dir().join(FOLDER))
}

fn extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// A name of the folder that a choice may hold: one plain file name of a
/// kind taken, never a path, nothing hidden.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_STEM + 5
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.contains("..")
        && Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name)
        && extension(Path::new(name)).is_some()
}

/// The copy's name for a file called `original`: its name kept to letters,
/// digits, `-` and `_`, shortened, then `-2`, `-3`… while `taken`.
pub fn copy_name(original: &Path, taken: impl Fn(&str) -> bool) -> Option<String> {
    let ext = extension(original)?;
    let stem: String = original
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(MAX_STEM)
        .collect();
    let stem = stem.trim_matches('_');
    let stem = if stem.is_empty() { "sound" } else { stem };
    (1..1000)
        .map(|n| {
            if n == 1 {
                format!("{stem}.{ext}")
            } else {
                format!("{stem}-{n}.{ext}")
            }
        })
        .find(|name| !taken(name))
}

/// A sound file's bytes made into a clip, within the limits.
pub fn decode(bytes: &[u8]) -> Result<SoundClip, FileError> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(FileError::TooLarge);
    }
    let clip = SoundClip::decode(bytes).map_err(|_| FileError::NotSound)?;
    if clip.duration().as_secs_f64() > MAX_SECONDS {
        return Err(FileError::TooLong);
    }
    Ok(clip)
}

/// Reads a file within the size limit (its size is looked at first).
fn read(path: &Path) -> Result<Vec<u8>, FileError> {
    let meta = std::fs::metadata(path).map_err(|_| FileError::Unreadable)?;
    if !meta.is_file() {
        return Err(FileError::Unreadable);
    }
    if meta.len() > MAX_BYTES {
        return Err(FileError::TooLarge);
    }
    std::fs::read(path).map_err(|_| FileError::Unreadable)
}

/// Checks the file at `source` and copies it into `folder`: the copy's
/// name.
pub fn import(source: &Path, folder: &Path) -> Result<String, FileError> {
    if extension(source).is_none() {
        return Err(FileError::Kind);
    }
    let bytes = read(source)?;
    decode(&bytes)?;
    std::fs::create_dir_all(folder).map_err(|_| FileError::Copy)?;
    let name = copy_name(source, |name| folder.join(name).exists()).ok_or(FileError::Copy)?;
    crate::atomic_file::write(&folder.join(&name), &bytes).map_err(|_| FileError::Copy)?;
    log::info!("background alerts: a sound file was copied as {name}");
    Ok(name)
}

/// The clip of the folder's file `name`.
pub fn load(folder: &Path, name: &str) -> Result<SoundClip, FileError> {
    if !valid_name(name) {
        return Err(FileError::Unreadable);
    }
    decode(&read(&folder.join(name))?)
}

/// The folder's sound files, by name.
pub fn list(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|name| valid_name(name))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}
