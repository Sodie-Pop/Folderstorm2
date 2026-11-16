//! Frame tests: an `egui::Context` run as the app runs it (the end of the
//! world's labels, then the desktops, then a window), the frame drawn with
//! the software painter as the renderer draws it: what precedes the end of
//! the world's labels is cut to the world viewport.
//! `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps the pictures
//! (`desktop-*.png`).

use super::store::{Desktop, Size, Target};
use super::surface::{self, Room};
use super::targets::{Facts, Folders};
use super::{Env, Request, demo};
use crate::folderstorm::viewport::{self, MonitorLayout};
use crate::ui::toolbar::COMMANDS;
use egui::{Color32, Pos2, Rect, pos2, vec2};
use uuid::Uuid;

/// What the renderer leaves where egui draws nothing: the world's pixels
/// in these tests, and black margins.
const WORLD: Color32 = Color32::from_rgb(1, 2, 3);
/// The test's window, drawn over a desktop.
const WINDOW: Color32 = Color32::from_rgb(250, 250, 250);
/// A label of the world painted in a margin: the renderer cuts it.
const LABEL: Color32 = Color32::from_rgb(255, 0, 0);

/// A viewer window at 100 %, its world viewport, its desktops, and what the
/// last frame painted.
struct Bench {
    ctx: egui::Context,
    p: crate::theme::Palette,
    icons: crate::ui::icons::Icons,
    size: [u32; 2],
    /// The world's part of the window in pixels; None: all of it.
    world: Option<[u32; 4]>,
    monitors: Option<MonitorLayout>,
    desktop: Desktop,
    facts: Facts,
    /// The inventory of the account [`ME`] (the demo's).
    inv: crate::world::inventory::Inventory,
    /// RLVa hides the Inventory (`@showinv`).
    no_inventory: bool,
    hidden: bool,
    /// A window drawn over that rectangle, above the desktops.
    window: Option<Rect>,
    /// A label of the world painted over that rectangle, before the end of
    /// the world's labels.
    label: Option<Rect>,
    /// What the desktops asked, over the frames.
    out: Vec<Request>,
    texts: Vec<(String, Rect)>,
    pixels: Vec<u32>,
    painter: crate::crash::software::Painter,
    pointer: Pos2,
    time: f64,
}

fn rgb(c: Color32) -> u32 {
    (u32::from(c.r()) << 16) | (u32::from(c.g()) << 8) | u32::from(c.b())
}

/// What the renderer does with the world's labels
/// (`aurora_render::Renderer::render`, `ui_world::take_end`): the
/// primitives before the end of the world's labels are cut to the world
/// viewport, `world` in pixels; the end itself is taken out.
fn world_labels_cut(primitives: &mut Vec<egui::ClippedPrimitive>, world: Option<[u32; 4]>, pixels_per_point: f32) {
    let end = primitives
        .iter()
        .position(|p| matches!(p.primitive, egui::epaint::Primitive::Callback(_)))
        .expect("the end of the world's labels is in the frame");
    primitives.remove(end);
    let Some([x, y, w, h]) = world else {
        return;
    };
    let world = Rect::from_min_size(
        pos2(x as f32, y as f32) / pixels_per_point,
        vec2(w as f32, h as f32) / pixels_per_point,
    );
    for primitive in &mut primitives[..end] {
        primitive.clip_rect = primitive.clip_rect.intersect(world);
    }
}

/// The account logged in.
const ME: Uuid = Uuid::from_u128(1);

/// Three monitors of 640 × 400 side by side under a window over the three.
fn three_monitors() -> MonitorLayout {
    MonitorLayout {
        window: [0, 0, 1920, 400],
        monitors: vec![[0, 0, 640, 400], [640, 0, 640, 400], [1280, 0, 640, 400]],
    }
}

impl Bench {
    /// A window over three monitors, the world on the middle one, the
    /// shortcuts on.
    fn new() -> Self {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let icons = crate::ui::icons::Icons::load(&ctx, None);
        let size = [1920, 400];
        Bench {
            ctx,
            p,
            icons,
            size,
            world: Some([640, 0, 640, 400]),
            monitors: Some(three_monitors()),
            desktop: Desktop {
                shortcuts_on: true,
                ..Default::default()
            },
            facts: Facts::default(),
            inv: {
                let mut inv = crate::world::inventory::Inventory::default();
                crate::world::inventory::demo::seed(&mut inv, ME);
                inv
            },
            no_inventory: false,
            hidden: false,
            window: None,
            label: None,
            out: Vec::new(),
            texts: Vec::new(),
            pixels: vec![0; (size[0] * size[1]) as usize],
            painter: Default::default(),
            pointer: pos2(960.0, 200.0),
            time: 0.0,
        }
    }

