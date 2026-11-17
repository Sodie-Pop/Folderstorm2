//! Folderstorm: the icon of a custom Inventory tab (asked by the human:
//! "custom icons for custom inventory tabs").
//!
//! A custom tab shows the bookmark unless the user picks another icon:
//! right click on the tab › "Icon ›", or the "Icon" row of "Customize tab…".
//! The choice is a fixed list of Tabler icons Aurora embeds (`CHOICES`),
//! kept by name in `custom_tabs[].icon`, left out while it is the default.
//! A name this version does not know (a hand edit, a later version) is kept
//! but drawn as the default.
//!
//! Where it shows: Aurora's narrow tab icons and its wide shortcuts column
//! (in place of the bookmark), and the Legacy tab strip, which is text only:
//! there a chosen icon is drawn before the name (`legacy_label`,
//! `paint_legacy`), and a tab left at the default stays text only, as the
//! built-in tabs are.

use super::*;
use serde::Deserialize as _;

/// The icon of a custom tab nobody chose one for.
pub const DEFAULT: &str = "bookmark-filled";

/// The icons one can choose, each with the key of its name (the tooltip in
/// the grid), in the grid's order. Kept by icon name, never by position.
pub const CHOICES: &[(&str, &str)] = &[
    ("shirt-filled", "inventory-tab-icon-shirt"),
    ("jacket", "inventory-tab-icon-jacket"),
    ("shoe", "inventory-tab-icon-shoe"),
    ("mood-smile-filled", "inventory-tab-icon-face"),
    ("user-filled", "inventory-tab-icon-body"),
    ("walk", "inventory-tab-icon-walk"),
    ("run", "inventory-tab-icon-run"),
    ("wand", "inventory-tab-icon-wand"),
    ("sparkles-filled", "inventory-tab-icon-sparkles"),
    ("cube", "inventory-tab-icon-cube"),
    ("box", "inventory-tab-icon-box"),
    ("package", "inventory-tab-icon-package"),
    ("gift-filled", "inventory-tab-icon-gift"),
    ("home-filled", "inventory-tab-icon-home"),
    ("armchair", "inventory-tab-icon-armchair"),
    ("tree", "inventory-tab-icon-tree"),
    ("leaf-filled", "inventory-tab-icon-leaf"),
    ("paw-filled", "inventory-tab-icon-paw"),
    ("photo-filled", "inventory-tab-icon-photo"),
    ("camera-filled", "inventory-tab-icon-camera"),
    ("palette-filled", "inventory-tab-icon-palette"),
    ("brush", "inventory-tab-icon-brush"),
    ("music", "inventory-tab-icon-music"),
    ("headphones-filled", "inventory-tab-icon-headphones"),
    ("movie", "inventory-tab-icon-movie"),
    ("file-text-filled", "inventory-tab-icon-notecard"),
    ("book-filled", "inventory-tab-icon-book"),
    ("map-pin-filled", "inventory-tab-icon-map-pin"),
    ("compass-filled", "inventory-tab-icon-compass"),
    ("car-filled", "inventory-tab-icon-car"),
    ("key-filled", "inventory-tab-icon-key"),
    ("heart-filled", "inventory-tab-icon-heart"),
    ("flag-filled", "inventory-tab-icon-flag"),
    ("tag-filled", "inventory-tab-icon-tag"),
    ("bolt-filled", "inventory-tab-icon-bolt"),
    ("sun-filled", "inventory-tab-icon-sun"),
    ("moon-filled", "inventory-tab-icon-moon"),
    ("device-gamepad-filled", "inventory-tab-icon-gamepad"),
    ("shopping-cart-filled", "inventory-tab-icon-cart"),
    ("code", "inventory-tab-icon-code"),
];

/// The icons of the grid in a row.
const COLUMNS: usize = 8;

/// The side of the icon drawn before a name in the Legacy tab strip.
const LEGACY_SIDE: f32 = 12.0;

/// The chosen icon, when it is one of `CHOICES` (None: the default).
pub fn chosen(tab: &CustomTab) -> Option<&'static str> {
    let name = tab.icon.as_deref()?;
    CHOICES.iter().map(|(icon, _)| *icon).find(|icon| *icon == name)
}

/// The icon the tab is drawn with.
pub fn of(tab: &CustomTab) -> &'static str {
    chosen(tab).unwrap_or(DEFAULT)
}

/// Sets the tab's icon; None, the default or a name not in `CHOICES`
/// takes the default (nothing kept).
pub fn set(tab: &mut CustomTab, icon: Option<&str>) {
    tab.icon = icon
        .filter(|name| *name != DEFAULT)
        .and_then(|name| CHOICES.iter().find(|(icon, _)| *icon == name))
        .map(|(icon, _)| (*icon).to_owned());
}

/// The icon of the tab strips' slot `slot` (a custom tab's; the default
/// for any other slot).
pub fn of_slot(prefs: &InventoryPreferences, slot: usize) -> &'static str {
    slot.checked_sub(SLOT_BASE)
        .and_then(|n| prefs.custom_tabs.get(n))
        .map_or(DEFAULT, of)
}

/// Reads `custom_tabs[].icon` leniently: a text is kept (trimmed, one
/// line), anything else (a number, a list, an empty text) is no icon, and
/// the rest of the settings load.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().map(clean_name).filter(|name| !name.is_empty()))
}

