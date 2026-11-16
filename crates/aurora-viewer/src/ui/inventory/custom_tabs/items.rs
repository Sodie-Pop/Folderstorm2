//! Folderstorm: single items pinned to a custom Inventory tab (feature 2.13,
//! "Inventory quick access"; folderstorm/plans/inventory-quick-access.md).
//!
//! Beside its folders (`folders.rs`), a custom tab may pin items: an outfit
//! piece, a gesture, a landmark, an object, without its folder. They show at
//! the top of the tab's list, with its filters and search applied to them
//! too. Like the folders they are kept by id and by account; one deleted or
//! in the Trash is skipped but kept, and comes back if restored.
//!
//! A link pins what it points to (a folder link adds its folder to the
//! scope), so the pin survives the link being removed, an outfit edited.
//!
//! Plain logic, tested without a window.

use super::folders::{self, Change, Chosen, Status, TooMany};
use super::*;

/// The most items one tab pins on one account.
pub const MAX_ITEMS: usize = 64;

/// What a selected entry gives a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Folder(Uuid),
    Item(Uuid),
}

/// What `id` gives a tab: a folder, or an item (a link: what it points to,
/// a folder for a folder link). None for an unknown id or a broken link.
pub fn resolve(inv: &Inventory, id: Uuid) -> Option<Target> {
    if inv.folders.contains_key(&id) {
        return Some(Target::Folder(id));
    }
    inv.items.get(&id)?;
    let original = rules::original(inv, id)?;
    if inv.folders.contains_key(&original) {
        Some(Target::Folder(original))
    } else if inv.items.contains_key(&original) {
        Some(Target::Item(original))
    } else {
        None
    }
}

/// Where a pinned item stands, by the folders' rule (`folders::status`): in
/// the agent's inventory and out of the Trash, never the Library.
pub fn status(inv: &Inventory, id: Uuid) -> Status {
    let Some(item) = inv.items.get(&id) else {
        return Status::Missing;
    };
    if rules::library(inv, id) || (!inv.lib_root.is_nil() && rules::under(inv, item.parent, inv.lib_root)) {
        return Status::Refused;
    }
    if rules::system(inv, 14).is_some_and(|trash| rules::under(inv, item.parent, trash)) {
        return Status::Trash;
    }
    if inv.root.is_nil() || !rules::under(inv, item.parent, inv.root) {
        return Status::Missing;
    }
    Status::Live
}

/// The tab's pinned items on this account.
pub fn of(tab: &CustomTab, account: Uuid) -> &[Uuid] {
    tab.items.get(&account).map_or(&[], Vec::as_slice)
}

/// Sets the tab's pinned items on this account (none: the entry goes).
pub fn set(tab: &mut CustomTab, account: Uuid, items: Vec<Uuid>) {
    if items.is_empty() {
        tab.items.remove(&account);
    } else {
        tab.items.insert(account, items);
    }
}

/// The pinned items the search shows for this tab on this account: the
/// live ones.
pub fn search_pins(inv: &Inventory, tab: &CustomTab, account: Uuid) -> Vec<Uuid> {
    of(tab, account)
        .iter()
        .copied()
        .filter(|id| status(inv, *id) == Status::Live)
        .collect()
}

/// Which list would go over its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Folders,
    Items,
}

/// What adding a selection to a tab did, folders and items together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Added {
    /// Entries the tab did not have.
    pub new: usize,
    /// Entries it had already (not repeated).
    pub already: usize,
}

/// The tab's folders and pinned items once `chosen` joins them (or
/// replaces them, or none: `how`), checked against both limits first.
pub fn apply(tab: &mut CustomTab, account: Uuid, chosen: &Chosen, how: Change) -> Result<Added, Limit> {
    let current_folders = folders::of(tab, account).to_vec();
    let current_items = of(tab, account).to_vec();
    let new_folders = folders::change(&current_folders, &chosen.folders, how).map_err(|TooMany| Limit::Folders)?;
    let new_items = folders::change_within(&current_items, &chosen.items, how, MAX_ITEMS).map_err(|TooMany| Limit::Items)?;
    let already = if how == Change::Add {
        chosen.folders.iter().filter(|id| current_folders.contains(id)).count()
            + chosen.items.iter().filter(|id| current_items.contains(id)).count()
    } else {
        0
    };
    let new = (new_folders.len() + new_items.len()).saturating_sub(if how == Change::Add {
        current_folders.len() + current_items.len()
    } else {
        0
    });
    folders::set(tab, account, new_folders);
    set(tab, account, new_items);
    Ok(Added { new, already })
}

/// What adding `chosen` would do, without changing the tab.
pub fn would_add(tab: &CustomTab, account: Uuid, chosen: &Chosen) -> Result<Added, Limit> {
    let mut copy = tab.clone();
    apply(&mut copy, account, chosen, Change::Add)
}

#[cfg(test)]
mod tests;
