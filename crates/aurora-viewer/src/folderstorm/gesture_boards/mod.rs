//! Gesture boards (FEATURES 3.1, folderstorm/plans/gesture-boards.md): a
//! window of tiles that play gestures and sounds of the user's inventory,
//! in several saved boards. Folderstorm (C++): "World → Gesture board…"
//! (doc/help.md "Gesture boards", doc/plans/gesture-board.md).
//!
//! - [`store`]: the boards as the settings keep them, per account. Pure.
//! - [`layout`]: tile sizes, pages and search. Pure.
//! - [`folders`]: whole folders dropped on a board, walked with their
//!   sub-folders for their gestures and sounds. Pure.
//! - `window`, `dialogs`: the window (egui), which leaves what is clicked
//!   to play as [`Request`]s.
//! - `app/folderstorm_gesture_boards.rs`: plays them through Aurora's own
//!   paths (the Gestures window's Play, the inventory's sound trigger) and
//!   saves the settings.
//!
//! The window's open state lives in egui's memory, as the layouts'
//! switcher's: the menu entry and the toolbar button flip it.

mod dialogs;
pub mod folders;
pub mod layout;
pub mod store;
#[cfg(test)]
mod tests;
mod window;

use crate::theme::Palette;
use crate::world::inventory::{Inventory, actions as rules};
use uuid::Uuid;

pub use window::{Facts, Request, demo_drop, demo_search, show};

/// The window's id.
pub const WINDOW: &str = "fs_gesture_board";
/// The toolbar command (`ui/toolbar/catalogue.rs`).
pub const COMMAND: &str = "gesture_board";
/// The board's icon, in the menu, on the toolbar and on its title.
pub const ICON: &str = "layout-grid-filled";
/// The mark of a sound's tile.
pub const SOUND_ICON: &str = "volume";

fn open_id() -> egui::Id {
    egui::Id::unique("fs_gesture_board_open")
}

/// Whether the window is open.
pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false)
}

fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// The menu entry and the toolbar button: a window shown closes, a
/// minimized one expands, a closed one opens (`widgets::toggle_floater`).
pub fn toggle(ctx: &egui::Context) {
    let mut open = is_open(ctx);
    crate::ui::widgets::toggle_floater(ctx, WINDOW, &mut open);
    set_open(ctx, open);
}

/// Opens the window (expanded).
pub fn open(ctx: &egui::Context) {
    let mut open = is_open(ctx);
    crate::ui::widgets::open_floater(ctx, WINDOW, &mut open);
    set_open(ctx, open);
}

/// The World menu's entry (the hook of `ui/bars.rs`).
pub fn menu_item(ui: &mut egui::Ui, p: &Palette) {
    if crate::ui::menu::item(ui, p, ICON, aurora_i18n::tr!("fs-gesture-board-menu")) {
        toggle(ui.ctx());
    }
}

/// A click on the toolbar button of `command` (the arm of
/// `bars::run_command`).
pub fn run(ctx: &egui::Context, command: &str) {
    if command == COMMAND {
        toggle(ctx);
    }
}

/// How the button of `command` looks: ours is lit while the window is
/// open. Another command's look is given back as it came.
pub fn look(ctx: &egui::Context, command: &str, active: bool, usable: bool, tip: String) -> (bool, bool, String) {
    if command == COMMAND {
        (is_open(ctx), usable, tip)
    } else {
        (active, usable, tip)
    }
}

/// What a tile plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gesture,
    Sound,
}

/// A tile's item as the inventory has it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A gesture or a sound of the user's inventory: `original` is the item
    /// itself, or the one its link points to; `name` the tile's item's name.
    Ready { original: Uuid, kind: Kind, name: String },
    /// Not in the inventory (deleted, or not loaded yet), or a broken link.
    Missing,
    /// In the Trash.
    Trash,
    /// In the Library: not the user's.
    Library,
    /// Neither a gesture nor a sound.
    Other,
}

impl Item {
    /// Why the tile cannot play, as its tooltip says.
    pub fn why(&self) -> Option<&'static str> {
        match self {
            Item::Ready { .. } => None,
            Item::Missing => Some(aurora_i18n::tr!("fs-gesture-board-missing")),
            Item::Trash => Some(aurora_i18n::tr!("fs-gesture-board-trash")),
            Item::Library => Some(aurora_i18n::tr!("fs-gesture-board-library")),
            Item::Other => Some(aurora_i18n::tr!("fs-gesture-board-other")),
        }
    }
}

/// What the tile of `item` plays (decision 3 of the plan): a gesture
/// (asset type 21) or a sound (1) of the user's own inventory, a link
/// followed (`actions::original`, as LLInvFVBridgeAction::doAction does);
/// never one of the Library or of the Trash.
pub fn resolve(inv: &Inventory, item: Uuid) -> Item {
    let Some(it) = inv.items.get(&item) else {
        return Item::Missing;
    };
    let Some(original) = rules::original(inv, item).and_then(|id| inv.items.get(&id)) else {
        return Item::Missing;
    };
    let kind = match original.asset_type {
        21 => Kind::Gesture,
        1 => Kind::Sound,
        _ => return Item::Other,
    };
    if rules::library(inv, item) || rules::library(inv, original.id) {
        return Item::Library;
    }
    if rules::in_type(inv, item, 14) || rules::in_type(inv, original.id, 14) {
        return Item::Trash;
    }
    Item::Ready {
        original: original.id,
        kind,
        name: it.name.clone(),
    }
}
