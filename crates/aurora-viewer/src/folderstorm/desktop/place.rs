//! Where the desktops go: the slots (each monitor's part of the viewer
//! window), the room a desktop is laid in, its grid of cells, and the cell
//! a shortcut is shown in (pure rectangle maths, in egui points;
//! folderstorm/plans/margin-desktop.md, "Definitions").

use super::store::MAX_SLOTS;
use crate::folderstorm::backgrounds::place::{areas, margins};
use crate::folderstorm::viewport::MonitorLayout;
use egui::{Pos2, Rect, Vec2, vec2};

/// What a room keeps free along its edges, in points.
pub const PAD: f32 = 8.0;

/// The slots of a window `screen` (in points): each monitor's part of it,
/// from left to right, then top to bottom; the window itself when no
/// monitor is known (Wayland) or none is under it. At most [`MAX_SLOTS`].
pub fn slots(screen: Rect, layout: Option<&MonitorLayout>, pixels_per_point: f32) -> Vec<Rect> {
    let mut out = match layout {
        Some(layout) => areas(screen, layout.window, &layout.monitors, false, pixels_per_point),
        None => vec![screen],
    };
    out.retain(|r| r.is_finite() && r.is_positive());
    // (whole points: two monitors side by side never tie on a rounding)
    let key = |r: &Rect| (r.left().round() as i64, r.top().round() as i64);
    out.sort_by_key(key);
    // mirrored monitors are one slot
    out.dedup_by_key(|r| key(r));
    out.truncate(usize::from(MAX_SLOTS));
    out
}

/// Where the bars are while the world has its own viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bars {
    /// Laid out in this area: the world (*Inside the world*), or the world
    /// grown by the bars that stand in its margins (*Around the world*).
    Confined(Rect),
    /// Over the whole window: the room they leave for the windows.
    Window(Rect),
}

/// The room of a desktop in `slot`: the largest part of the slot that is
/// clear of the world and of the bars, less [`PAD`] on each side. None
/// when the slot is under the world, or what is left holds no `cell`.
pub fn room(slot: Rect, world: Rect, bars: Bars, cell: Vec2) -> Option<Rect> {
    let (kept, limit) = match bars {
        Bars::Confined(area) if area.is_finite() && area.is_positive() => (area.union(world), slot),
        Bars::Confined(_) => (world, slot),
        Bars::Window(work) => (world, slot.intersect(work)),
    };
    if !limit.is_finite() || !limit.is_positive() {
        return None;
    }
    let holds = |r: &Rect| r.width() >= cell.x && r.height() >= cell.y;
    if !kept.intersect(limit).is_positive() {
        // the world and its bars are on other monitors
        return Some(limit.shrink(PAD)).filter(holds);
    }
    let mut best: Option<Rect> = None;
    for margin in margins(limit, kept) {
        let room = margin.shrink(PAD);
        if holds(&room) && best.is_none_or(|b| room.area() > b.area()) {
            best = Some(room);
        }
    }
    best
}

/// The cells of a room: all of one size, from its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub origin: Pos2,
    pub cell: Vec2,
    pub cols: u16,
    pub rows: u16,
}

impl Grid {
    /// The grid of `room`; None when it holds no cell.
    pub fn new(room: Rect, cell: Vec2) -> Option<Self> {
        if !(cell.x > 0.0 && cell.y > 0.0) || !room.is_finite() {
            return None;
        }
        let count = |span: f32, side: f32| ((span + 0.01) / side).floor().clamp(0.0, 1023.0) as u16;
        let (cols, rows) = (count(room.width(), cell.x), count(room.height(), cell.y));
        (cols > 0 && rows > 0).then_some(Grid {
            origin: room.min,
            cell,
            cols,
            rows,
        })
    }

    pub fn contains(&self, (col, row): (u16, u16)) -> bool {
        col < self.cols && row < self.rows
    }

    /// The rectangle of a cell.
    pub fn rect(&self, (col, row): (u16, u16)) -> Rect {
        Rect::from_min_size(
            self.origin + vec2(f32::from(col) * self.cell.x, f32::from(row) * self.cell.y),
            self.cell,
        )
    }

    /// The cell under `pos`.
    pub fn cell_at(&self, pos: Pos2) -> Option<(u16, u16)> {
        let at = (pos - self.origin) / self.cell;
        if at.x < 0.0 || at.y < 0.0 {
            return None;
        }
        Some((at.x.floor() as u16, at.y.floor() as u16)).filter(|cell| self.contains(*cell))
    }