    fn folders(&self) -> Folders<'_> {
        Folders {
            inv: &self.inv,
            account: ME,
            no_inventory: self.no_inventory,
            colors: None,
        }
    }

    /// A shortcut to the folder `n` of the account `account`.
    fn add_folder(&mut self, n: u128, account: Uuid, slot: u8, cell: (u16, u16)) -> u32 {
        let target = Target::Folder {
            folder: Uuid::from_u128(n),
            account,
        };
        self.desktop.add(target, slot, cell).expect("added")
    }

    fn folder_name(&self, n: u128) -> String {
        crate::ui::inventory::folder_name(&self.inv, Uuid::from_u128(n))
    }

    /// The folders asked to be opened.
    fn opened(&self) -> Vec<Uuid> {
        self.out
            .iter()
            .filter_map(|r| match r {
                Request::OpenFolder { folder, .. } => Some(*folder),
                _ => None,
            })
            .collect()
    }

    fn notices(&self) -> Vec<&str> {
        self.out
            .iter()
            .filter_map(|r| match r {
                Request::Notice(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Inventory carried from an Inventory window, held over `at` then
    /// released there.
    fn drop_inventory(&mut self, ids: &[u128], at: Pos2) {
        crate::ui::inventory::drag::demo_carry(&self.ctx, ids.iter().map(|n| Uuid::from_u128(*n)).collect());
        self.pointer = at;
        self.frames(2);
        self.frame(vec![self.press(at, egui::PointerButton::Primary, false)]);
        self.frames(2);
    }

    fn add(&mut self, command: &str, slot: u8, cell: (u16, u16)) -> u32 {
        self.desktop.add(Target::command(command), slot, cell).expect("added")
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        viewport::set_world_area(&self.ctx, self.world);
        if let Some(layout) = &self.monitors {
            viewport::set_monitors(&self.ctx, layout.clone());
        }
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(self.size[0] as f32, self.size[1] as f32));
        let world = self
            .world
            .map(|[x, y, w, h]| Rect::from_min_size(pos2(x as f32, y as f32), vec2(w as f32, h as f32)));
        let (window, label, hidden, facts, p) = (self.window, self.label, self.hidden, self.facts, self.p);
        let (desktop, icons) = (&mut self.desktop, &self.icons);
        let folders = Folders {
            inv: &self.inv,
            account: ME,
            no_inventory: self.no_inventory,
            colors: None,
        };
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
                let ctx = ui.ctx().clone();
                // a label of the world, then the end of the world's labels,
                // as app.rs paints them on egui's background layer
                if let Some(rect) = label {
                    ctx.layer_painter(egui::LayerId::background()).rect_filled(rect, 0.0, LABEL);
                }
                aurora_render::end_world_labels(&ctx);
                // the bars inside the world, as by default
                viewport::set_bars_area(&ctx, world);
                viewport::bars::set_sides(&ctx, None);
                crate::ui::layout::set_work_area(&ctx, world.unwrap_or(screen));
                let env = Env {
                    p: &p,
                    icons,
                    facts,
                    folders,
                    hidden,
                };
                out = super::show(&ctx, desktop, &env);
                // a window, drawn after the desktops as before them: its
                // layer is above theirs
                if let Some(rect) = window {
                    egui::Area::new(egui::Id::unique("a window")).fixed_pos(rect.min).show(&ctx, |ui| {
                        ui.allocate_rect(rect, egui::Sense::click());
                        ui.painter().rect_filled(rect, 0.0, WINDOW);
                    });
                }
            },
        );
        self.out.extend(out);
        self.texts.clear();
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                _ => {}
            }
        }
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut self.texts);
        }
        let shapes = std::mem::take(&mut output.shapes);
        let mut primitives = self.ctx.tessellate(shapes, output.pixels_per_point);
        world_labels_cut(&mut primitives, self.world, output.pixels_per_point);
        self.painter.update_textures(&output.textures_delta);
        // the world drawn by the renderer, black around it
        self.pixels.fill(0);
        let [wx, wy, ww, wh] = self.world.unwrap_or([0, 0, self.size[0], self.size[1]]);
        for y in wy..wy + wh {
            for x in wx..wx + ww {
                self.pixels[(y * self.size[0] + x) as usize] = rgb(WORLD);
            }
        }
        let size = [self.size[0] as usize, self.size[1] as usize];
        self.painter.draw(&mut self.pixels, size, &primitives, output.pixels_per_point);
        self.painter.free_textures(&output.textures_delta);
        output.textures_delta.clear();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![egui::Event::PointerMoved(self.pointer)]);
        }
    }

    fn at(&self, x: u32, y: u32) -> Color32 {
        let p = self.pixels[(y * self.size[0] + x) as usize];
        Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, p as u8)
    }

    /// How many pixels of `rect` are not the margins' black.
    fn painted(&self, rect: Rect) -> usize {
        let mut n = 0;
        for y in rect.top() as u32..rect.bottom() as u32 {
            for x in rect.left() as u32..rect.right() as u32 {
                n += usize::from(self.at(x, y) != Color32::BLACK);
            }
        }
        n
    }

    /// The world's pixels are all the world's.
    fn world_untouched(&self) -> bool {
        let [wx, wy, ww, wh] = self.world.expect("a world viewport");
        (wy..wy + wh).all(|y| (wx..wx + ww).all(|x| self.at(x, y) == WORLD))
    }

    fn rooms(&self) -> Vec<Room> {
        super::rooms(&self.ctx, self.desktop.size)
    }

    fn room(&self, slot: u8) -> Room {
        *self.rooms().iter().find(|room| room.slot == slot).expect("that slot has a room")
    }

    /// The cell the shortcut `id` is drawn in, in its slot's room.
    fn cell_of(&self, id: u32) -> Rect {
        let slot = self.desktop.shortcut(id).expect("a shortcut").slot;
        let room = self.room(slot);
        room.grid.rect(surface::shown_cell(&self.desktop, &room, id).expect("shown"))
    }

    fn drawn(&self, text: &str) -> Option<Rect> {
        self.texts.iter().find(|(t, _)| t == text).map(|(_, rect)| *rect)
    }

    fn press(&self, at: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click_with(&mut self, at: Pos2, button: egui::PointerButton) {
        self.pointer = at;
        self.frame(vec![egui::Event::PointerMoved(at)]);
        self.frame(vec![self.press(at, button, true)]);
        self.frame(vec![self.press(at, button, false)]);
        self.frames(1);
    }

    fn click(&mut self, at: Pos2) {
        self.click_with(at, egui::PointerButton::Primary);
    }

    fn double_click(&mut self, at: Pos2) {
        self.click(at);
        self.click(at);
    }

    /// Presses at `from`, moves to `to` in steps, releases there.
    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.pointer = from;
        self.frame(vec![egui::Event::PointerMoved(from)]);
        self.frame(vec![self.press(from, egui::PointerButton::Primary, true)]);
        for step in 1..=6 {
            let at = from + (to - from) * (step as f32 / 6.0);
            self.pointer = at;
            self.frame(vec![egui::Event::PointerMoved(at)]);
        }
        self.frame(vec![self.press(to, egui::PointerButton::Primary, false)]);
        self.frames(2);
    }

    fn ran(&self) -> Vec<&str> {
        self.out
            .iter()
            .filter_map(|r| match r {
                Request::Run(command) => Some(command.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The last frame kept as `<folder>/<name>.png` when
    /// `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>`.
    fn picture(&self, name: &str) {
        let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") else {
            return;
        };
        let rgba: Vec<_> = self
            .pixels
            .iter()
            .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
            .collect();
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::save_buffer(path, &rgba, self.size[0], self.size[1], image::ColorType::Rgba8).expect("picture written");
    }
}

fn label_of(command: &str) -> &'static str {
    COMMANDS.iter().find(|c| c.id == command).expect("in the catalogue").label()
}

#[test]
fn the_icons_named_here_are_embedded() {
    for icon in surface::ICONS
        .iter()
        .chain(&["help-circle-filled", "compass-filled", "device-desktop-filled"])
        .chain(&[super::targets::ICON_FOLDER, super::targets::ICON_LOCKED])
    {
        assert!(crate::ui::icons::embedded(icon), "{icon} is not an embedded Tabler icon");
    }
}

/// The reason of this layer (PR #78's bug): what is painted before the end
/// of the world's labels is cut to the world viewport by the renderer, so
/// it never shows in a margin. The desktops are areas of egui's background
/// order: after the whole background layer, before every window.
#[test]
fn shortcuts_show_in_a_margin_after_the_renderer_s_cut_of_the_labels_and_under_a_window() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    let build = b.add("build", 2, (1, 2));
    // a label of the world over the left margin: the cut takes it
    b.label = Some(Rect::from_min_size(pos2(300.0, 300.0), vec2(40.0, 40.0)));
    // (egui shows a new area from its second frame)
    b.frames(3);
    assert_eq!(
        b.at(320, 320),
        Color32::BLACK,
        "the harness cuts what precedes the end of the labels, as the renderer"
    );
    let rooms = b.rooms();
    assert_eq!(
        rooms.iter().map(|r| r.slot).collect::<Vec<_>>(),
        vec![0, 2],
        "the side monitors; none under the world"
    );
    for id in [people, build] {
        let cell = b.cell_of(id);
        assert!(
            b.painted(cell) > 200,
            "the shortcut {id} is drawn in its margin, not cut: {}",
            b.painted(cell)
        );
    }
    assert!(b.world_untouched());
    // its name under its icon, inside its cell
    let name = b.drawn(label_of("people")).expect("the name is drawn");
    assert!(b.cell_of(people).contains_rect(name), "{name:?}");
    assert!(b.cell_of(build).left() > 1280.0, "on the third monitor");
    b.picture("desktop-margins");
    // a window over the first shortcut covers it
    let cell = b.cell_of(people);
    b.window = Some(cell.expand(4.0));
    b.frames(3);
    let c = cell.center();
    assert_eq!(b.at(c.x as u32, c.y as u32), WINDOW, "the window is drawn above the desktop");
    assert_eq!(b.at(cell.left() as u32 + 6, cell.top() as u32 + 12), WINDOW);
    assert!(b.painted(b.cell_of(build)) > 200, "the other shortcut still shows");
    b.picture("desktop-under-a-window");
}

