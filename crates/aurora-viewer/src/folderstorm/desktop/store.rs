//! The margins' desktop as `settings.json` keeps it (field
//! `folderstorm_desktop`), its limits and the operations of the surface.
//! Pure: nothing here touches egui, the inventory or the network
//! (folderstorm/plans/margin-desktop.md, "Data model and storage").
//!
//! Each account has its own desktop: the shortcuts and where they stand
//! are kept by account id (`accounts`), as the gesture boards are. The
//! choices of Preferences (the switch, the size, the taskbar) are the
//! profile's. [`Desktop`] holds the shortcuts of the account logged in
//! ([`Desktop::sign_in`]) and the other accounts' aside, unread by the
//! surface: another account's shortcuts never show.
//!
//! Before that, the shortcuts were the profile's (`shortcuts` beside the
//! choices). Such a file is still read: a folder shortcut goes to the
//! account it names, the rest waits for the first account that logs in.
//!
//! A shortcut keeps a command id, an inventory folder id or an inventory
//! item id, never an asset id, a name read from the inventory or a window
//! title: what it shows, and what kind of item it opens, is read when it
//! is drawn. Only a name the user typed is saved.
//!
//! Reading never fails and never loses what it cannot use: a shortcut that
//! cannot be read (a newer version's target, a damaged file) is kept as it
//! was read ([`Desktop::kept`]) and written back unchanged; values out of
//! their range are brought back into it.

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// At most this many shortcuts for an account.
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
    /// An Inventory folder of the desktop's account: its id there, never
    /// an asset id nor its name.
    Folder { folder: Uuid },
    /// A command of the toolbar catalogue, by its id.
    Command { command: String },
    /// An Inventory item of the desktop's account (a landmark): its item
    /// id there, a link followed when it was added. Never its asset id,
    /// its name or its kind, which are read from the inventory when it is
    /// drawn or opened.
    Item { item: Uuid },
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

/// The shortcuts of one account, as `accounts` keeps them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Board {
    /// The id the next shortcut takes.
    pub next_id: u32,
    pub shortcuts: Vec<Shortcut>,
    /// Shortcuts this version could not read, written back as they were.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<serde_json::Value>,
}

impl Board {
    /// Whatever `entries` hold that this version reads; the rest is kept
    /// unread.
    fn read(next_id: Option<&serde_json::Value>, entries: impl Iterator<Item = serde_json::Value>) -> Self {
        let mut out = Board {
            next_id: next_id.and_then(|v| v.as_u64()).map_or(0, |n| n.min(u64::from(u32::MAX)) as u32),
            ..Default::default()
        };
        for entry in entries {
            match serde_json::from_value::<Shortcut>(entry.clone()) {
                Ok(shortcut) if out.shortcuts.len() < MAX_SHORTCUTS && out.shortcuts.iter().all(|s| s.id != shortcut.id) => {
                    out.shortcuts.push(shortcut.normalized());
                }
                _ => out.kept.push(entry),
            }
        }
        out.raise_next_id();
        out
    }

    /// The next id is above every id read.
    fn raise_next_id(&mut self) {
        let above = self.shortcuts.iter().map(|s| s.id).max().map_or(0, |m| m.saturating_add(1));
        self.next_id = self.next_id.max(above);
    }

    pub fn is_empty(&self) -> bool {
        self.shortcuts.is_empty() && self.kept.is_empty()
    }

    /// Takes a shortcut made elsewhere (the profile's desktop of an older
    /// version). It keeps its id unless one here has it; beyond the limit
    /// it is kept unread rather than dropped.
    fn absorb(&mut self, mut shortcut: Shortcut) {
        if self.shortcuts.len() >= MAX_SHORTCUTS {
            if let Ok(value) = serde_json::to_value(&shortcut) {
                self.kept.push(value);
            }
            return;
        }
        if self.shortcuts.iter().any(|s| s.id == shortcut.id) {
            shortcut.id = self.next_id.max(1);
        }
        self.shortcuts.push(shortcut);
        self.raise_next_id();
    }

