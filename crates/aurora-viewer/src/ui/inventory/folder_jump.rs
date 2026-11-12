//! Folderstorm: "Go to folder…" (Ctrl+J), a searchable list of every
//! inventory folder by its name or its parent path.
//!
//! Port of Folderstorm's FSFloaterFolderJump (fsfloaterfolderjump.cpp,
//! Folderstorm contributors, originally LGPL 2.1): the same ranking (exact
//! name, then name prefix, then every word in the name, then words found in
//! the parent path), at most 60 results, the top-level folders while the
//! search is empty, Trash and the Library left out. Opening a folder opens it
//! in a folder window, so the inventory's own search stays as it is; inside a
//! folder window, the window moves to that folder instead.
//!
//! The list itself follows Aurora's "Move to…" picker (`move_to.rs`).

use super::*;
use crate::ui::widgets::{Floater, above_footer, flat_button, search_field};
use rayon::prelude::*;

/// The most results listed; typing more narrows the list.
pub(super) const MAX_RESULTS: usize = 60;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub(super) id: Uuid,
    pub(super) name: String,
    /// "A / B" of the parents below My Inventory; empty for a top-level folder.
    pub(super) parent_path: String,
    folded_name: String,
    folded_path: String,
}

impl Entry {
    fn top_level(&self) -> bool {
        self.parent_path.is_empty()
    }
}

pub(super) struct FolderJump {
    pub(super) query: String,
    focus: bool,
    selected: Option<Uuid>,
    generation: Option<u64>,
    entries: Vec<Entry>,
    results: Vec<usize>,
    total: usize,
    shown_query: Option<String>,
}

impl FolderJump {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            focus: true,
            selected: None,
            generation: None,
            entries: Vec::new(),
            results: Vec::new(),
            total: 0,
            shown_query: None,
        }
    }

    fn update(&mut self, inv: &Inventory) {
        let regenerated = self.generation != Some(inv.generation);
        if regenerated {
            self.entries = index(inv);
            self.generation = Some(inv.generation);
        }
        let query = self.query.trim().to_lowercase();
        let new_query = self.shown_query.as_ref() != Some(&query);
        if new_query || regenerated {
            (self.results, self.total) = search(&self.entries, &query);
            let still_listed = self
                .selected
                .is_some_and(|id| self.results.iter().any(|&i| self.entries[i].id == id));
            // a new search selects its best match (FSFloaterFolderJump's
            // mChooseFirst); an inventory update keeps the chosen row
            if new_query || !still_listed {
                self.selected = self.results.first().map(|&i| self.entries[i].id);
            }
            self.shown_query = Some(query);
        }
    }

    fn step(&mut self, down: bool) {
        let at = self
            .selected
            .and_then(|id| self.results.iter().position(|&i| self.entries[i].id == id));
        let next = match (at, down) {
            (None, _) => 0,
            (Some(i), true) => (i + 1).min(self.results.len().saturating_sub(1)),
            (Some(i), false) => i.saturating_sub(1),
        };
        self.selected = self.results.get(next).map(|&i| self.entries[i].id);
    }
}

/// Every folder of My Inventory with its parent path, Trash and its contents
/// excepted (FSFloaterFolderJump::extendIndex).
pub(super) fn index(inv: &Inventory) -> Vec<Entry> {
    let trash = rules::system(inv, 14);
    let mut entries = Vec::new();
    let mut seen = HashSet::from([inv.root]);
    // (folder, its path below My Inventory)
    let mut queue = std::collections::VecDeque::from([(inv.root, String::new())]);
    while let Some((parent, path)) = queue.pop_front() {
        let Some(folder) = inv.folders.get(&parent) else { continue };
        for &child in &folder.children {
            if Some(child) == trash || !seen.insert(child) {
                continue;
            }
            let Some(info) = inv.folders.get(&child) else { continue };
            if info.library {
                continue;
            }
            let name = folder_name(inv, child);
            let child_path = if path.is_empty() {
                name.clone()
            } else {
                format!("{path} / {name}")
            };
            entries.push(Entry {
                id: child,
                folded_name: name.to_lowercase(),
                folded_path: path.to_lowercase(),
                parent_path: path.clone(),
                name,
            });
            queue.push_back((child, child_path));
        }
    }
    entries
}

/// FSFloaterFolderJump's matchRank: lower is better, `None` is no match.
fn rank(entry: &Entry, query: &str, words: &[&str]) -> Option<u8> {
    if entry.folded_name == query {
        return Some(0);
    }
    if entry.folded_name.starts_with(query) {
        return Some(1);
    }
    let mut all_in_name = true;
    for word in words {
        if entry.folded_name.contains(word) {
            continue;
        }
        if !entry.folded_path.contains(word) {
            return None;
        }
        all_in_name = false;
    }
    Some(if all_in_name { 2 } else { 3 })
}