#[test]
fn nothing_is_drawn_without_a_margin_or_while_off_or_hidden() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    b.frames(3);
    let cell = b.cell_of(people);
    assert!(b.painted(cell) > 200);
    assert!(super::takes_pointer(&b.ctx, cell.center()));
    // off
    b.desktop.shortcuts_on = false;
    b.frames(3);
    assert_eq!(b.painted(cell), 0);
    assert!(!super::takes_pointer(&b.ctx, cell.center()), "an area not shown takes nothing");
    b.desktop.shortcuts_on = true;
    b.frames(3);
    assert!(b.painted(cell) > 200);
    // the interface hidden, immersive mode, mouselook without the interface
    b.hidden = true;
    b.frames(3);
    assert_eq!(b.painted(cell), 0);
    assert!(!super::takes_pointer(&b.ctx, cell.center()));
    b.hidden = false;
    // the world fills the window (one monitor, login, a whole-window photo)
    b.world = None;
    b.frames(3);
    assert!(b.rooms().is_empty());
    assert_eq!(b.drawn(label_of("people")), None);
    assert!(!super::takes_pointer(&b.ctx, cell.center()));
    // the shortcut is kept for when the window is stretched again
    assert!(b.desktop.shortcut(people).is_some());
    b.world = Some([640, 0, 640, 400]);
    b.frames(3);
    assert!(b.painted(cell) > 200);
}

