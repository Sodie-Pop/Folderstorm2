//! Folderstorm: the folder scope of a custom Inventory tab, on screen
//! (feature 2.2; the rules are in `folders.rs`).
//!
//! - right click on folders › "Save as Inventory tab…": a new custom tab
//!   scoped to them, named after the first, its Rename dialog open
//!   (Folderstorm's gear menu "Save selected folders as view…");
//! - right click on a custom tab › "Add selected folders", "Use only selected
//!   folders", "Show all folders" (Folderstorm's tab menu);
//! - "Customize tab…" lists the tab's folders by path, each with a remove
//!   button, and offers the same three actions;
//! - a note above the list when some of the tab's folders are gone, or when
//!   it is scoped on another account only.
//!
//! What these need from the inventory (the selection that may be chosen, the
//! paths and states of the tab's folders) is read once a frame by
//! `before_list`, before the list is searched; the menus and the dialog,
//! drawn where the inventory is not at hand, read it from there.

use super::folders::{self, Change, MAX_FOLDERS, Status};
use super::*;

/// What the window saw of the inventory this frame (`before_list`).
#[derive(Default)]
pub(in crate::ui::inventory) struct Seen {
    /// The account whose folders the tabs show (nil: none yet).
    account: Uuid,
    /// The inventory is there (its root known).
    ready: bool,
    /// The window's selection, when it may become a scope.
    choosable: Vec<Uuid>,
    /// The tab shown, its folders on this account, by path.
    rows: Vec<Row>,
    /// Its folders and the inventory generation `rows` were read from.
    key: Option<(u64, u64, Vec<Uuid>)>,
    /// It is scoped on another account only.
    elsewhere: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::inventory) struct Row {
    pub id: Uuid,
    pub path: String,
    pub status: Status,
}

impl Seen {
    fn unavailable(&self) -> usize {
        self.rows.iter().filter(|r| r.status != Status::Live).count()
    }
}

fn shown_tab<'a>(st: &InventoryUi, prefs: &'a InventoryPreferences) -> Option<&'a CustomTab> {
    let id = active(st)?;
    prefs.custom_tabs.iter().find(|t| t.id == id)
}

/// Once a frame, before the list of the window is searched: what the menus
/// and the dialog need, and the scope handed to the search (none unless a
/// scoped custom tab is shown).
pub(in crate::ui::inventory) fn before_list(inv: &Inventory, st: &mut InventoryUi, prefs: &InventoryPreferences, account: Uuid) {
    let seen = &mut st.custom_tabs.seen;
    seen.account = account;
    seen.ready = !inv.root.is_nil();
    // a folder window has no custom tabs (Folderstorm: not in single-folder mode)
    seen.choosable = if st.scope.is_nil() && st.selection.len() <= MAX_FOLDERS && !account.is_nil() {
        let selection: Vec<Uuid> = st.selection.iter().copied().collect();
        folders::choosable(inv, &selection)
    } else {
        Vec::new()
    };
    let tab = shown_tab(st, prefs);
    let saved = tab.map(|t| folders::of(t, account).to_vec()).unwrap_or_default();
    let key = (inv.generation, tab.map_or(0, |t| t.id), saved.clone());
    let seen = &mut st.custom_tabs.seen;
    seen.elsewhere = tab.is_some_and(|t| folders::elsewhere(t, account));
    if seen.key.as_ref() != Some(&key) {
        seen.rows = saved
            .iter()
            .map(|id| {
                let status = folders::status(inv, *id);
                let path = if status == Status::Missing {
                    aurora_i18n::tr!("inventory-folder-tab-deleted-folder").to_owned()
                } else {
                    super::super::inventory_path(inv, *id)
                };
                Row { id: *id, path, status }
            })
            .collect();
        seen.key = Some(key);
    }
    let roots = tab.and_then(|t| folders::search_roots(inv, t, account));
    st.view.set_folder_scope(roots);
}

fn apply(st: &mut InventoryUi, prefs: &mut InventoryPreferences, id: u64, how: Change) {
    let account = st.custom_tabs.seen.account;
    if account.is_nil() {
        return;
    }
    let chosen = st.custom_tabs.seen.choosable.clone();
    let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) else {
        return;
    };
    match folders::change(folders::of(tab, account), &chosen, how) {
        Ok(new) => folders::set(tab, account, new),
        Err(folders::TooMany) => st.message = aurora_i18n::tr!("inventory-folder-tab-limit", count = MAX_FOLDERS),
    }
}

