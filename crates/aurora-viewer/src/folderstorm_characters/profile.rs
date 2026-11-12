//! `--profile <name>` in the viewer (session-host plan, section 5; the rules
//! are `folderstorm_session::profile`). Read through the lazy `cli::get()`:
//! the settings file is read on the first line of `main`, before anything
//! else starts, and its folder already depends on the profile.
//!
//! - settings: `<settings folder>/profiles/<name>` (`settings::config_dir`);
//! - asset cache: its own folder beside the main cache, or the main cache
//!   when the profile's settings say to share it (`settings::cache_dir`);
//! - the cache key's fallback file stays in the main settings folder, so
//!   every profile seals its cache with the one key (the credential store's
//!   entries are shared too: saved passwords are keyed by grid and account);
//! - one process per profile: `profile.lock` in its settings folder, held
//!   for the process's life; a second launch brings the first forward;
//! - the `secondlife://` hand-over, the log and the window title name it.

use ::folderstorm_session::profile::{self as rules, NameError, ProfileName};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long a second launch tries to reach the viewer of its profile (it
/// may be starting) or to take the profile over (it may be closing).
const LEASE_WAIT: Duration = Duration::from_secs(10);

/// The lease file in the profile's settings folder.
const LEASE_FILE: &str = "profile.lock";

/// The profile of this process; None for the main profile, and for a name
/// that is refused ([`check`] then ends the process before anything is
/// written).
pub fn current() -> Option<&'static ProfileName> {
    if cfg!(test) {
        // unit tests run as the main profile, whatever their arguments
        return None;
    }
    static PROFILE: OnceLock<Option<ProfileName>> = OnceLock::new();
    PROFILE
        .get_or_init(|| crate::cli::get().profile.as_deref().and_then(|raw| ProfileName::parse(raw).ok()))
        .as_ref()
}

/// The `--profile` value refused, with why.
fn refused() -> Option<(&'static str, NameError)> {
    let raw = crate::cli::get().profile.as_deref()?;
    ProfileName::parse(raw).err().map(|e| (raw, e))
}

/// Before anything opens: false when the profile name is refused (the user
/// is told why, and the process ends).
pub fn check() -> bool {
    let Some((raw, error)) = refused() else {
        return true;
    };
    let why = match error {
        NameError::Empty => aurora_i18n::tr!("folderstorm-profile-empty").to_owned(),
        NameError::TooLong => aurora_i18n::tr!("folderstorm-profile-too-long", max = rules::MAX_LEN),
        NameError::Character(c) => aurora_i18n::tr!("folderstorm-profile-character", character = c.to_string()),
        NameError::Start => aurora_i18n::tr!("folderstorm-profile-start").to_owned(),
        NameError::Reserved => aurora_i18n::tr!("folderstorm-profile-reserved").to_owned(),
    };
    tell(&aurora_i18n::tr!("folderstorm-profile-refused", name = raw.to_owned(), why = why));
    false
}

/// A message before the window: on the console, and in a native box (a
/// Windows release build has no console). None in automated runs.
fn tell(text: &str) {
    eprintln!("{text}");
    if std::env::var_os("AURORA_CAPTURE").is_some() || std::env::var_os("AURORA_PROFILE").is_some() {
        return;
    }
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(folderstorm_brand::PRODUCT)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// `settings::config_dir` of this process, from the main one.
pub fn config_dir(main: PathBuf) -> PathBuf {
    rules::config_dir(main, current())
}

/// The main profile's settings folder, whatever this process's profile.
pub fn main_config_dir() -> PathBuf {
    rules::main_config_dir(crate::settings::config_dir(), current())
}

/// `settings::cache_dir` of this process, from the main cache root (the
/// default or chosen folder, before the demo's subfolder).
pub fn cache_root(main: PathBuf) -> PathBuf {
    rules::cache_root(main, current(), share_main_cache())
}

/// The folder of every profile's own cache, for the list of folders left
/// after an uninstall (whatever this process's profile).
pub fn profile_caches() -> PathBuf {
    rules::profile_caches(crate::settings::default_cache_root())
}

/// Whether this profile shares the main cache: read once from its settings
/// file, so that every cache folder of the run is the same one (a change
/// applies at the next start, as the cache location does).
fn share_main_cache() -> bool {
    static SHARE: OnceLock<bool> = OnceLock::new();
    *SHARE.get_or_init(|| {
        current().is_some()
            && std::fs::read(crate::settings::Settings::path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|v| {
                    v.pointer("/folderstorm_characters/share_main_cache")
                        .and_then(serde_json::Value::as_bool)
                })
                .unwrap_or(false)
    })
}

/// Whether the cache this process used last (`last`) may be removed now
/// that its cache is elsewhere: always for the main profile, as before; a
/// named profile only removes its own, never the main cache it may have
/// shared.
pub fn may_remove_cache(last: &Path) -> bool {
    current().is_none_or(|name| rules::owns_cache(last, name))
}

/// The scope of the Windows `secondlife://` hand-over, from `--title`'s.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn scope_slug(title: Option<String>) -> Option<String> {
    rules::scope_slug(current(), title.as_deref())
}

/// What the profile adds to the Linux session-bus name: `.P_<name>`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn bus_suffix() -> String {
    current().map(|name| format!(".{}", rules::bus_element(name))).unwrap_or_default()
}

/// The log's name for a named profile (`aurora-alt1`); None for the main
/// profile.
pub fn log_stem(base: &str) -> Option<String> {
    rules::log_stem(base, current(), crate::cli::title_slug().as_deref())
}

/// The window title's text after the version: the profile, then `--title`.
pub fn title_text(title: Option<&str>) -> Option<String> {
    rules::title_text(current(), title)
}

/// The arguments that keep this profile across a restart of the viewer.
pub fn restart_args() -> Vec<String> {
    current()
        .map(|name| vec!["--profile".to_owned(), name.shown().to_owned()])
        .unwrap_or_default()
}

/// Whether a link handed over by a later launch is a request to bring this
/// window forward (a second launch of this profile).
pub fn is_raise_request(url: &str) -> bool {
    url == folderstorm_brand::PROFILE_RAISE_REQUEST
}

/// The process's hold on its profile folder.
static LEASE: OnceLock<std::fs::File> = OnceLock::new();

/// One process per named profile (the main profile allows several, as
/// before). False when this launch must end: the viewer of this profile
/// was brought forward, or it could not be reached nor taken over.
pub fn take_lease() -> bool {
    if current().is_none() {
        return true;
    }
    let dir = crate::settings::config_dir();
    let file = std::fs::create_dir_all(&dir).and_then(|()| {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LEASE_FILE))
    });
    let file = match file {
        Ok(file) => file,
        Err(e) => {
            // (no log yet: the console)
            eprintln!("profile lease not taken ({e}): starting without it");
            return true;
        }
    };
    let until = Instant::now() + LEASE_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => {
                let _ = LEASE.set(file);
                return true;
            }
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(e)) => {
                eprintln!("profile lease not taken ({e}): starting without it");
                return true;
            }
        }
        // the viewer that holds it brings its window forward
        if crate::slurl_handoff::hand_off(folderstorm_brand::PROFILE_RAISE_REQUEST) {
            return false;
        }
        if Instant::now() >= until {
            let name = current().map(ProfileName::shown).unwrap_or_default().to_owned();
            tell(&aurora_i18n::tr!("folderstorm-profile-in-use", name = name));
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
