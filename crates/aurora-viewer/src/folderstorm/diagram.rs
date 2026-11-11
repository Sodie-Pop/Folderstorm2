//! Folderstorm: the interactive diagram of the world viewport
//! (folderstorm/plans/workspaces.md section 9.1, FEATURES.md row 1.2).
//!
//! Folderstorm (C++) planned a miniature of the viewer window, its world
//! rectangle and the monitors under it (doc/plans/visual-viewport-setup.md):
//! the world's edges are dragged to set the margins, and a monitor is picked
//! to put the world on it. This module holds the geometry, pure and tested:
//! the desktop as the app last saw it, the scale into the diagram, the
//! world's edges, snapping to monitor borders, dragging with the quarter
//! limit of [`viewport::MIN_SHARE`], and monitor picking. The drawing and the
//! pointer handling are in [`widget`].
//!
//! Every length here is in window pixels (physical, like the margins), with
//! the origin at the top-left of the viewer window's inside. Monitors keep
//! their desktop rectangle, physical too, and are moved into the window's
//! frame: a monitor left of or above the main one (negative desktop
//! coordinates) simply lands at negative window coordinates. A monitor's
//! scale only shows in its label: margins are physical pixels, so a DPI
//! change does not move them (plan section 8.6).

pub mod widget;

use super::viewport::{self, MonitorLayout, Overrides, ViewportSettings};
use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// One monitor on the desktop: (x, y, width, height) in physical pixels and
/// its scale (1.0 for 100 %).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub rect: [i32; 4],
    pub scale: f32,
}

/// The viewer window and the monitors, as the app last saw them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Desktop {
    /// The window's inside, in physical pixels.
    pub window: (u32, u32),
    /// Where that inside sits on the desktop. None where the system does not
    /// tell (Wayland): the window is then drawn alone, without monitors.
    pub position: Option<(i32, i32)>,
    pub monitors: Vec<Monitor>,
}

impl Desktop {
    /// A window seen without its place on the desktop (Wayland, or before
    /// the app has looked).
    pub fn window_only(window: (u32, u32)) -> Self {
        Self {
            window,
            position: None,
            monitors: Vec::new(),
        }
    }

