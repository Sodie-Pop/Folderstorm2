//! Folderstorm: the margins' desktop in the app (the rules are in
//! `crate::folderstorm::desktop`; folderstorm/plans/margin-desktop.md).
//!
//! Gives the desktops what the app knows this frame (what greys a toolbar
//! button, whether the interface is hidden), and runs what a shortcut asks
//! through Aurora's own path: a command through `ui::bars::run_command`,
//! its action handed to the app in the frame's one slot (`UiActions::bar`),
//! exactly as a toolbar button does, so every check the app makes on that
//! action (RLVa, the region's rights) is made here too. A folder shortcut
//! opens the folder's own window (`InventoryUi::open_folder_window`, as
//! *Open in a new window* does), one window per folder, and never while
//! RLVa hides the Inventory (`@showinv`). Folders come from the Inventory's
//! menu (*Add to desktop*, `InvAction::FolderstormDesktopAdd`) or are
//! dropped on a desktop; only their ids are kept.

use super::*;
use crate::folderstorm::desktop::{self, Facts, Folders, Request, place, targets};
use crate::world::notice_topics::Topic;
use uuid::Uuid;

/// The size a folder's window opens with (Aurora's own, `ui/inventory.rs`).
const FOLDER_WINDOW: egui::Vec2 = egui::vec2(420.0, 440.0);

/// What opening a folder shortcut did.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Opened {
    /// RLVa hides the Inventory: no window on a part of it either.
    Refused,
    /// The folder is not one a shortcut opens (gone since it was drawn).
    Gone,
    /// Its window was open: brought to the front (the floater's id).
    Front(String),
    /// A new window, placed beside the shortcut.
    New(String),
}

/// Opens the window of `folder`, or brings it to the front when it is open:
/// one window per folder. A new one goes beside the shortcut (`cell`) in
/// its `room`.
fn open_folder(
    ctx: &egui::Context,
    windows: &mut ui::inventory::InventoryUi,
    inv: &crate::world::inventory::Inventory,
    folder: Uuid,
    no_inventory: bool,
    cell: egui::Rect,
    room: egui::Rect,
) -> Opened {
    if no_inventory {
        return Opened::Refused;
    }
    if targets::folder_state(inv, folder) != targets::FolderState::Live {
        return Opened::Gone;
    }
    if let Some((window, _)) = windows.folderstorm_folder_windows().into_iter().find(|(_, root)| *root == folder) {
        // (expanded when it was collapsed)
        ui::widgets::bring_floater_front(ctx, &window);
        return Opened::Front(window);
    }
    let Some(window) = windows.folderstorm_open_folder(folder) else {
        return Opened::Gone;
    };
    ui::widgets::place_floater(ctx, &window, place::beside(cell, room, FOLDER_WINDOW), FOLDER_WINDOW);
    Opened::New(window)
}

/// The desktop changed and is not saved yet.
fn dirty_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_dirty")
}

/// Runs the command `id` as its toolbar button does, behind the button's
/// gates (`targets::command_gate`, checked again here: what greys it may
/// have changed since the shortcut was drawn). The action for the app.
fn run(ctx: &egui::Context, id: &str, facts: &Facts, panels: &mut ui::Panels) -> BarAction {
    if targets::command_gate(id, facts).is_err() {
        return BarAction::None;
    }
    ui::bars::run_command(ctx, id, panels)
}