/// The indices of the listed entries, best first, and how many matched in
/// all. An empty query lists the top-level folders.
pub(super) fn search(entries: &[Entry], query: &str) -> (Vec<usize>, usize) {
    let words: Vec<&str> = query.split([' ', '/', '\t']).filter(|w| !w.is_empty()).collect();
    let mut matches: Vec<(u8, usize)> = entries
        .par_iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            if words.is_empty() {
                entry.top_level().then_some((0, i))
            } else {
                rank(entry, query, &words).map(|r| (r, i))
            }
        })
        .collect();
    let total = matches.len();
    let key = |&(r, i): &(u8, usize)| {
        let e = &entries[i];
        (r, e.folded_name.clone(), e.folded_path.clone(), e.id)
    };
    if matches.len() > MAX_RESULTS {
        matches.select_nth_unstable_by_key(MAX_RESULTS - 1, key);
        matches.truncate(MAX_RESULTS);
    }
    matches.sort_unstable_by_key(key);
    (matches.into_iter().map(|(_, i)| i).collect(), total)
}

/// Opens the dialog for this inventory window (gear menu, Ctrl+J).
pub(super) fn open(st: &mut InventoryUi) {
    st.folder_jump = Some(FolderJump::new());
}

/// The Legacy style's entry, beside Collapse and Expand.
pub(super) fn legacy_button(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi) {
    if crate::ui::widgets::underlined_button(ui, p, aurora_i18n::tr!("inventory-folder-jump-menu"), st.folder_jump.is_some())
        .on_hover_text(aurora_i18n::tr!("inventory-folder-jump-menu-tip"))
        .clicked()
    {
        open(st);
    }
}

/// The Aurora style's entry, in the toolbar's menu.
pub(super) fn menu_item(ui: &mut egui::Ui, p: &Palette, st: &mut InventoryUi) {
    if crate::ui::menu::item_tip(
        ui,
        p,
        "folder-open-filled",
        aurora_i18n::tr!("inventory-folder-jump-menu"),
        true,
        aurora_i18n::tr!("inventory-folder-jump-menu-tip"),
    ) {
        open(st);
    }
}

/// Ctrl+J (Cmd+J on macOS) in the front inventory window.
pub(super) fn shortcut(ui: &egui::Ui, st: &mut InventoryUi) {
    if st.folder_jump.is_some() || ui.ctx().top_layer_id() != Some(ui.layer_id()) {
        return;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::J)) {
        open(st);
    }
}

/// The folder window that shows `id`: this window when it already lists a
/// single folder (FSFloaterFolderJump::openSelected's single folder mode),
/// a new folder window otherwise.
fn go(inv: &Inventory, st: &mut InventoryUi, id: Uuid) {
    let single_folder = !st.is_main() && !st.scope.is_nil() && st.scope != inv.root;
    if single_folder {
        st.navigate_to = Some(id);
    } else {
        st.open_folder_window(id);
    }
}