    /// Takes everything of `other`.
    fn merge(&mut self, other: Board) {
        for shortcut in other.shortcuts {
            self.absorb(shortcut);
        }
        self.kept.extend(other.kept);
        self.next_id = self.next_id.max(other.next_id);
    }
}

/// The entries of `shortcuts` then of `kept` in `value` (what an older run
/// kept is read again: this version may know it).
fn entries(value: &serde_json::Value) -> impl Iterator<Item = serde_json::Value> + '_ {
    ["shortcuts", "kept"]
        .into_iter()
        .filter_map(|key| value.get(key))
        .filter_map(|list| list.as_array())
        .flatten()
        .cloned()
}

/// The account a folder shortcut of an older version names
/// (`target.account`), when `entry` is one.
fn named_account(entry: &serde_json::Value) -> Option<Uuid> {
    let target = entry.get("target")?;
    target.get("folder")?;
    let account = target.get("account")?.as_str()?.parse::<Uuid>().ok()?;
    (!account.is_nil()).then_some(account)
}

/// The desktop: the profile's choices, and the shortcuts of the account
/// logged in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Desktop {
    /// *Shortcuts in the margins*.
    pub shortcuts_on: bool,
    pub size: Size,
    /// A single click opens (checkpoint 3 of the plan gives it a switch).
    pub single_click: bool,
    pub taskbar: Taskbar,
    /// The id the next shortcut of [`Desktop::account`] takes.
    pub next_id: u32,
    /// The shortcuts of [`Desktop::account`]. While no account is logged
    /// in: those of an older version's file that no account has taken yet
    /// (they are the first one's to log in).
    pub shortcuts: Vec<Shortcut>,
    /// Shortcuts of that account this version could not read, written back
    /// as they were.
    pub kept: Vec<serde_json::Value>,
    /// Whose shortcuts these are: the account logged in. Not written.
    pub account: Option<Uuid>,
    /// The other accounts' shortcuts: never shown, written back.
    pub others: std::collections::BTreeMap<Uuid, Board>,
    /// Entries of `accounts` this version could not read (a key that is
    /// not an account id), written back as they were.
    pub kept_accounts: std::collections::BTreeMap<String, serde_json::Value>,
}

