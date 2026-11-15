//! Where the tiles go: their sizes, the pages they fill, and the search.
//! Pure: sizes in points, texts given by the caller (folderstorm/plans/
//! gesture-boards.md, decisions 6 and 7).
//!
//! Folderstorm (C++)'s "compact board refinement": tiles are packed in
//! order, in rows ("shelves") as wide as the window, as many rows as its
//! height holds; what does not fit goes to the next page. Only the page
//! shown is drawn. A tile larger than the window is drawn at the window's
//! size, without changing its stored size.

use super::store::{Account, Board};
use egui::{Rect, Vec2, pos2, vec2};
use uuid::Uuid;

/// Space between two tiles, in points.
pub const GAP: f32 = 4.0;
/// Room on each side of a tile's label, in points.
pub const PAD: f32 = 6.0;
/// Room taken by a sound's speaker at the end of its tile, in points.
pub const SPEAKER: f32 = 14.0;

/// The width of a tile on `board` whose label is `label` points wide:
/// its size (its own, else the board's), or with "Show full names" as wide
/// as its whole label (and its speaker), never narrower than its size,
/// never wider than `room` (the board's width).
pub fn tile_width(board: &Board, size: [f32; 2], label: f32, sound: bool, room: f32) -> f32 {
    let base = size[0];
    let wanted = if board.full_names {
        base.max(label + 2.0 * PAD + if sound { SPEAKER } else { 0.0 })
    } else {
        base
    };
    wanted.min(room.max(1.0))
}

/// The tiles of one page: their places in the board's order (index into
/// the sizes given, and rectangle from the top-left of the room).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub tiles: Vec<(usize, Rect)>,
}

impl Page {
    /// The first and one past the last index of the page.
    pub fn span(&self) -> std::ops::Range<usize> {
        match (self.tiles.first(), self.tiles.last()) {
            (Some(first), Some(last)) => first.0..last.0 + 1,
            _ => 0..0,
        }
    }
}

/// The pages that `sizes` fill in a room of `room`, rows packed left to
/// right in order, at most `per_page` tiles a page (0: no cap). A tile is
/// cut to the room's size. Never empty: one empty page for no tile.
pub fn pages(sizes: &[Vec2], room: Vec2, per_page: usize) -> Vec<Page> {
    let room = vec2(room.x.max(1.0), room.y.max(1.0));
    let mut pages = vec![Page::default()];
    let (mut x, mut y, mut row) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (index, size) in sizes.iter().enumerate() {
        let size = vec2(size.x.clamp(1.0, room.x), size.y.clamp(1.0, room.y));
        let page_len = pages.last().map_or(0, |p| p.tiles.len());
        // a new row when this one has no room left
        if page_len > 0 && x > 0.0 && x + size.x > room.x {
            x = 0.0;
            y += row + GAP;
            row = 0.0;
        }
        // a new page when the room has no row left, or the page is full
        if page_len > 0 && (y + size.y > room.y || (per_page > 0 && page_len >= per_page)) {
            pages.push(Page::default());
            x = 0.0;
            y = 0.0;
            row = 0.0;
        }
        let rect = Rect::from_min_size(pos2(x, y), size);
        if let Some(page) = pages.last_mut() {
            page.tiles.push((index, rect));
        }
        x += size.x + GAP;
        row = row.max(size.y);
    }
    pages
}

/// The page that holds the tile at `index`, or the last page.
pub fn page_of(pages: &[Page], index: usize) -> usize {
    pages
        .iter()
        .position(|p| p.span().contains(&index))
        .unwrap_or(pages.len().saturating_sub(1))
}

/// A tile found by the search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub board: u32,
    pub tile: u32,
}

/// The tiles whose label or item name holds `query` (letter case aside),
/// on the board `current` only when `current_only`, else on every board,
/// the current one first. `name` gives an item's name in the inventory
/// (None when it is not there). An empty query finds nothing.
pub fn search(account: &Account, query: &str, current: Option<u32>, current_only: bool, name: impl Fn(Uuid) -> Option<String>) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let boards = account
        .boards
        .iter()
        .filter(|b| Some(b.id) == current)
        .chain(account.boards.iter().filter(|b| !current_only && Some(b.id) != current));
    let mut hits = Vec::new();
    for board in boards {
        for tile in &board.tiles {
            let in_label = tile.label.as_ref().is_some_and(|l| l.to_lowercase().contains(&query));
            if in_label || name(tile.item).is_some_and(|n| n.to_lowercase().contains(&query)) {
                hits.push(Hit {
                    board: board.id,
                    tile: tile.id,
                });
            }
        }
    }
    hits
}
