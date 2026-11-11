//! "New character profile…" (session-host plan, section 13, checkpoint 4):
//! what a new profile takes from the profile it is made from, and what it
//! never takes. Pure: no file is read here. The host walks the source's
//! settings folder and asks [`part`] about each name, then passes
//! `settings.json` through [`strip_settings`].
//!
//! - **Only what is named here is copied** (an allow-list): the
//!   preferences (`settings.json`, which holds the key bindings), the
//!   windows' layout (`ui_layout.ron`) and the skins (`skins/`). Anything
//!   else in the folder is left where it is, whatever it is called today
//!   or tomorrow: the accounts' files and conversation transcripts
//!   (`accounts/`), the saved accounts of the login screen
//!   (`login_accounts.json`), the cache key's fallback file, the other
//!   profiles (`profiles/`), the host's own file, locks, photos.
//!   Passwords are not in the folder at all (the system's credential
//!   store keeps them, by grid and account), and nothing here reads it.
//! - **The preferences lose what belongs to an account, a window or this
//!   profile**: the remembered user name and last login, the window's
//!   place and size, every setting kept per account (an object keyed by
//!   account ids, a list of them), the cache folder, the profile's own
//!   choices for several characters, the local assistant's permission and
//!   the user's own service keys. The new character starts with the
//!   interface of the profile it was made from, and no account.
//!
//! The creators' permissions are not concerned: these are the user's own
//! settings files, never an asset or an asset id.

use crate::profile::ProfileName;
use serde_json::Value;

/// What the new profile is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The main profile (a launch without `--profile`).
    Main,
    /// Another named profile.
    Profile(ProfileName),
    /// Nothing: the viewer's defaults.
    Defaults,
}

/// The preferences file: copied through [`strip_settings`].
pub const SETTINGS_FILE: &str = "settings.json";
/// Files of the settings folder copied as they are.
const FILES: &[&str] = &["ui_layout.ron"];
/// Folders of the settings folder copied whole (plain files only).
const FOLDERS: &[&str] = &["skins"];

/// How deep a copied folder is followed, how many files a copy takes, and
/// the largest of them: a skin is a few small files, anything beyond is
/// not one.
pub const MAX_DEPTH: usize = 6;
pub const MAX_FILES: usize = 2000;
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// How an entry of the source's settings folder is copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The preferences: read, stripped, written.
    Settings,
    /// A file copied as it is.
    File,
    /// A folder copied whole.
    Folder,
}

/// What becomes of the entry `name` at the top of the source's settings
/// folder (`folder`: it is one). None: it is never copied. Names are
/// compared without case, as Windows folders do.
pub fn part(name: &str, folder: bool) -> Option<Part> {
    let is = |wanted: &&str| wanted.eq_ignore_ascii_case(name);
    if folder {
        return FOLDERS.iter().any(is).then_some(Part::Folder);
    }
    if is(&SETTINGS_FILE) {
        Some(Part::Settings)
    } else if FILES.iter().any(is) {
        Some(Part::File)
    } else {
        None
    }
}

/// Whether a name found inside a copied folder may be copied: a plain
/// name, never a way out of the folder.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// Preferences that are not copied: the login, the window, this profile's
/// own folders and choices.
const LEFT_OUT: &[&str] = &[
    // the login: no account comes with the copy
    "username",
    "remember_username",
    "last_login",
    "remember_password",
    "mfa_hash",
    // the window: each character has its own place (the host's stage)
    "window_width",
    "window_height",
    "window_x",
    "window_y",
    "window_maximized",
    "window_fullscreen",
    // the profile's own cache folder (never the source's)
    "cache_location",
    // the profile's own choices for several characters: a named profile's
    // defaults differ from the main profile's
    "folderstorm_characters",
    // access by a local assistant is granted per profile, by the user
    "local_assistant",
];

/// The same inside a section: the user's own service keys and this
/// installation's random id for them.
const LEFT_OUT_NESTED: &[(&str, &[&str])] = &[("gifs", &["giphy_key", "klipy_key", "customer_id"])];

/// The names [`strip_settings`] removes by name, for the viewer's check
/// that its settings still call them so: top-level names, then
/// `(section, name)` pairs.
pub fn left_out() -> (&'static [&'static str], &'static [(&'static str, &'static [&'static str])]) {
    (LEFT_OUT, LEFT_OUT_NESTED)
}