#[test]
fn the_pointer_is_the_desktop_s_over_a_room_only() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    b.frames(3);
    let room = b.room(0).rect;
    let cell = b.cell_of(people);
    // over a shortcut, and over an empty part of the room
    assert!(super::takes_pointer(&b.ctx, cell.center()));
    assert!(super::takes_pointer(&b.ctx, room.max - vec2(20.0, 20.0)));
    assert!(super::takes_pointer(&b.ctx, b.room(2).rect.center()));
    // egui itself does not count it as interface: the reason of the hook
    b.pointer = cell.center();
    b.frames(2);
    assert!(
        !b.ctx.is_pointer_over_egui(),
        "an area of the background order is not interface for egui"
    );
    // over the world
    assert!(!super::takes_pointer(&b.ctx, pos2(960.0, 200.0)));
    // under a window: the window's layer is the topmost
    b.window = Some(cell.expand(4.0));
    b.frames(3);
    assert!(!super::takes_pointer(&b.ctx, cell.center()), "the window wins");
    assert!(b.ctx.is_pointer_over_egui(), "and it is interface as any window");
    assert!(
        super::takes_pointer(&b.ctx, room.max - vec2(20.0, 20.0)),
        "beside the window, still the desktop"
    );
}

#[test]
fn a_double_click_opens_and_a_single_click_selects() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    let map = b.add("worldmap", 0, (0, 1));
    b.frames(3);
    let at = b.cell_of(people).center();
    b.click(at);
    assert!(b.ran().is_empty(), "one click opens nothing");
    assert_eq!(surface::state(&b.ctx).selected, Some(people));
    // (long enough after the first click not to be its double)
    b.time += 2.0;
    b.double_click(at);
    assert_eq!(b.ran(), vec!["people"], "one request for a double click");
    // a click on an empty cell selects nothing
    let free = b.room(0).grid.rect((3, 3)).center();
    b.click(free);
    assert_eq!(surface::state(&b.ctx).selected, None);
    // with "open with a single click", one click is enough
    b.out.clear();
    b.desktop.single_click = true;
    b.time += 2.0;
    b.click(b.cell_of(map).center());
    assert_eq!(b.ran(), vec!["worldmap"]);
}

