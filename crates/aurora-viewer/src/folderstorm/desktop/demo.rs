//! `AURORA_DEMO_DESKTOP=<mode>`: a desktop made once at the start of a demo
//! run, in the demo's own settings (docs/TESTING.md).
//!
//! - `1`: shortcuts on the left monitor (six commands, one renamed with an
//!   icon of its own, one that only works from its toolbar button, one the
//!   catalogue no longer has; three folders of the demo inventory and one
//!   that is gone) and two on the third monitor.
//! - `open`: the same; at frame 200 the first folder shortcut is opened
//!   (its window beside it), at frame 300 the People shortcut.
//! - `drop`: the same; at frame 200 a folder is added where a drop from
//!   the Inventory would put it, at frame 300 it is moved to the third
//!   monitor.
//! - `rlva`: the same under `@showinv`: the folder shortcuts are locked and
//!   unnamed, and opening one at frame 200 is refused.
//! - `single`: the same shortcuts with the world filling the window:
//!   nothing shows.

use super::store::{Desktop, Target};
use crate::folderstorm::viewport::ViewportSettings;
use uuid::Uuid;

/// The frame at which `open` opens a folder, `drop` adds one, and `rlva`
/// tries to open one.
pub const OPEN_FRAME: u64 = 200;
/// The frame at which `open` opens [`OPENED`] and `drop` moves its folder.
pub const SECOND_FRAME: u64 = 300;
/// The command `open` opens.
pub const OPENED: &str = "people";

/// Folders of the demo's inventory (`crate::demo`, the login's skeleton:
/// Clothes, Landmarks, the Photo Album), then one that does not exist.
const FOLDERS: [u128; 4] = [11, 12, 31, 0xF01D_E400];
/// The folder `drop` adds: Objects.
const DROPPED: u128 = 14;
/// The cell of the first of them, on the left monitor; the others follow
/// down its column.
pub const FOLDERS_AT: (u16, u16) = (1, 0);
/// The cell `drop` drops its folder on.
pub const DROPPED_AT: (u16, u16) = (2, 1);
/// Where `drop` then moves it: a slot and a cell.
pub const MOVED_TO: (u8, (u16, u16)) = (2, (1, 0));

/// The n-th folder of the demo's shortcuts.
pub fn folder(n: usize) -> Uuid {
    crate::demo::u(FOLDERS[n % FOLDERS.len()])
}

/// The folder `drop` adds.
pub fn dropped() -> Uuid {
    crate::demo::u(DROPPED)
}

/// The demo's folder shortcuts (in the desktop of the demo's account).
pub fn seed_folders(desktop: &mut Desktop) {
    for n in 0..FOLDERS.len() {
        let target = Target::Folder { folder: folder(n) };
        let _ = desktop.add(target, 0, (FOLDERS_AT.0, FOLDERS_AT.1 + n as u16));
    }
}

/// The left monitor's commands, in the order they fill its first column.
const LEFT: &[&str] = &[
    "conversations",
    OPENED,
    "inventory",
    "worldmap",
    "snapshot",
    "home",
    // greyed: its button is its control
    "favorites",
    // greyed: gone from the catalogue
    "retired-command",
];

/// The third monitor's commands.
const RIGHT: &[&str] = &["preferences", "build"];

/// Makes the desktop of `mode` in `desktop`; turns the viewport on (the
/// world in the middle third of a window of `width` pixels) when it is
/// off, or off for `single`. False for a mode it does not know.
pub fn seed(mode: &str, desktop: &mut Desktop, viewport: &mut ViewportSettings, width: u32) -> bool {
    if !matches!(mode, "1" | "open" | "drop" | "rlva" | "single") {
        return false;
    }
    *desktop = Desktop {
        shortcuts_on: true,
        ..Default::default()
    };
    for (row, command) in LEFT.iter().enumerate() {
        let _ = desktop.add(Target::command(command), 0, (0, row as u16));
    }
    for (row, command) in RIGHT.iter().enumerate() {
        let _ = desktop.add(Target::command(command), 2, (0, row as u16));
    }
    // a name and an icon of its own
    if let Some(map) = desktop.shortcuts.iter_mut().find(|s| s.target == Target::command("worldmap")) {
        map.name = Some("Where am I?".to_owned());
        map.icon = Some("compass-filled".to_owned());
    }
    if mode == "single" {
        viewport.enabled = false;
    } else if !viewport.enabled {
        let third = width / 3;
        *viewport = ViewportSettings {
            enabled: true,
            left: third,
            right: third,
            ..viewport.clone()
        };
    }
    true
}
