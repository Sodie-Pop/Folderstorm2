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
    world.filter(|_| s.bars_in_viewport).map(|rect| in_points(rect, pixels_per_point))
}

/// A rectangle of window pixels (x, y, width, height) in egui points.
fn in_points([x, y, w, h]: [u32; 4], pixels_per_point: f32) -> egui::Rect {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    egui::Rect::from_min_size(
        egui::pos2(x as f32 / ppp, y as f32 / ppp),
        egui::vec2(w as f32 / ppp, h as f32 / ppp),
    )
}

/// Remembers the bars' area for this frame (`bars_screen`, `floater_room`).
pub fn set_bars_area(ctx: &egui::Context, rect: Option<egui::Rect>) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(bars_frame_id(), BarsArea { frame, rect }));
}

/// The bars' area and the frame it was set in: a screen that draws no bars
/// (login, the full-frame photo view) leaves an old one behind.
#[derive(Clone, Copy, Debug)]
struct BarsArea {
    frame: u64,
    rect: Option<egui::Rect>,
}

fn bars_frame_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_bars_frame")
}

/// The bars' area when they are confined to the world viewport this frame
/// or the last one (a window shown before the bars this frame reads the
/// last one), None when they follow the whole window.
fn confined_bars(ctx: &egui::Context) -> Option<egui::Rect> {
    let now = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<BarsArea>(bars_frame_id()))
        .filter(|area| area.frame.saturating_add(1) >= now)
        .and_then(|area| area.rect)
}

/// The room a floating window has to fit in: a larger resizable one is
/// shrunk to it, and the shrunk size becomes its size (Aurora's
/// `ui::widgets::Floater`). That room is Aurora's work area `work`, the
/// window between the bars; with the bars inside the world viewport
/// (`bars`), that work area is the world, so the room is the whole viewer
/// window (`window`) instead: a window in the margins keeps its size while
/// the world is narrower than it, as when the viewer window goes from two
/// monitors to one. Where windows open, and how large, is still the work
/// area's business; their positions follow Aurora's anchors as before.
pub fn fit_room(work: egui::Rect, bars: Option<egui::Rect>, window: egui::Rect) -> egui::Rect {
    match bars {
        Some(_) if window.is_finite() && window.is_positive() => window,
        _ => work,
    }
}

/// [`fit_room`] for this frame, from the work area Aurora gives a floater.
pub fn floater_room(ctx: &egui::Context, work: egui::Rect) -> egui::Rect {
    fit_room(work, confined_bars(ctx), ctx.content_rect())
}

/// The world's part of the window (`aurora_render::Renderer::world_rect`,
/// in pixels) and the frame it was set in.
#[derive(Clone, Copy, Debug)]
struct WorldArea {
    frame: u64,
    rect: Option<[u32; 4]>,
}

fn world_area_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_world_area")
}

/// Remembers the world's part of the window for this frame (None: the
/// world fills it), whatever the bars do: the dialogs centre in it.
pub fn set_world_area(ctx: &egui::Context, rect: Option<[u32; 4]>) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(world_area_id(), WorldArea { frame, rect }));
}

/// The rectangle dialogs centre in, of the window `window`: the world's part
/// of it, `world`, when there is one, the window otherwise.
fn dialog_rect(world: Option<egui::Rect>, window: egui::Rect) -> egui::Rect {
    world
        .map(|world| world.intersect(window))
        .filter(|world| world.is_finite() && world.is_positive())
        .unwrap_or(window)
}

/// The rectangle the dialogs, prompts and alerts centre in this frame: the
/// world viewport (set this frame or the last; a screen with no world, as
/// the login, leaves none behind), the window when the world fills it.
pub fn dialog_area(ctx: &egui::Context) -> egui::Rect {
    let now = ctx.cumulative_frame_nr();
    let world = ctx
        .data(|d| d.get_temp::<WorldArea>(world_area_id()))
        .filter(|area| area.frame.saturating_add(1) >= now)
        .and_then(|area| area.rect)
        .map(|rect| in_points(rect, ctx.pixels_per_point()));
    dialog_rect(world, ctx.content_rect())
}

/// What moves a point at `align` of `window` to the same point of `area`:
/// the offset that carries an area anchored to the window (egui's
/// `Area::anchor`) into `area`.
fn anchor_shift(align: egui::Align2, area: egui::Rect, window: egui::Rect) -> egui::Vec2 {
    align.pos_in_rect(&area) - align.pos_in_rect(&window)
}

