//! The gesture boards' tests: the store, the layout and the search (pure),
//! what a tile plays, and the window drawn frame by frame
//! (`tests/frames.rs`).

mod folders;
mod frames;

use super::layout::{self, GAP};
use super::store::{self, Account, Error, MAX_BOARDS, MAX_TILES, Store};
use super::{Item, Kind, resolve};
use crate::world::inventory::Inventory;
use aurora_net::inventory::InvItem;
use egui::{Vec2, vec2};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn account_with(items: &[u128]) -> (Account, u32) {
    let mut account = Account::default();
    let board = account.new_board("Greetings").expect("a board");
    let items: Vec<_> = items.iter().map(|n| id(*n)).collect();
    account.add_items(board, &items, None).expect("added");
    (account, board)
}

fn items_of(account: &Account, board: u32) -> Vec<u128> {
    account
        .board(board)
        .expect("board")
        .tiles
        .iter()
        .map(|t| t.item.as_u128())
        .collect()
}

fn order(board: &store::Board) -> Vec<u128> {
    board.tiles.iter().map(|t| t.item.as_u128()).collect()
}

// ---- store -------------------------------------------------------------

#[test]
fn a_new_board_is_selected_and_named_cleanly() {
    let mut account = Account::default();
    assert_eq!(account.new_board("   "), Err(Error::EmptyName));
    let first = account.new_board("  Driving\n ").expect("made");
    assert_eq!(account.board(first).map(|b| b.name.as_str()), Some("Driving"));
    assert_eq!(account.current_id(), Some(first));
    let second = account.new_board(&"x".repeat(200)).expect("made");
    assert_ne!(first, second);
    assert_eq!(account.current_id(), Some(second));
    assert_eq!(account.board(second).map(|b| b.name.chars().count()), Some(store::MAX_NAME));
    // a selection that is gone falls back on the first board
    account.selected = Some(999);
    assert_eq!(account.current_id(), Some(first));
}

#[test]
fn boards_are_limited() {
    let mut account = Account::default();
    for n in 0..MAX_BOARDS {
        account.new_board(&format!("Board {n}")).expect("made");
    }
    assert_eq!(account.new_board("One more"), Err(Error::TooManyBoards));
    let first = account.boards[0].id;
    assert_eq!(account.duplicate_board(first, "Copy"), Err(Error::TooManyBoards));
}

#[test]
fn an_item_is_added_once_per_board() {
    let (mut account, board) = account_with(&[1, 2]);
    let added = account.add_items(board, &[2, 3, 3, 1].map(id), None).expect("added");
    assert_eq!(added.added.len(), 1, "only the new item");
    assert_eq!(added.existing.len(), 2, "the two already there, once each");
    assert_eq!(items_of(&account, board), [1, 2, 3]);
    // at a place: before the tile there
    account.add_items(board, &[id(9)], Some(1)).expect("added");
    assert_eq!(items_of(&account, board), [1, 9, 2, 3]);
    // the same item on another board is fine
    let other = account.new_board("Party").expect("made");
    account.add_items(other, &[id(1)], None).expect("added");
    assert_eq!(items_of(&account, other), [1]);
    // tile ids are unique in the account
    let mut ids: Vec<u32> = account.boards.iter().flat_map(|b| b.tiles.iter().map(|t| t.id)).collect();
    let all = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all);
    // the nil id is never added
    assert!(account.add_items(other, &[Uuid::nil()], None).expect("nothing").added.is_empty());
}

#[test]
fn a_full_board_refuses_more() {
    let items: Vec<u128> = (1..=MAX_TILES as u128).collect();
    let (mut account, board) = account_with(&items);
    assert_eq!(account.board(board).map(|b| b.tiles.len()), Some(MAX_TILES));
    assert_eq!(account.add_items(board, &[id(5000)], None), Err(Error::TooManyTiles));
    // an item already there is still found
    let added = account.add_items(board, &[id(1), id(5001)], None).expect("one found");
    assert_eq!((added.existing.len(), added.refused), (1, 1));
}