fn remove(st: &InventoryUi, prefs: &mut InventoryPreferences, id: u64, folder: impl Fn(&Uuid) -> bool) {
    let account = st.custom_tabs.seen.account;
    if let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) {
        let kept = folders::of(tab, account).iter().copied().filter(|f| !folder(f)).collect();
        folders::set(tab, account, kept);
    }
}

/// The three entries of a custom tab's menu (and of its Customize dialog).
fn actions(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences, id: u64, in_menu: bool) {
    let seen = &st.custom_tabs.seen;
    let can_choose = !seen.choosable.is_empty();
    let scoped = prefs
        .custom_tabs
        .iter()
        .find(|t| t.id == id)
        .is_some_and(|t| !folders::of(t, seen.account).is_empty());
    let select_tip = aurora_i18n::tr!("inventory-folder-tab-select-tip", count = MAX_FOLDERS);
    let entries = [
        (
            Change::Add,
            "folder-filled",
            aurora_i18n::tr!("inventory-folder-tab-add"),
            can_choose,
            if can_choose { "" } else { select_tip.as_str() },
        ),
        (
            Change::Replace,
            "filter",
            aurora_i18n::tr!("inventory-folder-tab-replace"),
            can_choose,
            if can_choose { "" } else { select_tip.as_str() },
        ),
        (
            Change::Clear,
            "x",
            aurora_i18n::tr!("inventory-folder-tab-clear"),
            scoped,
            aurora_i18n::tr!("inventory-folder-tab-clear-tip"),
        ),
    ];
    for (how, icon, label, enabled, tip) in entries {
        let clicked = if in_menu {
            menu::item_tip(ui, p, icon, label, enabled, tip)
        } else {
            let resp = ui.add_enabled_ui(enabled, |ui| flat_button(ui, p, label)).inner;
            let resp = if tip.is_empty() {
                resp
            } else {
                resp.on_hover_text(tip).on_disabled_hover_text(tip)
            };
            resp.clicked() && enabled
        };
        if clicked {
            apply(st, prefs, id, how);
        }
    }
}

/// The entries of a custom tab's right-click menu, after Clone.
pub(super) fn tab_menu(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences, id: u64) {
    menu::separator(ui, p);
    actions(ui, p, st, prefs, id, true);
}

/// The "Folders" part of "Customize tab…", above the filters.
pub(in crate::ui::inventory) fn editor(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    let Some(id) = active(st) else {
        return;
    };
    ui.label(RichText::new(aurora_i18n::tr!("inventory-folder-tab-heading")).color(p.muted));
    let seen = &st.custom_tabs.seen;
    let mut drop = None;
    if seen.rows.is_empty() {
        ui.label(aurora_i18n::tr!("inventory-folder-tab-all"));
        if seen.elsewhere {
            ui.label(
                RichText::new(aurora_i18n::tr!("inventory-folder-tab-elsewhere"))
                    .size(12.0)
                    .color(p.muted),
            );
        }
    }
    for row in &seen.rows {
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, p, "x", aurora_i18n::tr!("inventory-folder-tab-remove"), true) {
                drop = Some(row.id);
            }
            let color = if row.status == Status::Live { p.ink } else { p.muted };
            ui.add(egui::Label::new(RichText::new(&row.path).color(color)).truncate())
                .on_hover_text(&row.path);
            let tag = match row.status {
                Status::Live => None,
                Status::Missing => Some(aurora_i18n::tr!("inventory-folder-tab-status-missing")),
                Status::Trash => Some(aurora_i18n::tr!("inventory-folder-tab-status-trash")),
                Status::Refused => Some(aurora_i18n::tr!("inventory-folder-tab-status-refused")),
            };
            if let Some(tag) = tag {
                ui.label(RichText::new(tag).size(12.0).color(p.warn));
            }
        });
    }
    if let Some(folder) = drop {
        remove(st, prefs, id, |f| *f == folder);
    }
    ui.horizontal_wrapped(|ui| actions(ui, p, st, prefs, id, false));
    ui.label(
        RichText::new(aurora_i18n::tr!("inventory-folder-tab-help", count = MAX_FOLDERS))
            .size(12.0)
            .color(p.muted),
    );
    ui.separator();
}

