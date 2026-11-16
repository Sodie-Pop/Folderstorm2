//! Whole folders dropped on a board (`folders.rs`): the walk, its order,
//! what it leaves out, the board's limit and what the board says.

use super::super::folders::{self, Found, MAX_FOLDERS};
use super::super::store::{Account, MAX_TILES};
use super::{id, inventory, item};
use crate::world::inventory::{FetchState, Folder, Inventory};
use aurora_i18n::tr;
use aurora_net::inventory::InvFolder;
use uuid::Uuid;

/// The folder dropped in most tests.
pub(super) const DROPPED: u128 = 607000;
/// Its sub-folder "Mid", which a link of the dropped folder points to.
const MID: u128 = 607100;
/// A sub-folder not loaded yet.
const UNLOADED: u128 = 607300;

pub(super) fn folder(inv: &mut Inventory, n: u128, parent: Uuid, name: &str, state: FetchState) {
    let library = inv.folders.get(&parent).is_some_and(|f| f.library);
    inv.folders.insert(
        id(n),
        Folder {
            info: InvFolder {
                id: id(n),
                parent,
                name: name.into(),
                type_default: -1,
                version: 1,
                ..Default::default()
            },
            children: Vec::new(),
            items: Vec::new(),
            state,
            library,
        },
    );
    if let Some(f) = inv.folders.get_mut(&parent) {
        f.children.push(id(n));
    }
    inv.generation += 1;
}

/// The test inventory (`super::inventory`) with a folder "Board stuff" in
/// the user's test folder:
///
/// ```text
/// Board stuff/        b gesture, a sound, Box (object), Wave link (→ Wave hello),
///                     Broken (a gesture's broken link), Folder link (→ Mid)
///   Zed/              z1 (gesture)
///   Mid/              m1 (sound)
///     Deep/           d1 (gesture)
/// ```
pub(super) fn with_folders() -> Inventory {
    let mut inv = inventory();
    add_board_stuff(&mut inv);
    inv
}

/// Adds "Board stuff" (see [`with_folders`]) to `inv`.
pub(super) fn add_board_stuff(inv: &mut Inventory) {
    let mine = id(8000);
    folder(inv, DROPPED, mine, "Board stuff", FetchState::Fetched);
    folder(inv, 607200, id(DROPPED), "Zed", FetchState::Fetched);
    folder(inv, MID, id(DROPPED), "Mid", FetchState::Fetched);
    folder(inv, 607110, id(MID), "Deep", FetchState::Fetched);
    let mut broken = item(610705, id(DROPPED), "Broken", 24, id(9999));
    broken.inv_type = 20;
    let mut folder_link = item(610706, id(DROPPED), "Folder link", 25, id(MID));
    folder_link.inv_type = 8;
    inv.add_items(vec![
        item(610701, id(DROPPED), "b gesture", 21, id(1701)),
        item(610702, id(DROPPED), "a sound", 1, id(1702)),
        item(610703, id(DROPPED), "Box", 6, id(1703)),
        item(610704, id(DROPPED), "Wave link", 24, id(100)),
        broken,
        folder_link,
        item(610721, id(607200), "z1", 21, id(1721)),
        item(610711, id(MID), "m1", 1, id(1711)),
        item(617111, id(607110), "d1", 21, id(17111)),
    ]);
}

fn items(found: &Found) -> Vec<u128> {
    found.items.iter().map(|(item, _)| item.as_u128()).collect()
}

fn tiles(account: &Account, board: u32) -> Vec<u128> {
    account
        .board(board)
        .expect("board")
        .tiles
        .iter()
        .map(|t| t.item.as_u128())
        .collect()
}

#[test]
fn a_folder_gives_its_gestures_and_sounds_depth_first_by_name() {
    let inv = with_folders();
    let found = folders::collect(&inv, &[id(DROPPED)]);
    // its own items by name, then "Mid" (and "Deep" in it), then "Zed"; a
    // link stays a link (it plays its original)
    assert_eq!(items(&found), [610702, 610701, 610704, 610711, 617111, 610721]);
    assert_eq!(found.items[2], (id(610704), id(100)), "the link and its original");
    assert_eq!(found.other, 2, "the object and the link to a folder");
    assert_eq!(found.refused, 1, "the broken link to a gesture");
    assert_eq!(found.twice, 0, "the folder link is not followed: Mid once");
    assert!(found.loading.is_empty());
    assert!(!found.cut);
    // the same, whatever order the inventory keeps its lists in
    let mut shuffled = with_folders();
    for f in shuffled.folders.values_mut() {
        f.items.reverse();
        f.children.reverse();
    }
    assert_eq!(folders::collect(&shuffled, &[id(DROPPED)]), found);
}

