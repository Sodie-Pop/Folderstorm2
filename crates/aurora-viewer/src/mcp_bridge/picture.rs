//! Folderstorm: the local assistant's pictures (folderstorm/plans/mcp.md,
//! "Phase 3, part 1"): what size the photo job draws for a `camera_snapshot`,
//! which part of it is kept, and the picture encoded within the byte cap.
//! Pure functions, so the tests check the sizes without a GPU; the photo
//! itself is Aurora's (`app/snapshot.rs`, `Purpose::Assistant`).

use folderstorm_mcp_bridge::protocol::SNAPSHOT_EDGE_MAX;
use std::path::{Path, PathBuf};

/// The longest side the photo job draws for a picture: a small crop of a
/// large picture is scaled up past it rather than drawn larger.
pub const RENDER_MAX: u32 = 4096;
/// The shortest side drawn or sent.
const SIDE_MIN: u32 = 16;

/// What the picture's proportions follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The whole window.
    Window,
    /// The world's part of the window (the custom world viewport), the
    /// whole window without one.
    World,
}

/// The part of the source kept, in fractions of its width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Crop {
    pub const WHOLE: Crop = Crop {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
}

impl Format {
    pub fn mime(self) -> &'static str {
        match self {
            Format::Jpeg => "image/jpeg",
            Format::Png => "image/png",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "jpeg",
            Format::Png => "png",
        }
    }
}

/// A `camera_snapshot` as asked, its arguments checked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Request {
    pub source: Source,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub max_edge: u32,
    pub crop: Crop,
    pub interface: bool,
    pub huds: bool,
    pub format: Format,
    pub save: bool,
}

/// What the photo job draws: the whole source at `size`, with or without
/// the interface and the HUDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Render {
    pub size: (u32, u32),
    pub interface: bool,
    pub huds: bool,
}

/// A picture planned: the photo drawn, the part of it kept (x, y, width,
/// height in its pixels) and the size sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub render: Render,
    pub cut: [u32; 4],
    pub out: (u32, u32),
}

/// Plan a picture for a window of `window` pixels whose world viewport is
/// `world` (None: the whole window).
///
/// Both sides asked: that size, the cropped source centre-cropped to its
/// proportions (Folderstorm (C++)'s `camera_snapshot`). One side: the other
/// follows the cropped source. Neither: the cropped source at its size in
/// the window, its long edge brought down to `max_edge`. The photo is drawn
/// so that the part kept has the pixels sent (`RENDER_MAX` at most); with
/// the interface, at the window's own size, as it is on screen.
pub fn plan(req: &Request, window: (u32, u32), world: Option<(u32, u32)>) -> Plan {
    let window = (window.0.max(1), window.1.max(1));
    let source = match req.source {
        Source::Window => window,
        Source::World => world.map_or(window, |(w, h)| (w.max(1), h.max(1))),
    };
    let (sw, sh) = (f64::from(source.0), f64::from(source.1));
    let crop = req.crop;
    let cw = (f64::from(crop.width) * sw).max(1.0);
    let ch = (f64::from(crop.height) * sh).max(1.0);
    let cx = f64::from(crop.x) * sw;
    let cy = f64::from(crop.y) * sh;
    let side = |v: f64| (v.round() as u32).clamp(1, SNAPSHOT_EDGE_MAX);
    let out = match (req.width, req.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, side(f64::from(w) * ch / cw)),
        (None, Some(h)) => (side(f64::from(h) * cw / ch), h),
        (None, None) => {
            let scale = (f64::from(req.max_edge) / cw.max(ch)).min(1.0);
            (side(cw * scale), side(ch * scale))
        }
    };
    // the part with the picture's proportions, in the middle of the crop
    let ratio = f64::from(out.0) / f64::from(out.1.max(1));
    let (rw, rh) = if cw / ch > ratio { (ch * ratio, ch) } else { (cw, cw / ratio) };
    let (rx, ry) = (cx + (cw - rw) / 2.0, cy + (ch - rh) / 2.0);
    let mut k = if req.interface {
        1.0
    } else {
        (f64::from(out.0) / rw).max(f64::from(out.1) / rh)
    };
    if sw.max(sh) * k > f64::from(RENDER_MAX) {
        k = f64::from(RENDER_MAX) / sw.max(sh);
    }
    if sw.min(sh) * k < f64::from(SIDE_MIN) {
        k = f64::from(SIDE_MIN) / sw.min(sh);
    }
    let render = ((sw * k).round().max(1.0) as u32, (sh * k).round().max(1.0) as u32);
    let x = ((rx * k).floor() as u32).min(render.0 - 1);
    let y = ((ry * k).floor() as u32).min(render.1 - 1);
    let w = ((rw * k).round() as u32).clamp(1, render.0 - x);
    let h = ((rh * k).round() as u32).clamp(1, render.1 - y);
    Plan {
        render: Render {
            size: render,
            interface: req.interface,
            huds: req.huds,
        },
        cut: [x, y, w, h],
        out,
    }
}

