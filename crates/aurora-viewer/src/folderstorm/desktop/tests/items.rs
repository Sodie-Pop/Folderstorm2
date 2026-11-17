//! Frame tests of the item shortcuts (checkpoint 5 of the plan): a
//! landmark on a desktop, drawn, opened, greyed, hidden by RLVa, dropped
//! from the Inventory.

use super::*;
use crate::folderstorm::desktop::targets::{self, Greyed};
use aurora_net::inventory::InvItem;

/// A landmark of the bench's inventory, in the folder [`MINE`].
const LANDMARK: u128 = 9100;
/// A link to it, in [`SUB`].
const LINK: u128 = 9101;
/// A landmark in the Trash.
const TRASHED: u128 = 9102;
/// A texture: not a kind a shortcut opens.
const TEXTURE: u128 = 9105;
/// The landmark's asset: never drawn, never asked for by the desktop.
const ASSET: u128 = 0x00A5_5E70_0000_9900;

impl Bench {
    /// The bench with items in its inventory: a landmark "Beach", a link
    /// to it, a landmark in the Trash and a texture.
    fn with_items() -> Self {
        let mut b = Bench::new();
        let trash = crate::world::inventory::actions::system(&b.inv, 14).expect("a Trash");
        let item = |id: u128, parent: Uuid, name: &str, kind: i32, asset: u128| InvItem {
            id: Uuid::from_u128(id),
            parent,
            name: name.into(),
            asset_type: kind,
            inv_type: kind,
            asset_id: Uuid::from_u128(asset),
            owner: ME,
            base_mask: 0x7fff_ffff,
            owner_mask: 0x7fff_ffff,
            ..Default::default()
        };
        b.inv.add_items(vec![
            item(LANDMARK, Uuid::from_u128(MINE), "Beach", 3, ASSET),
            InvItem {
                asset_type: 24,
                ..item(LINK, Uuid::from_u128(SUB), "Beach", 3, LANDMARK)
            },
            item(TRASHED, trash, "Old place", 3, 9902),
            item(TEXTURE, Uuid::from_u128(MINE), "A picture", 0, 9905),
        ]);
        b
    }

    /// A shortcut to the item `n`.
    fn add_item(&mut self, n: u128, slot: u8, cell: (u16, u16)) -> u32 {
        let target = Target::Item { item: Uuid::from_u128(n) };
        self.desktop.add(target, slot, cell).expect("added")
    }

    /// The items asked to be opened.
    fn opened_items(&self) -> Vec<Uuid> {
        self.out
            .iter()
            .filter_map(|r| match r {
                Request::OpenItem(item) => Some(*item),
                _ => None,
            })
            .collect()
    }

    /// Nothing drawn says an id of the inventory.
    fn says_no_id(&self) -> bool {
        self.texts.iter().all(|(text, _)| {
            [LANDMARK, LINK, ASSET]
                .iter()
                .all(|n| !text.contains(&Uuid::from_u128(*n).to_string()))
        })
    }
}

#[test]
fn the_inventory_s_icons_of_the_items_are_in_the_skin() {
    let b = Bench::new();
    for icon in ["Inv_Landmark", targets::ICON_ITEM] {
        assert!(
            b.icons.get(icon).is_some() || crate::ui::icons::global(icon).is_some(),
            "{icon} is not an icon of the skin"
        );
    }
}

#[test]
fn a_landmark_shortcut_shows_its_name_and_asks_for_its_teleport() {
    let mut b = Bench::with_items();
    let beach = b.add_item(LANDMARK, 0, (0, 0));
    b.frames(3);
    let cell = b.cell_of(beach);
    assert!(
        b.drawn("Beach").is_some_and(|text| cell.contains_rect(text)),
        "the landmark's name, read now"
    );
    assert!(b.world_untouched() && b.says_no_id());
    b.picture("desktop-landmark");
    // a single click selects, a double click asks for the teleport
    b.click(cell.center());
    assert!(b.opened_items().is_empty());
    b.time += 2.0;
    b.double_click(cell.center());
    assert_eq!(b.opened_items(), vec![Uuid::from_u128(LANDMARK)]);
    assert!(b.opened().is_empty() && b.ran().is_empty());
    // its menu: Teleport, and Show in inventory
    b.out.clear();
    b.time += 2.0;
    b.click_with(cell.center(), egui::PointerButton::Secondary);
    assert!(b.drawn(aurora_i18n::tr!("fs-desktop-open")).is_none(), "its own verb instead");
    let teleport = b.drawn(aurora_i18n::tr!("common-teleport")).expect("Teleport is in its menu");
    assert!(b.says_no_id());
    b.picture("desktop-landmark-menu");
    b.click(teleport.center());
    assert_eq!(b.out, vec![Request::OpenItem(Uuid::from_u128(LANDMARK))]);
    b.out.clear();
    b.menu(cell.center(), aurora_i18n::tr!("ui-context-show-in-inventory"));
    assert_eq!(b.out, vec![Request::ShowItem(Uuid::from_u128(LANDMARK))]);
    // with a single click to open, one click asks
    b.out.clear();
    b.desktop.single_click = true;
    b.time += 2.0;
    b.click(cell.center());
    assert_eq!(b.opened_items(), vec![Uuid::from_u128(LANDMARK)]);
    // Enter on the focused shortcut too
    b.out.clear();
    b.key(egui::Key::Enter);
    assert_eq!(b.opened_items(), vec![Uuid::from_u128(LANDMARK)]);
}

