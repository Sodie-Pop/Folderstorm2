//! The gesture boards as `settings.json` keeps them (field
//! `folderstorm_gesture_boards`), per account, their limits and the
//! operations of the window. Pure: nothing here touches egui, the
//! inventory or the network (folderstorm/plans/gesture-boards.md, "Data
//! model").
//!
//! A tile keeps an inventory item id, never an asset id nor anything read
//! from the asset: what it is (a gesture, a sound) and its name are read
//! from the inventory when it is drawn and when it is played.
//!
//! Reading never fails and never loses what it cannot use: a board, or a
//! tile, that cannot be read (a newer version's, a damaged file) is kept as
//! it was read ([`Account::kept`], [`Board::kept`]) and written back
//! unchanged; values out of their range are brought back into it.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// At most this many boards for an account.
pub const MAX_BOARDS: usize = 32;
/// At most this many tiles on a board (Folderstorm (C++)'s limit).
pub const MAX_TILES: usize = 1024;
/// A board's name and a tile's label have at most this many characters.
pub const MAX_NAME: usize = 64;
/// A tile's width, in points.
pub const WIDTHS: std::ops::RangeInclusive<f32> = 48.0..=480.0;
/// A tile's height, in points.
pub const HEIGHTS: std::ops::RangeInclusive<f32> = 24.0..=240.0;
/// The size of every tile of a new board (Folderstorm (C++)'s compact
/// tiles were 100 × 28; a little wider for the speaker of a sound).
pub const DEFAULT_TILE: [f32; 2] = [112.0, 30.0];
/// "Buttons per page" other than automatic: within this range.
pub const PER_PAGE: std::ops::RangeInclusive<u16> = 4..=200;

/// Every account's boards.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub accounts: BTreeMap<Uuid, Account>,
}

/// One account's boards.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Account {
    /// In the user's order.
    pub boards: Vec<Board>,
    /// The board shown (its id); the first one when unset or gone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<u32>,
    /// Search the board shown only, instead of all the boards.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub search_current: bool,
    /// Boards this version could not read, written back as they were.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<serde_json::Value>,
}

/// One board.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Board {
    /// Made by the account, kept through renames.
    pub id: u32,
    pub name: String,
    /// In the user's order.
    pub tiles: Vec<Tile>,
    /// The size of its tiles that have none of their own, in points.
    pub tile: [f32; 2],
    /// A tile is as wide as its full label (never wider than the board).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub full_names: bool,
    /// At most this many tiles on a page; 0: as many as the window holds.
    #[serde(skip_serializing_if = "is_zero")]
    pub per_page: u16,
    /// Tiles this version could not read, written back as they were.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<serde_json::Value>,
}

/// One tile: an inventory item and how it looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    /// Made by the account, unique among all its boards' tiles.
    pub id: u32,
    /// The inventory item (a gesture, a sound, or a link to one).
    pub item: Uuid,
    /// Shown instead of the item's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Its face, instead of the theme's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// Its own size, instead of the board's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
}

fn is_zero(n: &u16) -> bool {
    *n == 0
}

/// Why an operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The name is empty once trimmed.
    EmptyName,
    /// Already as many boards as allowed.
    TooManyBoards,
    /// Already as many tiles as allowed on that board.
    TooManyTiles,
    /// No such board or tile.
    Missing,
}

impl Error {
    /// What the window says about it.
    pub fn text(self) -> String {
        match self {
            Error::EmptyName => aurora_i18n::tr!("fs-gesture-board-error-name").to_owned(),
            Error::TooManyBoards => aurora_i18n::tr!("fs-gesture-board-error-boards", count = MAX_BOARDS),
            Error::TooManyTiles => aurora_i18n::tr!("fs-gesture-board-error-tiles", count = MAX_TILES),
            Error::Missing => aurora_i18n::tr!("fs-gesture-board-error-missing").to_owned(),
        }
    }
}

