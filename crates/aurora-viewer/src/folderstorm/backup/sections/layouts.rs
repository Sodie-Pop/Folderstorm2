//! Layouts and workspaces (`settings.folderstorm_workspaces`, rows 1.3 to
//! 1.7) and the default background around the world (row 1.8), with the
//! pictures they use.
//!
//! Both are read back through the layouts' own tolerant reader (`Store`'s
//! `From<Raw>`): its limits, its names and its checks apply to a file as to
//! `settings.json`. A workspace's Inventory folders and HUDs keep the
//! account they were saved by: another account skips them, as here.

use super::super::section::{Env, Exported, How, Mode, Outcome, Section, Target};
use super::{entries, merge_named};
use crate::folderstorm::backgrounds::Background;
use crate::folderstorm::workspaces::Store;
use crate::folderstorm::workspaces::store::{Record, same_name};
use aurora_i18n::tr;
use serde_json::Value;
use uuid::Uuid;

/// What says where the window is now, never a part of a backup.
const THIS_SESSION: &[&str] = &["modified", "pending", "window_held"];

pub struct Layouts;

impl Section for Layouts {
    fn id(&self) -> &'static str {
        "layouts"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-layouts").to_owned()
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        let store = &env.settings.folderstorm_workspaces;
        let mut value = serde_json::to_value(store).ok()?;
        if let Value::Object(map) = &mut value {
            map.remove("background");
            for key in THIS_SESSION {
                map.remove(*key);
            }
        }
        let files = store
            .layouts
            .iter()
            .filter_map(Record::layout)
            .filter_map(|layout| picture_file(layout.background.as_ref()?))
            .collect();
        Some(Exported { value, files })
    }

    fn import(&self, target: &mut Target, value: Value, how: &How) -> Result<Outcome, String> {
        let Value::Object(mut incoming) = value else {
            return Err(tr!("fs-backup-unreadable").to_owned());
        };
        let mut out = Outcome::default();
        let mut workspaces = entries(incoming.get("workspaces"));
        let dropped = drop_inventory(&mut workspaces, how);
        if dropped > 0 {
            out.note(tr!("fs-backup-note-workspace-inventory", count = dropped));
        }
        incoming.insert("workspaces".into(), Value::Array(workspaces));
        let store = &mut target.settings.folderstorm_workspaces;
        let mut current = serde_json::to_value(&*store).map_err(|e| e.to_string())?;
        let result = match how.mode {
            Mode::Replace => {
                let mut result = incoming;
                for key in THIS_SESSION {
                    result.remove(*key);
                }
                if let Some(background) = current.get("background") {
                    result.insert("background".into(), background.clone());
                }
                Value::Object(result)
            }
            Mode::Merge => {
                let Value::Object(map) = &mut current else {
                    return Err(tr!("fs-backup-unreadable").to_owned());
                };
                for list in ["layouts", "workspaces"] {
                    let mut mine = entries(map.get(list));
                    merge_named(&mut mine, entries(incoming.get(list)), same_name);
                    map.insert(list.into(), Value::Array(mine));
                }
                let mut favorites = entries(map.get("favorites"));
                for favorite in entries(incoming.get("favorites")) {
                    if !favorites.contains(&favorite) {
                        favorites.push(favorite);
                    }
                }
                map.insert("favorites".into(), Value::Array(favorites));
                current
            }
        };
        let wanted = |value: &Value, list: &str| entries(value.get(list)).len();
        let (layouts, spaces) = (wanted(&result, "layouts"), wanted(&result, "workspaces"));
        let read: Store = serde_json::from_value(result).map_err(|_| tr!("fs-backup-unreadable").to_owned())?;
        let left = layouts.saturating_sub(read.layouts.len()) + spaces.saturating_sub(read.workspaces.len());
        if left > 0 {
            out.note(tr!("fs-backup-note-limit", count = left));
        }
        let unusable = read.layouts.iter().filter(|r| r.layout().is_none()).count()
            + read.workspaces.iter().filter(|r| r.workspace().is_none()).count();
        if unusable > 0 {
            out.note(tr!("fs-backup-note-kept", count = unusable));
        }
        *store = read;
        Ok(out)
    }

    fn inventory_refs(&self, value: &Value) -> usize {
        entries(value.get("workspaces"))
            .iter()
            .map(|entry| {
                let count = |part: &str, list: &str| {
                    entry
                        .get(part)
                        .and_then(|p| p.get(list))
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len)
                };
                count("folders", "windows") + count("huds", "items")
            })
            .sum()
    }
}

/// Takes the folders and HUDs of another account's Inventory out of the
/// workspaces, when the import leaves them out; how many entries went.
fn drop_inventory(workspaces: &mut [Value], how: &How) -> usize {
    let mut dropped = 0;
    for entry in workspaces.iter_mut() {
        let Value::Object(map) = entry else { continue };
        for (part, list) in [("folders", "windows"), ("huds", "items")] {
            let account = map
                .get(part)
                .and_then(|p| p.get("account"))
                .and_then(Value::as_str)
                .and_then(|text| Uuid::parse_str(text).ok());
            if let Some(account) = account
                && how.drops(account)
            {
                dropped += map
                    .get(part)
                    .and_then(|p| p.get(list))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                map.remove(part);
            }
        }
    }
    dropped
}

/// The picture of a background, as a file of the backup.
fn picture_file(background: &Background) -> Option<String> {
    background.picture.as_ref().map(|picture| format!("backgrounds/{}", picture.file))
}

/// The default background around the world (row 1.8): a colour, or a
/// picture and how it fills the margins.
pub struct DefaultBackground;

impl Section for DefaultBackground {
    fn id(&self) -> &'static str {
        "background"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-background").to_owned()
    }

    fn merges(&self) -> bool {
        false
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        let background = &env.settings.folderstorm_workspaces.background;
        Some(Exported {
            value: serde_json::to_value(background).ok()?,
            files: picture_file(background).into_iter().collect(),
        })
    }

    fn import(&self, target: &mut Target, value: Value, _: &How) -> Result<Outcome, String> {
        if !value.is_object() {
            return Err(tr!("fs-backup-unreadable").to_owned());
        }
        target.settings.folderstorm_workspaces.background = Background::read(value);
        Ok(Outcome::default())
    }
}
