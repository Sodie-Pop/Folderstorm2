//! Fitting a saved window place to the interface of now
//! (folderstorm/plans/workspaces.md sections 5 and 8.6). Pure.
//!
//! A layout keeps each window's place in the interface it was saved in
//! ([`Frame`]). In the same interface the place is used as it is. In
//! another one (a smaller viewer window, a monitor gone, another scale):
//!
//! - the window keeps its offset from the nearest of the eight points
//!   Aurora anchors windows to (the corners and the middles of the sides,
//!   in the order of `ui/layout/anchor.rs`), so a window on the right edge
//!   stays on the right edge;
//! - its size is cut to the interface if it is larger;
//! - it is kept inside, its title bar in reach (`layout::keep_title_in`);
//! - with another number of pixels per point, positions go through
//!   physical pixels (they are tied to the monitors) and sizes stay in
//!   points (so text stays readable).
//!
//! The saved place is never written back: fitting is for the session.

use super::store::{Frame, Placement};
use egui::{Align2, Pos2, Rect, Vec2, pos2, vec2};

/// Positions and sizes this close, in points, are the same.
const SAME: f32 = 0.5;

/// Aurora's eight anchors, in its order of preference on a tie.
const POINTS: [Align2; 8] = [
    Align2::LEFT_TOP,
    Align2::RIGHT_TOP,
    Align2::LEFT_BOTTOM,
    Align2::RIGHT_BOTTOM,
    Align2::CENTER_TOP,
    Align2::RIGHT_CENTER,
    Align2::CENTER_BOTTOM,
    Align2::LEFT_CENTER,
];

/// The anchor of `window` in `room`: the point of the room whose matching
/// point of the window is nearest, and the offset between the two.
fn nearest(room: Rect, window: Rect) -> (Align2, Vec2) {
    let offset = |point: Align2| point.pos_in_rect(&window) - point.pos_in_rect(&room);
    let mut best = (POINTS[0], offset(POINTS[0]));
    for point in POINTS.into_iter().skip(1) {
        let candidate = offset(point);
        if candidate.length_sq() < best.1.length_sq() {
            best = (point, candidate);
        }
    }
    best
}

/// A window's place for this session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fitted {
    /// Its top-left corner, from the interface's.
    pub pos: Pos2,
    pub size: Vec2,
    pub minimized: bool,
    /// The saved place did not fit as it was: moved or made smaller.
    pub changed: bool,
}

impl Frame {
    pub fn size(&self) -> Vec2 {
        vec2(self.width, self.height)
    }

    /// The same interface, for placing windows: the same size and scale.
    pub fn same(&self, other: &Frame) -> bool {
        (self.size() - other.size()).abs().max_elem() <= SAME && (self.pixels_per_point - other.pixels_per_point).abs() <= 0.001
    }
}

