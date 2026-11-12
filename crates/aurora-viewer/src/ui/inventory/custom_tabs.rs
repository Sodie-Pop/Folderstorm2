//! Folderstorm: custom Inventory tabs.
//!
//! Port of Folderstorm's custom Inventory tabs (doc/help.md, "Custom
//! Inventory tabs"): a tab of its own after the built-in ones, which keeps
//! its filters and its search by itself. "Customize tab…" beside the search
//! box edits the filters of the tab shown; a right click on a tab's name
//! offers Customize tab…, Rename… and Clone (and Delete…); customizing an
//! inactive tab selects it first.
//!
//! What a custom tab remembers is Aurora's own filter model
//! (`view::Filters`, the dialog of "Inventory filters", advanced search
//! rules included) plus the search text; nothing else is invented. Every
//! custom tab is `InventoryUi::tab == CUSTOM_TAB`, a result list over the
//! whole inventory like Recent or Worn; which one is shown is
//! `State::active`. While one is shown, the window's live filters and search
//! are written back to its entry in the preferences each frame (`sync`), so
//! they are saved with the other settings and come back with the tab.
//!
//! The tab strips (the Legacy window's, Aurora's narrow icon tabs and its
//! wide shortcuts column) number their tabs as "slots": a built-in tab is
//! its own number, a custom tab is `SLOT_BASE` plus its place in the list.

use super::view::{Age, Filters, InventoryPreferences};
use super::*;
use crate::ui::widgets::{Floater, flat_button};
use crate::ui::{menu, widgets};
use serde::{Deserialize, Serialize};

/// `InventoryUi::tab` of every custom tab (the built-in tabs are 0 to 4).
pub const CUSTOM_TAB: usize = 5;
/// Slot numbers from here on are custom tabs, by position in the list.
pub const SLOT_BASE: usize = 100;
/// The most custom tabs one can make (the tab strips stay readable).
pub const MAX_TABS: usize = 10;
/// The longest name of a tab, in characters.
pub const MAX_NAME: usize = 40;

/// One custom tab, saved with the inventory preferences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTab {
    /// Never zero, unique among the tabs, kept when the tab is renamed.
    pub id: u64,
    pub name: String,
    pub filters: Filters,
    /// The quick search text (the advanced search keeps its rules in
    /// `filters.smart`, and the text typed before it here).
    pub search: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    /// Another custom tab already has this name (the case is ignored).
    Taken,
    /// The tab to rename is gone.
    Missing,
}

// ---- The list of tabs: plain logic, tested without a window ----

/// A name as it is kept: trimmed, one line, at most `MAX_NAME` characters.
pub fn clean_name(raw: &str) -> String {
    let one_line: String = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    one_line.trim().chars().take(MAX_NAME).collect::<String>().trim_end().to_owned()
}

fn next_id(tabs: &[CustomTab]) -> u64 {
    tabs.iter().map(|t| t.id).max().unwrap_or(0).saturating_add(1)
}

pub fn name_taken(tabs: &[CustomTab], name: &str, except: Option<u64>) -> bool {
    let wanted = name.to_lowercase();
    tabs.iter().any(|t| Some(t.id) != except && t.name.to_lowercase() == wanted)
}

/// `base` if no tab has it, else `base 2`, `base 3`…, within `MAX_NAME`.
pub fn unique_name(tabs: &[CustomTab], base: &str) -> String {
    let base = clean_name(base);
    if !base.is_empty() && !name_taken(tabs, &base, None) {
        return base;
    }
    let base = if base.is_empty() { "Tab".to_owned() } else { base };
    (2_u32..)
        .map(|n| {
            let suffix = format!(" {n}");
            let room = MAX_NAME.saturating_sub(suffix.chars().count());
            format!("{}{suffix}", base.chars().take(room).collect::<String>().trim_end())
        })
        .find(|candidate| !name_taken(tabs, candidate, None))
        .unwrap_or(base)
}