#[test]
fn tiles_move_before_the_tile_dropped_on() {
    let (mut account, board_id) = account_with(&[1, 2, 3, 4]);
    let board = account.board_mut(board_id).expect("board");
    assert!(board.move_tile(0, 2), "1 before 3");
    assert_eq!(order(board), [2, 1, 3, 4]);
    assert!(board.move_tile(3, 0), "4 first");
    assert_eq!(order(board), [4, 2, 1, 3]);
    assert!(board.move_tile(0, 4), "4 last");
    assert_eq!(order(board), [2, 1, 3, 4]);
    // onto itself or right after itself: nothing moves
    assert!(!board.move_tile(1, 1) && !board.move_tile(1, 2) && !board.move_tile(9, 0) && !board.move_tile(0, 9));
}

#[test]
fn a_copy_keeps_the_tiles_with_new_ids() {
    let (mut account, board) = account_with(&[1, 2]);
    {
        let b = account.board_mut(board).expect("board");
        b.tiles[0].label = Some("Hi".into());
        b.tiles[0].color = Some([1, 2, 3]);
        b.tiles[1].size = Some([200.0, 50.0]);
        b.full_names = true;
    }
    let copy = account.duplicate_board(board, "Greetings 2").expect("copied");
    assert_eq!(account.current_id(), Some(copy));
    assert_eq!(
        account.boards.iter().map(|b| b.id).collect::<Vec<_>>(),
        [board, copy],
        "right after it"
    );
    let (a, b) = (account.board(board).expect("a"), account.board(copy).expect("b"));
    assert!(b.full_names);
    for (x, y) in a.tiles.iter().zip(&b.tiles) {
        assert_ne!(x.id, y.id);
        assert_eq!((x.item, &x.label, x.color, x.size), (y.item, &y.label, y.color, y.size));
    }
}

#[test]
fn renaming_keeps_the_board_and_deleting_shows_the_next() {
    let mut account = Account::default();
    let a = account.new_board("A").expect("a");
    let b = account.new_board("B").expect("b");
    let c = account.new_board("C").expect("c");
    account.selected = Some(b);
    account.rename_board(b, "Bee").expect("renamed");
    assert_eq!(account.current().map(|x| (x.id, x.name.as_str())), Some((b, "Bee")));
    assert_eq!(account.rename_board(b, " "), Err(Error::EmptyName));
    assert_eq!(account.rename_board(77, "X"), Err(Error::Missing));
    assert!(account.move_board(c, -1));
    assert_eq!(account.boards.iter().map(|x| x.id).collect::<Vec<_>>(), [a, c, b]);
    account.delete_board(b).expect("deleted");
    assert_eq!(account.current_id(), Some(c), "the one before it, the last one");
    account.delete_board(c).expect("deleted");
    assert_eq!(account.current_id(), Some(a), "the last one left");
    assert!(!account.move_board(a, 1) && !account.move_board(a, -1));
}

#[test]
fn every_tile_can_take_the_board_size_again() {
    let (mut account, board) = account_with(&[1, 2, 3]);
    let b = account.board_mut(board).expect("board");
    b.tiles[1].size = Some([300.0, 60.0]);
    assert_eq!(b.tiles[1].size_on(b), [300.0, 60.0]);
    assert_eq!(
        b.tiles[0].size_on(b),
        store::DEFAULT_TILE,
        "by default every tile has the board's size"
    );
    assert!(b.same_size_for_all());
    assert!(!b.same_size_for_all(), "nothing left to change");
    b.tile = [90.0, 40.0];
    assert!(b.tiles.iter().all(|t| t.size_on(b) == [90.0, 40.0]), "all at once");
}

