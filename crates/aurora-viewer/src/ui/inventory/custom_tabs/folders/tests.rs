use super::*;
use crate::ui::inventory::view::ViewCache;
use crate::world::inventory::{FetchState, Folder};
use aurora_net::inventory::{InvFolder, InvItem};

pub(in crate::ui::inventory) fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

pub(in crate::ui::inventory) fn account() -> Uuid {
    id(7)
}

pub(in crate::ui::inventory) const CLOTHING: u128 = 10;
pub(in crate::ui::inventory) const SHOES: u128 = 11;
pub(in crate::ui::inventory) const BOOTS: u128 = 12;
pub(in crate::ui::inventory) const BODY: u128 = 20;
pub(in crate::ui::inventory) const OTHER: u128 = 30;
pub(in crate::ui::inventory) const TRASH: u128 = 40;
pub(in crate::ui::inventory) const OLD: u128 = 41;
pub(in crate::ui::inventory) const OUTFITS: u128 = 50;
pub(in crate::ui::inventory) const PARTY: u128 = 51;
pub(in crate::ui::inventory) const LIBRARY_CLOTHES: u128 = 60;
pub(in crate::ui::inventory) const SHIRT: u128 = 110;
pub(in crate::ui::inventory) const DELETED: u128 = 999;

/// My Inventory (1): Clothing (Shirt) › Shoes (Sneaker) › Boots (Boot),
/// Body (Skin, a texture), Other (Unrelated), Trash › Old (Old hat),
/// My Outfits › Party (Party dress); Library (2) › Lib clothes (Lib shirt).
pub(in crate::ui::inventory) fn inventory() -> Inventory {
    let mut inv = Inventory {
        root: id(1),
        lib_root: id(2),
        ..Default::default()
    };
    for (n, parent, kind, name, library) in [
        (1, 0, 8, "My Inventory", false),
        (2, 0, 8, "Library", true),
        (CLOTHING, 1, -1, "Clothing", false),
        (SHOES, CLOTHING, -1, "Shoes", false),
        (BOOTS, SHOES, -1, "Boots", false),
        (BODY, 1, -1, "Body", false),
        (OTHER, 1, -1, "Other", false),
        (TRASH, 1, 14, "Trash", false),
        (OLD, TRASH, -1, "Old", false),
        (OUTFITS, 1, 48, "My Outfits", false),
        (PARTY, OUTFITS, -1, "Party", false),
        (LIBRARY_CLOTHES, 2, -1, "Lib clothes", true),
    ] {
        inv.folders.insert(
            id(n),
            Folder {
                info: InvFolder {
                    id: id(n),
                    parent: if parent == 0 { Uuid::nil() } else { id(parent) },
                    type_default: kind,
                    name: name.into(),
                    version: 1,
                    ..Default::default()
                },
                children: vec![],
                items: vec![],
                state: FetchState::Fetched,
                library,
            },
        );
    }
    let parents: Vec<_> = inv.folders.values().map(|f| (f.info.id, f.info.parent)).collect();
    for (child, parent) in parents {
        if let Some(f) = inv.folders.get_mut(&parent) {
            f.children.push(child);
        }
    }
    let item = |n: u128, parent: u128, name: &str, kind: i32| InvItem {
        id: id(n),
        parent: id(parent),
        name: name.into(),
        asset_type: kind,
        inv_type: kind,
        owner: account(),
        creator: account(),
        owner_mask: rules::COPY | rules::MODIFY | rules::TRANSFER,
        ..Default::default()
    };
    inv.add_items(vec![
        item(SHIRT, CLOTHING, "Shirt", 6),
        item(111, SHOES, "Sneaker", 6),
        item(112, BOOTS, "Boot", 6),
        item(120, BODY, "Skin", 0),
        item(130, OTHER, "Unrelated", 6),
        item(141, OLD, "Old hat", 6),
        item(151, PARTY, "Party dress", 6),
        item(160, LIBRARY_CLOTHES, "Lib shirt", 6),
    ]);
    inv.generation += 1;
    inv
}

pub(in crate::ui::inventory) fn move_folder(inv: &mut Inventory, n: u128, to: u128) {
    let old = inv.folders[&id(n)].info.parent;
    if let Some(f) = inv.folders.get_mut(&old) {
        f.children.retain(|c| *c != id(n));
    }
    if let Some(f) = inv.folders.get_mut(&id(to)) {
        f.children.push(id(n));
    }
    if let Some(f) = inv.folders.get_mut(&id(n)) {
        f.info.parent = id(to);
    }
    inv.generation += 1;
}