/// A new tab at the end of the list; None at the limit.
pub fn add(tabs: &mut Vec<CustomTab>, name: &str, filters: Filters, search: String) -> Option<u64> {
    if tabs.len() >= MAX_TABS {
        return None;
    }
    let id = next_id(tabs);
    let name = unique_name(tabs, name);
    tabs.push(CustomTab { id, name, filters, search });
    Some(id)
}

pub fn rename(tabs: &mut [CustomTab], id: u64, raw: &str) -> Result<(), NameError> {
    let name = clean_name(raw);
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name_taken(tabs, &name, Some(id)) {
        return Err(NameError::Taken);
    }
    let tab = tabs.iter_mut().find(|t| t.id == id).ok_or(NameError::Missing)?;
    tab.name = name;
    Ok(())
}

/// A copy of the tab right after it, named `copy_name` (made unique).
pub fn clone_tab(tabs: &mut Vec<CustomTab>, id: u64, copy_name: &str) -> Option<u64> {
    if tabs.len() >= MAX_TABS {
        return None;
    }
    let at = tabs.iter().position(|t| t.id == id)?;
    let mut copy = tabs[at].clone();
    copy.id = next_id(tabs);
    copy.name = unique_name(tabs, copy_name);
    let new = copy.id;
    tabs.insert(at + 1, copy);
    Some(new)
}

pub fn delete(tabs: &mut Vec<CustomTab>, id: u64) -> bool {
    let before = tabs.len();
    tabs.retain(|t| t.id != id);
    tabs.len() != before
}

/// Settings edited by hand: ids that are zero or repeated get a new one,
/// empty names a name. Returns whether anything changed.
pub fn normalize(tabs: &mut [CustomTab]) -> bool {
    let mut changed = false;
    let mut seen = std::collections::HashSet::new();
    let mut next = tabs.iter().map(|t| t.id).max().unwrap_or(0);
    for tab in tabs.iter_mut() {
        if tab.id == 0 || !seen.insert(tab.id) {
            next = next.saturating_add(1);
            tab.id = next;
            seen.insert(next);
            changed = true;
        }
        let name = clean_name(&tab.name);
        if name != tab.name || name.is_empty() {
            tab.name = if name.is_empty() { format!("Tab {}", tab.id) } else { name };
            changed = true;
        }
    }
    changed
}

/// The tab's filters and search as the built-in tab `tab` shows them, for
/// "Clone" of a built-in tab. Recent keeps its age (`Age` as in
/// `ViewCache::search`) and Worn its rule; Favorites and the rest are the
/// finder and search as they are.
pub fn from_builtin(tab: usize, mut filters: Filters, search: String, prefs: &InventoryPreferences) -> (Filters, String) {
    use super::smart::{Field, Node, Rule, SmartQuery};
    let mut search = search;
    if tab == 2 && filters.age == Age::Any {
        if prefs.recent_hours == 0 {
            filters.age = Age::SinceLogout;
        } else {
            filters.age = Age::Newer;
            filters.hours = prefs.recent_hours;
        }
    }
    if tab == 3 {
        // Worn is a rule of the advanced search. Quick filters are replaced
        // by its rules, so the rule joins the query only when they would not
        // be lost: the text typed becomes rules, the finder stays as it is.
        if let Some(query) = filters.smart.as_mut() {
            if !query.uses(Field::Worn) {
                query.root.nodes.push(Node::Rule(Rule::of(Field::Worn)));
            }
        } else if !filters.active() {
            let mut query = SmartQuery::from_quick(&search, prefs.search_field);
            query.root.nodes.push(Node::Rule(Rule::of(Field::Worn)));
            filters.smart = Some(query);
            search.clear();
        }
    }
    (filters, search)
}

// ---- The window's state ----

#[derive(Default)]
pub(super) struct State {
    /// The custom tab shown while `InventoryUi::tab == CUSTOM_TAB`.
    active: Option<u64>,
    /// Its name, for the places that have no preferences at hand.
    name: String,
    rename: Option<Rename>,
    delete: Option<(u64, String)>,
}

struct Rename {
    id: u64,
    text: String,
    focus: bool,
}