#[test]
fn the_store_reads_back_what_it_wrote() {
    let (mut account, board) = account_with(&[1, 2]);
    account.board_mut(board).expect("board").tiles[0].label = Some("Wave".into());
    account.search_current = true;
    let mut store = Store::default();
    store.accounts.insert(id(42), account);
    let text = serde_json::to_string(&store).expect("written");
    assert!(!text.contains("asset"), "no asset id is written: {text}");
    let back: Store = serde_json::from_str(&text).expect("read");
    assert_eq!(back, store);
    // and an empty store is no board
    assert_eq!(serde_json::from_str::<Store>("{}").expect("read"), Store::default());
}

#[test]
fn what_cannot_be_read_is_kept_as_it_was() {
    let text = r#"{"accounts":{"00000000-0000-0000-0000-00000000002a":{
        "boards":[
            {"id":1,"name":"Good","tile":[9999,-5],"per_page":1,"tiles":[
                {"id":1,"item":"00000000-0000-0000-0000-000000000001","label":"  Hi\n","size":[1e9,10]},
                {"id":2,"item":"not an id"},
                {"id":3,"item":"00000000-0000-0000-0000-000000000001"},
                {"id":1,"item":"00000000-0000-0000-0000-000000000002"}
            ]},
            {"id":1,"name":"Same id"},
            {"id":2,"name":"   "},
            {"future":"board"}
        ],
        "selected":1}}}"#;
    let store: Store = serde_json::from_str(text).expect("read never fails");
    let account = &store.accounts[&id(42)];
    assert_eq!(account.boards.len(), 1);
    assert_eq!(
        account.kept.len(),
        3,
        "the board with the same id, the unnamed one and the unknown one"
    );
    let board = &account.boards[0];
    assert_eq!(
        board.tile,
        [*store::WIDTHS.end(), *store::HEIGHTS.start()],
        "sizes brought into range"
    );
    assert_eq!(board.per_page, *store::PER_PAGE.start());
    assert_eq!(board.tiles.len(), 1);
    assert_eq!(board.tiles[0].label.as_deref(), Some("Hi"));
    assert_eq!(board.tiles[0].size, Some([*store::WIDTHS.end(), *store::HEIGHTS.start()]));
    assert_eq!(board.kept.len(), 3, "a bad id, the same item again, the same tile id again");
    // written back: the kept entries are there, as they were
    let again = serde_json::to_value(&store).expect("written");
    let account = &again["accounts"]["00000000-0000-0000-0000-00000000002a"];
    assert!(account["kept"].as_array().is_some_and(|k| k.iter().any(|b| b["future"] == "board")));
    assert!(
        account["boards"][0]["kept"]
            .as_array()
            .is_some_and(|k| k.iter().any(|t| t["item"] == "not an id"))
    );
}

// ---- layout ------------------------------------------------------------

fn sizes(list: &[(f32, f32)]) -> Vec<Vec2> {
    list.iter().map(|(w, h)| vec2(*w, *h)).collect()
}

#[test]
fn tiles_fill_rows_then_pages() {
    // 3 tiles of 100 a row in 320 (100 + 4 + 100 + 4 + 100 = 308), 2 rows
    // of 30 in 70 (30 + 4 + 30 = 64)
    let pages = layout::pages(&sizes(&[(100.0, 30.0); 14]), vec2(320.0, 70.0), 0);
    assert_eq!(pages.len(), 3);
    assert_eq!(pages.iter().map(|p| p.tiles.len()).collect::<Vec<_>>(), [6, 6, 2]);
    assert_eq!(pages[0].span(), 0..6);
    assert_eq!(pages[1].span(), 6..12);
    let corners: Vec<_> = pages[0].tiles.iter().map(|(_, r)| (r.left(), r.top())).collect();
    let row = 30.0 + GAP;
    assert_eq!(
        corners,
        [(0.0, 0.0), (104.0, 0.0), (208.0, 0.0), (0.0, row), (104.0, row), (208.0, row)]
    );
    assert_eq!(layout::page_of(&pages, 7), 1);
    assert_eq!(layout::page_of(&pages, 99), 2, "past the end: the last page");
    // a cap on the page
    let capped = layout::pages(&sizes(&[(100.0, 30.0); 14]), vec2(320.0, 70.0), 4);
    assert_eq!(capped.iter().map(|p| p.tiles.len()).collect::<Vec<_>>(), [4, 4, 4, 2]);
    // nothing: one empty page
    assert_eq!(layout::pages(&[], vec2(100.0, 100.0), 0).len(), 1);
}

