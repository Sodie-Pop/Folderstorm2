//! The sections of a backup, one per feature, in the order the import
//! dialog lists them. A feature adds its own here: one type implementing
//! [`Section`] and one line of [`SECTIONS`]
//! (folderstorm/plans/account-backup.md, "Adding a section").
//!
//! Left out on purpose: the local assistant (its permissions open a local
//! API: a shared file must not turn them on), the chat image upload (its
//! host and upload folders decide where pictures go), the session host's
//! other settings (windows and places of this PC, the controller access).

use super::section::Section;
use serde_json::Value;

pub mod desktop;
pub mod gestures;
pub mod host;
pub mod inventory;
pub mod layouts;
pub mod plain;

/// Every section this version writes and reads.
pub static SECTIONS: &[&dyn Section] = &[
    &layouts::Layouts,
    &layouts::DefaultBackground,
    &desktop::Desktop,
    &inventory::Tabs,
    &inventory::Windows,
    &gestures::Boards,
    &plain::Viewport,
    &plain::Media,
    &host::Characters,
];

/// The section of this id.
pub fn find(id: &str) -> Option<&'static dyn Section> {
    SECTIONS.iter().copied().find(|section| section.id() == id)
}

/// The entries of a JSON array (none for anything else).
pub(super) fn entries(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// Adds `incoming`'s named entries to `current`: one whose `name` is the
/// same as an entry of `current` (`same`) takes its place, the others go
/// at the end. How many took a place.
pub(super) fn merge_named(current: &mut Vec<Value>, incoming: Vec<Value>, same: fn(&str, &str) -> bool) -> usize {
    let mut replaced = 0;
    for entry in incoming {
        let name = entry.get("name").and_then(Value::as_str).map(str::to_owned);
        let at = name.as_deref().and_then(|name| {
            current
                .iter()
                .position(|other| other.get("name").and_then(Value::as_str).is_some_and(|o| same(o, name)))
        });
        match at {
            Some(at) => {
                current[at] = entry;
                replaced += 1;
            }
            None => current.push(entry),
        }
    }
    replaced
}

/// Whether two names are the same, ignoring case and the spaces around.
pub(super) fn same_text(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}
