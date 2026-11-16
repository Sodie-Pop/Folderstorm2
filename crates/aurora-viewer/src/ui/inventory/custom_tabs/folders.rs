//! Folderstorm: the folder scope of a custom Inventory tab (feature 2.2).
//!
//! Port of Folderstorm's "saved views" (branch `codex/inventory-view-folders`:
//! `FSInventoryCustomTabs::selectedFolders`, `changeFolderScope` and
//! `LLInventoryFilter::refreshViewFolders` / `passesViewFolders`). A custom
//! tab may be limited to chosen folders: it then lists those folders and
//! everything below them, at any depth, under their path, with its filters
//! and search applied inside. The folders are kept by id, so a moved folder
//! follows; one deleted or in the Trash is skipped but kept, and comes back
//! if it is restored.
//!
//! The ids are those of one account: each tab keeps a list per account id
//! (`CustomTab::folders`). On an account with no list the tab is not scoped.
//!
//! Plain logic, tested without a window; `folder_ui.rs` draws it.

use super::*;
use std::collections::BTreeMap;

/// The most folders one tab is scoped to (Folderstorm's
/// `FSInventoryViewFolderLimit`).
pub const MAX_FOLDERS: usize = 32;

/// What a saved folder is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// In the inventory, outside the Trash: the tab shows it.
    Live,
    /// Not in the inventory (deleted, or not loaded).
    Missing,
    /// The Trash or a folder in it.
    Trash,
    /// Never a scope: the inventory's root or the Library (a hand-edited
    /// setting).
    Refused,
}

/// Where a folder stands, as `refreshViewFolders` checks it: it must be in
/// the agent's inventory (under its root) and out of the Trash.
pub fn status(inv: &Inventory, id: Uuid) -> Status {
    let Some(folder) = inv.folders.get(&id) else {
        return Status::Missing;
    };
    if id.is_nil() || id == inv.root || id == inv.lib_root || folder.library || rules::library(inv, id) {
        return Status::Refused;
    }
    if rules::system(inv, 14).is_some_and(|trash| rules::under(inv, id, trash)) {
        return Status::Trash;
    }
    if inv.root.is_nil() || !rules::under(inv, id, inv.root) {
        return Status::Missing;
    }
    Status::Live
}

/// Why a selection cannot go into a tab (the tooltip of a greyed entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing is selected.
    Empty,
    /// A link in the selection points to nothing loaded.
    Broken,
    /// The inventory's root, the Library, the Trash or what it holds (or an
    /// entry not in the inventory).
    NotAllowed,
    /// More than `MAX_FOLDERS` folders.
    TooMany,
    /// More than `items::MAX_ITEMS` items.
    TooManyItems,
}

/// What a selection gives a tab (feature 2.13): its folders join the
/// scope, its items are pinned. A link gives what it points to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chosen {
    /// In the order of their paths.
    pub folders: Vec<Uuid>,
    /// In the order of their paths.
    pub items: Vec<Uuid>,
}

impl Chosen {
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.items.is_empty()
    }

    /// Every folder and item, folders first.
    pub fn all(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.folders.iter().chain(&self.items).copied()
    }
}

/// What a selection may give a tab, folders and items in the order of
/// their paths; refused unless every entry is live (the Trash or the
/// Library refuses it all, as `FSInventoryCustomTabs::selectedFolders`
/// does for folders), nor over `MAX_FOLDERS` folders or
/// `items::MAX_ITEMS` items. A link gives its original (a folder link its
/// folder), as `items::resolve` says.
pub fn check(inv: &Inventory, selection: &[Uuid]) -> Result<Chosen, Refusal> {
    if selection.is_empty() {
        return Err(Refusal::Empty);
    }
    let mut folders = Vec::new();
    let mut items = Vec::new();
    for id in selection {
        match super::items::resolve(inv, *id) {
            None if inv.items.contains_key(id) => return Err(Refusal::Broken),
            None => return Err(Refusal::NotAllowed),
            Some(super::items::Target::Folder(folder)) => {
                if status(inv, folder) != Status::Live {
                    return Err(Refusal::NotAllowed);
                }
                folders.push(folder);
            }
            Some(super::items::Target::Item(item)) => {
                if super::items::status(inv, item) != Status::Live {
                    return Err(Refusal::NotAllowed);
                }
                items.push(item);
            }
        }
    }
    let by_path = |ids: Vec<Uuid>| {
        let mut ids: Vec<(String, Uuid)> = ids
            .into_iter()
            .map(|id| (super::super::inventory_path(inv, id).to_lowercase(), id))
            .collect();
        ids.sort();
        ids.dedup_by_key(|(_, id)| *id);
        ids.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    };
    let chosen = Chosen {
        folders: by_path(folders),
        items: by_path(items),
    };
    if chosen.folders.len() > MAX_FOLDERS {
        return Err(Refusal::TooMany);
    }
    if chosen.items.len() > super::items::MAX_ITEMS {
        return Err(Refusal::TooManyItems);
    }
    Ok(chosen)
}

