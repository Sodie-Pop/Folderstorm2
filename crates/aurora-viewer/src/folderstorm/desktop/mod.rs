//! Folderstorm: the margins' desktop (FEATURES.md row 1.10;
//! folderstorm/plans/margin-desktop.md).
//!
//! Asked by the human; Folderstorm (C++) had nothing of it. On each monitor
//! under the viewer window that the world viewport leaves free, a desktop:
//! shortcuts on a grid, over the margins' background (row 1.8) and under
//! every window. A shortcut to a toolbar command runs what its toolbar
//! button runs (`ui::bars::run_command`, through the app's glue,
//! `app/folderstorm_desktop.rs`), behind the same gates. A shortcut to an
//! Inventory folder opens that folder's own window (Aurora's *Open in a
//! new window*), one window per folder; it keeps the folder's id, nothing else.
//! A shortcut to an Inventory item opens it as the Inventory does (a
//! landmark: the teleport, after Aurora's question; a notecard: its
//! window); it keeps the item's id, never its asset's. A shortcut to an
//! outfit wears it, as *Replace Current Outfit* does, after the question
//! of outfit safety.
//! Each account has its own desktop ([`store`]): the shortcuts shown are
//! those of the account logged in, and the choices of Preferences are the
//! profile's. A shortcut can be renamed, given another icon ([`icons`]),
//! moved to another monitor and arranged by name; a toolbar button is
//! dropped on a desktop, or sent there from its menu ([`toolbar_menu`]).
//! A taskbar ([`taskbar`], optional, off by default) along the bottom of
//! one margin monitor holds the collapsed windows as buttons.
//!
//! - [`store`]: what `settings.json` keeps (field `folderstorm_desktop`).
//! - [`place`]: slots, rooms and cells (pure maths).
//! - [`targets`]: what a shortcut shows, and the gates of a command.
//! - [`surface`]: the areas, the shortcuts' faces, clicks, drags, menus,
//!   the rename field and the keyboard.
//! - [`icons`]: the icons a shortcut can be given.
//! - [`taskbar`]: the windows seen, those held, the island of buttons.
//! - [`demo`]: the demo, `AURORA_DEMO_DESKTOP`.
//!
//! Drawing order: each desktop is an `egui::Area` of `Order::Background`.
//! egui paints such areas after its own background layer, where the
//! world's labels, the end-of-labels marker of the renderer and the
//! margins' background are, and before every window. So a shortcut is
//! never cut to the world viewport with the labels, and a window always
//! covers it. Nothing is drawn, and nothing is computed, while the switch
//! is off or the world fills the window.

pub mod demo;
pub mod icons;
pub mod place;
pub mod store;
pub mod surface;
pub mod targets;
pub mod taskbar;

#[cfg(test)]
mod taskbar_tests;
#[cfg(test)]
mod tests;

pub use store::Desktop;
pub use surface::{Added, add_folders, add_targets};
pub use targets::{Facts, Folders, Wearing};

use crate::folderstorm::viewport;
use crate::theme::Palette;
use crate::ui::icons::Icons;
use aurora_i18n::tr;
use egui::{Rect, vec2};
use uuid::Uuid;

/// What the app gives the desktops each frame.
pub struct Env<'a> {
    pub p: &'a Palette,
    pub icons: &'a Icons,
    /// What greys a toolbar button now.
    pub facts: Facts,
    /// The Inventory as the folder shortcuts read it: RLVa's `@showinv`, the folders' colours.
    pub folders: Folders<'a>,
    /// The interface is hidden (its shortcut, immersive mode, mouselook
    /// without the interface): no desktop.
    pub hidden: bool,
    /// The keyboard goes to the desktop now (the key action *Focus the
    /// desktop*): to the selected shortcut, else to the first one.
    pub focus: bool,
}

/// What the desktops ask the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Run this command of the toolbar catalogue, as its button does.
    Run(String),
    /// Open this folder's own window, or bring it to the front; a new one
    /// goes beside the shortcut (`cell`) in its `room`.
    OpenFolder { folder: Uuid, cell: Rect, room: Rect },
    /// Show this folder in the main Inventory window.
    ShowFolder(Uuid),
    /// Open this item of the Inventory as the Inventory opens its kind (a
    /// landmark: teleport there, after Aurora's question; a notecard: its
    /// window).
    OpenItem(Uuid),
    /// Show this item in the main Inventory window.
    ShowItem(Uuid),
    /// Wear this outfit (its folder of My Outfits), after the question of
    /// outfit safety.
    WearOutfit(Uuid),
    /// Say this in the chat's notices (what a drop added, or why not).
    Notice(String),
    /// The desktop changed: save the settings once the pointer is up.
    Changed,
    /// Open Preferences on the desktop's group.
    Settings,
    /// Open Aurora's *Toolbar buttons* window, to drag a command from.
    ToolbarButtons,
}