/// A name or label as kept: trimmed, cut at [`MAX_NAME`] characters, without
/// line breaks or other control characters.
pub fn clean_name(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// A size brought into the tiles' range (a hand-edited file).
pub fn clamp_size([w, h]: [f32; 2]) -> [f32; 2] {
    let fix = |v: f32, range: &std::ops::RangeInclusive<f32>, usual: f32| {
        if v.is_finite() {
            v.clamp(*range.start(), *range.end())
        } else {
            usual
        }
    };
    [fix(w, &WIDTHS, DEFAULT_TILE[0]), fix(h, &HEIGHTS, DEFAULT_TILE[1])]
}

/// "Buttons per page" brought into its range (0 stays automatic).
pub fn clamp_per_page(n: u16) -> u16 {
    if n == 0 { 0 } else { n.clamp(*PER_PAGE.start(), *PER_PAGE.end()) }
}

impl Tile {
    fn normalized(mut self) -> Self {
        self.label = self.label.map(|l| clean_name(&l)).filter(|l| !l.is_empty());
        self.size = self.size.map(clamp_size);
        self
    }

    /// The tile's size on `board`.
    pub fn size_on(&self, board: &Board) -> [f32; 2] {
        self.size.unwrap_or(board.tile)
    }
}

impl Board {
    /// A new, empty board.
    pub fn new(id: u32, name: &str) -> Self {
        Self {
            id,
            name: clean_name(name),
            tiles: Vec::new(),
            tile: DEFAULT_TILE,
            full_names: false,
            per_page: 0,
            kept: Vec::new(),
        }
    }

    pub fn tile(&self, id: u32) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.id == id)
    }

    pub fn tile_mut(&mut self, id: u32) -> Option<&mut Tile> {
        self.tiles.iter_mut().find(|t| t.id == id)
    }

    /// Where the tile `id` is in the board's order.
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.tiles.iter().position(|t| t.id == id)
    }

    /// The tile of `item`, if the board has it.
    pub fn tile_of(&self, item: Uuid) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.item == item)
    }

    /// Moves the tile at `from` so that it comes before the tile that was
    /// at `to` (`to` = the number of tiles: at the end). False when nothing
    /// moved.
    pub fn move_tile(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tiles.len() || to > self.tiles.len() || to == from || to == from + 1 {
            return false;
        }
        let tile = self.tiles.remove(from);
        let to = if to > from { to - 1 } else { to };
        self.tiles.insert(to, tile);
        true
    }

    /// Every tile takes the board's size again.
    pub fn same_size_for_all(&mut self) -> bool {
        let mut changed = false;
        for tile in &mut self.tiles {
            changed |= tile.size.take().is_some();
        }
        changed
    }
}

/// What [`Account::add_items`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Added {
    /// The new tiles' ids, in order.
    pub added: Vec<u32>,
    /// Items the board had already: their tiles' ids.
    pub existing: Vec<u32>,
    /// Items left out because the board is full.
    pub refused: usize,
}

impl Account {
    /// The board shown: the selected one, else the first.
    pub fn current(&self) -> Option<&Board> {
        self.selected.and_then(|id| self.board(id)).or_else(|| self.boards.first())
    }

    /// The id of the board shown.
    pub fn current_id(&self) -> Option<u32> {
        self.current().map(|b| b.id)
    }

    pub fn board(&self, id: u32) -> Option<&Board> {
        self.boards.iter().find(|b| b.id == id)
    }

    pub fn board_mut(&mut self, id: u32) -> Option<&mut Board> {
        self.boards.iter_mut().find(|b| b.id == id)
    }

    fn next_board_id(&self) -> u32 {
        self.boards.iter().map(|b| b.id).max().map_or(1, |m| m.saturating_add(1))
    }

    fn next_tile_id(&self) -> u32 {
        self.boards
            .iter()
            .flat_map(|b| b.tiles.iter().map(|t| t.id))
            .max()
            .map_or(1, |m| m.saturating_add(1))
    }

    /// A new, empty board at the end, selected.
    pub fn new_board(&mut self, name: &str) -> Result<u32, Error> {
        let name = clean_name(name);
        if name.is_empty() {
            return Err(Error::EmptyName);
        }
        if self.boards.len() >= MAX_BOARDS {
            return Err(Error::TooManyBoards);
        }
        let id = self.next_board_id();
        self.boards.push(Board::new(id, &name));
        self.selected = Some(id);
        Ok(id)
    }

    /// Renames a board; its id, and so its selection, stay.
    pub fn rename_board(&mut self, id: u32, name: &str) -> Result<(), Error> {
        let name = clean_name(name);
        if name.is_empty() {
            return Err(Error::EmptyName);
        }
        let board = self.board_mut(id).ok_or(Error::Missing)?;
        board.name = name;
        Ok(())
    }

    /// A copy of a board, with new ids, right after it, selected. Its
    /// tiles keep their items, labels, colours, sizes and order.
    pub fn duplicate_board(&mut self, id: u32, name: &str) -> Result<u32, Error> {
        let name = clean_name(name);
        if name.is_empty() {
            return Err(Error::EmptyName);
        }
        if self.boards.len() >= MAX_BOARDS {
            return Err(Error::TooManyBoards);
        }
        let at = self.boards.iter().position(|b| b.id == id).ok_or(Error::Missing)?;
        let mut copy = self.boards[at].clone();
        copy.id = self.next_board_id();
        copy.name = name;
        let first = self.next_tile_id();
        for (n, tile) in copy.tiles.iter_mut().enumerate() {
            tile.id = first.saturating_add(n as u32);
        }
        let new = copy.id;
        self.boards.insert(at + 1, copy);
        self.selected = Some(new);
        Ok(new)
    }

