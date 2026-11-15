//! The board window drawn frame by frame, with the software painter:
//! what it shows, what a click plays, the pages, full names, the search
//! (Ctrl+F) on all boards or one, an inventory drop and the editor.
//! `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps the pictures
//! (`gesture-board*.png`).

use super::super::dialogs::Dialog;
use super::super::store::Account;
use super::super::window::{View, view_id};
use super::super::{self as boards, Facts, Kind, Request};
use super::{id, inventory, item};
use crate::world::inventory::Inventory;
use aurora_i18n::tr;
use egui::{Pos2, Rect, pos2, vec2};
use uuid::Uuid;

const WIDTH: u32 = 720;
const HEIGHT: u32 = 460;

struct Bench {
    ctx: egui::Context,
    p: crate::theme::Palette,
    _icons: crate::ui::icons::Icons,
    account: Account,
    inv: Inventory,
    blocked: Option<&'static str>,
    time: f64,
    texts: Vec<(String, Rect, bool)>,
    out: Vec<Request>,
    pixels: Vec<u32>,
    painter: crate::crash::software::Painter,
    pointer: Pos2,
}

/// Gestures 200.., sounds 300.., one long name, in the test folder.
fn seeded() -> (Inventory, Account) {
    let mut inv = inventory();
    let mine = id(8000);
    let names = [
        "Bow",
        "Clap clap clap",
        "Dance a little jig with everyone around",
        "Laugh",
        "Shrug",
        "Thumbs up",
        "Wave goodbye",
    ];
    let sounds = ["Chime", "Applause", "Drum roll please", "Crickets"];
    let mut items = Vec::new();
    for (n, name) in names.iter().enumerate() {
        items.push(item(200 + n as u128, mine, name, 21, id(1200 + n as u128)));
    }
    for (n, name) in sounds.iter().enumerate() {
        items.push(item(300 + n as u128, mine, name, 1, id(1300 + n as u128)));
    }
    inv.add_items(items);
    let mut account = Account::default();
    let greetings = account.new_board("Greetings").expect("board");
    let mut tiles = vec![id(100), id(101)];
    tiles.extend((200..207).map(id));
    tiles.extend((300..304).map(id));
    tiles.push(id(999)); // gone
    account.add_items(greetings, &tiles, None).expect("added");
    {
        let board = account.board_mut(greetings).expect("board");
        board.tiles[0].label = Some("Hi!".into());
        board.tiles[0].color = Some([46, 125, 50]);
        board.tiles[1].color = Some([255, 179, 0]);
        board.tiles[3].size = Some([150.0, 44.0]);
    }
    let party = account.new_board("Party").expect("board");
    account.add_items(party, &[id(301), id(206), id(101)], None).expect("added");
    account.selected = Some(greetings);
    (inv, account)
}

