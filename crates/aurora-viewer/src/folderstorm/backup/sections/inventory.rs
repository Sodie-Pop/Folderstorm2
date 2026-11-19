//! The Inventory's custom tabs (`settings.inventory.custom_tabs`, rows 2.1,
//! 2.2 and 2.13) and each Inventory window's pin and quiet mode
//! (`settings.inventory.window_modes`, row 2.13).
//!
//! A tab's folders and pinned items are kept by account id: another account
//! does not see them, as here. A folder window's mode is kept by folder id
//! alone; an import into another account may leave those out.

use super::super::section::{Env, Exported, How, Mode, Outcome, Section, Target};
use super::same_text;
use crate::ui::inventory::custom_tabs::{self, CustomTab, MAX_TABS};
use crate::ui::inventory::window_mode::{self, MAX_FOLDER_WINDOWS, Modes};
use aurora_i18n::tr;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

pub struct Tabs;

impl Section for Tabs {
    fn id(&self) -> &'static str {
        "inventory_tabs"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-inventory-tabs").to_owned()
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        Some(Exported::value(serde_json::to_value(&env.settings.inventory.custom_tabs).ok()?))
    }

    fn import(&self, target: &mut Target, value: Value, how: &How) -> Result<Outcome, String> {
        let Value::Array(items) = value else {
            return Err(tr!("fs-backup-unreadable").to_owned());
        };
        let mut out = Outcome::default();
        let (mut unread, mut dropped) = (0, 0);
        let mut incoming: Vec<CustomTab> = Vec::new();
        for item in items {
            match serde_json::from_value::<CustomTab>(item) {
                Ok(mut tab) => {
                    dropped += drop_accounts(&mut tab.folders, how) + drop_accounts(&mut tab.items, how);
                    incoming.push(tab);
                }
                Err(_) => unread += 1,
            }
        }
        if dropped > 0 {
            out.note(tr!("fs-backup-note-tabs-inventory", count = dropped));
        }
        if unread > 0 {
            out.note(tr!("fs-backup-note-unread", count = unread));
        }
        let tabs = &mut target.settings.inventory.custom_tabs;
        let mut left = 0;
        match how.mode {
            Mode::Replace => {
                left = incoming.len().saturating_sub(MAX_TABS);
                incoming.truncate(MAX_TABS);
                *tabs = incoming;
            }
            Mode::Merge => {
                for mut tab in incoming {
                    if let Some(mine) = tabs.iter_mut().find(|t| same_text(&t.name, &tab.name)) {
                        // the tab keeps its id (its window), and what it
                        // holds for the accounts the file does not name
                        tab.id = mine.id;
                        for (account, ids) in std::mem::take(&mut mine.folders) {
                            tab.folders.entry(account).or_insert(ids);
                        }
                        for (account, ids) in std::mem::take(&mut mine.items) {
                            tab.items.entry(account).or_insert(ids);
                        }
                        *mine = tab;
                    } else if tabs.len() < MAX_TABS {
                        if tabs.iter().any(|t| t.id == tab.id) {
                            // a new id below (`normalize`)
                            tab.id = 0;
                        }
                        tabs.push(tab);
                    } else {
                        left += 1;
                    }
                }
            }
        }
        custom_tabs::normalize(tabs);
        if left > 0 {
            out.note(tr!("fs-backup-note-limit", count = left));
        }
        Ok(out)
    }

    fn inventory_refs(&self, value: &Value) -> usize {
        let count = |tab: &Value, key: &str| {
            tab.get(key)
                .and_then(Value::as_object)
                .map_or(0, |by_account| by_account.values().filter_map(Value::as_array).map(Vec::len).sum())
        };
        super::entries(Some(value))
            .iter()
            .map(|tab| count(tab, "folders") + count(tab, "items"))
            .sum()
    }
}

/// Takes out the ids of the accounts the import leaves out; how many.
fn drop_accounts(by_account: &mut BTreeMap<Uuid, Vec<Uuid>>, how: &How) -> usize {
    let mut dropped = 0;
    by_account.retain(|account, ids| {
        let keep = !how.drops(*account);
        if !keep {
            dropped += ids.len();
        }
        keep
    });
    dropped
}

pub struct Windows;

/// Whether a key of the window modes is one this version writes.
fn known_key(key: &str) -> bool {
    match key.split_once(':') {
        None => matches!(key, "main" | "additional"),
        Some(("additional", slot)) => slot.parse::<u8>().is_ok(),
        Some(("tab", id)) => id.parse::<u64>().is_ok(),
        Some(("folder", id)) => Uuid::parse_str(id).is_ok(),
        _ => false,
    }
}

impl Section for Windows {
    fn id(&self) -> &'static str {
        "inventory_windows"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-inventory-windows").to_owned()
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        Some(Exported::value(serde_json::to_value(&env.settings.inventory.window_modes).ok()?))
    }

    fn import(&self, target: &mut Target, value: Value, how: &How) -> Result<Outcome, String> {
        let Value::Object(incoming) = value else {
            return Err(tr!("fs-backup-unreadable").to_owned());
        };
        let mut out = Outcome::default();
        let modes = &mut target.settings.inventory.window_modes;
        let mut map = match how.mode {
            Mode::Replace => Map::new(),
            Mode::Merge => match serde_json::to_value(&*modes) {
                Ok(Value::Object(map)) => map,
                _ => Map::new(),
            },
        };
        let mut dropped = 0;
        for (key, mode) in incoming {
            if !known_key(&key) || serde_json::from_value::<window_mode::Mode>(mode.clone()).is_err() {
                continue;
            }
            if key.starts_with("folder:") && how.drops_untagged() {
                dropped += 1;
                continue;
            }
            map.insert(key, mode);
        }
        // at most so many folder windows, as `Modes::set` keeps
        let folders: Vec<String> = map.keys().filter(|k| k.starts_with("folder:")).cloned().collect();
        for key in folders.iter().take(folders.len().saturating_sub(MAX_FOLDER_WINDOWS)) {
            map.remove(key);
        }
        if dropped > 0 {
            out.note(tr!("fs-backup-note-windows-inventory", count = dropped));
        }
        *modes = serde_json::from_value::<Modes>(Value::Object(map)).map_err(|_| tr!("fs-backup-unreadable").to_owned())?;
        Ok(out)
    }

    fn inventory_refs(&self, value: &Value) -> usize {
        value
            .as_object()
            .map_or(0, |map| map.keys().filter(|k| k.starts_with("folder:")).count())
    }
}
