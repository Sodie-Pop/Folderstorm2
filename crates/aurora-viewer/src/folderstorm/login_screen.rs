//! Folderstorm: the login screen kept on one monitor.
//!
//! Folderstorm (C++) laid the login and its second-factor prompt out in the
//! custom world viewport's slice of the window (FEATURES row 1.1). Here the
//! whole login screen (the brand and the login card, the account cards, the
//! version and news, the community links, the buttons along the bottom, the
//! account manager, the second-factor and password dialogs, the dialogs and
//! windows opened from it) is laid out in one rectangle of the window:
//!
//! - the world viewport's, when one is set and holds
//!   (`viewport::world_rect`, the quarter-of-the-window rule included);
//! - otherwise, when the window spans several monitors (each holding at
//!   least [`MIN_COVER`] of it), the monitor holding the most of the window;
//!   on a tie, the one at the desktop's origin (Windows' primary monitor),
//!   else the first one the system lists;
//! - otherwise the whole window, as in Aurora.
//!
//! The rectangle is given to egui as the room the interface lays itself out
//! in (`RawInput::safe_area_insets`, which `Context::content_rect` leaves
//! out), the way the session host's bar keeps its strip: the login screen
//! lays itself out in `content_rect`. The sky behind it still fills the
//! window ([`sky`]), so the other monitors are not left black.

use super::viewport::{self, MonitorLayout, Overrides, ViewportSettings};
use egui::epaint::MarginF32;

/// The least share of the window's area a monitor must hold to count as
/// one the window spans: a window reaching a few pixels over a neighbour
/// stays a window on one monitor, laid out as in Aurora.
pub const MIN_COVER: f32 = 0.10;

/// The rectangle the login screen is laid out in, in window pixels (x, y,
/// width, height) of a window `size` pixels large; None for the whole
/// window. `monitors` is the app's latest look at the desktop
/// (`viewport::monitors`), None where the system gives no window position
/// (Wayland).
pub fn rect(size: (u32, u32), s: &ViewportSettings, monitors: Option<&MonitorLayout>) -> Option<[u32; 4]> {
    if let Some(world) = viewport::world_rect(size, s, Overrides::default()) {
        return Some(world);
    }
    monitor_rect(size, monitors?)
}

/// The area two desktop rectangles (x, y, width, height) share, in pixels.
fn overlap(a: [i32; 4], b: [i32; 4]) -> i64 {
    let span = |a0: i32, al: i32, b0: i32, bl: i32| {
        let lo = i64::from(a0).max(i64::from(b0));
        let hi = (i64::from(a0) + i64::from(al)).min(i64::from(b0) + i64::from(bl));
        (hi - lo).max(0)
    };
    span(a[0], a[2], b[0], b[2]) * span(a[1], a[3], b[1], b[3])
}

fn holds_origin([x, y, w, h]: [i32; 4]) -> bool {
    x <= 0 && y <= 0 && i64::from(x) + i64::from(w) > 0 && i64::from(y) + i64::from(h) > 0
}

/// The part of the window on the monitor the login screen goes to, when
/// the window spans several monitors ([`rect`]'s rule); None otherwise.
fn monitor_rect(size: (u32, u32), layout: &MonitorLayout) -> Option<[u32; 4]> {
    let clamp = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    let window = [layout.window[0], layout.window[1], clamp(size.0), clamp(size.1)];
    let area = i64::from(window[2]) * i64::from(window[3]);
    if area <= 0 {
        return None;
    }
    let covered: Vec<(usize, i64)> = layout
        .monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| (index, overlap(window, *monitor)))
        .filter(|&(_, shared)| shared as f64 >= area as f64 * f64::from(MIN_COVER))
        .collect();
    if covered.len() < 2 {
        return None;
    }
    let most = covered.iter().map(|&(_, shared)| shared).max()?;
    let mut ties = covered.iter().filter(|&&(_, shared)| shared == most).map(|&(index, _)| index);
    let first = ties.clone().next()?;
    let chosen = ties.find(|&index| holds_origin(layout.monitors[index])).unwrap_or(first);
    let (left, right, top, bottom) = viewport::margins_for_monitor(window, layout.monitors[chosen])?;
    let width = size.0.checked_sub(left.saturating_add(right)).filter(|&w| w > 0)?;
    let height = size.1.checked_sub(top.saturating_add(bottom)).filter(|&h| h > 0)?;
    let rect = [left, top, width, height];
    (rect != [0, 0, size.0, size.1]).then_some(rect)
}

/// The insets that leave `rect` (window pixels; None: the whole window) of
/// a window `size` pixels large, in egui points.
fn insets_of(size: (u32, u32), rect: Option<[u32; 4]>, pixels_per_point: f32) -> MarginF32 {
    let Some([x, y, w, h]) = rect else {
        return MarginF32::ZERO;
    };
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let points = |px: u32| px as f32 / ppp;
    MarginF32 {
        left: points(x),
        top: points(y),
        right: points(size.0.saturating_sub(x.saturating_add(w))),
        bottom: points(size.1.saturating_sub(y.saturating_add(h))),
    }
}

/// The insets the interface's pass is given: what the session host's bar
/// already keeps (`bar`, None when no host started the viewer) and, on the
/// login screen, what lies outside its rectangle (`rect`, [`rect`]). Both
/// leave a rectangle of the window; the room is what both leave.
pub fn insets(bar: Option<egui::SafeAreaInsets>, size: (u32, u32), rect: Option<[u32; 4]>, pixels_per_point: f32) -> egui::SafeAreaInsets {
    let bar = bar.map(|b| b.0).unwrap_or(MarginF32::ZERO);
    let login = insets_of(size, rect, pixels_per_point);
    egui::SafeAreaInsets(MarginF32 {
        left: bar.left.max(login.left),
        right: bar.right.max(login.right),
        top: bar.top.max(login.top),
        bottom: bar.bottom.max(login.bottom),
    })
}

fn sky_id() -> egui::Id {
    egui::Id::unique("folderstorm_login_screen_sky")
}

/// Remembers, for this frame, the room the login screen's sky fills while
/// the screen keeps to a rectangle (`Some`: the window, under the session
/// host's bar if any), or that it does not (`None`). Said every frame.
pub fn set_sky(ctx: &egui::Context, room: Option<egui::Rect>) {
    ctx.data_mut(|d| d.insert_temp(sky_id(), room));
}

/// The rectangle the login screen's sky (Aurora's animated background, and
/// the same sky seen through the frosted cards) is painted over: the whole
/// window while the screen keeps to one monitor, `content_rect` otherwise.
pub fn sky(ctx: &egui::Context) -> egui::Rect {
    ctx.data(|d| d.get_temp::<Option<egui::Rect>>(sky_id()))
        .flatten()
        .unwrap_or_else(|| ctx.content_rect())
}

#[cfg(test)]
mod tests;