/// The names a custom tab lists (items and folders) with this scope.
fn listed_with(
    inv: &Inventory,
    roots: Option<Vec<Uuid>>,
    text: &str,
    filters: &Filters,
    prefs: &InventoryPreferences,
) -> (Vec<String>, ViewCache) {
    let facts = Facts {
        agent: account(),
        ..Default::default()
    };
    let mut view = ViewCache::default();
    view.set_folder_scope(roots);
    view.search(
        inv,
        inv.root,
        text,
        prefs,
        filters,
        CUSTOM_TAB,
        None,
        &facts,
        account(),
        view::now(),
    );
    let mut names: Vec<String> = view
        .results
        .iter()
        .map(|i| {
            inv.items
                .get(i)
                .map(|it| it.name.clone())
                .or_else(|| inv.folders.get(i).map(|f| f.info.name.clone()))
                .unwrap_or_default()
        })
        .collect();
    names.sort();
    (names, view)
}

fn listed(inv: &Inventory, folders: &[u128], text: &str, filters: &Filters) -> Vec<String> {
    let tab = scoped_tab(folders);
    listed_with(
        inv,
        search_roots(inv, &tab, account()),
        text,
        filters,
        &InventoryPreferences::default(),
    )
    .0
}

fn scoped_tab(folders: &[u128]) -> CustomTab {
    let mut tab = CustomTab {
        id: 1,
        name: "Scoped".into(),
        ..Default::default()
    };
    set(&mut tab, account(), folders.iter().map(|n| id(*n)).collect());
    tab
}

// ---- which folders ----

#[test]
fn a_folder_is_live_only_in_the_agents_inventory_out_of_the_trash() {
    let mut inv = inventory();
    assert_eq!(status(&inv, id(CLOTHING)), Status::Live);
    assert_eq!(status(&inv, id(BOOTS)), Status::Live);
    assert_eq!(status(&inv, id(PARTY)), Status::Live, "outfit folders may be chosen");
    assert_eq!(status(&inv, id(DELETED)), Status::Missing);
    assert_eq!(status(&inv, id(TRASH)), Status::Trash, "the Trash itself");
    assert_eq!(status(&inv, id(OLD)), Status::Trash);
    assert_eq!(status(&inv, id(1)), Status::Refused, "the root");
    assert_eq!(status(&inv, id(2)), Status::Refused, "the Library");
    assert_eq!(status(&inv, id(LIBRARY_CLOTHES)), Status::Refused);
    // a folder whose parent is unknown is not shown (not under the root)
    inv.folders.get_mut(&id(OTHER)).expect("other").info.parent = id(DELETED);
    inv.generation += 1;
    assert_eq!(status(&inv, id(OTHER)), Status::Missing);
}

#[test]
fn only_a_selection_of_live_entries_can_be_chosen() {
    let inv = inventory();
    assert_eq!(
        choosable(&inv, &[id(SHOES), id(BODY), id(CLOTHING)]).folders,
        [id(BODY), id(CLOTHING), id(SHOES)],
        "in the order of their paths"
    );
    assert!(choosable(&inv, &[]).is_empty());
    let mixed = choosable(&inv, &[id(CLOTHING), id(SHIRT)]);
    assert_eq!(mixed.folders, [id(CLOTHING)], "feature 2.13: folders and items together");
    assert_eq!(mixed.items, [id(SHIRT)]);
    assert!(choosable(&inv, &[id(CLOTHING), id(OLD)]).is_empty(), "in the Trash");
    assert!(choosable(&inv, &[id(SHIRT), id(141)]).is_empty(), "an item in the Trash");
    assert!(choosable(&inv, &[id(TRASH)]).is_empty(), "the Trash");
    assert!(choosable(&inv, &[id(LIBRARY_CLOTHES)]).is_empty(), "the Library");
    assert!(choosable(&inv, &[id(160)]).is_empty(), "an item of the Library");
    assert!(choosable(&inv, &[id(1)]).is_empty(), "the root");
    assert!(choosable(&inv, &[id(DELETED)]).is_empty());
    let many: Vec<Uuid> = (0..=MAX_FOLDERS as u128).map(|n| id(3000 + n)).collect();
    assert!(choosable(&inv, &many).is_empty(), "over the limit");
}

#[test]
fn a_refused_selection_says_why() {
    let mut inv = inventory();
    assert_eq!(check(&inv, &[]), Err(Refusal::Empty));
    for refused in [OLD, TRASH, LIBRARY_CLOTHES, 1, DELETED, 141, 160] {
        assert_eq!(check(&inv, &[id(BODY), id(refused)]), Err(Refusal::NotAllowed), "{refused}");
    }
    let many: Vec<Uuid> = (0..=MAX_FOLDERS as u128).map(|n| id(3000 + n)).collect();
    for folder in &many {
        let mut f = inv.folders[&id(OTHER)].clone();
        f.info.id = *folder;
        f.info.parent = id(1);
        inv.folders.insert(*folder, f);
    }
    assert_eq!(check(&inv, &many), Err(Refusal::TooMany));
    assert_eq!(
        check(&inv, &[id(BODY)]),
        Ok(Chosen {
            folders: vec![id(BODY)],
            items: vec![]
        })
    );
}