#[test]
fn a_landmark_rlva_refuses_is_greyed_and_does_not_teleport() {
    let mut b = Bench::with_items();
    let beach = b.add_item(LANDMARK, 0, (0, 0));
    b.facts.landmark_blocked = true;
    b.frames(3);
    let cell = b.cell_of(beach);
    // its name stays: only the teleport is refused
    assert!(b.drawn("Beach").is_some());
    b.double_click(cell.center());
    assert!(b.opened_items().is_empty());
    b.time += 2.0;
    b.click_with(cell.center(), egui::PointerButton::Secondary);
    let teleport = b.drawn(aurora_i18n::tr!("common-teleport")).expect("listed, greyed");
    b.click(teleport.center());
    assert!(b.opened_items().is_empty());
    // it can still be renamed and shown in the Inventory
    b.frames(2);
    b.out.clear();
    b.menu(cell.center(), aurora_i18n::tr!("ui-context-show-in-inventory"));
    assert_eq!(b.out, vec![Request::ShowItem(Uuid::from_u128(LANDMARK))]);
    // the restriction lifted: it teleports again
    b.facts.landmark_blocked = false;
    b.out.clear();
    b.frames(2);
    b.time += 2.0;
    b.double_click(cell.center());
    assert_eq!(b.opened_items(), vec![Uuid::from_u128(LANDMARK)]);
}

#[test]
fn showinv_locks_the_item_shortcuts_and_hides_their_names() {
    let mut b = Bench::with_items();
    let beach = b.add_item(LANDMARK, 0, (0, 0));
    b.desktop.shortcuts[0].name = Some("My own name for it".into());
    b.frames(3);
    assert!(b.drawn("My own name for it").is_some());
    b.no_inventory = true;
    b.frames(2);
    assert!(
        b.texts
            .iter()
            .all(|(text, _)| !text.contains("Beach") && !text.contains("My own name")),
        "nothing of the item is said while the Inventory is hidden: {:?}",
        b.texts
    );
    assert!(b.drawn(aurora_i18n::tr!("fs-desktop-item")).is_some());
    b.picture("desktop-item-showinv");
    let cell = b.cell_of(beach);
    b.double_click(cell.center());
    assert!(b.opened_items().is_empty(), "it does not open");
    // neither renamed nor shown in the Inventory meanwhile
    b.time += 2.0;
    b.click(cell.center());
    b.key(egui::Key::F2);
    assert_eq!(surface::state(&b.ctx).rename, None);
    b.time += 2.0;
    b.click_with(cell.center(), egui::PointerButton::Secondary);
    let show = b.drawn(aurora_i18n::tr!("ui-context-show-in-inventory")).expect("listed, greyed");
    b.click(show.center());
    assert!(b.out.iter().all(|r| !matches!(r, Request::ShowItem(_))));
    // (a click on the desktop closes the menu)
    b.time += 2.0;
    b.click(b.room(0).grid.rect((3, 3)).center());
    // and no item is added
    b.frames(2);
    b.out.clear();
    let at = b.room(0).grid.rect((2, 2)).center();
    b.drop_inventory(&[LINK], at);
    assert_eq!(b.desktop.shortcuts.len(), 1);
    assert_eq!(b.notices(), vec![targets::Refusal::Rlva.text()]);
    // the restriction lifted: as before
    b.no_inventory = false;
    b.frames(2);
    assert!(b.drawn("My own name for it").is_some());
    b.time += 2.0;
    b.double_click(cell.center());
    assert_eq!(b.opened_items(), vec![Uuid::from_u128(LANDMARK)]);
}

#[test]
fn an_item_that_is_gone_or_in_the_trash_is_greyed_and_removed_from_its_menu() {
    let mut b = Bench::with_items();
    let gone = b.add_item(123_456, 0, (0, 0));
    let trashed = b.add_item(TRASHED, 0, (0, 1));
    let other = b.add_item(TEXTURE, 0, (0, 2));
    b.frames(3);
    for id in [gone, trashed, other] {
        assert!(b.painted(b.cell_of(id)) > 100, "shown, greyed");
        b.time += 2.0;
        b.double_click(b.cell_of(id).center());
    }
    assert!(b.opened_items().is_empty());
    // nothing of an item that is not the user's to open is said
    assert!(
        b.texts
            .iter()
            .all(|(text, _)| !text.contains("Old place") && !text.contains("A picture"))
    );
    // why, as its tooltip says
    let face = |b: &Bench, id: u32| {
        let folders = Folders {
            inv: &b.inv,
            no_inventory: false,
            colors: None,
        };
        targets::face(b.desktop.shortcut(id).expect("a shortcut"), &b.facts, &folders).greyed
    };
    assert_eq!(face(&b, gone), Some(Greyed::ItemGone));
    assert_eq!(face(&b, trashed), Some(Greyed::ItemTrash));
    assert_eq!(face(&b, other), Some(Greyed::ItemOther));
    // the menu: Remove works
    b.menu(b.cell_of(gone).center(), aurora_i18n::tr!("fs-desktop-remove"));
    assert!(b.desktop.shortcut(gone).is_none());
    assert!(b.desktop.shortcut(trashed).is_some());
}