/// Whether a text has the shape of an account id (a UUID with dashes).
fn id_shaped(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Whether a top-level value is kept per account: an account id, a list of
/// them, or an object keyed by them.
fn per_account(value: &Value) -> bool {
    match value {
        Value::String(text) => id_shaped(text),
        Value::Array(items) => !items.is_empty() && items.iter().all(|i| i.as_str().is_some_and(id_shaped)),
        Value::Object(map) => !map.is_empty() && map.keys().all(|k| id_shaped(k)),
        _ => false,
    }
}

/// The preferences a new profile starts with, from its source's
/// `settings.json`: without the login, the window, the profile's own
/// choices and everything kept per account. False when `settings` is not
/// a settings file (not an object): nothing of it is to be written.
pub fn strip_settings(settings: &mut Value) -> bool {
    let Value::Object(map) = settings else {
        return false;
    };
    for name in LEFT_OUT {
        map.remove(*name);
    }
    for (section, names) in LEFT_OUT_NESTED {
        if let Some(Value::Object(inner)) = map.get_mut(*section) {
            for name in *names {
                inner.remove(*name);
            }
        }
    }
    map.retain(|_, value| !per_account(value));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_preferences_the_layout_and_the_skins_are_copied() {
        assert_eq!(part("settings.json", false), Some(Part::Settings));
        assert_eq!(part("ui_layout.ron", false), Some(Part::File));
        assert_eq!(part("skins", true), Some(Part::Folder));
        // Windows folders ignore case
        assert_eq!(part("Settings.JSON", false), Some(Part::Settings));
        assert_eq!(part("Skins", true), Some(Part::Folder));
        // a folder called like a file, a file called like a folder
        assert_eq!(part("settings.json", true), None);
        assert_eq!(part("ui_layout.ron", true), None);
        assert_eq!(part("skins", false), None);
    }

    #[test]
    fn accounts_transcripts_credentials_caches_and_locks_are_never_copied() {
        for (name, folder) in [
            // accounts: per-account files and conversation transcripts
            ("accounts", true),
            ("login_accounts.json", false),
            ("rlva-login.json", false),
            ("login_favorites.json", false),
            // keys and credentials
            ("cache.key", false),
            ("credentials.json", false),
            ("keyring", true),
            // caches and what belongs to this PC
            ("cache", true),
            ("cache_location.txt", false),
            ("graphics-before-safe.json", false),
            ("blender-installations.json", false),
            // the other profiles, the host, locks
            ("profiles", true),
            ("session-host.json", false),
            ("session-host.lock", false),
            ("profile.lock", false),
            // the rest of the folder
            ("photos", true),
            ("photo-luts", true),
            ("dictionaries", true),
            ("autoreplace.xml", false),
            ("layout-autosave-0f8fad5b-d9cb-469f-a165-70867728950e", true),
            ("settings.json.tmp", false),
            ("settings.json.bak", false),
            ("anything-new.json", false),
            ("", false),
            ("..", true),
        ] {
            assert_eq!(part(name, folder), None, "{name}");
        }
    }

    #[test]
    fn names_inside_a_copied_folder_never_lead_out_of_it() {
        for good in ["theme.json", "aurora", "icons", "a b.png", ".hidden"] {
            assert!(plain_name(good), "{good}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", "C:", "x\0y", "../accounts"] {
            assert!(!plain_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_copy_keeps_the_interface_and_drops_the_login_and_the_window() {
        let mut settings = json!({
            "username": "someone.resident",
            "remember_username": true,
            "last_login": ["https://login.example/cgi-bin/login.cgi", "someone.resident"],
            "remember_password": true,
            "mfa_hash": "abc",
            "window_width": 1600,
            "window_height": 900,
            "window_x": 40,
            "window_y": null,
            "window_maximized": true,
            "window_fullscreen": false,
            "cache_location": "D:\\cache",
            "folderstorm_characters": {"behind_3d": "Normal", "share_main_cache": true},
            "local_assistant": {"enabled": true},
            "gifs": {"giphy_key": "k1", "klipy_key": "k2", "customer_id": "c", "provider": "Giphy"},
            // the interface: kept
            "font_scale": 1.25,
            "language": "fr",
            "keybinds": {"map": [["Jump", ["E", null]]]},
            "grid": "SecondLife",
            "grids": [{"name": "A grid", "uri": "https://grid.example/"}],
            "chat_log_location": "D:\\logs",
            "audio": {"master": 0.5, "output_device": "Speakers"},
            "draw_distance": 128.0,
        });
        assert!(strip_settings(&mut settings));
        assert_eq!(
            settings,
            json!({
                "gifs": {"provider": "Giphy"},
                "font_scale": 1.25,
                "language": "fr",
                "keybinds": {"map": [["Jump", ["E", null]]]},
                "grid": "SecondLife",
                "grids": [{"name": "A grid", "uri": "https://grid.example/"}],
                "chat_log_location": "D:\\logs",
                "audio": {"master": 0.5, "output_device": "Speakers"},
                "draw_distance": 128.0,
            })
        );
    }

    #[test]
    fn everything_kept_per_account_stays_behind() {
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let other = "7C9E6679-7425-40DE-944B-E07FC1F90AE7";
        let mut settings = json!({
            "hover_height_account": id,
            "hover_height_accounts": {id: {"offset": 0.1}},
            "bridge": {id: {"enabled": true}, other: {"enabled": false}},
            "parcel_music_accounts": [id, other],
            "a_future_per_account_setting": {other: 3},
            // not per account: an empty map or list, a map of other names,
            // a list of other things, an id deeper inside a section
            "movements": {},
            "empty_list": [],
            "colours": {"accent": "#7c4dff"},
            "mixed": {id: 1, "default": 2},
            "names": ["a", id],
            "snapshot": {"last_folder": id},
            "text": "0f8fad5b-d9cb-469f-a165-70867728950",
        });
        assert!(strip_settings(&mut settings));
        let mut kept: Vec<&str> = settings.as_object().expect("object").keys().map(String::as_str).collect();
        kept.sort_unstable();
        assert_eq!(kept, ["colours", "empty_list", "mixed", "movements", "names", "snapshot", "text"]);
    }

    #[test]
    fn what_is_not_a_settings_file_is_not_written() {
        for mut not_settings in [json!([1, 2]), json!("text"), json!(null), json!(3)] {
            assert!(!strip_settings(&mut not_settings));
        }
        assert!(strip_settings(&mut json!({})));
    }

    #[test]
    fn the_names_left_out_are_listed_for_the_viewer_s_check() {
        let (top, nested) = left_out();
        assert!(top.contains(&"username") && top.contains(&"window_x"));
        assert!(
            nested
                .iter()
                .any(|(section, names)| *section == "gifs" && names.contains(&"giphy_key"))
        );
    }
}