#[test]
fn a_greyed_shortcut_does_not_open() {
    let mut b = Bench::new();
    let sit = b.add("sit", 0, (0, 0));
    let favorites = b.add("favorites", 0, (0, 1));
    let gone = b.add("retired-command", 0, (0, 2));
    b.facts.sit_locked = true;
    b.frames(3);
    for id in [sit, favorites, gone] {
        b.time += 2.0;
        b.double_click(b.cell_of(id).center());
    }
    assert!(
        b.ran().is_empty(),
        "RLVa's lock, a button that is its control, a command that is gone: {:?}",
        b.ran()
    );
    // RLVa lets go: Sit opens again
    b.facts.sit_locked = false;
    b.time += 2.0;
    b.double_click(b.cell_of(sit).center());
    assert_eq!(b.ran(), vec!["sit"]);
}

#[test]
fn a_shortcut_is_dragged_to_a_cell_swapped_and_taken_to_another_monitor() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    let map = b.add("worldmap", 0, (0, 1));
    b.frames(3);
    let grid = b.room(0).grid;
    // to a free cell: it snaps to the grid
    b.drag(b.cell_of(people).center(), grid.rect((2, 1)).center() + vec2(9.0, -7.0));
    assert_eq!(b.desktop.shortcut(people).map(|s| (s.slot, s.col, s.row)), Some((0, 2, 1)));
    assert_eq!(b.cell_of(people), grid.rect((2, 1)));
    assert!(b.out.contains(&Request::Changed), "to be saved");
    assert!(b.ran().is_empty(), "a drag opens nothing");
    // onto a taken cell: the two swap
    b.drag(b.cell_of(map).center(), b.cell_of(people).center());
    assert_eq!(b.desktop.shortcut(map).map(|s| (s.col, s.row)), Some((2, 1)));
    assert_eq!(b.desktop.shortcut(people).map(|s| (s.col, s.row)), Some((0, 1)));
    // to the third monitor's room
    let far = b.room(2).grid.rect((1, 0));
    b.drag(b.cell_of(people).center(), far.center());
    assert_eq!(b.desktop.shortcut(people).map(|s| (s.slot, s.col, s.row)), Some((2, 1, 0)));
    assert_eq!(b.cell_of(people), far);
    // released over the world: it stays where it was
    b.out.clear();
    b.drag(b.cell_of(people).center(), pos2(960.0, 200.0));
    assert_eq!(b.desktop.shortcut(people).map(|s| (s.slot, s.col, s.row)), Some((2, 1, 0)));
    assert!(!b.out.contains(&Request::Changed));
}

#[test]
fn the_menus_add_a_command_and_remove_a_shortcut() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    b.frames(3);
    // the shortcut's menu
    b.click_with(b.cell_of(people).center(), egui::PointerButton::Secondary);
    let open = b.drawn(aurora_i18n::tr!("fs-desktop-open")).expect("Open is in the menu");
    assert!(
        b.drawn(aurora_i18n::tr!("fs-desktop-add-command")).is_none(),
        "not the desktop's menu"
    );
    b.time += 2.0;
    b.click(open.center());
    assert_eq!(b.ran(), vec!["people"]);
    b.time += 2.0;
    b.click_with(b.cell_of(people).center(), egui::PointerButton::Secondary);
    let remove = b.drawn(aurora_i18n::tr!("fs-desktop-remove")).expect("Remove is in the menu");
    b.click(remove.center());
    assert!(b.desktop.shortcuts.is_empty(), "only the shortcut goes");
    assert!(b.out.contains(&Request::Changed));
    // the desktop's menu, on a free cell: the command lands in that cell
    let grid = b.room(0).grid;
    let cell = (1, 2);
    b.time += 2.0;
    b.click_with(grid.rect(cell).center(), egui::PointerButton::Secondary);
    let add = b
        .drawn(aurora_i18n::tr!("fs-desktop-add-command"))
        .expect("Add a command is in the menu");
    b.click(add.center());
    b.frames(3);
    // the first command of the list that may be added
    let (command, label, ..) = *super::targets::addable()
        .iter()
        .find(|(_, _, _, refused)| refused.is_none())
        .expect("a command to add");
    let row = b
        .drawn(label)
        .unwrap_or_else(|| panic!("“{label}” is listed; drawn: {:?}", b.texts));
    b.picture("desktop-add-a-command");
    b.click(row.center());
    b.frames(2);
    let added = b.desktop.shortcuts.first().expect("added");
    assert_eq!(added.target, Target::command(command));
    assert_eq!((added.slot, added.cell()), (0, cell));
    assert_eq!(surface::state(&b.ctx).selected, Some(added.id));
}

