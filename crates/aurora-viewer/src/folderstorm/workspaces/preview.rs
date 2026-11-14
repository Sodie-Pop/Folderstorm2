//! The picture of a layout before it is applied
//! (folderstorm/plans/workspaces.md section 9.3; checkpoint 5 in detail,
//! point 3): what applying it would give on the desktop of now.
//!
//! Folderstorm (C++) had "Preview…", which applied the selected workspace
//! for Preferences' OK or Cancel (doc/help.md "Workspaces"). Layouts here
//! apply at once and *Previous arrangement* undoes; what is shown before
//! is a picture, worked out with the rules an apply follows: the viewer
//! window where [`monitors::resolve`] would put it, the world inside it
//! (`viewport::world_rect`, with its quarter-of-the-window rule), each
//! window where [`fit`] would put it in that window. [`picture`] is pure;
//! [`show`] draws it.

use super::fit::fit;
use super::monitors;
use super::store::{Frame, Layout, Monitor};
use super::window::{Goal, Seen, to_i32};
use super::windows;
use crate::folderstorm::diagram::{self, Mapping, World};
use crate::folderstorm::viewport::ViewportSettings;
use crate::theme::Palette;
use egui::{Rect, Stroke, StrokeKind, pos2, vec2};

/// The desktop of now, as an apply would find it.
#[derive(Clone, Copy, Debug)]
pub struct Now<'a> {
    /// The monitors (none where the system does not list them).
    pub monitors: &'a [Monitor],
    /// The viewer window as the layouts drive it; None: there is none to
    /// drive (a capture, a forced size).
    pub seen: Option<Seen>,
    /// The window's inside, in pixels, and where it is on the desktop when
    /// the system tells.
    pub window: (u32, u32),
    pub position: Option<(i32, i32)>,
    pub pixels_per_point: f32,
    /// The world viewport's settings of now.
    pub world: &'a ViewportSettings,
}

/// What will not be as the layout has it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Notes {
    /// Monitors of the layout that are not there.
    pub missing: usize,
    /// The viewer window cannot go where the layout has it: it stays.
    pub left: bool,
    /// The layout's lock will not be given: a monitor is missing.
    pub unlocked: bool,
    /// The layout has a viewer window and there is none to drive here.
    pub no_window: bool,
    /// The margins leave the world under a quarter of the window: the world
    /// fills it.
    pub set_aside: bool,
    /// Windows whose saved place does not fit as it is.
    pub fitted: usize,
    /// Windows of the layout this version does not know: left alone.
    pub unknown: usize,
}

/// A layout applied now, in desktop pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    /// The monitors of now, then the layout's own that are missing; with
    /// each, whether it is there.
    pub monitors: Vec<(Rect, bool)>,
    /// The viewer window's inside.
    pub window: Rect,
    /// The world, when it does not fill the window.
    pub world: Option<Rect>,
    /// The layout's windows (none for a layout without that part).
    pub windows: Vec<Rect>,
    pub notes: Notes,
}

fn rect_of([x, y, width, height]: [i32; 4]) -> Rect {
    Rect::from_min_size(pos2(x as f32, y as f32), vec2(width.max(0) as f32, height.max(0) as f32))
}

fn monitor_rect(m: &Monitor) -> [i32; 4] {
    [m.x, m.y, to_i32(m.width), to_i32(m.height)]
}

/// The monitor of `now` holding the point, as a rectangle.
fn monitor_holding(at: (i32, i32), now: &[Monitor]) -> Option<[i32; 4]> {
    monitors::monitor_at(i64::from(at.0), i64::from(at.1), now)
        .and_then(|n| now.get(n))
        .map(monitor_rect)
}

/// Where the viewer window would be, with `goal` reached from the window
/// `here` (x, y, width, height).
fn window_after(goal: Goal, here: [i32; 4], now: &[Monitor]) -> [i32; 4] {
    let [x, y, width, height] = here;
    let middle = (x.saturating_add(width / 2), y.saturating_add(height / 2));
    match goal {
        // maximized: over the monitor that holds it (less the task bar,
        // which the picture does not know)
        Goal::Normal { at, maximized: true, .. } => at
            .and_then(|at| monitor_holding(at, now))
            .or_else(|| monitor_holding(middle, now))
            .unwrap_or(here),
        Goal::Normal { at, size, .. } => {
            let (x, y) = at.unwrap_or((x, y));
            [x, y, to_i32(size.0), to_i32(size.1)]
        }
        Goal::Fullscreen { on } => monitor_holding(on.unwrap_or(middle), now).unwrap_or(here),
        Goal::Borderless { rect } => rect,
    }
}

