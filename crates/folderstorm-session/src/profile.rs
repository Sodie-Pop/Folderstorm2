//! `--profile <name>`: one character's own settings, cache, log, link
//! hand-over and window title (session-host plan, section 5). The main
//! profile (no argument) keeps every path and name it had: each function
//! here gives back what it was given when there is no profile.
//!
//! A name is checked before it is used anywhere: 1 to [`MAX_LEN`] letters,
//! digits, `_` or `-`, starting with a letter or a digit, compared without
//! case (Windows folders ignore it, so `Alt1` and `alt1` are one profile on
//! every system); `main` is the main profile's. Anything else is refused,
//! never turned into a path.

use std::path::{Path, PathBuf};

/// The longest profile name.
pub const MAX_LEN: usize = 32;

/// Names a profile cannot take.
const RESERVED: &[&str] = &["main"];

/// A checked profile name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileName {
    /// As typed: the window title shows it.
    shown: String,
    /// Lower case: folders, link scopes and the log name use it.
    key: String,
}

/// Why a name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// A character other than a letter, a digit, `_` or `-`.
    Character(char),
    /// It starts with `_` or `-`.
    Start,
    Reserved,
}

impl ProfileName {
    pub fn parse(raw: &str) -> Result<Self, NameError> {
        let name = raw.trim();
        let Some(first) = name.chars().next() else {
            return Err(NameError::Empty);
        };
        if name.chars().count() > MAX_LEN {
            return Err(NameError::TooLong);
        }
        if let Some(c) = name.chars().find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-')) {
            return Err(NameError::Character(c));
        }
        if !first.is_ascii_alphanumeric() {
            return Err(NameError::Start);
        }
        let key = name.to_ascii_lowercase();
        if RESERVED.contains(&key.as_str()) {
            return Err(NameError::Reserved);
        }
        Ok(Self {
            shown: name.to_owned(),
            key,
        })
    }

    pub fn shown(&self) -> &str {
        &self.shown
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

/// The settings folder of a profile inside the main one (`main`, the demo's
/// subfolder included): `<main>/profiles/<name>`.
pub fn config_dir(main: PathBuf, name: Option<&ProfileName>) -> PathBuf {
    match name {
        Some(name) => main.join(folderstorm_brand::PROFILES_FOLDER).join(name.key()),
        None => main,
    }
}

/// The main settings folder, from a profile's: what [`config_dir`] added
/// taken off.
pub fn main_config_dir(dir: PathBuf, name: Option<&ProfileName>) -> PathBuf {
    match name {
        Some(_) => dir.parent().and_then(Path::parent).map_or(dir.clone(), Path::to_path_buf),
        None => dir,
    }
}

/// The cache root of a profile, from the main one (`main`: the default or
/// chosen cache folder, before the demo's subfolder). Its own folder by
/// default, beside the main cache and never inside it: the main viewer
/// removes its whole cache folder when the user moves it
/// (`cache_location.rs`). `share`: the main cache itself (decision 4 of the
/// plan: an option per profile; the files are safe with several writers).
pub fn cache_root(main: PathBuf, name: Option<&ProfileName>, share: bool) -> PathBuf {
    let Some(name) = name.filter(|_| !share) else {
        return main;
    };
    profile_caches(main).join(name.key())
}

/// The folder that holds the profiles' own caches, beside the main cache
/// `main`: `<main>-profiles`.
pub fn profile_caches(main: PathBuf) -> PathBuf {
    match main.file_name() {
        Some(leaf) => main.with_file_name(format!("{}-{}", leaf.to_string_lossy(), folderstorm_brand::PROFILES_FOLDER)),
        None => main.join(folderstorm_brand::PROFILES_FOLDER),
    }
}

/// Whether `path`, the cache a profile used at its last run, is that
/// profile's own (as [`cache_root`] makes it, the demo's subfolder
/// included): the only cache a profile may remove when its cache moves.
/// The main cache, which it may have shared, is never one.
pub fn owns_cache(path: &Path, name: &ProfileName) -> bool {
    let path = if path.file_name().is_some_and(|n| n == "demo") {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let suffix = format!("-{}", folderstorm_brand::PROFILES_FOLDER);
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name.key()))
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
}

