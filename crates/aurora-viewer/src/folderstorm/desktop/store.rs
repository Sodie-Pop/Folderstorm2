//! The margins' desktop as `settings.json` keeps it (field
//! `folderstorm_desktop`), its limits and the operations of the surface.
//! Pure: nothing here touches egui, the inventory or the network
//! (folderstorm/plans/margin-desktop.md, "Data model and storage").
//!
//! A shortcut keeps a command id or an inventory folder id, never an asset
//! id, a name read from the inventory or a window title: what it shows is
//! read when it is drawn. Only a name the user typed is saved.
//!
//! Reading never fails and never loses what it cannot use: a shortcut that
//! cannot be read (a newer version's target, a damaged file) is kept as it
//! was read ([`Desktop::kept`]) and written back unchanged; values out of
//! their range are brought back into it.

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// At most this many shortcuts for a profile.
pub const MAX_SHORTCUTS: usize = 256;
/// A shortcut's own name has at most this many characters.
pub const MAX_NAME: usize = 64;
/// At most this many slots (monitors under the viewer window) hold a
/// desktop; a slot read beyond is brought back to the last one.
pub const MAX_SLOTS: u8 = 16;
/// A cell's column or row is below this (a hand-edited file).
pub const MAX_CELL: u16 = 1024;

/// The size of the shortcuts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Small, Size::Medium, Size::Large];

    /// The side of a shortcut's icon, in points.
    pub fn icon(self) -> f32 {
        match self {
            Size::Small => 24.0,
            Size::Medium => 32.0,
            Size::Large => 48.0,
        }
    }

    /// A cell of the grid (width, height), in points.
    pub fn cell(self) -> [f32; 2] {
        match self {
            Size::Small => [76.0, 68.0],
            Size::Medium => [92.0, 84.0],
            Size::Large => [112.0, 104.0],
        }
    }
}

/// The taskbar's choices (checkpoint 4 of the plan draws it; the record is
/// here so that the file's shape is settled).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Taskbar {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub on: bool,
    /// The slot it stands in; None: the widest room.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
    /// Every open window instead of the collapsed ones only.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub all_windows: bool,
}

/// What a shortcut opens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    /// An Inventory folder of one account: its id there, never an asset
    /// id nor its name. Another account of the profile sees it greyed.
    Folder { folder: Uuid, account: Uuid },
    /// A command of the toolbar catalogue, by its id.
    Command { command: String },
}

impl Target {
    pub fn command(id: &str) -> Self {
        Target::Command { command: id.to_owned() }
    }
}

/// One shortcut: what it opens and where it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    /// Made by the desktop, kept through moves and renames.
    pub id: u32,
    pub target: Target,
    /// The n-th monitor under the viewer window, from 0.
    #[serde(default)]
    pub slot: u8,
    /// Its cell in that slot's grid.
    #[serde(default)]
    pub col: u16,
    #[serde(default)]
    pub row: u16,
    /// Shown instead of the target's own name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A Tabler icon shown instead of the target's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl Shortcut {
    pub fn cell(&self) -> (u16, u16) {
        (self.col, self.row)
    }

    fn normalized(mut self) -> Self {
        self.slot = self.slot.min(MAX_SLOTS - 1);
        self.col = self.col.min(MAX_CELL - 1);
        self.row = self.row.min(MAX_CELL - 1);
        self.name = self.name.map(|n| clean_name(&n)).filter(|n| !n.is_empty());
        self.icon = self.icon.map(|i| clean_name(&i)).filter(|i| !i.is_empty());
        self
    }
}

/// The desktop of a profile: its choices and its shortcuts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Desktop {
    /// *Shortcuts in the margins*.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shortcuts_on: bool,
    pub size: Size,
    /// A single click opens (checkpoint 3 of the plan gives it a switch).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub single_click: bool,
    pub taskbar: Taskbar,
    /// The id the next shortcut takes.
    pub next_id: u32,
    pub shortcuts: Vec<Shortcut>,
    /// Shortcuts this version could not read, written back as they were.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<serde_json::Value>,
}