    /// The window's own rectangle, in window pixels.
    pub fn window_rect(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(self.window.0 as f32, self.window.1 as f32))
    }

    /// The monitors in window pixels, in their order; none without a window
    /// position.
    pub fn monitors_in_window(&self) -> Vec<Rect> {
        let Some((wx, wy)) = self.position else { return Vec::new() };
        self.monitors
            .iter()
            .map(|m| {
                let [x, y, w, h] = m.rect;
                Rect::from_min_size(
                    pos2((x as i64 - wx as i64) as f32, (y as i64 - wy as i64) as f32),
                    vec2(w.max(0) as f32, h.max(0) as f32),
                )
            })
            .collect()
    }

    /// Everything the diagram shows: the window and every monitor.
    pub fn bounds(&self) -> Rect {
        self.monitors_in_window()
            .into_iter()
            .filter(|r| r.is_positive())
            .fold(self.window_rect(), |all, r| all.union(r))
    }

    /// What "Fit to a monitor" works from, when the window's place is known.
    pub fn monitor_layout(&self) -> Option<MonitorLayout> {
        let (x, y) = self.position?;
        Some(MonitorLayout {
            window: [x, y, clamp_i32(self.window.0), clamp_i32(self.window.1)],
            monitors: self.monitors.iter().map(|m| m.rect).collect(),
        })
    }

    /// What putting the world on monitor `index` gives.
    pub fn fit_monitor(&self, index: usize) -> Fit {
        let (Some(layout), Some(monitor)) = (self.monitor_layout(), self.monitors.get(index)) else {
            return Fit::NotUnder;
        };
        let Some((left, right, top, bottom)) = viewport::margins_for_monitor(layout.window, monitor.rect) else {
            return Fit::NotUnder;
        };
        let margins = Margins { left, right, top, bottom };
        let s = ViewportSettings {
            enabled: true,
            ..margins.on(&ViewportSettings::default())
        };
        if viewport::world_rect(self.window, &s, Overrides::default()).is_none() && margins != Margins::default() {
            // a sliver of the monitor under the window: the world would be
            // set aside at once (Folderstorm (C++) refused such a fit too)
            return Fit::TooSmall;
        }
        Fit::Margins(margins)
    }

    /// The monitor under a point of the window (window pixels): the first
    /// in the system's order when monitors overlap (mirrored displays).
    pub fn monitor_at(&self, at: Pos2) -> Option<usize> {
        self.monitors_in_window().iter().position(|r| r.contains(at))
    }

    /// The monitor selected at first: the one holding most of the world,
    /// else most of the window; the first one on a tie. None without
    /// monitors under the window.
    pub fn first_selection(&self, world: Rect) -> Option<usize> {
        let monitors = self.monitors_in_window();
        let best = |area: Rect| {
            monitors
                .iter()
                .enumerate()
                .map(|(n, r)| (n, overlap(*r, area)))
                .filter(|(_, a)| *a > 0.0)
                .fold(None::<(usize, f32)>, |best, (n, a)| match best {
                    Some((_, b)) if b >= a => best,
                    _ => Some((n, a)),
                })
                .map(|(n, _)| n)
        };
        best(world).or_else(|| best(self.window_rect()))
    }

    /// Where an edge may snap along its axis, in window pixels: the window's
    /// two sides and the monitor borders between them.
    pub fn snap_targets(&self, edge: Edge) -> Vec<f32> {
        let window = self.window_rect();
        let (low, high) = match edge.axis() {
            Axis::X => (window.left(), window.right()),
            Axis::Y => (window.top(), window.bottom()),
        };
        let mut targets = vec![low, high];
        for r in self.monitors_in_window() {
            let (a, b) = match edge.axis() {
                Axis::X => (r.left(), r.right()),
                Axis::Y => (r.top(), r.bottom()),
            };
            targets.extend([a, b].into_iter().filter(|t| *t > low && *t < high));
        }
        targets.sort_by(f32::total_cmp);
        targets.dedup();
        targets
    }
}

fn clamp_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

fn overlap(a: Rect, b: Rect) -> f32 {
    let i = a.intersect(b);
    if i.is_positive() { i.area() } else { 0.0 }
}

/// What "Put the world on this monitor" can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Margins(Margins),
    /// The monitor is not under the window (or its place is unknown).
    NotUnder,
    /// Too little of it is under the window: the world would be under a
    /// quarter of the window and set aside.
    TooSmall,
}

/// The four margins, in window pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Margins {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl Margins {
    pub fn of(s: &ViewportSettings) -> Self {
        Self {
            left: s.left,
            right: s.right,
            top: s.top,
            bottom: s.bottom,
        }
    }

    /// `s` with these margins.
    pub fn on(self, s: &ViewportSettings) -> ViewportSettings {
        ViewportSettings {
            left: self.left,
            right: self.right,
            top: self.top,
            bottom: self.bottom,
            ..s.clone()
        }
    }

    pub fn write(self, s: &mut ViewportSettings) {
        (s.left, s.right, s.top, s.bottom) = (self.left, self.right, self.top, self.bottom);
    }

    fn get(self, edge: Edge) -> u32 {
        match edge {
            Edge::Left => self.left,
            Edge::Right => self.right,
            Edge::Top => self.top,
            Edge::Bottom => self.bottom,
        }
    }

    fn set(&mut self, edge: Edge, v: u32) {
        match edge {
            Edge::Left => self.left = v,
            Edge::Right => self.right = v,
            Edge::Top => self.top = v,
            Edge::Bottom => self.bottom = v,
        }
    }
}

/// How the world stands with the current settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum World {
    /// The viewport is off: the world fills the window.
    Off,
    /// On, with margins that leave nothing out: the world fills the window.
    Whole,
    /// On, but the margins leave the world under a quarter of the window:
    /// they are set aside and the world fills the window.
    SetAside,
    /// The world's rectangle, in window pixels.
    Custom(Rect),
}

