//! Workspaces as `settings.json` keeps them (field `folderstorm_workspaces`,
//! entry `workspaces`), their limits and the list's operations
//! (folderstorm/plans/workspaces.md, checkpoint 6 in detail). Pure: nothing
//! here touches egui or the viewer.
//!
//! Folderstorm (C++) saved a workspace as a full snapshot, its own copy of
//! the layout included (doc/help.md "Workspaces"). Here a **workspace**
//! says *what* is open: the windows of the allowlist it has open, and
//! optionally the layout applied with it, by name. Where the windows go is
//! the layout's business, so any workspace works on any layout.
//!
//! As for layouts, reading never fails and never drops an entry: one made
//! by a newer version, or one that breaks a limit, is kept exactly as it
//! was read, shown as such, never applied and written back unchanged.

use super::store::{Favorite, Kept, MAX_WINDOWS, NameError, SCHEMA, Store, StoreError, Unusable, name_error, same_name, tidy};
use super::windows::{self, Window};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// At most this many workspaces.
pub const MAX_WORKSPACES: usize = 32;
/// A window id has at most this many characters (as in a layout).
const MAX_ID: usize = 64;

/// The window a workspace never opens or closes, and does not record:
/// Conversations. Folderstorm (C++): "loading a workspace leaves its
/// open/closed state and selected conversations unchanged".
const LEFT_ALONE: &str = "conversations";

/// Whether a workspace opens and closes the window `id`: every window of
/// the allowlist but Conversations.
pub fn switched(id: &str) -> bool {
    id != LEFT_ALONE && windows::find(id).is_some()
}

/// The windows a workspace opens and closes, in the allowlist's order.
pub fn switchable() -> impl Iterator<Item = &'static Window> {
    windows::all().iter().filter(|window| window.id != LEFT_ALONE)
}

/// A named workspace: which windows are open, and the layout that goes
/// with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub schema: u32,
    pub name: String,
    /// The layout applied with the workspace, by name. None: the layout in
    /// use stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    /// The ids of the windows it has open (`super::windows`); every other
    /// window it switches is closed. Ids this version does not know are
    /// kept and ignored.
    #[serde(default)]
    pub open: BTreeSet<String>,
    /// What this version does not know of the entry, kept as read.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl Workspace {
    /// A workspace of this version's schema. Only the ids of windows a
    /// workspace switches are taken from `open`.
    pub fn new(name: &str, layout: Option<&str>, open: impl IntoIterator<Item = String>) -> Self {
        Workspace {
            schema: SCHEMA,
            name: tidy(name).to_owned(),
            layout: layout.map(|layout| tidy(layout).to_owned()),
            open: open.into_iter().filter(|id| switched(id)).collect(),
            other: BTreeMap::new(),
        }
    }

    /// Whether every value is within the limits. A workspace that fails is
    /// never applied.
    pub fn check(&self) -> Result<(), Unusable> {
        if self.schema > SCHEMA {
            return Err(Unusable::Newer);
        }
        let ok = self.schema >= 1
            && name_error(&self.name).is_none()
            && self.layout.as_deref().is_none_or(|layout| name_error(layout).is_none())
            && self.open.len() <= MAX_WINDOWS
            && self
                .open
                .iter()
                .all(|id| !id.is_empty() && id.chars().count() <= MAX_ID && !id.chars().any(char::is_control));
        if ok { Ok(()) } else { Err(Unusable::Invalid) }
    }

    /// The windows it opens that this version knows, in the allowlist's
    /// order.
    pub fn opens(&self) -> impl Iterator<Item = &'static Window> + '_ {
        switchable().filter(|window| self.open.contains(window.id))
    }

    /// Whether the windows open now (`open`) are the ones it has open.
    pub fn matches(&self, open: &BTreeSet<String>) -> bool {
        switchable().all(|window| self.open.contains(window.id) == open.contains(window.id))
    }
}

/// One entry of the list of workspaces.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Workspace(Workspace),
    Kept(Kept),
}

impl Serialize for Record {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Record::Workspace(workspace) => workspace.serialize(serializer),
            Record::Kept(kept) => kept.raw.serialize(serializer),
        }
    }
}