#[test]
fn entries_dropped_together_go_by_name_and_once_each() {
    let inv = with_folders();
    // "Board stuff" before "Wave hello": the link inside is found first, the
    // gesture it points to counts once
    let found = folders::collect(&inv, &[id(100), id(DROPPED), id(DROPPED)]);
    assert_eq!(items(&found), [610702, 610701, 610704, 610711, 617111, 610721]);
    assert_eq!(found.twice, 1);
    // a sub-folder dropped with its parent is walked once
    let found = folders::collect(&inv, &[id(MID), id(DROPPED)]);
    assert_eq!(found.twice, 0);
    assert_eq!(items(&found), [610702, 610701, 610704, 610711, 617111, 610721]);
    // a link to a folder dropped on its own: its folder
    let found = folders::collect(&inv, &[id(610706)]);
    assert_eq!(items(&found), [610711, 617111]);
}

#[test]
fn the_library_and_the_trash_give_nothing() {
    let inv = with_folders();
    let trash = crate::world::appearance::system_folder(&inv, 14).expect("a Trash");
    let found = folders::collect(&inv, &[trash]);
    assert!(found.items.is_empty());
    assert_eq!(found.refused, 1, "the gesture in the Trash");
    assert!(found.loading.is_empty(), "never fetched for nothing");
    let found = folders::collect(&inv, &[inv.lib_root]);
    assert!(found.items.is_empty());
    assert_eq!(found.refused, 1, "the Library's sound");
    assert!(found.loading.is_empty());
    // a folder holding the test folder and the Trash (the whole inventory,
    // whose root the test inventory has not): the Trash met on the way is
    // skipped
    let mut inv = inv;
    folder(&mut inv, 609000, Uuid::nil(), "Everything", FetchState::Fetched);
    for child in [id(8000), trash] {
        if let Some(f) = inv.folders.get_mut(&child) {
            f.info.parent = id(609000);
        }
        if let Some(f) = inv.folders.get_mut(&id(609000)) {
            f.children.push(child);
        }
    }
    let found = folders::collect(&inv, &[id(609000)]);
    let all: Vec<u128> = found.items.iter().map(|(_, original)| original.as_u128()).collect();
    assert!(all.contains(&100) && all.contains(&617111));
    assert!(!all.contains(&104), "nothing of the Trash");
    // what the outline says while it is carried
    assert!(folders::welcome(&inv, &[id(DROPPED)]));
    assert!(folders::welcome(&inv, &[id(103), id(100)]), "one gesture is enough");
    assert!(!folders::welcome(&inv, &[trash]));
    assert!(!folders::welcome(&inv, &[inv.lib_root]));
    assert!(!folders::welcome(&inv, &[id(103), id(104)]), "an object, a gesture in the Trash");
}

#[test]
fn a_folder_not_loaded_is_asked_for_and_what_is_known_is_added() {
    let mut inv = with_folders();
    folder(&mut inv, UNLOADED, id(DROPPED), "Unloaded", FetchState::Unknown);
    folder(&mut inv, 607310, id(UNLOADED), "Known below", FetchState::Fetched);
    inv.add_items(vec![item(617311, id(607310), "known", 21, id(17311))]);
    let found = folders::collect(&inv, &[id(DROPPED)]);
    assert_eq!(found.loading, [id(UNLOADED)]);
    assert!(items(&found).contains(&617311), "what is known below it is found");
    let mut account = Account::default();
    let board = account.new_board("Folder").expect("board");
    let outcome = folders::add_found(&inv, &mut account, board, &found, None).expect("added");
    assert_eq!(outcome.loading, 1);
    assert!(
        outcome.summary().contains(&tr!("fs-gesture-board-folder-loading", count = 1)),
        "{}",
        outcome.summary()
    );
}

