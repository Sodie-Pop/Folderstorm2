//! Where a background goes: the margins around the world, the areas a
//! picture is laid in, and the picture in each (pure rectangle maths, in
//! egui points).

use super::record::Fit;
use egui::{Rect, Vec2, pos2, vec2};

/// The most tiles drawn in one area; a smaller picture is drawn larger.
pub const MAX_TILES: usize = 4096;

/// The window minus the world: up to four rectangles that do not overlap
/// and never cover `world` (the left and right ones the window's height,
/// the top and bottom ones between them). None of them when the world
/// fills the window or is not in it.
pub fn margins(window: Rect, world: Rect) -> Vec<Rect> {
    let world = world.intersect(window);
    if !window.is_positive() || !world.is_positive() {
        return Vec::new();
    }
    let candidates = [
        Rect::from_min_max(window.min, pos2(world.min.x, window.max.y)),
        Rect::from_min_max(pos2(world.max.x, window.min.y), window.max),
        Rect::from_min_max(pos2(world.min.x, window.min.y), pos2(world.max.x, world.min.y)),
        Rect::from_min_max(pos2(world.min.x, world.max.y), pos2(world.max.x, window.max.y)),
    ];
    candidates.into_iter().filter(|r| r.width() >= 0.5 && r.height() >= 0.5).collect()
}

/// The areas a picture is laid in, in points: each monitor's part of the
/// window, or the window itself when `span`, when no monitor is known, or
/// when none overlaps it. `window` and `monitors` are (x, y, width,
/// height) on the desktop in pixels (`viewport::MonitorLayout`); `screen`
/// is the window in points.
pub fn areas(screen: Rect, window: [i32; 4], monitors: &[[i32; 4]], span: bool, pixels_per_point: f32) -> Vec<Rect> {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    if span {
        return vec![screen];
    }
    let [wx, wy, ww, wh] = window;
    let mut out = Vec::new();
    for &[mx, my, mw, mh] in monitors {
        let x0 = mx.max(wx);
        let y0 = my.max(wy);
        let x1 = (mx.saturating_add(mw)).min(wx.saturating_add(ww));
        let y1 = (my.saturating_add(mh)).min(wy.saturating_add(wh));
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let part = Rect::from_min_max(
            screen.min + vec2((x0 - wx) as f32, (y0 - wy) as f32) / ppp,
            screen.min + vec2((x1 - wx) as f32, (y1 - wy) as f32) / ppp,
        );
        out.push(part.intersect(screen));
    }
    out.retain(|r| r.is_positive());
    if out.is_empty() {
        out.push(screen);
    }
    out
}

/// Where a picture of `size` (in points: its own pixels for Centre and
/// Tile, any size of its proportions otherwise) goes in `area`. For Fill
/// and Centre it may be larger than the area: what overflows is clipped.
/// For Tile, the first tile, at the area's top-left corner.
pub fn placed(area: Rect, size: Vec2, fit: Fit) -> Rect {
    if !(size.x > 0.0 && size.y > 0.0) || !area.is_positive() {
        return area;
    }
    match fit {
        Fit::Stretch => area,
        Fit::Fill | Fit::Fit => {
            let sx = area.width() / size.x;
            let sy = area.height() / size.y;
            let scale = if fit == Fit::Fill { sx.max(sy) } else { sx.min(sy) };
            Rect::from_center_size(area.center(), size * scale)
        }
        Fit::Centre => Rect::from_center_size(area.center(), size),
        Fit::Tile => Rect::from_min_size(area.min, size),
    }
}

