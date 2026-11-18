//! Painting the background in the margins, and keeping its texture: the
//! picture decoded on a thread when it changes, uploaded once, drawn from
//! the next frame on (folderstorm/plans/workspaces.md, "1.8 in detail",
//! sections 3 and 4).

use super::import::{self, Decoded, LoadError, TEXTURE_SIDE};
use super::place;
use super::record::{Background, Fit, Kind};
use egui::epaint::ClippedShape;
use egui::{Color32, Rect, Vec2, pos2};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// What became of the picture in effect, for Preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not tried yet: no margin to paint, or no picture chosen.
    Unknown,
    Loading,
    Shown,
    Missing,
    Unreadable,
}

/// A decode under way: the thread leaves its result here.
type Pending = Arc<Mutex<Option<Result<Decoded, LoadError>>>>;

/// The picture's texture, and its decoding. Also the loader of each
/// picture a desktop shortcut wears (`desktop::style_ui`).
#[derive(Default)]
pub(crate) struct Loader {
    /// The copy decoded or being decoded, and the side it was asked at.
    file: Option<String>,
    side: u32,
    pending: Option<Pending>,
    /// The texture, the picture's own size, and the frame it was made in
    /// (drawn from the next one: egui hands it to the GPU with that frame).
    texture: Option<(egui::TextureHandle, [u32; 2], u64)>,
    failed: Option<LoadError>,
}

fn loader_id() -> egui::Id {
    egui::Id::unique("folderstorm_background_loader")
}

fn loader(ctx: &egui::Context) -> Arc<Mutex<Loader>> {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<Loader>>>(loader_id()).clone())
}

/// What happened to the picture `file` (a copy's name), as far as the
/// painting has tried it.
pub fn status(ctx: &egui::Context, file: &str) -> Status {
    let loader = loader(ctx);
    let Ok(loader) = loader.lock() else {
        return Status::Unknown;
    };
    if loader.file.as_deref() != Some(file) {
        return Status::Unknown;
    }
    match (&loader.texture, loader.failed, &loader.pending) {
        (_, Some(LoadError::Missing), _) => Status::Missing,
        (_, Some(LoadError::Unreadable), _) => Status::Unreadable,
        (Some(_), None, _) => Status::Shown,
        (None, None, Some(_)) => Status::Loading,
        (None, None, None) => Status::Unknown,
    }
}

/// Forgets the texture and what went wrong: the picture is read again
/// (chosen again after it went missing).
pub fn forget(ctx: &egui::Context) {
    if let Ok(mut loader) = loader(ctx).lock() {
        loader.forget();
    }
}

/// The side to decode a picture at for a window of `window` pixels: its
/// own pixels for Centre and Tile, about the window's size otherwise (no
/// more texels than drawn: egui's textures have no mipmaps).
fn side_for(window: [u32; 2], fit: Fit, gpu: usize) -> u32 {
    let gpu = u32::try_from(gpu).unwrap_or(u32::MAX).max(1);
    let want = match fit {
        Fit::Centre | Fit::Tile => TEXTURE_SIDE,
        Fit::Fill | Fit::Contain | Fit::Stretch => window[0].max(window[1]).div_ceil(1024).max(1) * 1024,
    };
    want.min(TEXTURE_SIDE).min(gpu)
}

impl Loader {
    /// The texture of `file` to draw this frame, with the picture's own
    /// size; starts or finishes its decoding.
    pub(crate) fn poll(&mut self, ctx: &egui::Context, folder: &Path, file: &str, side: u32) -> Option<(egui::TextureId, [u32; 2])> {
        let now = ctx.cumulative_frame_nr();
        let same = self.file.as_deref() == Some(file);
        // a larger side is worth decoding again only when the picture was
        // reduced (the window grew over more monitors)
        let reduced = self
            .texture
            .as_ref()
            .is_some_and(|(texture, own, _)| texture.size()[0] < own[0] as usize && texture.size()[1] < own[1] as usize);
        let larger = side > self.side && (reduced || self.pending.is_some());
        if !same || (larger && self.failed.is_none()) {
            if !same {
                self.texture = None;
            }
            self.file = Some(file.to_owned());
            self.side = side;
            self.failed = None;
            self.pending = Some(start(ctx, folder, file, side));
        }
        if let Some(pending) = &self.pending {
            let done = pending.lock().ok().and_then(|mut slot| slot.take());
            match done {
                Some(Ok(decoded)) => {
                    let texture = ctx.load_texture("folderstorm-background", decoded.image, egui::TextureOptions::LINEAR);
                    self.texture = Some((texture, decoded.size, now));
                    self.pending = None;
                    // drawn from the next frame, which may not come by itself
                    ctx.request_repaint();
                }
                Some(Err(error)) => {
                    log::info!("background picture: cannot be shown ({error:?}); its colour is shown");
                    self.texture = None;
                    self.failed = Some(error);
                    self.pending = None;
                }
                None => {}
            }
        }
        self.texture
            .as_ref()
            .filter(|(_, _, made)| *made < now)
            .map(|(texture, own, _)| (texture.id(), *own))
    }

