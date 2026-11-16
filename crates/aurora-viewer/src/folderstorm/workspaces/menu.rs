//! World › Layouts (folderstorm/plans/workspaces.md section 9.4;
//! checkpoint 4 in detail, point 7).
//!
//! The menus are drawn without the settings in reach, so this one reads a
//! small [`View`] of the store, published by the frame call when the list
//! changes, and leaves [`Request`]s that the next frame call runs
//! (`super::apply::frame`). The card shown when the monitors change
//! answers through the same requests, and so do the switcher, the toolbar's
//! two buttons and the keyboard's actions (checkpoint 5).

use super::apply::Parts;
use super::pick;
use super::store::Store;
use crate::theme::Palette;
use crate::ui::menu;
use aurora_i18n::tr;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// The embedded Tabler icons of the submenu.
pub(super) const ICON_LAYOUTS: &str = "layout-dashboard-filled";
pub(super) const ICON_LAYOUT: &str = "layout-filled";
pub(super) const ICON_WORKSPACE: &str = "app-window-filled";
pub(super) const ICON_OPEN: &str = "layout-dashboard-filled";
pub(super) const ICON_PREVIOUS: &str = "arrow-back-up";
pub(super) const ICON_UNLOCK: &str = "lock-open";

/// Something asked from the menu or the card, run before the next frame's
/// interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Apply the layout of this name.
    Apply(String),
    /// *Previous arrangement*.
    Previous,
    /// *Unlock the viewer window*.
    Unlock,
    /// *Layouts…*: Preferences on the Layouts tab.
    Preferences,
    /// The card was read: nothing to do but take it away.
    Dismiss,
    /// The switcher shown, or put away when it is (its toolbar button, its
    /// key, *Layouts…*).
    Switcher,
    /// The toolbar's toggle, or its key: the one of its two layouts that
    /// is not in use.
    Toggle,
    /// The favorite after (true) or before the layout in use.
    Favorite(bool),
    /// The star of the layout of this name, given or taken away.
    Star(String, bool),
    /// *Update*: the layout of this name takes the arrangement of now.
    Update(String),
    /// *Save new*: the arrangement of now, these parts of it, under a name.
    SaveNew(String, Parts),
    /// Apply the workspace of this name.
    ApplyWorkspace(String),
    /// The star of the workspace of this name, given or taken away.
    StarWorkspace(String, bool),
    /// *Update*: the workspace of this name takes the windows open now.
    UpdateWorkspace(String),
    /// *Save new* as a workspace: the windows open now under a name, with
    /// the layout in use when asked.
    SaveWorkspace(String, bool),
}

/// What a keyboard action asks (`crate::folderstorm::keys`). `hidden`: in
/// mouselook, or with the interface put away; the switcher's key then asks
/// nothing, its window could not be used. The others apply as anywhere
/// (section 8.7).
pub fn key_request(key: crate::folderstorm::keys::Key, hidden: bool) -> Option<Request> {
    use crate::folderstorm::keys::Key;
    match key {
        Key::LayoutSwitcher if hidden => None,
        Key::LayoutSwitcher => Some(Request::Switcher),
        Key::LayoutToggle => Some(Request::Toggle),
        Key::PreviousArrangement => Some(Request::Previous),
        Key::NextFavoriteLayout => Some(Request::Favorite(true)),
        Key::PreviousFavoriteLayout => Some(Request::Favorite(false)),
    }
}

fn requests_id() -> egui::Id {
    egui::Id::unique("folderstorm_workspaces_requests")
}

pub fn ask(ctx: &egui::Context, request: Request) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Request>>(requests_id()).push(request));
}

pub(super) fn take(ctx: &egui::Context) -> Vec<Request> {
    ctx.data_mut(|d| d.remove_temp::<Vec<Request>>(requests_id())).unwrap_or_default()
}

