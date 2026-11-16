//! Folderstorm: up to 16 additional Inventory windows (FEATURES.md row
//! 2.14; folderstorm/plans/inventory-multiple-windows.md).
//!
//! Aurora keeps one additional Inventory window (`ADDITIONAL_WINDOW`), where
//! the Legacy Viewer's *New Inventory Window* adds another one each time
//! (LLPanelMainInventory::newWindow, indra/newview/llpanelmaininventory.cpp).
//! Here there are 16 "slots": slot 1 is Aurora's window with its fixed id,
//! slots 2 to 16 take the ids that follow it, so each slot keeps its own
//! place and size in the layout file. Every slot is an `additional` entry of
//! `InventoryUi::windows`, drawn, fetched and closed under RLVa's @showinv
//! exactly as Aurora's window is; its slot is read from its id.
//!
//! - Ctrl+Shift+I and the "New Inventory window" button (in the main window
//!   and in every slot, both styles) open the lowest free slot. With all 16
//!   open, nothing opens and the Inventory's notice says so; the button is
//!   greyed;
//! - right click on the button: *New Inventory window* and *Close all
//!   additional windows* (the slots only, not the folder or tab windows);
//! - titles "Inventory 2" … "Inventory 17"; the main window stays
//!   "Inventory";
//! - a closed slot is dropped with its tabs and search, as Aurora's window is
//!   (LLFloaterSidePanelContainer::closeFloater destroys it), and its place
//!   is kept. Slots are not reopened at login; layouts and workspaces open,
//!   place and close each one (`folderstorm/workspaces/windows.rs`).
//!
//! The tab windows of the custom tabs (`custom_tabs/window.rs`) are not
//! slots and have no such button: each shows one tab, locked to it.

use super::*;

/// How many additional windows may be open at once.
pub(crate) const MAX_SLOTS: u8 = 16;

/// How far each slot first opens from the one before it, down and right.
const CASCADE: f32 = 24.0;

/// The window id of slot `slot` (1 to 16): slot 1 is Aurora's additional
/// window, the others follow its id.
pub(crate) fn id_of(slot: u8) -> Option<Uuid> {
    (1..=MAX_SLOTS)
        .contains(&slot)
        .then(|| Uuid::from_u128(ADDITIONAL_WINDOW.as_u128() + u128::from(slot - 1)))
}

/// The slot of the window `id`, if it is one.
pub(crate) fn slot_of(id: Uuid) -> Option<u8> {
    let offset = id.as_u128().checked_sub(ADDITIONAL_WINDOW.as_u128())?;
    u8::try_from(offset)
        .ok()
        .filter(|offset| *offset < MAX_SLOTS)
        .map(|offset| offset + 1)
}

/// The floater id of slot `slot` (its place in egui's memory and in the
/// layouts).
pub(crate) fn floater_of(slot: u8) -> Option<String> {
    id_of(slot).map(floater_id)
}

/// The slot of the floater id `window`, if it is one.
pub(crate) fn slot_of_floater(window: &str) -> Option<u8> {
    let slot = slot_of(window.strip_prefix("inventory_")?.parse().ok()?)?;
    (floater_of(slot)? == window).then_some(slot)
}

/// The title of slot `slot`: "Inventory 2" for slot 1, … "Inventory 17".
pub(crate) fn slot_title(slot: u8) -> String {
    aurora_i18n::tr!("inventory-windows-title", number = usize::from(slot) + 1)
}

/// The title of the additional window `id`.
pub(super) fn title(id: Uuid) -> String {
    slot_of(id).map_or_else(|| aurora_i18n::tr!("common-inventory").to_owned(), slot_title)
}

/// Where the additional window `id` first opens: slot 1 where Aurora's
/// window does (`first`), each next slot a step further, inside the screen.
pub(super) fn first_pos(id: Uuid, first: egui::Pos2, screen: egui::Rect) -> egui::Pos2 {
    let slot = slot_of(id).unwrap_or(1);
    if slot <= 1 {
        return first;
    }
    let step = f32::from(slot - 1) * CASCADE;
    let pos = first + egui::vec2(step, step);
    egui::pos2(
        pos.x.min(screen.right() - DEFAULT_SIZE.x).max(screen.left()),
        pos.y.min(screen.bottom() - DEFAULT_SIZE.y).max(screen.top()),
    )
}

impl InventoryUi {
    /// Slot `slot` is open (shown or minimized).
    pub(crate) fn folderstorm_slot_open(&self, slot: u8) -> bool {
        id_of(slot).is_some_and(|id| self.windows.iter().any(|w| w.id == id && w.open))
    }

    /// How many slots are open.
    pub(crate) fn folderstorm_open_slots(&self) -> usize {
        self.windows
            .iter()
            .filter(|w| w.open && w.additional && slot_of(w.id).is_some())
            .count()
    }