/// What another part of the viewer asks the desktops: taken by the app's
/// glue at its next frame ([`take_asks`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// A shortcut to this command of the toolbar catalogue, in the first
    /// free cell of the first desktop (*Add to desktop* of a toolbar
    /// button's menu).
    AddCommand(String),
    /// The keyboard to the desktop (the key action *Focus the desktop*).
    Focus,
    /// The keyboard to the taskbar (the key action *Focus the taskbar*).
    FocusTaskbar,
}

fn asks_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_asks")
}

pub fn ask(ctx: &egui::Context, ask: Ask) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Ask>>(asks_id()).push(ask));
}

pub fn take_asks(ctx: &egui::Context) -> Vec<Ask> {
    ctx.data_mut(|d| d.remove_temp::<Vec<Ask>>(asks_id())).unwrap_or_default()
}

/// The icon of *Add to desktop* (the Inventory's entry has the same).
pub const ICON_ADD_TO_DESKTOP: &str = "device-desktop-filled";

/// *Add to desktop* in the right-click menu of a toolbar button (the hook
/// of `ToolbarUi::context`; `command`: the button's, None on the toolbar
/// itself). Greyed, with the reason, for a command that never opens from a
/// shortcut (its button is its control, or Aurora cannot run it yet).
pub fn toolbar_menu(ui: &mut egui::Ui, p: &Palette, command: Option<&str>) {
    let Some(command) = command else {
        return;
    };
    let refused = targets::refuses_adding(command);
    let why = refused.map_or("", |why| why.text());
    if crate::ui::menu::item_tip(ui, p, ICON_ADD_TO_DESKTOP, tr!("fs-desktop-add"), refused.is_none(), why) {
        ask(ui.ctx(), Ask::AddCommand(command.to_owned()));
    }
}

/// The command of the toolbar button being dragged (from a toolbar or
/// from the *Toolbar buttons* window), for a drop on a desktop.
pub(crate) fn toolbar_cargo(ctx: &egui::Context) -> Option<String> {
    #[cfg(test)]
    if let Some(command) = ctx.data(|d| d.get_temp::<String>(tests::toolbar_cargo_id())) {
        return Some(command);
    }
    crate::ui::toolbar::ToolbarUi::carried_command(ctx)
}

/// What to say after a command was added from a toolbar button's menu.
/// `turned_on`: the shortcuts were off and this turned them on; `shown`: a
/// desktop shows now.
pub fn command_notice(added: surface::CommandAdded, turned_on: bool, shown: bool) -> String {
    match added {
        surface::CommandAdded::New(_) => {
            let mut parts = vec![tr!("fs-desktop-command-added").to_owned()];
            if turned_on {
                parts.push(tr!("fs-desktop-added-on").to_owned());
            }
            if !shown {
                parts.push(tr!("fs-desktop-added-hidden").to_owned());
            }
            parts.join(" ")
        }
        surface::CommandAdded::Existing(_) => tr!("fs-desktop-add-existing").to_owned(),
        surface::CommandAdded::Refused(why) => why.text(),
        surface::CommandAdded::Never(why) => why.text().to_owned(),
    }
}

/// How many shortcuts of the account stand on a monitor that has no room
/// now, while another has one (Preferences' note, with *Bring them here*).
/// None of them counts while no desktop shows at all: they are all kept
/// for when the window leaves room around the world.
pub fn unused(ctx: &egui::Context, desktop: &Desktop) -> usize {
    let live: Vec<u8> = rooms(ctx, desktop).iter().map(|room| room.slot).collect();
    if live.is_empty() { 0 } else { desktop.unused(&live) }
}

/// *Bring them here*: the shortcuts of the monitors not in use go to the
/// first desktop that shows.
pub fn bring_here(ctx: &egui::Context, desktop: &mut Desktop) -> surface::Brought {
    let rooms = rooms(ctx, desktop);
    surface::bring_here(desktop, &rooms)
}

/// Where the bars are this frame, for the rooms: confined to the world
/// viewport (inside it or around it), or over the whole window.
fn bars(ctx: &egui::Context) -> place::Bars {
    let rows = viewport::bars_screen(ctx);
    if rows == ctx.content_rect() {
        place::Bars::Window(crate::ui::layout::work_area(ctx))
    } else {
        place::Bars::Confined(viewport::bars::side_screen(ctx).union(rows))
    }
}