#[test]
fn the_demo_seeds_its_desktops() {
    let mut desktop = Desktop::default();
    let mut view = crate::folderstorm::viewport::ViewportSettings::default();
    assert!(!demo::seed("nope", &mut desktop, &mut view, 5760));
    assert_eq!(desktop, Desktop::default());
    assert!(demo::seed("1", &mut desktop, &mut view, 5760));
    assert!(desktop.shortcuts_on && view.enabled);
    assert_eq!((view.left, view.right), (1920, 1920), "the world in the middle third");
    assert_eq!(desktop.shortcuts.iter().filter(|s| s.slot == 0).count(), 8);
    assert_eq!(desktop.shortcuts.iter().filter(|s| s.slot == 2).count(), 2);
    assert!(desktop.shortcuts.iter().any(|s| s.name.is_some() && s.icon.is_some()));
    assert!(desktop.shortcuts.iter().any(|s| s.target == Target::command(demo::OPENED)));
    // a viewport already chosen is kept
    let mut chosen = crate::folderstorm::viewport::ViewportSettings {
        enabled: true,
        left: 100,
        ..Default::default()
    };
    assert!(demo::seed("open", &mut desktop, &mut chosen, 5760));
    assert_eq!(chosen.left, 100);
    // `single`: the world fills the window
    assert!(demo::seed("single", &mut desktop, &mut chosen, 5760));
    assert!(!chosen.enabled && desktop.shortcuts_on && !desktop.shortcuts.is_empty());
    // the folders of the demo inventory, and one that is gone
    for mode in ["drop", "rlva"] {
        assert!(demo::seed(mode, &mut desktop, &mut view, 5760), "{mode}");
    }
    demo::seed_folders(&mut desktop, ME);
    let b = Bench::new();
    let folders: Vec<_> = desktop
        .shortcuts
        .iter()
        .filter(|s| matches!(s.target, Target::Folder { .. }))
        .map(|s| super::targets::face(s, &b.facts, &b.folders()).greyed)
        .collect();
    assert_eq!(folders, [None, None, None, Some(super::targets::Greyed::FolderGone)]);
    assert_eq!(
        super::targets::folder_state(&b.inv, demo::DROPPED),
        super::targets::FolderState::Live
    );
    // the demo's desktop drawn: every size holds its names in its cells
    for size in Size::ALL {
        let mut b = Bench::new();
        assert!(demo::seed("1", &mut b.desktop, &mut view, 1920));
        demo::seed_folders(&mut b.desktop, ME);
        b.desktop.size = size;
        b.frames(3);
        for shortcut in b.desktop.shortcuts.clone() {
            assert!(b.painted(b.cell_of(shortcut.id)) > 100, "{size:?}: {:?}", shortcut.target);
        }
        assert!(b.world_untouched());
        b.picture(&format!("desktop-demo-{size:?}").to_lowercase());
    }
}

/// A folder of the demo inventory, its sub-folder, and one of its items.
const MINE: u128 = 8000;
const SUB: u128 = 8001;
const ITEM: u128 = 730;

#[test]
fn a_folder_shortcut_shows_its_folder_and_asks_for_its_window() {
    let mut b = Bench::new();
    let mine = b.add_folder(MINE, ME, 0, (0, 0));
    b.frames(3);
    let name = b.folder_name(MINE);
    let cell = b.cell_of(mine);
    assert!(b.drawn(&name).is_some_and(|text| cell.contains_rect(text)), "the folder's name, read now");
    assert!(b.world_untouched());
    b.double_click(cell.center());
    assert_eq!(b.opened(), vec![Uuid::from_u128(MINE)]);
    // the window goes beside that cell, in that room
    assert!(b.out.contains(&Request::OpenFolder {
        folder: Uuid::from_u128(MINE),
        cell,
        room: b.room(0).rect,
    }));
    assert!(b.ran().is_empty());
    // its menu: Open, and Show in inventory
    b.out.clear();
    b.time += 2.0;
    b.click_with(cell.center(), egui::PointerButton::Secondary);
    let show = b
        .drawn(aurora_i18n::tr!("ui-context-show-in-inventory"))
        .expect("Show in inventory is in a folder's menu");
    b.picture("desktop-folder-menu");
    b.click(show.center());
    assert_eq!(b.out, vec![Request::ShowFolder(Uuid::from_u128(MINE))]);
    // a command's menu has no such entry
    let people = b.add("people", 0, (0, 1));
    b.frames(2);
    b.time += 2.0;
    b.click_with(b.cell_of(people).center(), egui::PointerButton::Secondary);
    assert!(b.drawn(aurora_i18n::tr!("fs-desktop-open")).is_some());
    assert!(b.drawn(aurora_i18n::tr!("ui-context-show-in-inventory")).is_none());
}