impl Bench {
    fn new(size: egui::Vec2) -> Self {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let icons = crate::ui::icons::Icons::load(&ctx, None);
        let (inv, account) = seeded();
        let mut bench = Bench {
            ctx,
            p,
            _icons: icons,
            account,
            inv,
            blocked: None,
            time: 0.0,
            texts: Vec::new(),
            out: Vec::new(),
            pixels: vec![0; (WIDTH * HEIGHT) as usize],
            painter: Default::default(),
            pointer: pos2(5.0, 5.0),
        };
        crate::ui::widgets::place_floater(&bench.ctx, boards::WINDOW, pos2(40.0, 40.0), size);
        boards::open(&bench.ctx);
        bench.frames(4);
        bench
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(WIDTH as f32, HEIGHT as f32));
        self.time += 1.0 / 60.0;
        let blocked = self.blocked;
        let inv = &self.inv;
        let account = &mut self.account;
        let p = self.p;
        let mut out = Vec::new();
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(self.time),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx();
                crate::ui::layout::set_work_area(ctx, screen);
                crate::ui::layout::set_move_area(ctx, screen);
                let playing = |_: Uuid| false;
                let facts = Facts {
                    inv,
                    playing: &playing,
                    gestures_blocked: blocked,
                };
                out = boards::show(ctx, &p, account, &facts);
            },
        );
        self.out.extend(out);
        self.texts.clear();
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect, bool)>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                egui::Shape::Text(t) => out.push((
                    t.galley.text().to_owned(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    t.galley.elided,
                )),
                _ => {}
            }
        }
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut self.texts);
        }
        let shapes = std::mem::take(&mut output.shapes);
        let primitives = self.ctx.tessellate(shapes, output.pixels_per_point);
        self.pixels.fill(0);
        self.painter.paint(
            &mut self.pixels,
            [WIDTH as usize, HEIGHT as usize],
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
            self.p.navy,
        );
        output.textures_delta.clear();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![egui::Event::PointerMoved(self.pointer)]);
        }
    }

    /// Drawn whole (not cut short).
    fn shown(&self, text: &str) -> bool {
        self.texts.iter().any(|(t, _, elided)| t == text && !elided)
    }

    /// Drawn cut short.
    fn cut(&self, text: &str) -> bool {
        self.texts.iter().any(|(t, _, elided)| t == text && *elided)
    }

    fn rect(&self, text: &str) -> Rect {
        self.texts
            .iter()
            .find(|(t, _, _)| t == text)
            .map(|(_, r, _)| *r)
            .unwrap_or_else(|| {
                panic!(
                    "“{text}” is drawn; drawn: {:?}",
                    self.texts.iter().map(|(t, _, _)| t).collect::<Vec<_>>()
                )
            })
    }

    fn at(&self, text: &str) -> Pos2 {
        self.rect(text).center()
    }

    fn click_with(&mut self, at: Pos2, button: egui::PointerButton) {
        self.pointer = at;
        self.frame(vec![egui::Event::PointerMoved(at)]);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![press(true)]);
        self.frame(vec![press(false)]);
        self.frames(2);
    }

    fn click(&mut self, at: Pos2) {
        self.click_with(at, egui::PointerButton::Primary);
    }

    fn key(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![egui::Event::PointerMoved(self.pointer), event(true)]);
        self.frame(vec![event(false)]);
        self.frames(2);
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![egui::Event::Text(text.to_owned())]);
        self.frames(2);
    }

    fn capture(&self, name: &str) {
        if let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") {
            let rgba: Vec<_> = self
                .pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            image::save_buffer(path, &rgba, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("capture written");
        }
    }

    fn current(&self) -> &super::super::store::Board {
        self.account.current().expect("a board")
    }
}

/// The board as it opens: its name, its tiles (labels, the item's name
/// when there is none, a long name cut short, the gone item), and pages.
#[test]
fn the_board_is_drawn() {
    let mut bench = Bench::new(vec2(380.0, 230.0));
    bench.capture("gesture-board");
    for text in [
        tr!("fs-gesture-board-title"),
        "Greetings",
        "Hi!",
        "Air horn",
        "Bow",
        "Clap clap clap",
    ] {
        assert!(bench.shown(text), "“{text}” is drawn");
    }
    // the same size for all: the long name is cut short
    assert!(bench.cut("Dance a little jig with everyone around"));
    // 13 tiles do not fit: pages
    assert!(bench.shown(&tr!("fs-gesture-board-page", page = 1, count = 2)));
    assert!(!bench.shown(tr!("fs-gesture-board-unavailable")), "the gone item is on page 2");
    // the next page
    let label = bench.rect(&tr!("fs-gesture-board-page", page = 1, count = 2));
    let next = pos2(label.right() + 14.0, label.center().y);
    bench.click(next);
    assert!(bench.shown(&tr!("fs-gesture-board-page", page = 2, count = 2)), "page 2");
    assert!(bench.shown(tr!("fs-gesture-board-unavailable")));
    bench.capture("gesture-board-page-2");
    // a wider window holds the board on one page
    crate::ui::widgets::place_floater(&bench.ctx, boards::WINDOW, pos2(40.0, 40.0), vec2(600.0, 300.0));
    bench.frames(4);
    assert!(!bench.texts.iter().any(|(t, _, _)| t.contains(" / ")), "no pages row");
}