/// The custom tab shown in this window.
pub(super) fn active(st: &InventoryUi) -> Option<u64> {
    if st.tab == CUSTOM_TAB { st.custom_tabs.active } else { None }
}

/// Its name, for the title of the list.
pub(super) fn name_of(st: &InventoryUi) -> Option<&str> {
    active(st).map(|_| st.custom_tabs.name.as_str())
}

/// The filter editor is for a custom tab: it remembers by itself, so the
/// "keep by default" action of the built-in tabs is not offered.
pub(super) fn editing(st: &InventoryUi) -> bool {
    active(st).is_some()
}

/// The title of "Inventory filters" while a custom tab is shown.
pub(super) fn filters_title(st: &InventoryUi) -> Option<String> {
    name_of(st).map(|name| aurora_i18n::tr!("inventory-custom-tab-filters-title", name = name))
}

fn position(prefs: &InventoryPreferences, id: u64) -> Option<usize> {
    prefs.custom_tabs.iter().position(|t| t.id == id)
}

fn slot_id(prefs: &InventoryPreferences, slot: usize) -> Option<u64> {
    slot.checked_sub(SLOT_BASE).and_then(|n| prefs.custom_tabs.get(n)).map(|t| t.id)
}

/// The custom tabs for a tab strip: their slot and name.
pub(super) fn entries(prefs: &InventoryPreferences) -> Vec<(usize, String)> {
    prefs
        .custom_tabs
        .iter()
        .enumerate()
        .map(|(n, t)| (SLOT_BASE + n, t.name.clone()))
        .collect()
}

pub(super) fn selected(st: &InventoryUi, prefs: &InventoryPreferences, slot: usize) -> bool {
    if slot < SLOT_BASE {
        st.tab == slot
    } else {
        active(st).is_some() && slot_id(prefs, slot) == st.custom_tabs.active
    }
}

/// A click on a tab of a strip (or its being the tab to keep).
pub(super) fn select(st: &mut InventoryUi, prefs: &InventoryPreferences, slot: usize) {
    if slot < SLOT_BASE {
        set_tab(st, prefs, slot);
    } else if let Some(id) = slot_id(prefs, slot) {
        select_custom(st, prefs, id);
    }
}

fn select_custom(st: &mut InventoryUi, prefs: &InventoryPreferences, id: u64) {
    let Some(tab) = prefs.custom_tabs.iter().find(|t| t.id == id) else {
        return;
    };
    if active(st) == Some(id) {
        return;
    }
    if st.tab != CUSTOM_TAB {
        // What was typed stays with the tab left, as `set_tab` does.
        let text = if st.advanced { st.quick_text.clone() } else { st.search.clone() };
        st.searches[st.tab.min(5)] = text;
        st.switch_filter_tab(CUSTOM_TAB);
    } else {
        st.fetch_generation = None;
    }
    st.filters = tab.filters.clone();
    st.filters_initialized = true;
    st.saved_filters = None;
    st.similar = false;
    st.links_filter = None;
    st.active_smart = None;
    st.advanced = st.filters.smart.is_some();
    if st.advanced {
        st.quick_text = tab.search.clone();
        st.search.clear();
    } else {
        st.quick_text.clear();
        st.search = tab.search.clone();
    }
    st.custom_tabs.active = Some(id);
    st.custom_tabs.name.clone_from(&tab.name);
}

/// Writes the live filters and search of the custom tab shown to its entry
/// in the preferences, and leaves for All when the tab is gone (deleted in
/// another window). While Find links or Find similar have set the filters
/// aside, what is shown is temporary and is not kept.
pub(super) fn sync(st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    if st.tab != CUSTOM_TAB {
        return;
    }
    let Some(id) = st.custom_tabs.active else {
        set_tab(st, prefs, 0);
        return;
    };
    let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) else {
        st.custom_tabs.active = None;
        set_tab(st, prefs, 0);
        return;
    };
    if st.custom_tabs.name != tab.name {
        st.custom_tabs.name.clone_from(&tab.name);
    }
    if st.saved_filters.is_some() || st.similar || st.links_filter.is_some() {
        return;
    }
    if tab.filters != st.filters {
        tab.filters.clone_from(&st.filters);
    }
    let text = if st.advanced { &st.quick_text } else { &st.search };
    if tab.search != *text {
        tab.search.clone_from(text);
    }
}