#[test]
fn a_folder_that_is_gone_or_of_another_account_is_greyed_and_removed_from_its_menu() {
    let mut b = Bench::new();
    let gone = b.add_folder(123_456, ME, 0, (0, 0));
    let theirs = b.add_folder(MINE, Uuid::from_u128(2), 0, (0, 1));
    b.frames(3);
    // neither says a folder's name, neither opens
    assert!(b.drawn(&b.folder_name(MINE)).is_none(), "another account's folder is not named");
    for id in [gone, theirs] {
        assert!(b.painted(b.cell_of(id)) > 100, "shown, greyed");
        b.time += 2.0;
        b.double_click(b.cell_of(id).center());
    }
    assert!(b.opened().is_empty());
    // the menu: Open greyed, Remove works
    b.time += 2.0;
    b.click_with(b.cell_of(gone).center(), egui::PointerButton::Secondary);
    let open = b.drawn(aurora_i18n::tr!("fs-desktop-open")).expect("Open is listed");
    b.click(open.center());
    assert!(b.opened().is_empty() && b.out.iter().all(|r| !matches!(r, Request::ShowFolder(_))));
    b.frames(2);
    b.time += 2.0;
    b.click_with(b.cell_of(gone).center(), egui::PointerButton::Secondary);
    let remove = b.drawn(aurora_i18n::tr!("fs-desktop-remove")).expect("Remove is in the menu");
    b.click(remove.center());
    assert!(b.desktop.shortcut(gone).is_none());
    assert!(b.desktop.shortcut(theirs).is_some());
}

#[test]
fn showinv_locks_the_folder_shortcuts_and_hides_their_names() {
    let mut b = Bench::new();
    let mine = b.add_folder(MINE, ME, 0, (0, 0));
    b.desktop.shortcuts[0].name = Some("My own name for it".into());
    b.frames(3);
    assert!(b.drawn("My own name for it").is_some());
    b.no_inventory = true;
    b.frames(2);
    let name = b.folder_name(MINE);
    assert!(
        b.texts.iter().all(|(text, _)| !text.contains(&name) && !text.contains("My own name")),
        "nothing of the folder is said while the Inventory is hidden: {:?}",
        b.texts
    );
    assert!(b.drawn(aurora_i18n::tr!("fs-desktop-folder")).is_some());
    b.picture("desktop-showinv");
    b.double_click(b.cell_of(mine).center());
    assert!(b.opened().is_empty(), "it does not open");
    // and no folder is added
    let at = b.room(0).grid.rect((2, 2)).center();
    b.drop_inventory(&[SUB], at);
    assert_eq!(b.desktop.shortcuts.len(), 1);
    assert_eq!(b.notices(), vec![super::targets::Refusal::Rlva.text()]);
    // the restriction lifted: as before
    b.no_inventory = false;
    b.frames(2);
    assert!(b.drawn("My own name for it").is_some());
    b.time += 2.0;
    b.double_click(b.cell_of(mine).center());
    assert_eq!(b.opened(), vec![Uuid::from_u128(MINE)]);
}

