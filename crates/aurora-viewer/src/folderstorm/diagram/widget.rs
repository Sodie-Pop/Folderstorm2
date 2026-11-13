//! Folderstorm: the viewport diagram in egui (the geometry is in
//! `super`): drawn to scale in the room it is given, with the world's edges
//! dragged by the pointer and the monitors picked by a click.

use super::{Axis, Desktop, Edge, Fit, Mapping, Margins, World};
use crate::folderstorm::viewport::ViewportSettings;
use crate::theme::Palette;
use egui::{Pos2, Rect, Sense, Stroke, StrokeKind, vec2};

/// The diagram's memory between frames.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    drag: Option<Drag>,
    /// The monitor clicked; None follows the world (`Desktop::first_selection`).
    picked: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    edge: Edge,
    /// The margins when the drag began: Escape puts them back.
    start: Margins,
    /// The edge's position minus the pointer's when grabbed (window pixels
    /// along the edge's axis), so that the edge does not jump to the pointer.
    grab: f32,
    /// Escape was pressed: the rest of this drag does nothing.
    cancelled: bool,
}

/// Room between the diagram's border and what it draws, in points.
pub(super) const PAD: f32 = 10.0;

/// The diagram and, under it, "Put the world on this monitor" with what it
/// would do. Edits the margins of `s`; true when they changed.
pub fn show(ui: &mut egui::Ui, p: &Palette, s: &mut ViewportSettings, desktop: &Desktop) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        changed |= diagram(ui, p, s, desktop);
        changed |= actions(ui, p, s, desktop);
    });
    changed
}

fn state_id(ui: &egui::Ui) -> egui::Id {
    ui.scope_id().with("folderstorm-viewport-diagram")
}

fn load(ui: &egui::Ui) -> State {
    ui.ctx().data(|d| d.get_temp::<State>(state_id(ui))).unwrap_or_default()
}

fn store(ui: &egui::Ui, st: State) {
    ui.ctx().data_mut(|d| d.insert_temp(state_id(ui), st));
}

/// The monitor shown as selected: the one clicked while it exists, else the
/// one holding the world.
fn selected(st: &State, desktop: &Desktop, s: &ViewportSettings) -> Option<usize> {
    let count = desktop.monitors_in_window().len();
    st.picked
        .filter(|n| *n < count)
        .or_else(|| desktop.first_selection(world_or_window(desktop, s)))
}

/// The world's rectangle as it is, or the window when it fills it.
fn world_or_window(desktop: &Desktop, s: &ViewportSettings) -> Rect {
    match super::world(desktop.window, s) {
        World::Custom(r) => r,
        _ => desktop.window_rect(),
    }
}

