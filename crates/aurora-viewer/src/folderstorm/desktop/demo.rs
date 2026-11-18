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
//! - `edit`: the same; at frame 200 the name of the People shortcut is
//!   edited in place (as *Rename* or F2 does), at frame 300 the first
//!   folder shortcut gets a name and an icon of its own.
//! - `unused`: the same, and two shortcuts on a monitor the window does
//!   not have; at frame 200 Preferences opens on the desktop's group, with
//!   its note and *Bring them here*.
//! - `taskbar`: the same with the taskbar on the third monitor; at frame
//!   200 the Inventory and a folder's window are opened and collapsed (the
//!   taskbar holds them) and People is opened (not listed); at frame 300
//!   the Inventory is restored as a click on its button does.
//! - `overflow`: the taskbar with twenty collapsed windows (opened at
//!   frame 200): buttons that are their icon alone, and *More*.
//! - `items`: the same, and a third column of item shortcuts: a landmark,
//!   a notecard, an outfit, a shirt that is not worn (opening it adds
//!   it), a jacket that is (greyed) and one whose item is gone. At frame 200 the
//!   landmark is opened (Aurora's question before the teleport), at frame
//!   300 the notecard (its window).
//! - `outfit`: the same as `items`; at frame 200 the outfit shortcut is
//!   opened: the question of outfit safety, nothing worn before its answer.
//!
//! In `items`, `outfit` and `icons`, three shortcuts of the left monitor
//! wear a style of their own ([`seed_styles`]): *Conversations* a colour,
//! *Snapshot* a picture copied from a file (made by the demo), *Home* a
//! texture of the demo's Inventory (full permissions).
//! - `icons`: the same as `items`; at frame 200 the window *Shortcut icon*
//!   opens on *Home*: its colours, its picture, the textures offered.

use super::store::{Desktop, Target, Taskbar};
use super::style::{self, Picture};
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
/// The name and the icon `edit` gives the first folder shortcut.
pub const EDITED: (&str, &str) = ("Everyday outfits", "hanger-2-filled");
/// How many windows `overflow` collapses.
pub const OVERFLOW: usize = 20;
/// The slot of the taskbar of `taskbar` and `overflow`: the third monitor.
pub const TASKBAR_SLOT: u8 = 2;
/// The slot of the shortcuts of `unused`: a monitor no demo window has.
pub const UNUSED_SLOT: u8 = 7;
/// The commands of `unused`, kept on that slot.
const UNUSED: &[&str] = &["gestures", "appearance"];
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

/// The items of `items`, made up for the offline demo (they belong to no
/// one): a landmark in the Landmarks folder, a notecard in Objects.
const LANDMARK: u128 = 0xF01D_E500;
const NOTECARD: u128 = 0xF01D_E501;
/// An item that does not exist: its shortcut is greyed.
const ITEM_GONE: u128 = 0xF01D_E5FF;
/// The cell of the first item shortcut; the others follow down its column.
pub const ITEMS_AT: (u16, u16) = (2, 0);

/// The landmark of `items`.
pub fn landmark() -> Uuid {
    crate::demo::u(LANDMARK)
}

/// The notecard of `items`.
pub fn notecard() -> Uuid {
    crate::demo::u(NOTECARD)
}

/// The outfit of `items` and `outfit`: one of the demo's own
/// (`world::appearance::seed_demo`).
pub fn outfit() -> Uuid {
    Uuid::from_u128(703)
}

/// The clothing of `items` that is not worn: opening its shortcut adds it.
pub fn wearable() -> Uuid {
    Uuid::from_u128(722)
}

/// The demo's items, added to the demo's inventory, and their shortcuts
/// (in the desktop of the demo's account).
pub fn seed_items(desktop: &mut Desktop, inv: &mut crate::world::inventory::Inventory, agent: Uuid) {
    use aurora_net::inventory::InvItem;
    let item = |id: u128, parent: u128, name: &str, kind: i32| InvItem {
        id: crate::demo::u(id),
        parent: crate::demo::u(parent),
        name: name.to_owned(),
        asset_type: kind,
        inv_type: kind,
        // (the offline demo's own asset: it names nothing on a grid)
        asset_id: crate::demo::u(id + 0x1000),
        creator: agent,
        owner: agent,
        base_mask: 0x7fff_ffff,
        owner_mask: 0x7fff_ffff,
        ..Default::default()
    };
    inv.add_items(vec![
        item(LANDMARK, FOLDERS[1], "Aurora Welcome Area", 3),
        // (without an asset yet: it opens empty, as a new notecard does)
        InvItem {
            asset_id: Uuid::nil(),
            ..item(NOTECARD, DROPPED, "Shopping list", 7)
        },
    ]);
    // the demo's outfits (My Outfits and what they hold)
    crate::world::appearance::seed_demo(inv, agent);
    let targets = [
        Target::Item {
            item: crate::demo::u(LANDMARK),
        },
        Target::Item {
            item: crate::demo::u(NOTECARD),
        },
        Target::Outfit { outfit: outfit() },
        // (of the demo's wearables: a spare shirt, the jacket it wears)
        Target::Item { item: wearable() },
        Target::Item {
            item: Uuid::from_u128(715),
        },
        Target::Item {
            item: crate::demo::u(ITEM_GONE),
        },
    ];
    for (n, target) in targets.into_iter().enumerate() {
        let _ = desktop.add(target, 0, (ITEMS_AT.0, ITEMS_AT.1 + n as u16));
    }
}

