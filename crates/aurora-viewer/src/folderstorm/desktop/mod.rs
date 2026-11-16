//! Folderstorm: the margins' desktop (FEATURES.md row 1.10;
//! folderstorm/plans/margin-desktop.md).
//!
//! Asked by the human; Folderstorm (C++) had nothing of it. On each monitor
//! under the viewer window that the world viewport leaves free, a desktop:
//! shortcuts on a grid, over the margins' background (row 1.8) and under
//! every window. A shortcut to a toolbar command runs what its toolbar
//! button runs (`ui::bars::run_command`, through the app's glue,
//! `app/folderstorm_desktop.rs`), behind the same gates. Shortcuts to
//! Inventory folders, renaming and the taskbar are the plan's next
//! checkpoints; the record ([`store`]) already has their fields.
//!
//! - [`store`]: what `settings.json` keeps (field `folderstorm_desktop`).
//! - [`place`]: slots, rooms and cells (pure maths).
//! - [`targets`]: what a shortcut shows, and the gates of a command.
//! - [`surface`]: the areas, the shortcuts' faces, clicks, drags and menus.
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
pub mod place;
pub mod store;
pub mod surface;
pub mod targets;

#[cfg(test)]
mod tests;

pub use store::Desktop;
pub use targets::Facts;

use crate::folderstorm::viewport;
use crate::theme::Palette;
use crate::ui::icons::Icons;
use egui::vec2;
use uuid::Uuid;

/// What the app gives the desktops each frame.
pub struct Env<'a> {
    pub p: &'a Palette,
    pub icons: &'a Icons,
    /// What greys a toolbar button now.
    pub facts: Facts,
    /// The account logged in: it sees its own folder shortcuts only.
    pub account: Uuid,
    /// The interface is hidden (its shortcut, immersive mode, mouselook
    /// without the interface): no desktop.
    pub hidden: bool,
}

/// What the desktops ask the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Run this command of the toolbar catalogue, as its button does.
    Run(String),
    /// The desktop changed: save the settings once the pointer is up.
    Changed,
    /// Open Preferences on the desktop's group.
    Settings,
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

/// The rooms of this frame: one for each slot the world and the bars leave
/// a cell in. None while the world fills the window or no world is shown
/// (login, a photo taking the whole window).
pub fn rooms(ctx: &egui::Context, size: store::Size) -> Vec<surface::Room> {
    let Some(world) = viewport::world_area(ctx) else {
        return Vec::new();
    };
    let [w, h] = size.cell();
    let cell = vec2(w, h);
    let bars = bars(ctx);
    let layout = viewport::monitors(ctx);
    place::slots(ctx.content_rect(), layout.as_ref(), ctx.pixels_per_point())
        .into_iter()
        .enumerate()
        .filter_map(|(slot, rect)| {
            let rect = place::room(rect, world, bars, cell)?;
            let grid = place::Grid::new(rect, cell)?;
            Some(surface::Room {
                slot: slot as u8,
                rect,
                grid,
            })
        })
        .collect()
}

/// The desktops of this frame, drawn; what they ask is returned. To be
/// called in the interface's pass once the bars are laid out.
pub fn show(ctx: &egui::Context, desktop: &mut Desktop, env: &Env) -> Vec<Request> {
    if !desktop.shortcuts_on || env.hidden {
        return Vec::new();
    }
    let rooms = rooms(ctx, desktop.size);
    if rooms.is_empty() {
        return Vec::new();
    }
    surface::show(ctx, desktop, env, &rooms)
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