/// The world for a window of `size` and these settings (outside mouselook
/// and photos, as Preferences shows it).
pub fn world(size: (u32, u32), s: &ViewportSettings) -> World {
    if !s.enabled {
        return World::Off;
    }
    match viewport::world_rect(size, s, Overrides::default()) {
        Some([x, y, w, h]) => World::Custom(Rect::from_min_size(pos2(x as f32, y as f32), vec2(w as f32, h as f32))),
        None if Margins::of(s) == Margins::default() || size.0 == 0 || size.1 == 0 => World::Whole,
        None => World::SetAside,
    }
}

/// The rectangle the margins ask for, kept inside a window of `size` (it
/// may be empty): where the diagram draws the world's edges.
pub fn asked_rect(size: (u32, u32), m: Margins) -> Rect {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let left = (m.left as f32).min(w);
    let top = (m.top as f32).min(h);
    let right = (w - m.right as f32).clamp(left, w);
    let bottom = (h - m.bottom as f32).clamp(top, h);
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// One edge of the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

    pub fn axis(self) -> Axis {
        match self {
            Edge::Left | Edge::Right => Axis::X,
            Edge::Top | Edge::Bottom => Axis::Y,
        }
    }

    fn opposite(self) -> Edge {
        match self {
            Edge::Left => Edge::Right,
            Edge::Right => Edge::Left,
            Edge::Top => Edge::Bottom,
            Edge::Bottom => Edge::Top,
        }
    }

    /// The edge's line in a rectangle: its two ends.
    pub fn line(self, r: Rect) -> [Pos2; 2] {
        match self {
            Edge::Left => [r.left_top(), r.left_bottom()],
            Edge::Right => [r.right_top(), r.right_bottom()],
            Edge::Top => [r.left_top(), r.right_top()],
            Edge::Bottom => [r.left_bottom(), r.right_bottom()],
        }
    }
}

/// From window pixels into the diagram's points and back: one scale for
/// both axes, the picture centred in the room it is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    /// Diagram points per window pixel.
    pub scale: f32,
    /// The diagram point of the window's top-left corner.
    pub origin: Pos2,
}

impl Mapping {
    /// Fits `bounds` (window pixels) into `room` (points), `pad` points from
    /// its sides.
    pub fn fit(bounds: Rect, room: Rect, pad: f32) -> Self {
        let inner = room.shrink(pad);
        let size = bounds.size().max(Vec2::splat(1.0));
        let scale = (inner.width() / size.x).min(inner.height() / size.y).max(f32::MIN_POSITIVE);
        let drawn = size * scale;
        let corner = inner.center() - drawn / 2.0;
        Self {
            scale,
            origin: corner - bounds.min.to_vec2() * scale,
        }
    }

    pub fn to_diagram(self, p: Pos2) -> Pos2 {
        self.origin + p.to_vec2() * self.scale
    }

    pub fn to_window(self, p: Pos2) -> Pos2 {
        Pos2::ZERO + (p - self.origin) / self.scale
    }

    pub fn rect_to_diagram(self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_diagram(r.min), self.to_diagram(r.max))
    }
}

/// The diagram's height for a row `width` points wide: the bounds' own
/// proportions, within limits that keep Preferences readable.
pub fn height_for(width: f32, bounds: Rect) -> f32 {
    let ratio = bounds.height() / bounds.width().max(1.0);
    (width * ratio).clamp(DIAGRAM_MIN_H, DIAGRAM_MAX_H)
}

pub const DIAGRAM_MIN_H: f32 = 110.0;
pub const DIAGRAM_MAX_H: f32 = 240.0;
/// How near an edge the pointer grabs it, in diagram points.
pub const GRAB: f32 = 6.0;
/// How near a monitor border a dragged edge snaps to it, in diagram points.
pub const SNAP: f32 = 6.0;