/// "Customize tab…": the filter editor, for the tab shown.
pub(super) fn customize(st: &mut InventoryUi) {
    st.filters_open = true;
}

// ---- Tabs: the right-click menu ----

fn builtin_name(tab: usize) -> &'static str {
    match tab {
        2 => aurora_i18n::tr!("common-recent"),
        3 => aurora_i18n::tr!("common-worn"),
        4 => aurora_i18n::tr!("common-favorites"),
        _ => aurora_i18n::tr!("common-inventory"),
    }
}

/// What the built-in tab `tab` holds now: the live finder and search if it
/// is the one shown, else the finder it was left with (or its defaults).
/// The search box is shared between the tabs unless they keep their own.
fn builtin_snapshot(st: &InventoryUi, prefs: &InventoryPreferences, tab: usize) -> (Filters, String) {
    let live_text = if st.advanced { st.quick_text.clone() } else { st.search.clone() };
    if st.tab == tab {
        return from_builtin(tab, st.filters.clone(), live_text, prefs);
    }
    let parked = &st.tab_filters[tab.min(5)];
    let filters = if parked.filters_initialized {
        parked.filters.clone()
    } else {
        prefs.filters_for(tab).clone()
    };
    let text = if prefs.separate_searches {
        st.searches[tab.min(5)].clone()
    } else if st.tab == CUSTOM_TAB {
        String::new()
    } else {
        live_text
    };
    from_builtin(tab, filters, text, prefs)
}

fn clone_builtin(st: &mut InventoryUi, prefs: &mut InventoryPreferences, tab: usize) {
    sync(st, prefs);
    let (filters, search) = builtin_snapshot(st, prefs, tab);
    let name = aurora_i18n::tr!("inventory-custom-tab-clone-name", name = builtin_name(tab));
    if let Some(id) = add(&mut prefs.custom_tabs, &name, filters, search) {
        select_custom(st, prefs, id);
    }
}

fn clone_custom(st: &mut InventoryUi, prefs: &mut InventoryPreferences, id: u64) {
    sync(st, prefs);
    let Some(name) = prefs.custom_tabs.iter().find(|t| t.id == id).map(|t| t.name.clone()) else {
        return;
    };
    let name = aurora_i18n::tr!("inventory-custom-tab-clone-name", name = name.as_str());
    if let Some(copy) = clone_tab(&mut prefs.custom_tabs, id, &name) {
        select_custom(st, prefs, copy);
    }
}

/// Whether a right click on the tab has a menu: the built-in tabs offer
/// Clone, except Received (its items are not the inventory's own).
pub(super) fn has_menu(slot: usize) -> bool {
    slot >= SLOT_BASE || matches!(slot, 0 | 2 | 3 | 4)
}

pub(super) fn tab_menu(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences, slot: usize) {
    let room = prefs.custom_tabs.len() < MAX_TABS;
    let clone_tip = aurora_i18n::tr!("inventory-custom-tab-limit", count = MAX_TABS);
    let Some(id) = slot_id(prefs, slot) else {
        if slot < SLOT_BASE
            && has_menu(slot)
            && menu::item_tip(
                ui,
                p,
                "copy-filled",
                aurora_i18n::tr!("inventory-custom-tab-clone"),
                room,
                if room {
                    aurora_i18n::tr!("inventory-custom-tab-clone-builtin-tip")
                } else {
                    clone_tip.as_str()
                },
            )
        {
            clone_builtin(st, prefs, slot);
        }
        return;
    };
    if menu::item(ui, p, "adjustments-filled", aurora_i18n::tr!("inventory-custom-tab-customize")) {
        select_custom(st, prefs, id);
        customize(st);
    }
    if menu::item(ui, p, "pencil-filled", aurora_i18n::tr!("inventory-custom-tab-rename")) {
        let text = prefs
            .custom_tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        st.custom_tabs.rename = Some(Rename { id, text, focus: true });
    }
    if menu::item_tip(
        ui,
        p,
        "copy-filled",
        aurora_i18n::tr!("inventory-custom-tab-clone"),
        room,
        if room {
            aurora_i18n::tr!("inventory-custom-tab-clone-tip")
        } else {
            clone_tip.as_str()
        },
    ) {
        clone_custom(st, prefs, id);
    }
    menu::separator(ui, p);
    if menu::item(ui, p, "trash-filled", aurora_i18n::tr!("inventory-custom-tab-delete")) {
        let name = prefs
            .custom_tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        st.custom_tabs.delete = Some((id, name));
    }
}