    /// The cells in the order a desktop fills: down, then right.
    pub fn cells(&self) -> impl Iterator<Item = (u16, u16)> + use<> {
        let (cols, rows) = (self.cols, self.rows);
        (0..cols).flat_map(move |col| (0..rows).map(move |row| (col, row)))
    }

    /// The first cell, in that order, that is not in `taken`.
    pub fn first_free(&self, taken: &[(u16, u16)]) -> Option<(u16, u16)> {
        self.cells().find(|cell| !taken.contains(cell))
    }

    /// The free cell nearest to `wanted` (which may be outside the grid);
    /// of two as near, the first in the filling order.
    pub fn nearest_free(&self, wanted: (u16, u16), taken: &[(u16, u16)]) -> Option<(u16, u16)> {
        let distance = |(col, row): (u16, u16)| {
            let (dx, dy) = (i64::from(col) - i64::from(wanted.0), i64::from(row) - i64::from(wanted.1));
            dx * dx + dy * dy
        };
        // (`min_by_key` keeps the first of equal ones)
        self.cells().filter(|cell| !taken.contains(cell)).min_by_key(|cell| distance(*cell))
    }
}

/// Where a window of `size` opened from the shortcut in `cell` goes: in the
/// shortcut's `room`, beside it (to its right, else to its left), level
/// with it as far as the room goes. A room smaller than the window gives
/// its own corner.
pub fn beside(cell: Rect, room: Rect, size: Vec2) -> Pos2 {
    let right = cell.right() + PAD;
    let left = cell.left() - PAD - size.x;
    let x = if right + size.x <= room.right() {
        right
    } else if left >= room.left() {
        left
    } else {
        (room.right() - size.x).max(room.left())
    };
    let y = cell.top().min(room.bottom() - size.y).max(room.top());
    Pos2::new(x, y)
}

