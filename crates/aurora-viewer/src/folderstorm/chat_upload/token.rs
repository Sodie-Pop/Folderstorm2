//! The image host's token: the user's own, kept in the system's credential
//! store (Windows Credential Manager, macOS Keychain, Secret Service on
//! Linux) under `folderstorm_brand::KEYRING_IMAGE_HOST`, with the host's
//! address as the account, like the login secrets (`credentials.rs`); or
//! the token a release build carries for Folderstorm's own host.
//!
//! A token is never written to the settings, the logs or the screen, and
//! never sent to another host than the one it belongs to.

/// The token a build may carry for the default host, from the
/// `FOLDERSTORM_IMAGE_HOST_TOKEN` variable at compile time (a release
/// workflow's secret). Never committed.
pub fn built_in() -> Option<&'static str> {
    option_env!("FOLDERSTORM_IMAGE_HOST_TOKEN").filter(|token| !token.trim().is_empty())
}

/// What the upload sends as `authorization`.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    /// The user's own token for this host.
    User(String),
    /// The build's token: only for the default host.
    BuiltIn(String),
    /// No header: a host that takes uploads without a token.
    None,
}

impl Auth {
    pub fn header(&self) -> Option<&str> {
        match self {
            Auth::User(token) | Auth::BuiltIn(token) => Some(token.trim()),
            Auth::None => None,
        }
    }
}

impl std::fmt::Debug for Auth {
    // the token never reaches a log, even through a debug print
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Auth::User(_) => "Auth::User(…)",
            Auth::BuiltIn(_) => "Auth::BuiltIn(…)",
            Auth::None => "Auth::None",
        })
    }
}

/// The user's token for `origin` if they saved one; else the build's, only
/// when `origin` is the default host (a token made for Folderstorm's host
/// must never go to another); else none.
pub fn resolve(origin: &str, user: Option<String>, built_in: Option<&str>) -> Auth {
    if let Some(token) = user.filter(|t| usable(t)) {
        return Auth::User(token);
    }
    let default = super::settings::origin(folderstorm_brand::IMAGE_HOST);
    if let Some(token) = built_in.filter(|t| usable(t))
        && default.as_deref() == Some(origin)
    {
        return Auth::BuiltIn(token.to_owned());
    }
    Auth::None
}

/// A token that can go in a header: printable ASCII, bounded.
pub fn usable(token: &str) -> bool {
    let token = token.trim();
    !token.is_empty() && token.len() <= 4096 && token.bytes().all(|b| (0x21..0x7f).contains(&b))
}

/// Where the user's tokens are kept, one per host.
pub trait TokenStore: Send + Sync {
    fn read(&self, origin: &str) -> Option<String>;
    fn write(&self, origin: &str, token: &str) -> Result<(), String>;
    fn remove(&self, origin: &str) -> Result<(), String>;
}

/// The system's credential store.
pub struct SystemStore;

fn entry(origin: &str) -> Option<keyring::Entry> {
    // the isolated update tests never reach the user's store
    if aurora_updater::test_environment().is_some() || origin.is_empty() {
        return None;
    }
    keyring::Entry::new(folderstorm_brand::KEYRING_IMAGE_HOST, origin)
        .map_err(|e| log::warn!("chat upload: credential store unavailable: {e}"))
        .ok()
}

impl TokenStore for SystemStore {
    fn read(&self, origin: &str) -> Option<String> {
        match entry(origin)?.get_password() {
            Ok(token) => Some(token),
            Err(keyring::Error::NoEntry) => None,
            Err(_) => {
                log::warn!("chat upload: the image host token is unreadable");
                None
            }
        }
    }

    fn write(&self, origin: &str, token: &str) -> Result<(), String> {
        let entry = entry(origin).ok_or_else(|| "no credential store".to_owned())?;
        // the error's text names the store, never the secret
        entry.set_password(token.trim()).map_err(|e| e.to_string())
    }

    fn remove(&self, origin: &str) -> Result<(), String> {
        let Some(entry) = entry(origin) else { return Ok(()) };
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