#[test]
fn inventory_dropped_on_a_desktop_adds_its_landmarks_with_its_folders() {
    let mut b = Bench::with_items();
    b.frames(3);
    let grid = b.room(0).grid;
    // held over the desktop: a landmark is welcome
    crate::ui::inventory::drag::demo_carry(&b.ctx, vec![Uuid::from_u128(LINK)]);
    b.pointer = grid.rect((2, 2)).center();
    b.frames(2);
    assert!(b.desktop.shortcuts.is_empty(), "nothing before the release");
    // released: a shortcut to the landmark the link points to, in the cell
    // under the pointer
    b.drop_inventory(&[LINK], grid.rect((2, 2)).center());
    let added = b.desktop.shortcuts.last().expect("added").clone();
    assert_eq!(
        added.target,
        Target::Item {
            item: Uuid::from_u128(LANDMARK)
        }
    );
    assert_eq!((added.slot, added.cell()), (0, (2, 2)));
    assert!(crate::ui::inventory::drag::carried_ids(&b.ctx).is_none());
    assert!(b.out.contains(&Request::Changed) && b.notices().is_empty());
    assert!(b.opened_items().is_empty(), "added, not opened");
    b.frames(2);
    assert!(b.drawn("Beach").is_some() && b.says_no_id());
    // the landmark itself: the same shortcut, selected, not added twice
    b.out.clear();
    b.drop_inventory(&[LANDMARK], grid.rect((4, 0)).center());
    assert_eq!(b.desktop.shortcuts.len(), 1);
    assert_eq!(surface::state(&b.ctx).selected, Some(added.id));
    assert_eq!(b.notices(), vec![aurora_i18n::tr!("fs-desktop-add-existing")]);
    // a selection: its folder and its landmark go in, the rest is counted
    b.out.clear();
    b.drop_inventory(&[SUB, LANDMARK, TEXTURE, TRASHED], b.room(2).grid.rect((0, 0)).center());
    let there: Vec<_> = b
        .desktop
        .shortcuts
        .iter()
        .filter(|s| s.slot == 2)
        .map(|s| s.target.clone())
        .collect();
    assert_eq!(
        there,
        vec![
            Target::Folder {
                folder: Uuid::from_u128(SUB)
            },
            Target::Item {
                item: Uuid::from_u128(LANDMARK)
            }
        ]
    );
    assert_eq!(b.notices(), vec![aurora_i18n::tr!("fs-desktop-left-out", count = 2).as_str()]);
    // what cannot be a shortcut alone: refused, with the reason
    b.out.clear();
    for cargo in [vec![TEXTURE], vec![TRASHED], vec![ITEM]] {
        b.drop_inventory(&cargo, grid.rect((4, 2)).center());
    }
    assert_eq!(b.desktop.shortcuts.len(), 3);
    assert_eq!(b.notices(), vec![targets::Refusal::Nothing.text(); 3]);
}

#[test]
fn item_shortcuts_fill_a_desktop_as_folders_do_and_each_account_has_its_own() {
    let mut b = Bench::with_items();
    b.frames(3);
    // from the Inventory's menu: the first free cells of the first desktop
    let rooms = b.rooms();
    let picked = targets::offer(&b.inv, &[Uuid::from_u128(MINE), Uuid::from_u128(LINK)], false).expect("offered");
    let added = super::super::add_targets(&mut b.desktop, &rooms, &picked.targets, None, None);
    assert_eq!((added.added, added.existing), (2, 0));
    let cells: Vec<_> = b.desktop.shortcuts.iter().map(|s| (s.slot, s.cell())).collect();
    assert_eq!(cells, [(0, (0, 0)), (0, (0, 1))]);
    // a full desktop says so for an item too
    let mut full = Bench::with_items();
    full.frames(3);
    fill(&mut full);
    let more = super::super::add_targets(
        &mut full.desktop,
        &rooms,
        &[Target::Item {
            item: Uuid::from_u128(TRASHED),
        }],
        None,
        None,
    );
    assert_eq!((more.added, more.no_room), (0, 1));
    assert!(
        super::super::added_notice(&more, 0, false, true).is_some_and(|text| text.contains(&NoRoom::Full.text())),
        "the full-desktop notice"
    );
    // another account sees none of it
    b.desktop.sign_in(OTHER);
    b.frames(2);
    assert!(b.drawn("Beach").is_none() && b.desktop.shortcuts.is_empty());
    b.desktop.sign_in(ME);
    b.frames(2);
    assert!(b.drawn("Beach").is_some());
}
