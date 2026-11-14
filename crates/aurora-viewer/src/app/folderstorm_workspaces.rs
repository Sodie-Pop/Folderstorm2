//! Folderstorm: layouts and workspaces in the app (the rules are in
//! `crate::folderstorm::workspaces` and `crate::folderstorm::viewport::bars`;
//! folderstorm/plans/workspaces.md, checkpoints 3 to 5).
//!
//! The only place where a layout meets winit: each frame the viewer
//! window is described to the layouts ([`Seen`]) and what they ask of it
//! is done ([`Command`]). Everything else is decided in
//! `crate::folderstorm::workspaces`, without a window.

use super::*;
use crate::folderstorm::viewport;
use crate::folderstorm::workspaces::apply;
use crate::folderstorm::workspaces::window::{Command, Seen};

/// The viewer's windows for the layouts and the workspaces
/// (`apply::Shell`): which of the allowlisted windows are open, Aurora's
/// own way of opening each (the path of its menu entry or toolbar button:
/// `ui::bars::run_command`, `ui::controls::request`,
/// `InventoryUi::open_additional_window`), and of closing one (a workspace
/// closes the windows it does not list).
struct Windows<'a> {
    panels: &'a mut ui::Panels,
    inventory: &'a mut ui::inventory::InventoryUi,
    /// The camera and movement controls are Legacy's two windows.
    classic_controls: bool,
    /// Contacts is a window of its own, not a tab of Conversations.
    contacts_torn_off: bool,
    /// RLVa hides Inventory (@showinv), the mini-map, the world map.
    no_inventory: bool,
    no_minimap: bool,
    no_world_map: bool,
    /// The account logged in, and its Inventory: a workspace's folder
    /// windows are for one account and for folders that still exist.
    account: uuid::Uuid,
    folders: Option<&'a crate::world::inventory::Inventory>,
}

/// The `Panels` flag of an allowlisted window that has one of its own.
macro_rules! window_flag {
    ($panels:expr, $id:expr, $($borrow:tt)+) => {
        match $id {
            "inventory" => Some($($borrow)+ $panels.inventory),
            "conversations" => Some($($borrow)+ $panels.chat),
            ui::people::WINDOW => Some($($borrow)+ $panels.people),
            "contacts" => Some($($borrow)+ $panels.contacts),
            "places" => Some($($borrow)+ $panels.places),
            ui::search::WINDOW => Some($($borrow)+ $panels.search),
            "minimap" => Some($($borrow)+ $panels.minimap),
            "worldmap" => Some($($borrow)+ $panels.world_map),
            "perf" => Some($($borrow)+ $panels.perf),
            ui::controls::CLASSIC_CAMERA => Some($($borrow)+ $panels.camera_controls),
            ui::controls::CLASSIC_MOVEMENT => Some($($borrow)+ $panels.movement_controls),
            "gestures" => Some($($borrow)+ $panels.gestures),
            "appearance" => Some($($borrow)+ $panels.appearance),
            "ao" => Some($($borrow)+ $panels.ao),
            ui::region_tracker::WINDOW => Some($($borrow)+ $panels.region_tracker),
            ui::area_search::WINDOW => Some($($borrow)+ $panels.area_search),
            ui::quick_prefs::WINDOW => Some($($borrow)+ $panels.quick_prefs),
            ui::destinations::WINDOW => Some($($borrow)+ $panels.destinations),
            ui::money_tracker::WINDOW => Some($($borrow)+ $panels.money_tracker),
            _ => None,
        }
    };
}

impl Windows<'_> {
    /// Whether the viewer has the window `id` as things are: not while
    /// RLVa hides it, and not a window of the other look of the controls
    /// or a Contacts window while Contacts is a tab of Conversations.
    fn available(&self, id: &str) -> bool {
        use crate::folderstorm::workspaces::windows::ADDITIONAL_INVENTORY;
        match id {
            "inventory" | ADDITIONAL_INVENTORY => !self.no_inventory,
            "minimap" => !self.no_minimap,
            "worldmap" => !self.no_world_map,
            "contacts" => self.contacts_torn_off,
            ui::controls::WINDOW => !self.classic_controls,
            ui::controls::CLASSIC_CAMERA | ui::controls::CLASSIC_MOVEMENT => self.classic_controls,
            _ => true,
        }
    }
}

