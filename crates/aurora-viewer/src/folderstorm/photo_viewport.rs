//! Folderstorm: photo mode inside the custom world viewport
//! (`crate::folderstorm::viewport`).
//!
//! With the viewport on, Aurora's photo mode (the viewfinder, the photo
//! window's full-frame view, the held photo and the reel's quick view) works
//! in the world's part of the window instead of the whole window: its frame,
//! guide and controls are laid out in that rectangle, its clicks and wheel
//! act there only, and its photos have that rectangle's proportions and show
//! what it shows ("Window" size included). Preferences can give photo mode
//! the whole window back (`ViewportSettings::photo_whole_window`), as before.
//!
//! The rectangle is the renderer's world rectangle of the frame
//! (`aurora_render::Renderer::world_rect`), so the photo always frames what
//! is drawn. In window pixels for the app's input and photo sizes, in egui
//! points for the overlay, which reads it through [`screen`].

/// The rectangle photo mode works in, in window pixels (x, y, width,
/// height): the world's part of the window, None for the whole window
/// (no viewport this frame, or photo mode given the whole window).
pub fn rect(world: Option<[u32; 4]>, whole_window: bool) -> Option<[u32; 4]> {
    world.filter(|&[_, _, w, h]| !whole_window && w > 0 && h > 0)
}

/// The size a photo takes as "the window" (its "Window" size and
/// proportions): photo mode's rectangle, or the window.
pub fn window_size(rect: Option<[u32; 4]>, window: (u32, u32)) -> (u32, u32) {
    rect.map_or(window, |[_, _, w, h]| (w, h))
}

/// Photo mode's rectangle in window pixels as floats, the window's when
/// it has none.
pub fn px_rect(rect: Option<[u32; 4]>, window: (u32, u32)) -> [f32; 4] {
    let [x, y, w, h] = rect.unwrap_or([0, 0, window.0, window.1]);
    [x as f32, y as f32, w.max(1) as f32, h.max(1) as f32]
}

/// A window pixel as a fraction of photo mode's rectangle (the AF point is
/// kept so: 0.5, 0.5 is the middle of the frame).
pub fn unit_at([x, y, w, h]: [f32; 4], (px, py): (f32, f32)) -> egui::Pos2 {
    egui::pos2((px - x) / w, (py - y) / h)
}

/// A fraction of photo mode's rectangle back in window pixels.
pub fn px_at([x, y, w, h]: [f32; 4], unit: egui::Pos2) -> (f32, f32) {
    (x + unit.x * w, y + unit.y * h)
}

/// Whether photo mode takes a click or a notch of the wheel at the
/// window pixel `cursor`: inside its rectangle (the whole window without
/// one). Outside, in a margin, nothing of photo mode acts.
pub fn takes_pointer(rect: Option<[u32; 4]>, (px, py): (f32, f32)) -> bool {
    match rect {
        None => true,
        Some([x, y, w, h]) => {
            let (x, y) = (x as f32, y as f32);
            px >= x && py >= y && px < x + w as f32 && py < y + h as f32
        }
    }
}

/// Photo mode's rectangle in egui points; None for the whole window.
pub fn points(rect: Option<[u32; 4]>, pixels_per_point: f32) -> Option<egui::Rect> {
    let [x, y, w, h] = rect?;
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    Some(egui::Rect::from_min_size(
        egui::pos2(x as f32 / ppp, y as f32 / ppp),
        egui::vec2(w as f32 / ppp, h as f32 / ppp),
    ))
}

fn area_id() -> egui::Id {
    egui::Id::unique("folderstorm_photo_area")
}

/// Remembers photo mode's rectangle (points) for this frame: the app sets
/// it each frame, before the interface is laid out.
pub fn set_area(ctx: &egui::Context, rect: Option<egui::Rect>) {
    ctx.data_mut(|d| d.insert_temp(area_id(), rect));
}

/// The rectangle photo mode's overlay is laid out in (points): the world
/// viewport's, or the whole window.
pub fn screen(ctx: &egui::Context) -> egui::Rect {
    let window = ctx.content_rect();
    ctx.data(|d| d.get_temp::<Option<egui::Rect>>(area_id()))
        .flatten()
        // (inside the window: a window just made smaller keeps no part
        // of the world outside it)
        .map(|r| r.intersect(window))
        .filter(|r| r.is_positive())
        .unwrap_or(window)
}

#[cfg(test)]
#[path = "photo_viewport_tests.rs"]
mod tests;
