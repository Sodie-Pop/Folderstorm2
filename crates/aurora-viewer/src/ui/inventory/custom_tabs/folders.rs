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

/// The folders of a selection that may become a scope, in the order of
/// their paths; none unless every entry is a live folder (an item in the
/// selection, the Trash or the Library refuses it all, as
/// `FSInventoryCustomTabs::selectedFolders` does), nor over `MAX_FOLDERS`.
pub fn choosable(inv: &Inventory, selection: &[Uuid]) -> Vec<Uuid> {
    if selection.is_empty() || selection.len() > MAX_FOLDERS {
        return Vec::new();
    }
    if selection.iter().any(|id| status(inv, *id) != Status::Live) {
        return Vec::new();
    }
    let mut ids: Vec<(String, Uuid)> = selection
        .iter()
        .map(|id| (super::super::inventory_path(inv, *id).to_lowercase(), *id))
        .collect();
    ids.sort();
    ids.dedup_by_key(|(_, id)| *id);
    ids.into_iter().map(|(_, id)| id).collect()
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

/// Over `MAX_FOLDERS`: nothing changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooMany;

/// The tab's new folders, from its `current` ones and the `chosen` ones.
pub fn change(current: &[Uuid], chosen: &[Uuid], how: Change) -> Result<Vec<Uuid>, TooMany> {
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
    if out.len() > MAX_FOLDERS {
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

/// The tab is scoped on another account only: here it shows everything.
pub fn elsewhere(tab: &CustomTab, account: Uuid) -> bool {
    !tab.folders.is_empty() && of(tab, account).is_empty()
}

/// The folders the search keeps for this tab on this account: None when it
/// is not scoped, else its live folders (maybe none: then nothing shows).
pub fn search_roots(inv: &Inventory, tab: &CustomTab, account: Uuid) -> Option<Vec<Uuid>> {
    let saved = of(tab, account);
    if saved.is_empty() {
        return None;
    }
    Some(saved.iter().copied().filter(|id| status(inv, *id) == Status::Live).collect())
}

/// Settings edited by hand: nil ids and repeats are dropped, each list cut
/// to `MAX_FOLDERS`, empty lists and the nil account removed. Returns
/// whether anything changed.
pub fn normalize(folders: &mut BTreeMap<Uuid, Vec<Uuid>>) -> bool {
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
        ids.truncate(MAX_FOLDERS);
        changed |= ids.len() != before;
    }
    let before = folders.len();
    folders.retain(|_, ids| !ids.is_empty());
    changed || folders.len() != before
}

#[cfg(test)]
pub(super) mod tests;