    /// Nothing to draw: the texture goes (its GPU memory with it).
    fn forget(&mut self) {
        *self = Loader::default();
    }
}

/// Decodes `file` on a thread of its own; the frame is asked again when
/// it is done.
fn start(ctx: &egui::Context, folder: &Path, file: &str, side: u32) -> Pending {
    let pending: Pending = Arc::default();
    let slot = pending.clone();
    let (ctx, folder, file) = (ctx.clone(), folder.to_owned(), file.to_owned());
    let spawned = std::thread::Builder::new().name("folderstorm-background".into()).spawn(move || {
        let result = import::decode(&folder, &file, side);
        if let Ok(mut slot) = slot.lock() {
            *slot = Some(result);
        }
        ctx.request_repaint();
    });
    if spawned.is_err()
        && let Ok(mut slot) = pending.lock()
    {
        *slot = Some(Err(LoadError::Unreadable));
    }
    pending
}

/// The picture's tint: a grey of its brightness.
fn tint(brightness: u8) -> Color32 {
    let level = (f32::from(brightness.min(100)) * 2.55).round() as u8;
    Color32::from_gray(level)
}

/// The shapes of `background` in `margins` (points), the picture in
/// `areas`: the colour over each margin, then the picture clipped to it.
/// `picture`: its texture and its own size in pixels, when it can be drawn.
pub fn shapes(
    background: &Background,
    margins: &[Rect],
    areas: &[Rect],
    picture: Option<(egui::TextureId, [u32; 2])>,
    pixels_per_point: f32,
) -> Vec<ClippedShape> {
    let mut out = Vec::new();
    if !background.paints() {
        return out;
    }
    let [r, g, b, _] = background.colour;
    let colour = Color32::from_rgb(r, g, b);
    for margin in margins {
        out.push(ClippedShape::new(*margin, egui::Shape::rect_filled(*margin, 0.0, colour)));
    }
    let Some((texture, own)) = picture.filter(|_| background.kind == Kind::Picture) else {
        return out;
    };
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let size = Vec2::new(own[0] as f32, own[1] as f32) / ppp;
    let tint = tint(background.brightness);
    let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    for area in areas {
        for margin in margins {
            let clip = margin.intersect(*area);
            if !clip.is_positive() {
                continue;
            }
            let mut mesh = egui::Mesh::with_texture(texture);
            match background.fit {
                Fit::Tile => {
                    for tile in place::tiles(*area, size, clip) {
                        mesh.add_rect_with_uv(tile, whole, tint);
                    }
                }
                fit => mesh.add_rect_with_uv(place::placed(*area, size, fit), whole, tint),
            }
            out.push(ClippedShape::new(clip, egui::Shape::mesh(mesh)));
        }
    }
    out
}

/// Where the background goes among the frame's shapes ([`mark`]).
struct Here;

/// Marks where the background goes: called in the pass right after
/// `aurora_render::end_world_labels`, on the same layer. The renderer draws
/// what precedes that marker as the world's labels, cut to the world
/// viewport: shapes put before it never show in the margins. So the
/// background goes right after it, under everything else (the HUD
/// attachments' labels, the bars, the windows). Without the mark (a screen
/// with no world labels, so nothing is cut) it goes first. [`underlay_in`]
/// always takes the mark out again.
pub fn mark(ctx: &egui::Context) {
    ctx.layer_painter(egui::LayerId::background()).add(egui::PaintCallback {
        rect: ctx.content_rect(),
        callback: Arc::new(Here),
    });
}

fn is_mark(shape: &ClippedShape) -> bool {
    matches!(&shape.shape, egui::Shape::Callback(c) if c.callback.is::<Here>())
}

/// Where the background goes in `frame`: where [`mark`] was (taken out,
/// never left for the renderer), else at the start.
fn take_mark(frame: &mut Vec<ClippedShape>) -> usize {
    let Some(at) = frame.iter().position(is_mark) else {
        return 0;
    };
    frame.retain(|shape| !is_mark(shape));
    at
}

