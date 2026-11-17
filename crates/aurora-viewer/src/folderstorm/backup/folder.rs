//! The backups folder: `folderstorm_backups` in the profile's settings
//! folder. It holds the automatic backups only (a safety backup before each
//! import, the daily ones when chosen); Export… writes where the user says.
//!
//! A file's name says what it is: `<kind>_<YYYY-MM-DD_HHMMSS>_<account>.zip`,
//! the account as its id's 32 hex digits (or `none`). Only names
//! of that shape are listed, restored or removed.

use super::file::{self, Kind, Manifest};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The most backups listed.
pub const MAX_LISTED: usize = 40;

pub fn dir(config: &Path) -> PathBuf {
    config.join("folderstorm_backups")
}

/// The short form of an account in a file name.
fn account_key(account: Option<Uuid>) -> String {
    account.map_or_else(|| "none".to_owned(), |id| id.simple().to_string())
}

pub fn file_name(kind: Kind, stamp: &str, account: Option<Uuid>) -> String {
    format!("{}_{stamp}_{}.{}", kind.key(), account_key(account), file::EXTENSION)
}

/// What a name of the folder says: its kind, its time and its account key.
pub fn parse_name(name: &str) -> Option<(Kind, String, String)> {
    let stem = name.strip_suffix(&format!(".{}", file::EXTENSION))?;
    let mut parts = stem.splitn(4, '_');
    let kind = parts.next()?;
    let (date, time, account) = (parts.next()?, parts.next()?, parts.next()?);
    let kind = Kind::ALL.into_iter().find(|k| k.key() == kind)?;
    let digits = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || b == b'-');
    let account_ok = account == "none" || (account.len() == 32 && account.bytes().all(|b| b.is_ascii_hexdigit()));
    (digits(date, 10) && digits(time, 6) && account_ok).then(|| (kind, format!("{date}_{time}"), account.to_owned()))
}

/// Writes a backup's bytes into the folder; its path.
pub fn write(config: &Path, kind: Kind, stamp: &str, account: Option<Uuid>, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let dir = dir(config);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(file_name(kind, stamp, account));
    crate::atomic_file::write(&path, bytes)?;
    Ok(path)
}

/// Keeps the newest `keep` backups of `kind` for `account`, removing the
/// older ones.
pub fn prune(config: &Path, kind: Kind, account: Option<Uuid>, keep: usize) {
    let key = account_key(account);
    let mut names: Vec<(String, String)> = names(config)
        .into_iter()
        .filter_map(|name| {
            let (k, stamp, acct) = parse_name(&name)?;
            (k == kind && acct == key).then_some((stamp, name))
        })
        .collect();
    names.sort();
    let over = names.len().saturating_sub(keep);
    for (_, name) in names.into_iter().take(over) {
        if let Err(e) = std::fs::remove_file(dir(config).join(&name)) {
            log::info!("backup: an old automatic backup was not removed ({e})");
        }
    }
}

/// The names of the folder that are backups.
fn names(config: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir(config)) else {
        return Vec::new();
    };
    read.flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| parse_name(name).is_some())
        .collect()
}

/// Whether `account` has a daily backup of the day `date` (`YYYY-MM-DD`).
pub fn has_daily(config: &Path, account: Option<Uuid>, date: &str) -> bool {
    let key = account_key(account);
    names(config)
        .iter()
        .filter_map(|name| parse_name(name))
        .any(|(kind, stamp, acct)| kind == Kind::Daily && acct == key && stamp.get(..10) == Some(date))
}

/// A backup of the folder, as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub path: PathBuf,
    pub kind: Kind,
    /// `YYYY-MM-DD HH:MM`.
    pub created: String,
    /// The account's name, from the manifest.
    pub account: Option<String>,
    /// How many sections it holds.
    pub sections: usize,
}

/// The backups of the folder, newest first, at most [`MAX_LISTED`]; the
/// manifest of each is read for its account and its sections.
pub fn list(config: &Path) -> Vec<Listed> {
    let mut found: Vec<(String, String, Kind)> = names(config)
        .into_iter()
        .filter_map(|name| {
            let (kind, stamp, _) = parse_name(&name)?;
            Some((stamp, name, kind))
        })
        .collect();
    found.sort_by(|a, b| (&b.0, &b.1).cmp(&(&a.0, &a.1)));
    found
        .into_iter()
        .take(MAX_LISTED)
        .map(|(stamp, name, kind)| {
            let path = dir(config).join(name);
            let manifest = read_manifest(&path);
            Listed {
                created: file::created_text(&stamp),
                account: manifest.as_ref().and_then(|m| m.account.as_ref()).map(|a| a.name.clone()),
                sections: manifest.as_ref().map_or(0, |m| m.sections.len()),
                path,
                kind,
            }
        })
        .collect()
}

/// The manifest of a backup file, without reading the rest.
fn read_manifest(path: &Path) -> Option<Manifest> {
    use std::io::Read;
    let reader = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(reader).ok()?;
    let entry = zip.by_name("manifest.json").ok()?;
    let mut bytes = Vec::new();
    entry.take(file::MAX_JSON).read_to_end(&mut bytes).ok()?;
    let manifest: Manifest = serde_json::from_slice(&bytes).ok()?;
    (manifest.format == file::FORMAT).then_some(manifest)
}
