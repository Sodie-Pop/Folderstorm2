//! Folderstorm: the custom world viewport in the app (the rules are in
//! `crate::folderstorm::viewport`). Each frame the renderer is told the
//! world's part of the window; the camera's proportions, the HUDs, the name
//! tags and the mouselook cursor follow it, and picks outside it hit nothing
//! (`aurora_render::Renderer::set_world_rect`).

use super::*;
use crate::folderstorm::viewport::{self, Overrides};

impl App {
    /// When the world fills the window whatever the viewport settings.
    fn viewport_overrides(&self) -> Overrides {
        Overrides {
            mouselook: self.camera.mouselook(),
            full_frame: self.snapshots.capturing.is_some() || self.photo_full_frame() || !matches!(self.screen, Screen::World),
        }
    }

    /// The world's part of a window of `size` this frame, None for all of it.
    pub(super) fn world_viewport(&self, size: (u32, u32)) -> Option<[u32; 4]> {
        viewport::world_rect(size, &self.settings.folderstorm_viewport, self.viewport_overrides())
    }

    /// The world's part of a window of `size` for the HUDs (left, top,
    /// right, bottom in pixels), like the Legacy world view rectangle they
    /// are laid out in.
    pub(super) fn viewport_hud_area(&self, size: [u32; 2]) -> Option<[f32; 4]> {
        let [x, y, w, h] = self.world_viewport((size[0], size[1]))?;
        Some([x as f32, y as f32, (x + w) as f32, (y + h) as f32])
    }

    /// The centre of the world's part of a window of `size` (mouselook
    /// keeps the hidden cursor there), or of the window.
    pub(super) fn world_center(&self, size: (u32, u32)) -> (f32, f32) {
        match self.world_viewport(size) {
            Some([x, y, w, h]) => (x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0),
            None => (size.0 as f32 / 2.0, size.1 as f32 / 2.0),
        }
    }

    /// Width over height of the world's part of the window, when it has one.
    pub(super) fn world_aspect(&self) -> Option<f32> {
        let size = self.gfx.as_ref()?.window.inner_size();
        let [_, _, w, h] = self.world_viewport((size.width, size.height))?;
        Some(w as f32 / h.max(1) as f32)
    }
}

/// The camera's view-projection carried into window coordinates: points the
/// world's camera projects into its part of the window land there in a
/// projection over the whole window (name tags, floating text, markers).
pub(super) fn window_view_proj(vp: glam::Mat4, rect: Option<[u32; 4]>, window: (u32, u32)) -> glam::Mat4 {
    let Some([x, y, w, h]) = rect else { return vp };
    let (ww, wh) = (window.0.max(1) as f32, window.1.max(1) as f32);
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let scale = glam::Vec3::new(w / ww, h / wh, 1.0);
    let shift = glam::Vec3::new((2.0 * x + w) / ww - 1.0, 1.0 - (2.0 * y + h) / wh, 0.0);
    glam::Mat4::from_translation(shift) * glam::Mat4::from_scale(scale) * vp
}

#[cfg(test)]
mod tests {
    use super::window_view_proj;
    use glam::{Mat4, Vec4};

    fn ndc(m: Mat4, x: f32, y: f32) -> (f32, f32) {
        let c = m * Vec4::new(x, y, 0.5, 1.0);
        (c.x / c.w, c.y / c.w)
    }

    #[test]
    fn the_world_corners_land_on_its_part_of_the_window() {
        // window over three 1920x1080 monitors, the world on the middle one
        let m = window_view_proj(Mat4::IDENTITY, Some([1920, 0, 1920, 1080]), (5760, 1080));
        let to_px = |(x, y): (f32, f32)| ((x * 0.5 + 0.5) * 5760.0, (0.5 - y * 0.5) * 1080.0);
        let (l, t) = to_px(ndc(m, -1.0, 1.0));
        let (r, b) = to_px(ndc(m, 1.0, -1.0));
        assert!((l - 1920.0).abs() < 0.01 && (t - 0.0).abs() < 0.01, "{l} {t}");
        assert!((r - 3840.0).abs() < 0.01 && (b - 1080.0).abs() < 0.01, "{r} {b}");
        assert_eq!(window_view_proj(Mat4::IDENTITY, None, (5760, 1080)), Mat4::IDENTITY);
    }
}