#[test]
fn mixed_sizes_pack_in_order_and_big_tiles_are_cut_to_the_room() {
    let list = [(100.0, 30.0), (150.0, 60.0), (100.0, 30.0), (900.0, 900.0)];
    let pages = layout::pages(&sizes(&list), vec2(300.0, 200.0), 0);
    let first = &pages[0].tiles;
    // the second tile fits beside the first, the third goes on a row
    // under the tallest of the first row
    assert_eq!(first[1].1.left(), 104.0);
    assert_eq!(first[2].1.top(), 60.0 + GAP);
    // the huge one is drawn at the room's size, on a page of its own
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1].tiles[0].1.size(), vec2(300.0, 200.0));
}

#[test]
fn full_names_widen_a_tile_up_to_the_board() {
    let mut board = store::Board::new(1, "B");
    let size = board.tile;
    // off: the board's size, whatever the name
    assert_eq!(layout::tile_width(&board, size, 400.0, false, 300.0), size[0]);
    board.full_names = true;
    // on: as wide as the label and its margins…
    let w = layout::tile_width(&board, size, 150.0, false, 300.0);
    assert_eq!(w, 150.0 + 2.0 * layout::PAD);
    // …and the speaker of a sound…
    assert_eq!(layout::tile_width(&board, size, 150.0, true, 300.0), w + layout::SPEAKER);
    // …never narrower than its size, never wider than the board
    assert_eq!(layout::tile_width(&board, size, 10.0, false, 300.0), size[0]);
    assert_eq!(layout::tile_width(&board, size, 900.0, false, 300.0), 300.0);
}

#[test]
fn the_search_looks_at_labels_and_names_on_all_boards_or_one() {
    let (mut account, greetings) = account_with(&[1, 2, 3]);
    account.board_mut(greetings).expect("board").tiles[2].label = Some("Big WAVE".into());
    let party = account.new_board("Party").expect("made");
    account.add_items(party, &[id(4), id(1)], None).expect("added");
    let names = |item: Uuid| match item.as_u128() {
        1 => Some("Wave hello".to_owned()),
        2 => Some("Bow".to_owned()),
        3 => Some("Clap".to_owned()),
        4 => Some("Wave goodbye".to_owned()),
        _ => None,
    };
    let found = |query: &str, current: u32, only: bool| -> Vec<(u32, u128)> {
        layout::search(&account, query, Some(current), only, names)
            .into_iter()
            .map(|h| {
                let item = account.board(h.board).and_then(|b| b.tile(h.tile)).map_or(0, |t| t.item.as_u128());
                (h.board, item)
            })
            .collect()
    };
    // all boards, the current one first, letter case aside
    assert_eq!(
        found("wave", party, false),
        [(party, 4), (party, 1), (greetings, 1), (greetings, 3)]
    );
    assert_eq!(
        found("wave", greetings, false),
        [(greetings, 1), (greetings, 3), (party, 4), (party, 1)]
    );
    // the current board only
    assert_eq!(found("WAVE", greetings, true), [(greetings, 1), (greetings, 3)]);
    // a label is searched, and so is the item's name under it
    assert_eq!(found("clap", greetings, true), [(greetings, 3)]);
    assert_eq!(found("big", greetings, false), [(greetings, 3)]);
    // nothing typed finds nothing
    assert!(found("  ", greetings, false).is_empty());
}

// ---- what a tile plays -------------------------------------------------