/// `layout` applied on the desktop `now`.
pub fn picture(layout: &Layout, now: &Now) -> Picture {
    let mut notes = Notes::default();
    let (x, y) = now.position.unwrap_or_default();
    let here = [x, y, to_i32(now.window.0), to_i32(now.window.1)];
    // the viewer window: as an apply resolves it
    let asked = match (&layout.window, now.seen) {
        (Some(frame), Some(seen)) => Some(monitors::resolve(frame, &layout.monitors, now.monitors, &seen)),
        (Some(_), None) => {
            notes.no_window = true;
            None
        }
        (None, _) => None,
    };
    if let Some(asked) = &asked {
        notes.missing = asked.missing;
        notes.left = asked.left;
        notes.unlocked = asked.unlocked;
    }
    let window = asked
        .and_then(|asked| asked.plan)
        .map_or(here, |plan| window_after(plan.goal, here, now.monitors));
    let size = (window[2].max(0) as u32, window[3].max(0) as u32);
    let corner = vec2(window[0] as f32, window[1] as f32);
    // the world in that window
    let settings = layout.world.as_ref().unwrap_or(now.world);
    let world = match diagram::world(size, settings) {
        World::Custom(rect) => Some(rect.translate(corner)),
        World::SetAside => {
            notes.set_aside = true;
            None
        }
        World::Off | World::Whole => None,
    };
    // the windows, fitted to the interface that window gives
    let scale = if now.pixels_per_point.is_finite() && now.pixels_per_point > 0.0 {
        now.pixels_per_point
    } else {
        1.0
    };
    let interface = Frame {
        width: size.0 as f32 / scale,
        height: size.1 as f32 / scale,
        pixels_per_point: scale,
    };
    let mut places = Vec::new();
    for (id, place) in layout.windows.iter().flat_map(|saved| &saved.windows) {
        if windows::find(id).is_none() {
            notes.unknown += 1;
            continue;
        }
        let saved = layout.windows.as_ref().map_or(interface, |saved| saved.frame);
        let fitted = fit(place, &saved, &interface);
        notes.fitted += usize::from(fitted.changed);
        places.push(Rect::from_min_size(
            (fitted.pos.to_vec2() * scale).to_pos2() + corner,
            fitted.size * scale,
        ));
    }
    // the monitors of now, then the layout's that are gone
    let mut drawn: Vec<(Rect, bool)> = now.monitors.iter().map(|m| (rect_of(monitor_rect(m)), true)).collect();
    if !now.monitors.is_empty() {
        let gone = layout.monitors.iter().filter(|m| monitors::present(m, now.monitors).is_none());
        drawn.extend(gone.map(|m| (rect_of(monitor_rect(m)), false)));
    }
    Picture {
        monitors: drawn,
        window: rect_of(window),
        world,
        windows: places,
        notes,
    }
}

/// The picture's height, in points.
pub const HEIGHT: f32 = 132.0;