/// What the menu shows of the store.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// The layouts the menu lists, and whether each is in use: the usable
    /// favorites in their order, or every usable layout when there is no
    /// favorite.
    pub layouts: Vec<(String, bool)>,
    /// The workspaces the menu lists, by the same rule.
    pub workspaces: Vec<(String, bool)>,
    /// The viewer window is locked or borderless.
    pub held: bool,
    /// There is an arrangement to go back to.
    pub previous: bool,
    /// The layout the toolbar's toggle would apply now; None until its two
    /// layouts are chosen.
    pub toggle: Option<String>,
}

impl View {
    fn of(store: &Store, previous: bool) -> Self {
        let lines = pick::entries(store);
        let favorites = lines.iter().any(|line| line.favorite);
        let of = |kind: pick::Kind| -> Vec<(String, bool)> {
            lines
                .iter()
                .filter(|line| line.kind == kind && (line.favorite || !favorites))
                .filter_map(|line| Some((line.name(store)?.to_owned(), line.in_use(store))))
                .collect()
        };
        View {
            layouts: of(pick::Kind::Layout),
            workspaces: of(pick::Kind::Workspace),
            held: store.window_held,
            previous,
            toggle: pick::toggle_target(store)
                .ok()
                .and_then(|index| store.layout(index))
                .map(|layout| layout.name.clone()),
        }
    }
}

/// A number that changes when the view would: the names in their order,
/// the layout in use, the hold, *Previous*. Made each frame without
/// allocating; the view itself only when it differs.
pub(super) fn stamp(store: &Store, previous: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for record in &store.layouts {
        record.layout().map(|layout| layout.name.as_str()).hash(&mut hasher);
    }
    store.active_layout.hash(&mut hasher);
    (store.window_held, previous).hash(&mut hasher);
    for favorite in &store.favorites {
        favorite.layout().hash(&mut hasher);
    }
    store.toggle.hash(&mut hasher);
    for record in &store.workspaces {
        record.workspace().map(|workspace| workspace.name.as_str()).hash(&mut hasher);
    }
    store.active_workspace.hash(&mut hasher);
    for favorite in &store.favorites {
        favorite.workspace().hash(&mut hasher);
    }
    // (never the stamp of a session that has published nothing)
    hasher.finish() | 1
}

fn view_id() -> egui::Id {
    egui::Id::unique("folderstorm_workspaces_menu")
}

pub(super) fn publish(ctx: &egui::Context, store: &Store, previous: bool) {
    ctx.data_mut(|d| d.insert_temp(view_id(), Arc::new(View::of(store, previous))));
}

pub(super) fn view(ctx: &egui::Context) -> Arc<View> {
    ctx.data(|d| d.get_temp::<Arc<View>>(view_id())).unwrap_or_default()
}

/// The *Layouts* submenu of the World menu (the hook of `ui/bars.rs`).
pub fn show(ui: &mut egui::Ui, p: &Palette) {
    menu::submenu(ui, p, ICON_LAYOUTS, tr!("fs-layouts-menu"), true, |ui| body(ui, p));
}

pub(super) fn body(ui: &mut egui::Ui, p: &Palette) {
    let view = view(ui.ctx());
    let mut request = None;
    for (name, active) in &view.layouts {
        if menu::check(ui, p, ICON_LAYOUT, name, *active, true) {
            request = Some(Request::Apply(name.clone()));
        }
    }
    if !view.layouts.is_empty() {
        menu::separator(ui, p);
    }
    for (name, active) in &view.workspaces {
        if menu::check(ui, p, ICON_WORKSPACE, name, *active, true) {
            request = Some(Request::ApplyWorkspace(name.clone()));
        }
    }
    if !view.workspaces.is_empty() {
        menu::separator(ui, p);
    }
    if menu::item(ui, p, ICON_OPEN, tr!("fs-layouts-menu-open")) {
        request = Some(Request::Switcher);
    }
    if menu::item_if(ui, p, ICON_PREVIOUS, tr!("fs-layouts-previous"), view.previous) {
        request = Some(Request::Previous);
    }
    if view.held && menu::item(ui, p, ICON_UNLOCK, tr!("fs-layouts-unlock")) {
        request = Some(Request::Unlock);
    }
    if let Some(request) = request {
        ask(ui.ctx(), request);
    }
}