/// A click plays a gesture or a sound; a gone item plays nothing; a
/// gesture blocked (RLVa) plays nothing, a sound still does.
#[test]
fn a_click_plays_its_gesture_or_sound() {
    let mut bench = Bench::new(vec2(600.0, 300.0));
    let hi = bench.at("Hi!");
    bench.click(hi);
    assert_eq!(
        bench.out,
        [Request::Play {
            original: id(100),
            kind: Kind::Gesture
        }]
    );
    bench.out.clear();
    let horn = bench.at("Air horn");
    bench.click(horn);
    assert_eq!(
        bench.out,
        [Request::Play {
            original: id(101),
            kind: Kind::Sound
        }]
    );
    bench.out.clear();
    let gone = bench.at(tr!("fs-gesture-board-unavailable"));
    bench.click(gone);
    assert!(bench.out.is_empty(), "a gone item plays nothing");
    bench.blocked = Some(tr!("fs-gesture-board-blocked-rlva"));
    bench.frames(2);
    bench.click(hi);
    assert!(bench.out.is_empty(), "a blocked gesture plays nothing");
    bench.click(horn);
    assert_eq!(
        bench.out,
        [Request::Play {
            original: id(101),
            kind: Kind::Sound
        }]
    );
}

/// "Show full names": a tile as wide as its name, never wider than the
/// board.
#[test]
fn full_names_widen_the_tiles() {
    let mut bench = Bench::new(vec2(600.0, 300.0));
    assert!(bench.cut("Dance a little jig with everyone around"));
    let board = bench.account.current_id().expect("board");
    bench.account.board_mut(board).expect("board").full_names = true;
    bench.frames(3);
    bench.capture("gesture-board-full-names");
    assert!(bench.shown("Dance a little jig with everyone around"), "the whole name");
    // a board narrower than the name: cut to the board
    crate::ui::widgets::place_floater(&bench.ctx, boards::WINDOW, pos2(40.0, 40.0), vec2(230.0, 300.0));
    bench.frames(4);
    bench.capture("gesture-board-full-names-narrow");
    assert!(!bench.shown("Dance a little jig with everyone around"));
}

/// Ctrl+F searches all the boards; "This board only" keeps the board
/// shown; Esc goes back to the page.
#[test]
fn ctrl_f_searches_all_boards_or_this_one() {
    let mut bench = Bench::new(vec2(420.0, 260.0));
    bench.pointer = pos2(120.0, 140.0);
    bench.frames(2);
    bench.key(egui::Key::F, egui::Modifiers::COMMAND);
    assert!(bench.shown(tr!("fs-gesture-board-search-empty")), "the search is open");
    bench.type_text("a");
    bench.capture("gesture-board-search-typed");
    // "Applause" is on both boards; "Party"'s tiles come after
    bench.type_text("pplause");
    assert!(bench.shown(&tr!("fs-gesture-board-found", count = 2)));
    bench.capture("gesture-board-search");
    let only = bench.rect(tr!("fs-gesture-board-search-current"));
    bench.click(pos2(only.left() - 10.0, only.center().y));
    assert!(bench.account.search_current, "remembered for the account");
    assert!(bench.shown(&tr!("fs-gesture-board-found", count = 1)));
    // a result plays as the tile does
    let applause = bench.at("Applause");
    bench.click(applause);
    assert_eq!(
        bench.out,
        [Request::Play {
            original: id(301),
            kind: Kind::Sound
        }]
    );
    // Esc: back to the board
    bench.ctx.memory_mut(|m| m.request_focus(super::super::window::search_field_id()));
    bench.frames(1);
    bench.key(egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!bench.shown(tr!("fs-gesture-board-search-current")), "the search is closed");
    assert!(bench.shown("Hi!"));
}

