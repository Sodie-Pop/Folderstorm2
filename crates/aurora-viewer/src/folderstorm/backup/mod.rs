//! Folderstorm: back up an account's Folderstorm setup to one file and
//! import it later, on this PC or another (FEATURES.md row 0.6,
//! folderstorm/plans/account-backup.md).
//!
//! Aurora already backs up its whole settings (`settings/backup.rs`,
//! Preferences › Language and updates › Backup). This backup is narrower and
//! portable: the Folderstorm features of one profile and of the account
//! logged in, section by section, each one put back by Merge or Replace.
//!
//! - [`section`]: what a section is (the [`Section`] trait) and what it is
//!   given; [`sections`] holds the list. A feature adds its own section with
//!   one type and one line of that list.
//! - [`file`]: the file, a ZIP holding a manifest, one JSON per section and
//!   the copied pictures the sections use.
//! - [`secrets`]: the guard that keeps passwords, tokens, logins and
//!   capability URLs out of a backup.
//! - [`folder`]: the backups folder, its automatic backups and their list.
//! - [`apply`]: an import: what to take from a file, the safety backup made
//!   first, then each section.
//! - [`jobs`]: the file dialogs and the disk work, off the frame, and the
//!   once-a-frame hook ([`frame`]).
//! - [`demo`]: `AURORA_DEMO_BACKUP`.

pub mod apply;
pub mod demo;
pub mod file;
pub mod folder;
pub mod jobs;
pub mod secrets;
pub mod section;
pub mod sections;
#[cfg(test)]
mod tests;

pub use jobs::frame;
pub use section::Mode;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The automatic backups kept, at least and at most.
pub const KEEP: std::ops::RangeInclusive<u8> = 1..=30;
/// The safety backups made before an import that are kept.
pub const KEEP_BEFORE_IMPORT: usize = 10;

/// What `settings.json` keeps (field `folderstorm_backup`): the daily
/// automatic backup, off until chosen, and how many of them are kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Once a day, while logged in, a backup of the account's setup in the
    /// backups folder.
    pub daily: bool,
    /// The daily backups kept for each account ([`KEEP`]).
    pub keep: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self { daily: false, keep: 5 }
    }
}

impl Settings {
    /// [`Settings::keep`] within [`KEEP`].
    pub fn kept(&self) -> usize {
        usize::from(self.keep.clamp(*KEEP.start(), *KEEP.end()))
    }
}

/// The account logged in, as the app tells the backup each frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: Uuid,
    /// Its name, shown when a file is read back ("a backup of …").
    pub name: String,
}

impl Account {
    pub fn of(login: &aurora_net::LoginResponse) -> Self {
        let name = format!("{} {}", login.first_name, login.last_name);
        Account {
            id: login.agent_id,
            name: name.trim().to_owned(),
        }
    }
}

/// The settings folder of this profile, and the main profile's (the
/// session host's file is there).
#[derive(Clone, Debug)]
pub struct Folders {
    pub config: std::path::PathBuf,
    pub main_config: std::path::PathBuf,
}

impl Folders {
    pub fn of_this_viewer() -> Self {
        Folders {
            config: crate::settings::config_dir(),
            main_config: crate::folderstorm_characters::profile::main_config_dir(),
        }
    }
}
