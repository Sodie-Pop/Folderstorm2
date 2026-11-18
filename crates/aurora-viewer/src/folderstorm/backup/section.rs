//! What a section of a backup is: one feature's part of the setup, written
//! as one JSON value and read back by Merge or Replace. Each feature
//! implements [`Section`] once; `super::sections::SECTIONS` lists them.
//!
//! A section never fails a whole import: what it cannot read it leaves out
//! and says so ([`Outcome::notes`]), and an error ([`Section::import`]'s
//! `Err`) leaves that feature as it was.

use crate::settings::Settings;
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;

/// How a section is put back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The file's entries are added; one with the same name as an entry
    /// here takes its place, the others here stay.
    #[default]
    Merge,
    /// The feature becomes what the file holds.
    Replace,
}

/// What a section reads its part from.
pub struct Env<'a> {
    pub settings: &'a Settings,
    /// The account logged in; None before login.
    pub account: Option<Uuid>,
    /// The main profile's settings folder (the session host's file).
    pub main_config: &'a Path,
}

/// What a section writes into a backup.
#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    pub value: Value,
    /// Files of the viewer's own folders the value names, as
    /// `<folder>/<name>` (`super::file::FOLDERS`): copied into the backup
    /// and put back with the section.
    pub files: Vec<String>,
}

impl Exported {
    pub fn value(value: Value) -> Self {
        Exported { value, files: Vec::new() }
    }
}

/// What a section is put back into.
pub struct Target<'a> {
    pub settings: &'a mut Settings,
    pub main_config: &'a Path,
}

/// How an import goes, for every section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct How {
    pub mode: Mode,
    /// The account the backup was made by, if it says.
    pub source: Option<Uuid>,
    /// The account logged in now, if any.
    pub target: Option<Uuid>,
    /// Leave out what points to another account's Inventory (folder and
    /// item ids mean something for one account only).
    pub skip_inventory: bool,
}

impl How {
    /// Whether an entry tied to `account`'s Inventory is left out.
    pub fn drops(&self, account: Uuid) -> bool {
        self.skip_inventory && Some(account) != self.target
    }

    /// Whether an Inventory id the file does not tie to an account (it is
    /// the backup's account's) is left out.
    pub fn drops_untagged(&self) -> bool {
        self.skip_inventory && (self.source.is_none() || self.source != self.target)
    }
}

/// What a section did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// What was left out or changed on the way, said to the user.
    pub notes: Vec<String>,
}

impl Outcome {
    pub fn note(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }
}

/// One feature's part of a backup.
///
/// The recipe for a new one is in folderstorm/plans/account-backup.md,
/// "Adding a section".
pub trait Section: Sync {
    /// Its name in the file: lower case letters, digits and `_`, never
    /// changed once released (a file of an older version must still find
    /// it).
    fn id(&self) -> &'static str;
    /// Its name in the import dialog.
    fn label(&self) -> String;
    /// Whether Merge means something here; otherwise only Replace is
    /// offered (plain settings).
    fn merges(&self) -> bool {
        true
    }
    /// The section as it is now; None when there is nothing to write (an
    /// account's part before login).
    fn export(&self, env: &Env) -> Option<Exported>;
    /// Puts `value` back. `value` comes from a file: it is read with the
    /// feature's own tolerant reader, never trusted.
    fn import(&self, target: &mut Target, value: Value, how: &How) -> Result<Outcome, String>;
    /// How many entries of `value` point to an account's Inventory, for the
    /// warning of an import into another account.
    fn inventory_refs(&self, _value: &Value) -> usize {
        0
    }
}
