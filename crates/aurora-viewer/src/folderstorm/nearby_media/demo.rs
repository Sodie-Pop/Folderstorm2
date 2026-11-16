//! Folderstorm: `AURORA_DEMO_NEARBY_MEDIA` (`open` also pins the panel
//! open) adds two media sources to the demo for the panel's Show list: a
//! billboard past the east border of the demo parcel, and a small screen
//! worn on the chest by the demo's second avatar (Violet Wolf, who stands
//! on the parcel: in this parcel and on another avatar at once, as in the
//! Legacy Viewer). Media on other avatars is turned on for the demo, which
//! never saves its settings.

use aurora_net::NetEvent;
use glam::{Quat, Vec3};
use uuid::Uuid;

/// The billboard outside the parcel.
pub const OUTSIDE: Uuid = Uuid::from_u128(0xF05D_0000_0000_0000_0000_0000_0000_6D01);
/// The screen Violet Wolf wears.
pub const WORN: Uuid = Uuid::from_u128(0xF05D_0000_0000_0000_0000_0000_0000_6D02);
/// The demo's second avatar (`demo.rs`: local id 9000 + 1).
const WOLF_LOCAL_ID: u32 = 9001;
/// The chest attachment point.
const CHEST: u8 = 1;

/// The variable is set.
pub fn active() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("AURORA_DEMO_NEARBY_MEDIA").is_some())
}

/// The two objects (with the demo media screen's shape and media face),
/// each with its face carrying media and the page it shows.
pub fn sources() -> Vec<(NetEvent, Uuid, u8, &'static str)> {
    let (template, _, face) = crate::demo::media_screen();
    let NetEvent::ObjectUpdates { handle, objects } = template else {
        return Vec::new();
    };
    let Some(screen) = objects.into_iter().next() else {
        return Vec::new();
    };
    // 6 m past the parcel's east border (x 64..192), facing the plaza
    let mut outside = screen.clone();
    outside.full_id = OUTSIDE;
    outside.local_id = 0xF05D_6D01;
    let (x, y) = (198.0, 131.0);
    outside.position = Vec3::new(x, y, crate::demo::floor_at(x, y) + 1.9);
    // a hand-sized screen on the chest, its media face turned forward
    let mut worn = screen;
    worn.full_id = WORN;
    worn.local_id = 0xF05D_6D02;
    worn.parent_id = WOLF_LOCAL_ID;
    worn.state = ((CHEST & 0x0F) << 4) | ((CHEST & 0xF0) >> 4);
    worn.position = Vec3::new(0.16, 0.0, 0.05);
    worn.rotation = Quat::from_rotation_z(std::f32::consts::PI);
    worn.scale = Vec3::new(0.02, 0.32, 0.18);
    [
        (outside, aurora_i18n::tr!("fs-demo-nearby-media-outside-page")),
        (worn, aurora_i18n::tr!("fs-demo-nearby-media-worn-page")),
    ]
    .into_iter()
    .map(|(o, page)| {
        let id = o.full_id;
        (NetEvent::ObjectUpdates { handle, objects: vec![o] }, id, face, page)
    })
    .collect()
}