impl App {
    /// In the interface's pass, once the bars are laid out: the desktops
    /// of the margins, and what their shortcuts ask.
    pub(super) fn folderstorm_desktop(&mut self, ctx: &egui::Context, p: &crate::theme::Palette, a: &mut UiActions) {
        self.desktop_demo(ctx);
        if !self.settings.folderstorm_desktop.shortcuts_on {
            return;
        }
        let facts = self.desktop_facts(ctx);
        // (RlvBehaviourToggleHandler<RLV_BHVR_SHOWINV>: the Inventory is hidden)
        let no_inventory = self.world.rlva.has("showinv");
        let prefs = &self.settings.inventory;
        // hidden with the interface, and in mouselook as the bars are
        let mouselook = self.camera.mouselook() && !self.settings.camera.show_interface_in_mouselook;
        let env = desktop::Env {
            p,
            icons: &self.skin.icons,
            facts,
            folders: Folders {
                inv: &self.world.inventory,
                account: self.world.agent_id,
                no_inventory,
                colors: prefs.folder_colors_enabled.then_some(&prefs.folder_colors),
            },
            hidden: mouselook || self.interface.hidden || self.interface.immersive || ui::widgets::interface_hidden(ctx),
        };
        let mut requests = desktop::show(ctx, &mut self.settings.folderstorm_desktop, &env);
        requests.extend(self.desktop_demo_frame(ctx));
        for request in requests {
            match request {
                Request::Run(command) => {
                    let action = run(ctx, &command, &facts, &mut self.panels);
                    if !matches!(action, BarAction::None) {
                        a.bar = action;
                    }
                }
                Request::OpenFolder { folder, cell, room } => {
                    let opened = open_folder(ctx, &mut self.inventory_ui, &self.world.inventory, folder, no_inventory, cell, room);
                    if opened == Opened::Refused {
                        self.world.system_notice(aurora_i18n::tr!("rlva-blocked"), Topic::Warning);
                    }
                }
                Request::ShowFolder(folder) => {
                    if no_inventory {
                        self.world.system_notice(aurora_i18n::tr!("rlva-blocked"), Topic::Warning);
                    } else if targets::folder_state(&self.world.inventory, folder) == targets::FolderState::Live {
                        self.panels.inventory = true;
                        self.inventory_ui.show_folder(&self.world.inventory, folder);
                        ui::widgets::bring_floater_front(ctx, "inventory");
                    }
                }
                Request::Notice(text) => self.world.system_notice(text, Topic::Items),
                Request::Changed => {
                    ctx.data_mut(|d| d.insert_temp(dirty_id(), true));
                }
                Request::Settings => {
                    self.panels.settings = true;
                    self.options_ui.open(ui::options::Page::Layout, 0);
                    self.options_ui.reveal = Some((ui::options::Page::Layout, 0, aurora_i18n::tr!("fs-desktop-shortcuts")));
                }
            }
        }
        // saved once the pointer is up, as the gesture boards are
        let dirty = ctx.data(|d| d.get_temp::<bool>(dirty_id())).unwrap_or(false);
        if dirty && !ctx.input(|i| i.pointer.any_down()) {
            ctx.data_mut(|d| d.insert_temp(dirty_id(), false));
            self.settings.save();
        }
    }

    /// *Add to desktop* of the Inventory's menu
    /// (`InvAction::FolderstormDesktopAdd`): a shortcut to each folder of
    /// `ids`, in the first free cells of the first desktop. It turns the
    /// shortcuts on when they are off, and says so.
    pub(super) fn folderstorm_desktop_add(&mut self, ids: Vec<Uuid>) {
        let ctx = self.egui_ctx.clone();
        let no_inventory = self.world.rlva.has("showinv");
        let picked = match targets::offer(&self.world.inventory, &ids, no_inventory) {
            Ok(picked) => picked,
            Err(why) => {
                self.world.system_notice(why.text(), Topic::Warning);
                return;
            }
        };
        let account = self.world.agent_id;
        let d = &mut self.settings.folderstorm_desktop;
        let rooms = desktop::rooms(&ctx, d.size);
        let added = desktop::add_folders(d, &rooms, account, &picked.folders, None, None);
        let turned_on = added.added > 0 && !d.shortcuts_on;
        if added.added > 0 {
            d.shortcuts_on = true;
            self.settings.save();
        }
        if let Some(text) = desktop::added_notice(&added, picked.left_out, turned_on, !rooms.is_empty()) {
            self.world.system_notice(text, Topic::Items);
        }
    }

    /// Whether the pointer is over a desktop (the hook of
    /// `ui_takes_pointer`): a press, a release or the wheel there is the
    /// interface's, never the world's.
    pub(super) fn folderstorm_desktop_takes_pointer(&self) -> bool {
        let ppp = self.egui_ctx.pixels_per_point();
        desktop::takes_pointer(&self.egui_ctx, egui::pos2(self.cursor_pos.0 / ppp, self.cursor_pos.1 / ppp))
    }