/// The world's edge under the pointer (a diagram point), the nearest one
/// within [`GRAB`] points of its line.
pub fn edge_at(mapping: &Mapping, world: Rect, at: Pos2) -> Option<Edge> {
    let r = mapping.rect_to_diagram(world);
    Edge::ALL
        .into_iter()
        .filter_map(|edge| {
            let [a, b] = edge.line(r);
            let (along, across, lo, hi) = match edge.axis() {
                Axis::X => (at.y, (at.x - a.x).abs(), a.y, b.y),
                Axis::Y => (at.x, (at.y - a.y).abs(), a.x, b.x),
            };
            (across <= GRAB && along >= lo - GRAB && along <= hi + GRAB).then_some((edge, across))
        })
        .fold(None::<(Edge, f32)>, |best, (edge, d)| match best {
            Some((_, b)) if b <= d => best,
            _ => Some((edge, d)),
        })
        .map(|(edge, _)| edge)
}

/// The margins after dragging `edge` of `start` to `at` (window pixels,
/// along the edge's axis): snapped to the nearest of `targets` within
/// `snap` pixels, then kept between the window's side and the point where
/// the world would fall under a quarter of the window. The other margins
/// stay as they were.
pub fn drag(size: (u32, u32), start: Margins, edge: Edge, at: f32, targets: &[f32], snap: f32) -> Margins {
    let span = match edge.axis() {
        Axis::X => size.0,
        Axis::Y => size.1,
    };
    let at = if at.is_finite() { at } else { 0.0 };
    let at = targets
        .iter()
        .map(|t| (*t, (t - at).abs()))
        .filter(|(_, d)| *d <= snap)
        .fold(None::<(f32, f32)>, |best, (t, d)| match best {
            Some((_, b)) if b <= d => best,
            _ => Some((t, d)),
        })
        .map_or(at, |(t, _)| t);
    // the margin this position gives, measured from the edge's own side
    let wanted = match edge {
        Edge::Left | Edge::Top => at,
        Edge::Right | Edge::Bottom => span as f32 - at,
    };
    let keep = ((span as f32 * viewport::MIN_SHARE).ceil() as u32).max(1);
    let most = span.saturating_sub(start.get(edge.opposite()).saturating_add(keep));
    let margin = (wanted.round().max(0.0) as u32).min(most);
    let mut out = start;
    out.set(edge, margin);
    out
}

/// `AURORA_DEMO_MONITORS`: pretend monitors, read once.
#[derive(Clone, Debug, PartialEq)]
pub enum DemoMonitors {
    /// `wayland` or `none`: no window position, as on Wayland.
    NoPosition,
    /// Monitors, and where the window's inside sits (by default the top-left
    /// corner of all the monitors).
    Layout {
        monitors: Vec<Monitor>,
        window_at: Option<(i32, i32)>,
    },
}

impl DemoMonitors {
    /// The desktop these pretend monitors give a window of `size`.
    pub fn desktop(&self, size: (u32, u32)) -> Desktop {
        match self {
            DemoMonitors::NoPosition => Desktop::window_only(size),
            DemoMonitors::Layout { monitors, window_at } => {
                let corner = monitors
                    .iter()
                    .map(|m| (m.rect[0], m.rect[1]))
                    .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))
                    .unwrap_or((0, 0));
                Desktop {
                    window: size,
                    position: Some(window_at.unwrap_or(corner)),
                    monitors: monitors.clone(),
                }
            }
        }
    }
}