    /// Deletes a board (only its tiles: the inventory stays as it is). The
    /// board next to it is shown.
    pub fn delete_board(&mut self, id: u32) -> Result<(), Error> {
        let at = self.boards.iter().position(|b| b.id == id).ok_or(Error::Missing)?;
        self.boards.remove(at);
        if self.selected == Some(id) {
            self.selected = self.boards.get(at).or_else(|| self.boards.last()).map(|b| b.id);
        }
        Ok(())
    }

    /// Moves a board one place earlier (`-1`) or later (`1`).
    pub fn move_board(&mut self, id: u32, step: isize) -> bool {
        let Some(at) = self.boards.iter().position(|b| b.id == id) else {
            return false;
        };
        let to = at as isize + step;
        if to < 0 || to as usize >= self.boards.len() {
            return false;
        }
        self.boards.swap(at, to as usize);
        true
    }

    /// Adds tiles for `items` to the board `board`, before the tile at
    /// `at` (None: at the end). An item the board has already is not added
    /// again (Folderstorm (C++): its tile is shown instead); one the board
    /// has no room for is refused.
    pub fn add_items(&mut self, board: u32, items: &[Uuid], at: Option<usize>) -> Result<Added, Error> {
        let mut next = self.next_tile_id();
        let board = self.board_mut(board).ok_or(Error::Missing)?;
        let mut out = Added::default();
        let mut at = at.unwrap_or(board.tiles.len()).min(board.tiles.len());
        for item in items {
            if item.is_nil() {
                continue;
            }
            if let Some(tile) = board.tile_of(*item) {
                if !out.existing.contains(&tile.id) && !out.added.contains(&tile.id) {
                    out.existing.push(tile.id);
                }
                continue;
            }
            if board.tiles.len() >= MAX_TILES {
                out.refused += 1;
                continue;
            }
            board.tiles.insert(
                at,
                Tile {
                    id: next,
                    item: *item,
                    label: None,
                    color: None,
                    size: None,
                },
            );
            out.added.push(next);
            next = next.saturating_add(1);
            at += 1;
        }
        if out.added.is_empty() && out.existing.is_empty() && out.refused > 0 {
            return Err(Error::TooManyTiles);
        }
        Ok(out)
    }

    /// Removes a tile (never its inventory item).
    pub fn remove_tile(&mut self, board: u32, tile: u32) -> Result<(), Error> {
        let board = self.board_mut(board).ok_or(Error::Missing)?;
        let at = board.index_of(tile).ok_or(Error::Missing)?;
        board.tiles.remove(at);
        Ok(())
    }
}

// ---- reading -----------------------------------------------------------

/// The account as the file has it, before each board is read on its own.
#[derive(Default, Deserialize)]
#[serde(default)]
struct RawAccount {
    boards: Vec<serde_json::Value>,
    selected: Option<u32>,
    search_current: bool,
    kept: Vec<serde_json::Value>,
}

/// A board as the file has it, before each tile is read on its own.
#[derive(Deserialize)]
struct RawBoard {
    id: u32,
    name: String,
    #[serde(default)]
    tiles: Vec<serde_json::Value>,
    #[serde(default = "default_tile")]
    tile: [f32; 2],
    #[serde(default)]
    full_names: bool,
    #[serde(default)]
    per_page: u16,
    #[serde(default)]
    kept: Vec<serde_json::Value>,
}

fn default_tile() -> [f32; 2] {
    DEFAULT_TILE
}

impl<'de> Deserialize<'de> for Account {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawAccount::deserialize(deserializer)?;
        Ok(Account::from_raw(raw))
    }
}

impl Account {
    fn from_raw(raw: RawAccount) -> Self {
        let mut account = Account {
            boards: Vec::new(),
            selected: raw.selected,
            search_current: raw.search_current,
            kept: raw.kept,
        };
        let mut tile_ids = std::collections::BTreeSet::new();
        for value in raw.boards {
            let usable = account.boards.len() < MAX_BOARDS;
            match serde_json::from_value::<RawBoard>(value.clone()) {
                Ok(board) if usable && account.board(board.id).is_none() && !clean_name(&board.name).is_empty() => {
                    account.boards.push(Board::from_raw(board, &mut tile_ids));
                }
                _ => account.kept.push(value),
            }
        }
        account
    }
}

impl Board {
    fn from_raw(raw: RawBoard, tile_ids: &mut std::collections::BTreeSet<u32>) -> Self {
        let mut board = Board {
            id: raw.id,
            name: clean_name(&raw.name),
            tiles: Vec::new(),
            tile: clamp_size(raw.tile),
            full_names: raw.full_names,
            per_page: clamp_per_page(raw.per_page),
            kept: raw.kept,
        };
        for value in raw.tiles {
            let usable = board.tiles.len() < MAX_TILES;
            match serde_json::from_value::<Tile>(value.clone()) {
                Ok(tile) if usable && !tile.item.is_nil() && board.tile_of(tile.item).is_none() && tile_ids.insert(tile.id) => {
                    board.tiles.push(tile.normalized());
                }
                _ => board.kept.push(value),
            }
        }
        board
    }
}