impl Record {
    /// Reads one entry of the file. Never fails: what cannot be used is
    /// kept.
    fn read(raw: serde_json::Value) -> Self {
        let name = raw
            .get("name")
            .and_then(|name| name.as_str())
            .filter(|name| name_error(name).is_none())
            .map(str::to_owned);
        let newer = raw
            .get("schema")
            .and_then(|schema| schema.as_u64())
            .is_some_and(|schema| schema > u64::from(SCHEMA));
        if newer {
            return Record::Kept(Kept {
                name,
                why: Unusable::Newer,
                raw,
            });
        }
        match serde_json::from_value::<Workspace>(raw.clone()) {
            Ok(workspace) if workspace.check().is_ok() => Record::Workspace(workspace),
            _ => Record::Kept(Kept {
                name,
                why: Unusable::Invalid,
                raw,
            }),
        }
    }

    pub fn workspace(&self) -> Option<&Workspace> {
        match self {
            Record::Workspace(workspace) => Some(workspace),
            Record::Kept(_) => None,
        }
    }

    /// The name to show, if the entry has one.
    pub fn name(&self) -> Option<&str> {
        match self {
            Record::Workspace(workspace) => Some(&workspace.name),
            Record::Kept(kept) => kept.name.as_deref(),
        }
    }
}

/// The list of workspaces as the file has it (`raw`), each entry read on
/// its own; a second workspace of the same name cannot be used.
pub(super) fn read_list(raw: serde_json::Value) -> Vec<Record> {
    let entries = match raw {
        serde_json::Value::Array(entries) => entries,
        _ => Vec::new(),
    };
    if entries.len() > MAX_WORKSPACES {
        log::warn!("settings.json: more than {MAX_WORKSPACES} workspaces, the rest is left out");
    }
    let mut list: Vec<Record> = Vec::new();
    for raw in entries.into_iter().take(MAX_WORKSPACES) {
        let mut record = Record::read(raw);
        if let Record::Workspace(workspace) = &record
            && list
                .iter()
                .filter_map(Record::workspace)
                .any(|other| same_name(&other.name, &workspace.name))
            && let Ok(raw) = serde_json::to_value(workspace)
        {
            record = Record::Kept(Kept {
                name: Some(workspace.name.clone()),
                why: Unusable::Invalid,
                raw,
            });
        }
        list.push(record);
    }
    list
}

impl Store {
    /// The usable workspace of that name, and where it is in the list.
    pub fn find_workspace(&self, name: &str) -> Option<(usize, &Workspace)> {
        self.workspaces.iter().enumerate().find_map(|(index, record)| {
            record
                .workspace()
                .filter(|workspace| same_name(&workspace.name, name))
                .map(|workspace| (index, workspace))
        })
    }

    /// The usable workspace at `index`.
    pub fn workspace(&self, index: usize) -> Option<&Workspace> {
        self.workspaces.get(index).and_then(Record::workspace)
    }

    fn workspace_mut(&mut self, index: usize) -> Option<&mut Workspace> {
        match self.workspaces.get_mut(index) {
            Some(Record::Workspace(workspace)) => Some(workspace),
            _ => None,
        }
    }

