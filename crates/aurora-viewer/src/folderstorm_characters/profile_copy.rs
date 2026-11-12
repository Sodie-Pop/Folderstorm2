//! "New character profile…" of the session host (session-host plan,
//! section 13, checkpoint 4): makes the settings folder of a new profile
//! from the main profile, another profile or nothing. What is copied and
//! what never is are `folderstorm_session::copy`'s rules; this module
//! reads and writes the files.
//!
//! - **Only into a profile that does not exist and does not run.** A
//!   folder already there is never written into, whatever it holds, and a
//!   profile whose viewer runs (its lease is held, or the host is starting
//!   it) is refused first.
//! - **All or nothing.** The new folder is filled under another name
//!   beside the profiles (`.new-<name>-<random>`, which is no profile
//!   name) and renamed into place at the end; anything that fails before
//!   removes it, and a profile is never found half made.
//! - **No account comes with it**: `accounts/`, the login screen's saved
//!   accounts, transcripts, caches and keys are not among what is copied,
//!   and the preferences are stripped of the login, the window and what is
//!   kept per account. The system's credential store is never read.
//!
//! Every function takes the main settings folder as an argument, so the
//! tests run on folders of their own.

use ::folderstorm_session::copy::{self, Part, Source};
use ::folderstorm_session::profile::{self as rules, ProfileName};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Why a profile was not made.
#[derive(Debug)]
pub enum CopyError {
    /// The profile runs in a viewer.
    Running,
    /// A profile of that name already exists.
    Exists,
    /// The profile to copy from does not exist.
    SourceMissing,
    Io(io::Error),
}

impl From<io::Error> for CopyError {
    fn from(error: io::Error) -> Self {
        CopyError::Io(error)
    }
}

/// What became of the source's preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preferences {
    /// Copied, without the login, the window and the per-account settings.
    Copied,
    /// The source has none (it never ran, or "the defaults" was asked).
    None,
    /// The source's file does not read as settings: nothing of it was
    /// written, and the new profile starts with the defaults.
    Unreadable,
}

/// A profile that was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Created {
    pub preferences: Preferences,
    /// The other files copied (the layout, the skins' files).
    pub files: usize,
}

/// The folder that holds the named profiles.
fn profiles_dir(main: &Path) -> PathBuf {
    main.join(folderstorm_brand::PROFILES_FOLDER)
}

/// The named profiles that exist in the main settings folder `main`, by
/// name: the folders whose name is a profile's.
pub fn existing(main: &Path) -> Vec<ProfileName> {
    let Ok(entries) = fs::read_dir(profiles_dir(main)) else {
        return Vec::new();
    };
    let mut names: Vec<ProfileName> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| ProfileName::parse(entry.file_name().to_str()?).ok())
        // (a folder is a profile's when it is named by its key)
        .filter(|name| name.shown() == name.key())
        .collect();
    names.sort_by(|a, b| a.key().cmp(b.key()));
    names.truncate(200);
    names
}

/// Makes the profile `target` in the main settings folder `main`, from
/// `source`. `running`: whether a profile's viewer runs now.
pub fn create(main: &Path, source: &Source, target: &ProfileName, running: &dyn Fn(&ProfileName) -> bool) -> Result<Created, CopyError> {
    if running(target) {
        return Err(CopyError::Running);
    }
    let place = rules::config_dir(main.to_path_buf(), Some(target));
    // (a link is something too: `symlink_metadata` does not follow it)
    if fs::symlink_metadata(&place).is_ok() {
        return Err(CopyError::Exists);
    }
    let from = match source {
        Source::Main => Some(main.to_path_buf()),
        Source::Profile(name) => {
            let dir = rules::config_dir(main.to_path_buf(), Some(name));
            if !dir.is_dir() {
                return Err(CopyError::SourceMissing);
            }
            Some(dir)
        }
        Source::Defaults => None,
    };
    let profiles = profiles_dir(main);
    fs::create_dir_all(&profiles)?;
    // a name no profile can have (it starts with a dot)
    let random = super::transport::random_hex(16);
    let staging = profiles.join(format!(".new-{}-{random}", target.key()));
    fs::create_dir(&staging)?;
    let made = fill(from.as_deref(), &staging).and_then(|created| {
        // made whole: into place, unless a profile appeared meanwhile
        if fs::symlink_metadata(&place).is_ok() {
            return Err(CopyError::Exists);
        }
        fs::rename(&staging, &place)?;
        Ok(created)
    });
    if made.is_err()
        && let Err(e) = fs::remove_dir_all(&staging)
    {
        log::warn!("profile copy: {} was not removed ({e})", staging.display());
    }
    made
}

