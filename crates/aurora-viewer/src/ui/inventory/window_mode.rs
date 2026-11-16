//! Folderstorm: pin and quiet mode for every Inventory window (feature
//! 2.13, "Inventory quick access"; folderstorm/plans/inventory-quick-access.md).
//!
//! Two title bar buttons on the main window, the additional window, the tab
//! windows and the folder windows, in both styles:
//!
//! - **pin**: the window stays above the viewer's other windows (brought to
//!   the front of the windows' layer at every frame), the meaning of the pin
//!   of the session host's pop-out windows (always on top). Menus, tooltips
//!   and notifications still open over it;
//! - **quiet mode**: the window shows only its list (the tab or folder
//!   shown): no toolbar, search box, tab strip, chips, status line, nor
//!   Aurora's column, breadcrumb and details. The title bar stays, with these
//!   two buttons. The search and filters in effect stay in effect.
//!
//! Both are kept per window in the preferences (`InventoryPreferences::
//! window_modes`): the main window, each additional window by its slot
//! (`extra_windows.rs`), each tab window by its tab, each folder window by
//! its folder; only windows out of the default mode are written.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A window's pin and quiet mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mode {
    pub pinned: bool,
    pub quiet: bool,
}

impl Mode {
    fn is_default(self) -> bool {
        self == Self::default()
    }
}

/// The folder windows remembered at most (the oldest key goes first).
pub const MAX_FOLDER_WINDOWS: usize = 32;

/// Which window a mode belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Main,
    /// An additional window, by its slot (1 to 16, `extra_windows.rs`).
    Additional(u8),
    Tab(u64),
    Folder(Uuid),
}

impl Key {
    fn text(self) -> String {
        match self {
            Self::Main => "main".into(),
            // slot 1 keeps the key it had when there was one additional window
            Self::Additional(slot) if slot <= 1 => "additional".into(),
            Self::Additional(slot) => format!("additional:{slot}"),
            Self::Tab(id) => format!("tab:{id}"),
            Self::Folder(id) => format!("folder:{id}"),
        }
    }

    /// The key of a window of `InventoryUi::windows`: none for the
    /// Marketplace listings panel and the documents.
    pub(super) fn of_window(w: &InventoryWindow) -> Option<Self> {
        if let Some(tab) = custom_tabs::window::of(&w.state) {
            Some(Self::Tab(tab))
        } else if w.additional {
            Some(Self::Additional(extra_windows::slot_of(w.id).unwrap_or(1)))
        } else if w.id != listings::PANEL && !w.root.is_nil() {
            Some(Self::Folder(w.root))
        } else {
            None
        }
    }
}

/// The modes of the windows, saved with the inventory preferences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Modes(BTreeMap<String, Mode>);

impl Modes {
    pub fn get(&self, key: Key) -> Mode {
        self.0.get(&key.text()).copied().unwrap_or_default()
    }

    pub fn set(&mut self, key: Key, mode: Mode) {
        let text = key.text();
        if mode.is_default() {
            self.0.remove(&text);
            return;
        }
        self.0.insert(text.clone(), mode);
        // folder windows come and go: at most `MAX_FOLDER_WINDOWS` of them,
        // the one just set kept
        let folders: Vec<String> = self.0.keys().filter(|k| k.starts_with("folder:") && **k != text).cloned().collect();
        let over = (folders.len() + usize::from(text.starts_with("folder:"))).saturating_sub(MAX_FOLDER_WINDOWS);
        for key in folders.into_iter().take(over) {
            self.0.remove(&key);
        }
    }

    /// The tabs that are no longer there lose their window's mode.
    fn retain_tabs(&mut self, tabs: &[u64]) -> bool {
        let before = self.0.len();
        self.0.retain(|key, _| {
            key.strip_prefix("tab:")
                .is_none_or(|id| id.parse::<u64>().is_ok_and(|id| tabs.contains(&id)))
        });
        self.0.len() != before
    }
}

/// Once a frame, with the custom tabs: a deleted tab forgets its window's
/// mode.
pub(super) fn forget_deleted_tabs(prefs: &mut InventoryPreferences) {
    let tabs: Vec<u64> = prefs.custom_tabs.iter().map(|t| t.id).collect();
    prefs.window_modes.retain_tabs(&tabs);
}

/// The title bar buttons clicked this frame.
#[derive(Default)]
pub(super) struct Clicks {
    pin: bool,
    quiet: bool,
}

/// Before the window is shown: its mode for this frame (read by its
/// contents) and its two title bar buttons. No key (a document): nothing.
pub(super) fn decorate<'a>(
    floater: widgets::Floater<'a>,
    st: &mut InventoryUi,
    prefs: &InventoryPreferences,
    key: Option<Key>,
    clicks: &'a mut Clicks,
) -> widgets::Floater<'a> {
    let Some(key) = key else {
        st.window_mode = Mode::default();
        return floater;
    };
    let mode = prefs.window_modes.get(key);
    st.window_mode = mode;
    let Clicks { pin, quiet } = clicks;
    let (quiet_icon, quiet_tip) = if mode.quiet {
        ("arrows-maximize", aurora_i18n::tr!("inventory-quick-full"))
    } else {
        ("arrows-minimize", aurora_i18n::tr!("inventory-quick-quiet"))
    };
    let (pin_icon, pin_tip) = if mode.pinned {
        ("pinned-filled", aurora_i18n::tr!("inventory-quick-unpin"))
    } else {
        ("pin-filled", aurora_i18n::tr!("inventory-quick-pin"))
    };
    floater.action(quiet_icon, quiet_tip, quiet).action(pin_icon, pin_tip, pin)
}