/// The rooms of this frame before the taskbar takes its strip: one for
/// each slot the world and the bars leave a cell in. None while the world
/// fills the window or no world is shown (login, a photo taking the whole
/// window).
fn bare_rooms(ctx: &egui::Context, cell: egui::Vec2) -> Vec<(u8, Rect)> {
    let Some(world) = viewport::world_area(ctx) else {
        return Vec::new();
    };
    let bars = bars(ctx);
    let layout = viewport::monitors(ctx);
    place::slots(ctx.content_rect(), layout.as_ref(), ctx.pixels_per_point())
        .into_iter()
        .enumerate()
        .filter_map(|(slot, rect)| Some((slot as u8, place::room(rect, world, bars, cell)?)))
        .collect()
}

/// Where the taskbar stands this frame: its slot and its strip, along the
/// bottom of that slot's room. None while it is off, without a margin, or
/// when the monitor chosen for it has no room.
pub fn taskbar_strip(ctx: &egui::Context, desktop: &Desktop) -> Option<(u8, Rect)> {
    if !desktop.taskbar.on {
        return None;
    }
    let [w, h] = desktop.size.cell();
    let rooms = bare_rooms(ctx, vec2(w, h));
    let slot = place::taskbar_slot(&rooms, desktop.taskbar.slot)?;
    let (_, room) = rooms.iter().find(|(s, _)| *s == slot)?;
    Some((slot, place::taskbar_strip(*room).0))
}

/// The slots that have a room this frame: the monitors the taskbar can be
/// put on (Preferences).
pub fn live_slots(ctx: &egui::Context, desktop: &Desktop) -> Vec<u8> {
    let [w, h] = desktop.size.cell();
    bare_rooms(ctx, vec2(w, h)).iter().map(|(slot, _)| *slot).collect()
}

/// The desktops' rooms of this frame: one for each slot the world, the
/// bars and the taskbar leave a cell in.
pub fn rooms(ctx: &egui::Context, desktop: &Desktop) -> Vec<surface::Room> {
    let [w, h] = desktop.size.cell();
    let cell = vec2(w, h);
    let rooms = bare_rooms(ctx, cell);
    let taskbar = if desktop.taskbar.on {
        place::taskbar_slot(&rooms, desktop.taskbar.slot)
    } else {
        None
    };
    rooms
        .into_iter()
        .filter_map(|(slot, rect)| {
            let rect = if taskbar == Some(slot) {
                place::taskbar_strip(rect).1
            } else {
                rect
            };
            let grid = place::Grid::new(rect, cell)?;
            Some(surface::Room { slot, rect, grid })
        })
        .collect()
}

/// The desktops of this frame, drawn; what they ask is returned. To be
/// called in the interface's pass once the bars are laid out.
pub fn show(ctx: &egui::Context, desktop: &mut Desktop, env: &Env) -> Vec<Request> {
    if !desktop.shortcuts_on || env.hidden {
        return Vec::new();
    }
    let rooms = rooms(ctx, desktop);
    if rooms.is_empty() {
        return Vec::new();
    }
    surface::show(ctx, desktop, env, &rooms)
}

/// What to say after folders and items were added (`added`), `left_out`
/// entries of the selection not being ones that can be shortcuts. `turned_on`: the
/// shortcuts were off and this turned them on; `shown`: a desktop shows
/// now. None when there is nothing to say (a plain drop that worked).
pub fn added_notice(added: &Added, left_out: usize, turned_on: bool, shown: bool) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    // a drop is seen; an entry of a menu says what it did
    if added.added > 0 && (turned_on || !shown) {
        parts.push(tr!("fs-desktop-added", count = added.added));
    }
    if added.existing > 0 && added.added == 0 {
        parts.push(tr!("fs-desktop-add-existing").to_owned());
    }
    // a full room: what was left out when some went in, then why (as a
    // command added to that room says)
    if added.no_room > 0 {
        if added.added > 0 || added.existing > 0 {
            parts.push(tr!("fs-desktop-add-full", count = added.no_room));
        }
        parts.push(surface::NoRoom::Full.text());
    }
    if added.limit > 0 {
        parts.push(surface::NoRoom::Limit.text());
    }
    if left_out > 0 {
        parts.push(tr!("fs-desktop-left-out", count = left_out));
    }
    if added.added > 0 && turned_on {
        parts.push(tr!("fs-desktop-added-on").to_owned());
    }
    if added.added > 0 && !shown {
        parts.push(tr!("fs-desktop-added-hidden").to_owned());
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Whether the pointer at `pos` is a desktop's: the topmost layer under it
/// is one of our areas (the hook of `App::ui_takes_pointer`). egui does not
/// count an area of its background order as interface, so without this a
/// press on a shortcut would also press the world. A window over a desktop
/// is the topmost layer there, and still wins.
pub fn takes_pointer(ctx: &egui::Context, pos: egui::Pos2) -> bool {
    ctx.layer_id_at(pos)
        .is_some_and(|layer| layer.order == egui::Order::Background && (0..store::MAX_SLOTS).any(|slot| layer.id == surface::area_id(slot)))
}