fn diagram(ui: &mut egui::Ui, p: &Palette, s: &mut ViewportSettings, desktop: &Desktop) -> bool {
    let mut st = load(ui);
    let bounds = desktop.bounds();
    let width = ui.available_width().max(120.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, super::height_for(width, bounds)), Sense::click_and_drag());
    let mapping = Mapping::fit(bounds, rect, PAD);
    let size = desktop.window;
    let live = ui.is_enabled() && s.enabled && size.0 > 0 && size.1 > 0;
    let mut changed = false;

    // dragging an edge
    let asked = super::asked_rect(size, Margins::of(s));
    if live
        && resp.drag_started()
        && let Some(at) = ui.input(|i| i.pointer.press_origin())
        && let Some(edge) = super::edge_at(&mapping, asked, at)
    {
        let pointer = along(edge, mapping.to_window(at));
        let line = along(edge, edge.line(asked)[0]);
        st.drag = Some(Drag {
            edge,
            start: Margins::of(s),
            grab: line - pointer,
            cancelled: false,
        });
    }
    if let Some(drag) = st.drag.as_mut() {
        // (egui itself drops the drag on Escape: read it first)
        if !drag.cancelled && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            drag.cancelled = true;
            drag.start.write(s);
            changed = true;
        }
        if !live || resp.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
            st.drag = None;
        } else if !drag.cancelled
            && let Some(at) = resp.interact_pointer_pos()
        {
            let edge = drag.edge;
            let at = along(edge, mapping.to_window(at)) + drag.grab;
            let snap = super::SNAP / mapping.scale;
            let next = super::drag(size, drag.start, edge, at, &desktop.snap_targets(edge), snap);
            if next != Margins::of(s) {
                next.write(s);
                changed = true;
            }
        }
    }

    // picking a monitor (a double click puts the world on it)
    let monitor_under = |resp: &egui::Response| resp.interact_pointer_pos().and_then(|at| desktop.monitor_at(mapping.to_window(at)));
    if ui.is_enabled() && st.drag.is_none() {
        if resp.double_clicked() {
            if let Some(n) = monitor_under(&resp) {
                st.picked = Some(n);
                if live && let Fit::Margins(m) = desktop.fit_monitor(n) {
                    m.write(s);
                    changed = true;
                }
            }
        } else if resp.clicked()
            && let Some(n) = monitor_under(&resp)
        {
            st.picked = Some(n);
        }
    }

    let hovered_edge = match st.drag {
        Some(d) if !d.cancelled => Some(d.edge),
        Some(_) => None,
        None if live => resp
            .hover_pos()
            .and_then(|at| super::edge_at(&mapping, super::asked_rect(size, Margins::of(s)), at)),
        None => None,
    };
    if let Some(edge) = hovered_edge {
        ui.ctx().set_cursor_icon(match edge.axis() {
            Axis::X => egui::CursorIcon::ResizeHorizontal,
            Axis::Y => egui::CursorIcon::ResizeVertical,
        });
    } else if resp.hovered()
        && resp
            .hover_pos()
            .is_some_and(|at| desktop.monitor_at(mapping.to_window(at)).is_some())
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let selection = selected(&st, desktop, s);
    paint(
        ui,
        p,
        rect,
        &mapping,
        s,
        desktop,
        selection,
        hovered_edge,
        st.drag.filter(|d| !d.cancelled).map(|d| d.edge),
    );
    store(ui, st);
    changed
}