#[test]
fn inventory_dropped_on_a_desktop_adds_its_folders_and_leaves_its_items_out() {
    let mut b = Bench::new();
    assert!(b.inv.items.get(&Uuid::from_u128(ITEM)).is_some(), "an item of the demo inventory");
    let people = b.add("people", 0, (1, 1));
    b.frames(3);
    let grid = b.room(0).grid;
    // held over the desktop: the room is outlined
    crate::ui::inventory::drag::demo_carry(&b.ctx, vec![Uuid::from_u128(MINE)]);
    b.pointer = grid.rect((2, 2)).center();
    b.frames(2);
    let edge = b.room(0).rect.left_top() + vec2(40.0, 1.0);
    assert_ne!(b.at(edge.x as u32, edge.y as u32), Color32::BLACK, "the outline of the room");
    assert!(b.desktop.shortcuts.len() == 1, "nothing before the release");
    b.picture("desktop-drop-held");
    // released: a shortcut in the cell under the pointer; the payload is taken
    b.drop_inventory(&[MINE], grid.rect((2, 2)).center());
    let added = b.desktop.shortcuts.last().expect("added").clone();
    assert_eq!(
        added.target,
        Target::Folder {
            folder: Uuid::from_u128(MINE),
            account: ME
        }
    );
    assert_eq!((added.slot, added.cell()), (0, (2, 2)));
    assert!(crate::ui::inventory::drag::carried_ids(&b.ctx).is_none(), "nothing left for the world");
    assert!(b.out.contains(&Request::Changed));
    assert!(b.notices().is_empty(), "a drop that worked says nothing");
    assert_eq!(surface::state(&b.ctx).selected, Some(added.id));
    assert!(b.opened().is_empty() && b.ran().is_empty());
    // a selection: its folders are added, its items counted; a cell that is
    // taken gives the nearest free one
    b.out.clear();
    b.drop_inventory(&[SUB, ITEM], b.cell_of(people).center());
    let sub = b.desktop.shortcuts.last().expect("added").clone();
    assert_eq!(b.desktop.shortcuts.len(), 3);
    assert_ne!(sub.cell(), (1, 1), "People keeps its cell");
    assert_eq!(b.notices(), vec![aurora_i18n::tr!("fs-desktop-left-out", count = 1).as_str()]);
    // the same folder again on that desktop: selected, not added twice
    b.out.clear();
    b.drop_inventory(&[MINE], grid.rect((4, 0)).center());
    assert_eq!(b.desktop.shortcuts.len(), 3);
    assert_eq!(surface::state(&b.ctx).selected, Some(added.id));
    assert_eq!(b.notices(), vec![aurora_i18n::tr!("fs-desktop-add-existing")]);
    assert!(!b.out.contains(&Request::Changed));
    // but it may be on another monitor's desktop
    b.drop_inventory(&[MINE], b.room(2).grid.rect((0, 0)).center());
    assert_eq!(b.desktop.shortcuts.iter().filter(|s| s.slot == 2).count(), 1);
    // items alone, the Library, the root: refused, with the reason
    b.out.clear();
    let (library, root) = (b.inv.lib_root.as_u128(), b.inv.root.as_u128());
    for cargo in [vec![ITEM], vec![library], vec![root]] {
        b.drop_inventory(&cargo, grid.rect((4, 2)).center());
    }
    assert_eq!(b.desktop.shortcuts.len(), 4);
    assert_eq!(b.notices(), vec![super::targets::Refusal::NoFolder.text(); 3]);
    assert!(crate::ui::inventory::drag::carried_ids(&b.ctx).is_none());
    // released over the world: not the desktop's
    b.out.clear();
    b.drop_inventory(&[SUB], pos2(960.0, 200.0));
    assert!(b.out.is_empty() && b.desktop.shortcuts.len() == 4);
}

#[test]
fn folders_are_added_in_the_first_free_cells_and_kept_without_a_room() {
    let mut b = Bench::new();
    b.add("people", 0, (0, 0));
    b.frames(2);
    let rooms = b.rooms();
    let ids: Vec<Uuid> = [MINE, SUB].into_iter().map(Uuid::from_u128).collect();
    // from a menu: the first free cells of the first room, down its column
    let added = super::add_folders(&mut b.desktop, &rooms, ME, &ids, None, None);
    assert_eq!((added.added, added.existing, added.no_room), (2, 0, 0));
    let cells: Vec<_> = b.desktop.shortcuts[1..].iter().map(|s| (s.slot, s.cell())).collect();
    assert_eq!(cells, [(0, (0, 1)), (0, (0, 2))]);
    assert_eq!(added.first, Some(b.desktop.shortcuts[1].id));
    // again: there already
    let again = super::add_folders(&mut b.desktop, &rooms, ME, &ids, None, None);
    assert_eq!((again.added, again.existing, again.first), (0, 2, added.first));
    assert_eq!(super::added_notice(&again, 0, false, true).as_deref(), Some(aurora_i18n::tr!("fs-desktop-add-existing")));
    // the world fills the window: kept for when it leaves room, and said
    let mut unseen = Desktop::default();
    let kept = super::add_folders(&mut unseen, &[], ME, &ids, None, None);
    assert_eq!((kept.added, kept.no_room), (2, 0));
    let cells: Vec<_> = unseen.shortcuts.iter().map(|s| (s.slot, s.cell())).collect();
    assert_eq!(cells, [(0, (0, 0)), (0, (0, 1))]);
    let said = super::added_notice(&kept, 0, true, false).expect("a notice");
    assert!(said.contains(aurora_i18n::tr!("fs-desktop-added-on")) && said.contains(aurora_i18n::tr!("fs-desktop-added-hidden")));
    // a room with no free cell takes no more
    let mut full = Desktop::default();
    let grid = rooms[0].grid;
    for (n, cell) in grid.cells().enumerate() {
        full.add(Target::command(&format!("c{n}")), 0, cell).expect("room");
    }
    let none = super::add_folders(&mut full, &rooms, ME, &ids, Some(0), Some((1, 1)));
    assert_eq!((none.added, none.no_room), (0, 2));
    assert!(super::added_notice(&none, 0, false, true).is_some());
}