/// Copies what a new profile takes of the settings folder `from` into the
/// empty folder `into`.
fn fill(from: Option<&Path>, into: &Path) -> Result<Created, CopyError> {
    let mut created = Created {
        preferences: Preferences::None,
        files: 0,
    };
    let Some(from) = from else {
        return Ok(created);
    };
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        // the main profile never ran: nothing to take
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(created),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // (not followed: a link is neither a file nor a folder here)
        let kind = entry.file_type()?;
        if !kind.is_file() && !kind.is_dir() {
            continue;
        }
        match copy::part(name, kind.is_dir()) {
            None => {}
            Some(Part::Settings) => created.preferences = preferences(&entry.path(), &into.join(copy::SETTINGS_FILE))?,
            Some(Part::File) => {
                if plain_file(&entry.path(), &into.join(name))? {
                    created.files += 1;
                }
            }
            Some(Part::Folder) => folder(&entry.path(), &into.join(name), 1, &mut created.files)?,
        }
    }
    Ok(created)
}

/// The source's preferences, stripped, written as the new profile's.
fn preferences(from: &Path, to: &Path) -> Result<Preferences, CopyError> {
    if fs::metadata(from)?.len() > copy::MAX_FILE_BYTES {
        return Ok(Preferences::Unreadable);
    }
    let bytes = fs::read(from)?;
    let Ok(mut settings) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(Preferences::Unreadable);
    };
    if !copy::strip_settings(&mut settings) {
        return Ok(Preferences::Unreadable);
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
    fs::write(to, bytes)?;
    Ok(Preferences::Copied)
}

/// Copies one file; false when it is too large to be a settings file.
fn plain_file(from: &Path, to: &Path) -> Result<bool, CopyError> {
    if fs::metadata(from)?.len() > copy::MAX_FILE_BYTES {
        log::warn!("profile copy: {} is too large and was left out", from.display());
        return Ok(false);
    }
    fs::copy(from, to)?;
    Ok(true)
}

