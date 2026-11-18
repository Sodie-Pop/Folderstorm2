//! Folderstorm: the folder scope of a custom Inventory tab, on screen
//! (feature 2.2; the rules are in `folders.rs`).
//!
//! - right click on folders › "Create tab": a new custom tab scoped to them,
//!   named after the first, its Rename dialog open (Folderstorm's gear menu
//!   "Save selected folders as view…"); "Add to ›" lists the custom tabs,
//!   and the folders join the one picked (the entries are built in
//!   `context_catalog/folder_tabs.rs`);
//! - folders dragged from the list onto a custom tab's button join it too;
//! - right click on a custom tab › "Add the selection", "Use only the
//!   selection" (Folderstorm's tab menu; its "Show all folders" is not
//!   offered there: the tab menu's "Show all items" clears the filters and
//!   keeps the folders, `custom_tabs.rs`);
//! - "Customize tab…" lists the tab's folders by path, each with a remove
//!   button, and offers the same two actions and "Remove all…", which asks
//!   first;
//! - a note above the list when some of the tab's folders are gone, or when
//!   it is scoped on another account only.
//!
//! Items go into a tab the same ways (feature 2.13, `items.rs`): they are
//! pinned, the folders of the same selection join the scope.
//!
//! What these need from the inventory (the selection that may be chosen, the
//! paths and states of the tab's folders) is read once a frame by
//! `before_list`, before the list is searched; the menus and the dialog,
//! drawn where the inventory is not at hand, read it from there.

use super::folders::{self, Change, Chosen, MAX_FOLDERS, Refusal, Status};
use super::items::{self, MAX_ITEMS};
use super::*;

/// What the window saw of the inventory this frame (`before_list`).
#[derive(Default)]
pub(in crate::ui::inventory) struct Seen {
    /// The account whose folders the tabs show (nil: none yet).
    account: Uuid,
    /// The inventory is there (its root known).
    ready: bool,
    /// The window's selection, when it may go into a tab.
    choosable: Chosen,
    /// The names of `choosable`, folders first.
    names: Vec<String>,
    /// The tab shown, its folders then its pinned items on this account.
    rows: Vec<Row>,
    /// Its folders and items and the inventory generation `rows` were read
    /// from.
    key: Option<(u64, u64, Vec<Uuid>, Vec<Uuid>)>,
    /// It is scoped on another account only.
    elsewhere: bool,
    /// `AURORA_DEMO_FOLDER_TABS=menu`: lists drawn before the demo opens
    /// the folder menu (once the window has settled).
    demo_menu: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::inventory) struct Row {
    pub id: Uuid,
    /// A folder's path; a pinned item's name, then its folder's path.
    pub path: String,
    pub status: Status,
    /// A pinned item (else a folder).
    pub item: bool,
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
    demo_menu(st);
    super::window::keep(st, prefs);
    let seen = &mut st.custom_tabs.seen;
    seen.account = account;
    seen.ready = !inv.root.is_nil();
    // a folder window has no custom tabs (Folderstorm: not in single-folder mode)
    seen.choosable = if st.scope.is_nil() && st.selection.len() <= MAX_FOLDERS + MAX_ITEMS && !account.is_nil() {
        let selection: Vec<Uuid> = st.selection.iter().copied().collect();
        folders::choosable(inv, &selection)
    } else {
        Chosen::default()
    };
    seen.names = seen.choosable.all().map(|id| entry_name(inv, id)).collect();
    let tab = shown_tab(st, prefs);
    let saved = tab.map(|t| folders::of(t, account).to_vec()).unwrap_or_default();
    let pinned = tab.map(|t| items::of(t, account).to_vec()).unwrap_or_default();
    let key = (inv.generation, tab.map_or(0, |t| t.id), saved.clone(), pinned.clone());
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
                Row {
                    id: *id,
                    path,
                    status,
                    item: false,
                }
            })
            .chain(pinned.iter().map(|id| {
                let status = items::status(inv, *id);
                let path = if status == Status::Missing {
                    aurora_i18n::tr!("inventory-folder-tab-deleted-item").to_owned()
                } else {
                    super::super::inventory_path(inv, *id)
                };
                Row {
                    id: *id,
                    path,
                    status,
                    item: true,
                }
            }))
            .collect();
        seen.key = Some(key);
    }
    let roots = tab.and_then(|t| folders::search_roots(inv, t, account));
    let pins = tab.map(|t| items::search_pins(inv, t, account)).unwrap_or_default();
    let flat = tab.is_some_and(|t| t.flat);
    st.view.set_tab_scope(roots, pins, flat);
}

