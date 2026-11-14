//! Folderstorm: the custom world viewport in the app (the rules are in
//! `crate::folderstorm::viewport`). Each frame the renderer is told the
//! world's part of the window; the camera's proportions, the HUDs, the name
//! tags and the mouselook cursor follow it, and picks outside it hit nothing
//! (`aurora_render::Renderer::set_world_rect`).

use super::*;
use crate::folderstorm::photo_viewport;
use crate::folderstorm::viewport::{self, Overrides};

impl App {
    /// When the world fills the window whatever the viewport settings: a
    /// photo or the full-frame photo view only when photo mode has the
    /// whole window (by default it stays in the viewport,
    /// `crate::folderstorm::photo_viewport`).
    fn viewport_overrides(&self) -> Overrides {
        let photo =
            self.settings.folderstorm_viewport.photo_whole_window && (self.snapshots.capturing.is_some() || self.photo_full_frame());
        Overrides {
            mouselook: self.camera.mouselook(),
            full_frame: photo || !matches!(self.screen, Screen::World),
        }
    }

    /// The rectangle photo mode works in this frame, in window pixels: the
    /// world viewport the renderer draws, None for the whole window.
    pub(super) fn photo_rect(&self, renderer: &Renderer) -> Option<[u32; 4]> {
        photo_viewport::rect(renderer.world_rect(), self.settings.folderstorm_viewport.photo_whole_window)
    }

    /// The size photo mode's photos take as "the window": its "Window" size
    /// and proportions, and the live view their focus and detail match.
    pub(super) fn photo_window(&self, renderer: &Renderer) -> (u32, u32) {
        photo_viewport::window_size(self.photo_rect(renderer), renderer.size())
    }

    /// Photo mode's rectangle in window pixels (the AF point's frame).
    pub(super) fn photo_px(&self, renderer: &Renderer) -> [f32; 4] {
        photo_viewport::px_rect(self.photo_rect(renderer), renderer.size())
    }

    /// Whether the pointer is where photo mode takes clicks and the wheel
    /// (never in a margin of the viewport).
    pub(super) fn photo_takes_pointer(&self) -> bool {
        let rect = self.gfx.as_ref().and_then(|g| self.photo_rect(&g.renderer));
        photo_viewport::takes_pointer(rect, self.cursor_pos)
    }

    /// Tells photo mode's overlay its rectangle for this frame, before the
    /// interface is laid out.
    pub(super) fn publish_photo_area(&self, renderer: &Renderer) {
        let area = photo_viewport::points(self.photo_rect(renderer), self.egui_ctx.pixels_per_point());
        photo_viewport::set_area(&self.egui_ctx, area);
    }

    /// The world's part of a window of `size` this frame, None for all of
    /// it: under the session host's bar when the bar is over the world
    /// viewport (`app/folderstorm_characters/bar.rs`).
    pub(super) fn world_viewport(&self, size: (u32, u32)) -> Option<[u32; 4]> {
        let world = self.whole_world_viewport(size)?;
        Some(match self.folderstorm_bar_strip(size) {
            Some(crate::folderstorm_characters::bar::Strip::World(strip)) => crate::folderstorm_characters::bar::world_under(world, strip),
            _ => world,
        })
    }

    /// The world's part of a window of `size` this frame by the viewport's
    /// settings alone, before the session host's bar takes its strip.
    pub(super) fn whole_world_viewport(&self, size: (u32, u32)) -> Option<[u32; 4]> {
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

    /// `AURORA_DEMO_VIEWPORT=<mode>`: the viewport turned on once, at the
    /// start of a demo run (in the demo's own settings): `1` keeps the
    /// right two thirds of the window, `bars-window` the same with the bars
    /// over the whole window, `middle` the middle third (as the middle one
    /// of three monitors), `mouselook-full` drops it in mouselook,
    /// `inventory` keeps the right 40 % and opens Inventory in the left
    /// margin, half the window wide: wider than the world, and kept so.
    /// `right-half` and `left-half` keep the right or the left half (the
    /// world on one of two monitors, for photo mode in the viewport with
    /// `AURORA_DEMO_SNAPSHOT=viewfinder`), `photo-window` the right half
    /// with photo mode given the whole window.
    pub(super) fn viewport_demo(&mut self, size: (u32, u32)) {
        static APPLIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !self.demo || APPLIED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Ok(mode) = std::env::var("AURORA_DEMO_VIEWPORT") else { return };
        let third = size.0 / 3;
        let half = size.0 / 2;
        let s = &mut self.settings.folderstorm_viewport;
        *s = viewport::ViewportSettings {
            enabled: true,
            left: match mode.as_str() {
                "inventory" => size.0 / 5 * 3,
                "right-half" | "photo-window" => half,
                "left-half" => 0,
                _ => third,
            },
            right: match mode.as_str() {
                "middle" => third,
                "left-half" => size.0 - half,
                _ => 0,
            },
            in_mouselook: mode != "mouselook-full",
            bars_in_viewport: mode != "bars-window",
            photo_whole_window: mode == "photo-window",
            ..Default::default()
        };
        if mode == "inventory" {
            let ppp = self.egui_ctx.pixels_per_point();
            let (width, height) = (size.0 as f32 / ppp, size.1 as f32 / ppp);
            self.panels.inventory = true;
            crate::ui::widgets::place_floater(
                &self.egui_ctx,
                "inventory",
                egui::pos2(8.0, 48.0),
                egui::vec2(width * 0.5, (height * 0.7).max(300.0)),
            );
        }
    }

    /// Width over height of the world's part of the window, when it has one.
    pub(super) fn world_aspect(&self) -> Option<f32> {
        let size = self.gfx.as_ref()?.window.inner_size();
        let [_, _, w, h] = self.world_viewport((size.width, size.height))?;
        Some(w as f32 / h.max(1) as f32)
    }
}

/// Tells Preferences where the window and the monitors are, about once a
/// second (listing the monitors is a system call): the "Fit to a monitor"
/// buttons and the diagram. In a demo run, `AURORA_DEMO_MONITORS` pretends
/// the monitors (`crate::folderstorm::diagram::parse_demo_monitors`).
pub(super) fn publish_monitors(ctx: &egui::Context, window: &winit::window::Window, frame: u64) {
    use crate::folderstorm::diagram::{self, Desktop, Monitor};
    if !frame.is_multiple_of(60) {
        return;
    }
    let size = window.inner_size();
    let size = (size.width, size.height);
    let desktop = match diagram::demo_monitors() {
        Some(demo) => demo.desktop(size),
        None => Desktop {
            window: size,
            // (Wayland gives no window position: the diagram shows the
            // window alone, and there is no "Fit to a monitor")
            position: window.inner_position().ok().map(|at| (at.x, at.y)),
            monitors: window
                .available_monitors()
                .map(|m| Monitor {
                    rect: [m.position().x, m.position().y, m.size().width as i32, m.size().height as i32],
                    scale: m.scale_factor() as f32,
                })
                .collect(),
        },
    };
    if let Some(layout) = desktop.monitor_layout() {
        viewport::set_monitors(ctx, layout);
    }
    diagram::set_desktop(ctx, desktop);
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