/// The texture of `items` a shortcut wears: made up for the offline demo,
/// full permissions, in the Photo Album.
const TEXTURE: u128 = 0xF01D_E502;
/// The commands that wear a style in `items`: a colour, a picture copied
/// from a file, the demo's texture.
pub const STYLED: [&str; 3] = ["conversations", "snapshot", "home"];

/// The asset of the demo's texture (the offline demo's own: it names
/// nothing on a grid). The app puts [`texture_image`] there, as the fetch
/// of a texture would.
pub fn texture_asset() -> Uuid {
    crate::demo::u(TEXTURE + 0x1000)
}

/// The pixels of the demo's texture: blue and white diagonal stripes.
pub fn texture_image() -> egui::ColorImage {
    let side = 64;
    let pixels = (0..side * side)
        .map(|n| {
            let (x, y) = (n % side, n / side);
            if (x + y) / 8 % 2 == 0 {
                egui::Color32::from_rgb(0x3E, 0x63, 0xDD)
            } else {
                egui::Color32::from_rgb(0xF0, 0xF4, 0xFF)
            }
        })
        .collect();
    egui::ColorImage::new([side, side], pixels)
}

/// The picture of `items` copied as a file would be: a sun over the sea.
fn file_picture() -> Vec<u8> {
    let (w, h) = (96u32, 96u32);
    let picture = image::RgbaImage::from_fn(w, h, |x, y| {
        let (dx, dy) = (x as f32 - 48.0, y as f32 - 52.0);
        if dx * dx + dy * dy < 22.0 * 22.0 && y < 60 {
            image::Rgba([0xFF, 0xD2, 0x3F, 255])
        } else if y >= 60 {
            image::Rgba([0x12, 0x5E, 0x8A, 255])
        } else {
            let t = y as f32 / 60.0;
            image::Rgba([
                0xF7,
                (0x6B as f32 + 0x50 as f32 * (1.0 - t)) as u8,
                (0x15 as f32 + 0x90 as f32 * t) as u8,
                255,
            ])
        }
    });
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = picture.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// The styles of `items` ([`STYLED`]): the demo's texture added to its
/// inventory, the file picture copied into the account's folder of the
/// demo's own settings.
pub fn seed_styles(desktop: &mut Desktop, inv: &mut crate::world::inventory::Inventory, agent: Uuid) {
    use aurora_net::inventory::InvItem;
    inv.add_items(vec![InvItem {
        id: crate::demo::u(TEXTURE),
        parent: crate::demo::u(FOLDERS[2]),
        name: "Blue stripes".to_owned(),
        asset_type: style::AT_TEXTURE,
        inv_type: 0,
        asset_id: texture_asset(),
        creator: agent,
        owner: agent,
        base_mask: 0x7fff_ffff,
        owner_mask: 0x7fff_ffff,
        ..Default::default()
    }]);
    let id = |desktop: &Desktop, command: &str| {
        desktop
            .shortcuts
            .iter()
            .find(|s| s.target == Target::command(command))
            .map(|s| s.id)
    };
    if let Some(id) = id(desktop, STYLED[0]) {
        desktop.set_colour(id, Some(style::PRESETS[4]));
    }
    match style::store(&file_picture(), "sunset.png", &style::folder(agent)) {
        Ok(picture) => {
            if let Some(id) = id(desktop, STYLED[1]) {
                desktop.set_picture(id, Some(picture));
            }
        }
        Err(e) => log::warn!("AURORA_DEMO_DESKTOP: the demo's picture was not copied ({e:?})"),
    }
    if let Some(id) = id(desktop, STYLED[2]) {
        desktop.set_picture(
            id,
            Some(Picture::Texture {
                texture: crate::demo::u(TEXTURE),
            }),
        );
    }
}

/// `icons` at frame 200: the window *Shortcut icon* on the shortcut that
/// wears the demo's texture.
pub fn open_icon_window(ctx: &egui::Context, desktop: &Desktop) {
    if let Some(shortcut) = desktop.shortcuts.iter().find(|s| s.target == Target::command(STYLED[2])) {
        super::style_ui::open(ctx, shortcut.id);
    }
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
    if !matches!(
        mode,
        "1" | "open" | "drop" | "rlva" | "single" | "edit" | "unused" | "taskbar" | "overflow" | "items" | "outfit" | "icons"
    ) {
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
    if mode == "unused" {
        for (row, command) in UNUSED.iter().enumerate() {
            let _ = desktop.add(Target::command(command), UNUSED_SLOT, (0, row as u16));
        }
    }
    // a name and an icon of its own
    if let Some(map) = desktop.shortcuts.iter_mut().find(|s| s.target == Target::command("worldmap")) {
        map.name = Some("Where am I?".to_owned());
        map.icon = Some("compass-filled".to_owned());
    }
    if matches!(mode, "taskbar" | "overflow") {
        desktop.taskbar = Taskbar {
            on: true,
            slot: Some(TASKBAR_SLOT),
            all_windows: false,
        };
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
