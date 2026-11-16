//! Whole Inventory folders dropped on a board (folderstorm/plans/
//! gesture-boards.md, "Folders dropped on a board"): every gesture and
//! sound of the folders and of all their sub-folders goes on the board, in
//! a stable order, as far as the board has room. Pure: reads the
//! inventory, changes the account, and leaves the fetch of folders not
//! loaded yet to the app.
//!
//! What a folder gives follows the rule of one item dropped
//! ([`super::resolve`]): a gesture or a sound of the user's own inventory,
//! a link followed (the tile keeps the link's item id, as for one link
//! dropped); never one of the Library or of the Trash; a broken link is
//! left out. Links to folders are not followed (as the Legacy Viewer's
//! LLInventoryModel::collectDescendents), so a walk never loops nor walks a
//! folder twice.

use super::store::{Account, Error, MAX_TILES};
use super::{Item, resolve};
use crate::world::inventory::{FetchState, Inventory, actions as rules};
use std::collections::HashSet;
use uuid::Uuid;

/// A walk looks at this many folders at most.
pub const MAX_FOLDERS: usize = 5_000;
/// A walk looks at this many items at most (the board takes
/// [`MAX_TILES`] at most anyway).
pub const MAX_ITEMS: usize = 50_000;

/// What the folders (and items) dropped hold, in the order they go on the
/// board.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The gestures and sounds found (the item ids: a link stays a link),
    /// with their originals, once per original.
    pub items: Vec<(Uuid, Uuid)>,
    /// Gestures and sounds found a second time (the same original, or a
    /// link to one found already).
    pub twice: usize,
    /// Gestures and sounds a board refuses: of the Library, of the Trash,
    /// broken links.
    pub refused: usize,
    /// Items that are neither a gesture nor a sound (links to folders
    /// included): left out without a word, only counted.
    pub other: usize,
    /// Folders whose contents are not loaded yet: what they hold is not all
    /// known. Of the user's own inventory only (not the Library's, not the
    /// Trash's, whose items a board refuses anyway).
    pub loading: Vec<Uuid>,
    /// The walk stopped at [`MAX_FOLDERS`] or [`MAX_ITEMS`].
    pub cut: bool,
}

/// Whether `id` is a folder, or a link to one, as a drop carries it: its
/// folder.
pub fn folder_of(inv: &Inventory, id: Uuid) -> Option<Uuid> {
    rules::original(inv, id).filter(|f| inv.folders.contains_key(f))
}

/// Whether a drop of `cargo` can add anything, as the outline over the
/// board says while it is carried: one of its items is a gesture or a sound
/// the board takes, or one of its folders is of the user's own inventory
/// (what a folder holds is only looked at when it is dropped).
pub fn welcome(inv: &Inventory, cargo: &[Uuid]) -> bool {
    cargo.iter().take(256).any(|id| match folder_of(inv, *id) {
        Some(folder) => !folder_refused(inv, folder),
        None => matches!(resolve(inv, *id), Item::Ready { .. }),
    })
}

/// A folder whose items a board refuses all: of the Library, or the Trash
/// and what it holds.
fn folder_refused(inv: &Inventory, folder: Uuid) -> bool {
    rules::library(inv, folder) || rules::in_type(inv, folder, 14)
}

fn name_key(inv: &Inventory, id: Uuid) -> crate::natural_order::Key {
    let name = inv
        .folders
        .get(&id)
        .map(|f| f.info.name.as_str())
        .or_else(|| inv.items.get(&id).map(|it| it.name.as_str()))
        .unwrap_or_default();
    crate::natural_order::key(name)
}

/// Walks what is dropped (`cargo`: items and folders, as the Inventory
/// windows carry them) and lists the gestures and sounds it holds.
///
/// The order (stable, whatever order the inventory or the selection come
/// in): the entries dropped by name; in each folder, depth first, its own
/// items by name, then each of its sub-folders by name, with all it holds.
/// The Trash met inside a folder (the whole inventory dropped) is skipped.
pub fn collect(inv: &Inventory, cargo: &[Uuid]) -> Found {
    let mut found = Found::default();
    let mut seen_originals = HashSet::new();
    let mut seen_folders = HashSet::new();
    let mut items_looked_at = 0usize;
    let mut roots: Vec<Uuid> = cargo.to_vec();
    roots.sort_by_cached_key(|id| (name_key(inv, *id), *id));
    roots.dedup();
    for root in roots {
        let Some(folder) = folder_of(inv, root) else {
            items_looked_at += 1;
            take_item(inv, root, &mut found, &mut seen_originals);
            continue;
        };
        // (folder, refused: its items are of the Library or the Trash)
        let mut stack = vec![(folder, folder_refused(inv, folder))];
        while let Some((id, refused)) = stack.pop() {
            if !seen_folders.insert(id) {
                continue;
            }
            if seen_folders.len() > MAX_FOLDERS || items_looked_at >= MAX_ITEMS {
                found.cut = true;
                break;
            }
            let Some(f) = inv.folders.get(&id) else {
                continue;
            };
            if f.state != FetchState::Fetched && !refused && !f.library {
                found.loading.push(id);
            }
            let mut items: Vec<Uuid> = f
                .items
                .iter()
                .copied()
                .filter(|item| inv.items.get(item).is_some_and(|it| it.parent == id))
                .collect();
            items.sort_by_cached_key(|item| (name_key(inv, *item), *item));
            for item in items {
                if items_looked_at >= MAX_ITEMS {
                    found.cut = true;
                    break;
                }
                items_looked_at += 1;
                take_item(inv, item, &mut found, &mut seen_originals);
            }
            let mut children: Vec<Uuid> = f
                .children
                .iter()
                .copied()
                .filter(|child| {
                    inv.folders
                        .get(child)
                        .is_some_and(|c| c.info.parent == id && (refused || c.info.type_default != 14))
                })
                .collect();
            children.sort_by_cached_key(|child| (name_key(inv, *child), *child));
            // depth first, the first by name on top of the stack
            for child in children.into_iter().rev() {
                let library = inv.folders.get(&child).is_some_and(|c| c.library);
                stack.push((child, refused || library));
            }
        }
        if found.cut {
            break;
        }
    }
    found
}