// ---- The Legacy window ----

/// The Legacy window's tab strip: the built-in `tab_ids` (labelled by
/// `labels`) and then the custom tabs, each with its right-click menu.
pub(super) fn legacy_strip(
    ui: &mut egui::Ui,
    p: &Palette,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    tab_ids: &[usize],
    labels: &[(&str, bool)],
) {
    let custom = entries(prefs);
    let mut slots = tab_ids.to_vec();
    let mut all = labels.to_vec();
    for (slot, name) in &custom {
        slots.push(*slot);
        all.push((name.as_str(), true));
    }
    let mut visible = slots.iter().position(|slot| selected(st, prefs, *slot)).unwrap_or(0);
    let (_, responses) = widgets::tabs_with_responses(ui, p, &mut visible, &all);
    // the tab clicked (or the one kept) first: a menu choice below may select another
    select(st, prefs, slots[visible]);
    for (slot, response) in slots.iter().zip(&responses) {
        if has_menu(*slot) {
            menu::context_menu(response, p, |ui| tab_menu(ui, p, st, prefs, *slot));
        }
    }
}

/// The room "Customize tab…" takes beside the search box of the Legacy
/// window (none unless a custom tab is shown).
pub(super) fn legacy_button_width(ui: &egui::Ui, st: &InventoryUi) -> f32 {
    if active(st).is_none() {
        return 0.0;
    }
    let label = aurora_i18n::tr!("inventory-custom-tab-customize");
    let want = label.chars().count() as f32 * 7.0 + 28.0;
    want.min(ui.available_width() * 0.4) + ui.spacing().item_spacing.x
}

/// "Customize tab…" beside the search box of the Legacy window.
pub(super) fn legacy_button(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi) {
    if active(st).is_none() {
        return;
    }
    if flat_button(ui, p, aurora_i18n::tr!("inventory-custom-tab-customize"))
        .on_hover_text(aurora_i18n::tr!("inventory-custom-tab-customize-tip"))
        .clicked()
    {
        customize(st);
    }
}

// ---- Dialogs ----

/// Once a frame, after the window: keeps the tab shown (`sync`) and shows
/// the Rename and Delete dialogs.
pub(super) fn frame(ctx: &egui::Context, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    normalize(&mut prefs.custom_tabs);
    sync(st, prefs);
    rename_dialog(ctx, p, st, prefs);
    delete_dialog(ctx, p, st, prefs);
}