pub(super) fn show(ctx: &egui::Context, p: &Palette, icons: &Icons, inv: &Inventory, st: &mut InventoryUi, prefs: &InventoryPreferences) {
    let Some(mut jump) = st.folder_jump.take() else { return };
    jump.update(inv);
    let mut open = true;
    let mut close = false;
    let mut chosen = None;
    let window_id = format!("inventory_folder_jump_{}", st.ui_id);
    Floater::new(
        &window_id,
        aurora_i18n::tr!("inventory-folder-jump-title"),
        ctx.content_rect().center() - egui::vec2(270.0, 200.0),
        Vec2::new(540.0, 400.0),
    )
    .show(ctx, p, &mut open, |ui| {
        let width = ui.available_width();
        let filter = search_field(ui, p, &mut jump.query, aurora_i18n::tr!("inventory-folder-jump-filter"), width)
            .on_hover_text(aurora_i18n::tr!("inventory-folder-jump-filter-tip"));
        if jump.focus {
            filter.request_focus();
            jump.focus = false;
        }
        jump.update(inv);
        let keyboard = filter.has_focus() || filter.lost_focus();
        if keyboard {
            if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                jump.step(true);
            }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                jump.step(false);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                chosen = jump.selected;
            }
        }
        let (mut list, foot) = above_footer(ui);
        let list_height = list.available_height();
        egui::Frame::new().fill(p.field).show(&mut list, |ui| {
            ui.set_min_height(list_height);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt(("inventory_folder_jump_results", st.ui_id))
                .max_height(ui.available_height())
                .auto_shrink([false, false])
                .show_rows(ui, 40.0, jump.results.len(), |ui, range| {
                    for i in range {
                        let entry = &jump.entries[jump.results[i]];
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), egui::Sense::click());
                        if jump.selected == Some(entry.id) {
                            ui.painter()
                                .rect_filled(rect, ui.visuals().window_corner_radius, p.violet.gamma_multiply(0.35));
                            if response.rect.intersects(ui.clip_rect()) && keyboard {
                                ui.scroll_to_rect(rect, None);
                            }
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, ui.visuals().window_corner_radius, p.raised);
                        }
                        if let Some(icon) = icons.get("folder-filled") {
                            ui.painter().image(
                                icon.id(),
                                egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 5.0), Vec2::splat(16.0)),
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                super::folder_colors::tint(prefs, entry.id).unwrap_or(p.muted),
                            );
                        }
                        let parent = if entry.top_level() {
                            aurora_i18n::text("common-my-inventory").to_owned()
                        } else {
                            aurora_i18n::tr!("inventory-folder-jump-in", path = entry.parent_path.as_str())
                        };
                        for (text, offset, size, color) in [
                            (entry.name.as_str(), egui::vec2(28.0, 3.0), 13.0, p.ink),
                            (parent.as_str(), egui::vec2(28.0, 22.0), 11.0, p.muted),
                        ] {
                            let galley = egui::WidgetText::from(RichText::new(text).size(size).color(color)).into_galley(
                                ui,
                                Some(egui::TextWrapMode::Truncate),
                                (rect.width() - 34.0).max(0.0),
                                egui::TextStyle::Body,
                            );
                            ui.painter().galley(rect.min + offset, galley, color);
                        }
                        let response = response.on_hover_text(&parent);
                        if response.double_clicked() {
                            chosen = Some(entry.id);
                        } else if response.clicked() {
                            jump.selected = Some(entry.id);
                            filter.request_focus();
                        }
                    }
                });
        });
        foot.show(ui, |ui| {
            let status = if jump.total == 0 {
                if jump.query.trim().is_empty() && jump.entries.is_empty() {
                    aurora_i18n::text("inventory-folder-jump-looking").to_owned()
                } else {
                    aurora_i18n::text("inventory-folder-jump-none").to_owned()
                }
            } else if jump.total > jump.results.len() {
                aurora_i18n::tr!("inventory-folder-jump-limited", limit = jump.results.len(), count = jump.total)
            } else if jump.query.trim().is_empty() {
                aurora_i18n::text("inventory-folder-jump-top").to_owned()
            } else {
                aurora_i18n::tr!("inventory-folder-jump-matches", count = jump.total)
            };
            ui.label(RichText::new(status).size(11.0).color(p.muted));
            ui.horizontal(|ui| {
                let enabled = jump.selected.is_some();
                ui.add_enabled_ui(enabled, |ui| {
                    if flat_button(ui, p, aurora_i18n::tr!("inventory-folder-jump-open"))
                        .on_hover_text(aurora_i18n::tr!("inventory-folder-jump-open-tip"))
                        .clicked()
                    {
                        chosen = jump.selected;
                    }
                });
                if flat_button(ui, p, aurora_i18n::tr!("common-cancel")).clicked() {
                    close = true;
                }
            });
        });
    });
    // the folder may have gone since the list was made
    if let Some(id) = chosen.filter(|id| inv.folders.contains_key(id)) {
        go(inv, st, id);
        return;
    }
    if open && !close {
        st.folder_jump = Some(jump);
    }
}