/// A folder's name, or an item's.
fn entry_name(inv: &Inventory, id: Uuid) -> String {
    if inv.folders.contains_key(&id) {
        super::super::folder_name(inv, id)
    } else {
        rules::name(inv, id)
    }
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
    if how == Change::Add {
        st.message = add_chosen(tab, account, &chosen, &st.custom_tabs.seen.names);
        return;
    }
    if let Err(limit) = items::apply(tab, account, &chosen, how) {
        st.message = limit_text(limit);
    }
}

/// Why a tab cannot take more.
fn limit_text(limit: items::Limit) -> String {
    match limit {
        items::Limit::Folders => aurora_i18n::tr!("inventory-folder-tab-limit", count = MAX_FOLDERS),
        items::Limit::Items => aurora_i18n::tr!("inventory-folder-tab-item-limit", count = MAX_ITEMS),
    }
}

/// Adds the `chosen` folders and items to the tab (those it has already
/// are not repeated) and says what was done, for the status line. `names`
/// are their names, used when there is one.
fn add_chosen(tab: &mut CustomTab, account: Uuid, chosen: &Chosen, names: &[String]) -> String {
    let name = tab.name.clone();
    let name = name.as_str();
    match items::would_add(tab, account, chosen) {
        Err(limit) => limit_text(limit),
        Ok(added) if added.new == 0 => match names {
            [folder] => aurora_i18n::tr!("inventory-folder-tab-already-one", folder = folder.as_str(), tab = name),
            _ => aurora_i18n::tr!("inventory-folder-tab-already-many", tab = name),
        },
        Ok(_) => {
            let Ok(added) = items::apply(tab, account, chosen, Change::Add) else {
                return String::new();
            };
            let done = match names {
                [folder] => aurora_i18n::tr!("inventory-folder-tab-added-one", folder = folder.as_str(), tab = name),
                _ => aurora_i18n::tr!("inventory-folder-tab-added-many", count = added.new, tab = name),
            };
            if added.already == 0 {
                done
            } else {
                let already = aurora_i18n::tr!("inventory-folder-tab-added-already", count = added.already);
                format!("{done} {already}")
            }
        }
    }
}

/// Removes the folders and pinned items `gone` says from the tab.
fn remove(st: &InventoryUi, prefs: &mut InventoryPreferences, id: u64, gone: impl Fn(&Uuid) -> bool) {
    let account = st.custom_tabs.seen.account;
    if let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) {
        let kept = folders::of(tab, account).iter().copied().filter(|f| !gone(f)).collect();
        folders::set(tab, account, kept);
        let kept = items::of(tab, account).iter().copied().filter(|f| !gone(f)).collect();
        items::set(tab, account, kept);
    }
}

/// The entries of a custom tab's menu that change its folders and pins
/// (and of its Customize dialog, which adds "Remove all…").
fn actions(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences, id: u64, in_menu: bool) {
    let seen = &st.custom_tabs.seen;
    let can_choose = !seen.choosable.is_empty();
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
    if !in_menu {
        remove_all_button(ui, p, st, prefs, id);
    }
}

/// "Remove all…" in the Customize dialog, on a line of its own: asks before
/// the tab forgets its folders and pins on this account.
fn remove_all_button(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &InventoryPreferences, id: u64) {
    let Some(tab) = prefs.custom_tabs.iter().find(|t| t.id == id) else {
        return;
    };
    let scoped = folders::scoped(tab, st.custom_tabs.seen.account);
    ui.end_row();
    let tip = aurora_i18n::tr!("inventory-folder-tab-remove-all-tip");
    let resp = ui
        .add_enabled_ui(scoped, |ui| flat_button(ui, p, aurora_i18n::tr!("inventory-folder-tab-remove-all")))
        .inner
        .on_hover_text(tip)
        .on_disabled_hover_text(tip);
    if resp.clicked() && scoped {
        st.custom_tabs.unscope = Some((id, tab.name.clone()));
    }
}

/// The question of "Remove all…": once confirmed, the tab forgets its
/// folders and pinned items on this account and shows the whole inventory,
/// with its filters and search.
pub(super) fn remove_all_dialog(ctx: &egui::Context, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    let Some((id, name)) = st.custom_tabs.unscope.take() else {
        return;
    };
    if position(prefs, id).is_none() {
        return;
    }
    let answer = confirm(
        ctx,
        p,
        &format!("inventory_custom_tab_unscope_{}", st.ui_id),
        aurora_i18n::tr!("inventory-folder-tab-remove-all-title"),
        &aurora_i18n::tr!("inventory-folder-tab-remove-all-question", name = name.as_str()),
        aurora_i18n::tr!("inventory-folder-tab-remove-all-confirm"),
    );
    match answer {
        Some(true) => apply(st, prefs, id, Change::Clear),
        Some(false) => {}
        None => st.custom_tabs.unscope = Some((id, name)),
    }
}