impl apply::Shell for Windows<'_> {
    fn is_open(&self, window: &crate::folderstorm::workspaces::windows::Window) -> bool {
        use crate::folderstorm::workspaces::windows::ADDITIONAL_INVENTORY;
        let id = window.id;
        match id {
            ADDITIONAL_INVENTORY => self.inventory.additional_open(),
            // Aurora's one window for the camera and the movement controls
            ui::controls::WINDOW => !self.classic_controls && (self.panels.camera_controls || self.panels.movement_controls),
            ui::controls::CLASSIC_CAMERA | ui::controls::CLASSIC_MOVEMENT if !self.classic_controls => false,
            _ => window_flag!(self.panels, id, &).is_some_and(|open| *open),
        }
    }

    fn open(&mut self, ctx: &egui::Context, window: &crate::folderstorm::workspaces::windows::Window) -> bool {
        use crate::folderstorm::workspaces::windows::ADDITIONAL_INVENTORY;
        let id = window.id;
        if !self.available(id) {
            return false;
        }
        match id {
            ADDITIONAL_INVENTORY => self.inventory.open_additional_window(ctx),
            // on its Camera tab, as the View button opens it
            ui::controls::WINDOW => ui::controls::request(
                ctx,
                &mut self.panels.camera_controls,
                &mut self.panels.movement_controls,
                false,
                false,
                false,
            ),
            // (the mini-map has no title bar: its flag alone, as its
            // toolbar button)
            "minimap" => self.panels.minimap = true,
            _ => match window_flag!(self.panels, id, &mut) {
                Some(open) => ui::widgets::open_floater(ctx, id, open),
                // a window of the allowlist this file does not know
                None => return false,
            },
        }
        true
    }

    fn close(&mut self, _: &egui::Context, window: &crate::folderstorm::workspaces::windows::Window) {
        use crate::folderstorm::workspaces::windows::ADDITIONAL_INVENTORY;
        let id = window.id;
        match id {
            // as `app/rlva.rs` closes it under @showinv
            ADDITIONAL_INVENTORY => self.inventory.close_additional_window(),
            // Aurora's one window for the camera and the movement controls
            ui::controls::WINDOW => {
                self.panels.camera_controls = false;
                self.panels.movement_controls = false;
            }
            // the flag alone, as the window's own cross leaves it
            _ => {
                if let Some(open) = window_flag!(self.panels, id, &mut) {
                    *open = false;
                }
            }
        }
    }

    fn account(&self) -> Option<uuid::Uuid> {
        Some(self.account)
    }

    fn folder_windows(&self) -> Vec<(String, uuid::Uuid)> {
        self.inventory.folderstorm_folder_windows()
    }

    fn has_folder(&self, folder: uuid::Uuid) -> bool {
        self.folders.is_some_and(|inventory| inventory.folders.contains_key(&folder))
    }

    fn open_folder(&mut self, _: &egui::Context, folder: uuid::Uuid) -> Option<String> {
        // (@showinv hides the Inventory: no window on a part of it either)
        if self.no_inventory {
            return None;
        }
        self.inventory.folderstorm_open_folder(folder)
    }

    fn close_folder(&mut self, window: &str) {
        self.inventory.folderstorm_close_folder(window);
    }
}