/// After the window `floater` is shown (`open` or not): pinned, it goes in
/// front of the other windows; a title bar button clicked changes its mode.
/// (A window brought to the front by a click comes back behind it on the
/// next frame.)
pub(super) fn apply(ctx: &egui::Context, floater: &str, open: bool, clicks: Clicks, prefs: &mut InventoryPreferences, key: Option<Key>) {
    let Some(key) = key else {
        return;
    };
    let mut mode = prefs.window_modes.get(key);
    if mode.pinned && open && !widgets::interface_hidden(ctx) && !widgets::floater_minimized(ctx, floater) {
        widgets::bring_floater_front(ctx, floater);
    }
    if !clicks.pin && !clicks.quiet {
        return;
    }
    mode.pinned ^= clicks.pin;
    mode.quiet ^= clicks.quiet;
    prefs.window_modes.set(key, mode);
}

/// The window shows only its list this frame.
pub(super) fn quiet(st: &InventoryUi) -> bool {
    st.window_mode.quiet
}

/// Aurora's layout for this frame: the narrow one in quiet mode and in a
/// tab window (a small window), else the one of its width.
pub(super) fn layout(st: &InventoryUi, of_width: aurora::Layout) -> aurora::Layout {
    if quiet(st) || custom_tabs::window::of(st).is_some() {
        aurora::Layout::Compact
    } else {
        of_width
    }
}

/// The list's own drop targets and keyboard shortcuts, around `zone`.
#[allow(clippy::too_many_arguments)]
fn around_list(
    ui: &mut egui::Ui,
    p: &Palette,
    zone: &egui::Response,
    inv: &mut Inventory,
    target: Uuid,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    facts: &Facts,
    actions: &mut Vec<InvAction>,
) {
    task_drop::target(ui, p, zone, inv, target, actions);
    drag::target(ui, p, zone, inv, target, st, prefs, facts, actions);
    clipboard::shortcuts(ui, inv, target, st, prefs, facts, actions);
}

/// Quiet mode in the Legacy style, for the main, additional and tab
/// windows (`inventory_contents`): the list of the tab shown, alone.
#[allow(clippy::too_many_arguments)]
pub(super) fn legacy_list(
    ui: &mut egui::Ui,
    p: &Palette,
    icons: &Icons,
    inv: &mut Inventory,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    facts: &Facts,
    actions: &mut Vec<InvAction>,
) {
    let (root, filtered) = list_scope(inv, st, prefs, facts);
    let list_h = (ui.available_height() - 10.0).max(40.0);
    let zone = tree::frame(ui, p, |ui| {
        list(ui, p, icons, inv, st, prefs, facts, actions, list_h, root, filtered, true);
    });
    let target = list_destination(st, root);
    around_list(ui, p, &zone.response, inv, target, st, prefs, facts, actions);
}

/// Quiet mode in a Legacy folder window (`folder_window_contents`): its
/// folder's list, searched as that window does.
#[allow(clippy::too_many_arguments)]
pub(super) fn legacy_folder_list(
    ui: &mut egui::Ui,
    p: &Palette,
    icons: &Icons,
    inv: &mut Inventory,
    root: Uuid,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    facts: &Facts,
    actions: &mut Vec<InvAction>,
) {
    let q = st.search.trim().to_lowercase();
    let filtered = !q.is_empty() || st.links_filter.is_some() || st.filters.active();
    if filtered {
        request_search(inv, root, st, prefs);
        st.view.search(
            inv,
            root,
            &q,
            prefs,
            &st.filters,
            0,
            st.links_filter,
            facts,
            facts.agent,
            view::now(),
        );
    } else if st.fetch_all {
        st.fetch_all = !rules::request_tree(inv, root);
    }
    let list_h = (ui.available_height() - 4.0).max(40.0);
    let zone = egui::Frame::new().fill(p.field).show(ui, |ui| {
        clipboard::list_focus(ui, st, list_h);
        if filtered {
            prepare_result_tree(st, inv, root);
            search_scroll(ui, p, icons, inv, st, prefs, facts, actions, list_h);
            return;
        }
        rows::scroll(ui, p, icons, inv, st, prefs, facts, actions, list_h, root, false);
    });
    around_list(ui, p, &zone.response, inv, root, st, prefs, facts, actions);
}

/// Quiet mode in Aurora's window (any of its windows): the narrow tree of
/// the tab or folder shown, in all the room.
#[allow(clippy::too_many_arguments)]
pub(super) fn aurora_list(
    ui: &mut egui::Ui,
    p: &Palette,
    icons: &Icons,
    inv: &mut Inventory,
    st: &mut InventoryUi,
    prefs: &mut InventoryPreferences,
    facts: &Facts,
    actions: &mut Vec<InvAction>,
) {
    let (root, filtered) = list_scope(inv, st, prefs, facts);
    let room = ui.available_rect_before_wrap();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(room)
            .id_salt("folderstorm_quiet_list")
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(room.intersect(ui.clip_rect()));
    let out = list(
        &mut child,
        p,
        icons,
        inv,
        st,
        prefs,
        facts,
        actions,
        room.height(),
        root,
        filtered,
        false,
    );
    ui.advance_cursor_after_rect(room);
    let zone = ui.interact(out.inner_rect, ui.scope_id().with("folderstorm_quiet_zone"), egui::Sense::hover());
    let target = list_destination(st, root);
    around_list(ui, p, &zone, inv, target, st, prefs, facts, actions);
}

#[cfg(test)]
mod tests;