/// The offset to add to an area anchored at `align` of the window to anchor
/// it at the same point of [`dialog_area`]. egui still keeps the area on the
/// window: a dialog wider or taller than the world spills over its margins
/// rather than off the screen.
pub fn dialog_offset(ctx: &egui::Context, align: egui::Align2) -> egui::Vec2 {
    anchor_shift(align, dialog_area(ctx), ctx.content_rect())
}

/// The offset to add to an area anchored at `align` of the window to anchor
/// it at the same point of the bars' area ([`bars_screen`]): for what hangs
/// from the bars (the notification toasts under the bell, the chat lines
/// over the chat bar, the banners under the top bar).
pub fn bars_offset(ctx: &egui::Context, align: egui::Align2) -> egui::Vec2 {
    anchor_shift(align, bars_screen(ctx), ctx.content_rect())
}

/// A modal's area, which egui centres on the window, centred in
/// [`dialog_area`] instead (`ui::widgets::Modal`).
pub fn modal_area(ctx: &egui::Context, area: egui::Area) -> egui::Area {
    area.anchor(egui::Align2::CENTER_CENTER, dialog_offset(ctx, egui::Align2::CENTER_CENTER))
}

/// How far, in points, a floater's default rect may sit from the window's
/// centre and still count as centred on it.
const CENTRED_SLACK: f32 = 2.0;

/// Where a floater whose default rect is `rect` opens, in a window
/// `window` whose dialogs centre in `area` ([`dialog_area`]): a rect centred
/// on the window (Aurora's `content_rect().center() - size / 2` for pay, buy
/// land, profiles, groups, the script editor…) is centred in `area`
/// instead, wholly inside it on each axis it fits on, on the window
/// otherwise. Any other rect, or `area` being the window, is untouched.
fn floater_place(rect: egui::Rect, window: egui::Rect, area: egui::Rect) -> egui::Rect {
    if area == window || !rect.is_finite() || !window.is_positive() || rect.center().distance(window.center()) > CENTRED_SLACK {
        return rect;
    }
    let placed = egui::Rect::from_center_size(area.center(), rect.size());
    let axis = |min: f32, len: f32, area: egui::Rangef, window: egui::Rangef| {
        let bounds = if len <= area.span() { area } else { window };
        min.clamp(bounds.min, (bounds.max - len).max(bounds.min))
    };
    let min = egui::pos2(
        axis(placed.min.x, placed.width(), area.x_range(), window.x_range()),
        axis(placed.min.y, placed.height(), area.y_range(), window.y_range()),
    );
    egui::Rect::from_min_size(min, rect.size())
}

