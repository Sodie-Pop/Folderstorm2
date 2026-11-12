//! Folderstorm: a custom world viewport.
//!
//! Port of Folderstorm's custom world viewport (doc/help.md "Wide-screen
//! layout", Folderstorm contributors, originally LGPL 2.1): the window may be
//! stretched over several monitors while the 3D world (its camera, its HUDs,
//! the mouse in the world) stays in one rectangle, set by left, right, top and
//! bottom margins. Windows (inventory, map, chat, Preferences…) can sit in the
//! margins without covering the world. Folderstorm scaled two opposite
//! margins down when they left less than 5% of the window; here, margins that
//! leave the world less than a quarter of the window's width or height are
//! set aside and the world fills the window, so that a window shrunk from two
//! monitors to one (a monitor unplugged) never leaves the interface beside a
//! sliver of world. A separate switch keeps
//! or drops the viewport in mouselook. By default the top, bottom and side
//! bars follow the viewport instead of the whole window.
//!
//! Aurora's renderer draws the world at the rectangle's size and copies it
//! into the window there (`aurora_render::Renderer::set_world_rect`); this
//! module only decides the rectangle.

use serde::{Deserialize, Serialize};

/// The least share of the window's width and height the world keeps; below
/// it the margins are set aside until the window grows or they change.
pub const MIN_SHARE: f32 = 0.25;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewportSettings {
    /// "Use a custom world viewport" (off by default).
    pub enabled: bool,
    /// Margins in window pixels.
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
    /// The viewport stays in mouselook; off, mouselook fills the window.
    pub in_mouselook: bool,
    /// The top, bottom and side bars stay inside the viewport; off, they
    /// follow the whole window as in Aurora.
    pub bars_in_viewport: bool,
}

impl Default for ViewportSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
            in_mouselook: true,
            bars_in_viewport: true,
        }
    }
}

/// When the world fills the window whatever the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    pub mouselook: bool,
    /// A photo being taken, the photo viewfinder in full frame, or a screen
    /// with no world (login).
    pub full_frame: bool,
}

/// The world's span between two opposite margins, or None when it keeps
/// less than [`MIN_SHARE`] of `span`.
fn fit_pair(a: u32, b: u32, span: u32) -> Option<u32> {
    let keep = ((span as f32 * MIN_SHARE).ceil() as u32).max(1);
    span.checked_sub(a.saturating_add(b)).filter(|&world| world >= keep)
}

/// The world's part of a window of `size` (x, y, width, height in window
/// pixels), or None when the world fills the window.
pub fn world_rect(size: (u32, u32), s: &ViewportSettings, now: Overrides) -> Option<[u32; 4]> {
    if !s.enabled || now.full_frame || (now.mouselook && !s.in_mouselook) || size.0 == 0 || size.1 == 0 {
        return None;
    }
    let width = fit_pair(s.left, s.right, size.0)?;
    let height = fit_pair(s.top, s.bottom, size.1)?;
    let rect = [s.left, s.top, width, height];
    (rect != [0, 0, size.0, size.1]).then_some(rect)
}

/// The margins that put the world exactly on one monitor: `window` and
/// `monitor` are (x, y, width, height) on the desktop, in pixels. None when
/// the monitor does not overlap the window.
pub fn margins_for_monitor(window: [i32; 4], monitor: [i32; 4]) -> Option<(u32, u32, u32, u32)> {
    let [wx, wy, ww, wh] = window;
    let [mx, my, mw, mh] = monitor;
    let x0 = mx.max(wx);
    let y0 = my.max(wy);
    let x1 = (mx + mw).min(wx + ww);
    let y1 = (my + mh).min(wy + wh);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(((x0 - wx) as u32, (wx + ww - x1) as u32, (y0 - wy) as u32, (wy + wh - y1) as u32))
}

/// The bars' area this frame, in egui points: the world viewport's when
/// the bars stay inside it. None leaves them on the whole window.
pub fn bars_rect(s: &ViewportSettings, world: Option<[u32; 4]>, pixels_per_point: f32) -> Option<egui::Rect> {
    let [x, y, w, h] = world.filter(|_| s.bars_in_viewport)?;
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

fn bars_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_bars")
}

/// Remembers the bars' area for this frame (`bars_screen`).
pub fn set_bars_area(ctx: &egui::Context, rect: Option<egui::Rect>) {
    ctx.data_mut(|d| d.insert_temp::<Option<egui::Rect>>(bars_id(), rect));
}

/// The window and the monitors on the desktop, as (x, y, width, height)
/// in pixels: what "Fit to a monitor" works from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorLayout {
    pub window: [i32; 4],
    pub monitors: Vec<[i32; 4]>,
}

fn monitors_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_monitors")
}

