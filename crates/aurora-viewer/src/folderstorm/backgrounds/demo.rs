//! `AURORA_DEMO_BACKGROUND=<mode>`: a background chosen once at the start
//! of a demo run, with a picture made here (no picture file in the
//! repository), copied the way a chosen one is (docs/TESTING.md).

use super::import;
use super::record::{Background, Fit, Kind, Picture};
use crate::folderstorm::viewport::ViewportSettings;
use crate::folderstorm::workspaces::Store;
use crate::folderstorm::workspaces::store::Layout;
use std::path::Path;

/// The demo's picture, 480 × 270: a dusk gradient, a sun, hills, an
/// arrow pointing up at the top-left (to see a picture upside down or
/// mirrored) and a light frame (to see what a fit cuts).
pub fn picture() -> image::RgbaImage {
    let (w, h) = (480u32, 270u32);
    image::RgbaImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        // the frame
        if x < 4 || y < 4 || x >= w - 4 || y >= h - 4 {
            return image::Rgba([235, 225, 200, 255]);
        }
        // the arrow: a triangle over a stem, near the top-left corner
        let (ax, ay) = (x as f32 - 40.0, y as f32 - 20.0);
        if (0.0..=30.0).contains(&ay) && ax.abs() <= ay * 0.6 || (30.0..=60.0).contains(&ay) && ax.abs() <= 5.0 {
            return image::Rgba([255, 255, 255, 255]);
        }
        // the hills
        let hill = 0.72 + 0.08 * (fx * 9.0).sin() + 0.05 * (fx * 23.0 + 1.0).sin();
        if fy > hill {
            let shade = (40.0 + 30.0 * (1.0 - fy)) as u8;
            return image::Rgba([shade / 2, shade, shade / 2 + 10, 255]);
        }
        // the sun
        let (sx, sy) = (fx - 0.68, (fy - 0.55) * h as f32 / w as f32);
        if sx * sx + sy * sy < 0.006 {
            return image::Rgba([255, 214, 120, 255]);
        }
        // the sky
        let r = (40.0 + 200.0 * fy) as u8;
        let g = (50.0 + 90.0 * fy) as u8;
        let b = (120.0 - 40.0 * fy) as u8;
        image::Rgba([r, g, b, 255])
    })
}

/// [`picture`] as a PNG file's bytes.
pub fn png() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    if picture().write_to(&mut out, image::ImageFormat::Png).is_err() {
        return Vec::new();
    }
    out.into_inner()
}

/// The name of the copy `missing` points at: never written.
pub const MISSING: &str = "0000000000000000.png";

/// Chooses the background of `mode` in `store` (the default, and for
/// `layout` a layout "Wall" with its own, made active), the picture copied
/// into `folder`; turns the viewport on (the world in the middle third of
/// a window of `width` pixels) when it is off. False for a mode it does
/// not know.
pub fn seed(mode: &str, store: &mut Store, viewport: &mut ViewportSettings, width: u32, folder: &Path) -> bool {
    let fit = match mode {
        "1" | "fill" | "span" | "dim" | "layout" => Fit::Fill,
        "fit" => Fit::Fit,
        "stretch" => Fit::Stretch,
        "centre" | "center" => Fit::Centre,
        "tile" => Fit::Tile,
        "colour" | "color" | "missing" => Fit::Fill,
        _ => return false,
    };
    let picture = match mode {
        "missing" => Some(Picture {
            file: MISSING.to_owned(),
            name: "gone.png".to_owned(),
        }),
        _ => match import::store(&png(), "demo-dusk.png", folder) {
            Ok(picture) => Some(picture),
            Err(error) => {
                log::warn!("demo background: the picture cannot be copied ({error:?})");
                None
            }
        },
    };
    let kind = match mode {
        "colour" | "color" => Kind::Colour,
        _ => Kind::Picture,
    };
    store.background = Background {
        kind,
        colour: match mode {
            "missing" => [72, 24, 32, 255],
            _ => [24, 30, 52, 255],
        },
        picture,
        fit,
        span: mode == "span",
        brightness: if mode == "dim" { 40 } else { 100 },
    };
    if mode == "layout" {
        let mut wall = Layout::new("Wall", None, None, Vec::new());
        wall.background = Some(Background {
            fit: Fit::Tile,
            colour: [10, 10, 10, 255],
            ..store.background.clone()
        });
        if store.find("Wall").is_none() && store.add(wall).is_ok() {
            store.set_active(Some("Wall".to_owned()));
        }
    }
    if !viewport.enabled {
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