impl App {
    /// Each frame, before the interface's pass: a layout's viewer window
    /// is put in place and kept locked, the windows of a layout applied in
    /// the last frames take their places, the monitors are watched, and
    /// the active layout is marked modified when something of it was moved
    /// since (`apply::frame`). The windows a layout was saved with open
    /// are opened through [`Windows`].
    pub(super) fn folderstorm_workspaces_frame(&mut self, window: &Window) {
        if !matches!(self.screen, Screen::World) {
            return;
        }
        let seen = self.folderstorm_viewer_window(window);
        self.layouts_demo(seen.as_ref());
        let rlva = &self.world.rlva;
        let mut windows = Windows {
            panels: &mut self.panels,
            inventory: &mut self.inventory_ui,
            // as `request_controls` has it
            classic_controls: self.settings.classic_controls && self.camera.free().is_none(),
            contacts_torn_off: self.settings.contacts.torn_off,
            // what `app/rlva.rs` closes every frame is not opened
            no_inventory: rlva.has("showinv"),
            no_minimap: rlva.has("showminimap"),
            no_world_map: rlva.has("showworldmap"),
            account: self.world.agent_id,
            folders: Some(&self.world.inventory),
        };
        let outcome = apply::frame(&self.egui_ctx, &mut apply::Live::of(&mut self.settings), seen, &mut windows);
        for command in &outcome.commands {
            self.folderstorm_viewer_command(window, *command);
        }
        // (not in mouselook or with the interface put away, where a key may
        // have asked: no window shows)
        if outcome.preferences && !self.camera.mouselook() && !ui::widgets::interface_hidden(&self.egui_ctx) {
            // as the Me menu's Preferences does, on our tab
            ui::widgets::open_floater(&self.egui_ctx, "options", &mut self.panels.settings);
            self.options_ui.open(ui::options::Page::Layout, 3);
        }
        if outcome.save {
            self.settings.save();
        }
    }

    /// In the interface's pass: the card shown when the monitors changed
    /// (`crate::folderstorm::workspaces::notice`) and the switcher
    /// (`crate::folderstorm::workspaces::switcher`). The card is not drawn
    /// where it could not be clicked: in mouselook, or with the interface
    /// put away; it waits for the interface to be back.
    pub(super) fn folderstorm_workspaces_notice(&self, ctx: &egui::Context, p: &crate::theme::Palette) {
        if matches!(self.screen, Screen::World) {
            let hidden = self.camera.mouselook() || ui::widgets::interface_hidden(ctx);
            crate::folderstorm::workspaces::interface(ctx, p, hidden);
        }
    }

    /// One of the layouts' keyboard actions (Preferences › Shortcuts; the
    /// arm of `on_input`): a request the next frame runs, as the menu's.
    pub(super) fn folderstorm_key(&mut self, key: crate::folderstorm::keys::Key) {
        use crate::folderstorm::workspaces::menu;
        let hidden = self.camera.mouselook() || ui::widgets::interface_hidden(&self.egui_ctx);
        if let Some(request) = menu::key_request(key, hidden) {
            menu::ask(&self.egui_ctx, request);
        }
    }

    /// The viewer window as the layouts see it. None while it is not
    /// theirs to move: a capture, a forced size (`AURORA_WINDOW_SIZE`).
    fn folderstorm_viewer_window(&self, window: &Window) -> Option<Seen> {
        use std::hash::{Hash, Hasher};
        if self.capture.is_some() || forced_window_size().is_some() {
            return None;
        }
        let mut id = std::collections::hash_map::DefaultHasher::new();
        window.id().hash(&mut id);
        let size = window.inner_size();
        let scale = window.scale_factor();
        let s = &self.settings;
        // (Aurora keeps the normal size in points, the corner in pixels)
        let pixels = |points: u32| (f64::from(points) * scale).round() as i32;
        Some(Seen {
            id: id.finish(),
            inner: (size.width, size.height),
            // (Wayland gives no position)
            outer: window.outer_position().ok().map(|at| (at.x, at.y)),
            scale: scale as f32,
            maximized: window.is_maximized(),
            fullscreen: window.fullscreen().is_some(),
            minimized: window.is_minimized().unwrap_or(false),
            decorated: window.is_decorated(),
            resizable: window.is_resizable(),
            normal: s
                .window_x
                .zip(s.window_y)
                .map(|(x, y)| [x, y, pixels(s.window_width), pixels(s.window_height)]),
        })
    }