#[test]
fn add_replace_and_clear_change_the_folders() {
    let (a, b, c) = (id(CLOTHING), id(BODY), id(OTHER));
    assert_eq!(change(&[a, b], &[b, c], Change::Add), Ok(vec![a, b, c]));
    assert_eq!(change(&[a, b], &[c], Change::Replace), Ok(vec![c]));
    assert_eq!(change(&[a, b], &[c], Change::Clear), Ok(vec![]));
    let full: Vec<Uuid> = (0..MAX_FOLDERS as u128).map(|n| id(1000 + n)).collect();
    assert_eq!(change(&full, &[a], Change::Add), Err(TooMany));
    assert_eq!(change(&full, &full, Change::Add).map(|v| v.len()), Ok(MAX_FOLDERS));
}

// ---- per account, saved ----

#[test]
fn the_folders_belong_to_one_account() {
    let mut tab = scoped_tab(&[CLOTHING]);
    assert_eq!(of(&tab, account()), [id(CLOTHING)]);
    let other = id(8);
    assert!(of(&tab, other).is_empty());
    assert!(elsewhere(&tab, other), "scoped on another account only");
    assert!(!elsewhere(&tab, account()));
    set(&mut tab, other, vec![id(BODY)]);
    assert_eq!(of(&tab, other), [id(BODY)]);
    assert_eq!(of(&tab, account()), [id(CLOTHING)]);
    set(&mut tab, account(), Vec::new());
    assert!(!tab.folders.contains_key(&account()), "an empty list removes the scope");
}