/// The place of a window saved at `place` in `saved`, in the interface
/// `now`.
pub fn fit(place: &Placement, saved: &Frame, now: &Frame) -> Fitted {
    let size = vec2(place.width, place.height);
    if saved.same(now) {
        return Fitted {
            pos: pos2(place.x, place.y),
            size,
            minimized: place.minimized,
            changed: false,
        };
    }
    // positions are tied to the monitors' pixels, sizes to the text
    let scale = if now.pixels_per_point > 0.0 && saved.pixels_per_point > 0.0 {
        saved.pixels_per_point / now.pixels_per_point
    } else {
        1.0
    };
    let was_room = Rect::from_min_size(Pos2::ZERO, saved.size() * scale);
    let was = Rect::from_min_size(pos2(place.x * scale, place.y * scale), size);
    let room = Rect::from_min_size(Pos2::ZERO, now.size());
    let (point, offset) = nearest(was_room, was);
    let size = size.min(room.size());
    let local = point.pos_in_rect(&Rect::from_min_size(Pos2::ZERO, size));
    let pos = point.pos_in_rect(&room) + offset - local.to_vec2();
    let pos = crate::ui::layout::keep_title_in(room, pos, size, crate::ui::layout::Edges::default());
    let changed =
        (pos - pos2(place.x, place.y)).abs().max_elem() > SAME || (size - vec2(place.width, place.height)).abs().max_elem() > SAME;
    Fitted {
        pos,
        size,
        minimized: place.minimized,
        changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: f32, height: f32) -> Frame {
        Frame {
            width,
            height,
            pixels_per_point: 1.0,
        }
    }

    fn place(x: f32, y: f32, width: f32, height: f32) -> Placement {
        Placement {
            x,
            y,
            width,
            height,
            minimized: false,
        }
    }

    #[test]
    fn the_same_interface_gives_the_saved_place_exactly() {
        let saved = frame(3840.0, 1080.0);
        // even one that hangs over an edge, as the user left it
        for p in [
            place(8.0, 48.0, 900.0, 700.0),
            place(3700.0, 900.0, 400.0, 300.0),
            place(-50.0, 10.0, 300.0, 200.0),
        ] {
            let fitted = fit(&p, &saved, &frame(3840.2, 1080.0));
            assert_eq!(
                (fitted.pos, fitted.size, fitted.changed),
                (pos2(p.x, p.y), vec2(p.width, p.height), false)
            );
        }
        let minimized = Placement {
            minimized: true,
            ..place(10.0, 10.0, 500.0, 400.0)
        };
        assert!(fit(&minimized, &saved, &saved).minimized);
    }

    #[test]
    fn a_smaller_interface_keeps_each_window_at_its_nearest_anchor() {
        let (saved, now) = (frame(3840.0, 1080.0), frame(1920.0, 1080.0));
        // top left: stays where it is
        let left = fit(&place(8.0, 48.0, 900.0, 700.0), &saved, &now);
        assert_eq!((left.pos, left.size, left.changed), (pos2(8.0, 48.0), vec2(900.0, 700.0), false));
        // against the right edge, 20 points from it: still 20 points from it
        let right = fit(&place(3420.0, 56.0, 400.0, 600.0), &saved, &now);
        assert_eq!(
            (right.pos, right.size, right.changed),
            (pos2(1500.0, 56.0), vec2(400.0, 600.0), true)
        );
        // bottom right corner
        let corner = fit(&place(3530.0, 770.0, 300.0, 300.0), &saved, &now);
        assert_eq!(corner.pos, pos2(1610.0, 770.0));
        // centred at the bottom: centred at the bottom
        let dock = fit(&place(1720.0, 960.0, 400.0, 100.0), &saved, &now);
        assert_eq!(dock.pos, pos2(760.0, 960.0));
    }

    #[test]
    fn a_larger_interface_keeps_the_anchor_too() {
        let (saved, now) = (frame(1920.0, 1080.0), frame(3840.0, 1200.0));
        let right = fit(&place(1500.0, 56.0, 400.0, 600.0), &saved, &now);
        assert_eq!((right.pos, right.size), (pos2(3420.0, 56.0), vec2(400.0, 600.0)));
        let bottom_left = fit(&place(10.0, 700.0, 300.0, 370.0), &saved, &now);
        assert_eq!(bottom_left.pos, pos2(10.0, 820.0));
    }

    #[test]
    fn a_window_larger_than_the_interface_is_cut_to_it_and_the_record_untouched() {
        let (saved, now) = (frame(3840.0, 1080.0), frame(1280.0, 720.0));
        let record = place(100.0, 60.0, 1800.0, 900.0);
        let before = record;
        let fitted = fit(&record, &saved, &now);
        assert_eq!(fitted.size, vec2(1280.0, 720.0));
        assert_eq!(fitted.pos, pos2(0.0, 0.0), "inside, its title bar at the top");
        assert!(fitted.changed);
        assert_eq!(record, before);
    }

    #[test]
    fn the_title_bar_stays_in_reach() {
        let (saved, now) = (frame(3840.0, 1080.0), frame(1920.0, 1080.0));
        // on the right monitor, nearer the top middle than the top right:
        // 830 points right of the middle would hang 70 points out of the
        // new room
        let fitted = fit(&place(2550.0, 100.0, 400.0, 300.0), &saved, &now);
        let room = Rect::from_min_size(Pos2::ZERO, now.size());
        assert_eq!(fitted.pos, pos2(1520.0, 100.0), "brought back inside");
        assert!(fitted.changed);
        // a place saved far outside (a hand-edited file) comes back in
        let far = fit(&place(-30000.0, 30000.0, 400.0, 300.0), &saved, &now);
        assert!(room.contains_rect(Rect::from_min_size(far.pos, far.size)));
    }

    #[test]
    fn another_scale_moves_positions_through_pixels_and_keeps_sizes_in_points() {
        // saved at 100 % on a 3840 × 2160 pixel window, applied at 200 %:
        // the same pixels are 1920 × 1080 points
        let saved = Frame {
            width: 3840.0,
            height: 2160.0,
            pixels_per_point: 1.0,
        };
        let now = Frame {
            width: 1920.0,
            height: 1080.0,
            pixels_per_point: 2.0,
        };
        // 200 pixels from the left and the top: 100 points now
        let fitted = fit(&place(200.0, 200.0, 400.0, 300.0), &saved, &now);
        assert_eq!((fitted.pos, fitted.size), (pos2(100.0, 100.0), vec2(400.0, 300.0)));
        // against the right edge: still against it, with its size in points
        let right = fit(&place(3440.0, 200.0, 400.0, 300.0), &saved, &now);
        assert_eq!((right.pos, right.size), (pos2(1520.0, 100.0), vec2(400.0, 300.0)));
        // the other way: 100 points at 200 % are 200 points at 100 %
        let back = fit(&place(100.0, 100.0, 400.0, 300.0), &now, &saved);
        assert_eq!((back.pos, back.size), (pos2(200.0, 200.0), vec2(400.0, 300.0)));
    }

    #[test]
    fn the_minimized_state_is_kept() {
        let minimized = Placement {
            minimized: true,
            ..place(3420.0, 56.0, 400.0, 600.0)
        };
        let fitted = fit(&minimized, &frame(3840.0, 1080.0), &frame(1920.0, 1080.0));
        assert!(fitted.minimized);
        assert_eq!(fitted.size, vec2(400.0, 600.0), "its expanded size");
    }

    #[test]
    fn the_anchor_rule_is_aurora_s() {
        let room = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        // (the case of `ui/layout/anchor.rs`: the top-right corners are the
        // nearest pair, though the centre is nearer the top middle)
        let rect = Rect::from_min_size(pos2(550.0, 30.0), vec2(600.0, 100.0));
        assert_eq!(nearest(room, rect), (Align2::RIGHT_TOP, vec2(-50.0, 30.0)));
        // a tie goes to the first of the order: the top middle before the
        // bottom one
        let centred = Rect::from_center_size(room.center(), vec2(200.0, 200.0));
        assert_eq!(nearest(room, centred), (Align2::CENTER_TOP, vec2(0.0, 300.0)));
    }
}