/// `check`, without the reason: nothing when refused.
pub fn choosable(inv: &Inventory, selection: &[Uuid]) -> Chosen {
    check(inv, selection).unwrap_or_default()
}

/// How the tab's folders change (the three entries of Folderstorm's tab
/// menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The chosen folders join the ones the tab has.
    Add,
    /// Only the chosen folders.
    Replace,
    /// No scope: the whole inventory.
    Clear,
}

/// Over the limit (`MAX_FOLDERS`, or `items::MAX_ITEMS`): nothing changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooMany;

/// The tab's new folders, from its `current` ones and the `chosen` ones.
pub fn change(current: &[Uuid], chosen: &[Uuid], how: Change) -> Result<Vec<Uuid>, TooMany> {
    change_within(current, chosen, how, MAX_FOLDERS)
}

/// `change`, with at most `limit` in the list.
pub fn change_within(current: &[Uuid], chosen: &[Uuid], how: Change, limit: usize) -> Result<Vec<Uuid>, TooMany> {
    let mut out = match how {
        Change::Clear => return Ok(Vec::new()),
        Change::Replace => Vec::new(),
        Change::Add => current.to_vec(),
    };
    for id in chosen {
        if !out.contains(id) {
            out.push(*id);
        }
    }
    if out.len() > limit {
        return Err(TooMany);
    }
    Ok(out)
}

/// The tab's folders on this account (empty: not scoped).
pub fn of(tab: &CustomTab, account: Uuid) -> &[Uuid] {
    tab.folders.get(&account).map_or(&[], Vec::as_slice)
}

/// Sets the tab's folders on this account; an empty list removes the scope.
pub fn set(tab: &mut CustomTab, account: Uuid, folders: Vec<Uuid>) {
    if folders.is_empty() {
        tab.folders.remove(&account);
    } else {
        tab.folders.insert(account, folders);
    }
}

/// The tab is scoped (folders or pinned items) on another account only:
/// here it shows everything.
pub fn elsewhere(tab: &CustomTab, account: Uuid) -> bool {
    (!tab.folders.is_empty() || !tab.items.is_empty()) && !scoped(tab, account)
}

/// The tab has folders or pinned items on this account: it shows only
/// those.
pub fn scoped(tab: &CustomTab, account: Uuid) -> bool {
    !of(tab, account).is_empty() || !super::items::of(tab, account).is_empty()
}

/// The folders the search keeps for this tab on this account: None when it
/// is not scoped, else its live folders (maybe none: then nothing shows but
/// its pinned items).
pub fn search_roots(inv: &Inventory, tab: &CustomTab, account: Uuid) -> Option<Vec<Uuid>> {
    if !scoped(tab, account) {
        return None;
    }
    let saved = of(tab, account);
    Some(saved.iter().copied().filter(|id| status(inv, *id) == Status::Live).collect())
}

/// Settings edited by hand: nil ids and repeats are dropped, each list cut
/// to `MAX_FOLDERS`, empty lists and the nil account removed. Returns
/// whether anything changed.
pub fn normalize(folders: &mut BTreeMap<Uuid, Vec<Uuid>>) -> bool {
    normalize_within(folders, MAX_FOLDERS)
}

/// `normalize`, each list cut to `limit` (folders or pinned items).
pub fn normalize_within(folders: &mut BTreeMap<Uuid, Vec<Uuid>>, limit: usize) -> bool {
    let mut changed = folders.remove(&Uuid::nil()).is_some();
    for ids in folders.values_mut() {
        let before = ids.len();
        let mut at = 0;
        while at < ids.len() {
            if ids[at].is_nil() || ids[..at].contains(&ids[at]) {
                ids.remove(at);
            } else {
                at += 1;
            }
        }
        ids.truncate(limit);
        changed |= ids.len() != before;
    }
    let before = folders.len();
    folders.retain(|_, ids| !ids.is_empty());
    changed || folders.len() != before
}

#[cfg(test)]
pub(in crate::ui::inventory) mod tests;