/// A name from the settings as it is kept (`lenient`, `normalize`).
fn clean_name(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_owned()
}

/// Settings edited by hand: an empty name or the default's goes. Returns
/// whether anything changed.
pub fn normalize(tab: &mut CustomTab) -> bool {
    let Some(name) = tab.icon.as_deref() else {
        return false;
    };
    let clean = clean_name(name);
    if clean.is_empty() || clean == DEFAULT {
        tab.icon = None;
        return true;
    }
    if clean != name {
        tab.icon = Some(clean);
        return true;
    }
    false
}

/// The grid of icons, "Default" first: picks one for the tab `id`. Returns
/// whether one was picked.
fn grid(ui: &mut egui::Ui, p: &Palette, prefs: &mut InventoryPreferences, id: u64) -> bool {
    let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) else {
        return false;
    };
    let current = chosen(tab);
    let mut picked = None;
    egui::Grid::new(("inventory_custom_tab_icons", id))
        .num_columns(COLUMNS)
        .spacing(Vec2::splat(3.0))
        .show(ui, |ui| {
            for (n, (icon, key)) in CHOICES.iter().enumerate() {
                if widgets::icon_toggle(ui, p, icon, aurora_i18n::text(*key), current == Some(*icon)) {
                    picked = Some(Some(*icon));
                }
                if (n + 1) % COLUMNS == 0 {
                    ui.end_row();
                }
            }
        });
    if let Some(icon) = picked {
        set(tab, icon);
        return true;
    }
    false
}

/// "Icon ›" in a custom tab's right-click menu: "Default" (checked while no
/// icon is chosen), then the grid. A choice closes the menu.
pub(super) fn menu_entry(ui: &mut egui::Ui, p: &Palette, prefs: &mut InventoryPreferences, id: u64) {
    let Some(icon) = prefs.custom_tabs.iter().find(|t| t.id == id).map(of) else {
        return;
    };
    menu::submenu(ui, p, icon, aurora_i18n::tr!("inventory-tab-icon"), true, |ui| {
        let default = icon == DEFAULT;
        if menu::check(ui, p, DEFAULT, aurora_i18n::tr!("inventory-tab-icon-default"), default, true)
            && let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id)
        {
            set(tab, None);
        }
        menu::separator(ui, p);
        if grid(ui, p, prefs, id) {
            ui.close();
        }
    });
}

/// The "Icon" row of "Customize tab…", above the folders: the tab's icon
/// now, "Default", and the grid.
pub(in crate::ui::inventory) fn editor(ui: &mut egui::Ui, p: &Palette, prefs: &mut InventoryPreferences, id: u64) {
    let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) else {
        return;
    };
    ui.label(RichText::new(aurora_i18n::tr!("inventory-tab-icon")).color(p.muted));
    let default = chosen(tab).is_none();
    ui.horizontal(|ui| {
        widgets::glyph(ui, of(tab), p.ink, 16.0);
        let resp = ui
            .add_enabled_ui(!default, |ui| flat_button(ui, p, aurora_i18n::tr!("inventory-tab-icon-default")))
            .inner;
        if resp.on_hover_text(aurora_i18n::tr!("inventory-tab-icon-default-tip")).clicked() && !default {
            set(tab, None);
        }
    });
    grid(ui, p, prefs, id);
    widgets::divider(ui, p);
}

/// A Legacy strip's label for a custom tab: its name, after room for its
/// icon when one is chosen (`paint_legacy` draws it there).
pub(super) fn legacy_label(ui: &egui::Ui, tab: &CustomTab) -> String {
    if chosen(tab).is_none() {
        return tab.name.clone();
    }
    let font = legacy_font(ui);
    let space = ui.fonts_mut(|f| f.layout_no_wrap(" ".to_owned(), font, egui::Color32::PLACEHOLDER).size().x);
    let room = ((LEGACY_SIDE + 4.0) / space.max(1.0)).ceil() as usize;
    format!("{}{}", " ".repeat(room.max(1)), tab.name)
}

/// The font of the Legacy strip's labels (`widgets::tabs_with_responses`).
fn legacy_font(ui: &egui::Ui) -> egui::FontId {
    ui.style()
        .override_font_id
        .clone()
        .unwrap_or_else(|| egui::FontId::proportional(12.0))
}

/// Draws a custom tab's chosen icon in the room `legacy_label` left before
/// its name, in the tab's `rect` (`label` as the strip drew it centred).
pub(super) fn paint_legacy(ui: &egui::Ui, p: &Palette, tab: &CustomTab, label: &str, rect: egui::Rect, selected: bool) {
    let Some(icon) = chosen(tab) else {
        return;
    };
    let font = legacy_font(ui);
    let natural = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER).size().x);
    let width = natural.min((rect.width() - 8.0).max(0.0));
    let left = rect.center().x - width / 2.0;
    let center = egui::pos2(left + LEGACY_SIDE / 2.0 + 1.0, rect.center().y);
    let ink = if selected { p.ink } else { p.muted };
    widgets::paint_glyph(ui.painter(), icon, center, LEGACY_SIDE, ink);
}

#[cfg(test)]
mod tests;