    /// The workspace in use, while it is still in the list.
    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace
            .as_deref()
            .and_then(|name| self.find_workspace(name))
            .map(|(_, workspace)| workspace)
    }

    pub fn is_active_workspace(&self, index: usize) -> bool {
        self.workspace(index)
            .zip(self.active_workspace.as_deref())
            .is_some_and(|(workspace, active)| same_name(&workspace.name, active))
    }

    /// Whether an entry of the workspaces, usable or not, has this name.
    pub(super) fn workspace_listed(&self, name: &str) -> bool {
        self.workspaces
            .iter()
            .any(|record| record.name().is_some_and(|other| same_name(other, name)))
    }

    /// What is wrong with `name` for a new workspace, or for the one at
    /// `renamed`. A layout may have the same name.
    pub fn workspace_name_error(&self, name: &str, renamed: Option<usize>) -> Option<NameError> {
        if let Some(error) = name_error(name) {
            return Some(error);
        }
        let taken = self
            .workspaces
            .iter()
            .enumerate()
            .any(|(index, record)| Some(index) != renamed && record.name().is_some_and(|other| same_name(other, name)));
        taken.then_some(NameError::Taken)
    }

    /// Adds a workspace at the end and makes it the one in use. A layout
    /// it names that is not in the list is left out of it.
    pub fn add_workspace(&mut self, mut workspace: Workspace) -> Result<usize, StoreError> {
        if self.workspaces.len() >= MAX_WORKSPACES {
            return Err(StoreError::Full);
        }
        if let Some(error) = self.workspace_name_error(&workspace.name, None) {
            return Err(StoreError::Name(error));
        }
        workspace.layout = workspace
            .layout
            .as_deref()
            .and_then(|name| self.find(name))
            .map(|(_, layout)| layout.name.clone());
        workspace.check().map_err(|_| StoreError::Invalid)?;
        self.active_workspace = Some(workspace.name.clone());
        self.workspaces.push(Record::Workspace(workspace));
        Ok(self.workspaces.len() - 1)
    }

    /// *Update*: the workspace at `index` takes the windows `open` now and
    /// becomes the one in use. Its name, its layout and what this version
    /// does not know of it (ids included) are kept.
    pub fn update_workspace(&mut self, index: usize, open: &BTreeSet<String>) -> Result<(), StoreError> {
        let workspace = self.workspace_mut(index).ok_or(StoreError::Missing)?;
        let mut new: BTreeSet<String> = workspace.open.iter().filter(|id| !switched(id)).cloned().collect();
        new.extend(open.iter().filter(|id| switched(id)).cloned());
        if new.len() > MAX_WINDOWS {
            return Err(StoreError::Invalid);
        }
        workspace.open = new;
        let name = workspace.name.clone();
        self.active_workspace = Some(name);
        Ok(())
    }

    /// Chooses the layout applied with the workspace at `index`, or none.
    /// True when the choice changed.
    pub fn set_workspace_layout(&mut self, index: usize, layout: Option<&str>) -> bool {
        let layout = layout.and_then(|name| self.find(name)).map(|(_, layout)| layout.name.clone());
        match self.workspace_mut(index) {
            Some(workspace) if workspace.layout != layout => {
                workspace.layout = layout;
                true
            }
            _ => false,
        }
    }

    /// Puts the window `id` in the list of the workspace at `index`, or
    /// takes it out. True when the list changed.
    pub fn set_workspace_window(&mut self, index: usize, id: &str, on: bool) -> bool {
        if !switched(id) {
            return false;
        }
        match self.workspace_mut(index) {
            Some(workspace) if on && workspace.open.len() < MAX_WINDOWS => workspace.open.insert(id.to_owned()),
            Some(workspace) if !on => workspace.open.remove(id),
            _ => false,
        }
    }

    /// Renames the workspace at `index`; the mark of the one in use and its
    /// star follow.
    pub fn rename_workspace(&mut self, index: usize, name: &str) -> Result<(), StoreError> {
        if self.workspace(index).is_none() {
            return Err(StoreError::Missing);
        }
        if let Some(error) = self.workspace_name_error(name, Some(index)) {
            return Err(StoreError::Name(error));
        }
        let active = self.is_active_workspace(index);
        let new = tidy(name).to_owned();
        if let Some(workspace) = self.workspace_mut(index) {
            let old = std::mem::replace(&mut workspace.name, new.clone());
            if active {
                self.active_workspace = Some(new.clone());
            }
            for favorite in &mut self.favorites {
                if let Favorite::Workspace(name) = favorite
                    && same_name(name, &old)
                {
                    name.clone_from(&new);
                }
            }
        }
        Ok(())
    }

    /// Removes the entry at `index` of the workspaces, usable or not.
    pub fn remove_workspace(&mut self, index: usize) -> bool {
        if index >= self.workspaces.len() {
            return false;
        }
        if self.is_active_workspace(index) {
            self.active_workspace = None;
        }
        let removed = self.workspaces.remove(index);
        if let Some(name) = removed.name().filter(|name| !self.workspace_listed(name)) {
            self.favorites
                .retain(|favorite| !favorite.workspace().is_some_and(|other| same_name(other, name)));
        }
        true
    }

    /// Moves the entry at `index` of the workspaces one place up (`up`) or
    /// down; where it is afterwards.
    pub fn shift_workspace(&mut self, index: usize, up: bool) -> Option<usize> {
        let to = if up { index.checked_sub(1)? } else { index + 1 };
        if index >= self.workspaces.len() || to >= self.workspaces.len() {
            return None;
        }
        self.workspaces.swap(index, to);
        Some(to)
    }

    /// Whether the workspace of this name is a favorite.
    pub fn is_favorite_workspace(&self, name: &str) -> bool {
        self.favorites
            .iter()
            .any(|favorite| favorite.workspace().is_some_and(|other| same_name(other, name)))
    }

    /// Makes the workspace of this name a favorite (the last one) or takes
    /// its star away. True when the favorites changed.
    pub fn set_favorite_workspace(&mut self, name: &str, on: bool) -> bool {
        if on == self.is_favorite_workspace(name) {
            return false;
        }
        if !on {
            self.favorites
                .retain(|favorite| !favorite.workspace().is_some_and(|other| same_name(other, name)));
            return true;
        }
        let Some((_, workspace)) = self.find_workspace(name) else {
            return false;
        };
        if self.favorites.len() >= super::store::MAX_FAVORITES {
            return false;
        }
        self.favorites.push(Favorite::Workspace(workspace.name.clone()));
        true
    }

    /// A layout was renamed: the workspaces that name it follow.
    pub(super) fn workspaces_follow_layout(&mut self, old: &str, new: Option<&str>) {
        for record in &mut self.workspaces {
            if let Record::Workspace(workspace) = record
                && workspace.layout.as_deref().is_some_and(|name| same_name(name, old))
            {
                workspace.layout = new.map(str::to_owned);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::store::Layout;
    use crate::folderstorm::workspaces::windows::ADDITIONAL_INVENTORY;
    use serde_json::json;

    fn ids(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn sorting() -> Workspace {
        Workspace::new("Sorting", Some("1 monitor"), ids(&["inventory", ADDITIONAL_INVENTORY, "people"]))
    }

    fn store() -> Store {
        let mut store = Store::default();
        for name in ["2 monitors", "1 monitor"] {
            store
                .add(Layout::new(name, Some(Default::default()), None, Vec::new()))
                .expect("added");
        }
        store.set_active(None);
        store.add_workspace(sorting()).expect("added");
        store
            .add_workspace(Workspace::new("Exploring", None, ids(&["minimap", "worldmap"])))
            .expect("added");
        store
    }

    fn saved(store: &Store) -> serde_json::Value {
        serde_json::to_value(store).expect("saved")
    }

    fn read(value: serde_json::Value) -> Store {
        serde_json::from_value(value).expect("read")
    }

    #[test]
    fn workspaces_survive_a_save_and_a_read() {
        let mut store = store();
        assert!(store.set_favorite_workspace("Sorting", true));
        assert!(store.set_favorite("2 monitors", true));
        let file = saved(&store);
        assert_eq!(
            file["workspaces"][0],
            json!({
                "schema": 1, "name": "Sorting", "layout": "1 monitor",
                "open": ["inventory", ADDITIONAL_INVENTORY, "people"],
            })
        );
        assert_eq!(
            file["workspaces"][1],
            json!({ "schema": 1, "name": "Exploring", "open": ["minimap", "worldmap"] })
        );
        assert_eq!(file["active_workspace"], "Exploring");
        assert_eq!(file["favorites"], json!([{ "workspace": "Sorting" }, { "layout": "2 monitors" }]));
        assert_eq!(read(file), store);
    }

    #[test]
    fn a_store_without_workspaces_is_written_as_before() {
        let mut store = store();
        while store.remove_workspace(0) {}
        let file = saved(&store);
        assert!(file.get("workspaces").is_none() && file.get("active_workspace").is_none(), "{file}");
        // and a file from before reads with none
        let old = read(json!({ "layouts": file["layouts"].clone(), "active_layout": "1 monitor" }));
        assert!(old.workspaces.is_empty() && old.active_workspace.is_none());
        assert_eq!(old.active_layout.as_deref(), Some("1 monitor"));
    }

    #[test]
    fn a_workspace_holds_window_ids_and_never_conversations() {
        let workspace = Workspace::new("Chat", None, ids(&["conversations", "people", "options", "no-such-window"]));
        assert_eq!(workspace.open, ids(&["people"]), "only what a workspace switches");
        assert!(!switched("conversations") && switched("inventory") && !switched("options"));
        assert!(switchable().all(|window| window.id != "conversations"));
        let file = serde_json::to_value(&workspace).expect("saved");
        let fields: Vec<&str> = file.as_object().expect("an object").keys().map(String::as_str).collect();
        assert_eq!(fields, ["name", "open", "schema"], "nothing a window contains has a field");
    }

    #[test]
    fn newer_and_damaged_entries_are_kept_as_read_and_not_usable() {
        let newer = json!({ "schema": 9, "name": "Later", "open": [], "hud": ["x"] });
        let damaged = json!({ "schema": 1, "name": "", "open": 3 });
        let twice = json!({ "schema": 1, "name": "sorting", "open": [] });
        let mut file = saved(&store());
        let list = file["workspaces"].as_array_mut().expect("a list");
        list.extend([newer.clone(), damaged.clone(), twice.clone()]);
        let store = read(file);
        assert_eq!(store.workspaces.len(), 5);
        assert!(store.workspace(2).is_none() && store.workspace(3).is_none() && store.workspace(4).is_none());
        assert_eq!(store.workspaces[2].name(), Some("Later"));
        assert!(matches!(&store.workspaces[2], Record::Kept(kept) if kept.why == Unusable::Newer));
        assert!(matches!(&store.workspaces[4], Record::Kept(kept) if kept.why == Unusable::Invalid));
        let again = saved(&store);
        assert_eq!(again["workspaces"][2], newer);
        assert_eq!(again["workspaces"][3], damaged);
        assert_eq!(again["workspaces"][4], twice);
        // its name is taken all the same
        assert_eq!(store.workspace_name_error("later", None), Some(NameError::Taken));
    }

    #[test]
    fn unknown_fields_and_window_ids_are_kept_through_an_update() {
        let mut file = saved(&store());
        file["workspaces"][0]["open"] = json!(["inventory", "a-window-of-later"]);
        file["workspaces"][0]["hud"] = json!(["one"]);
        let mut store = read(file);
        store.update_workspace(0, &ids(&["people", "conversations"])).expect("updated");
        let file = saved(&store);
        assert_eq!(file["workspaces"][0]["open"], json!(["a-window-of-later", "people"]));
        assert_eq!(file["workspaces"][0]["hud"], json!(["one"]));
        assert_eq!(file["workspaces"][0]["layout"], "1 monitor", "its layout is kept");
        assert_eq!(store.active_workspace.as_deref(), Some("Sorting"));
        // what it opens: only windows of now
        let opens: Vec<&str> = store.workspace(0).expect("usable").opens().map(|window| window.id).collect();
        assert_eq!(opens, ["people"]);
    }

    #[test]
    fn limits_are_checked() {
        let mut store = store();
        let too_many: BTreeSet<String> = (0..=MAX_WINDOWS).map(|n| format!("w{n}")).collect();
        let mut workspace = sorting();
        workspace.name = "Many".to_owned();
        workspace.open = too_many;
        assert_eq!(store.add_workspace(workspace), Err(StoreError::Invalid));
        assert_eq!(store.add_workspace(sorting()), Err(StoreError::Name(NameError::Taken)));
        assert_eq!(
            store.add_workspace(Workspace::new("  ", None, BTreeSet::new())),
            Err(StoreError::Name(NameError::Empty))
        );
        // a layout may have the same name
        assert!(store.add_workspace(Workspace::new("1 monitor", None, BTreeSet::new())).is_ok());
        while store.workspaces.len() < MAX_WORKSPACES {
            let name = format!("W{}", store.workspaces.len());
            store.add_workspace(Workspace::new(&name, None, BTreeSet::new())).expect("added");
        }
        assert_eq!(
            store.add_workspace(Workspace::new("One more", None, BTreeSet::new())),
            Err(StoreError::Full)
        );
    }

    #[test]
    fn a_workspace_names_only_a_layout_of_the_list() {
        let mut store = store();
        let index = store
            .add_workspace(Workspace::new("Photos", Some("No such layout"), BTreeSet::new()))
            .expect("added");
        assert_eq!(store.workspace(index).expect("usable").layout, None);
        assert!(store.set_workspace_layout(index, Some("2 MONITORS")));
        assert_eq!(store.workspace(index).expect("usable").layout.as_deref(), Some("2 monitors"));
        assert!(!store.set_workspace_layout(index, Some("2 monitors")), "no change");
        // a file naming a layout that is gone reads without it
        let mut file = saved(&store);
        file["workspaces"][0]["layout"] = json!("Gone");
        assert_eq!(read(file).workspace(0).expect("usable").layout, None);
    }

    #[test]
    fn a_layout_renamed_or_deleted_is_followed() {
        let mut store = store();
        let (index, _) = store.find("1 monitor").expect("listed");
        store.rename(index, "Laptop").expect("renamed");
        assert_eq!(store.workspace(0).expect("usable").layout.as_deref(), Some("Laptop"));
        assert!(store.remove(index));
        assert_eq!(
            store.workspace(0).expect("usable").layout,
            None,
            "the layout in use is kept from now on"
        );
    }

    #[test]
    fn renaming_removing_and_moving_keep_the_marks_right() {
        let mut store = store();
        assert!(store.set_favorite_workspace("exploring", true));
        assert!(store.is_active_workspace(1));
        store.rename_workspace(1, "Walking").expect("renamed");
        assert_eq!(store.active_workspace.as_deref(), Some("Walking"));
        assert!(store.is_favorite_workspace("Walking") && !store.is_favorite_workspace("Exploring"));
        assert_eq!(store.rename_workspace(1, "sorting"), Err(StoreError::Name(NameError::Taken)));
        assert!(store.rename_workspace(1, "WALKING").is_ok(), "its own name in another case");
        assert_eq!(store.shift_workspace(1, true), Some(0));
        assert!(store.is_active_workspace(0));
        assert_eq!(store.shift_workspace(0, true), None);
        assert!(store.remove_workspace(0));
        assert!(store.active_workspace.is_none() && store.favorites.is_empty());
        assert!(!store.remove_workspace(5));
    }

    #[test]
    fn the_windows_of_a_workspace_are_edited_one_by_one() {
        let mut store = store();
        assert!(store.set_workspace_window(1, "places", true));
        assert!(!store.set_workspace_window(1, "places", true));
        assert!(!store.set_workspace_window(1, "conversations", true), "never Conversations");
        assert!(!store.set_workspace_window(1, "options", true), "never a window off the list");
        assert!(store.set_workspace_window(1, "minimap", false));
        assert_eq!(store.workspace(1).expect("usable").open, ids(&["places", "worldmap"]));
    }

    #[test]
    fn a_workspace_matches_the_open_windows_but_for_conversations() {
        let workspace = sorting();
        assert!(workspace.matches(&ids(&["inventory", ADDITIONAL_INVENTORY, "people"])));
        assert!(workspace.matches(&ids(&["inventory", ADDITIONAL_INVENTORY, "people", "conversations", "options"])));
        assert!(!workspace.matches(&ids(&["inventory", "people"])));
        assert!(!workspace.matches(&ids(&["inventory", ADDITIONAL_INVENTORY, "people", "worldmap"])));
    }

    #[test]
    fn a_stale_favorite_workspace_is_dropped_and_none_is_twice() {
        let mut file = saved(&store());
        file["favorites"] = json!([{ "workspace": "Sorting" }, { "workspace": "Gone" }, { "workspace": "sorting" }, { "tool": 1 }]);
        let store = read(file);
        assert_eq!(saved(&store)["favorites"], json!([{ "workspace": "Sorting" }, { "tool": 1 }]));
    }
}