/// The entries of a custom tab's right-click menu, after Clone.
pub(super) fn tab_menu(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences, id: u64) {
    menu::separator(ui, p);
    actions(ui, p, st, prefs, id, true);
}

/// The "Icon" and "Folders" parts of "Customize tab…", above the filters.
pub(in crate::ui::inventory) fn editor(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    let Some(id) = active(st) else {
        return;
    };
    super::icon::editor(ui, p, prefs, id);
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
            if row.item {
                widgets::glyph(ui, "pinned-filled", p.muted, 13.0);
            }
            let color = if row.status == Status::Live { p.ink } else { p.muted };
            ui.add(egui::Label::new(RichText::new(&row.path).color(color)).truncate())
                .on_hover_text(if row.item {
                    format!("{}\n{}", aurora_i18n::tr!("inventory-quick-pinned"), row.path)
                } else {
                    row.path.clone()
                });
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
    ui.horizontal_wrapped(|ui| {
        // a button too long for the line goes to the next one, whole
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        actions(ui, p, st, prefs, id, false);
    });
    ui.label(
        RichText::new(aurora_i18n::tr!("inventory-folder-tab-help", count = MAX_FOLDERS))
            .size(12.0)
            .color(p.muted),
    );
    super::flat::editor_switch(ui, p, prefs, id);
    widgets::divider(ui, p);
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

// ---- The folder menu: Create tab, Add to › ----

/// The text of a refusal, for the tooltip of a greyed entry or the status
/// line.
fn refusal_text(why: Refusal) -> String {
    match why {
        Refusal::Empty => aurora_i18n::tr!("inventory-folder-tab-why-empty").to_owned(),
        Refusal::Broken => aurora_i18n::tr!("inventory-folder-tab-why-broken").to_owned(),
        Refusal::NotAllowed => aurora_i18n::tr!("inventory-folder-tab-why-refused").to_owned(),
        Refusal::TooMany => aurora_i18n::tr!("inventory-folder-tab-limit", count = MAX_FOLDERS),
        Refusal::TooManyItems => aurora_i18n::tr!("inventory-folder-tab-item-limit", count = MAX_ITEMS),
    }
}

/// The folders and items of `ids` that can go into a tab, or why none can:
/// not in a folder window, on a known account, live entries only
/// (`folders::check`).
fn chosen(inv: &Inventory, st: &InventoryUi, account: Uuid, ids: &[Uuid]) -> Result<Chosen, String> {
    if account.is_nil() || inv.root.is_nil() {
        return Err(aurora_i18n::tr!("inventory-folder-tab-why-loading").to_owned());
    }
    if !st.scope.is_nil() {
        // a folder window has no custom tabs (Folderstorm: not in single-folder mode)
        return Err(aurora_i18n::tr!("inventory-folder-tab-why-window").to_owned());
    }
    folders::check(inv, ids).map_err(refusal_text)
}

/// One custom tab under "Add to ›".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::inventory) struct TabOffer {
    pub id: u64,
    pub name: String,
    /// It has every folder and item of the selection already (a check mark;
    /// picking it says so).
    pub has_all: bool,
    /// Why the folders cannot join it (greyed), if they cannot.
    pub refused: Option<String>,
}

/// What a folder's right-click menu offers for the selection `ids`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::inventory) struct Offer {
    /// "Create tab" (and "New tab…"): allowed, or why not.
    pub create: Result<(), String>,
    /// "Add to ›" as a whole: allowed, or why not.
    pub add: Result<(), String>,
    /// The custom tabs, in their order.
    pub tabs: Vec<TabOffer>,
}

pub(in crate::ui::inventory) fn offer(
    inv: &Inventory,
    st: &InventoryUi,
    prefs: &InventoryPreferences,
    account: Uuid,
    ids: &[Uuid],
) -> Offer {
    let chosen = chosen(inv, st, account, ids);
    let create = match &chosen {
        Err(why) => Err(why.clone()),
        Ok(_) if prefs.custom_tabs.len() >= MAX_TABS => Err(aurora_i18n::tr!("inventory-custom-tab-limit", count = MAX_TABS)),
        Ok(_) => Ok(()),
    };
    let tabs = prefs
        .custom_tabs
        .iter()
        .map(|tab| {
            let (has_all, refused) = match &chosen {
                Err(why) => (false, Some(why.clone())),
                Ok(chosen) => match items::would_add(tab, account, chosen) {
                    Ok(added) => (added.new == 0, None),
                    Err(limit) => (false, Some(limit_text(limit))),
                },
            };
            TabOffer {
                id: tab.id,
                name: tab.name.clone(),
                has_all,
                refused,
            }
        })
        .collect();
    Offer {
        create,
        add: chosen.map(|_| ()),
        tabs,
    }
}