/// Draws `picture` in a rectangle as wide as `ui` gives and [`HEIGHT`]
/// tall: the monitors (the missing ones dashed), the viewer window, the
/// world lit, the windows.
pub fn show(ui: &mut egui::Ui, p: &Palette, picture: &Picture) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(rect, ui.visuals().window_corner_radius, p.field);
    let bounds = picture
        .monitors
        .iter()
        .map(|(monitor, _)| *monitor)
        .chain([picture.window])
        .filter(|r| r.is_positive())
        .reduce(|all, r| all.union(r));
    let Some(bounds) = bounds else { return response };
    let mapping = Mapping::fit(bounds, rect, 8.0);
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
    let none = egui::CornerRadius::ZERO;
    let bold = egui::FontId::new(11.0, crate::ui::fonts::semibold(ui.ctx()));
    for (monitor, there) in &picture.monitors {
        let d = mapping.rect_to_diagram(*monitor);
        if *there {
            painter.rect_filled(d, none, p.raised);
            painter.rect_stroke(d, none, Stroke::new(1.0, p.muted_dim), StrokeKind::Inside);
        } else {
            let corners = [d.left_top(), d.right_top(), d.right_bottom(), d.left_bottom(), d.left_top()];
            painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(1.0, p.warn), 4.0, 3.0));
        }
    }
    let window = mapping.rect_to_diagram(picture.window);
    painter.rect_filled(window, none, p.panel);
    let world = picture.world.map_or(window, |world| mapping.rect_to_diagram(world));
    painter.rect_filled(world, none, p.teal.gamma_multiply(0.28));
    if picture.world.is_some() {
        painter.rect_stroke(world, none, Stroke::new(1.0, p.teal), StrokeKind::Inside);
    }
    for place in &picture.windows {
        let d = mapping.rect_to_diagram(*place).intersect(window);
        if d.is_positive() {
            painter.rect_filled(d, none, p.violet.gamma_multiply(0.45));
            painter.rect_stroke(d, none, Stroke::new(1.0, p.violet_light), StrokeKind::Inside);
        }
    }
    painter.rect_stroke(window, none, Stroke::new(1.5, p.ink), StrokeKind::Outside);
    // the monitors' borders and numbers again, over the window that covers
    // them: where one monitor ends and the next begins
    for (n, (monitor, there)) in picture.monitors.iter().enumerate() {
        let d = mapping.rect_to_diagram(*monitor);
        if *there && d.intersects(window) {
            painter.rect_stroke(d, none, Stroke::new(1.0, p.ink.gamma_multiply(0.25)), StrokeKind::Inside);
        }
        if d.width() >= 16.0 && d.height() >= 16.0 {
            let ink = if *there { p.ink.gamma_multiply(0.7) } else { p.warn };
            painter.text(
                d.left_bottom() + vec2(4.0, -2.0),
                egui::Align2::LEFT_BOTTOM,
                (n + 1).to_string(),
                bold.clone(),
                ink,
            );
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::store::{Placement, Placements, WindowFrame};
    use std::collections::BTreeMap;

    const LEFT: Monitor = Monitor {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };
    const RIGHT: Monitor = Monitor {
        x: 1920,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };

    fn seen(x: i32, y: i32, width: u32, height: u32) -> Seen {
        Seen {
            id: 1,
            inner: (width, height),
            outer: Some((x, y)),
            scale: 1.0,
            maximized: false,
            fullscreen: false,
            minimized: false,
            decorated: true,
            resizable: true,
            normal: None,
        }
    }

    fn now<'a>(monitors: &'a [Monitor], seen: Option<Seen>, world: &'a ViewportSettings) -> Now<'a> {
        Now {
            monitors,
            seen,
            window: seen.map_or((1280, 900), |seen| seen.inner),
            position: seen.and_then(|seen| seen.outer),
            pixels_per_point: 1.0,
            world,
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(width, height))
    }

    /// The world on the right monitor of a window over both, Inventory in
    /// the left margin.
    fn wide() -> Layout {
        let world = ViewportSettings {
            enabled: true,
            left: 1920,
            ..Default::default()
        };
        let windows = Placements {
            frame: Frame {
                width: 3840.0,
                height: 1040.0,
                pixels_per_point: 1.0,
            },
            windows: BTreeMap::from([
                (
                    "inventory".to_owned(),
                    Placement {
                        x: 10.0,
                        y: 50.0,
                        width: 900.0,
                        height: 700.0,
                        minimized: false,
                    },
                ),
                (
                    "a window of a later version".to_owned(),
                    Placement {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                        minimized: false,
                    },
                ),
            ]),
        };
        Layout::new("Wide", Some(world), Some(windows), vec![LEFT, RIGHT]).with_window(Some(WindowFrame::Normal {
            x: Some(0),
            y: Some(0),
            width: 3840,
            height: 1040,
            maximized: false,
            locked: true,
        }))
    }

    #[test]
    fn on_its_own_setup_a_layout_is_drawn_as_saved() {
        let world = ViewportSettings::default();
        let both = [LEFT, RIGHT];
        let picture = picture(&wide(), &now(&both, Some(seen(100, 60, 1280, 900)), &world));
        assert_eq!(
            picture.monitors,
            [(rect(0.0, 0.0, 1920.0, 1080.0), true), (rect(1920.0, 0.0, 1920.0, 1080.0), true)]
        );
        assert_eq!(
            picture.window,
            rect(0.0, 0.0, 3840.0, 1040.0),
            "where the layout has it, not where it is"
        );
        assert_eq!(picture.world, Some(rect(1920.0, 0.0, 1920.0, 1040.0)));
        assert_eq!(picture.windows, [rect(10.0, 50.0, 900.0, 700.0)]);
        assert_eq!(
            picture.notes,
            Notes {
                unknown: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_missing_monitor_is_drawn_dashed_and_the_window_cut_to_what_is_there() {
        let world = ViewportSettings::default();
        let one = [LEFT];
        let picture = picture(&wide(), &now(&one, Some(seen(100, 60, 1280, 900)), &world));
        assert_eq!(
            picture.monitors,
            [(rect(0.0, 0.0, 1920.0, 1080.0), true), (rect(1920.0, 0.0, 1920.0, 1080.0), false)]
        );
        // the size cut to the monitor there is (checkpoint 4 as built)
        assert_eq!(picture.window, rect(0.0, 0.0, 1920.0, 1040.0));
        // a left margin of the whole window: under a quarter, the world fills it
        assert_eq!(picture.world, None);
        assert!(picture.notes.set_aside);
        assert_eq!(picture.notes.missing, 1);
        assert!(picture.notes.unlocked, "no lock while a monitor is missing");
        // Inventory keeps its corner, in a window half as wide
        assert_eq!(picture.windows, [rect(10.0, 50.0, 900.0, 700.0)]);
        assert_eq!(picture.notes.fitted, 0);
    }

    #[test]
    fn without_a_viewer_window_part_the_window_is_drawn_where_it_is() {
        let mut layout = wide();
        layout.window = None;
        let world = ViewportSettings::default();
        let both = [LEFT, RIGHT];
        let picture = picture(&layout, &now(&both, Some(seen(100, 60, 800, 600)), &world));
        assert_eq!(picture.window, rect(100.0, 60.0, 800.0, 600.0));
        // a margin of 1920 pixels in a window of 800: set aside
        assert!(picture.notes.set_aside && picture.world.is_none());
        // Inventory, 900 × 700: cut to the room it has, from the same corner
        assert_eq!(picture.notes.fitted, 1);
        let inventory = picture.windows[0];
        assert_eq!(inventory.size(), vec2(800.0, 600.0));
        assert!(picture.window.expand(60.0).contains_rect(inventory), "{inventory:?}");
    }

    #[test]
    fn without_a_world_part_the_world_of_now_is_drawn() {
        let mut layout = wide();
        layout.world = None;
        layout.windows = None;
        let world = ViewportSettings {
            enabled: true,
            right: 1920,
            ..Default::default()
        };
        let both = [LEFT, RIGHT];
        let picture = picture(&layout, &now(&both, Some(seen(0, 0, 3840, 1040)), &world));
        assert_eq!(picture.world, Some(rect(0.0, 0.0, 1920.0, 1040.0)));
        assert!(picture.windows.is_empty());
        assert_eq!(picture.notes, Notes::default());
    }

    #[test]
    fn with_no_window_to_drive_the_part_is_said_to_be_left() {
        let world = ViewportSettings::default();
        let picture = picture(&wide(), &now(&[], None, &world));
        assert!(picture.notes.no_window);
        assert_eq!(picture.window, rect(0.0, 0.0, 1280.0, 900.0));
        assert!(picture.monitors.is_empty(), "no list of monitors: none drawn, none missing");
        assert_eq!(picture.notes.missing, 0);
    }

    #[test]
    fn full_screen_and_borderless_cover_their_monitors() {
        let world = ViewportSettings::default();
        let both = [LEFT, RIGHT];
        let here = now(&both, Some(seen(100, 60, 1280, 900)), &world);
        let full = Layout::new("Full", None, None, vec![LEFT, RIGHT]).with_window(Some(WindowFrame::Fullscreen { monitor: 1 }));
        assert_eq!(picture(&full, &here).window, rect(1920.0, 0.0, 1920.0, 1080.0));
        let over = Layout::new("Over", None, None, vec![LEFT, RIGHT]).with_window(Some(WindowFrame::Borderless {
            monitors: vec![0, 1],
            locked: false,
        }));
        assert_eq!(picture(&over, &here).window, rect(0.0, 0.0, 3840.0, 1080.0));
        let maximized = Layout::new("Max", None, None, vec![LEFT, RIGHT]).with_window(Some(WindowFrame::Normal {
            x: Some(2000),
            y: Some(40),
            width: 1280,
            height: 900,
            maximized: true,
            locked: false,
        }));
        assert_eq!(picture(&maximized, &here).window, rect(1920.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn another_scale_moves_positions_through_pixels() {
        let mut layout = wide();
        layout.window = None;
        layout.world = None;
        let world = ViewportSettings::default();
        let both = [LEFT, RIGHT];
        let mut here = now(&both, Some(seen(0, 0, 3840, 1040)), &world);
        here.pixels_per_point = 2.0;
        let picture = picture(&layout, &here);
        // saved 10 points of one pixel from the left: still 10 pixels from
        // it; its size in points, twice the pixels, cut to the height it
        // then fills from the top
        let inventory = picture.windows[0];
        assert_eq!(inventory.left_top(), pos2(10.0, 0.0));
        assert_eq!(inventory.size(), vec2(1800.0, 1040.0));
        assert_eq!(picture.notes.fitted, 1);
    }
}
