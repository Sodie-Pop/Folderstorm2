//! Folderstorm: the right-click menu of folders and items › "Create tab" and
//! "Add to ›" (folder-scoped Inventory tabs, feature 2.2; items are pinned,
//! feature 2.13). What may be chosen and the changes are in
//! `custom_tabs/folder_ui.rs`; this builds the entries of the shared menu,
//! so both Inventory styles have them.
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