/// Reads `AURORA_DEMO_MONITORS`: monitors separated by commas or
/// semicolons, each `WxH[+X+Y][@scale]` (X11 geometry: `-1920+0` for a
/// monitor left of the main one; `@1.5` or `@150%` for its scale). A monitor
/// without a position goes right of the previous one, tops aligned; the
/// first at 0,0. An optional `window+X+Y` item puts the window's inside
/// there. `wayland` or `none` pretends the window's position is unknown.
/// Anything else makes the whole value an error.
pub fn parse_demo_monitors(text: &str) -> Result<DemoMonitors, String> {
    let items: Vec<&str> = text.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()).collect();
    if items.len() == 1 && matches!(items[0].to_ascii_lowercase().as_str(), "wayland" | "none") {
        return Ok(DemoMonitors::NoPosition);
    }
    let mut monitors: Vec<Monitor> = Vec::new();
    let mut window_at = None;
    for item in items {
        if let Some(rest) = item.strip_prefix("window") {
            let (offset, tail) = offsets(rest).ok_or_else(|| format!("`{item}`: expected window+X+Y"))?;
            if !tail.is_empty() {
                return Err(format!("`{item}`: unexpected `{tail}`"));
            }
            window_at = Some(offset);
            continue;
        }
        let (geometry, scale) = match item.split_once('@') {
            Some((g, s)) => (g, parse_scale(s).ok_or_else(|| format!("`{item}`: bad scale `{s}`"))?),
            None => (item, 1.0),
        };
        let (w, rest) = number(geometry).ok_or_else(|| format!("`{item}`: expected WxH"))?;
        let rest = rest
            .strip_prefix(['x', 'X', '×'])
            .ok_or_else(|| format!("`{item}`: expected WxH"))?;
        let (h, rest) = number(rest).ok_or_else(|| format!("`{item}`: expected WxH"))?;
        let at = if rest.is_empty() {
            monitors.last().map_or((0, 0), |m| (m.rect[0].saturating_add(m.rect[2]), m.rect[1]))
        } else {
            let (offset, tail) = offsets(rest).ok_or_else(|| format!("`{item}`: expected +X+Y after the size"))?;
            if !tail.is_empty() {
                return Err(format!("`{item}`: unexpected `{tail}`"));
            }
            offset
        };
        if w == 0 || h == 0 || w > 65_536 || h > 65_536 {
            return Err(format!("`{item}`: size out of range"));
        }
        monitors.push(Monitor {
            rect: [at.0, at.1, w as i32, h as i32],
            scale,
        });
    }
    if monitors.is_empty() {
        return Err("no monitor".into());
    }
    Ok(DemoMonitors::Layout { monitors, window_at })
}

/// Leading digits as a number, and the rest.
fn number(s: &str) -> Option<(u32, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    Some((s[..end].parse().ok()?, &s[end..]))
}

/// A signed number (`+12`, `-1920`), and the rest.
fn signed(s: &str) -> Option<(i32, &str)> {
    let negative = s.starts_with('-');
    let digits = s.strip_prefix(['+', '-'])?;
    let (n, rest) = number(digits)?;
    let n = i32::try_from(n).ok().filter(|n| *n <= 1_000_000)?;
    Some((if negative { -n } else { n }, rest))
}

/// `+X+Y` with either sign, and the rest.
fn offsets(s: &str) -> Option<((i32, i32), &str)> {
    let (x, rest) = signed(s)?;
    let (y, rest) = signed(rest)?;
    Some(((x, y), rest))
}

fn parse_scale(s: &str) -> Option<f32> {
    let s = s.trim();
    let value: f32 = match s.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f32>().ok()? / 100.0,
        None => s.parse().ok()?,
    };
    (value.is_finite() && (0.25..=8.0).contains(&value)).then_some(value)
}

/// `AURORA_DEMO_MONITORS` in a demo run, read once; a bad value is logged
/// and ignored.
pub fn demo_monitors() -> Option<&'static DemoMonitors> {
    static DEMO: std::sync::OnceLock<Option<DemoMonitors>> = std::sync::OnceLock::new();
    DEMO.get_or_init(|| {
        std::env::var_os("AURORA_DEMO")?;
        let text = std::env::var("AURORA_DEMO_MONITORS").ok()?;
        parse_demo_monitors(&text)
            .map_err(|e| log::warn!("AURORA_DEMO_MONITORS ignored: {e}"))
            .ok()
    })
    .as_ref()
}

fn desktop_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_desktop")
}

/// The app's latest look at the window and the monitors, for the diagram.
pub fn set_desktop(ctx: &egui::Context, desktop: Desktop) {
    ctx.data_mut(|d| d.insert_temp(desktop_id(), desktop));
}

/// The window and the monitors, once the app has looked; else the window
/// alone, at the size egui gives it.
pub fn desktop(ctx: &egui::Context) -> Desktop {
    ctx.data(|d| d.get_temp::<Desktop>(desktop_id())).unwrap_or_else(|| {
        let size = ctx.content_rect().size() * ctx.pixels_per_point();
        let px = |v: f32| if v.is_finite() { v.round().max(0.0) as u32 } else { 0 };
        Desktop::window_only((px(size.x), px(size.y)))
    })
}

#[cfg(test)]
mod tests;