/// A point's coordinate along an edge's axis.
fn along(edge: Edge, at: Pos2) -> f32 {
    match edge.axis() {
        Axis::X => at.x,
        Axis::Y => at.y,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    ui: &egui::Ui,
    p: &Palette,
    rect: Rect,
    mapping: &Mapping,
    s: &ViewportSettings,
    desktop: &Desktop,
    selection: Option<usize>,
    lit: Option<Edge>,
    dragging: Option<Edge>,
) {
    let radius = ui.visuals().window_corner_radius;
    ui.painter().rect_filled(rect, radius, p.field);
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
    let none = egui::CornerRadius::ZERO;
    let window = desktop.window_rect();
    let window_d = mapping.rect_to_diagram(window);
    let small = egui::FontId::proportional(11.0);
    let bold = egui::FontId::new(12.0, crate::ui::fonts::semibold(ui.ctx()));

    // the monitors, numbered, with their size and a scale other than 100 %
    for (n, r) in desktop.monitors_in_window().into_iter().enumerate() {
        let d = mapping.rect_to_diagram(r);
        let under = r.intersects(window) && r.intersect(window).is_positive();
        let fill = if under { p.raised } else { p.raised.gamma_multiply(0.5) };
        painter.rect_filled(d, none, fill);
        painter.rect_stroke(d, none, Stroke::new(1.0, p.muted_dim), StrokeKind::Inside);
        if d.width() < 28.0 || d.height() < 18.0 {
            continue;
        }
        let ink = if under { p.ink } else { p.muted };
        let number = painter.layout_no_wrap((n + 1).to_string(), bold.clone(), ink);
        let corner = d.left_top() + vec2(5.0, 3.0);
        let line_h = number.size().y;
        painter.galley(corner, number, ink);
        if let Some(m) = desktop.monitors.get(n) {
            let text = if (m.scale - 1.0).abs() > 0.005 {
                aurora_i18n::tr!(
                    "folderstorm-viewport-monitor-scaled",
                    width = m.rect[2],
                    height = m.rect[3],
                    percent = (m.scale * 100.0).round() as i64
                )
            } else {
                format!("{}×{}", m.rect[2], m.rect[3])
            };
            let galley = painter.layout_no_wrap(text, small.clone(), p.muted);
            if galley.size().x + 10.0 <= d.width() && line_h + galley.size().y + 6.0 <= d.height() {
                painter.galley(corner + vec2(0.0, line_h), galley, p.muted);
            }
        }
    }

    // the window: its margins dark, the world lit
    painter.rect_filled(window_d, none, p.panel);
    let world = super::world(desktop.window, s);
    let asked = mapping.rect_to_diagram(super::asked_rect(desktop.window, Margins::of(s)));
    let world_d = match world {
        World::Custom(r) => mapping.rect_to_diagram(r),
        _ => window_d,
    };
    let (fill, edge_ink) = match world {
        World::Off => (p.muted.gamma_multiply(0.18), p.muted),
        _ => (p.teal.gamma_multiply(0.28), p.teal),
    };
    painter.rect_filled(world_d, none, fill);
    // the bars: along the top and bottom of the area they follow
    let bars = if s.bars_in_viewport { world_d } else { window_d };
    let strip = 3.0_f32.min(bars.height() / 6.0);
    for y in [bars.top(), bars.bottom() - strip] {
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(bars.left(), y), vec2(bars.width(), strip)),
            none,
            p.violet.gamma_multiply(0.7),
        );
    }
    if matches!(world, World::Custom(_)) {
        painter.rect_stroke(world_d, none, Stroke::new(1.5, edge_ink), StrokeKind::Inside);
    }
    if world == World::SetAside {
        // the margins asked, set aside while they leave under a quarter
        let corners = [
            asked.left_top(),
            asked.right_top(),
            asked.right_bottom(),
            asked.left_bottom(),
            asked.left_top(),
        ];
        painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(1.5, p.warn), 5.0, 4.0));
    }
    painter.rect_stroke(window_d, none, Stroke::new(1.5, p.ink), StrokeKind::Outside);

    // the edges that can be dragged: a handle in the middle of each, the
    // one under the pointer lit
    if ui.is_enabled() && s.enabled {
        for edge in Edge::ALL {
            let [a, b] = edge.line(asked);
            if Some(edge) == lit {
                painter.line_segment([a, b], Stroke::new(3.0, p.violet_light));
            }
            let mid = a + (b - a) / 2.0;
            let handle = match edge.axis() {
                Axis::X => Rect::from_center_size(mid, vec2(4.0, 16.0)),
                Axis::Y => Rect::from_center_size(mid, vec2(16.0, 4.0)),
            };
            let ink = if Some(edge) == lit { p.violet_light } else { p.ink };
            painter.rect_filled(handle, crate::ui::widgets::inner_radius(ui.visuals()), ink);
        }
    }
    // the margin being dragged, by its edge
    if let Some(edge) = dragging {
        let m = Margins::of(s);
        let value = match edge {
            Edge::Left => m.left,
            Edge::Right => m.right,
            Edge::Top => m.top,
            Edge::Bottom => m.bottom,
        };
        let [a, b] = edge.line(asked);
        let mid = a + (b - a) / 2.0;
        let (offset, align) = match edge {
            Edge::Left => (vec2(-8.0, -14.0), egui::Align2::RIGHT_CENTER),
            Edge::Right => (vec2(8.0, -14.0), egui::Align2::LEFT_CENTER),
            Edge::Top => (vec2(14.0, -8.0), egui::Align2::LEFT_BOTTOM),
            Edge::Bottom => (vec2(14.0, 8.0), egui::Align2::LEFT_TOP),
        };
        painter.text(mid + offset, align, format!("{value} px"), bold, p.ink);
    }

    // the selected monitor, outlined over the window
    if let Some(r) = selection.and_then(|n| desktop.monitors_in_window().get(n).copied()) {
        painter.rect_stroke(
            mapping.rect_to_diagram(r).shrink(1.0),
            none,
            Stroke::new(2.0, p.violet_light),
            StrokeKind::Inside,
        );
    }
}

