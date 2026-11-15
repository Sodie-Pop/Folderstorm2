use super::super::folders::tests::{BODY, BOOTS, CLOTHING, DELETED, OTHER, SHIRT, account, id, inventory};
use super::*;
use crate::ui::inventory::view::{TreeState, ViewCache};
use aurora_net::inventory::InvItem;

const SKIN: u128 = 120;
const UNRELATED: u128 = 130;
const OLD_HAT: u128 = 141;
const LIB_SHIRT: u128 = 160;
const SHIRT_LINK: u128 = 170;
const BODY_LINK: u128 = 171;
const BROKEN_LINK: u128 = 172;

/// The test inventory, with a link to the Shirt and one to the Body folder
/// in Other, and a link to nothing.
fn linked() -> Inventory {
    let mut inv = inventory();
    let link = |n: u128, kind: i32, to: u128, name: &str| InvItem {
        id: id(n),
        parent: id(OTHER),
        name: name.into(),
        asset_type: kind,
        inv_type: 18,
        asset_id: id(to),
        owner: account(),
        ..Default::default()
    };
    inv.add_items(vec![
        link(SHIRT_LINK, 24, SHIRT, "Shirt link"),
        link(BODY_LINK, 25, BODY, "Body link"),
        link(BROKEN_LINK, 24, 4242, "Broken link"),
    ]);
    inv.generation += 1;
    inv
}

fn tab() -> CustomTab {
    CustomTab {
        id: 1,
        name: "Quick".into(),
        ..Default::default()
    }
}

fn chosen(folders: &[u128], items: &[u128]) -> Chosen {
    Chosen {
        folders: folders.iter().map(|n| id(*n)).collect(),
        items: items.iter().map(|n| id(*n)).collect(),
    }
}

/// The rows of the tab's list, in order, by name (folders and items).
fn rows(inv: &Inventory, tab: &CustomTab, text: &str, filters: &Filters, flat: bool) -> Vec<String> {
    let facts = Facts {
        agent: account(),
        ..Default::default()
    };
    let prefs = InventoryPreferences::default();
    let mut view = ViewCache::default();
    view.set_tab_scope(folders::search_roots(inv, tab, account()), search_pins(inv, tab, account()), flat);
    view.search(
        inv,
        inv.root,
        text,
        &prefs,
        filters,
        CUSTOM_TAB,
        None,
        &facts,
        account(),
        view::now(),
    );
    let mut tree = TreeState::default();
    tree.prepare(&view, inv.root, CUSTOM_TAB, false);
    tree.rows.iter().map(|row| rules::name(inv, row.id)).collect()
}

// ---- which items ----

#[test]
fn a_link_gives_what_it_points_to() {
    let inv = linked();
    assert_eq!(resolve(&inv, id(SHIRT)), Some(Target::Item(id(SHIRT))));
    assert_eq!(resolve(&inv, id(CLOTHING)), Some(Target::Folder(id(CLOTHING))));
    assert_eq!(
        resolve(&inv, id(SHIRT_LINK)),
        Some(Target::Item(id(SHIRT))),
        "the item, not the link"
    );
    assert_eq!(
        resolve(&inv, id(BODY_LINK)),
        Some(Target::Folder(id(BODY))),
        "a folder link: its folder"
    );
    assert_eq!(resolve(&inv, id(BROKEN_LINK)), None);
    assert_eq!(resolve(&inv, id(DELETED)), None);
    // a selection: the link and its original are one pin
    let picked = folders::check(&inv, &[id(SHIRT_LINK), id(SHIRT), id(BODY_LINK)]).expect("allowed");
    assert_eq!(picked, chosen(&[BODY], &[SHIRT]));
    assert_eq!(folders::check(&inv, &[id(SHIRT), id(BROKEN_LINK)]), Err(folders::Refusal::Broken));
}

#[test]
fn an_item_is_live_only_in_the_agents_inventory_out_of_the_trash() {
    let mut inv = inventory();
    assert_eq!(status(&inv, id(SHIRT)), Status::Live);
    assert_eq!(status(&inv, id(112)), Status::Live, "deep in a folder");
    assert_eq!(status(&inv, id(OLD_HAT)), Status::Trash);
    assert_eq!(status(&inv, id(LIB_SHIRT)), Status::Refused, "the Library");
    assert_eq!(status(&inv, id(DELETED)), Status::Missing);
    if let Some(it) = inv.items.get_mut(&id(UNRELATED)) {
        it.parent = id(DELETED);
    }
    assert_eq!(status(&inv, id(UNRELATED)), Status::Missing, "a folder not loaded");
}

// ---- the tab's lists ----

#[test]
fn adding_pins_items_and_scopes_folders_once() {
    let mut tab = tab();
    let added = apply(&mut tab, account(), &chosen(&[CLOTHING], &[SKIN]), Change::Add).expect("room");
    assert_eq!(added, Added { new: 2, already: 0 });
    assert_eq!(folders::of(&tab, account()), [id(CLOTHING)]);
    assert_eq!(of(&tab, account()), [id(SKIN)]);
    let again = chosen(&[CLOTHING], &[SKIN, UNRELATED]);
    assert_eq!(would_add(&tab, account(), &again), Ok(Added { new: 1, already: 2 }));
    assert_eq!(of(&tab, account()), [id(SKIN)], "would_add changes nothing");
    apply(&mut tab, account(), &again, Change::Add).expect("room");
    assert_eq!(of(&tab, account()), [id(SKIN), id(UNRELATED)]);
    assert!(folders::scoped(&tab, account()));
    apply(&mut tab, account(), &chosen(&[], &[SHIRT]), Change::Replace).expect("room");
    assert!(folders::of(&tab, account()).is_empty(), "replaced: the selection only");
    assert_eq!(of(&tab, account()), [id(SHIRT)]);
    assert!(folders::scoped(&tab, account()), "pins alone scope the tab");
    apply(&mut tab, account(), &Chosen::default(), Change::Clear).expect("clear");
    assert!(!folders::scoped(&tab, account()) && tab.items.is_empty() && tab.folders.is_empty());
}

