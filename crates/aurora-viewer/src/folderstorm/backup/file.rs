//! The backup file: a ZIP holding
//!
//! - `manifest.json`: what the file is ([`FORMAT`], [`VERSION`]), the
//!   viewer that wrote it, when, the account, and its sections with the
//!   files each one uses;
//! - `sections/<id>.json`: one section's value;
//! - `files/<folder>/<name>`: a copied file a section names (a background
//!   picture), only from the folders of [`FOLDERS`].
//!
//! Reading never trusts the file: its size, its entries and their sizes
//! are bounded, a name in it is never a path, a file of a folder must pass
//! that folder's check, and a section this version does not know (a newer
//! one's) is listed and left alone.

use super::section::Env;
use super::{Account, secrets, sections};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// What the manifest says the file is: any other ZIP is refused.
pub const FORMAT: &str = "folderstorm-account-backup";
/// The version of the format this build writes. A newer file is read for
/// what this build knows of it.
pub const VERSION: u32 = 1;
/// The files' extension: a plain ZIP, which anyone can open to look.
pub const EXTENSION: &str = "zip";

/// The largest backup file read.
pub const MAX_FILE: u64 = 256 * 1024 * 1024;
/// The most entries in it.
pub const MAX_ENTRIES: usize = 512;
/// The largest manifest or section.
pub const MAX_JSON: u64 = 16 * 1024 * 1024;

const MANIFEST: &str = "manifest.json";

/// A folder of the viewer whose files a section may name.
pub struct Folder {
    /// Its name in `<folder>/<name>`.
    pub name: &'static str,
    /// Where it is, from the profile's settings folder.
    pub path: fn(&Path) -> PathBuf,
    /// Whether a file of the backup may be written there under this name,
    /// with these bytes.
    pub accepts: fn(&str, &[u8]) -> bool,
}

/// The folders whose files go into a backup.
pub const FOLDERS: &[Folder] = &[Folder {
    name: "backgrounds",
    path: |config| config.join("folderstorm_backgrounds"),
    accepts: |name, bytes| {
        use crate::folderstorm::backgrounds::{import, record};
        record::managed_name(name)
            && bytes.len() as u64 <= import::MAX_BYTES
            // the name is the hash of the picture: anything else was edited
            && name.get(..16) == Some(format!("{:016x}", import::hash(bytes)).as_str())
    },
}];

/// A file's `<folder>/<name>`, split; None when it is not one of ours.
pub fn split(file: &str) -> Option<(&'static Folder, &str)> {
    let (folder, name) = file.split_once('/')?;
    let folder = FOLDERS.iter().find(|f| f.name == folder)?;
    let plain = !name.is_empty() && !name.contains(['/', '\\', ':']) && !name.starts_with('.');
    plain.then_some((folder, name))
}

/// What made the backup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Export… in Preferences.
    #[default]
    Manual,
    /// The state before an import, to undo it.
    BeforeImport,
    /// The daily automatic backup.
    Daily,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Manual, Kind::BeforeImport, Kind::Daily];

    /// Its name in a file name of the backups folder.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Manual => "manual",
            Kind::BeforeImport => "before-import",
            Kind::Daily => "daily",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Manual => aurora_i18n::tr!("fs-backup-kind-manual"),
            Kind::BeforeImport => aurora_i18n::tr!("fs-backup-kind-before-import"),
            Kind::Daily => aurora_i18n::tr!("fs-backup-kind-daily"),
        }
    }
}

/// One section as the manifest lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub id: String,
    /// Its name when written (shown for a section this version does not
    /// know).
    pub label: String,
    /// The files it uses, `<folder>/<name>`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

/// The manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    /// The viewer's name and version that wrote it.
    pub product: String,
    pub viewer: String,
    /// Local time, `YYYY-MM-DD_HHMMSS`.
    pub created: String,
    pub kind: Kind,
    /// The account logged in when it was written; None before login.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
    pub sections: Vec<Entry>,
}

/// A backup, written or read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Backup {
    pub manifest: Manifest,
    /// Each section's value, by id.
    pub values: BTreeMap<String, Value>,
    /// The files' bytes, by `<folder>/<name>`; filled off the frame
    /// ([`Backup::read_files`]) or when read.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Why a file was not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// Not a ZIP, or a damaged one.
    NotZip,
    /// A ZIP, but not one of these backups.
    NotABackup,
    /// Over the limits.
    TooLarge,
}

impl ReadError {
    pub fn text(self) -> &'static str {
        match self {
            ReadError::NotZip | ReadError::NotABackup => aurora_i18n::tr!("fs-backup-not-a-backup"),
            ReadError::TooLarge => aurora_i18n::tr!("fs-backup-too-large"),
        }
    }
}

impl Backup {
    /// The setup as it is now, every section that has something to write:
    /// taken on the frame. The files follow off it ([`Backup::read_files`]).
    pub fn capture(env: &Env, kind: Kind, account: Option<Account>) -> Backup {
        let mut backup = Backup {
            manifest: Manifest {
                format: FORMAT.into(),
                version: VERSION,
                product: folderstorm_brand::PRODUCT.into(),
                viewer: aurora_version::version().to_string(),
                created: crate::snapshot::local_stamp(),
                kind,
                account,
                sections: Vec::new(),
            },
            ..Backup::default()
        };
        for section in sections::SECTIONS {
            let Some(mut exported) = section.export(env) else {
                continue;
            };
            let found = secrets::findings(&exported.value);
            if !found.is_empty() {
                // a section must never write these: said, and left out
                log::warn!("backup: section {} left out private values at {}", section.id(), found.join(", "));
                secrets::scrub(&mut exported.value);
            }
            exported.files.retain(|file| split(file).is_some());
            exported.files.sort();
            exported.files.dedup();
            backup.manifest.sections.push(Entry {
                id: section.id().into(),
                label: section.label(),
                files: exported.files,
            });
            backup.values.insert(section.id().into(), exported.value);
        }
        backup
    }