/// An inventory gesture or sound dropped on the board is added (once);
/// anything else is refused.
#[test]
fn an_inventory_drop_adds_a_tile() {
    let mut bench = Bench::new(vec2(600.0, 300.0));
    // a new board, empty
    bench.account.new_board("Empty").expect("board");
    bench.frames(2);
    assert!(bench.shown(tr!("fs-gesture-board-empty-tiles")));
    let room = bench.at(tr!("fs-gesture-board-empty-tiles"));
    let drop = |bench: &mut Bench, item: Uuid| {
        egui::DragAndDrop::set_payload(&bench.ctx, crate::ui::inventory::InvDrag(item));
        bench.pointer = room;
        bench.frames(2);
        bench.frame(vec![egui::Event::PointerButton {
            pos: room,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        bench.frames(2);
    };
    drop(&mut bench, id(203));
    assert_eq!(bench.current().tiles.iter().map(|t| t.item).collect::<Vec<_>>(), [id(203)]);
    drop(&mut bench, id(203));
    assert_eq!(bench.current().tiles.len(), 1, "once");
    assert!(bench.shown(tr!("fs-gesture-board-already")));
    drop(&mut bench, id(103));
    assert_eq!(bench.current().tiles.len(), 1, "an object is refused");
    assert!(bench.shown(tr!("fs-gesture-board-drop-refused")));
    drop(&mut bench, id(104));
    assert_eq!(bench.current().tiles.len(), 1, "one in the Trash too");
}

/// Right-click › Edit…: a label and a colour, saved; the inventory item
/// keeps its name.
#[test]
fn a_tile_is_edited() {
    let mut bench = Bench::new(vec2(600.0, 300.0));
    let bow = bench.at("Bow");
    bench.click_with(bow, egui::PointerButton::Secondary);
    let edit = bench.at(tr!("fs-gesture-board-edit"));
    bench.click(edit);
    assert!(bench.shown(tr!("fs-gesture-board-edit-title")));
    bench.ctx.memory_mut(|m| m.surrender_focus(m.focused().unwrap_or(egui::Id::NULL)));
    // the label field: click it, type
    let label_field = bench.at(tr!("fs-gesture-board-label")) + vec2(60.0, 22.0);
    bench.click(label_field);
    bench.type_text("Deep bow");
    // the first swatch
    let theme = bench.rect(tr!("fs-gesture-board-theme-color"));
    bench.click(pos2(theme.right() + 18.0, theme.center().y));
    bench.capture("gesture-board-edit");
    bench.click(bench.at(tr!("common-save")));
    let tile = bench.current().tiles.iter().find(|t| t.item == id(200)).expect("tile");
    assert_eq!(tile.label.as_deref(), Some("Deep bow"));
    assert!(tile.color.is_some());
    assert_eq!(bench.inv.items[&id(200)].name, "Bow", "the item keeps its name");
    assert!(bench.shown("Deep bow"));
}

/// The Add picker lists the user's gestures and sounds (not the Trash's,
/// not the Library's), marks those already on the board, and adds the
/// chosen ones.
#[test]
fn the_picker_adds_gestures_and_sounds() {
    let mut bench = Bench::new(vec2(600.0, 300.0));
    let empty = bench.account.new_board("Empty").expect("board");
    bench.account.add_items(empty, &[id(101)], None).expect("added");
    bench
        .ctx
        .data_mut(|d| d.get_temp_mut_or_default::<View>(view_id()).dialog = Some(Dialog::add()));
    bench.frames(3);
    bench.capture("gesture-board-add");
    for text in ["Air horn", "Applause", "Chime", "Bow"] {
        assert!(bench.shown(text), "“{text}” is listed");
    }
    let listed: Vec<String> = super::super::dialogs::catalogue(&bench.inv)
        .into_iter()
        .map(|(_, name, _)| name)
        .collect();
    for text in ["Wave hello", "Air horn", "Bow", "Chime"] {
        assert!(listed.iter().any(|n| n == text), "“{text}” is listed");
    }
    for text in ["Old bow", "Library chime", "Box", "Wave (link)"] {
        assert!(!listed.iter().any(|n| n == text), "“{text}” is not listed: {listed:?}");
    }
    bench.click(bench.at("Chime"));
    bench.click(bench.at("Bow"));
    bench.click(bench.at(&tr!("fs-gesture-board-add-count", count = 2)));
    let items: Vec<_> = bench.current().tiles.iter().map(|t| t.item).collect();
    assert_eq!(items, [id(101), id(200), id(300)], "in the picker's order, after the tiles there");
}