/// Why an operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Already as many shortcuts as allowed.
    Full,
    /// That slot's desktop has a shortcut to the same thing already (its id).
    Existing(u32),
    /// No such shortcut.
    Missing,
}

/// A name as kept: trimmed, cut at [`MAX_NAME`] characters, without line
/// breaks or other control characters.
pub fn clean_name(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

impl<'de> Deserialize<'de> for Desktop {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_value(serde_json::Value::deserialize(deserializer)?))
    }
}

impl Desktop {
    /// Whatever `value` holds that this version reads; the rest of its
    /// shortcuts is kept unread.
    pub fn from_value(value: serde_json::Value) -> Self {
        let read = |key: &str| value.get(key).cloned();
        let flag = |key: &str| read(key).and_then(|v| v.as_bool()).unwrap_or(false);
        let mut out = Desktop {
            shortcuts_on: flag("shortcuts_on"),
            size: read("size").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            single_click: flag("single_click"),
            taskbar: read("taskbar").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            next_id: read("next_id")
                .and_then(|v| v.as_u64())
                .map_or(0, |n| n.min(u64::from(u32::MAX)) as u32),
            shortcuts: Vec::new(),
            kept: Vec::new(),
        };
        out.taskbar.slot = out.taskbar.slot.map(|slot| slot.min(MAX_SLOTS - 1));
        // what an older run kept is read again: this version may know it
        let entries = ["shortcuts", "kept"]
            .into_iter()
            .filter_map(read)
            .filter_map(|list| list.as_array().cloned())
            .flatten();
        for entry in entries {
            match serde_json::from_value::<Shortcut>(entry.clone()) {
                Ok(shortcut) if out.shortcuts.len() < MAX_SHORTCUTS && out.shortcut(shortcut.id).is_none() => {
                    out.shortcuts.push(shortcut.normalized());
                }
                _ => out.kept.push(entry),
            }
        }
        let above = out.shortcuts.iter().map(|s| s.id).max().map_or(0, |m| m.saturating_add(1));
        out.next_id = out.next_id.max(above);
        out
    }

    pub fn shortcut(&self, id: u32) -> Option<&Shortcut> {
        self.shortcuts.iter().find(|s| s.id == id)
    }

    fn shortcut_mut(&mut self, id: u32) -> Option<&mut Shortcut> {
        self.shortcuts.iter_mut().find(|s| s.id == id)
    }

    /// A new shortcut to `target` in `slot`, at `cell`. Refused when the
    /// slot has one to the same thing, or the desktop is full.
    pub fn add(&mut self, target: Target, slot: u8, (col, row): (u16, u16)) -> Result<u32, Error> {
        let slot = slot.min(MAX_SLOTS - 1);
        if let Some(same) = self.shortcuts.iter().find(|s| s.slot == slot && s.target == target) {
            return Err(Error::Existing(same.id));
        }
        if self.shortcuts.len() >= MAX_SHORTCUTS {
            return Err(Error::Full);
        }
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        self.shortcuts.push(
            Shortcut {
                id,
                target,
                slot,
                col,
                row,
                name: None,
                icon: None,
            }
            .normalized(),
        );
        Ok(id)
    }

    /// Puts the shortcut `id` in `cell` of `slot`. False when nothing
    /// changed: it stood there, it is gone, or that slot's desktop has a
    /// shortcut to the same thing already.
    pub fn place(&mut self, id: u32, slot: u8, (col, row): (u16, u16)) -> bool {
        let to = (slot.min(MAX_SLOTS - 1), col.min(MAX_CELL - 1), row.min(MAX_CELL - 1));
        let Some(target) = self.shortcut(id).map(|s| s.target.clone()) else {
            return false;
        };
        if self.shortcuts.iter().any(|s| s.id != id && s.slot == to.0 && s.target == target) {
            return false;
        }
        let Some(shortcut) = self.shortcut_mut(id) else {
            return false;
        };
        if (shortcut.slot, shortcut.col, shortcut.row) == to {
            return false;
        }
        (shortcut.slot, shortcut.col, shortcut.row) = to;
        true
    }

