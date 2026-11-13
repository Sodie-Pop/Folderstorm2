//! Folderstorm: layouts in the app (the rules are in
//! `crate::folderstorm::workspaces` and `crate::folderstorm::viewport::bars`;
//! folderstorm/plans/workspaces.md, checkpoints 3 and 4).
//!
//! The only place where a layout meets winit: each frame the viewer
//! window is described to the layouts ([`Seen`]) and what they ask of it
//! is done ([`Command`]). Everything else is decided in
//! `crate::folderstorm::workspaces`, without a window.

use super::*;
use crate::folderstorm::viewport;
use crate::folderstorm::workspaces::apply;
use crate::folderstorm::workspaces::window::{Command, Seen};

impl App {
    /// Each frame, before the interface's pass: a layout's viewer window
    /// is put in place and kept locked, the windows of a layout applied in
    /// the last frames take their places, the monitors are watched, and
    /// the active layout is marked modified when something of it was moved
    /// since (`apply::frame`).
    pub(super) fn folderstorm_workspaces_frame(&mut self, window: &Window) {
        if !matches!(self.screen, Screen::World) {
            return;
        }
        let seen = self.folderstorm_viewer_window(window);
        self.layouts_demo(seen.as_ref());
        let outcome = apply::frame(&self.egui_ctx, &mut apply::Live::of(&mut self.settings), seen);
        for command in &outcome.commands {
            self.folderstorm_viewer_command(window, *command);
        }
        if outcome.preferences {
            // as the Me menu's Preferences does, on our tab
            ui::widgets::open_floater(&self.egui_ctx, "options", &mut self.panels.settings);
            self.options_ui.open(ui::options::Page::Layout, 3);
        }
        if outcome.save {
            self.settings.save();
        }
    }

    /// The card shown when the monitors changed, in the interface's pass
    /// (`crate::folderstorm::workspaces::notice`).
    pub(super) fn folderstorm_workspaces_notice(&self, ctx: &egui::Context, p: &crate::theme::Palette) {
        if matches!(self.screen, Screen::World) {
            crate::folderstorm::workspaces::notice::show(ctx, p);
        }
    }

    /// The viewer window as the layouts see it. None while it is not
    /// theirs to move: a capture, a forced size (`AURORA_WINDOW_SIZE`).
    fn folderstorm_viewer_window(&self, window: &Window) -> Option<Seen> {
        use std::hash::{Hash, Hasher};
        if self.capture.is_some() || forced_window_size().is_some() {
            return None;
        }
        let mut id = std::collections::hash_map::DefaultHasher::new();
        window.id().hash(&mut id);
        let size = window.inner_size();
        let scale = window.scale_factor();
        let s = &self.settings;
        // (Aurora keeps the normal size in points, the corner in pixels)
        let pixels = |points: u32| (f64::from(points) * scale).round() as i32;
        Some(Seen {
            id: id.finish(),
            inner: (size.width, size.height),
            // (Wayland gives no position)
            outer: window.outer_position().ok().map(|at| (at.x, at.y)),
            scale: scale as f32,
            maximized: window.is_maximized(),
            fullscreen: window.fullscreen().is_some(),
            minimized: window.is_minimized().unwrap_or(false),
            decorated: window.is_decorated(),
            resizable: window.is_resizable(),
            normal: s
                .window_x
                .zip(s.window_y)
                .map(|(x, y)| [x, y, pixels(s.window_width), pixels(s.window_height)]),
        })
    }

    /// Does one thing a layout asks of the viewer window.
    fn folderstorm_viewer_command(&mut self, window: &Window, command: Command) {
        use winit::dpi::{PhysicalPosition, PhysicalSize};
        log::debug!("layouts: viewer window: {command:?}");
        match command {
            // Aurora's own full screen, entered and left by its own code
            // (`apply_fullscreen_setting`, before the next frame): the
            // windowed geometry it keeps stays right
            Command::Fullscreen(on) => self.settings.window_fullscreen = on,
            Command::Maximized(on) => window.set_maximized(on),
            Command::Decorations(on) => window.set_decorations(on),
            Command::Resizable(on) => window.set_resizable(on),
            Command::Position(x, y) => window.set_outer_position(PhysicalPosition::new(x, y)),
            Command::Size(width, height) => {
                let _ = window.request_inner_size(PhysicalSize::new(width, height));
            }
        }
        // the new place is the one Aurora remembers for the next start
        // (`record_window_geometry`)
        if matches!(command, Command::Position(..) | Command::Size(..) | Command::Maximized(_)) {
            self.geom_changed = Some(Instant::now());
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
    ///
    /// With a viewer window to move (`seen`: not under a forced size):
    /// `toggle` gives "Wide" the window as it is and "Single" its left
    /// half, and applies them in turn every 300 frames; `lock` gives
    /// "Wide" the window as it is, locked, and applies it; `unplug`, with
    /// pretend monitors (`AURORA_DEMO_MONITORS`), saves "Wide" on all of
    /// them and "Single" on all but the last, applies "Wide", and takes
    /// the last monitor away at frame 600: the card offers "Single".
    fn layouts_demo(&mut self, seen: Option<&Seen>) {
        use crate::folderstorm::workspaces::store::{Layout, Placement, Placements, Store, WindowFrame};
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
            let monitors = capture::monitors(&ctx);
            let frame = seen
                .filter(|_| matches!(mode.as_str(), "toggle" | "lock"))
                .and_then(|seen| capture::viewer_window(seen, &monitors));
            // "Single": the left half of the window, never under its minimum
            let half_frame = match (&frame, seen) {
                (Some(WindowFrame::Normal { x, y, width, height, .. }), Some(seen)) if mode == "toggle" => Some(WindowFrame::Normal {
                    x: *x,
                    y: *y,
                    width: (*width / 2).max((800.0 * seen.scale).ceil() as u32),
                    height: *height,
                    maximized: false,
                    locked: false,
                }),
                _ => None,
            };
            let fewer = match mode.as_str() {
                "unplug" if monitors.len() > 1 => monitors[..monitors.len() - 1].to_vec(),
                _ => monitors.clone(),
            };
            let layout = |name: &str, world: viewport::ViewportSettings, places: [(&str, Placement); 3]| {
                let windows = Placements {
                    frame: room.frame,
                    windows: places.into_iter().map(|(id, place)| (id.to_owned(), place)).collect(),
                };
                let (window, monitors) = match name {
                    "Wide" => (frame.clone().map(|frame| frame.with_lock(mode == "lock")), monitors.clone()),
                    _ => (half_frame.clone(), fewer.clone()),
                };
                Layout::new(name, Some(world), Some(windows), monitors).with_window(window)
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
            "apply-wide" | "lock" => apply_at(0, 180, self),
            "apply-single" => {
                apply_at(0, 180, self);
                apply_at(1, 360, self);
            }
            "toggle" if self.frame_count >= 180 && (self.frame_count - 180).is_multiple_of(300) => {
                let index = ((self.frame_count - 180) / 300 % 2) as usize;
                apply_at(index, self.frame_count, self);
            }
            "unplug" => {
                apply_at(0, 180, self);
                if self.frame_count == 600 {
                    crate::folderstorm::diagram::demo_unplug();
                }
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