#[test]
fn a_full_tab_refuses_more_items_and_changes_nothing() {
    let mut tab = tab();
    let full: Vec<Uuid> = (0..MAX_ITEMS as u128).map(|n| id(5000 + n)).collect();
    set(&mut tab, account(), full.clone());
    let more = chosen(&[CLOTHING], &[SKIN]);
    assert_eq!(apply(&mut tab, account(), &more, Change::Add), Err(Limit::Items));
    assert!(folders::of(&tab, account()).is_empty(), "the folder did not join either");
    assert_eq!(of(&tab, account()), full.as_slice());
    let folders_full: Vec<Uuid> = (0..folders::MAX_FOLDERS as u128).map(|n| id(6000 + n)).collect();
    folders::set(&mut tab, account(), folders_full);
    assert_eq!(apply(&mut tab, account(), &chosen(&[BODY], &[]), Change::Add), Err(Limit::Folders));
}

#[test]
fn pins_belong_to_one_account_and_round_trip_through_the_settings() {
    let mut tab = tab();
    set(&mut tab, account(), vec![id(SKIN)]);
    assert!(of(&tab, id(8)).is_empty());
    assert!(folders::elsewhere(&tab, id(8)), "pinned on another account only");
    tab.flat = true;
    let mut prefs = InventoryPreferences::default();
    prefs.custom_tabs.push(tab.clone());
    let saved = serde_json::to_string(&prefs).expect("serialize");
    let loaded: InventoryPreferences = serde_json::from_str(&saved).expect("deserialize");
    assert_eq!(loaded.custom_tabs[0], tab);
    let plain = serde_json::to_string(&CustomTab::default()).expect("serialize");
    assert!(
        !plain.contains("\"items\"") && !plain.contains("\"flat\""),
        "nothing written when unused"
    );
    // hand-edited: nil ids and repeats dropped, cut to the limit
    let mut long: Vec<Uuid> = vec![Uuid::nil(), id(SKIN), id(SKIN)];
    long.extend((0..80).map(|n| id(7000 + n)));
    tab.items.insert(account(), long);
    let mut tabs = vec![tab];
    assert!(super::super::normalize(&mut tabs));
    assert_eq!(of(&tabs[0], account()).len(), MAX_ITEMS);
    assert_eq!(of(&tabs[0], account())[0], id(SKIN));
}

// ---- the list ----

#[test]
fn pinned_items_show_at_the_top_once() {
    let inv = linked();
    let mut tab = tab();
    folders::set(&mut tab, account(), vec![id(BOOTS)]);
    // Skin, the Shirt, and the Boot that the Boots folder holds too
    set(&mut tab, account(), vec![id(SKIN), id(SHIRT), id(112)]);
    let names = rows(&inv, &tab, "", &Filters::default(), false);
    let mut top = names[..3].to_vec();
    top.sort();
    assert_eq!(top, ["Boot", "Shirt", "Skin"], "on top: {names:?}");
    assert_eq!(names.iter().filter(|n| *n == "Boot").count(), 1, "once: {names:?}");
    assert!(names.contains(&"Boots".to_owned()), "the scope below: {names:?}");
    assert!(!names.contains(&"Body".to_owned()), "a pin brings no folder path: {names:?}");
    assert!(!names.contains(&"Unrelated".to_owned()));
}

#[test]
fn a_tab_of_pins_shows_only_them_and_its_search_narrows_them() {
    let inv = inventory();
    let mut tab = tab();
    set(&mut tab, account(), vec![id(SKIN), id(SHIRT), id(DELETED), id(OLD_HAT)]);
    let mut names = rows(&inv, &tab, "", &Filters::default(), false);
    names.sort();
    assert_eq!(names, ["Shirt", "Skin"], "the gone ones skipped");
    assert_eq!(rows(&inv, &tab, "sk", &Filters::default(), false), ["Skin"]);
    let textures = Filters {
        types: 1 << 0,
        ..Default::default()
    };
    assert_eq!(rows(&inv, &tab, "", &textures, false), ["Skin"], "the filters too");
    assert_eq!(of(&tab, account()).len(), 4, "the gone ones are kept");
}

#[test]
fn a_flat_list_holds_the_items_only_pins_first() {
    let inv = inventory();
    let mut tab = tab();
    folders::set(&mut tab, account(), vec![id(CLOTHING)]);
    set(&mut tab, account(), vec![id(UNRELATED)]);
    let names = rows(&inv, &tab, "", &Filters::default(), true);
    assert_eq!(names[0], "Unrelated", "the pin first: {names:?}");
    let mut rest = names[1..].to_vec();
    rest.sort();
    assert_eq!(rest, ["Boot", "Shirt", "Sneaker"], "every item below, no folder: {names:?}");
    assert_eq!(rows(&inv, &tab, "boot", &Filters::default(), true), ["Boot"]);
    // no scope at all: the whole inventory, still without folders
    let whole = rows(&inv, &CustomTab::default(), "", &Filters::default(), true);
    assert!(whole.contains(&"Skin".to_owned()) && !whole.contains(&"Body".to_owned()));
}

#[test]
fn the_folder_of_a_flat_row_is_its_path() {
    let inv = inventory();
    let path = super::super::flat::folder_path(&inv, id(112)).expect("in a folder");
    assert!(path.ends_with("Boots") && path.contains("Shoes"), "{path}");
    assert_eq!(super::super::flat::folder_path(&inv, id(DELETED)), None);
}