/// "Put the world on this monitor" for the selected one, and the state of
/// the world in words when it needs them.
fn actions(ui: &mut egui::Ui, p: &Palette, s: &mut ViewportSettings, desktop: &Desktop) -> bool {
    use aurora_i18n::tr;
    let st = load(ui);
    let selection = selected(&st, desktop, s);
    let fit = selection.map(|n| desktop.fit_monitor(n));
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let enabled = matches!(fit, Some(Fit::Margins(_)));
        let label = match selection {
            Some(n) => tr!("folderstorm-viewport-put-world-on", n = n + 1),
            None => tr!("folderstorm-viewport-put-world").to_owned(),
        };
        let resp = crate::ui::widgets::action_button_if(
            ui,
            p,
            "device-desktop-filled",
            &label,
            crate::ui::widgets::ActionKind::Flat,
            vec2(0.0, 26.0),
            enabled,
        );
        if resp.clicked()
            && let Some(Fit::Margins(m)) = fit
        {
            m.write(s);
            changed = true;
        }
        let note = match (desktop.position, fit) {
            (None, _) => Some(tr!("folderstorm-viewport-no-monitors")),
            (_, Some(Fit::NotUnder)) => Some(tr!("folderstorm-viewport-not-under")),
            (_, Some(Fit::TooSmall)) => Some(tr!("folderstorm-viewport-too-small")),
            (_, None) => Some(tr!("folderstorm-viewport-no-monitors")),
            (_, Some(Fit::Margins(_))) => None,
        };
        if let Some(note) = note {
            ui.label(egui::RichText::new(note).size(12.0).color(p.muted));
        }
    });
    if super::world(desktop.window, s) == World::SetAside {
        ui.label(egui::RichText::new(tr!("folderstorm-viewport-set-aside")).size(12.0).color(p.warn));
    }
    changed
}

/// The height of [`monitors`]' picture, in points.
const PREVIEW_H: f32 = 96.0;

/// A small picture of monitors (each x, y, width, height in desktop
/// pixels), numbered, with a window's frame over them: the monitors a
/// saved layout counts in, and where it puts the viewer window. The
/// `chosen` ones are lit. With `pick`, a click on a monitor returns its
/// place in the list.
pub fn monitors(
    ui: &mut egui::Ui,
    p: &Palette,
    monitors: &[[i32; 4]],
    window: Option<[i32; 4]>,
    chosen: &[usize],
    pick: bool,
) -> Option<usize> {
    let rect_of = |[x, y, w, h]: [i32; 4]| Rect::from_min_size(egui::pos2(x as f32, y as f32), vec2(w.max(0) as f32, h.max(0) as f32));
    let rects: Vec<Rect> = monitors.iter().map(|m| rect_of(*m)).collect();
    let bounds = rects
        .iter()
        .copied()
        .chain(window.map(rect_of))
        .filter(|r| r.is_positive())
        .reduce(|all, r| all.union(r))?;
    let width = (PREVIEW_H * bounds.width() / bounds.height().max(1.0)).clamp(120.0, ui.available_width().max(120.0));
    let sense = if pick { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, PREVIEW_H), sense);
    let mapping = Mapping::fit(bounds, rect, 6.0);
    let none = egui::CornerRadius::ZERO;
    ui.painter().rect_filled(rect, ui.visuals().window_corner_radius, p.field);
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
    let bold = egui::FontId::new(12.0, crate::ui::fonts::semibold(ui.ctx()));
    let hovered = resp
        .hover_pos()
        .and_then(|at| rects.iter().position(|r| r.contains(mapping.to_window(at))));
    for (n, r) in rects.iter().enumerate() {
        let d = mapping.rect_to_diagram(*r);
        let lit = chosen.contains(&n);
        let fill = if lit { p.violet.gamma_multiply(0.55) } else { p.raised };
        painter.rect_filled(d, none, fill);
        let edge = if pick && hovered == Some(n) { p.violet_light } else { p.muted_dim };
        painter.rect_stroke(d, none, Stroke::new(1.0, edge), StrokeKind::Inside);
        if d.width() >= 16.0 && d.height() >= 16.0 {
            let ink = if lit { p.ink } else { p.muted };
            painter.text(
                d.left_top() + vec2(5.0, 3.0),
                egui::Align2::LEFT_TOP,
                (n + 1).to_string(),
                bold.clone(),
                ink,
            );
        }
    }
    if let Some(window) = window.map(rect_of).filter(|r| r.is_positive()) {
        painter.rect_stroke(mapping.rect_to_diagram(window), none, Stroke::new(1.5, p.teal), StrokeKind::Inside);
    }
    if pick && hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked().then_some(hovered).flatten()
}
