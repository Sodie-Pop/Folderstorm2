//! Folderstorm: gesture boards in the app (the rules are in
//! `crate::folderstorm::gesture_boards`; folderstorm/plans/gesture-boards.md).
//!
//! Draws the board window with the account's boards, the inventory and the
//! gestures' state, and plays what is clicked through Aurora's own paths:
//! a gesture as the Gestures window's Play does (`gestures::Action::Play`:
//! activated, loaded, played, or stopped when it is playing; RLVa
//! `@sendgesture` checked in `gesture_play`), a sound as the inventory's
//! double-click does (`InvAction::TriggerSound`: `send_sound_trigger`, heard
//! in the world, as LLSoundBridge's "sound_play").

use super::*;
use crate::folderstorm::gesture_boards::{self as boards, Facts, Kind, Request};
use std::collections::BTreeSet;
use uuid::Uuid;

/// What the app keeps for the boards between frames (egui's memory).
#[derive(Clone, Default)]
struct Glue {
    /// Items asked from the server once in this session.
    fetched: BTreeSet<Uuid>,
    /// The inventory generation last looked at for missing items.
    generation: Option<u64>,
    /// The boards changed and are not saved yet.
    dirty: bool,
    /// The demo was seeded.
    seeded: bool,
}

fn glue_id() -> egui::Id {
    egui::Id::unique("fs_gesture_board_glue")
}

impl App {
    /// In the interface's pass, after the Gestures window.
    pub(super) fn folderstorm_gesture_boards(&mut self, ctx: &egui::Context, p: &crate::theme::Palette) {
        let mut glue: Glue = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Glue>(glue_id())));
        self.gesture_boards_demo(ctx, &mut glue);
        if boards::is_open(ctx) {
            self.gesture_boards_frame(ctx, p, &mut glue);
        }
        // saved once the pointer is up (a slider being dragged changes the
        // boards at every frame), as the inventory's preferences are
        if glue.dirty && !ctx.input(|i| i.pointer.any_down()) {
            glue.dirty = false;
            self.settings.save();
        }
        ctx.data_mut(|d| d.insert_temp(glue_id(), glue));
    }

    fn gesture_boards_frame(&mut self, ctx: &egui::Context, p: &crate::theme::Palette, glue: &mut Glue) {
        let agent = self.world.agent_id;
        let blocked = if self.world.rlva.has("sendgesture") {
            Some(aurora_i18n::tr!("fs-gesture-board-blocked-rlva"))
        } else if !self.gesture_state.manager.enabled {
            Some(aurora_i18n::tr!("fs-gesture-board-blocked-off"))
        } else {
            None
        };
        let manager = &self.gesture_state.manager;
        let playing = |id: Uuid| manager.is_playing(id);
        let facts = Facts {
            inv: &self.world.inventory,
            playing: &playing,
            gestures_blocked: blocked,
        };
        let account = self.settings.folderstorm_gesture_boards.accounts.entry(agent).or_default();
        let before = account.clone();
        let requests = boards::show(ctx, p, account, &facts);
        if *account != before {
            glue.dirty = true;
        }
        // items of the boards the inventory does not hold (yet): asked
        // once, as the gestures' code asks for the Current Outfit's
        let generation = self.world.inventory.generation;
        if !self.demo && glue.generation != Some(generation) {
            glue.generation = Some(generation);
            let missing: Vec<Uuid> = account
                .boards
                .iter()
                .flat_map(|b| b.tiles.iter().map(|t| t.item))
                .filter(|id| !self.world.inventory.items.contains_key(id))
                .filter(|id| glue.fetched.insert(*id))
                .collect();
            for items in missing.chunks(100) {
                self.send(NetCommand::FetchItems {
                    items: items.to_vec(),
                    owner: agent,
                });
            }
        }
        for request in requests {
            self.gesture_board_request(request);
        }
    }

    fn gesture_board_request(&mut self, request: Request) {
        match request {
            Request::Play {
                original,
                kind: Kind::Gesture,
            } => self.gesture_action(crate::ui::gestures::Action::Play(original)),
            Request::Play { original, kind: Kind::Sound } => {
                // send_sound_trigger sends nothing for a null sound
                if let Some(asset) = self
                    .world
                    .inventory
                    .items
                    .get(&original)
                    .filter(|it| it.asset_type == 1 && !it.asset_id.is_nil())
                    .map(|it| it.asset_id)
                {
                    self.apply_inventory_action(crate::ui::inventory::InvAction::TriggerSound(asset));
                }
            }
            Request::ShowInventory(id) => {
                self.inventory_ui.show_original(&self.world.inventory, id);
                self.panels.inventory = true;
            }
        }
    }

    /// `AURORA_DEMO_GESTURE_BOARDS=<mode>` (docs/TESTING.md): the demo's
    /// inventory gets a few gestures and sounds, the account two boards
    /// ("Greetings": gestures and sounds, labels, colours, a tile of its
    /// own size and one whose item is gone; "Party": sounds), and the window
    /// opens. `1` only that; `full` turns "Show full names" on; `pages`
    /// makes the window small, so that the board has pages; `search` opens
    /// the search on all the boards with "wave" typed; `play` clicks a
    /// gesture's tile and a sound's tile at frame 300 (the log says "demo
    /// sound trigger").
    fn gesture_boards_demo(&mut self, ctx: &egui::Context, glue: &mut Glue) {
        if !self.demo || !matches!(self.screen, Screen::World) {
            return;
        }
        let Ok(mode) = std::env::var("AURORA_DEMO_GESTURE_BOARDS") else {
            return;
        };
        if !glue.seeded && self.frame_count >= 30 {
            glue.seeded = true;
            let agent = self.world.agent_id;
            let (sound, clip) = crate::demo::chime();
            self.scene.sounds.insert(sound, clip);
            let items = demo_items(&mut self.world.inventory, agent, sound);
            for (item, definition) in items {
                if let Ok(data) = definition.encode() {
                    self.demo_documents.insert(item, data);
                }
            }
            let account = self.settings.folderstorm_gesture_boards.accounts.entry(agent).or_default();
            if account.boards.is_empty() {
                *account = demo_account(mode == "full");
            }
            if mode == "search" {
                account.search_current = false;
            }
            let area = crate::ui::layout::work_area(ctx);
            let size = if mode == "pages" { egui::vec2(300.0, 150.0) } else { egui::vec2(380.0, 230.0) };
            ui::widgets::place_floater(ctx, boards::WINDOW, egui::pos2(area.left() + 380.0, area.top() + 120.0), size);
            boards::open(ctx);
        }
        if mode == "search" && self.frame_count == 120 {
            boards::demo_search(ctx, "wave");
        }
        if mode == "play" && self.frame_count == 300 {
            for (item, kind) in [(DEMO_WAVE, Kind::Gesture), (DEMO_CHIME, Kind::Sound)] {
                log::info!("demo gesture board: play {kind:?}");
                self.gesture_board_request(Request::Play { original: item, kind });
            }
        }
    }
}