    /// Ctrl+Shift+I and the "New Inventory window" button: the lowest free
    /// slot opens, in front. With all 16 open nothing opens and the
    /// Inventory's notice says so. Returns the slot opened.
    pub fn folderstorm_new_window(&mut self, ctx: &egui::Context) -> Option<u8> {
        let Some(slot) = (1..=MAX_SLOTS).find(|slot| !self.folderstorm_slot_open(*slot)) else {
            self.notice = Some((
                aurora_i18n::tr!("inventory-windows-all-open", max = MAX_SLOTS),
                std::time::Instant::now(),
            ));
            return None;
        };
        self.folderstorm_open_slot(ctx, slot);
        if let Some(floater) = floater_of(slot) {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::unique(&floater)));
        }
        Some(slot)
    }

    /// Opens slot `slot`, or brings it to the front when it is open (a
    /// workspace, slot 1 as `open_additional_window`). A closed slot still
    /// kept by a document or dialog it opened is the same window again.
    pub(crate) fn folderstorm_open_slot(&mut self, ctx: &egui::Context, slot: u8) {
        // slot 1: Aurora's own way, which does the same
        if slot == 1 {
            self.open_additional_window(ctx);
            return;
        }
        let Some(id) = id_of(slot) else { return };
        let floater = floater_id(id);
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            if w.open {
                widgets::expand_floater(ctx, &floater);
            } else {
                widgets::open_floater(ctx, &floater, &mut w.open);
            }
            return;
        }
        let mut open = false;
        widgets::open_floater(ctx, &floater, &mut open);
        self.windows.push(InventoryWindow {
            id,
            root: Uuid::nil(),
            additional: true,
            state: Box::default(),
            open,
        });
    }

    /// Closes slot `slot` alone (a workspace that does not list it).
    pub(crate) fn folderstorm_close_slot(&mut self, slot: u8) {
        let Some(id) = id_of(slot) else { return };
        for w in self.windows.iter_mut().filter(|w| w.id == id) {
            w.open = false;
        }
    }

    /// *Close all additional windows*: every slot, not the folder or tab
    /// windows (RLVa's `close_additional_window` closes those too).
    pub(crate) fn folderstorm_close_all_slots(&mut self) {
        for w in self.windows.iter_mut().filter(|w| w.additional && slot_of(w.id).is_some()) {
            w.open = false;
        }
    }
}

// ---- The button, in every window that has it ----

/// What a button or its menu asks of the main window, which has the
/// windows (`frame`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    New,
    CloseAll,
}

fn request_id() -> egui::Id {
    egui::Id::unique("folderstorm_inventory_windows_request")
}

fn open_count_id() -> egui::Id {
    egui::Id::unique("folderstorm_inventory_windows_open")
}

fn menu_id(window: Uuid) -> egui::Id {
    egui::Id::unique(("folderstorm_inventory_windows_menu", window))
}

/// Where the button of the window `window` (nil: the main one) was drawn
/// last, for the demo and the tests.
pub(super) fn button_rect_id(window: Uuid) -> egui::Id {
    egui::Id::unique(("folderstorm_inventory_windows_button", window))
}

fn request(ctx: &egui::Context, request: Request) {
    ctx.data_mut(|d| d.insert_temp(request_id(), Some(request)));
    ctx.request_repaint();
}

/// Once a frame, in the main window's `show` before any window is drawn:
/// what a button asked last frame is done, and the buttons learn how many
/// slots are open.
pub(super) fn frame(ctx: &egui::Context, st: &mut InventoryUi) {
    match ctx.data_mut(|d| d.remove_temp::<Option<Request>>(request_id())).flatten() {
        Some(Request::New) => {
            st.folderstorm_new_window(ctx);
        }
        Some(Request::CloseAll) => st.folderstorm_close_all_slots(),
        None => {}
    }
    let open = st.folderstorm_open_slots();
    ctx.data_mut(|d| d.insert_temp(open_count_id(), open));
}

/// The "New Inventory window" button of one window.
struct Button {
    /// The window it is in (nil: the main one).
    window: Uuid,
    /// The slots open this frame.
    open: usize,
}

impl Button {
    /// The main window and the slots have it; the folder and tab windows
    /// do not.
    fn of(ctx: &egui::Context, st: &InventoryUi) -> Option<Self> {
        (st.is_main() || slot_of(st.ui_id).is_some()).then(|| Self {
            window: st.ui_id,
            open: ctx.data(|d| d.get_temp(open_count_id())).unwrap_or(0),
        })
    }

    fn tip(&self) -> String {
        aurora_i18n::tr!("inventory-windows-new-tip", open = self.open, max = MAX_SLOTS)
    }

    /// Lit while any slot is open.
    fn lit(&self) -> bool {
        self.open > 0
    }

    /// Greyed with all 16 open.
    fn full(&self) -> bool {
        self.open >= usize::from(MAX_SLOTS)
    }