impl Serialize for Desktop {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        fn value<T: Serialize, E: serde::ser::Error>(of: &T) -> Result<serde_json::Value, E> {
            serde_json::to_value(of).map_err(E::custom)
        }
        let mut out = serde_json::Map::new();
        if self.shortcuts_on {
            out.insert("shortcuts_on".into(), true.into());
        }
        out.insert("size".into(), value(&self.size)?);
        if self.single_click {
            out.insert("single_click".into(), true.into());
        }
        out.insert("taskbar".into(), value(&self.taskbar)?);
        let mine = Board {
            next_id: self.next_id,
            shortcuts: self.shortcuts.clone(),
            kept: self.kept.clone(),
        };
        let mut accounts = serde_json::Map::new();
        for (key, kept) in &self.kept_accounts {
            accounts.insert(key.clone(), kept.clone());
        }
        for (account, board) in self.others.iter().filter(|(_, board)| !board.is_empty()) {
            accounts.insert(account.to_string(), value(board)?);
        }
        match self.account {
            Some(account) if !mine.is_empty() => {
                accounts.insert(account.to_string(), value(&mine)?);
            }
            // no account has taken them yet: as the older version wrote them
            None if !mine.is_empty() => {
                out.insert("next_id".into(), mine.next_id.into());
                out.insert("shortcuts".into(), value(&mine.shortcuts)?);
                if !mine.kept.is_empty() {
                    out.insert("kept".into(), mine.kept.into());
                }
            }
            _ => {}
        }
        if !accounts.is_empty() {
            out.insert("accounts".into(), accounts.into());
        }
        out.serialize(serializer)
    }
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
    /// shortcuts is kept unread. No account is logged in yet
    /// ([`Desktop::sign_in`]).
    pub fn from_value(value: serde_json::Value) -> Self {
        let read = |key: &str| value.get(key).cloned();
        let flag = |key: &str| read(key).and_then(|v| v.as_bool()).unwrap_or(false);
        let mut out = Desktop {
            shortcuts_on: flag("shortcuts_on"),
            size: read("size").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            single_click: flag("single_click"),
            taskbar: read("taskbar").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            ..Default::default()
        };
        out.taskbar.slot = out.taskbar.slot.map(|slot| slot.min(MAX_SLOTS - 1));
        if let Some(accounts) = value.get("accounts").and_then(|v| v.as_object()) {
            for (key, board) in accounts {
                match key.parse::<Uuid>() {
                    Ok(account) if !account.is_nil() && board.is_object() && !out.others.contains_key(&account) => {
                        out.others.insert(account, Board::read(board.get("next_id"), entries(board)));
                    }
                    _ => {
                        out.kept_accounts.insert(key.clone(), board.clone());
                    }
                }
            }
        }
        // an older version's file, the shortcuts being the profile's: a
        // folder goes to the account it names, the rest waits for the
        // first account that logs in
        let (folders, rest): (Vec<_>, Vec<_>) = entries(&value).partition(|entry| named_account(entry).is_some());
        let mine = Board::read(value.get("next_id"), rest.into_iter());
        (out.next_id, out.shortcuts, out.kept) = (mine.next_id, mine.shortcuts, mine.kept);
        for entry in folders {
            let Some(account) = named_account(&entry) else {
                continue;
            };
            let board = out.others.entry(account).or_default();
            match serde_json::from_value::<Shortcut>(entry.clone()) {
                Ok(shortcut) => board.absorb(shortcut.normalized()),
                Err(_) => board.kept.push(entry),
            }
        }
        out
    }

    /// The account logged in is `account` (nil: none, the login screen):
    /// its shortcuts become the desktop's, the previous account's are put
    /// aside. Shortcuts no account had taken (an older version's file) are
    /// given to it. True when it took such shortcuts: save the settings,
    /// so that they stay this account's. Cheap when the account is the
    /// same.
    pub fn sign_in(&mut self, account: Uuid) -> bool {
        let account = (!account.is_nil()).then_some(account);
        if self.account == account {
            return false;
        }
        let was = Board {
            next_id: std::mem::take(&mut self.next_id),
            shortcuts: std::mem::take(&mut self.shortcuts),
            kept: std::mem::take(&mut self.kept),
        };
        let mut mine = account.and_then(|a| self.others.remove(&a)).unwrap_or_default();
        let mut took = false;
        match self.account {
            Some(previous) => {
                if !was.is_empty() {
                    self.others.insert(previous, was);
                }
            }
            // the first account to log in
            None => {
                took = !was.is_empty();
                mine.merge(was);
            }
        }
        self.account = account;
        (self.next_id, self.shortcuts, self.kept) = (mine.next_id, mine.shortcuts, mine.kept);
        took
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

    /// Gives the shortcut `id` the name `text` ([`clean_name`]); an empty
    /// one gives it its target's own name back. Never renames what it
    /// opens. True when its name changed.
    pub fn rename(&mut self, id: u32, text: &str) -> bool {
        let name = Some(clean_name(text)).filter(|name| !name.is_empty());
        match self.shortcut_mut(id) {
            Some(shortcut) if shortcut.name != name => {
                shortcut.name = name;
                true
            }
            _ => false,
        }
    }

    /// Gives the shortcut `id` the icon `icon` (a Tabler name); None gives
    /// it its target's own back. True when its icon changed.
    pub fn set_icon(&mut self, id: u32, icon: Option<&str>) -> bool {
        let icon = icon.map(clean_name).filter(|icon| !icon.is_empty());
        match self.shortcut_mut(id) {
            Some(shortcut) if shortcut.icon != icon => {
                shortcut.icon = icon;
                true
            }
            _ => false,
        }
    }

    /// Puts the shortcuts `order` of `slot` in a grid of `rows` rows, from
    /// its first cell, down then right (*Arrange by name*: the surface
    /// gives the order, which reads names this record does not keep). True
    /// when one moved.
    pub fn arrange(&mut self, slot: u8, order: &[u32], rows: u16) -> bool {
        let rows = u32::from(rows.max(1));
        let mut changed = false;
        let mut n = 0u32;
        for id in order {
            let Some(shortcut) = self.shortcuts.iter_mut().find(|s| s.id == *id && s.slot == slot) else {
                continue;
            };
            let to = ((n / rows).min(u32::from(MAX_CELL - 1)) as u16, (n % rows) as u16);
            n += 1;
            changed |= shortcut.cell() != to;
            (shortcut.col, shortcut.row) = to;
        }
        changed
    }

    /// How many shortcuts stand on a slot that is not one of `live` (the
    /// slots that have a room now): kept, not shown.
    pub fn unused(&self, live: &[u8]) -> usize {
        self.shortcuts.iter().filter(|s| !live.contains(&s.slot)).count()
    }

    /// Removes every shortcut of the account logged in (what this version
    /// could not read is kept). How many went.
    pub fn clear(&mut self) -> usize {
        let count = self.shortcuts.len();
        self.shortcuts.clear();
        count
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
    fn a_shortcut_is_renamed_given_an_icon_and_arranged() {
        let mut d = Desktop::default();
        d.sign_in(Uuid::from_u128(1));
        let people = d.add(Target::command("people"), 0, (3, 1)).expect("added");
        let build = d.add(Target::command("build"), 0, (0, 5)).expect("added");
        let other = d.add(Target::command("people"), 2, (4, 4)).expect("added");
        // a name is cleaned; an empty one gives the target's own back
        assert!(d.rename(people, "  My\tfriends  "));
        assert_eq!(d.shortcut(people).and_then(|s| s.name.clone()), Some("My friends".to_owned()));
        assert!(!d.rename(people, "My friends"), "the same name: nothing changed");
        assert!(d.rename(people, &"x".repeat(200)));
        assert_eq!(
            d.shortcut(people).and_then(|s| s.name.as_ref()).map(|n| n.chars().count()),
            Some(MAX_NAME)
        );
        assert!(d.rename(people, "   "));
        assert_eq!(d.shortcut(people).and_then(|s| s.name.clone()), None);
        assert!(!d.rename(999, "nobody"));
        // an icon, and the target's own back
        assert!(d.set_icon(build, Some("star-filled")));
        assert!(!d.set_icon(build, Some("star-filled")));
        assert_eq!(d.shortcut(build).and_then(|s| s.icon.clone()), Some("star-filled".to_owned()));
        assert!(d.set_icon(build, None));
        assert_eq!(d.shortcut(build).and_then(|s| s.icon.clone()), None);
        // arranged: down then right, the other slot's untouched
        assert!(d.arrange(0, &[build, other, people], 1));
        assert_eq!(d.shortcut(build).map(Shortcut::cell), Some((0, 0)));
        assert_eq!(d.shortcut(people).map(Shortcut::cell), Some((1, 0)));
        assert_eq!(d.shortcut(other).map(Shortcut::cell), Some((4, 4)));
        assert!(d.arrange(0, &[people, build], 4));
        assert_eq!(d.shortcut(people).map(Shortcut::cell), Some((0, 0)));
        assert_eq!(d.shortcut(build).map(Shortcut::cell), Some((0, 1)));
        assert!(!d.arrange(0, &[people, build], 4), "already so");
        // what stands on a slot without a room
        assert_eq!(d.unused(&[0, 2]), 0);
        assert_eq!(d.unused(&[0]), 1);
        assert_eq!(d.unused(&[]), 3);
    }

    #[test]
    fn an_item_shortcut_keeps_its_item_id_and_is_read_back() {
        let mut d = Desktop::default();
        d.sign_in(Uuid::from_u128(1));
        let item = Target::Item { item: Uuid::from_u128(7) };
        let id = d.add(item.clone(), 0, (2, 3)).expect("added");
        // once on a desktop, as a folder or a command
        assert_eq!(d.add(item.clone(), 0, (0, 0)), Err(Error::Existing(id)));
        assert!(d.add(item.clone(), 1, (0, 0)).is_ok());
        // a folder of the same id is another thing
        assert!(
            d.add(
                Target::Folder {
                    folder: Uuid::from_u128(7)
                },
                0,
                (0, 0)
            )
            .is_ok()
        );
        let value = written(&d);
        let kept = &value["accounts"][Uuid::from_u128(1).to_string()]["shortcuts"][0];
        assert_eq!(kept["target"], json!({ "item": Uuid::from_u128(7) }), "its item id, nothing else");
        let mut back = Desktop::from_value(value);
        back.sign_in(Uuid::from_u128(1));
        assert_eq!(back.shortcuts, d.shortcuts);
        assert_eq!(back.shortcut(id).map(|s| &s.target), Some(&item));
    }

    #[test]
    fn removing_all_takes_this_account_s_shortcuts_only() {
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut d = Desktop::default();
        d.sign_in(first);
        d.add(Target::command("people"), 0, (0, 0)).expect("added");
        d.sign_in(second);
        d.add(Target::command("build"), 0, (0, 0)).expect("added");
        d.add(Target::command("people"), 0, (0, 1)).expect("added");
        d.kept.push(json!({ "id": 9, "target": { "landmark": "abc" } }));
        assert_eq!(d.clear(), 2);
        assert!(d.shortcuts.is_empty());
        assert_eq!(d.kept.len(), 1, "what this version cannot read is kept");
        d.sign_in(first);
        assert_eq!(d.shortcuts.len(), 1, "the other account's desktop is untouched");
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
        assert_eq!(value["shortcuts"][1]["target"], json!({ "folder": Uuid::from_u128(7) }));
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
    fn each_account_has_its_own_desktop() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let folder = |n: u128| Target::Folder {
            folder: Uuid::from_u128(n),
        };
        let mut d = Desktop {
            shortcuts_on: true,
            size: Size::Small,
            ..Default::default()
        };
        assert!(!d.sign_in(a), "nothing to take");
        let people = d.add(Target::command("people"), 0, (0, 0)).expect("added");
        let mine = d.add(folder(10), 0, (0, 1)).expect("added");
        // the same folder twice in one desktop: refused
        assert_eq!(d.add(folder(10), 0, (0, 2)), Err(Error::Existing(mine)));
        let of_a = d.shortcuts.clone();
        // another account on the same profile: an empty desktop, the
        // profile's choices
        assert!(!d.sign_in(b));
        assert!(d.shortcuts.is_empty() && d.shortcut(people).is_none());
        assert!(d.shortcuts_on && d.size == Size::Small);
        // the same folder id there is its own shortcut, at its own place
        d.add(folder(10), 1, (4, 4)).expect("added");
        d.add(Target::command("worldmap"), 0, (0, 0)).expect("added");
        let of_b = d.shortcuts.clone();
        // written by account, without the account in the target
        let value = written(&d);
        assert!(value.get("shortcuts").is_none() && value.get("next_id").is_none());
        assert_eq!(
            value["accounts"][a.to_string()]["shortcuts"][1]["target"],
            json!({ "folder": Uuid::from_u128(10) })
        );
        assert_eq!(value["accounts"][b.to_string()]["shortcuts"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            (value["shortcuts_on"].clone(), value["size"].clone()),
            (json!(true), json!("small"))
        );
        // back to the first account: its desktop as it was
        assert!(!d.sign_in(a));
        assert_eq!(d.shortcuts, of_a);
        assert_eq!(written(&d), value, "the file does not depend on who is logged in");
        // read again: nothing shows before an account logs in
        let mut read: Desktop = serde_json::from_value(value.clone()).expect("read");
        assert!(read.shortcuts.is_empty() && read.account.is_none());
        assert_eq!(written(&read), value);
        assert!(!read.sign_in(b));
        assert_eq!(read.shortcuts, of_b);
        assert!(!read.sign_in(a));
        assert_eq!(read.shortcuts, of_a);
        // ids go on from where that account's were
        assert!(read.add(Target::command("inventory"), 0, (0, 5)).expect("added") > mine);
        // logged out: nothing shows, nothing is lost
        assert!(!read.sign_in(Uuid::nil()));
        assert!(read.shortcuts.is_empty());
        assert_eq!(read.others.len(), 2);
        // an account that removed everything leaves no entry
        assert!(!read.sign_in(b));
        for id in of_b.iter().map(|s| s.id) {
            read.remove(id).expect("removed");
        }
        assert!(written(&read)["accounts"].get(b.to_string()).is_none());
    }

    #[test]
    fn a_profile_s_desktop_of_an_older_version_goes_to_its_accounts() {
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let newer = json!({ "id": 9, "target": { "landmark": "abc" } });
        // checkpoints 1 and 2: the shortcuts beside the choices, a folder
        // naming its account
        let old = json!({
            "shortcuts_on": true,
            "size": "large",
            "next_id": 7,
            "shortcuts": [
                { "id": 1, "target": { "command": "people" }, "slot": 0, "col": 0, "row": 0, "name": "Friends" },
                { "id": 2, "target": { "folder": Uuid::from_u128(10), "account": a }, "slot": 0, "col": 1, "row": 0 },
                { "id": 3, "target": { "folder": Uuid::from_u128(20), "account": b }, "slot": 2, "col": 1, "row": 3 },
                { "id": 4, "target": { "command": "worldmap" }, "slot": 2, "col": 0, "row": 1 },
                newer,
            ],
        });
        let read: Desktop = serde_json::from_value(old.clone()).expect("read");
        assert!(read.shortcuts_on && read.size == Size::Large);
        // the folders are their accounts' already; the rest waits
        assert_eq!(read.shortcuts.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 4]);
        assert_eq!(read.kept, vec![newer.clone()]);
        assert_eq!(read.others.keys().copied().collect::<Vec<_>>(), [a, b]);
        // written before anyone logged in: nothing lost, and read the same
        let again: Desktop = serde_json::from_value(written(&read)).expect("read");
        assert_eq!(again, read);
        // B logs in first: the commands are B's, with B's folder, where
        // they stood
        let mut first_b = read.clone();
        assert!(first_b.sign_in(b), "took the profile's shortcuts: to be saved");
        let places: Vec<_> = first_b.shortcuts.iter().map(|s| (s.id, s.slot, s.cell())).collect();
        assert_eq!(places, [(3, 2, (1, 3)), (1, 0, (0, 0)), (4, 2, (0, 1))]);
        assert_eq!(first_b.shortcut(1).and_then(|s| s.name.as_deref()), Some("Friends"));
        assert_eq!(first_b.kept, vec![newer.clone()]);
        assert_eq!(first_b.next_id, 7);
        let value = written(&first_b);
        assert!(value.get("shortcuts").is_none() && value.get("kept").is_none());
        // A then: its folder only
        assert!(!first_b.sign_in(a));
        let targets: Vec<_> = first_b.shortcuts.iter().map(|s| s.target.clone()).collect();
        assert_eq!(
            targets,
            [Target::Folder {
                folder: Uuid::from_u128(10)
            }]
        );
        // an account the file never named: an empty desktop
        assert!(!first_b.sign_in(c));
        assert!(first_b.shortcuts.is_empty() && first_b.kept.is_empty());
        // the same after the file was written and read
        let mut later: Desktop = serde_json::from_value(value).expect("read");
        assert!(!later.sign_in(a), "nothing left to take");
        assert_eq!(later.shortcuts.len(), 1);
        assert!(!later.sign_in(b));
        assert_eq!(later.shortcuts.len(), 3);
        // A logs in first instead: everything but B's folder
        let mut first_a = read;
        assert!(first_a.sign_in(a));
        assert_eq!(first_a.shortcuts.iter().map(|s| s.id).collect::<Vec<_>>(), [2, 1, 4]);
        assert!(!first_a.sign_in(b));
        assert_eq!(first_a.shortcuts.iter().map(|s| s.id).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn accounts_are_read_leniently_and_nothing_is_lost() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let newer = json!({ "id": 2, "target": { "landmark": "abc" } });
        let value = json!({
            "accounts": {
                (a.to_string()): {
                    "next_id": "many",
                    "shortcuts": [
                        { "id": 5, "target": { "command": "people" }, "slot": 99, "name": "  " },
                        newer,
                        { "id": 5, "target": { "command": "inventory" } },
                    ],
                },
                "not-an-account": { "shortcuts": [] },
                (Uuid::nil().to_string()): { "shortcuts": [{ "id": 1, "target": { "command": "build" } }] },
                (b.to_string()): "damaged",
            },
            // an older version's shortcuts beside them, their ids taken
            "shortcuts": [
                { "id": 5, "target": { "folder": Uuid::from_u128(10), "account": a } },
                { "id": 6, "target": { "folder": Uuid::from_u128(11), "account": "nobody" } },
                { "id": 5, "target": { "command": "worldmap" } },
            ],
        });
        let mut read: Desktop = serde_json::from_value(value).expect("reading never fails");
        assert_eq!(read.kept_accounts.len(), 3);
        let again: Desktop = serde_json::from_value(written(&read)).expect("read");
        assert_eq!(again, read);
        assert!(read.sign_in(a));
        // A's own, A's folder under a new id, then the profile's (the
        // folder without an account, and a command under a new id)
        let ids: Vec<_> = read.shortcuts.iter().map(|s| s.id).collect();
        assert_eq!(ids, [5, 6, 7, 8]);
        assert_eq!(read.shortcuts[0].slot, MAX_SLOTS - 1);
        assert_eq!(read.shortcuts[0].name, None);
        assert_eq!(
            read.shortcuts[1].target,
            Target::Folder {
                folder: Uuid::from_u128(10)
            }
        );
        assert_eq!(read.shortcuts[3].target, Target::command("worldmap"));
        assert_eq!(read.next_id, 9);
        assert_eq!(read.kept.len(), 2, "the newer target and the second id 5");
        let out = written(&read);
        for key in ["not-an-account", &Uuid::nil().to_string(), &b.to_string()] {
            assert!(out["accounts"].get(key).is_some(), "{key} is written back");
        }
        // an account at its limit: what comes from the profile is kept
        let mut full = Desktop::default();
        full.sign_in(a);
        for n in 0..MAX_SHORTCUTS {
            full.add(Target::command(&format!("c{n}")), 0, (0, 0)).expect("room");
        }
        let mut value = written(&full);
        value["shortcuts"] = json!([{ "id": 1, "target": { "command": "extra" } }]);
        let mut read: Desktop = serde_json::from_value(value).expect("read");
        assert!(read.sign_in(a));
        assert_eq!((read.shortcuts.len(), read.kept.len()), (MAX_SHORTCUTS, 1));
        // `accounts` that is not a table
        let read: Desktop = serde_json::from_value(json!({ "accounts": [1, 2] })).expect("read");
        assert_eq!(read, Desktop::default());
    }
}