/// The cell each shortcut is shown in: `stored` gives each one's id and
/// the cell it keeps. One whose cell is outside the grid (under a bar or
/// the world now), or taken by a shortcut of a smaller id, is shown in the
/// nearest free cell; its stored cell is not changed. One that finds no
/// free cell is not shown. The same at every call.
pub fn shown(stored: &[(u32, (u16, u16))], grid: &Grid) -> Vec<(u32, (u16, u16))> {
    let mut stored = stored.to_vec();
    stored.sort_by_key(|(id, _)| *id);
    let mut taken: Vec<(u16, u16)> = Vec::with_capacity(stored.len());
    let mut out = Vec::with_capacity(stored.len());
    let mut displaced = Vec::new();
    for (id, cell) in stored {
        if grid.contains(cell) && !taken.contains(&cell) {
            taken.push(cell);
            out.push((id, cell));
        } else {
            displaced.push((id, cell));
        }
    }
    for (id, cell) in displaced {
        if let Some(free) = grid.nearest_free(cell, &taken) {
            taken.push(free);
            out.push((id, free));
        }
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn px(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    const CELL: Vec2 = vec2(92.0, 84.0);

    #[test]
    fn a_window_opens_beside_its_shortcut_in_its_room() {
        let room = px(8.0, 8.0, 624.0, 384.0);
        let size = vec2(420.0, 300.0);
        let grid = Grid::new(room, CELL).expect("a grid");
        // to the right of the shortcut, level with it
        let first = grid.rect((0, 0));
        assert_eq!(beside(first, room, size), pos2(first.right() + PAD, first.top()));
        // no room to the right: to its left
        let far = grid.rect((5, 0));
        assert_eq!(beside(far, room, size), pos2(far.left() - PAD - size.x, far.top()));
        // neither: against the room's right edge
        let middle = grid.rect((3, 0));
        assert_eq!(beside(middle, room, size).x, room.right() - size.x);
        // a shortcut low in the room: the window stays in the room
        let low = grid.rect((0, 3));
        assert_eq!(beside(low, room, size).y, room.bottom() - size.y);
        // a room smaller than the window: its corner
        let small = px(100.0, 50.0, 200.0, 200.0);
        assert_eq!(beside(px(100.0, 50.0, 92.0, 84.0), small, size), small.min);
    }

    fn three() -> MonitorLayout {
        MonitorLayout {
            window: [0, 0, 5760, 1080],
            monitors: vec![[3840, 0, 1920, 1080], [0, 0, 1920, 1080], [1920, 0, 1920, 1080]],
        }
    }

    #[test]
    fn slots_are_the_monitors_from_left_to_right() {
        // one monitor
        let screen = px(0.0, 0.0, 1920.0, 1080.0);
        let one = MonitorLayout {
            window: [0, 0, 1920, 1080],
            monitors: vec![[0, 0, 1920, 1080]],
        };
        assert_eq!(slots(screen, Some(&one), 1.0), vec![screen]);
        // three, listed in any order
        let screen = px(0.0, 0.0, 5760.0, 1080.0);
        assert_eq!(
            slots(screen, Some(&three()), 1.0),
            vec![
                px(0.0, 0.0, 1920.0, 1080.0),
                px(1920.0, 0.0, 1920.0, 1080.0),
                px(3840.0, 0.0, 1920.0, 1080.0)
            ]
        );
        // two at 200 %, the left one at negative desktop coordinates
        let screen = px(0.0, 0.0, 1920.0, 540.0);
        let two = MonitorLayout {
            window: [-1920, 0, 3840, 1080],
            monitors: vec![[0, 0, 1920, 1080], [-1920, 0, 1920, 1080]],
        };
        assert_eq!(
            slots(screen, Some(&two), 2.0),
            vec![px(0.0, 0.0, 960.0, 540.0), px(960.0, 0.0, 960.0, 540.0)]
        );
        // one above the other: top to bottom
        let screen = px(0.0, 0.0, 1920.0, 2160.0);
        let stacked = MonitorLayout {
            window: [0, 0, 1920, 2160],
            monitors: vec![[0, 1080, 1920, 1080], [0, 0, 1920, 1080]],
        };
        assert_eq!(
            slots(screen, Some(&stacked), 1.0),
            vec![px(0.0, 0.0, 1920.0, 1080.0), px(0.0, 1080.0, 1920.0, 1080.0)]
        );
        // no monitor list (Wayland), or none under the window: the window
        assert_eq!(slots(screen, None, 1.0), vec![screen]);
        let away = MonitorLayout {
            window: [0, 0, 1920, 2160],
            monitors: vec![[9000, 0, 100, 100]],
        };
        assert_eq!(slots(screen, Some(&away), 1.0), vec![screen]);
        // mirrored monitors are one slot
        let mirrored = MonitorLayout {
            window: [0, 0, 1920, 2160],
            monitors: vec![[0, 0, 1920, 2160], [0, 0, 1920, 2160]],
        };
        assert_eq!(slots(screen, Some(&mirrored), 1.0).len(), 1);
    }

    #[test]
    fn a_room_is_the_slot_less_the_world_and_the_bars() {
        let all = slots(px(0.0, 0.0, 5760.0, 1080.0), Some(&three()), 1.0);
        let world = px(1920.0, 0.0, 1920.0, 1080.0);
        // the bars inside the world: the side monitors are rooms, whole
        let inside = Bars::Confined(world);
        assert_eq!(room(all[0], world, inside, CELL), Some(all[0].shrink(PAD)));
        assert_eq!(room(all[1], world, inside, CELL), None, "under the world");
        assert_eq!(room(all[2], world, inside, CELL), Some(all[2].shrink(PAD)));
        // the bars around the world: a side toolbar 48 points wide in each
        // margin, along the world's height
        let around = Bars::Confined(px(1872.0, 0.0, 2016.0, 1080.0));
        assert_eq!(room(all[0], world, around, CELL), Some(px(0.0, 0.0, 1872.0, 1080.0).shrink(PAD)));
        assert_eq!(room(all[2], world, around, CELL), Some(px(3888.0, 0.0, 1872.0, 1080.0).shrink(PAD)));
        // the bars over the whole window: between the top and bottom bars
        let work = px(8.0, 60.0, 5744.0, 950.0);
        let window = Bars::Window(work);
        assert_eq!(room(all[0], world, window, CELL), Some(px(8.0, 60.0, 1912.0, 950.0).shrink(PAD)));
        assert_eq!(room(all[1], world, window, CELL), None);
        assert_eq!(room(all[2], world, window, CELL), Some(px(3840.0, 60.0, 1912.0, 950.0).shrink(PAD)));
    }

    #[test]
    fn the_world_on_part_of_a_monitor_leaves_the_largest_strip() {
        let slot = px(0.0, 0.0, 1920.0, 1080.0);
        // the world keeps the right two thirds: the left third is the room
        let world = px(640.0, 0.0, 1280.0, 1080.0);
        assert_eq!(
            room(slot, world, Bars::Confined(world), CELL),
            Some(px(0.0, 0.0, 640.0, 1080.0).shrink(PAD))
        );
        // a strip thinner than a cell (with its padding) is no room
        let world = px(100.0, 0.0, 1820.0, 1080.0);
        assert_eq!(room(slot, world, Bars::Confined(world), CELL), None);
        // margins on every side: the largest one
        let world = px(900.0, 100.0, 900.0, 800.0);
        let got = room(slot, world, Bars::Confined(world), CELL).expect("a room");
        assert_eq!(got, px(0.0, 0.0, 900.0, 1080.0).shrink(PAD));
        assert!(!got.intersects(world));
        // the world fills the slot
        assert_eq!(room(slot, slot, Bars::Confined(slot), CELL), None);
        // a world that spans two monitors and a half
        let slot = px(3840.0, 0.0, 1920.0, 1080.0);
        let world = px(0.0, 0.0, 4800.0, 1080.0);
        assert_eq!(
            room(slot, world, Bars::Confined(world), CELL),
            Some(px(4800.0, 0.0, 960.0, 1080.0).shrink(PAD))
        );
    }

    #[test]
    fn cells_follow_the_size_and_fill_down_then_right() {
        use super::super::store::Size;
        let room = px(8.0, 8.0, 1904.0, 1064.0);
        for (size, cols, rows) in [(Size::Small, 25, 15), (Size::Medium, 20, 12), (Size::Large, 17, 10)] {
            let [w, h] = size.cell();
            let grid = Grid::new(room, vec2(w, h)).expect("cells");
            assert_eq!((grid.cols, grid.rows), (cols, rows), "{size:?}");
            assert!(room.contains_rect(grid.rect((cols - 1, rows - 1))), "the last cell is in the room");
            assert!(size.icon() < w && size.icon() < h);
        }
        let grid = Grid::new(room, CELL).expect("cells");
        assert_eq!(grid.rect((0, 0)), px(8.0, 8.0, 92.0, 84.0));
        assert_eq!(grid.rect((2, 1)), px(192.0, 92.0, 92.0, 84.0));
        assert_eq!(grid.cell_at(pos2(200.0, 100.0)), Some((2, 1)));
        assert_eq!(grid.cell_at(pos2(4.0, 100.0)), None, "left of the room");
        assert_eq!(grid.cell_at(pos2(1900.0, 100.0)), None, "past the last column");
        let order: Vec<_> = grid.cells().take(13).collect();
        assert_eq!(order[0], (0, 0));
        assert_eq!(order[11], (0, 11));
        assert_eq!(order[12], (1, 0));
        assert_eq!(grid.first_free(&[(0, 0), (0, 1), (0, 3)]), Some((0, 2)));
        // a room too small, or no size
        assert_eq!(Grid::new(px(0.0, 0.0, 50.0, 500.0), CELL), None);
        assert_eq!(Grid::new(room, Vec2::ZERO), None);
    }

    #[test]
    fn a_shortcut_on_a_blocked_cell_is_shown_in_the_nearest_free_one() {
        // three columns of two rows
        let grid = Grid::new(px(0.0, 0.0, 276.0, 168.0), CELL).expect("cells");
        assert_eq!((grid.cols, grid.rows), (3, 2));
        let stored = [
            (3, (0, 0)),
            // the cell of a smaller id: displaced
            (5, (0, 0)),
            (4, (2, 1)),
            // beyond the grid (the window was wider when it was put there)
            (9, (7, 1)),
            (8, (2, 9)),
        ];
        let first = shown(&stored, &grid);
        assert_eq!(first, vec![(3, (0, 0)), (4, (2, 1)), (5, (0, 1)), (8, (1, 1)), (9, (2, 0))]);
        // the same at every call, in whatever order they are given
        let mut reversed = stored;
        reversed.reverse();
        assert_eq!(shown(&reversed, &grid), first);
        assert_eq!(shown(&stored, &grid), first);
        // more shortcuts than cells: the last ones are not shown
        let many: Vec<_> = (1..=8).map(|id| (id, (0, 0))).collect();
        let got = shown(&many, &grid);
        assert_eq!(got.len(), 6);
        let mut cells: Vec<_> = got.iter().map(|(_, cell)| *cell).collect();
        cells.sort();
        cells.dedup();
        assert_eq!(cells.len(), 6, "one shortcut a cell");
    }
}