/// The demo's gestures and sounds (item ids).
const DEMO_WAVE: Uuid = Uuid::from_u128(960_001);
const DEMO_CHIME: Uuid = Uuid::from_u128(960_101);
const DEMO_GONE: Uuid = Uuid::from_u128(960_999);

/// Gestures and sounds in the demo's inventory (in its test folder), and
/// the gestures' definitions: each one says something in the chat.
fn demo_items(
    inv: &mut crate::world::inventory::Inventory,
    agent: Uuid,
    sound: Uuid,
) -> Vec<(Uuid, aurora_assets::gesture::Gesture)> {
    use aurora_assets::gesture::{Gesture, Step};
    if inv.root.is_nil() || !inv.items.contains_key(&Uuid::from_u128(8106)) {
        crate::world::inventory::demo::seed(inv, agent);
    }
    let parent = Uuid::from_u128(8000);
    let gestures = ["Wave hello", "Bow", "Clap clap clap", "Laugh out loud", "Thumbs up", "Shrug", "Dance a little jig with everyone around", "Wave goodbye"];
    let sounds = ["Chime", "Air horn", "Applause", "Drum roll please", "Crickets"];
    let mut out = Vec::new();
    let mut items = Vec::new();
    for (n, name) in gestures.iter().enumerate() {
        let id = Uuid::from_u128(960_001 + n as u128);
        items.push(aurora_net::inventory::InvItem {
            id,
            parent,
            name: (*name).into(),
            asset_type: 21,
            inv_type: 20,
            asset_id: Uuid::from_u128(961_001 + n as u128),
            creator: agent,
            owner: agent,
            base_mask: 0x7fff_ffff,
            owner_mask: 0x7fff_ffff,
            ..Default::default()
        });
        out.push((
            id,
            Gesture {
                steps: vec![Step::Chat {
                    text: format!("*{name}*"),
                    flags: 0,
                }],
                ..Default::default()
            },
        ));
    }
    for (n, name) in sounds.iter().enumerate() {
        items.push(aurora_net::inventory::InvItem {
            id: Uuid::from_u128(960_101 + n as u128),
            parent,
            name: (*name).into(),
            asset_type: 1,
            inv_type: 1,
            asset_id: sound,
            creator: agent,
            owner: agent,
            base_mask: 0x7fff_ffff,
            owner_mask: 0x7fff_ffff,
            ..Default::default()
        });
    }
    inv.add_items(items);
    out
}

/// The demo's two boards.
fn demo_account(full_names: bool) -> boards::store::Account {
    use boards::store::Account;
    let mut account = Account::default();
    let gestures: Vec<Uuid> = (0..8).map(|n| Uuid::from_u128(960_001 + n)).collect();
    let sounds: Vec<Uuid> = (0..5).map(|n| Uuid::from_u128(960_101 + n)).collect();
    let Ok(greetings) = account.new_board("Greetings") else {
        return account;
    };
    let mut items = Vec::new();
    for (n, gesture) in gestures.iter().enumerate() {
        items.push(*gesture);
        if let Some(sound) = sounds.get(n) {
            items.push(*sound);
        }
    }
    items.push(DEMO_GONE);
    let _ = account.add_items(greetings, &items, None);
    if let Some(board) = account.board_mut(greetings) {
        board.full_names = full_names;
        for tile in &mut board.tiles {
            match tile.item {
                id if id == DEMO_WAVE => {
                    tile.label = Some("Hi!".into());
                    tile.color = Some([46, 125, 50]);
                }
                id if id == DEMO_CHIME => tile.color = Some([255, 179, 0]),
                id if id == Uuid::from_u128(960_003) => tile.size = Some([150.0, 44.0]),
                id if id == Uuid::from_u128(960_102) => tile.color = Some([198, 40, 40]),
                _ => {}
            }
        }
    }
    if let Ok(party) = account.new_board("Party") {
        let _ = account.add_items(party, &[sounds[2], sounds[1], gestures[6], gestures[7]], None);
    }
    account.selected = Some(greetings);
    account
}