    /// After the button is drawn at `rect`: its click and its right-click
    /// menu.
    fn after(&self, ui: &mut egui::Ui, p: &Palette, rect: egui::Rect, clicked: bool) {
        let ctx = ui.ctx().clone();
        ctx.data_mut(|d| d.insert_temp(button_rect_id(self.window), rect));
        if clicked && !self.full() {
            request(&ctx, Request::New);
        }
        let menu = menu_id(self.window);
        if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.secondary_clicked()) {
            let pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or(rect.center());
            open_menu(&ctx, self.window, pos);
        }
        let Some(pos) = ctx.data(|d| d.get_temp::<egui::Pos2>(menu.with("pos"))) else {
            return;
        };
        let mut chosen = None;
        crate::ui::menu::context_menu_at(ui, menu, pos, p, |ui| {
            if crate::ui::menu::item_tip(
                ui,
                p,
                ADDITIONAL_ICON,
                aurora_i18n::tr!("inventory-windows-new"),
                !self.full(),
                &self.tip(),
            ) {
                chosen = Some(Request::New);
            }
            if crate::ui::menu::item_if(ui, p, "x", aurora_i18n::tr!("inventory-windows-close-all"), self.open > 0) {
                chosen = Some(Request::CloseAll);
            }
        });
        if !egui::Popup::is_id_open(&ctx, menu) {
            ctx.data_mut(|d| d.remove::<egui::Pos2>(menu.with("pos")));
        }
        if let Some(chosen) = chosen {
            request(&ctx, chosen);
        }
    }
}

/// Opens the button's menu of the window `window` at `pos`.
fn open_menu(ctx: &egui::Context, window: Uuid, pos: egui::Pos2) {
    let menu = menu_id(window);
    ctx.data_mut(|d| d.insert_temp(menu.with("pos"), pos));
    egui::Popup::open_id(ctx, menu);
}

/// The Legacy style's button, beside "+" in the status line. Returns where
/// it was drawn; none in a window without it.
pub(super) fn legacy_button(ui: &mut egui::Ui, p: &Palette, st: &InventoryUi) -> Option<egui::Rect> {
    let button = Button::of(ui.ctx(), st)?;
    let tip = button.tip();
    let drawn = ui.scope(|ui| {
        if button.full() {
            widgets::icon_button(ui, p, ADDITIONAL_ICON, &tip, false)
        } else {
            widgets::icon_toggle(ui, p, ADDITIONAL_ICON, &tip, button.lit())
        }
    });
    button.after(ui, p, drawn.response.rect, drawn.inner);
    Some(drawn.response.rect)
}

/// Aurora's button in its toolbar, drawn by `draw` (its tool button, given
/// the tooltip and whether it is lit). Returns where it was drawn; none in
/// a window without it.
pub(super) fn aurora_button(
    ui: &mut egui::Ui,
    p: &Palette,
    st: &InventoryUi,
    draw: impl FnOnce(&mut egui::Ui, &str, bool) -> egui::Response,
) -> Option<egui::Rect> {
    let button = Button::of(ui.ctx(), st)?;
    let tip = button.tip();
    let response = ui
        .add_enabled_ui(!button.full(), |ui| draw(ui, &tip, button.lit()))
        .inner
        .on_disabled_hover_text(&tip);
    button.after(ui, p, response.rect, response.clicked());
    Some(response.rect)
}

// ---- Demo ----

/// `AURORA_DEMO_INVENTORY=extra-windows|aurora-extra-windows`: the main
/// window and slots 1, 2 and 3 open side by side, slot 2 on the Recent tab
/// with a search typed, slot 3 minimized. `extra-windows-full`: all 16 slots
/// open where they first open (a cascade), the notice that no more can
/// open, and the main window's button menu open with *New Inventory window*
/// greyed.
pub fn demo_frame(ctx: &egui::Context, st: &mut InventoryUi, prefs: &InventoryPreferences, frame: u64) {
    let Ok(mode) = std::env::var("AURORA_DEMO_INVENTORY") else { return };
    if !mode.contains("extra-windows") {
        return;
    }
    let full = mode.ends_with("-full");
    match frame {
        3 => {
            widgets::place_floater(ctx, "inventory", egui::pos2(30.0, 60.0), egui::vec2(DEFAULT_SIZE.x, 640.0));
            let count = if full { MAX_SLOTS } else { 3 };
            for _ in 0..count {
                st.folderstorm_new_window(ctx);
            }
            if full {
                return;
            }
            for slot in 1..=3u8 {
                if let Some(floater) = floater_of(slot) {
                    let x = 30.0 + f32::from(slot) * (DEFAULT_SIZE.x + 12.0);
                    widgets::place_floater(ctx, &floater, egui::pos2(x, 60.0), DEFAULT_SIZE);
                }
            }
            if let Some(w) = st.windows.iter_mut().find(|w| slot_of(w.id) == Some(2)) {
                set_tab(&mut w.state, prefs, 2);
                w.state.search = "shirt".into();
            }
        }
        30 if !full => {
            if let Some(floater) = floater_of(3) {
                widgets::minimize_floater(ctx, &floater);
            }
        }
        // (again later: the notice fades after a few seconds)
        40 | 400 if full => {
            st.folderstorm_new_window(ctx);
            if let Some(rect) = ctx.data(|d| d.get_temp::<egui::Rect>(button_rect_id(Uuid::nil()))) {
                open_menu(ctx, Uuid::nil(), rect.center());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
