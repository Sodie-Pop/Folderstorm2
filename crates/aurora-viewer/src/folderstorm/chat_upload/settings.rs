//! The saved choices of chat image uploads (Preferences › Chat › Image
//! uploads). The host's token is never here: it lives in the system's
//! credential store (`token.rs`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Largest image the user can allow, in MiB. Zipline's own default limit
/// is far above it; Folderstorm's host takes 10 MiB.
pub const MAX_MIB_LIMIT: u32 = 32;
/// The default size limit, Folderstorm's host's.
pub const DEFAULT_MAX_MIB: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Stored")]
pub struct UploadSettings {
    /// Off until the user turns it on: nothing leaves the viewer before.
    pub enabled: bool,
    /// The host's address: `https://` and a host name, nothing after it.
    pub host: String,
    /// The Zipline folder that takes uploads (`x-zipline-folder`) on each
    /// host, by origin (`https://host[:port]`): a folder entered for one
    /// host is never sent to another. Needed by a host that takes uploads
    /// without a token.
    pub folders: BTreeMap<String, String>,
    /// Largest image sent, in MiB.
    pub max_mib: u32,
    /// Show what will be uploaded and wait for Upload, every time.
    pub confirm: bool,
}

impl Default for UploadSettings {
    fn default() -> Self {
        UploadSettings {
            enabled: false,
            host: folderstorm_brand::IMAGE_HOST.to_owned(),
            folders: BTreeMap::new(),
            max_mib: DEFAULT_MAX_MIB,
            confirm: true,
        }
    }
}

impl UploadSettings {
    /// The size limit in bytes, within the allowed range.
    pub fn max_bytes(&self) -> u64 {
        u64::from(self.max_mib.clamp(1, MAX_MIB_LIMIT)) * 1024 * 1024
    }

    /// The host's origin when the address is usable.
    pub fn origin(&self) -> Option<String> {
        origin(&self.host)
    }

    /// The folder the user entered for `origin`, when one is set and
    /// well-formed.
    pub fn folder_for(&self, origin: &str) -> Option<&str> {
        let folder = self.folders.get(origin)?.trim();
        valid_folder(folder).then_some(folder)
    }

    /// What the user typed as the folder of `origin` (empty: none).
    pub fn folder_text(&self, origin: &str) -> &str {
        self.folders.get(origin).map_or("", String::as_str)
    }

    /// Sets (or, empty, removes) the folder of `origin`.
    pub fn set_folder(&mut self, origin: &str, folder: String) {
        if folder.is_empty() {
            self.folders.remove(origin);
        } else {
            self.folders.insert(origin.to_owned(), folder);
        }
    }
}

/// The section as it is read: today's, or an older one whose single
/// `folder` belonged to the host set with it.
#[derive(Deserialize)]
#[serde(default)]
struct Stored {
    enabled: bool,
    host: String,
    folders: BTreeMap<String, String>,
    /// Before folders were kept by host.
    folder: String,
    max_mib: u32,
    confirm: bool,
}

impl Default for Stored {
    fn default() -> Self {
        let d = UploadSettings::default();
        Stored {
            enabled: d.enabled,
            host: d.host,
            folders: d.folders,
            folder: String::new(),
            max_mib: d.max_mib,
            confirm: d.confirm,
        }
    }
}

impl From<Stored> for UploadSettings {
    fn from(s: Stored) -> Self {
        let mut folders = s.folders;
        // the old folder goes with the host it was entered with, and with
        // no other; without a usable host it is dropped
        let old = s.folder.trim();
        if !old.is_empty()
            && let Some(origin) = origin(&s.host)
        {
            folders.entry(origin).or_insert_with(|| old.to_owned());
        }
        UploadSettings {
            enabled: s.enabled,
            host: s.host,
            folders,
            max_mib: s.max_mib,
            confirm: s.confirm,
        }
    }
}

/// A Zipline folder id: letters, digits, `-` and `_`.
pub fn valid_folder(folder: &str) -> bool {
    !folder.is_empty() && folder.len() <= 64 && folder.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The upload folder a build may carry for the default host, from the
/// `FOLDERSTORM_IMAGE_HOST_FOLDER` variable at compile time. Anyone who
/// has it can upload to that folder: it is never committed, logged or
/// shown.
pub fn built_in_folder() -> Option<&'static str> {
    option_env!("FOLDERSTORM_IMAGE_HOST_FOLDER")
        .map(str::trim)
        .filter(|f| valid_folder(f))
}

/// The folder an upload to `origin` names: the user's own if they entered
/// one; else the build's, only for the default host; else none.
pub fn resolve_folder(origin: &str, user: Option<&str>, built_in: Option<&str>) -> Option<String> {
    if let Some(folder) = user.map(str::trim).filter(|f| valid_folder(f)) {
        return Some(folder.to_owned());
    }
    let default = self::origin(folderstorm_brand::IMAGE_HOST);
    built_in
        .map(str::trim)
        .filter(|f| valid_folder(f) && default.as_deref() == Some(origin))
        .map(str::to_owned)
}

/// `https://host[:port]` of an address the user typed: a host and nothing
/// else (no user name, no query). Plain `http://` only to this computer,
/// for local tests.
pub fn origin(address: &str) -> Option<String> {
    let address = address.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(address).ok()?;
    let host = url.host_str()?;
    let secure = url.scheme() == "https";
    if !(secure || (url.scheme() == "http" && is_loopback(host))) {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    if url.path() != "/" && !url.path().is_empty() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

pub(super) fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

/// The host name shown to the user ("images.sodie.online").
pub fn host_name(origin: &str) -> String {
    reqwest::Url::parse(origin)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}