#[test]
fn the_folders_round_trip_through_the_settings() {
    let mut prefs = InventoryPreferences::default();
    super::super::add(&mut prefs.custom_tabs, "Plain", Filters::default(), String::new()).expect("room");
    prefs.custom_tabs.push(scoped_tab(&[CLOTHING, BODY]));
    prefs.custom_tabs[1].id = 2;
    let saved = serde_json::to_string(&prefs).expect("serialize");
    assert_eq!(saved.matches("\"folders\"").count(), 1, "an unscoped tab writes no folders");
    let loaded: InventoryPreferences = serde_json::from_str(&saved).expect("deserialize");
    assert_eq!(loaded, prefs);
    assert_eq!(of(&loaded.custom_tabs[1], account()), [id(CLOTHING), id(BODY)]);
    // a tab saved before folder scopes
    let old: InventoryPreferences = serde_json::from_str(r#"{"custom_tabs":[{"id":4,"name":"Old"}]}"#).expect("old");
    assert!(old.custom_tabs[0].folders.is_empty());
}

#[test]
fn hand_edited_folder_lists_are_repaired() {
    let mut tab = scoped_tab(&[]);
    tab.folders.insert(Uuid::nil(), vec![id(CLOTHING)]);
    tab.folders.insert(id(8), vec![]);
    let mut long: Vec<Uuid> = vec![Uuid::nil(), id(CLOTHING), id(CLOTHING)];
    long.extend((0..40).map(|n| id(2000 + n)));
    tab.folders.insert(account(), long);
    assert!(normalize(&mut tab.folders));
    assert_eq!(tab.folders.len(), 1);
    let kept = of(&tab, account());
    assert_eq!(kept.len(), MAX_FOLDERS);
    assert_eq!(kept[0], id(CLOTHING));
    assert!(!normalize(&mut tab.folders), "a clean list is left alone");
}

// ---- the search ----

#[test]
fn a_scope_lists_its_folders_and_all_below_them() {
    let inv = inventory();
    assert_eq!(
        listed(&inv, &[CLOTHING], "", &Filters::default()),
        ["Boot", "Boots", "Clothing", "Shirt", "Shoes", "Sneaker"]
    );
    assert_eq!(
        listed(&inv, &[BOOTS, BODY], "", &Filters::default()),
        ["Body", "Boot", "Boots", "Skin"],
        "the union of the folders"
    );
    let whole = listed_with(&inv, None, "", &Filters::default(), &InventoryPreferences::default()).0;
    assert!(whole.contains(&"Unrelated".to_owned()), "no scope: the whole inventory");
}

#[test]
fn the_parents_of_a_scoped_folder_are_only_its_path() {
    let inv = inventory();
    let tab = scoped_tab(&[BOOTS]);
    let (names, view) = listed_with(
        &inv,
        search_roots(&inv, &tab, account()),
        "",
        &Filters::default(),
        &InventoryPreferences::default(),
    );
    assert_eq!(names, ["Boot", "Boots"]);
    let branches: std::collections::HashSet<Uuid> = view.result_folders().collect();
    for parent in [1, CLOTHING, SHOES] {
        assert!(branches.contains(&id(parent)), "the path through {parent}");
    }
    assert!(!names.contains(&"Shirt".to_owned()) && !names.contains(&"Sneaker".to_owned()));
}

#[test]
fn filters_and_search_apply_inside_the_scope() {
    let inv = inventory();
    assert_eq!(listed(&inv, &[CLOTHING], "boot", &Filters::default()), ["Boot", "Boots"]);
    assert!(listed(&inv, &[BODY], "boot", &Filters::default()).is_empty());
    let textures = Filters {
        types: 1 << 0,
        ..Default::default()
    };
    assert_eq!(listed(&inv, &[CLOTHING, BODY], "", &textures), ["Skin"]);
    assert!(listed(&inv, &[CLOTHING], "", &textures).is_empty());
    let advanced = Filters {
        smart: Some(crate::ui::inventory::smart::SmartQuery::from_quick(
            "s",
            crate::ui::inventory::view::SearchField::Name,
        )),
        ..Default::default()
    };
    let names = listed(&inv, &[CLOTHING], "", &advanced);
    assert!(names.contains(&"Shirt".to_owned()) && !names.contains(&"Skin".to_owned()));
}

#[test]
fn a_moved_folder_is_followed() {
    let mut inv = inventory();
    move_folder(&mut inv, SHOES, OTHER);
    assert_eq!(
        listed(&inv, &[SHOES], "", &Filters::default()),
        ["Boot", "Boots", "Shoes", "Sneaker"],
        "the scope follows the folder"
    );
    assert_eq!(
        listed(&inv, &[CLOTHING], "", &Filters::default()),
        ["Clothing", "Shirt"],
        "what left the folder leaves the tab"
    );
}

#[test]
fn deleted_and_trashed_folders_are_skipped_but_kept() {
    let mut inv = inventory();
    let tab = scoped_tab(&[CLOTHING, DELETED]);
    assert_eq!(search_roots(&inv, &tab, account()), Some(vec![id(CLOTHING)]));
    assert_eq!(listed(&inv, &[CLOTHING, DELETED], "", &Filters::default()).len(), 6);
    move_folder(&mut inv, CLOTHING, TRASH);
    assert_eq!(status(&inv, id(CLOTHING)), Status::Trash);
    assert_eq!(search_roots(&inv, &tab, account()), Some(vec![]), "scoped to nothing left");
    let prefs = InventoryPreferences {
        search_trash: true,
        ..Default::default()
    };
    let (names, _) = listed_with(&inv, search_roots(&inv, &tab, account()), "", &Filters::default(), &prefs);
    assert!(names.is_empty(), "an empty scope shows nothing, not everything");
    assert_eq!(of(&tab, account()).len(), 2, "the ids stay, for a restore");
    move_folder(&mut inv, CLOTHING, 1);
    assert_eq!(
        listed(&inv, &[CLOTHING, DELETED], "", &Filters::default()).len(),
        6,
        "restored, it is back"
    );
}

#[test]
fn a_scoped_outfit_folder_shows_even_when_search_skips_outfits() {
    let inv = inventory();
    let prefs = InventoryPreferences {
        search_outfits: false,
        ..Default::default()
    };
    let whole = listed_with(&inv, None, "", &Filters::default(), &prefs).0;
    assert!(!whole.contains(&"Party dress".to_owned()), "outfits hidden by the preference");
    let tab = scoped_tab(&[PARTY, CLOTHING]);
    let names = listed_with(&inv, search_roots(&inv, &tab, account()), "", &Filters::default(), &prefs).0;
    assert!(names.contains(&"Party dress".to_owned()) && names.contains(&"Shirt".to_owned()));
}

#[test]
fn a_changed_scope_searches_again() {
    let inv = inventory();
    let prefs = InventoryPreferences::default();
    let facts = Facts {
        agent: account(),
        ..Default::default()
    };
    let mut view = ViewCache::default();
    let search = |view: &mut ViewCache| {
        view.search(
            &inv,
            inv.root,
            "",
            &prefs,
            &Filters::default(),
            CUSTOM_TAB,
            None,
            &facts,
            account(),
            1000,
        );
        view.results.len()
    };
    view.set_folder_scope(Some(vec![id(BODY)]));
    assert_eq!(search(&mut view), 2);
    view.set_folder_scope(Some(vec![id(CLOTHING)]));
    assert_eq!(search(&mut view), 6);
    view.set_folder_scope(None);
    assert!(search(&mut view) > 6);
}