/// A new custom tab scoped to the folders of `ids` and pinning its items,
/// named after the first folder (or item) and shown, its Rename dialog open
/// (`FSInventoryCustomTabs::createFolderView`).
pub(in crate::ui::inventory) fn save_as_tab(
    inv: &Inventory,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    account: Uuid,
    ids: &[Uuid],
) -> Option<u64> {
    if let Err(why) = offer(inv, st, prefs, account, ids).create {
        st.message = why;
        return None;
    }
    let chosen = folders::choosable(inv, ids);
    let name = entry_name(inv, chosen.all().next()?);
    sync(st, prefs);
    let id = add(&mut prefs.custom_tabs, &name, Filters::default(), String::new())?;
    let tab = prefs.custom_tabs.iter_mut().find(|t| t.id == id)?;
    folders::set(tab, account, chosen.folders);
    items::set(tab, account, chosen.items);
    let text = tab.name.clone();
    select_custom(st, prefs, id);
    st.custom_tabs.rename = Some(Rename { id, text, focus: true });
    Some(id)
}

/// "Add to › <tab>": the folders of `ids` join the tab `id` and its items
/// are pinned (those it has already are not repeated), and the status line
/// says what was done or why not. The tab shown stays.
pub(in crate::ui::inventory) fn add_to_tab(
    inv: &Inventory,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    account: Uuid,
    id: u64,
    ids: &[Uuid],
) {
    let chosen = match chosen(inv, st, account, ids) {
        Ok(chosen) => chosen,
        Err(why) => {
            st.message = why;
            return;
        }
    };
    let names: Vec<String> = chosen.all().map(|id| entry_name(inv, id)).collect();
    if let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) {
        st.message = add_chosen(tab, account, &chosen, &names);
    }
}

// ---- Folders dragged onto a tab ----

/// A custom tab's button in a tab strip (`slot`, its `response`) takes the
/// folders and items dragged from the list: they join the tab as with
/// "Add to ›".
/// The outline and the cursor say whether it would take them.
pub(in crate::ui::inventory) fn drop_on_tab(
    ui: &egui::Ui,
    p: &Palette,
    inv: &Inventory,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    slot: usize,
    response: &egui::Response,
) {
    let Some(id) = slot_id(prefs, slot) else {
        return;
    };
    let Some((ids, released)) = super::super::drag::give_target(ui, response.rect) else {
        return;
    };
    let account = st.custom_tabs.seen.account;
    let allowed = offer(inv, st, prefs, account, &ids)
        .tabs
        .iter()
        .any(|t| t.id == id && t.refused.is_none());
    if !allowed {
        super::super::drag::feedback(ui.ctx(), egui::CursorIcon::NotAllowed);
    }
    super::super::tree::drop_outline(ui, p, response.rect, allowed);
    if released {
        add_to_tab(inv, st, prefs, account, id, &ids);
    }
}

// ---- Demo ----

/// The demo's Clothing and Body folders (`seed_view`).
const DEMO_FOLDERS: [u128; 2] = [8203, 8204];

fn demo_menu(st: &mut InventoryUi) {
    match st.custom_tabs.seen.demo_menu {
        Some(0) => {
            st.custom_tabs.seen.demo_menu = None;
            st.selection = DEMO_FOLDERS.map(Uuid::from_u128).into();
            st.demo_menu = Some(Uuid::from_u128(DEMO_FOLDERS[0]));
        }
        Some(n) => st.custom_tabs.seen.demo_menu = Some(n - 1),
        None => {}
    }
}

/// `AURORA_DEMO_INVENTORY=folder-tabs|aurora-folder-tabs`: a tab scoped to
/// the demo's "Clothing" and "Body" folders and to a deleted one, shown with
/// its Customize dialog open. With `AURORA_DEMO_FOLDER_TABS=menu`, a second
/// tab, and the Inventory tab shown with those two folders selected and
/// their right-click menu open.
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
        let ids = [DEMO_FOLDERS[0], DEMO_FOLDERS[1], 0x00de_1e7e_d000].map(Uuid::from_u128).to_vec();
        folders::set(tab, account, ids);
    }
    if std::env::var("AURORA_DEMO_FOLDER_TABS").is_ok_and(|v| v == "menu") {
        add(
            &mut prefs.custom_tabs,
            aurora_i18n::tr!("inventory-folder-tab-demo-other"),
            Filters::default(),
            String::new(),
        );
        st.custom_tabs.seen.demo_menu = Some(650);
        return;
    }
    select_custom(st, prefs, id);
    customize(st);
}

#[cfg(test)]
mod tests;