    /// Reads the files the sections name from the profile's folder
    /// `config`; a missing one is left out (its section shows the colour
    /// instead, as here).
    pub fn read_files(&mut self, config: &Path) {
        let wanted: Vec<String> = self.manifest.sections.iter().flat_map(|e| e.files.clone()).collect();
        for file in wanted {
            let Some((folder, name)) = split(&file) else {
                continue;
            };
            match std::fs::read((folder.path)(config).join(name)) {
                Ok(bytes) if (folder.accepts)(name, &bytes) => {
                    self.files.insert(file, bytes);
                }
                Ok(_) => log::info!("backup: a file of {} is not as named, left out", folder.name),
                Err(_) => log::info!("backup: a file of {} is missing, left out", folder.name),
            }
        }
    }

    pub fn to_zip(&self) -> std::io::Result<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file(MANIFEST, options).map_err(std::io::Error::other)?;
        zip.write_all(&serde_json::to_vec_pretty(&self.manifest).map_err(std::io::Error::other)?)?;
        for (id, value) in &self.values {
            zip.start_file(format!("sections/{id}.json"), options)
                .map_err(std::io::Error::other)?;
            zip.write_all(&serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?)?;
        }
        for (file, bytes) in &self.files {
            // (pictures are compressed already)
            zip.start_file(format!("files/{file}"), stored).map_err(std::io::Error::other)?;
            zip.write_all(bytes)?;
        }
        Ok(zip.finish().map_err(std::io::Error::other)?.into_inner())
    }

    /// A backup read back from a file's bytes.
    pub fn from_zip(bytes: &[u8]) -> Result<Backup, ReadError> {
        if bytes.len() as u64 > MAX_FILE {
            return Err(ReadError::TooLarge);
        }
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| ReadError::NotZip)?;
        if zip.len() > MAX_ENTRIES {
            return Err(ReadError::TooLarge);
        }
        let manifest = read_entry(&mut zip, MANIFEST, MAX_JSON)?.ok_or(ReadError::NotABackup)?;
        let manifest: Manifest = serde_json::from_slice(&manifest).map_err(|_| ReadError::NotABackup)?;
        if manifest.format != FORMAT {
            return Err(ReadError::NotABackup);
        }
        let mut backup = Backup {
            manifest,
            ..Backup::default()
        };
        // only what the manifest lists, once, under a plain name
        let mut seen = std::collections::BTreeSet::new();
        backup.manifest.sections.retain(|entry| valid_id(&entry.id) && seen.insert(entry.id.clone()));
        let mut total = 0u64;
        for entry in &mut backup.manifest.sections {
            entry.label = entry.label.chars().filter(|c| !c.is_control()).take(80).collect();
            if let Some(bytes) = read_entry(&mut zip, &format!("sections/{}.json", entry.id), MAX_JSON)? {
                total += bytes.len() as u64;
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(mut value) => {
                        secrets::scrub(&mut value);
                        backup.values.insert(entry.id.clone(), value);
                    }
                    Err(_) => log::info!("backup: section {} does not read, left out", entry.id),
                }
            }
            entry.files.retain(|file| split(file).is_some());
            for file in &entry.files {
                if backup.files.contains_key(file) {
                    continue;
                }
                let Some((folder, name)) = split(file) else { continue };
                let Some(bytes) = read_entry(&mut zip, &format!("files/{file}"), MAX_FILE)? else {
                    continue;
                };
                total += bytes.len() as u64;
                if total > MAX_FILE {
                    return Err(ReadError::TooLarge);
                }
                if (folder.accepts)(name, &bytes) {
                    backup.files.insert(file.clone(), bytes);
                } else {
                    log::info!("backup: a file of {} is not as named, left out", folder.name);
                }
            }
        }
        // a section listed without its value is not offered
        let values = &backup.values;
        backup.manifest.sections.retain(|entry| values.contains_key(&entry.id));
        Ok(backup)
    }

    /// Written by a newer version of the format.
    pub fn newer(&self) -> bool {
        self.manifest.version > VERSION
    }

    /// The sections this version does not know.
    pub fn unknown(&self) -> Vec<&Entry> {
        self.manifest
            .sections
            .iter()
            .filter(|entry| sections::find(&entry.id).is_none())
            .collect()
    }

    /// When it was written, `YYYY-MM-DD HH:MM`.
    pub fn created(&self) -> String {
        created_text(&self.manifest.created)
    }
}

/// `YYYY-MM-DD_HHMMSS` as `YYYY-MM-DD HH:MM`.
pub fn created_text(stamp: &str) -> String {
    match (stamp.get(..10), stamp.get(11..13), stamp.get(13..15)) {
        (Some(date), Some(h), Some(m)) => format!("{date} {h}:{m}"),
        _ => stamp.chars().filter(|c| !c.is_control()).take(20).collect(),
    }
}

/// A section's id as the file may have it.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// One entry of the ZIP, at most `limit` bytes; None when it is not there.
fn read_entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str, limit: u64) -> Result<Option<Vec<u8>>, ReadError> {
    let entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(_) => return Err(ReadError::NotZip),
    };
    if entry.size() > limit {
        return Err(ReadError::TooLarge);
    }
    let mut bytes = Vec::new();
    entry
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadError::NotZip)?;
    if bytes.len() as u64 > limit {
        return Err(ReadError::TooLarge);
    }
    Ok(Some(bytes))
}