/// The default position of floater `id` of `size` this frame (`ui::widgets::Floater`):
/// on its first placement only (egui holds no state for it yet: neither
/// shown before nor restored from the saved layout), one centred on the
/// window opens centred in the world viewport instead ([`floater_place`]).
/// With the viewport off, or the world filling the window, it is unchanged.
pub fn floater_default(ctx: &egui::Context, id: egui::Id, pos: egui::Pos2, size: egui::Vec2) -> egui::Pos2 {
    if egui::AreaState::load(ctx, id).is_some() {
        return pos;
    }
    floater_place(egui::Rect::from_min_size(pos, size), ctx.content_rect(), dialog_area(ctx)).min
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
    confined_bars(ctx).unwrap_or_else(|| ctx.content_rect())
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

    fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    #[test]
    fn floaters_fit_the_whole_window_while_the_bars_are_in_the_world() {
        let window = rect(0.0, 0.0, 2560.0, 1080.0);
        // the world on the right 640 px, the bars and the work area inside it
        let world = rect(1920.0, 0.0, 640.0, 1080.0);
        let work = rect(1928.0, 48.0, 624.0, 980.0);
        assert_eq!(fit_room(work, Some(world), window), window);
        // viewport off, bars over the whole window, or a world filling the
        // window (no bars area then): Aurora's work area as before
        let between_bars = rect(8.0, 48.0, 2544.0, 980.0);
        assert_eq!(fit_room(between_bars, None, window), between_bars);
        // a window with no size yet keeps the work area
        assert_eq!(fit_room(work, Some(world), egui::Rect::NOTHING), work);
        assert_eq!(fit_room(work, Some(world), rect(0.0, 0.0, 0.0, 1080.0)), work);
    }

    #[test]
    fn the_bars_area_is_forgotten_by_screens_without_bars() {
        let ctx = egui::Context::default();
        let screen = rect(0.0, 0.0, 1200.0, 800.0);
        let work = rect(808.0, 40.0, 384.0, 720.0);
        let run = |set: Option<Option<egui::Rect>>| {
            let mut room = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    if let Some(bars) = set {
                        set_bars_area(ui.ctx(), bars);
                    }
                    room = floater_room(ui.ctx(), work);
                },
            );
            output.textures_delta.clear();
            room
        };
        assert_eq!(run(None), work, "nothing set yet");
        assert_eq!(run(Some(Some(rect(800.0, 0.0, 400.0, 800.0)))), screen);
        // a window shown before the bars reads the last frame's area
        assert_eq!(run(None), screen);
        // then a screen without bars (login) no longer has one
        assert_eq!(run(None), work);
        assert_eq!(run(Some(None)), work, "bars over the whole window");
    }

    #[test]
    fn the_side_bars_are_laid_out_in_the_world_while_confined_to_it() {
        let ctx = egui::Context::default();
        let screen = rect(0.0, 0.0, 1200.0, 800.0);
        let world = rect(800.0, 0.0, 400.0, 800.0);
        let run = |set: Option<Option<egui::Rect>>| {
            let mut area = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    if let Some(bars) = set {
                        set_bars_area(ui.ctx(), bars);
                    }
                    area = bars_screen(ui.ctx());
                },
            );
            output.textures_delta.clear();
            area
        };
        assert_eq!(run(None), screen, "nothing set yet: the whole window");
        assert_eq!(run(Some(Some(world))), world);
        assert_eq!(run(Some(None)), screen, "bars over the whole window");
    }

    /// The case of the plan's checkpoint 1 (folderstorm/plans/workspaces.md
    /// section 5): Inventory in the left margin, 800 points wide; the
    /// viewer window goes from 1920 to 1280 points with a 960-point left
    /// margin, leaving a 320-point world (a quarter, still kept).
    #[test]
    fn a_floater_in_a_margin_keeps_its_size_when_the_world_is_narrower() {
        const ID: &str = "folderstorm_margin_floater";
        let shown = |ctx: &egui::Context, width: f32, viewport: bool, place: bool| {
            let screen = rect(0.0, 0.0, width, 1080.0);
            let world = rect(960.0, 0.0, width - 960.0, 1080.0);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    let ctx = ui.ctx();
                    let p = crate::theme::Theme::default().palette();
                    // what app.rs does: the bars, and the work area, inside
                    // the world when the viewport is on
                    set_bars_area(ctx, viewport.then_some(world));
                    crate::ui::layout::set_work_area(ctx, world.shrink2(egui::vec2(8.0, 40.0)));
                    crate::ui::layout::set_move_area(ctx, screen);
                    if place {
                        crate::ui::widgets::place_floater(ctx, ID, egui::pos2(8.0, 60.0), egui::vec2(800.0, 600.0));
                    }
                    crate::ui::widgets::Floater::new(ID, "Inventory", egui::pos2(8.0, 60.0), egui::vec2(800.0, 600.0)).show(
                        ctx,
                        &p,
                        &mut true,
                        |ui| ui.label("folders"),
                    );
                },
            );
            output.textures_delta.clear();
            ctx.memory(|m| m.area_rect(egui::Id::unique(ID))).expect("shown")
        };
        for viewport in [true, false] {
            let ctx = egui::Context::default();
            ctx.all_styles_mut(|s| s.animation_time = 0.0);
            shown(&ctx, 1920.0, viewport, true);
            let mut before = egui::Rect::NOTHING;
            for _ in 0..4 {
                before = shown(&ctx, 1920.0, viewport, false);
            }
            assert!(before.width() > 700.0, "placed at its size: {before:?}");
            let mut after = egui::Rect::NOTHING;
            for _ in 0..4 {
                after = shown(&ctx, 1280.0, viewport, false);
            }
            if viewport {
                assert_eq!(after.size(), before.size(), "kept its size in the margin");
                assert_eq!(after.min, before.min, "and its place");
                // and that size is still the one it comes back with
                let mut grown = egui::Rect::NOTHING;
                for _ in 0..4 {
                    grown = shown(&ctx, 1920.0, viewport, false);
                }
                assert_eq!(grown.size(), before.size());
            } else {
                // without the viewport Aurora's rule is untouched: brought
                // down to the work area
                assert!(after.width() <= 304.5, "shrunk to the work area: {after:?}");
            }
        }
    }

    #[test]
    fn dialogs_centre_in_the_world_and_stay_on_the_window() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        let right = rect(1920.0, 0.0, 1920.0, 1080.0);
        assert_eq!(dialog_rect(Some(right), window), right);
        assert_eq!(dialog_rect(None, window), window);
        // a world reaching past the window keeps its part on the window, an
        // empty one leaves the window
        assert_eq!(dialog_rect(Some(rect(1920.0, 0.0, 4000.0, 1080.0)), window), right);
        assert_eq!(dialog_rect(Some(rect(1920.0, 0.0, 0.0, 1080.0)), window), window);
        assert_eq!(dialog_rect(Some(egui::Rect::NOTHING), window), window);
        // the offset of an anchored area: centre to centre, corner to corner
        let centre = anchor_shift(egui::Align2::CENTER_CENTER, right, window);
        assert_eq!(centre, egui::vec2(960.0, 0.0));
        assert_eq!(anchor_shift(egui::Align2::RIGHT_TOP, right, window), egui::Vec2::ZERO);
        assert_eq!(anchor_shift(egui::Align2::LEFT_BOTTOM, right, window), egui::vec2(1920.0, 0.0));
        assert_eq!(anchor_shift(egui::Align2::CENTER_CENTER, window, window), egui::Vec2::ZERO);
    }

    /// A modal (the local assistant's Ask dialog is one: `widgets::Modal`)
    /// in a window over two 1920x1080 monitors, `world` the world's part of
    /// it this frame; `width` the dialog's. Its rect after a few frames.
    fn modal_rect(ctx: &egui::Context, world: Option<Option<[u32; 4]>>, width: f32) -> egui::Rect {
        let p = crate::theme::Theme::default().palette();
        let mut shown = egui::Rect::NOTHING;
        for _ in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect(0.0, 0.0, 3840.0, 1080.0)),
                    ..Default::default()
                },
                |ui| {
                    if let Some(world) = world {
                        set_world_area(ui.ctx(), world);
                    }
                    let modal = crate::ui::widgets::Modal::new(egui::Id::unique("folderstorm_viewport_modal")).show(ui.ctx(), |ui| {
                        ui.set_width(width);
                        ui.label(egui::RichText::new("“claude-code” asks to:").color(p.ink));
                        ui.label("Rename an item");
                    });
                    shown = modal.response.rect;
                },
            );
            output.textures_delta.clear();
        }
        shown
    }

    #[test]
    fn a_modal_is_centred_in_the_world_viewport() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        for (world, area) in [
            ([1920, 0, 1920, 1080], rect(1920.0, 0.0, 1920.0, 1080.0)),
            ([0, 0, 1920, 1080], rect(0.0, 0.0, 1920.0, 1080.0)),
        ] {
            let ctx = egui::Context::default();
            let shown = modal_rect(&ctx, Some(Some(world)), 380.0);
            assert!(area.contains(shown.center()), "{shown:?} centred in {area:?}");
            assert!((shown.center().x - area.center().x).abs() < 1.0, "{shown:?}");
            assert!(area.contains_rect(shown), "wholly in the world: {shown:?}");
        }
        // the world filling the window, or no viewport: centred on the window
        let ctx = egui::Context::default();
        let shown = modal_rect(&ctx, Some(None), 380.0);
        assert!((shown.center().x - window.center().x).abs() < 1.0, "{shown:?}");
        let shown = modal_rect(&egui::Context::default(), None, 380.0);
        assert!((shown.center().x - window.center().x).abs() < 1.0, "{shown:?}");
    }

    #[test]
    fn a_modal_wider_than_the_world_stays_on_the_window() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        // the world on the right quarter, the dialog wider than it
        let ctx = egui::Context::default();
        let shown = modal_rect(&ctx, Some(Some([2880, 0, 960, 1080])), 1400.0);
        assert!(shown.width() > 960.0, "{shown:?}");
        assert!(window.expand(0.5).contains_rect(shown), "on the window: {shown:?}");
        assert!((shown.right() - window.right()).abs() < 1.0, "against the window's edge: {shown:?}");
    }

    #[test]
    fn the_world_area_is_forgotten_by_screens_without_a_world() {
        let ctx = egui::Context::default();
        let right = [1920, 0, 1920, 1080];
        let in_world = modal_rect(&ctx, Some(Some(right)), 380.0);
        assert!(in_world.center().x > 1920.0, "{in_world:?}");
        // the login: nothing sets the world's part any more
        let login = modal_rect(&ctx, None, 380.0);
        assert!((login.center().x - 1920.0).abs() < 1.0, "{login:?}");
    }

    #[test]
    fn a_window_centred_floater_opens_centred_in_the_world() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        let size = egui::vec2(500.0, 400.0);
        let centred = egui::Rect::from_center_size(window.center(), size);
        for world in [rect(1920.0, 0.0, 1920.0, 1080.0), rect(0.0, 0.0, 1920.0, 1080.0)] {
            let placed = floater_place(centred, window, world);
            assert_eq!(placed.size(), size);
            assert_eq!(placed.center(), world.center(), "centred in {world:?}");
            assert!(world.contains_rect(placed));
            // a point or two off the window's centre still counts
            let near = floater_place(centred.translate(egui::vec2(1.5, -1.0)), window, world);
            assert!(world.contains_rect(near), "{near:?}");
            // any other default stays where it was asked
            let aside = egui::Rect::from_min_size(egui::pos2(40.0, 80.0), size);
            assert_eq!(floater_place(aside, window, world), aside);
            let off_centre = centred.translate(egui::vec2(30.0, 0.0));
            assert_eq!(floater_place(off_centre, window, world), off_centre);
        }
        // viewport off, or the world filling the window: unchanged
        assert_eq!(floater_place(centred, window, window), centred);
    }

    #[test]
    fn a_floater_larger_than_the_world_stays_on_the_window() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        let world = rect(2880.0, 0.0, 960.0, 1080.0);
        let centred = egui::Rect::from_center_size(window.center(), egui::vec2(1400.0, 600.0));
        let placed = floater_place(centred, window, world);
        assert!(window.contains_rect(placed), "on the window: {placed:?}");
        assert_eq!(placed.right(), window.right(), "against the window's edge");
        // the height fits: centred in the world on that axis
        assert_eq!(placed.center().y, world.center().y);
        // larger than the window too: from the window's corner
        let huge = egui::Rect::from_center_size(window.center(), egui::vec2(5000.0, 600.0));
        assert_eq!(floater_place(huge, window, world).left(), window.left());
    }

    /// Floater `id`, defaulting to the window's centre, shown a few frames
    /// in a 3840x1080 window with `world` the world's part of it (None: the
    /// viewport off). Its rect.
    fn floater_rect(ctx: &egui::Context, id: &str, world: Option<[u32; 4]>) -> egui::Rect {
        let screen = rect(0.0, 0.0, 3840.0, 1080.0);
        let size = egui::vec2(500.0, 400.0);
        for _ in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    let ctx = ui.ctx();
                    let p = crate::theme::Theme::default().palette();
                    set_world_area(ctx, world);
                    crate::ui::layout::set_work_area(ctx, screen.shrink2(egui::vec2(8.0, 40.0)));
                    crate::ui::layout::set_move_area(ctx, screen);
                    let pos = ctx.content_rect().center() - size * 0.5;
                    crate::ui::widgets::Floater::new(id, "Pay", pos, size).show(ctx, &p, &mut true, |ui| ui.label("L$"));
                },
            );
            output.textures_delta.clear();
        }
        ctx.memory(|m| m.area_rect(egui::Id::unique(id))).expect("shown")
    }

    #[test]
    fn a_floater_opens_in_the_world_only_on_its_first_placement() {
        const ID: &str = "folderstorm_viewport_floater";
        let world = rect(1920.0, 0.0, 1920.0, 1080.0);
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let shown = floater_rect(&ctx, ID, Some([1920, 0, 1920, 1080]));
        assert!(world.contains_rect(shown), "{shown:?}");
        assert!((shown.center().x - world.center().x).abs() < 1.0, "{shown:?}");
        // the viewport off: on the window's centre as in Aurora
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let plain = floater_rect(&ctx, ID, None);
        assert!((plain.center().x - 1920.0).abs() < 1.0, "{plain:?}");
        // a window already placed (shown before, or restored from the saved
        // layout) does not move when the viewport comes on
        let again = floater_rect(&ctx, ID, Some([1920, 0, 1920, 1080]));
        assert_eq!(again.min, plain.min);
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