/// One item of a folder (or dropped alone): a gesture or a sound the board
/// takes, or one it refuses, or something else.
fn take_item(inv: &Inventory, item: Uuid, found: &mut Found, seen: &mut HashSet<Uuid>) {
    let Some(it) = inv.items.get(&item) else {
        return;
    };
    // a link to a folder is not followed
    if it.asset_type == 25 {
        found.other += 1;
        return;
    }
    match resolve(inv, item) {
        Item::Ready { original, .. } => {
            if seen.insert(original) {
                found.items.push((item, original));
            } else {
                found.twice += 1;
            }
        }
        Item::Library | Item::Trash => found.refused += 1,
        // a broken link: refused when it was a gesture's or a sound's
        // (its inventory type tells), else another kind of item
        Item::Missing if matches!(it.inv_type, 1 | 20) => found.refused += 1,
        Item::Missing | Item::Other => found.other += 1,
    }
}

/// What a drop of folders did to the board.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The new tiles, in order.
    pub added: Vec<u32>,
    /// Tiles of the board that play what was found already.
    pub existing: Vec<u32>,
    /// Gestures and sounds the board had already (or found twice).
    pub already: usize,
    /// Left out: the board is full.
    pub full: usize,
    /// Refused: of the Library, of the Trash, broken links.
    pub refused: usize,
    /// Neither gestures nor sounds.
    pub other: usize,
    /// Folders still loading.
    pub loading: usize,
    /// The walk stopped early.
    pub cut: bool,
}

/// Adds what `found` holds to `board`, before the tile at `at` (None: at
/// the end), leaving out what the board has already: a tile of the same
/// item, or one that plays the same original (a link and its gesture).
pub fn add_found(inv: &Inventory, account: &mut Account, board: u32, found: &Found, at: Option<usize>) -> Result<Outcome, Error> {
    let tiles = &account.board(board).ok_or(Error::Missing)?.tiles;
    let mut on_board: std::collections::HashMap<Uuid, u32> = std::collections::HashMap::new();
    for tile in tiles {
        on_board.entry(tile.item).or_insert(tile.id);
        if let Item::Ready { original, .. } = resolve(inv, tile.item) {
            on_board.entry(original).or_insert(tile.id);
        }
    }
    let mut out = Outcome {
        already: found.twice,
        refused: found.refused,
        other: found.other,
        loading: found.loading.len(),
        cut: found.cut,
        ..Default::default()
    };
    let mut new = Vec::new();
    for (item, original) in &found.items {
        match on_board.get(item).or_else(|| on_board.get(original)) {
            Some(tile) => {
                out.already += 1;
                if !out.existing.contains(tile) {
                    out.existing.push(*tile);
                }
            }
            None => new.push(*item),
        }
    }
    match account.add_items(board, &new, at) {
        Ok(added) => {
            out.added = added.added;
            out.full = added.refused;
        }
        Err(Error::TooManyTiles) => out.full = new.len(),
        Err(error) => return Err(error),
    }
    Ok(out)
}

impl Outcome {
    /// What the board says about it: what was added, then what was left
    /// out and why.
    pub fn summary(&self) -> String {
        use aurora_i18n::tr;
        let mut parts = Vec::new();
        let nothing_found = self.added.is_empty() && self.already == 0 && self.full == 0 && self.refused == 0;
        if nothing_found && self.loading == 0 {
            parts.push(tr!("fs-gesture-board-folder-empty").to_owned());
        } else {
            parts.push(tr!("fs-gesture-board-folder-added", count = self.added.len()));
        }
        if self.already > 0 {
            parts.push(tr!("fs-gesture-board-folder-already", count = self.already));
        }
        if self.full > 0 {
            parts.push(tr!("fs-gesture-board-folder-full", count = self.full, max = MAX_TILES));
        }
        if self.refused > 0 {
            parts.push(tr!("fs-gesture-board-folder-refused", count = self.refused));
        }
        if self.other > 0 {
            parts.push(tr!("fs-gesture-board-folder-other", count = self.other));
        }
        if self.loading > 0 {
            parts.push(tr!("fs-gesture-board-folder-loading", count = self.loading));
        }
        if self.cut {
            parts.push(tr!("fs-gesture-board-folder-cut").to_owned());
        }
        parts.join(tr!("fs-gesture-board-folder-separator"))
    }
}