/// The app's latest look at the window and the monitors.
pub fn set_monitors(ctx: &egui::Context, layout: MonitorLayout) {
    ctx.data_mut(|d| d.insert_temp(monitors_id(), layout));
}

/// The window and the monitors, once the app has looked.
pub fn monitors(ctx: &egui::Context) -> Option<MonitorLayout> {
    ctx.data(|d| d.get_temp::<MonitorLayout>(monitors_id()))
}

/// The rectangle the bars are laid out in: the world viewport's when they
/// stay inside it, the window's otherwise.
pub fn bars_screen(ctx: &egui::Context) -> egui::Rect {
    ctx.data(|d| d.get_temp::<egui::Rect>(bars_id()))
        .unwrap_or_else(|| ctx.content_rect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(left: u32, right: u32, top: u32, bottom: u32) -> ViewportSettings {
        ViewportSettings {
            enabled: true,
            left,
            right,
            top,
            bottom,
            ..Default::default()
        }
    }

    #[test]
    fn off_or_empty_margins_fill_the_window() {
        let size = (5760, 1080);
        assert_eq!(world_rect(size, &ViewportSettings::default(), Overrides::default()), None);
        assert_eq!(world_rect(size, &on(0, 0, 0, 0), Overrides::default()), None);
    }

    #[test]
    fn three_monitors_world_on_the_middle_one() {
        let rect = world_rect((5760, 1080), &on(1920, 1920, 0, 0), Overrides::default());
        assert_eq!(rect, Some([1920, 0, 1920, 1080]));
    }

    #[test]
    fn margins_leaving_less_than_a_quarter_fill_the_window() {
        // a window over two monitors shrunk to one: the margin set for the
        // left monitor would leave nothing, so the world takes the window
        let two = on(1920, 0, 0, 0);
        assert_eq!(world_rect((3840, 1080), &two, Overrides::default()), Some([1920, 0, 1920, 1080]));
        assert_eq!(world_rect((1920, 1080), &two, Overrides::default()), None);
        assert_eq!(world_rect((2000, 1080), &two, Overrides::default()), None, "an 80 px sliver");
        // exactly a quarter is kept, just under it is not
        assert_eq!(
            world_rect((1000, 800), &on(375, 375, 0, 0), Overrides::default()),
            Some([375, 0, 250, 800])
        );
        assert_eq!(world_rect((1000, 800), &on(376, 375, 0, 0), Overrides::default()), None);
        // the height alike, and margins larger than the window
        assert_eq!(world_rect((1000, 800), &on(0, 0, 700, 0), Overrides::default()), None);
        assert_eq!(world_rect((1000, 800), &on(u32::MAX, u32::MAX, 0, 0), Overrides::default()), None);
    }

    #[test]
    fn mouselook_and_full_frame_photos_fill_the_window_when_asked() {
        let size = (3840, 1080);
        let mut s = on(1920, 0, 0, 0);
        let mouselook = Overrides {
            mouselook: true,
            ..Default::default()
        };
        assert!(world_rect(size, &s, mouselook).is_some(), "kept in mouselook by default");
        s.in_mouselook = false;
        assert_eq!(world_rect(size, &s, mouselook), None);
        let photo = Overrides {
            full_frame: true,
            ..Default::default()
        };
        assert_eq!(world_rect(size, &s, photo), None);
    }

    #[test]
    fn bars_follow_the_viewport_unless_asked_not_to() {
        let mut s = on(1920, 0, 0, 0);
        let rect = bars_rect(&s, Some([1920, 0, 1920, 1080]), 2.0).expect("confined");
        assert_eq!(rect, egui::Rect::from_min_size(egui::pos2(960.0, 0.0), egui::vec2(960.0, 540.0)));
        assert_eq!(bars_rect(&s, None, 1.0), None, "no viewport this frame");
        s.bars_in_viewport = false;
        assert_eq!(bars_rect(&s, Some([1920, 0, 1920, 1080]), 1.0), None);
    }

    #[test]
    fn margins_for_a_monitor_inside_the_window() {
        // window over two 1920x1080 monitors, the right one at x = 1920
        let window = [0, 0, 3840, 1080];
        assert_eq!(margins_for_monitor(window, [1920, 0, 1920, 1080]), Some((1920, 0, 0, 0)));
        assert_eq!(margins_for_monitor(window, [0, 0, 1920, 1080]), Some((0, 1920, 0, 0)));
        // a monitor left of the main one (negative desktop coordinates), the
        // window partly over it and a taller monitor
        assert_eq!(
            margins_for_monitor([-1000, 0, 3000, 1080], [-1920, -200, 1920, 1440]),
            Some((0, 2000, 0, 0))
        );
        assert_eq!(margins_for_monitor(window, [5000, 0, 1920, 1080]), None);
    }
}
