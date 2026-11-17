//! Folderstorm: the right-click menu of folders and items › "Create tab" and
//! "Add to ›" (folder-scoped Inventory tabs, feature 2.2; items are pinned,
//! feature 2.13). What may be chosen and the changes are in
//! `custom_tabs/folder_ui.rs`; this builds the entries of the shared menu,
//! so both Inventory styles have them.
//!
//! And "Add to desktop" (the margins' desktop, FEATURES row 1.10): a
//! shortcut to each folder of the selection, and to each item of a kind a
//! shortcut opens (a landmark), through `InvAction::FolderstormDesktopAdd`.
//! Which folders and items can be shortcuts is in
//! `crate::folderstorm::desktop::targets`.
use super::*;
use crate::ui::inventory::custom_tabs::folder_ui;

pub(in crate::ui::inventory) enum Command {
    /// A new custom tab scoped to these folders (and pinning these items).
    Create(Vec<Uuid>),
    /// These folders (and items) join the custom tab with this id.
    AddTo(u64, Vec<Uuid>),
}

/// The entry ids, kept by the menu layouts.
const CREATE: &str = "folder-tab";
const ADD: &str = "folder-tab-add";
const NEW: &str = "folder-tab-add-new";
const DESKTOP: &str = "desktop-add";

fn tab_entry(id: u64) -> String {
    format!("{ADD}:{id}")
}

/// After "Review bulk changes", on folders and items: "Create tab", then "Add to ›"
/// listing the custom tabs (a check mark on those that have the folders
/// already) and "New tab…". A greyed entry says why in its tooltip.
pub(super) fn entries(c: &mut Catalog, inv: &Inventory, st: &InventoryUi, prefs: &InventoryPreferences, facts: &Facts, ids: &[Uuid]) {
    let offer = folder_ui::offer(inv, st, prefs, facts.agent, ids);
    c.add(
        CREATE,
        TEXT,
        "bookmarks",
        aurora_i18n::tr!("inventory-folder-tab-create"),
        offer.create.is_ok(),
        Command::Create(ids.to_vec()).into(),
    );
    let zone = c.group(
        ADD,
        TEXT,
        "bookmark-filled",
        aurora_i18n::tr!("inventory-folder-tab-add-to"),
        offer.add.is_ok(),
        Direction::Horizontal,
        false,
    );
    if let Err(why) = &offer.add {
        c.help.insert(ADD.into(), why.clone());
    }
    for tab in &offer.tabs {
        let id = tab_entry(tab.id);
        c.add(
            id.clone(),
            &zone,
            "bookmark-filled",
            &tab.name,
            tab.refused.is_none(),
            Command::AddTo(tab.id, ids.to_vec()).into(),
        );
        if tab.has_all
            && let Some(entry) = c.entries.iter_mut().rev().find(|e| e.id == id)
        {
            entry.checked = Some(true);
            c.help
                .insert(id.clone(), aurora_i18n::tr!("inventory-folder-tab-has-tip").to_owned());
        }
        if let Some(why) = &tab.refused {
            c.help.insert(id, why.clone());
        }
    }
    if !offer.tabs.is_empty() {
        c.divider(NEW, &zone);
    }
    c.add(
        NEW,
        &zone,
        "plus",
        aurora_i18n::tr!("inventory-folder-tab-new"),
        offer.create.is_ok(),
        Command::Create(ids.to_vec()).into(),
    );
    if let Err(why) = &offer.create {
        c.help.insert(CREATE.into(), why.clone());
        c.help.insert(NEW.into(), why.clone());
    }
    desktop_entry(c, inv, ids);
}

/// "Add to desktop": greyed, with the reason, when the selection has
/// nothing that can be a shortcut (other kinds of items, the Trash, folders
/// of the Library). Whether RLVa hides the Inventory is checked by the app
/// when it adds them.
fn desktop_entry(c: &mut Catalog, inv: &Inventory, ids: &[Uuid]) {
    let offer = crate::folderstorm::desktop::targets::offer(inv, ids, false);
    c.add(
        DESKTOP,
        TEXT,
        "device-desktop-filled",
        aurora_i18n::tr!("fs-desktop-add"),
        offer.is_ok(),
        super::Command::Invoke(InvAction::FolderstormDesktopAdd(ids.to_vec())),
    );
    if let Err(why) = offer {
        c.help.insert(DESKTOP.into(), why.text().to_owned());
    }
}

impl From<Command> for super::Command {
    fn from(command: Command) -> Self {
        Self::FolderTab(command)
    }
}

pub(super) fn execute(command: Command, inv: &Inventory, st: &mut InventoryUi, prefs: &mut InventoryPreferences, facts: &Facts) {
    match command {
        Command::Create(ids) => {
            folder_ui::save_as_tab(inv, st, prefs, facts.agent, &ids);
        }
        Command::AddTo(tab, ids) => folder_ui::add_to_tab(inv, st, prefs, facts.agent, tab, &ids),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_to_desktop_is_offered_for_folders_and_greyed_with_the_reason_otherwise() {
        let mut inv = Inventory {
            root: Uuid::from_u128(1),
            ..Default::default()
        };
        crate::world::inventory::demo::seed(&mut inv, Uuid::from_u128(42));
        // a landmark of the inventory: an item a shortcut opens
        let landmark = Uuid::from_u128(9100);
        inv.add_items(vec![aurora_net::inventory::InvItem {
            id: landmark,
            parent: Uuid::from_u128(8000),
            name: "Beach".into(),
            asset_type: 3,
            inv_type: 3,
            asset_id: Uuid::from_u128(9900),
            ..Default::default()
        }]);
        let entry = |ids: &[Uuid]| {
            let mut c = Catalog::default();
            desktop_entry(&mut c, &inv, ids);
            let e = c.entries.iter().find(|e| e.id == DESKTOP).expect("listed");
            let adds = matches!(
                c.commands.first(),
                Some(Some(super::super::Command::Invoke(InvAction::FolderstormDesktopAdd(carried)))) if carried == ids
            );
            (e.enabled, adds, c.help.get(DESKTOP).cloned())
        };
        let (mine, item) = (Uuid::from_u128(8000), Uuid::from_u128(710));
        assert!(inv.items.get(&item).is_some());
        // a folder, and a selection with one
        assert_eq!(entry(&[mine]), (true, true, None));
        assert_eq!(entry(&[item, mine]), (true, true, None));
        // an item of a kind a shortcut opens, alone or with others
        assert_eq!(entry(&[landmark]), (true, true, None));
        assert_eq!(entry(&[item, landmark]), (true, true, None));
        // another kind of item, the Library, the Trash: greyed, and why
        let why = Some(crate::folderstorm::desktop::targets::Refusal::Nothing.text().to_owned());
        let trash = rules::system(&inv, 14).expect("a Trash");
        for refused in [item, inv.lib_root, trash] {
            let (enabled, _, help) = entry(&[refused]);
            assert!(!enabled, "{refused}");
            assert_eq!(help, why);
        }
    }
}
