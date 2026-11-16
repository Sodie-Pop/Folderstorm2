//! Folderstorm: a custom Inventory tab as one flat list (feature 2.13,
//! "Inventory quick access").
//!
//! A per-tab switch (`CustomTab::flat`): the tab lists every item it shows,
//! under its folders and its pinned items (pinned first), without folder
//! rows, in the window's sort (`view/folder_scope.rs` builds the list). Each
//! row says its folder: muted at its right when the row leaves room, and in
//! its tooltip.

use super::*;

/// "Flat list" in a custom tab's right-click menu: a check mark while on.
pub(super) fn menu_entry(ui: &mut egui::Ui, p: &Palette, prefs: &mut InventoryPreferences, id: u64) {
    let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) else {
        return;
    };
    let mut on = tab.flat;
    let clicked = menu::toggle(ui, p, "list-filled", aurora_i18n::tr!("inventory-quick-flat"), &mut on);
    if clicked {
        tab.flat = on;
    }
}

/// The same switch in "Customize tab…", under the folders.
pub(super) fn editor_switch(ui: &mut egui::Ui, p: &Palette, prefs: &mut InventoryPreferences, id: u64) {
    let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) else {
        return;
    };
    widgets::checkbox(ui, p, &mut tab.flat, aurora_i18n::tr!("inventory-quick-flat"))
        .on_hover_text(aurora_i18n::tr!("inventory-quick-flat-tip"));
}

/// The folder an item of a flat list is in, as its path.
pub(in crate::ui::inventory) fn folder_path(inv: &Inventory, id: Uuid) -> Option<String> {
    let parent = rules::parent(inv, id).filter(|p| inv.folders.contains_key(p))?;
    Some(super::super::inventory_path(inv, parent))
}

/// After a row of the result list (drawn in `row`): in a flat list, the
/// item's folder in its tooltip, and muted at the row's right when it
/// leaves room.
pub(in crate::ui::inventory) fn row_path(ui: &mut egui::Ui, p: &Palette, inv: &Inventory, st: &InventoryUi, id: Uuid, row: egui::Rect) {
    if st.tab != CUSTOM_TAB || !st.view.flat() || inv.folders.contains_key(&id) {
        return;
    }
    let Some(path) = folder_path(inv, id) else {
        return;
    };
    let tip = aurora_i18n::tr!("inventory-quick-in-folder", path = path.as_str());
    let right = ui.clip_rect().right() - 8.0;
    let left = row.right() + 16.0;
    if right - left > 48.0 {
        let font = egui::FontId::proportional(11.0);
        let mut job = egui::text::LayoutJob::single_section(path, egui::TextFormat::simple(font, p.muted));
        job.wrap = egui::text::TextWrapping::truncate_at_width(right - left);
        let galley = ui.painter().layout_job(job);
        let pos = egui::pos2(right - galley.size().x, row.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, p.muted);
    }
    let sense = ui.interact(
        egui::Rect::from_min_max(row.min, egui::pos2(right.max(row.right()), row.max.y)),
        ui.scope_id().with(("folderstorm_flat_path", id)),
        egui::Sense::hover(),
    );
    sense.on_hover_text(tip);
}