/// Above the list of a scoped tab: how many of its folders are gone, with
/// "Remove them"; or that it is scoped on another account only. Returns the
/// height left for the list.
pub(in crate::ui::inventory) fn note(
    ui: &mut egui::Ui,
    p: &Palette,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    list_h: f32,
) -> f32 {
    let Some(id) = active(st) else {
        return list_h;
    };
    let seen = &st.custom_tabs.seen;
    if !seen.ready {
        return list_h;
    }
    let gone = seen.unavailable();
    if gone == 0 && !seen.elsewhere {
        return list_h;
    }
    let top = ui.cursor().top();
    let mut clear = false;
    ui.horizontal_wrapped(|ui| {
        if gone > 0 {
            ui.label(
                RichText::new(aurora_i18n::tr!("inventory-folder-tab-note-gone", count = gone))
                    .size(12.0)
                    .color(p.warn),
            );
            if flat_button(ui, p, aurora_i18n::tr!("inventory-folder-tab-note-remove"))
                .on_hover_text(aurora_i18n::tr!("inventory-folder-tab-note-remove-tip"))
                .clicked()
            {
                clear = true;
            }
        } else {
            ui.label(
                RichText::new(aurora_i18n::tr!("inventory-folder-tab-elsewhere"))
                    .size(12.0)
                    .color(p.muted),
            );
        }
    });
    if clear {
        let gone: Vec<Uuid> = seen.rows.iter().filter(|r| r.status != Status::Live).map(|r| r.id).collect();
        remove(st, prefs, id, |f| gone.contains(f));
    }
    (list_h - (ui.cursor().top() - top)).max(40.0)
}

// ---- A new tab from folders ----

/// "Save as Inventory tab…" can make a tab of these entries: folders only,
/// room for one more tab, in the main window, on a known account.
pub(in crate::ui::inventory) fn can_save(
    inv: &Inventory,
    st: &InventoryUi,
    prefs: &InventoryPreferences,
    account: Uuid,
    ids: &[Uuid],
) -> bool {
    !account.is_nil() && st.scope.is_nil() && prefs.custom_tabs.len() < MAX_TABS && !folders::choosable(inv, ids).is_empty()
}

/// A new custom tab scoped to the folders `ids`, named after the first and
/// shown, its Rename dialog open (`FSInventoryCustomTabs::createFolderView`).
pub(in crate::ui::inventory) fn save_as_tab(
    inv: &Inventory,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    account: Uuid,
    ids: &[Uuid],
) -> Option<u64> {
    if !can_save(inv, st, prefs, account, ids) {
        return None;
    }
    let chosen = folders::choosable(inv, ids);
    let name = super::super::folder_name(inv, *chosen.first()?);
    sync(st, prefs);
    let id = add(&mut prefs.custom_tabs, &name, Filters::default(), String::new())?;
    let tab = prefs.custom_tabs.iter_mut().find(|t| t.id == id)?;
    folders::set(tab, account, chosen);
    let text = tab.name.clone();
    select_custom(st, prefs, id);
    st.custom_tabs.rename = Some(Rename { id, text, focus: true });
    Some(id)
}

// ---- Demo ----

/// `AURORA_DEMO_INVENTORY=folder-tabs|aurora-folder-tabs`: a tab scoped to
/// the demo's "Clothing" and "Body" folders and to a deleted one, shown with
/// its Customize dialog open.
pub fn demo(st: &mut InventoryUi, prefs: &mut InventoryPreferences, account: Uuid) {
    prefs.custom_tabs.clear();
    let Some(id) = add(
        &mut prefs.custom_tabs,
        aurora_i18n::tr!("inventory-folder-tab-demo-name"),
        Filters::default(),
        String::new(),
    ) else {
        return;
    };
    if let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) {
        // seed_view's Clothing and Body, and a folder that no longer exists
        let ids = [8203, 8204, 0x00de_1e7e_d000].map(Uuid::from_u128).to_vec();
        folders::set(tab, account, ids);
    }
    select_custom(st, prefs, id);
    customize(st);
}

#[cfg(test)]
mod tests;