/// Copies a folder's plain files and folders, links left out, to a depth
/// and a number of files a skin never reaches.
fn folder(from: &Path, to: &Path, depth: usize, files: &mut usize) -> Result<(), CopyError> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| copy::plain_name(name)) else {
            continue;
        };
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if depth < copy::MAX_DEPTH {
                folder(&entry.path(), &to.join(name), depth + 1, files)?;
            }
        } else if kind.is_file() {
            if *files >= copy::MAX_FILES {
                return Err(CopyError::Io(io::Error::other("too many files to be a skin")));
            }
            if plain_file(&entry.path(), &to.join(name))? {
                *files += 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::BTreeSet;

    fn name(raw: &str) -> ProfileName {
        ProfileName::parse(raw).expect("a valid name")
    }

    /// A main settings folder of its own for one test.
    fn main_dir(test: &str) -> PathBuf {
        let dir = crate::settings::config_dir()
            .join("profile-copy-tests")
            .join(format!("{test}-{}", super::super::transport::random_hex(12)));
        fs::create_dir_all(&dir).expect("folder");
        dir
    }

    fn put(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().expect("parent")).expect("folders");
        fs::write(file, text).expect("written");
    }

    /// Every file under `dir`, with `/` between the names.
    fn listing(dir: &Path) -> BTreeSet<String> {
        fn walk(root: &Path, dir: &Path, into: &mut BTreeSet<String>) {
            for entry in fs::read_dir(dir).expect("read").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, into);
                } else {
                    let relative = path.strip_prefix(root).expect("inside");
                    into.insert(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut files = BTreeSet::new();
        walk(dir, dir, &mut files);
        files
    }

    const ACCOUNT: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    /// A main profile as a viewer leaves it, with everything a copy must
    /// not take.
    fn used_main(test: &str) -> PathBuf {
        let main = main_dir(test);
        let settings = json!({
            "username": "someone.resident",
            "remember_username": true,
            "last_login": ["uri", "someone.resident"],
            "remember_password": true,
            "window_width": 1600,
            "window_x": 40,
            "font_scale": 1.25,
            "language": "fr",
            "keybinds": {"map": [["Jump", ["E", null]]]},
            "parcel_music_accounts": [ACCOUNT],
            "bridge": {ACCOUNT: {"enabled": true}},
        });
        put(&main, "settings.json", &settings.to_string());
        put(&main, "ui_layout.ron", "(layout)");
        put(&main, "skins/aurora/theme.json", "{\"accent\":1}");
        put(&main, "skins/aurora/layout.json", "{}");
        put(&main, "skins/aurora/icons/star.svg", "<svg/>");
        // never copied
        put(&main, "login_accounts.json", "[{\"user\":\"someone\"}]");
        put(&main, &format!("accounts/{ACCOUNT}/contact_sets.json"), "{}");
        put(&main, &format!("accounts/{ACCOUNT}/agni/chat/Jane Doe.txt"), "a private word");
        put(&main, "cache.key", "0123");
        put(&main, "rlva-login.json", "{}");
        put(&main, "session-host.json", "{}");
        put(&main, "session-host.lock", "");
        put(&main, "autoreplace.xml", "<x/>");
        put(&main, "photos/one.png", "png");
        put(
            &main,
            "profiles/old/settings.json",
            "{\"font_scale\":2.0,\"username\":\"other.resident\"}",
        );
        put(&main, "profiles/old/profile.lock", "");
        put(&main, "profiles/old/accounts/x/y.txt", "z");
        main
    }

    fn never(_: &ProfileName) -> bool {
        false
    }

    fn settings_of(main: &Path, profile: &str) -> Value {
        let path = rules::config_dir(main.to_path_buf(), Some(&name(profile))).join("settings.json");
        serde_json::from_slice(&fs::read(path).expect("settings")).expect("json")
    }

    #[test]
    fn a_copy_of_the_main_profile_has_its_interface_and_no_account() {
        let main = used_main("main");
        let created = create(&main, &Source::Main, &name("Alt1"), &never).expect("made");
        assert_eq!(
            created,
            Created {
                preferences: Preferences::Copied,
                files: 4
            }
        );
        let new = main.join("profiles").join("alt1");
        let expected: BTreeSet<String> = [
            "settings.json",
            "ui_layout.ron",
            "skins/aurora/theme.json",
            "skins/aurora/layout.json",
            "skins/aurora/icons/star.svg",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(listing(&new), expected);
        assert_eq!(
            settings_of(&main, "Alt1"),
            json!({"font_scale": 1.25, "language": "fr", "keybinds": {"map": [["Jump", ["E", null]]]}})
        );
        assert_eq!(fs::read_to_string(new.join("ui_layout.ron")).expect("layout"), "(layout)");
        // nothing of an account anywhere in the new profile
        for file in listing(&new) {
            let text = fs::read_to_string(new.join(&file)).expect("text");
            assert!(!text.contains("someone") && !text.contains(ACCOUNT), "{file}");
        }
        // the source is as it was, and no staging folder is left
        assert!(main.join("login_accounts.json").exists() && main.join("accounts").exists());
        assert_eq!(existing(&main), [name("alt1"), name("old")]);
        let left: Vec<String> = fs::read_dir(main.join("profiles"))
            .expect("profiles")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_copy_of_another_profile_takes_that_profile_s_interface() {
        let main = used_main("other");
        let created = create(&main, &Source::Profile(name("Old")), &name("New"), &never).expect("made");
        assert_eq!(
            created,
            Created {
                preferences: Preferences::Copied,
                files: 0
            }
        );
        assert_eq!(settings_of(&main, "new"), json!({"font_scale": 2.0}));
        let files = listing(&main.join("profiles").join("new"));
        assert_eq!(files.len(), 1, "no lock, no accounts: {files:?}");
        // from a profile that does not exist: refused, nothing made
        let missing = create(&main, &Source::Profile(name("Nobody")), &name("New2"), &never);
        assert!(matches!(missing, Err(CopyError::SourceMissing)));
        assert!(!main.join("profiles").join("new2").exists());
    }

    #[test]
    fn a_profile_from_the_defaults_is_an_empty_folder() {
        let main = used_main("defaults");
        let created = create(&main, &Source::Defaults, &name("Fresh"), &never).expect("made");
        assert_eq!(
            created,
            Created {
                preferences: Preferences::None,
                files: 0
            }
        );
        let new = main.join("profiles").join("fresh");
        assert!(new.is_dir() && listing(&new).is_empty());
        // and from a main profile that never ran
        let empty = main_dir("never-ran").join("not-made-yet");
        let created = create(&empty, &Source::Main, &name("First"), &never).expect("made");
        assert_eq!(created.preferences, Preferences::None);
        assert!(empty.join("profiles").join("first").is_dir());
    }

    #[test]
    fn an_existing_profile_is_never_written_into() {
        let main = used_main("exists");
        let before = listing(&main);
        for target in ["old", "OLD", "Old"] {
            let refused = create(&main, &Source::Main, &name(target), &never);
            assert!(matches!(refused, Err(CopyError::Exists)), "{target}");
        }
        // an empty folder of that name is a profile too
        fs::create_dir_all(main.join("profiles").join("empty")).expect("folder");
        assert!(matches!(
            create(&main, &Source::Defaults, &name("empty"), &never),
            Err(CopyError::Exists)
        ));
        // onto itself
        assert!(matches!(
            create(&main, &Source::Profile(name("old")), &name("old"), &never),
            Err(CopyError::Exists)
        ));
        assert_eq!(listing(&main), before, "nothing was touched");
        assert_eq!(settings_of(&main, "old")["font_scale"], json!(2.0));
    }

    #[test]
    fn a_running_profile_is_refused_before_anything_is_looked_at() {
        let main = used_main("running");
        let before = listing(&main);
        let runs = |profile: &ProfileName| profile.key() == "busy" || profile.key() == "old";
        // whether its folder exists or not
        for target in ["Busy", "old"] {
            let refused = create(&main, &Source::Main, &name(target), &runs);
            assert!(matches!(refused, Err(CopyError::Running)), "{target}");
        }
        assert_eq!(listing(&main), before);
        assert!(!main.join("profiles").join("busy").exists());
        // copying from a profile that runs is fine: it is only read
        let created = create(&main, &Source::Profile(name("old")), &name("Twin"), &runs).expect("made");
        assert_eq!(created.preferences, Preferences::Copied);
    }

    #[test]
    fn settings_that_do_not_read_are_not_copied_as_they_are() {
        let main = main_dir("unreadable");
        put(&main, "settings.json", "{\"username\": \"someone.resident\", half");
        put(&main, "ui_layout.ron", "(layout)");
        let created = create(&main, &Source::Main, &name("Alt"), &never).expect("made");
        assert_eq!(
            created,
            Created {
                preferences: Preferences::Unreadable,
                files: 1
            }
        );
        let new = main.join("profiles").join("alt");
        assert!(!new.join("settings.json").exists());
        assert!(new.join("ui_layout.ron").exists());
        // a file that is JSON and not settings
        let main = main_dir("not-settings");
        put(&main, "settings.json", "[\"someone.resident\"]");
        let created = create(&main, &Source::Main, &name("Alt"), &never).expect("made");
        assert_eq!(created.preferences, Preferences::Unreadable);
        assert!(listing(&main.join("profiles").join("alt")).is_empty());
    }

    #[test]
    fn a_copy_that_fails_leaves_no_profile_and_no_staging_folder() {
        let main = main_dir("fails");
        // a skin deeper than any skin is copied as far as the limit, and a
        // file where a skin's folder name should be read is an error
        put(&main, "settings.json", "{\"font_scale\": 1.5}");
        let mut deep = String::from("skins");
        for level in 0..copy::MAX_DEPTH + 3 {
            deep.push_str(&format!("/d{level}"));
            put(&main, &format!("{deep}/f.txt"), "x");
        }
        let created = create(&main, &Source::Main, &name("Deep"), &never).expect("made");
        assert_eq!(created.files, copy::MAX_DEPTH - 1, "the deeper files are left out");
        // the profiles folder cannot be made: an error, and nothing left
        let blocked = main_dir("blocked");
        put(&blocked, "settings.json", "{}");
        put(&blocked, "profiles", "a file where the folder should be");
        let failed = create(&blocked, &Source::Main, &name("Alt"), &never);
        assert!(matches!(failed, Err(CopyError::Io(_))), "{failed:?}");
        assert!(blocked.join("profiles").is_file());
        assert!(existing(&blocked).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn links_are_never_followed() {
        use std::os::unix::fs::symlink;
        let main = used_main("links");
        let secret = main_dir("links-secret");
        put(&secret, "password.txt", "a secret");
        symlink(secret.join("password.txt"), main.join("skins").join("aurora").join("linked.txt")).expect("link");
        symlink(&secret, main.join("skins").join("linked")).expect("link");
        fs::remove_file(main.join("ui_layout.ron")).expect("removed");
        symlink(secret.join("password.txt"), main.join("ui_layout.ron")).expect("link");
        let created = create(&main, &Source::Main, &name("Alt"), &never).expect("made");
        assert_eq!(created.files, 3, "the skin's own files only");
        let files = listing(&main.join("profiles").join("alt"));
        assert!(
            !files
                .iter()
                .any(|f| f.contains("linked") || f.contains("password") || f == "ui_layout.ron")
        );
        // a link where the new profile would be is "exists"
        symlink(&secret, main.join("profiles").join("ghost")).expect("link");
        assert!(matches!(
            create(&main, &Source::Main, &name("ghost"), &never),
            Err(CopyError::Exists)
        ));
        assert_eq!(listing(&secret).len(), 1, "nothing was written through the link");
    }

    #[test]
    fn only_folders_named_as_profiles_are_listed() {
        let main = main_dir("listing");
        for folder in ["alt1", "b-2", ".new-x-123", "Upper", "main", "has space"] {
            fs::create_dir_all(main.join("profiles").join(folder)).expect("folder");
        }
        put(&main, "profiles/file", "not a folder");
        let names: Vec<String> = existing(&main).iter().map(|n| n.key().to_owned()).collect();
        assert_eq!(names, ["alt1", "b-2"]);
        assert!(existing(&main.join("nowhere")).is_empty());
    }

    /// The names the copy strips by name are the names Aurora's settings
    /// file uses: an upstream rename shows here, not in a copied login.
    #[test]
    fn the_settings_left_out_are_still_called_so() {
        let saved = serde_json::to_value(crate::settings::Settings::default()).expect("settings");
        let saved = saved.as_object().expect("an object");
        let (top, nested) = copy::left_out();
        for name in top {
            // (read for migration only, never written)
            if *name == "mfa_hash" {
                continue;
            }
            assert!(saved.contains_key(*name), "settings.json no longer has `{name}`");
        }
        for (section, names) in nested {
            let inner = saved.get(*section).and_then(Value::as_object);
            let inner = inner.unwrap_or_else(|| panic!("settings.json no longer has `{section}`"));
            for name in *names {
                assert!(inner.contains_key(*name), "settings.json no longer has `{section}.{name}`");
            }
        }
    }
}