    /// What greys a toolbar button this frame: the facts the toolbar's row
    /// reads (`ui::bars`), from where the app puts them.
    fn desktop_facts(&self, ctx: &egui::Context) -> Facts {
        use crate::folderstorm::workspaces::toolbar;
        Facts {
            photo_roll_empty: self.toolbar_ui.photo_roll_empty,
            no_guide: !self.toolbar_ui.destinations,
            // (set each frame by `draw_ui`, RlvActions::canStand / canGroundSit)
            sit_locked: ctx.data(|d| d.get_temp::<bool>(ui::bars::rlva_sit_locked_id())).unwrap_or(false),
            toggle_unset: !toolbar::look(ctx, toolbar::TOGGLE, false, true, String::new()).1,
        }
    }

    /// `AURORA_DEMO_DESKTOP=<mode>`: a desktop made once, at the start of a
    /// demo run (in the demo's own settings), the viewport turned on when
    /// it is off (`crate::folderstorm::desktop::demo`).
    fn desktop_demo(&mut self, ctx: &egui::Context) {
        static SEEDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !self.demo || SEEDED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Ok(mode) = std::env::var("AURORA_DEMO_DESKTOP") else {
            return;
        };
        let width = (ctx.content_rect().width() * ctx.pixels_per_point()).round() as u32;
        let s = &mut self.settings;
        if !desktop::demo::seed(&mode, &mut s.folderstorm_desktop, &mut s.folderstorm_viewport, width) {
            log::warn!("AURORA_DEMO_DESKTOP={mode}: not a mode of the demo");
            return;
        }
        // the demo inventory, for the folder shortcuts
        let agent = self.world.agent_id;
        crate::world::inventory::demo::seed(&mut self.world.inventory, agent);
        desktop::demo::seed_folders(&mut s.folderstorm_desktop, agent);
        if mode == "rlva" {
            self.settings.rlva_enabled = true;
            self.world.rlva_configure(true);
            self.rlva_demo_message("@showinv=n".to_owned());
        }
    }