/// The tiles of a picture of `size` covering `area` from its top-left
/// corner, only those meeting `visible` (the part of the area that is
/// shown). At most [`MAX_TILES`] over the whole area: a smaller picture is
/// drawn larger, by a whole factor.
pub fn tiles(area: Rect, size: Vec2, visible: Rect) -> Vec<Rect> {
    if !(size.x > 0.0 && size.y > 0.0) || !area.is_positive() {
        return Vec::new();
    }
    let count = |size: Vec2| ((area.width() / size.x).ceil() as usize).saturating_mul((area.height() / size.y).ceil() as usize);
    // (from about the right factor: a picture of a fraction of a point
    // would otherwise take many rounds)
    let mut factor = ((count(size) as f32 / MAX_TILES as f32).sqrt().floor()).max(1.0);
    let mut tile = size * factor;
    while count(tile) > MAX_TILES {
        factor += 1.0;
        tile = size * factor;
    }
    let visible = visible.intersect(area);
    if !visible.is_positive() {
        return Vec::new();
    }
    // only the columns and rows that meet the visible part
    let first_col = ((visible.min.x - area.min.x) / tile.x).floor().max(0.0) as usize;
    let last_col = ((visible.max.x - area.min.x) / tile.x).ceil() as usize;
    let first_row = ((visible.min.y - area.min.y) / tile.y).floor().max(0.0) as usize;
    let last_row = ((visible.max.y - area.min.y) / tile.y).ceil() as usize;
    let mut out = Vec::new();
    for row in first_row..last_row {
        for col in first_col..last_col {
            let min = area.min + vec2(col as f32 * tile.x, row as f32 * tile.y);
            out.push(Rect::from_min_size(min, tile));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    /// No two rectangles overlap, none meets the world, and together with
    /// the world they cover the window.
    fn tiles_the_window(window: Rect, world: Rect, margins: &[Rect]) {
        let area: f32 = margins.iter().map(|m| m.area()).sum::<f32>() + world.intersect(window).area();
        assert!((area - window.area()).abs() < 0.01, "{area} vs {}", window.area());
        for (i, a) in margins.iter().enumerate() {
            assert!(!a.intersects(world) || a.intersect(world).area() < 1e-3, "{a:?} meets the world");
            assert!(window.contains_rect(*a));
            for b in &margins[i + 1..] {
                assert!(a.intersect(*b).area() < 1e-3, "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn the_margins_surround_the_world_and_never_cover_it() {
        let window = r(0.0, 0.0, 5760.0, 1080.0);
        // the middle one of three monitors
        let world = r(1920.0, 0.0, 1920.0, 1080.0);
        let m = margins(window, world);
        assert_eq!(m, vec![r(0.0, 0.0, 1920.0, 1080.0), r(3840.0, 0.0, 1920.0, 1080.0)]);
        tiles_the_window(window, world, &m);
        // the right half: one margin
        let world = r(1920.0, 0.0, 1920.0, 1080.0);
        let window2 = r(0.0, 0.0, 3840.0, 1080.0);
        assert_eq!(margins(window2, world), vec![r(0.0, 0.0, 1920.0, 1080.0)]);
        // the bars around: an eighth above and below, a third at the left
        let world = r(1280.0, 135.0, 2560.0, 810.0);
        let m = margins(window2, world);
        assert_eq!(m.len(), 3);
        tiles_the_window(window2, world, &m);
        // all four sides
        let world = r(100.0, 50.0, 1000.0, 600.0);
        let m = margins(r(0.0, 0.0, 1280.0, 800.0), world);
        assert_eq!(m.len(), 4);
        tiles_the_window(r(0.0, 0.0, 1280.0, 800.0), world, &m);
        // the world fills the window, or is not in it
        assert!(margins(window, window).is_empty());
        assert!(margins(window, r(9000.0, 0.0, 10.0, 10.0)).is_empty());
        // in points at 150 %: the same shapes
        let world = r(1280.0, 0.0, 1280.0, 720.0);
        let m = margins(r(0.0, 0.0, 2560.0, 720.0), world);
        assert_eq!(m, vec![r(0.0, 0.0, 1280.0, 720.0)]);
    }

    #[test]
    fn each_monitor_is_an_area_unless_the_picture_spans_them() {
        let screen = r(0.0, 0.0, 5760.0, 1080.0);
        let monitors = [[0, 0, 1920, 1080], [1920, 0, 1920, 1080], [3840, 0, 1920, 1080]];
        let window = [0, 0, 5760, 1080];
        assert_eq!(
            areas(screen, window, &monitors, false, 1.0),
            vec![
                r(0.0, 0.0, 1920.0, 1080.0),
                r(1920.0, 0.0, 1920.0, 1080.0),
                r(3840.0, 0.0, 1920.0, 1080.0)
            ]
        );
        assert_eq!(areas(screen, window, &monitors, true, 1.0), vec![screen]);
        // the window's inside starts at (100, 40) on the desktop, at 200 %
        let screen = r(0.0, 0.0, 1900.0, 520.0);
        let window = [100, 40, 3800, 1040];
        let monitors = [[0, 0, 1920, 1080], [1920, 0, 2560, 1440]];
        assert_eq!(
            areas(screen, window, &monitors, false, 2.0),
            vec![r(0.0, 0.0, 910.0, 520.0), r(910.0, 0.0, 990.0, 520.0)]
        );
        // no monitor known (Wayland), or none under the window
        assert_eq!(areas(screen, window, &[], false, 2.0), vec![screen]);
        assert_eq!(areas(screen, window, &[[9000, 0, 100, 100]], false, 2.0), vec![screen]);
    }

    #[test]
    fn each_fit_places_the_picture() {
        let area = r(0.0, 0.0, 1920.0, 1080.0);
        let square = vec2(500.0, 500.0);
        // Fill: covers, the sides cut
        assert_eq!(placed(area, square, Fit::Fill), r(0.0, -420.0, 1920.0, 1920.0));
        // Fit: whole, the colour around
        assert_eq!(placed(area, square, Fit::Fit), r(420.0, 0.0, 1080.0, 1080.0));
        assert_eq!(placed(area, square, Fit::Stretch), area);
        assert_eq!(placed(area, square, Fit::Centre), r(710.0, 290.0, 500.0, 500.0));
        assert_eq!(placed(area, square, Fit::Tile), r(0.0, 0.0, 500.0, 500.0));
        // larger than the area: Centre overflows on both sides
        assert_eq!(placed(area, vec2(3840.0, 2160.0), Fit::Centre), r(-960.0, -540.0, 3840.0, 2160.0));
        // an area that is not on the window's corner
        let area = r(1920.0, 0.0, 1920.0, 1080.0);
        assert_eq!(placed(area, vec2(3840.0, 2160.0), Fit::Fit), area);
        // nothing to place
        assert_eq!(placed(area, Vec2::ZERO, Fit::Fill), area);
    }

    #[test]
    fn tiles_cover_the_visible_part_and_are_capped() {
        let area = r(0.0, 0.0, 1000.0, 500.0);
        let all = tiles(area, vec2(300.0, 200.0), area);
        assert_eq!(all.len(), 4 * 3);
        assert_eq!(all[0], r(0.0, 0.0, 300.0, 200.0));
        assert_eq!(*all.last().expect("tiles"), r(900.0, 400.0, 300.0, 200.0));
        // only the ones the margin shows
        let left = tiles(area, vec2(300.0, 200.0), r(0.0, 0.0, 250.0, 500.0));
        assert_eq!(left.len(), 3);
        assert!(left.iter().all(|t| t.min.x == 0.0));
        let middle = tiles(area, vec2(300.0, 200.0), r(310.0, 210.0, 10.0, 10.0));
        assert_eq!(middle, vec![r(300.0, 200.0, 300.0, 200.0)]);
        // a tiny picture over a wide window: drawn larger
        let wide = r(0.0, 0.0, 7680.0, 2160.0);
        let tiny = tiles(wide, vec2(4.0, 4.0), wide);
        assert!(tiny.len() <= MAX_TILES, "{}", tiny.len());
        let size = tiny[0].size();
        assert!(size.x > 4.0 && (size.x / 4.0).fract() == 0.0, "{size:?}");
        assert!(tiles(wide, Vec2::ZERO, wide).is_empty());
        assert!(tiles(wide, vec2(10.0, 10.0), r(9000.0, 0.0, 10.0, 10.0)).is_empty());
    }
}