/// The scope of the `secondlife://` hand-over on Windows (pipe and window
/// property names), from the `--title` part (`title`, already a slug of
/// letters, digits and dashes): a profile adds `@<name>`, which no title
/// can give, so a launch with `--profile Alt1 --url …` reaches Alt1 alone.
pub fn scope_slug(name: Option<&ProfileName>, title: Option<&str>) -> Option<String> {
    let Some(name) = name else {
        return title.map(str::to_owned);
    };
    Some(match title {
        Some(title) => format!("@{}-{title}", name.key()),
        None => format!("@{}", name.key()),
    })
}

/// The element a profile adds to the Linux session-bus name: `P_` then the
/// name, with `_` written `__` and `-` written `_d` (a bus name element
/// holds `[A-Za-z0-9_]` only), so that two names never give one element.
pub fn bus_element(name: &ProfileName) -> String {
    let mut element = String::from("P_");
    for c in name.key().chars() {
        match c {
            '_' => element.push_str("__"),
            '-' => element.push_str("_d"),
            c => element.push(c),
        }
    }
    element
}

/// The log's name for a profile: `<base>-<name>` (`aurora-alt1.log`), with
/// the `--title` slug after it when there is one. None for the main
/// profile, which keeps its own rule (`logging.rs`).
pub fn log_stem(base: &str, name: Option<&ProfileName>, title: Option<&str>) -> Option<String> {
    let name = name?;
    Some(match title {
        Some(title) => format!("{base}-{}-{title}", name.key()),
        None => format!("{base}-{}", name.key()),
    })
}