    /// Does one thing a layout asks of the viewer window.
    fn folderstorm_viewer_command(&mut self, window: &Window, command: Command) {
        use winit::dpi::{PhysicalPosition, PhysicalSize};
        log::debug!("layouts: viewer window: {command:?}");
        match command {
            // Aurora's own full screen, entered and left by its own code
            // (`apply_fullscreen_setting`, before the next frame): the
            // windowed geometry it keeps stays right
            Command::Fullscreen(on) => self.settings.window_fullscreen = on,
            Command::Maximized(on) => window.set_maximized(on),
            Command::Decorations(on) => window.set_decorations(on),
            Command::Resizable(on) => window.set_resizable(on),
            Command::Position(x, y) => window.set_outer_position(PhysicalPosition::new(x, y)),
            Command::Size(width, height) => {
                let _ = window.request_inner_size(PhysicalSize::new(width, height));
            }
        }
        // the new place is the one Aurora remembers for the next start
        // (`record_window_geometry`)
        if matches!(command, Command::Position(..) | Command::Size(..) | Command::Maximized(_)) {
            self.geom_changed = Some(Instant::now());
        }
    }

    /// `AURORA_DEMO_LAYOUTS=<mode>`: two layouts seeded once in the demo's
    /// own settings, with Inventory, the world map and People open and
    /// Preferences on the Layouts tab. "Wide": the world on the right half
    /// of the window, the bars around it, the windows in the left margin.
    /// "Single": no viewport, the windows over the world. `1` only seeds
    /// them; `apply-wide` applies "Wide" at frame 180; `apply-single`
    /// applies "Wide" then, at frame 360, "Single" (windows made small in
    /// the margin get their sizes back); `reopen` closes Inventory and the
    /// world map at frame 120 and applies "Wide" at frame 180, which opens
    /// them where it has them.
    ///
    /// With a viewer window to move (`seen`: not under a forced size):
    /// `toggle` gives "Wide" the window as it is and "Single" its left
    /// half, and applies them in turn every 300 frames; `lock` gives
    /// "Wide" the window as it is, locked, and applies it; `unplug`, with
    /// pretend monitors (`AURORA_DEMO_MONITORS`), saves "Wide" on all of
    /// them and "Single" on all but the last, applies "Wide", and takes
    /// the last monitor away at frame 600: the card offers "Single".
    ///
    /// `AURORA_DEMO_SWITCHER=1` (checkpoint 5) seeds the same two layouts,
    /// makes both favorites and the two layouts of the toolbar's toggle,
    /// and opens the switcher instead of Preferences; `=toggle` also gives
    /// them the viewer window as `AURORA_DEMO_LAYOUTS=toggle` does, and
    /// asks for the toggle every 300 frames, as its toolbar button would.
    ///
    /// `AURORA_DEMO_WORKSPACES=1` (checkpoint 6) seeds the same as
    /// `AURORA_DEMO_SWITCHER=1` and two workspaces, both favorites:
    /// "Sorting" (Inventory, the additional Inventory window, People; with
    /// the layout "Single") and "Exploring" (mini-map, world map, Places;
    /// any layout). `=apply` also applies "Sorting" at frame 180 and
    /// "Exploring" at frame 360. `=folders` (part 2) opens windows on the
    /// first two folders of the demo's Inventory at frame 120, saves them
    /// at frame 240 as the workspace "Folders" (*Remember Inventory
    /// folders*), closes them at frame 300 and applies "Folders" at frame
    /// 420: they come back at their places.
    fn layouts_demo(&mut self, seen: Option<&Seen>) {
        use crate::folderstorm::workspaces::store::{Layout, Placement, Placements, Store, WindowFrame};
        use crate::folderstorm::workspaces::{capture, windows};
        static SEEDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !self.demo {
            return;
        }
        let mode = match (
            std::env::var("AURORA_DEMO_LAYOUTS"),
            std::env::var("AURORA_DEMO_SWITCHER"),
            std::env::var("AURORA_DEMO_WORKSPACES"),
        ) {
            (Ok(mode), _, _) => mode,
            (_, Ok(switcher), _) if switcher == "toggle" => "switcher-toggle".to_owned(),
            (_, Ok(_), _) => "switcher".to_owned(),
            (_, _, Ok(workspaces)) if workspaces == "apply" => "switcher-workspaces-apply".to_owned(),
            (_, _, Ok(workspaces)) if workspaces == "folders" => "switcher-workspaces-folders".to_owned(),
            (_, _, Ok(_)) => "switcher-workspaces".to_owned(),
            _ => return,
        };
        let workspaces = mode.starts_with("switcher-workspaces");
        let switcher = mode.starts_with("switcher");
        // the modes that flip the viewer window between the two layouts
        let flips = matches!(mode.as_str(), "toggle" | "switcher-toggle");
        let ctx = self.egui_ctx.clone();
        let Some(room) = capture::room(&ctx) else { return };
        if !SEEDED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let (width, height) = (room.frame.width, room.frame.height);
            let place = |x: f32, y: f32, w: f32, h: f32| Placement {
                x: x.round(),
                y: y.round(),
                width: w.round(),
                height: h.round(),
                minimized: false,
                // (saved with its windows open: an apply opens them)
                open: true,
            };
            let monitors = capture::monitors(&ctx);
            let frame = seen
                .filter(|_| flips || mode == "lock")
                .and_then(|seen| capture::viewer_window(seen, &monitors));
            // "Single": the left half of the window, never under its minimum
            let half_frame = match (&frame, seen) {
                (Some(WindowFrame::Normal { x, y, width, height, .. }), Some(seen)) if flips => Some(WindowFrame::Normal {
                    x: *x,
                    y: *y,
                    width: (*width / 2).max((800.0 * seen.scale).ceil() as u32),
                    height: *height,
                    maximized: false,
                    locked: false,
                }),
                _ => None,
            };
            let fewer = match mode.as_str() {
                "unplug" if monitors.len() > 1 => monitors[..monitors.len() - 1].to_vec(),
                _ => monitors.clone(),
            };
            let layout = |name: &str, world: viewport::ViewportSettings, places: [(&str, Placement); 3]| {
                let windows = Placements {
                    frame: room.frame,
                    windows: places.into_iter().map(|(id, place)| (id.to_owned(), place)).collect(),
                };
                let (window, monitors) = match name {
                    "Wide" => (frame.clone().map(|frame| frame.with_lock(mode == "lock")), monitors.clone()),
                    _ => (half_frame.clone(), fewer.clone()),
                };
                Layout::new(name, Some(world), Some(windows), monitors).with_window(window)
            };
            let half = (width * room.frame.pixels_per_point / 2.0).round() as u32;
            let wide = layout(
                "Wide",
                viewport::ViewportSettings {
                    enabled: true,
                    left: half,
                    bars_around: true,
                    ..Default::default()
                },
                [
                    ("inventory", place(8.0, 48.0, width * 0.2, height * 0.8)),
                    ("worldmap", place(16.0 + width * 0.2, 48.0, width * 0.26, height * 0.55)),
                    (
                        crate::ui::people::WINDOW,
                        place(16.0 + width * 0.2, 56.0 + height * 0.55, width * 0.2, height * 0.3),
                    ),
                ],
            );
            let single = layout(
                "Single",
                Default::default(),
                [
                    ("inventory", place(width - 400.0, 60.0, 380.0, height * 0.6)),
                    ("worldmap", place(width * 0.3, height * 0.2, 560.0, 440.0)),
                    (crate::ui::people::WINDOW, place(20.0, 60.0, 300.0, height * 0.5)),
                ],
            );
            debug_assert!(
                wide.windows
                    .iter()
                    .flat_map(|w| w.windows.keys())
                    .all(|id| windows::find(id).is_some())
            );
            let store = &mut self.settings.folderstorm_workspaces;
            *store = Store::default();
            for seeded in [wide, single] {
                if let Err(error) = store.add(seeded) {
                    log::warn!("AURORA_DEMO_LAYOUTS: layout not seeded: {error:?}");
                }
            }
            store.set_active(None);
            self.panels.inventory = true;
            self.panels.world_map = true;
            self.panels.people = true;
            if switcher {
                for (place, name) in ["Wide", "Single"].into_iter().enumerate() {
                    store.set_favorite(name, true);
                    store.set_toggle(place, Some(name));
                }
                if workspaces {
                    use crate::folderstorm::workspaces::workspace::Workspace;
                    let ids = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
                    let seeded = [
                        Workspace::new(
                            "Sorting",
                            Some("Single"),
                            ids(&["inventory", windows::ADDITIONAL_INVENTORY, crate::ui::people::WINDOW]),
                        ),
                        Workspace::new("Exploring", None, ids(&["minimap", "worldmap", "places"])),
                    ];
                    for workspace in seeded {
                        let name = workspace.name.clone();
                        if let Err(error) = store.add_workspace(workspace) {
                            log::warn!("AURORA_DEMO_WORKSPACES: workspace not seeded: {error:?}");
                        }
                        store.set_favorite_workspace(&name, true);
                    }
                    store.active_workspace = None;
                }
                crate::folderstorm::workspaces::switcher::open(&ctx);
            } else {
                self.panels.settings = true;
                self.options_ui.open(ui::options::Page::Layout, 3);
            }
        }
        let apply_at = |index: usize, frame: u64, app: &mut Self| {
            if app.frame_count == frame
                && let Err(error) = apply::apply(&ctx, &mut apply::Live::of(&mut app.settings), index)
            {
                log::warn!("AURORA_DEMO_LAYOUTS: layout not applied: {error:?}");
            }
        };
        match mode.as_str() {
            "apply-wide" | "lock" => apply_at(0, 180, self),
            // Inventory and the world map closed, then "Wide": it opens them
            "reopen" => {
                if self.frame_count == 120 {
                    self.panels.inventory = false;
                    self.panels.world_map = false;
                }
                apply_at(0, 180, self);
            }
            "apply-single" => {
                apply_at(0, 180, self);
                apply_at(1, 360, self);
            }
            "toggle" if self.frame_count >= 180 && (self.frame_count - 180).is_multiple_of(300) => {
                let index = ((self.frame_count - 180) / 300 % 2) as usize;
                apply_at(index, self.frame_count, self);
            }
            // as a click on the toolbar's toggle: "Wide", then "Single", …
            "switcher-toggle" if self.frame_count >= 180 && (self.frame_count - 180).is_multiple_of(300) => {
                crate::folderstorm::workspaces::menu::ask(&ctx, crate::folderstorm::workspaces::menu::Request::Toggle);
            }
            // "Sorting" (with its layout), then "Exploring" (on the same one)
            "switcher-workspaces-apply" => {
                for (index, frame) in [(0, 180), (1, 360)] {
                    if self.frame_count == frame
                        && let Err(error) = apply::apply_workspace(&ctx, &mut apply::Live::of(&mut self.settings), index)
                    {
                        log::warn!("AURORA_DEMO_WORKSPACES: workspace not applied: {error:?}");
                    }
                }
            }
            "switcher-workspaces-folders" => {
                let places = [egui::pos2(40.0, 80.0), egui::pos2(480.0, 120.0)];
                match self.frame_count {
                    120 => {
                        let inventory = &self.world.inventory;
                        let children = inventory.folders.get(&inventory.root).map(|root| root.children.clone());
                        for (folder, at) in children.unwrap_or_default().into_iter().zip(places) {
                            if let Some(window) = self.inventory_ui.folderstorm_open_folder(folder) {
                                ui::widgets::place_floater(&ctx, &window, at, egui::vec2(420.0, 440.0));
                            }
                        }
                    }
                    240 => {
                        let keep = apply::Keep {
                            layout: false,
                            folders: true,
                        };
                        if let Err(error) = apply::save_new_workspace(&ctx, &mut apply::Live::of(&mut self.settings), "Folders", keep) {
                            log::warn!("AURORA_DEMO_WORKSPACES: folders not saved: {error:?}");
                        }
                    }
                    300 => {
                        for (window, _) in self.inventory_ui.folderstorm_folder_windows() {
                            self.inventory_ui.folderstorm_close_folder(&window);
                        }
                    }
                    420 => {
                        let index = self
                            .settings
                            .folderstorm_workspaces
                            .find_workspace("Folders")
                            .map(|(index, _)| index);
                        if let Some(index) = index
                            && let Err(error) = apply::apply_workspace(&ctx, &mut apply::Live::of(&mut self.settings), index)
                        {
                            log::warn!("AURORA_DEMO_WORKSPACES: workspace not applied: {error:?}");
                        }
                    }
                    _ => {}
                }
            }
            "unplug" => {
                apply_at(0, 180, self);
                if self.frame_count == 600 {
                    crate::folderstorm::diagram::demo_unplug();
                }
            }
            _ => {}
        }
    }

    /// The bars' area this frame, in points: inside the custom world
    /// viewport `world` (the renderer's, in window pixels, already under
    /// the session host's bar), or around it: the top and bottom bars
    /// above and below the world, the side toolbars in its left and right
    /// margins, each where it has room (`viewport::bars`). None leaves the
    /// bars on the whole window of `size` pixels.
    pub(super) fn folderstorm_bars_rect(&self, ctx: &egui::Context, world: Option<[u32; 4]>, size: (u32, u32)) -> Option<egui::Rect> {
        let s = &self.settings.folderstorm_viewport;
        let inside = viewport::bars_rect(s, world, ctx.pixels_per_point()).filter(|_| s.bars_around);
        let Some(inside) = inside else {
            viewport::bars::set_sides(ctx, None);
            return viewport::bars_rect(s, world, ctx.pixels_per_point());
        };
        let mut bars = viewport::bars::thickness(ctx, self.interface.last_room.map(|(room, _)| room));
        // the session host's bar is docked over the world's top
        // (`app/folderstorm_characters/bar.rs`): the top bars stay under it
        if self.whole_world_viewport(size) != world {
            bars.top = 0.0;
        }
        let areas = viewport::bars::Areas::around(inside, ctx.content_rect(), bars);
        viewport::bars::set_sides(ctx, Some((inside, areas.sides)));
        Some(areas.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::apply::Shell;
    use crate::folderstorm::workspaces::windows;

    fn closed() -> ui::Panels {
        let mut panels = ui::Panels::from_settings(&crate::settings::Settings::default());
        for window in windows::all() {
            if let Some(open) = window_flag!(panels, window.id, &mut) {
                *open = false;
            }
        }
        panels
    }

    /// Every window of the allowlist has its way of being opened here: a
    /// window added to the list without one fails this test.
    #[test]
    fn every_allowlisted_window_is_opened_the_app_s_way() {
        for classic_controls in [false, true] {
            let ctx = egui::Context::default();
            let mut panels = closed();
            let mut inventory = ui::inventory::InventoryUi::default();
            let mut shell = Windows {
                panels: &mut panels,
                inventory: &mut inventory,
                classic_controls,
                contacts_torn_off: true,
                no_inventory: false,
                no_minimap: false,
                no_world_map: false,
                account: uuid::Uuid::nil(),
                folders: None,
            };
            for window in windows::all() {
                assert!(!shell.is_open(window), "{}", window.id);
                // the controls of the other look are not windows of now
                let other_look = match window.id {
                    ui::controls::WINDOW => classic_controls,
                    ui::controls::CLASSIC_CAMERA | ui::controls::CLASSIC_MOVEMENT => !classic_controls,
                    _ => false,
                };
                assert_eq!(shell.open(&ctx, window), !other_look, "{}", window.id);
                assert_eq!(shell.is_open(window), !other_look, "{}", window.id);
            }
            assert!(panels.inventory && panels.chat && panels.minimap && panels.world_map && panels.money_tracker);
            assert!(inventory.additional_open());
            assert_eq!(
                panels.movement_controls, classic_controls,
                "Aurora's window opens on its Camera tab"
            );
        }
    }

    /// A workspace closes every window it switches the app's way.
    #[test]
    fn every_allowlisted_window_is_closed_the_app_s_way() {
        for classic_controls in [false, true] {
            let ctx = egui::Context::default();
            let mut panels = closed();
            let mut inventory = ui::inventory::InventoryUi::default();
            let mut shell = Windows {
                panels: &mut panels,
                inventory: &mut inventory,
                classic_controls,
                contacts_torn_off: true,
                no_inventory: false,
                no_minimap: false,
                no_world_map: false,
                account: uuid::Uuid::nil(),
                folders: None,
            };
            for window in windows::all() {
                if shell.open(&ctx, window) {
                    assert!(shell.is_open(window), "{}", window.id);
                    shell.close(&ctx, window);
                }
                assert!(!shell.is_open(window), "{}", window.id);
            }
            assert!(!panels.inventory && !panels.minimap && !panels.camera_controls && !panels.movement_controls);
            assert!(!inventory.additional_open());
        }
    }

    /// A workspace's folder windows go through the Inventory's own way of
    /// opening one; none while RLVa hides the Inventory, and only on a
    /// folder the Inventory has.
    #[test]
    fn folder_windows_are_opened_listed_and_closed_through_the_inventory() {
        let ctx = egui::Context::default();
        let mut panels = closed();
        let mut inventory = ui::inventory::InventoryUi::default();
        let world = crate::world::inventory::Inventory::default();
        let folder = uuid::Uuid::from_u128(8000);
        for no_inventory in [false, true] {
            let mut shell = Windows {
                panels: &mut panels,
                inventory: &mut inventory,
                classic_controls: false,
                contacts_torn_off: false,
                no_inventory,
                no_minimap: false,
                no_world_map: false,
                account: uuid::Uuid::from_u128(7),
                folders: Some(&world),
            };
            assert_eq!(shell.account(), Some(uuid::Uuid::from_u128(7)));
            assert!(!shell.has_folder(folder), "not in this Inventory");
            if no_inventory {
                assert_eq!(shell.open_folder(&ctx, folder), None, "@showinv");
                continue;
            }
            let window = shell.open_folder(&ctx, folder).expect("opened");
            assert_eq!(shell.folder_windows(), [(window.clone(), folder)]);
            shell.close_folder(&window);
            assert!(shell.folder_windows().is_empty());
        }
    }

    #[test]
    fn what_rlva_hides_or_the_settings_do_not_have_is_left_closed() {
        let ctx = egui::Context::default();
        let mut panels = closed();
        let mut inventory = ui::inventory::InventoryUi::default();
        let mut shell = Windows {
            panels: &mut panels,
            inventory: &mut inventory,
            classic_controls: false,
            contacts_torn_off: false,
            no_inventory: true,
            no_minimap: true,
            no_world_map: true,
            account: uuid::Uuid::nil(),
            folders: None,
        };
        for id in ["inventory", windows::ADDITIONAL_INVENTORY, "minimap", "worldmap", "contacts"] {
            let window = windows::find(id).expect("on the list");
            assert!(!shell.open(&ctx, window), "{id}");
            assert!(!shell.is_open(window), "{id}");
        }
        // the others still open
        assert!(shell.open(&ctx, windows::find("places").expect("on the list")));
        assert!(!panels.inventory && !panels.minimap && !panels.world_map && !panels.contacts && panels.places);
        assert!(!inventory.additional_open());
    }
}
