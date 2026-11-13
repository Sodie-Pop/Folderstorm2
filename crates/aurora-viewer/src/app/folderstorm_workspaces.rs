//! Folderstorm: layouts in the app (the rules are in
//! `crate::folderstorm::workspaces` and `crate::folderstorm::viewport::bars`;
//! folderstorm/plans/workspaces.md, checkpoint 3).

use super::*;
use crate::folderstorm::viewport;
use crate::folderstorm::workspaces::apply;

impl App {
    /// Each frame, before the interface's pass: the windows of a layout
    /// applied in the last frame take their places, and the active layout
    /// is marked modified when one of its windows was moved since
    /// (`apply::frame`).
    pub(super) fn folderstorm_workspaces_frame(&mut self) {
        if !matches!(self.screen, Screen::World) {
            return;
        }
        self.layouts_demo();
        if apply::frame(&self.egui_ctx, &mut apply::Live::of(&mut self.settings)) {
            self.settings.save();
        }
    }

    /// `AURORA_DEMO_LAYOUTS=<mode>`: two layouts seeded once in the demo's
    /// own settings, with Inventory, the world map and People open and
    /// Preferences on the Layouts tab. "Wide": the world on the right half
    /// of the window, the bars around it, the windows in the left margin.
    /// "Single": no viewport, the windows over the world. `1` only seeds
    /// them; `apply-wide` applies "Wide" at frame 180; `apply-single`
    /// applies "Wide" then, at frame 360, "Single" (windows made small in
    /// the margin get their sizes back).
    fn layouts_demo(&mut self) {
        use crate::folderstorm::workspaces::store::{Layout, Placement, Placements, Store};
        use crate::folderstorm::workspaces::{capture, windows};
        static SEEDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !self.demo {
            return;
        }
        let Ok(mode) = std::env::var("AURORA_DEMO_LAYOUTS") else { return };
        let ctx = self.egui_ctx.clone();
        let Some(room) = capture::room(&ctx) else { return };
        if !SEEDED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let (width, height) = (room.frame.width, room.frame.height);
            let place = |x: f32, y: f32, w: f32, h: f32| Placement {
                x: x.round(),
                y: y.round(),
                width: w.round(),
                height: h.round(),
                minimized: false,
            };
            let layout = |name: &str, world: viewport::ViewportSettings, places: [(&str, Placement); 3]| {
                let windows = Placements {
                    frame: room.frame,
                    windows: places.into_iter().map(|(id, place)| (id.to_owned(), place)).collect(),
                };
                Layout::new(name, Some(world), Some(windows), capture::monitors(&ctx))
            };
            let half = (width * room.frame.pixels_per_point / 2.0).round() as u32;
            let wide = layout(
                "Wide",
                viewport::ViewportSettings {
                    enabled: true,
                    left: half,
                    bars_around: true,
                    ..Default::default()
                },
                [
                    ("inventory", place(8.0, 48.0, width * 0.2, height * 0.8)),
                    ("worldmap", place(16.0 + width * 0.2, 48.0, width * 0.26, height * 0.55)),
                    (
                        crate::ui::people::WINDOW,
                        place(16.0 + width * 0.2, 56.0 + height * 0.55, width * 0.2, height * 0.3),
                    ),
                ],
            );
            let single = layout(
                "Single",
                Default::default(),
                [
                    ("inventory", place(width - 400.0, 60.0, 380.0, height * 0.6)),
                    ("worldmap", place(width * 0.3, height * 0.2, 560.0, 440.0)),
                    (crate::ui::people::WINDOW, place(20.0, 60.0, 300.0, height * 0.5)),
                ],
            );
            debug_assert!(
                wide.windows
                    .iter()
                    .flat_map(|w| w.windows.keys())
                    .all(|id| windows::find(id).is_some())
            );
            let store = &mut self.settings.folderstorm_workspaces;
            *store = Store::default();
            for seeded in [wide, single] {
                if let Err(error) = store.add(seeded) {
                    log::warn!("AURORA_DEMO_LAYOUTS: layout not seeded: {error:?}");
                }
            }
            store.set_active(None);
            self.panels.inventory = true;
            self.panels.world_map = true;
            self.panels.people = true;
            self.panels.settings = true;
            self.options_ui.open(ui::options::Page::Layout, 3);
        }
        let apply_at = |index: usize, frame: u64, app: &mut Self| {
            if app.frame_count == frame
                && let Err(error) = apply::apply(&ctx, &mut apply::Live::of(&mut app.settings), index)
            {
                log::warn!("AURORA_DEMO_LAYOUTS: layout not applied: {error:?}");
            }
        };
        match mode.as_str() {
            "apply-wide" => apply_at(0, 180, self),
            "apply-single" => {
                apply_at(0, 180, self);
                apply_at(1, 360, self);
            }
            _ => {}
        }
    }

    /// The bars' area this frame, in points: inside the custom world
    /// viewport `world` (the renderer's, in window pixels, already under
    /// the session host's bar), or around it, each side's bars in the
    /// margin when it has room (`viewport::bars`). None leaves the bars on
    /// the whole window of `size` pixels.
    pub(super) fn folderstorm_bars_rect(&self, ctx: &egui::Context, world: Option<[u32; 4]>, size: (u32, u32)) -> Option<egui::Rect> {
        let s = &self.settings.folderstorm_viewport;
        let inside = viewport::bars_rect(s, world, ctx.pixels_per_point())?;
        if !s.bars_around {
            return Some(inside);
        }
        let mut bars = viewport::bars::thickness(ctx, self.interface.last_room.map(|(room, _)| room));
        // the session host's bar is docked over the world's top
        // (`app/folderstorm_characters/bar.rs`): the top bars stay under it
        if self.whole_world_viewport(size) != world {
            bars.top = 0.0;
        }
        Some(viewport::bars::around(inside, ctx.content_rect(), bars))
    }
}