/// The text after the product name and version in the window title: the
/// profile as typed, then the `--title` text when there is one.
pub fn title_text(name: Option<&ProfileName>, title: Option<&str>) -> Option<String> {
    let Some(name) = name else {
        return title.map(str::to_owned);
    };
    Some(match title {
        Some(title) => format!("{} - {title}", name.shown()),
        None => name.shown().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> ProfileName {
        ProfileName::parse(raw).expect("a valid name")
    }

    #[test]
    fn names_are_short_and_safe_and_compared_without_case() {
        let alt = name(" Alt1 ");
        assert_eq!((alt.shown(), alt.key()), ("Alt1", "alt1"));
        assert_eq!(name("alt1").key(), name("ALT1").key());
        assert!(ProfileName::parse("a_b-9").is_ok());
        assert!(ProfileName::parse(&"x".repeat(MAX_LEN)).is_ok());
        assert_eq!(ProfileName::parse(""), Err(NameError::Empty));
        assert_eq!(ProfileName::parse("   "), Err(NameError::Empty));
        assert_eq!(ProfileName::parse(&"x".repeat(MAX_LEN + 1)), Err(NameError::TooLong));
        assert_eq!(ProfileName::parse("Main"), Err(NameError::Reserved));
        assert_eq!(ProfileName::parse("-x"), Err(NameError::Start));
        assert_eq!(ProfileName::parse("_x"), Err(NameError::Start));
        // nothing that could leave the profiles folder or name a device
        for (bad, c) in [
            ("../x", '.'),
            ("a/b", '/'),
            ("a\\b", '\\'),
            ("C:", ':'),
            ("a b", ' '),
            ("é", 'é'),
            ("a.b", '.'),
        ] {
            assert_eq!(ProfileName::parse(bad), Err(NameError::Character(c)), "{bad}");
        }
    }

    #[test]
    fn the_main_profile_keeps_every_path_and_name() {
        let root = PathBuf::from("/cfg/demo");
        assert_eq!(config_dir(root.clone(), None), root);
        assert_eq!(main_config_dir(root.clone(), None), root);
        assert_eq!(cache_root(PathBuf::from("/cache"), None, false), PathBuf::from("/cache"));
        assert_eq!(scope_slug(None, Some("test-x")).as_deref(), Some("test-x"));
        assert_eq!(scope_slug(None, None), None);
        assert_eq!(log_stem("aurora", None, Some("t")), None);
        assert_eq!(title_text(None, Some("T")).as_deref(), Some("T"));
        assert_eq!(title_text(None, None), None);
    }

    #[test]
    fn a_profile_has_its_own_settings_folder_inside_the_main_one() {
        let alt = name("Alt1");
        let dir = config_dir(PathBuf::from("/cfg/demo"), Some(&alt));
        assert_eq!(dir, Path::new("/cfg/demo").join("profiles").join("alt1"));
        assert_eq!(main_config_dir(dir, Some(&alt)), PathBuf::from("/cfg/demo"));
    }

    #[test]
    fn a_profile_cache_is_beside_the_main_cache_unless_shared() {
        let alt = name("Alt1");
        let own = cache_root(PathBuf::from("/c/Folderstorm2Cache"), Some(&alt), false);
        assert_eq!(own, Path::new("/c/Folderstorm2Cache-profiles").join("alt1"));
        assert!(!own.starts_with("/c/Folderstorm2Cache/"));
        assert_eq!(cache_root(PathBuf::from("/c/x"), Some(&alt), true), PathBuf::from("/c/x"));
        assert_eq!(cache_root(PathBuf::from(""), Some(&alt), false), Path::new("profiles").join("alt1"));
        assert_eq!(profile_caches(PathBuf::from("/c/cache")), PathBuf::from("/c/cache-profiles"));
    }

    #[test]
    fn a_profile_removes_only_its_own_cache() {
        let alt = name("Alt1");
        let own = cache_root(PathBuf::from("/c/cache"), Some(&alt), false);
        assert!(owns_cache(&own, &alt));
        assert!(owns_cache(&own.join("demo"), &alt));
        // the main cache it shared, another profile's, anything else
        assert!(!owns_cache(Path::new("/c/cache"), &alt));
        assert!(!owns_cache(Path::new("/c/cache/demo"), &alt));
        assert!(!owns_cache(
            &cache_root(PathBuf::from("/c/cache"), Some(&name("Alt2")), false),
            &alt
        ));
        assert!(!owns_cache(Path::new("/c/alt1"), &alt));
    }

    #[test]
    fn link_scopes_never_meet_a_title_or_another_profile() {
        let alt = name("Alt1");
        assert_eq!(scope_slug(Some(&alt), None).as_deref(), Some("@alt1"));
        assert_eq!(scope_slug(Some(&alt), Some("t")).as_deref(), Some("@alt1-t"));
        // a title slug holds letters, digits and dashes: never an `@`
        assert_ne!(scope_slug(None, Some("alt1")), scope_slug(Some(&alt), None));
        assert_eq!(bus_element(&alt), "P_alt1");
        assert_ne!(bus_element(&name("a-b")), bus_element(&name("a_b")));
        assert_ne!(bus_element(&name("a_d")), bus_element(&name("a-")));
        for raw in ["a-b", "a_b", "x__-_-", "9"] {
            assert!(bus_element(&name(raw)).chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        }
    }

    #[test]
    fn log_and_title_name_the_profile() {
        let alt = name("Alt1");
        assert_eq!(log_stem("aurora", Some(&alt), None).as_deref(), Some("aurora-alt1"));
        assert_eq!(
            log_stem("aurora-demo", Some(&alt), Some("t")).as_deref(),
            Some("aurora-demo-alt1-t")
        );
        assert_eq!(title_text(Some(&alt), None).as_deref(), Some("Alt1"));
        assert_eq!(title_text(Some(&alt), Some("Test")).as_deref(), Some("Alt1 - Test"));
    }
}