    /// Only the shortcut goes, never what it opens.
    pub fn remove(&mut self, id: u32) -> Result<Shortcut, Error> {
        let at = self.shortcuts.iter().position(|s| s.id == id).ok_or(Error::Missing)?;
        Ok(self.shortcuts.remove(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn written(desktop: &Desktop) -> serde_json::Value {
        serde_json::to_value(desktop).expect("written")
    }

    #[test]
    fn a_desktop_is_read_and_written_back_unchanged() {
        let mut d = Desktop {
            shortcuts_on: true,
            size: Size::Large,
            ..Default::default()
        };
        let people = d.add(Target::command("people"), 0, (0, 0)).expect("added");
        let folder = Target::Folder {
            folder: Uuid::from_u128(7),
            account: Uuid::from_u128(1),
        };
        d.add(folder, 2, (3, 1)).expect("added");
        d.shortcuts[0].name = Some("Friends and more".into());
        assert_eq!(d.shortcuts[0].id, people);
        d.taskbar = Taskbar {
            on: true,
            slot: Some(2),
            all_windows: true,
        };
        let value = written(&d);
        // the shape the plan gives
        assert_eq!(value["shortcuts"][0]["target"], json!({ "command": "people" }));
        assert_eq!(value["shortcuts"][0]["name"], json!("Friends and more"));
        assert_eq!(
            value["shortcuts"][1]["target"],
            json!({ "folder": Uuid::from_u128(7), "account": Uuid::from_u128(1) })
        );
        assert_eq!(value["size"], json!("large"));
        let read: Desktop = serde_json::from_value(value.clone()).expect("read");
        assert_eq!(read, d);
        assert_eq!(written(&read), value);
        // nothing chosen: off, medium, collapsed windows only
        let empty: Desktop = serde_json::from_value(json!({})).expect("read");
        assert_eq!(empty, Desktop::default());
        assert!(!empty.shortcuts_on && !empty.taskbar.on && !empty.taskbar.all_windows);
        assert_eq!(empty.size, Size::Medium);
        assert!(written(&empty).get("shortcuts_on").is_none());
    }

    #[test]
    fn what_cannot_be_read_is_kept_and_written_back() {
        let newer = json!({ "id": 9, "target": { "landmark": "abc" }, "slot": 0, "col": 0, "row": 0 });
        let damaged = json!("not a shortcut");
        let value = json!({
            "shortcuts_on": "yes",
            "size": "huge",
            "taskbar": [1, 2],
            "next_id": 2,
            "shortcuts": [
                { "id": 4, "target": { "command": "people" }, "slot": 3, "col": 7, "row": 2, "name": "  " },
                newer,
                damaged,
                // the same id twice: the second is kept unread
                { "id": 4, "target": { "command": "inventory" } },
                { "id": 5, "target": { "command": "inventory" } },
            ],
        });
        let read: Desktop = serde_json::from_value(value).expect("reading never fails");
        assert!(!read.shortcuts_on);
        assert_eq!(read.size, Size::Medium);
        assert_eq!(read.taskbar, Taskbar::default());
        assert_eq!(read.shortcuts.len(), 2);
        assert_eq!(read.kept.len(), 3);
        let four = read.shortcut(4).expect("read");
        assert_eq!((four.slot, four.col, four.row, four.name.clone()), (3, 7, 2, None));
        assert_eq!(four.target, Target::command("people"), "the first of the two with that id");
        let five = read.shortcut(5).expect("read");
        assert_eq!((five.slot, five.col, five.row), (0, 0, 0));
        // the next id is above every id read
        assert_eq!(read.next_id, 6);
        let again: Desktop = serde_json::from_value(written(&read)).expect("read");
        assert_eq!(again, read);
        assert!(again.kept.contains(&newer) && again.kept.contains(&damaged));
        // values out of their range
        let value = json!({ "shortcuts": [{ "id": 1, "target": { "command": "people" }, "slot": 200, "col": 60000, "row": 2,
            "name": "a\tb" }], "taskbar": { "slot": 99 } });
        let read: Desktop = serde_json::from_value(value).expect("read");
        let one = read.shortcut(1).expect("read");
        assert_eq!((one.slot, one.col, one.row), (MAX_SLOTS - 1, MAX_CELL - 1, 2));
        assert_eq!(one.name.as_deref(), Some("a b"));
        assert_eq!(read.taskbar.slot, Some(MAX_SLOTS - 1));
        // not an object at all
        let read: Desktop = serde_json::from_value(json!(12)).expect("read");
        assert_eq!(read, Desktop::default());
    }

    #[test]
    fn shortcuts_are_added_placed_and_removed_within_the_limits() {
        let mut d = Desktop::default();
        let people = d.add(Target::command("people"), 0, (0, 0)).expect("added");
        assert_eq!(people, 1);
        // the same thing twice: refused in one desktop, allowed on another slot
        assert_eq!(d.add(Target::command("people"), 0, (1, 0)), Err(Error::Existing(people)));
        let other = d.add(Target::command("people"), 1, (0, 0)).expect("another slot");
        assert_ne!(other, people);
        // placed, and a swap is two placements
        let map = d.add(Target::command("worldmap"), 0, (0, 1)).expect("added");
        assert!(d.place(people, 0, (0, 1)));
        assert!(d.place(map, 0, (0, 0)));
        assert!(!d.place(map, 0, (0, 0)), "it stands there");
        assert!(!d.place(99, 0, (0, 0)));
        assert_eq!(d.shortcut(people).map(Shortcut::cell), Some((0, 1)));
        // to another slot
        assert!(d.place(map, 2, (4, 4)));
        assert_eq!(d.shortcut(map).map(|s| s.slot), Some(2));
        // but not to a slot that has the same thing
        assert!(!d.place(people, 1, (5, 5)), "slot 1 has People already");
        assert_eq!(d.shortcut(people).map(|s| s.slot), Some(0));
        // a name of its own is cleaned and cut when it is read
        let mut value = written(&d);
        value["shortcuts"][0]["name"] = json!(format!("  {}\n", "x".repeat(100)));
        let read: Desktop = serde_json::from_value(value).expect("read");
        assert_eq!(read.shortcuts[0].name.as_ref().map(|n| n.chars().count()), Some(MAX_NAME));
        // removed: its id is never given again
        assert_eq!(d.remove(people).map(|s| s.id), Ok(people));
        assert_eq!(d.remove(people), Err(Error::Missing));
        let next = d.add(Target::command("people"), 0, (0, 0)).expect("added");
        assert!(next > map);
        // full
        let mut d = Desktop::default();
        for n in 0..MAX_SHORTCUTS {
            d.add(Target::command(&format!("c{n}")), 0, (0, 0)).expect("room");
        }
        assert_eq!(d.add(Target::command("one-more"), 0, (0, 0)), Err(Error::Full));
        // a file with more than the limit: the rest is kept, not dropped
        let mut value = written(&d);
        let extra = json!({ "id": 9000, "target": { "command": "extra" } });
        value["shortcuts"].as_array_mut().expect("a list").push(extra.clone());
        let read: Desktop = serde_json::from_value(value).expect("read");
        assert_eq!(read.shortcuts.len(), MAX_SHORTCUTS);
        assert_eq!(read.kept, vec![extra]);
    }

    #[test]
    fn a_folder_is_kept_with_its_account() {
        let (me, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut d = Desktop::default();
        let folder = |n: u128, account| Target::Folder {
            folder: Uuid::from_u128(n),
            account,
        };
        let mine = d.add(folder(10, me), 0, (0, 1)).expect("added");
        // the same folder twice in one desktop: refused
        assert_eq!(d.add(folder(10, me), 0, (0, 2)), Err(Error::Existing(mine)));
        // the same id on another account is another folder
        let theirs = d.add(folder(10, other), 0, (0, 2)).expect("added");
        assert_ne!(mine, theirs);
        let read: Desktop = serde_json::from_value(written(&d)).expect("read");
        assert_eq!(read.shortcut(theirs).map(|s| &s.target), Some(&folder(10, other)));
    }
}