    /// What the demo asks at this frame, as a shortcut would: `open` opens
    /// a folder shortcut then a command, `drop` adds a folder where a drop
    /// from the Inventory would then moves it to the third monitor, `rlva`
    /// tries to open a folder shortcut under `@showinv`.
    fn desktop_demo_frame(&mut self, ctx: &egui::Context) -> Vec<Request> {
        use desktop::demo;
        if !self.demo || ![demo::OPEN_FRAME, demo::SECOND_FRAME].contains(&self.frame_count) {
            return Vec::new();
        }
        let Ok(mode) = std::env::var("AURORA_DEMO_DESKTOP") else {
            return Vec::new();
        };
        let first = self.frame_count == demo::OPEN_FRAME;
        let d = &mut self.settings.folderstorm_desktop;
        let rooms = desktop::rooms(ctx, d.size);
        match (mode.as_str(), first) {
            ("open" | "rlva", true) => {
                let Some(room) = rooms.first() else {
                    return Vec::new();
                };
                vec![Request::OpenFolder {
                    folder: demo::folder(0),
                    cell: room.grid.rect(demo::FOLDERS_AT),
                    room: room.rect,
                }]
            }
            ("open", false) => vec![Request::Run(demo::OPENED.to_owned())],
            ("drop", true) => {
                desktop::add_folders(d, &rooms, self.world.agent_id, &[demo::DROPPED], Some(0), Some(demo::DROPPED_AT));
                Vec::new()
            }
            ("drop", false) => {
                let dropped = d
                    .shortcuts
                    .iter()
                    .find(|s| matches!(s.target, desktop::store::Target::Folder { folder, .. } if folder == demo::DROPPED))
                    .map(|s| s.id);
                if let Some(id) = dropped {
                    d.place(id, demo::MOVED_TO.0, demo::MOVED_TO.1);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels() -> ui::Panels {
        ui::Panels::from_settings(&crate::settings::Settings::default())
    }

    #[test]
    fn a_command_shortcut_runs_what_its_toolbar_button_runs() {
        let ctx = egui::Context::default();
        let free = Facts::default();
        // a window command toggles its panel, as the button does
        let mut p = panels();
        let was = p.gestures;
        assert!(matches!(run(&ctx, "gestures", &free, &mut p), BarAction::None));
        assert_eq!(p.gestures, !was);
        assert!(matches!(run(&ctx, "gestures", &free, &mut p), BarAction::None));
        assert_eq!(p.gestures, was, "open closes");
        // a command of the app is handed over as an action
        assert!(matches!(run(&ctx, "home", &free, &mut p), BarAction::TeleportHome));
        assert!(matches!(run(&ctx, "sit", &free, &mut p), BarAction::SitGround));
        assert!(matches!(run(&ctx, "stop_animations", &free, &mut p), BarAction::StopAnimations));
    }

    #[test]
    fn a_command_stops_at_its_button_s_gates() {
        let ctx = egui::Context::default();
        let mut p = panels();
        // Sit while RLVa would refuse it
        let locked = Facts {
            sit_locked: true,
            ..Default::default()
        };
        assert!(matches!(run(&ctx, "sit", &locked, &mut p), BarAction::None));
        // nothing to show
        let empty = Facts {
            photo_roll_empty: true,
            no_guide: true,
            ..Default::default()
        };
        assert!(matches!(run(&ctx, "photo_roll", &empty, &mut p), BarAction::None));
        let was = p.destinations;
        assert!(matches!(run(&ctx, "destinations", &empty, &mut p), BarAction::None));
        assert_eq!(p.destinations, was);
        assert!(matches!(run(&ctx, "photo_roll", &Facts::default(), &mut p), BarAction::PhotoRoll));
        // a button that is its own control, a command not in Aurora yet,
        // one the catalogue does not have
        for id in ["favorites", "vr_view", "speak", "flickr", "no-such-command"] {
            assert!(matches!(run(&ctx, id, &Facts::default(), &mut p), BarAction::None), "{id}");
        }
    }

    #[test]
    fn a_folder_shortcut_opens_one_window_and_then_brings_it_forward() {
        let ctx = egui::Context::default();
        let mut inv = crate::world::inventory::Inventory::default();
        crate::world::inventory::demo::seed(&mut inv, Uuid::from_u128(42));
        let (mine, sub) = (Uuid::from_u128(8000), Uuid::from_u128(8001));
        let mut windows = ui::inventory::InventoryUi::default();
        let room = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(1900.0, 1000.0));
        let cell = egui::Rect::from_min_size(room.min, egui::vec2(92.0, 84.0));
        // @showinv: no window on a part of the Inventory
        assert_eq!(open_folder(&ctx, &mut windows, &inv, mine, true, cell, room), Opened::Refused);
        assert!(windows.folderstorm_folder_windows().is_empty());
        // a new window
        let Opened::New(first) = open_folder(&ctx, &mut windows, &inv, mine, false, cell, room) else {
            panic!("a new window");
        };
        assert_eq!(windows.folderstorm_folder_windows(), [(first.clone(), mine)]);
        // again: the same window, to the front (expanded when collapsed)
        ui::widgets::minimize_floater(&ctx, &first);
        assert_eq!(open_folder(&ctx, &mut windows, &inv, mine, false, cell, room), Opened::Front(first.clone()));
        assert_eq!(windows.folderstorm_folder_windows().len(), 1, "one window per folder");
        // another folder: its own window
        assert!(matches!(open_folder(&ctx, &mut windows, &inv, sub, false, cell, room), Opened::New(_)));
        assert_eq!(windows.folderstorm_folder_windows().len(), 2);
        // closed with its cross: a new one next time
        windows.folderstorm_close_folder(&first);
        assert!(matches!(open_folder(&ctx, &mut windows, &inv, mine, false, cell, room), Opened::New(_)));
        // gone since the shortcut was drawn, the Library, the root: nothing
        let before = windows.folderstorm_folder_windows().len();
        for folder in [Uuid::from_u128(123_456), inv.lib_root, inv.root] {
            assert_eq!(open_folder(&ctx, &mut windows, &inv, folder, false, cell, room), Opened::Gone);
        }
        assert_eq!(windows.folderstorm_folder_windows().len(), before);
    }
}