/// A picture ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub size: (u32, u32),
    pub format: Format,
    /// The JPEG quality it took to fit.
    pub quality: Option<u8>,
}

/// The photo's pixels (RGBA, `size`) cut and scaled as `plan` says, then
/// encoded within `cap` bytes: a JPEG steps its quality down, then both
/// formats shrink by a quarter until the picture fits.
pub fn finish(pixels: Vec<u8>, size: (u32, u32), plan: &Plan, format: Format, cap: usize) -> Result<Encoded, String> {
    let image = image::RgbaImage::from_raw(size.0, size.1, pixels).ok_or("the photo came back without its pixels")?;
    // the photo may come back at another size than asked: the cut follows
    let (fx, fy) = (
        f64::from(size.0) / f64::from(plan.render.size.0.max(1)),
        f64::from(size.1) / f64::from(plan.render.size.1.max(1)),
    );
    let [x, y, w, h] = plan.cut;
    let x = ((f64::from(x) * fx) as u32).min(size.0.saturating_sub(1));
    let y = ((f64::from(y) * fy) as u32).min(size.1.saturating_sub(1));
    let w = ((f64::from(w) * fx).round() as u32).clamp(1, size.0 - x);
    let h = ((f64::from(h) * fy).round() as u32).clamp(1, size.1 - y);
    let cut = image::imageops::crop_imm(&image, x, y, w, h).to_image();
    let rgb = image::DynamicImage::ImageRgba8(cut).into_rgb8();
    let rgb = if rgb.dimensions() == plan.out {
        rgb
    } else {
        image::imageops::resize(&rgb, plan.out.0, plan.out.1, image::imageops::FilterType::Lanczos3)
    };
    encode_capped(rgb, format, cap)
}

fn encode(image: &image::RgbImage, format: Format, quality: u8) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let result = match format {
        Format::Jpeg => image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality).encode_image(image),
        Format::Png => image.write_with_encoder(image::codecs::png::PngEncoder::new(&mut bytes)),
    };
    result.map_err(|e| format!("the picture was not encoded: {e}"))?;
    Ok(bytes)
}

/// Encode within `cap` bytes (see `finish`).
pub fn encode_capped(mut image: image::RgbImage, format: Format, cap: usize) -> Result<Encoded, String> {
    let qualities: &[u8] = match format {
        Format::Jpeg => &[90, 80, 70, 60],
        Format::Png => &[0],
    };
    loop {
        for &quality in qualities {
            let bytes = encode(&image, format, quality)?;
            if bytes.len() <= cap {
                return Ok(Encoded {
                    bytes,
                    size: image.dimensions(),
                    format,
                    quality: (format == Format::Jpeg).then_some(quality),
                });
            }
        }
        let (w, h) = image.dimensions();
        let (w, h) = (w * 3 / 4, h * 3 / 4);
        if w.min(h) < SIDE_MIN {
            return Err("the picture does not fit in the size allowed".into());
        }
        image = image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle);
    }
}

/// Where a saved picture goes: `assistant-<date>-<time>.<ext>` in the
/// Photo window's folder, a number added when the name is taken.
pub fn save_path(folder: &Path, stamp: &str, format: Format) -> PathBuf {
    let ext = format.extension();
    let first = folder.join(format!("assistant-{stamp}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..1000)
        .map(|n| folder.join(format!("assistant-{stamp}-{n}.{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Write a picture (on a worker).
pub fn save(folder: &Path, stamp: &str, encoded: &Encoded) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("the photo folder could not be made: {e}"))?;
    let path = save_path(folder, stamp, encoded.format);
    crate::atomic_file::write(&path, &encoded.bytes).map_err(|e| format!("the picture was not saved: {e}"))?;
    Ok(path)
}
