//! The gesture boards of the account logged in
//! (`settings.folderstorm_gesture_boards`, row 3.1).
//!
//! Every tile is an item of that account's Inventory: the boards always go
//! back under the account the backup was made by, never under another one,
//! so an import into another account stores them for their own account
//! (shown when it logs in here), or leaves them out.

use super::super::section::{Env, Exported, How, Mode, Outcome, Section, Target};
use super::same_text;
use crate::folderstorm::gesture_boards::store::{Account, MAX_BOARDS};
use aurora_i18n::tr;
use serde_json::Value;

pub struct Boards;

impl Section for Boards {
    fn id(&self) -> &'static str {
        "gesture_boards"
    }

    fn label(&self) -> String {
        tr!("fs-backup-section-gesture-boards").to_owned()
    }

    fn export(&self, env: &Env) -> Option<Exported> {
        let account = env.account?;
        let boards = env
            .settings
            .folderstorm_gesture_boards
            .accounts
            .get(&account)
            .cloned()
            .unwrap_or_default();
        Some(Exported::value(serde_json::to_value(boards).ok()?))
    }

    fn import(&self, target: &mut Target, value: Value, how: &How) -> Result<Outcome, String> {
        let Some(owner) = how.source else {
            return Err(tr!("fs-backup-no-account").to_owned());
        };
        let mut out = Outcome::default();
        if how.drops(owner) {
            out.note(tr!("fs-backup-note-boards-inventory"));
            return Ok(out);
        }
        if !value.is_object() {
            return Err(tr!("fs-backup-unreadable").to_owned());
        }
        let incoming: Account = serde_json::from_value(value).map_err(|_| tr!("fs-backup-unreadable").to_owned())?;
        let accounts = &mut target.settings.folderstorm_gesture_boards.accounts;
        match how.mode {
            Mode::Replace => {
                accounts.insert(owner, incoming);
            }
            Mode::Merge => {
                let mine = accounts.entry(owner).or_default();
                let left = merge(mine, incoming);
                if left > 0 {
                    out.note(tr!("fs-backup-note-limit", count = left));
                }
            }
        }
        Ok(out)
    }

    fn inventory_refs(&self, value: &Value) -> usize {
        super::entries(value.get("boards"))
            .iter()
            .map(|board| board.get("tiles").and_then(Value::as_array).map_or(0, Vec::len))
            .sum()
    }
}

/// Adds the boards of `incoming` to `mine`: a board of the same name takes
/// the place (and the id) of the one there, the others go at the end, as
/// long as there is room. Every tile taken gets a new id, unique among the
/// account's tiles. How many boards were left out.
pub fn merge(mine: &mut Account, incoming: Account) -> usize {
    let mut next_board = mine.boards.iter().map(|b| b.id).max().unwrap_or(0);
    let mut next_tile = mine
        .boards
        .iter()
        .flat_map(|b| b.tiles.iter().map(|t| t.id))
        .max()
        .unwrap_or(0);
    let mut left = 0;
    for mut board in incoming.boards {
        let at = mine.boards.iter().position(|b| same_text(&b.name, &board.name));
        if at.is_none() && mine.boards.len() >= MAX_BOARDS {
            left += 1;
            continue;
        }
        for tile in &mut board.tiles {
            next_tile = next_tile.saturating_add(1);
            tile.id = next_tile;
        }
        match at {
            Some(at) => {
                board.id = mine.boards[at].id;
                mine.boards[at] = board;
            }
            None => {
                next_board = next_board.saturating_add(1);
                board.id = next_board;
                mine.boards.push(board);
            }
        }
    }
    mine.kept.extend(incoming.kept);
    left
}