/// `AURORA_DEMO_INVENTORY=folder-jump|aurora-folder-jump`: the dialog open
/// over the Legacy or Aurora inventory, searching "clo" (the demo's
/// Clothing folders, with their parent paths).
pub fn demo(st: &mut InventoryUi) {
    let mut jump = FolderJump::new();
    jump.query = "clo".into();
    st.folder_jump = Some(jump);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::inventory::Folder;
    use aurora_net::inventory::InvFolder;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn folder(inv: &mut Inventory, n: u128, parent: Uuid, name: &str, kind: i32) {
        inv.folders.insert(
            id(n),
            Folder {
                info: InvFolder {
                    id: id(n),
                    parent,
                    name: name.into(),
                    type_default: kind,
                    ..Default::default()
                },
                children: Vec::new(),
                items: Vec::new(),
                state: FetchState::Fetched,
                library: false,
            },
        );
        if let Some(parent) = inv.folders.get_mut(&parent) {
            parent.children.push(id(n));
        }
    }

    /// My Inventory / Clothes / Boots, Clothes / Winter / Boots,
    /// Bootstraps, Trash / Boots (left out).
    fn fixture() -> Inventory {
        let mut inv = Inventory {
            root: id(1),
            ..Default::default()
        };
        inv.folders.insert(
            inv.root,
            Folder {
                info: InvFolder {
                    id: inv.root,
                    type_default: 8,
                    ..Default::default()
                },
                children: Vec::new(),
                items: Vec::new(),
                state: FetchState::Fetched,
                library: false,
            },
        );
        folder(&mut inv, 10, id(1), "Clothes", -1);
        folder(&mut inv, 11, id(10), "Boots", -1);
        folder(&mut inv, 12, id(10), "Winter", -1);
        folder(&mut inv, 13, id(12), "Boots", -1);
        folder(&mut inv, 20, id(1), "Bootstraps", -1);
        folder(&mut inv, 30, id(1), "Trash", 14);
        folder(&mut inv, 31, id(30), "Boots", -1);
        inv
    }

    fn names(entries: &[Entry], (results, _): &(Vec<usize>, usize)) -> Vec<(String, String)> {
        results
            .iter()
            .map(|&i| (entries[i].name.clone(), entries[i].parent_path.clone()))
            .collect()
    }

    #[test]
    fn index_has_every_folder_with_its_path_but_not_trash() {
        let entries = index(&fixture());
        assert_eq!(entries.len(), 5);
        let winter_boots = entries.iter().find(|e| e.id == id(13)).expect("nested folder");
        assert_eq!(winter_boots.parent_path, "Clothes / Winter");
        assert!(entries.iter().all(|e| e.id != id(30) && e.id != id(31)));
    }

    #[test]
    fn exact_names_come_first_then_prefixes_then_path_matches() {
        let entries = index(&fixture());
        let found = search(&entries, "boots");
        assert_eq!(
            names(&entries, &found),
            [
                ("Boots".to_owned(), "Clothes".to_owned()),
                ("Boots".to_owned(), "Clothes / Winter".to_owned()),
                ("Bootstraps".to_owned(), String::new()),
            ]
        );
        // words may come from the parent path: "Clothes / Boots" style
        let found = search(&entries, "winter / boots");
        assert_eq!(names(&entries, &found), [("Boots".to_owned(), "Clothes / Winter".to_owned())]);
        assert_eq!(search(&entries, "nothing like this").1, 0);
    }

    #[test]
    fn empty_search_lists_top_level_folders() {
        let entries = index(&fixture());
        let found = search(&entries, "");
        assert_eq!(
            names(&entries, &found),
            [("Bootstraps".to_owned(), String::new()), ("Clothes".to_owned(), String::new())]
        );
    }

    #[test]
    fn results_stop_at_the_limit_but_count_every_match() {
        let mut inv = fixture();
        for n in 0..(MAX_RESULTS as u128 + 15) {
            folder(&mut inv, 1000 + n, id(1), &format!("Box {n:03}"), -1);
        }
        let entries = index(&inv);
        let (results, total) = search(&entries, "box");
        assert_eq!(results.len(), MAX_RESULTS);
        assert_eq!(total, MAX_RESULTS + 15);
        assert_eq!(entries[results[0]].name, "Box 000");
    }

    #[test]
    fn a_new_search_selects_its_best_match_and_arrows_move() {
        let inv = fixture();
        let mut jump = FolderJump::new();
        jump.query = "boots".into();
        jump.update(&inv);
        assert_eq!(jump.selected, Some(id(11)));
        jump.step(true);
        assert_eq!(jump.selected, Some(id(13)));
        jump.step(true);
        jump.step(true);
        assert_eq!(jump.selected, Some(id(20)), "stays on the last row");
        jump.query = "winter".into();
        jump.update(&inv);
        assert_eq!(jump.selected, Some(id(12)));
    }

    #[test]
    fn the_main_window_opens_a_folder_window_and_a_folder_window_moves() {
        let inv = fixture();
        let mut main = InventoryUi::default();
        go(&inv, &mut main, id(13));
        assert_eq!(main.windows.last().map(|w| w.root), Some(id(13)));
        let mut window = InventoryUi {
            ui_id: Uuid::new_v4(),
            scope: id(10),
            ..Default::default()
        };
        go(&inv, &mut window, id(13));
        assert_eq!(window.navigate_to, Some(id(13)));
        assert!(window.windows.is_empty());
    }
}