/// The background in effect painted under the interface this frame: its
/// shapes go where [`mark`] was in `frame` (the frame's shapes, after the
/// pass), else first. Nothing while the world fills the window.
pub fn underlay_in(ctx: &egui::Context, background: &Background, folder: &Path, frame: &mut Vec<ClippedShape>) {
    let at = take_mark(frame);
    let loader = loader(ctx);
    let Ok(mut loader) = loader.lock() else {
        return;
    };
    let Some(picture) = background.shown_picture() else {
        loader.forget();
        if background.paints() {
            paint(ctx, background, None, frame, at);
        }
        return;
    };
    let Some(world) = crate::folderstorm::viewport::world_area(ctx) else {
        // no margin: no work (the texture kept for when there is one)
        return;
    };
    let window = ctx.content_rect();
    if place::margins(window, world).is_empty() {
        return;
    }
    let ppp = ctx.pixels_per_point();
    let pixels = [(window.width() * ppp).round() as u32, (window.height() * ppp).round() as u32];
    let side = side_for(pixels, background.fit, ctx.input(|i| i.max_texture_side));
    let texture = loader.poll(ctx, folder, &picture.file, side);
    drop(loader);
    paint(ctx, background, texture, frame, at);
}

/// The shapes of `background` put at `at` in `frame`.
fn paint(
    ctx: &egui::Context,
    background: &Background,
    texture: Option<(egui::TextureId, [u32; 2])>,
    frame: &mut Vec<ClippedShape>,
    at: usize,
) {
    let Some(world) = crate::folderstorm::viewport::world_area(ctx) else {
        return;
    };
    let window = ctx.content_rect();
    let margins = place::margins(window, world);
    if margins.is_empty() {
        return;
    }
    let ppp = ctx.pixels_per_point();
    let areas = match crate::folderstorm::viewport::monitors(ctx) {
        Some(layout) => place::areas(window, layout.window, &layout.monitors, background.span, ppp),
        None => vec![window],
    };
    let mine = shapes(background, &margins, &areas, texture, ppp);
    let at = at.min(frame.len());
    frame.splice(at..at, mine);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_side_decoded_follows_the_window_and_the_fit() {
        assert_eq!(side_for([3840, 1080], Fit::Fill, 16384), 4096);
        assert_eq!(side_for([1920, 1080], Fit::Fill, 16384), 2048);
        assert_eq!(side_for([800, 600], Fit::Stretch, 16384), 1024);
        assert_eq!(side_for([800, 600], Fit::Tile, 16384), 4096);
        assert_eq!(side_for([800, 600], Fit::Centre, 2048), 2048, "the GPU's limit");
        assert_eq!(side_for([7680, 2160], Fit::Contain, 16384), 4096);
    }

    #[test]
    fn the_shapes_stay_in_the_margins() {
        let margins = [
            Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0)),
            Rect::from_min_size(pos2(1400.0, 0.0), vec2(600.0, 800.0)),
        ];
        let areas = [
            Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0)),
            Rect::from_min_size(pos2(1000.0, 0.0), vec2(1000.0, 800.0)),
        ];
        let background = Background {
            kind: Kind::Picture,
            brightness: 50,
            ..Default::default()
        };
        let texture = egui::TextureId::User(7);
        let shapes = shapes(&background, &margins, &areas, Some((texture, [400, 200])), 1.0);
        // the colour in each margin, then the picture in each margin's part
        // of each monitor
        assert_eq!(shapes.len(), 2 + 2);
        for shape in &shapes {
            assert!(margins.iter().any(|m| m.contains_rect(shape.clip_rect)), "{:?}", shape.clip_rect);
        }
        let egui::Shape::Mesh(mesh) = &shapes[2].shape else {
            panic!("the picture's mesh");
        };
        assert_eq!(mesh.texture_id, texture);
        assert_eq!(mesh.vertices[0].color, Color32::from_gray(128), "dimmed");
        // nothing chosen: nothing; a colour: the colour alone
        assert!(super::shapes(&Background::default(), &margins, &areas, Some((texture, [1, 1])), 1.0).is_empty());
        let colour = Background {
            kind: Kind::Colour,
            ..background.clone()
        };
        assert_eq!(super::shapes(&colour, &margins, &areas, Some((texture, [1, 1])), 1.0).len(), 2);
        // the picture not ready: the colour alone
        assert_eq!(super::shapes(&background, &margins, &areas, None, 1.0).len(), 2);
    }
}