#[test]
fn what_the_board_has_already_is_left_out() {
    let inv = with_folders();
    let mut account = Account::default();
    let board = account.new_board("Folder").expect("board");
    // the gesture "Wave hello" (the link inside plays it) and "z1"
    account.add_items(board, &[id(100), id(610721)], None).expect("added");
    let found = folders::collect(&inv, &[id(DROPPED)]);
    let outcome = folders::add_found(&inv, &mut account, board, &found, None).expect("added");
    assert_eq!(tiles(&account, board), [100, 610721, 610702, 610701, 610711, 617111]);
    assert_eq!(outcome.added.len(), 4);
    assert_eq!(outcome.already, 2);
    assert_eq!(outcome.existing.len(), 2);
    // dropped again: nothing new
    let outcome = folders::add_found(&inv, &mut account, board, &found, None).expect("added");
    assert!(outcome.added.is_empty());
    assert_eq!(outcome.already, 6);
    assert_eq!(tiles(&account, board).len(), 6);
    // at a place: in the walk's order, before the tile there
    let other = account.new_board("Other").expect("board");
    account.add_items(other, &[id(101)], None).expect("added");
    folders::add_found(&inv, &mut account, other, &folders::collect(&inv, &[id(MID)]), Some(0)).expect("added");
    assert_eq!(tiles(&account, other), [610711, 617111, 101]);
}

#[test]
fn a_full_board_takes_what_fits() {
    let inv = with_folders();
    let mut account = Account::default();
    let board = account.new_board("Full").expect("board");
    let fill: Vec<Uuid> = (0..MAX_TILES as u128 - 2).map(|n| id(500_000 + n)).collect();
    account.add_items(board, &fill, None).expect("added");
    let found = folders::collect(&inv, &[id(DROPPED)]);
    let outcome = folders::add_found(&inv, &mut account, board, &found, None).expect("added");
    assert_eq!(outcome.added.len(), 2, "the first two of the walk");
    assert_eq!(outcome.full, 4);
    let last: Vec<u128> = tiles(&account, board).into_iter().rev().take(2).collect();
    assert_eq!(last, [610701, 610702]);
    // full: nothing more, and said so
    let outcome = folders::add_found(&inv, &mut account, board, &folders::collect(&inv, &[id(MID)]), None).expect("said");
    assert_eq!(outcome.full, 2);
    assert!(outcome.added.is_empty());
    let summary = outcome.summary();
    assert!(summary.starts_with(&tr!("fs-gesture-board-folder-added", count = 0)), "{summary}");
    assert!(
        summary.contains(&tr!("fs-gesture-board-folder-full", count = 2, max = MAX_TILES)),
        "{summary}"
    );
}

#[test]
fn the_summary_says_what_was_added_and_left_out() {
    let inv = with_folders();
    let mut account = Account::default();
    let board = account.new_board("Folder").expect("board");
    let found = folders::collect(&inv, &[id(DROPPED)]);
    let summary = folders::add_found(&inv, &mut account, board, &found, None)
        .expect("added")
        .summary();
    let separator = tr!("fs-gesture-board-folder-separator");
    assert_eq!(
        summary,
        [
            tr!("fs-gesture-board-folder-added", count = 6),
            tr!("fs-gesture-board-folder-refused", count = 1),
            tr!("fs-gesture-board-folder-other", count = 2),
        ]
        .join(separator)
    );
    // a folder with neither gestures nor sounds
    let mut inv = with_folders();
    folder(&mut inv, 607400, id(8000), "Objects", FetchState::Fetched);
    inv.add_items(vec![item(617401, id(607400), "Box 2", 6, id(17401))]);
    let found = folders::collect(&inv, &[id(607400)]);
    let summary = folders::add_found(&inv, &mut account, board, &found, None)
        .expect("nothing")
        .summary();
    assert!(summary.starts_with(tr!("fs-gesture-board-folder-empty")), "{summary}");
}

#[test]
fn a_huge_tree_is_cut_short() {
    let mut inv = with_folders();
    folder(&mut inv, 800_000, id(8000), "Huge", FetchState::Fetched);
    for n in 0..MAX_FOLDERS as u128 + 10 {
        folder(&mut inv, 800_001 + n, id(800_000), &format!("Sub {n}"), FetchState::Fetched);
    }
    inv.add_items(vec![item(900_000, id(800_001), "first", 21, id(900_001))]);
    let found = folders::collect(&inv, &[id(800_000)]);
    assert!(found.cut);
    assert!(items(&found).contains(&900_000), "what was looked at is kept");
}
