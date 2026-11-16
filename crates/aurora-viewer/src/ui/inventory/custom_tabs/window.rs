//! Folderstorm: a custom Inventory tab in a window of its own (feature 2.13,
//! "Inventory quick access").
//!
//! Right click on a custom tab › "Open in a window": a small Inventory
//! window that shows just that tab, its folders, pinned items, filters and
//! search, without the tab strip. It is an entry of `InventoryUi::windows`,
//! drawn like Aurora's additional window (same contents, dialogs and
//! clipboard); its state is locked to the tab (`State::window`). One window
//! per tab: its id comes from the tab's, so its place and size are kept per
//! tab. It closes when the tab is deleted, and with the other Inventory
//! windows under RLVa's @showinv. It is not reopened at login, as the
//! additional and folder windows are not.

use super::*;

/// The window ids of the tab windows: this, with the tab's id in the low
/// bits.
const BASE: u128 = 0x7a3c_51e0_4b2d_4f86_9e17_0000_0000_0000;

/// The size a tab window first opens at.
pub(in crate::ui::inventory) const SIZE: Vec2 = Vec2::new(320.0, 420.0);

/// The window id of the tab `tab`.
pub(in crate::ui::inventory) fn window_id(tab: u64) -> Uuid {
    Uuid::from_u128(BASE | u128::from(tab))
}

/// The tab a window shows, when it is a tab window.
pub(in crate::ui::inventory) fn of(st: &InventoryUi) -> Option<u64> {
    st.custom_tabs.window
}

/// The window is a tab window.
pub(in crate::ui::inventory) fn is(w: &InventoryWindow) -> bool {
    of(&w.state).is_some()
}

/// A tab window's title: the tab's name.
pub(in crate::ui::inventory) fn title(st: &InventoryUi, prefs: &InventoryPreferences) -> Option<String> {
    let tab = of(st)?;
    Some(
        prefs
            .custom_tabs
            .iter()
            .find(|t| t.id == tab)
            .map_or_else(|| st.custom_tabs.name.clone(), |t| t.name.clone()),
    )
}

fn request_id() -> egui::Id {
    egui::Id::unique("folderstorm_open_tab_window")
}

/// "Open in a window", from whichever window's menu: the main window opens
/// it on its next frame (`open_requested`), where the windows are.
pub(in crate::ui::inventory) fn request_open(ctx: &egui::Context, tab: u64) {
    ctx.data_mut(|d| d.insert_temp(request_id(), tab));
}

/// Once a frame, in the main window's `show`, before its windows are drawn.
pub(in crate::ui::inventory) fn open_requested(ctx: &egui::Context, st: &mut InventoryUi, prefs: &InventoryPreferences) {
    if let Some(tab) = ctx.data_mut(|d| d.remove_temp::<u64>(request_id())) {
        open(ctx, st, prefs, tab);
    }
}

/// Opens the window of the tab `tab`, or brings it to the front.
pub(in crate::ui::inventory) fn open(ctx: &egui::Context, st: &mut InventoryUi, prefs: &InventoryPreferences, tab: u64) {
    if !prefs.custom_tabs.iter().any(|t| t.id == tab) {
        return;
    }
    let id = window_id(tab);
    let floater = super::super::floater_id(id);
    if let Some(w) = st.windows.iter_mut().find(|w| w.id == id) {
        if w.open {
            widgets::expand_floater(ctx, &floater);
        } else {
            widgets::open_floater(ctx, &floater, &mut w.open);
        }
        return;
    }
    let mut state = InventoryUi::default();
    state.custom_tabs.window = Some(tab);
    select_custom(&mut state, prefs, tab);
    let mut open = false;
    widgets::open_floater(ctx, &floater, &mut open);
    st.windows.push(InventoryWindow {
        id,
        root: Uuid::nil(),
        additional: false,
        state: Box::new(state),
        open,
    });
}

/// A tab window keeps its tab shown (a search typed or a filter changed
/// stays on that tab).
pub(super) fn keep(st: &mut InventoryUi, prefs: &InventoryPreferences) {
    if let Some(tab) = of(st)
        && active(st) != Some(tab)
        && prefs.custom_tabs.iter().any(|t| t.id == tab)
    {
        select_custom(st, prefs, tab);
    }
}

/// After a window is drawn: a tab window whose tab is gone closes.
pub(in crate::ui::inventory) fn after(w: &mut InventoryWindow, prefs: &InventoryPreferences) {
    if let Some(tab) = of(&w.state)
        && !prefs.custom_tabs.iter().any(|t| t.id == tab)
    {
        w.open = false;
    }
}

/// "Open in a window" in a custom tab's right-click menu.
pub(super) fn menu_entry(ui: &mut egui::Ui, p: &Palette, st: &InventoryUi, id: u64) {
    if of(st) == Some(id) {
        return;
    }
    if menu::item_tip(
        ui,
        p,
        "app-window-filled",
        aurora_i18n::tr!("inventory-quick-open-window"),
        true,
        aurora_i18n::tr!("inventory-quick-open-window-tip"),
    ) {
        request_open(ui.ctx(), id);
    }
}

// ---- Demo ----

/// `AURORA_DEMO_INVENTORY=quick-access|aurora-quick-access`: a tab "Quick
/// access" scoped to the demo's Body folder, pinning the demo gesture, the
/// demo object (through its link) and a deleted item, shown in the main
/// window and opened in its own window, pinned and quiet. With
/// `AURORA_DEMO_QUICK_ACCESS=flat`, the tab is a flat list.
pub fn demo(ctx: &egui::Context, st: &mut InventoryUi, prefs: &mut InventoryPreferences, account: Uuid) {
    prefs.custom_tabs.clear();
    let Some(id) = add(
        &mut prefs.custom_tabs,
        aurora_i18n::tr!("inventory-quick-demo-name"),
        Filters::default(),
        String::new(),
    ) else {
        return;
    };
    if let Some(tab) = prefs.custom_tabs.iter_mut().find(|t| t.id == id) {
        // seed_view's Body; the demo gesture, the object its link 8113
        // points to, an item that no longer exists
        folders::set(tab, account, vec![Uuid::from_u128(8204)]);
        items::set(tab, account, [8106, 8100, 0x00de_1e7e_d001].map(Uuid::from_u128).to_vec());
        tab.flat = std::env::var("AURORA_DEMO_QUICK_ACCESS").is_ok_and(|v| v == "flat");
    }
    select_custom(st, prefs, id);
    prefs.window_modes.set(
        super::super::window_mode::Key::Tab(id),
        super::super::window_mode::Mode { pinned: true, quiet: true },
    );
    open(ctx, st, prefs, id);
}