pub(super) fn item(n: u128, parent: Uuid, name: &str, asset_type: i32, asset: Uuid) -> InvItem {
    InvItem {
        id: id(n),
        parent,
        name: name.into(),
        asset_type,
        inv_type: match asset_type {
            21 => 20,
            1 => 1,
            _ => 6,
        },
        asset_id: asset,
        owner: id(42),
        base_mask: 0x7fff_ffff,
        owner_mask: 0x7fff_ffff,
        ..Default::default()
    }
}

/// An inventory with a gesture, a sound, a link to the gesture, an object,
/// a gesture in the Trash and a sound of the Library.
pub(super) fn inventory() -> Inventory {
    let mut inv = Inventory::default();
    crate::world::inventory::demo::seed(&mut inv, id(42));
    let mine = id(8000);
    // the demo's own Trash (a second one would make the Trash ambiguous)
    let trash = crate::world::appearance::system_folder(&inv, 14).expect("a Trash");
    let library = inv.lib_root;
    inv.add_items(vec![
        item(100, mine, "Wave hello", 21, id(1100)),
        item(101, mine, "Air horn", 1, id(1101)),
        item(102, mine, "Wave (link)", 24, id(100)),
        item(103, mine, "Box", 6, id(1103)),
        item(104, trash, "Old bow", 21, id(1104)),
        item(105, library, "Library chime", 1, id(1105)),
    ]);
    inv
}

#[test]
fn a_tile_plays_a_gesture_or_a_sound_of_the_users_own_inventory() {
    let inv = inventory();
    let ready = |n: u128| match resolve(&inv, id(n)) {
        Item::Ready { original, kind, name } => Some((original.as_u128(), kind, name)),
        _ => None,
    };
    assert_eq!(ready(100), Some((100, Kind::Gesture, "Wave hello".into())));
    assert_eq!(ready(101), Some((101, Kind::Sound, "Air horn".into())));
    // a link plays its original, under the link's name
    assert_eq!(ready(102), Some((100, Kind::Gesture, "Wave (link)".into())));
    assert_eq!(resolve(&inv, id(103)), Item::Other);
    assert_eq!(resolve(&inv, id(104)), Item::Trash);
    assert_eq!(resolve(&inv, id(105)), Item::Library);
    assert_eq!(resolve(&inv, id(999)), Item::Missing);
    for missing in [Item::Missing, Item::Trash, Item::Library, Item::Other] {
        assert!(missing.why().is_some());
    }
}

#[test]
fn the_tile_text_reads_on_its_colour() {
    let p = crate::theme::Theme::default().skin_palette();
    for fill in [p.violet, p.amber, p.panel, p.ink, p.success] {
        let ink = super::window::ink_on(&p, fill);
        let candidates = [p.ink, p.on_accent, p.navy, p.panel];
        assert!(candidates.iter().any(|c| c.to_opaque() == ink), "a colour of the palette");
        assert_ne!(ink, fill.to_opaque(), "never the fill itself");
    }
}

#[test]
fn the_toolbar_button_opens_and_closes_the_window() {
    let ctx = egui::Context::default();
    assert!(!super::is_open(&ctx));
    super::run(&ctx, super::COMMAND);
    assert!(super::is_open(&ctx));
    assert_eq!(
        super::look(&ctx, super::COMMAND, false, true, "tip".into()),
        (true, true, "tip".into())
    );
    assert_eq!(
        super::look(&ctx, "inventory", false, true, "tip".into()),
        (false, true, "tip".into()),
        "others as they came"
    );
    super::run(&ctx, "inventory");
    assert!(super::is_open(&ctx), "another command leaves it");
    super::run(&ctx, super::COMMAND);
    assert!(!super::is_open(&ctx));
    let command = crate::ui::toolbar::COMMANDS.iter().find(|c| c.id == super::COMMAND);
    assert!(command.is_some_and(|c| c.ready && c.icon == super::ICON));
    assert!(crate::ui::icons::embedded(super::ICON) && crate::ui::icons::embedded(super::SOUND_ICON));
}