fn rename_dialog(ctx: &egui::Context, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    let Some(mut dialog) = st.custom_tabs.rename.take() else {
        return;
    };
    if position(prefs, dialog.id).is_none() {
        return;
    }
    let mut open = true;
    let mut accept = false;
    let mut cancel = false;
    let window_id = format!("inventory_custom_tab_rename_{}", st.ui_id);
    Floater::new(
        &window_id,
        aurora_i18n::tr!("inventory-custom-tab-rename-title"),
        ctx.content_rect().center() - egui::vec2(180.0, 60.0),
        Vec2::new(360.0, 130.0),
    )
    .fixed()
    .show(ctx, p, &mut open, |ui| {
        ui.label(RichText::new(aurora_i18n::tr!("inventory-custom-tab-name-label")).color(p.muted));
        let field = ui.add(
            egui::TextEdit::singleline(&mut dialog.text)
                .char_limit(MAX_NAME)
                .desired_width(ui.available_width()),
        );
        if dialog.focus {
            field.request_focus();
            dialog.focus = false;
        }
        let name = clean_name(&dialog.text);
        let problem = if name.is_empty() {
            Some(aurora_i18n::tr!("inventory-custom-tab-name-empty"))
        } else if name_taken(&prefs.custom_tabs, &name, Some(dialog.id)) {
            Some(aurora_i18n::tr!("inventory-custom-tab-name-taken"))
        } else {
            None
        };
        if let Some(problem) = problem {
            ui.label(RichText::new(problem).size(12.0).color(p.warn));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(problem.is_none(), |ui| {
                if flat_button(ui, p, aurora_i18n::tr!("common-rename")).clicked() {
                    accept = true;
                }
            });
            if flat_button(ui, p, aurora_i18n::tr!("common-cancel")).clicked() {
                cancel = true;
            }
        });
        if field.has_focus() || field.lost_focus() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && problem.is_none() {
                accept = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        }
    });
    if accept {
        // a name taken in the meantime keeps the dialog open
        if rename(&mut prefs.custom_tabs, dialog.id, &dialog.text).is_ok() {
            sync(st, prefs);
            return;
        }
    }
    if open && !cancel {
        st.custom_tabs.rename = Some(dialog);
    }
}

fn delete_dialog(ctx: &egui::Context, p: &Palette, st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    let Some((id, name)) = st.custom_tabs.delete.take() else {
        return;
    };
    if position(prefs, id).is_none() {
        return;
    }
    let mut open = true;
    let mut cancel = false;
    let mut accept = false;
    let window_id = format!("inventory_custom_tab_delete_{}", st.ui_id);
    Floater::new(
        &window_id,
        aurora_i18n::tr!("inventory-custom-tab-delete-title"),
        ctx.content_rect().center() - egui::vec2(210.0, 70.0),
        Vec2::new(420.0, 140.0),
    )
    .fixed()
    .show(ctx, p, &mut open, |ui| {
        ui.label(aurora_i18n::tr!("inventory-custom-tab-delete-question", name = name.as_str()));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            // Cancel first and focused: Enter or Escape never delete
            let cancel_button = flat_button(ui, p, aurora_i18n::tr!("common-cancel"));
            if !ui.memory(|m| m.focused().is_some()) {
                cancel_button.request_focus();
            }
            if cancel_button.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            if flat_button(ui, p, aurora_i18n::tr!("common-delete")).clicked() {
                accept = true;
            }
        });
    });
    if accept {
        delete_and_leave(st, prefs, id);
        return;
    }
    if open && !cancel {
        st.custom_tabs.delete = Some((id, name));
    }
}

/// Deletes the tab; this window goes back to All if it showed it.
fn delete_and_leave(st: &mut InventoryUi, prefs: &mut InventoryPreferences, id: u64) {
    if active(st) == Some(id) {
        st.custom_tabs.active = None;
        set_tab(st, prefs, 0);
    }
    delete(&mut prefs.custom_tabs, id);
}

// ---- Demo ----

/// `AURORA_DEMO_INVENTORY=custom-tabs|aurora-custom-tabs`: two sample custom
/// tabs, the first one shown: the demo's textures and sounds, and what has
/// "demo" in its name.
pub fn demo(st: &mut InventoryUi, prefs: &mut InventoryPreferences) {
    prefs.custom_tabs.clear();
    let media = Filters {
        types: (1 << 0) | (1 << 1),
        ..Filters::default()
    };
    let first = add(
        &mut prefs.custom_tabs,
        aurora_i18n::tr!("inventory-custom-tab-demo-media"),
        media,
        String::new(),
    );
    add(
        &mut prefs.custom_tabs,
        aurora_i18n::tr!("inventory-custom-tab-demo-named"),
        Filters::default(),
        "demo".into(),
    );
    if let Some(id) = first {
        select_custom(st, prefs, id);
    }
}

#[cfg(test)]
mod tests;
