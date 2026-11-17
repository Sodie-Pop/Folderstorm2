//! `AURORA_DEMO_DESKTOP=<mode>`: a desktop made once at the start of a demo
//! run, in the demo's own settings (docs/TESTING.md).
//!
//! - `1`: shortcuts on the left monitor (six commands, one renamed with an
//!   icon of its own, one that only works from its toolbar button, one the
//!   catalogue no longer has) and two on the third monitor.
//! - `open`: the same; at frame 200 the People shortcut is opened.
//! - `single`: the same shortcuts with the world filling the window:
//!   nothing shows.

use super::store::{Desktop, Target};
use crate::folderstorm::viewport::ViewportSettings;

/// The frame at which `open` opens [`OPENED`].
pub const OPEN_FRAME: u64 = 200;
/// The command `open` opens.
pub const OPENED: &str = "people";

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
    if !matches!(mode, "1" | "open" | "single") {
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
