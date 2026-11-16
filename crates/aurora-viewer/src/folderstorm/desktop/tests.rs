//! Frame tests: an `egui::Context` run as the app runs it (the end of the
//! world's labels, then the desktops, then a window), the frame drawn with
//! the software painter as the renderer draws it: what precedes the end of
//! the world's labels is cut to the world viewport.
//! `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps the pictures
//! (`desktop-*.png`).

use super::store::{Desktop, MAX_SHORTCUTS, Size, Target};
use super::surface::{self, NoRoom, Room};
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
    /// The key action *Focus the desktop* was pressed: for the next frame.
    focus: bool,
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

/// Where a test puts the command of a toolbar button being dragged (the
/// toolbar's own payload is private to `ui/toolbar.rs`).
pub(super) fn toolbar_cargo_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_test_toolbar_cargo")
}

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
            desktop: {
                let mut desktop = Desktop {
                    shortcuts_on: true,
                    ..Default::default()
                };
                desktop.sign_in(ME);
                desktop
            },
            facts: Facts::default(),
            inv: {
                let mut inv = crate::world::inventory::Inventory {
                    root: Uuid::from_u128(77),
                    ..Default::default()
                };
                crate::world::inventory::demo::seed(&mut inv, ME);
                inv
            },
            no_inventory: false,
            hidden: false,
            focus: false,
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

    /// A shortcut to the folder `n`.
    fn add_folder(&mut self, n: u128, slot: u8, cell: (u16, u16)) -> u32 {
        let target = Target::Folder {
            folder: Uuid::from_u128(n),
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
        let focus = std::mem::take(&mut self.focus);
        let (desktop, icons) = (&mut self.desktop, &self.icons);
        let folders = Folders {
            inv: &self.inv,
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
                    focus,
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
    demo::seed_folders(&mut desktop);
    let folders: Vec<_> = desktop
        .shortcuts
        .iter()
        .filter(|s| matches!(s.target, Target::Folder { .. }))
        .map(|s| (s.slot, s.cell()))
        .collect();
    assert_eq!(
        folders,
        [(0, (1, 0)), (0, (1, 1)), (0, (1, 2)), (0, (1, 3))],
        "down the second column"
    );
    assert!(
        desktop.shortcuts.iter().all(|s| s.cell() != demo::DROPPED_AT),
        "the drop's cell is free"
    );
    // the demo's desktop drawn: every size holds its names in its cells
    for size in Size::ALL {
        let mut b = Bench::new();
        assert!(demo::seed("1", &mut b.desktop, &mut view, 1920));
        demo::seed_folders(&mut b.desktop);
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
const ITEM: u128 = 710;

#[test]
fn a_folder_shortcut_shows_its_folder_and_asks_for_its_window() {
    let mut b = Bench::new();
    let mine = b.add_folder(MINE, 0, (0, 0));
    b.frames(3);
    let name = b.folder_name(MINE);
    let cell = b.cell_of(mine);
    assert!(
        b.drawn(&name).is_some_and(|text| cell.contains_rect(text)),
        "the folder's name, read now"
    );
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
fn a_folder_that_is_gone_is_greyed_and_removed_from_its_menu() {
    let mut b = Bench::new();
    let gone = b.add_folder(123_456, 0, (0, 0));
    let live = b.add_folder(MINE, 0, (0, 1));
    b.frames(3);
    // shown greyed, and it does not open
    assert!(b.painted(b.cell_of(gone)) > 100, "shown, greyed");
    b.time += 2.0;
    b.double_click(b.cell_of(gone).center());
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
    assert!(b.desktop.shortcut(live).is_some());
}

/// Another account of the profile.
const OTHER: Uuid = Uuid::from_u128(2);

#[test]
fn each_account_sees_its_own_desktop_and_never_another_s() {
    let mut b = Bench::new();
    assert_eq!(b.desktop.account, Some(ME));
    let people = b.add("people", 0, (0, 0));
    let mine = b.add_folder(MINE, 0, (0, 1));
    b.frames(3);
    let name = b.folder_name(MINE);
    let (people_at, mine_at) = (b.cell_of(people), b.cell_of(mine));
    assert!(b.drawn(label_of("people")).is_some() && b.drawn(&name).is_some());
    b.picture("desktop-account-first");
    // another account logs in on the same profile: an empty desktop, the
    // same choices; nothing of the first account shows, not even greyed
    b.desktop.size = Size::Small;
    assert!(!b.desktop.sign_in(OTHER));
    b.frames(3);
    assert!(b.desktop.shortcuts_on && b.desktop.shortcuts.is_empty());
    assert!(b.drawn(label_of("people")).is_none() && b.drawn(&name).is_none());
    assert_eq!(b.painted(people_at) + b.painted(mine_at), 0, "its cells are bare");
    // and the first account's shortcuts do not open from where they stood
    b.time += 2.0;
    b.double_click(people_at.center());
    b.time += 2.0;
    b.double_click(mine_at.center());
    assert!(b.ran().is_empty() && b.opened().is_empty());
    // its own shortcuts, in the same cells
    let map = b.add("worldmap", 0, (0, 0));
    b.frames(3);
    assert!(b.drawn(label_of("worldmap")).is_some());
    b.picture("desktop-account-second");
    // back to the first: its desktop as it was, at the profile's size
    assert!(!b.desktop.sign_in(ME));
    b.frames(3);
    assert_eq!(b.desktop.size, Size::Small);
    assert_eq!(b.desktop.shortcut(people).map(|s| s.cell()), Some((0, 0)));
    assert_eq!(b.desktop.shortcut(mine).map(|s| s.cell()), Some((0, 1)));
    assert_eq!(b.desktop.shortcuts.len(), 2);
    assert!(b.drawn(label_of("people")).is_some() && b.drawn(&name).is_some());
    assert!(b.drawn(label_of("worldmap")).is_none());
    // logged out (the login screen): nothing
    assert!(!b.desktop.sign_in(Uuid::nil()));
    b.frames(3);
    assert!(b.drawn(label_of("people")).is_none() && b.drawn(label_of("worldmap")).is_none());
    assert!(!b.desktop.sign_in(OTHER));
    assert!(b.desktop.shortcut(map).is_some());
}

#[test]
fn showinv_locks_the_folder_shortcuts_and_hides_their_names() {
    let mut b = Bench::new();
    let mine = b.add_folder(MINE, 0, (0, 0));
    b.desktop.shortcuts[0].name = Some("My own name for it".into());
    b.frames(3);
    assert!(b.drawn("My own name for it").is_some());
    b.no_inventory = true;
    b.frames(2);
    let name = b.folder_name(MINE);
    assert!(
        b.texts
            .iter()
            .all(|(text, _)| !text.contains(&name) && !text.contains("My own name")),
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
            folder: Uuid::from_u128(MINE)
        }
    );
    assert_eq!((added.slot, added.cell()), (0, (2, 2)));
    assert!(
        crate::ui::inventory::drag::carried_ids(&b.ctx).is_none(),
        "nothing left for the world"
    );
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
    let added = super::add_folders(&mut b.desktop, &rooms, &ids, None, None);
    assert_eq!((added.added, added.existing, added.no_room), (2, 0, 0));
    let cells: Vec<_> = b.desktop.shortcuts[1..].iter().map(|s| (s.slot, s.cell())).collect();
    assert_eq!(cells, [(0, (0, 1)), (0, (0, 2))]);
    assert_eq!(added.first, Some(b.desktop.shortcuts[1].id));
    // again: there already
    let again = super::add_folders(&mut b.desktop, &rooms, &ids, None, None);
    assert_eq!((again.added, again.existing, again.first), (0, 2, added.first));
    assert_eq!(
        super::added_notice(&again, 0, false, true).as_deref(),
        Some(aurora_i18n::tr!("fs-desktop-add-existing"))
    );
    // the world fills the window: kept for when it leaves room, and said
    let mut unseen = Desktop::default();
    let kept = super::add_folders(&mut unseen, &[], &ids, None, None);
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
    let none = super::add_folders(&mut full, &rooms, &ids, Some(0), Some((1, 1)));
    assert_eq!((none.added, none.no_room), (0, 2));
    assert_eq!(super::added_notice(&none, 0, false, true), Some(NoRoom::Full.text()));
}

/// A desktop whose room 0 has a shortcut in every cell.
fn fill(b: &mut Bench) {
    let grid = b.room(0).grid;
    for (n, cell) in grid.cells().enumerate() {
        b.add(&format!("c{n}"), 0, cell);
    }
    b.frames(2);
}

#[test]
fn a_full_desktop_says_so_for_every_way_of_adding() {
    // what it says
    assert_eq!(NoRoom::Full.text(), aurora_i18n::tr!("fs-desktop-full"));
    assert!(NoRoom::Limit.text().contains(&MAX_SHORTCUTS.to_string()));
    // the folders of a selection: all left out, or some
    let all = surface::Added {
        no_room: 2,
        ..Default::default()
    };
    assert_eq!(super::added_notice(&all, 0, false, true), Some(NoRoom::Full.text()));
    let some = surface::Added {
        added: 1,
        no_room: 1,
        ..Default::default()
    };
    let said = super::added_notice(&some, 0, false, true).expect("a notice");
    assert!(said.starts_with(&aurora_i18n::tr!("fs-desktop-add-full", count = 1)) && said.ends_with(&NoRoom::Full.text()));
    let limit = surface::Added {
        limit: 1,
        ..Default::default()
    };
    assert_eq!(super::added_notice(&limit, 0, false, true), Some(NoRoom::Limit.text()));

    let mut b = Bench::new();
    b.frames(2);
    fill(&mut b);
    let room = b.room(0);
    let taken: Vec<_> = room.grid.cells().collect();
    let count = b.desktop.shortcuts.len();
    // a command: refused with the reason; one that desktop has is shown
    assert_eq!(surface::no_room(&b.desktop, &room.grid, &taken), Some(NoRoom::Full));
    assert_eq!(
        surface::add_command(&mut b.desktop, &room, &taken, "people", Some((0, 0))),
        surface::CommandAdded::Refused(NoRoom::Full)
    );
    let c0 = b.desktop.shortcuts[0].id;
    assert_eq!(
        surface::add_command(&mut b.desktop, &room, &taken, "c0", None),
        surface::CommandAdded::Existing(c0)
    );
    assert_eq!(b.desktop.shortcuts.len(), count);
    // another monitor's desktop has room
    let other = b.room(2);
    assert_eq!(surface::no_room(&b.desktop, &other.grid, &[]), None);
    assert!(matches!(
        surface::add_command(&mut b.desktop, &other, &[], "people", None),
        surface::CommandAdded::New(_)
    ));
    // folders (Add to desktop, a drop): none added, one there already found
    let ids: Vec<Uuid> = [MINE, SUB].into_iter().map(Uuid::from_u128).collect();
    let rooms = b.rooms();
    let none = super::add_folders(&mut b.desktop, &rooms, &ids, None, None);
    assert_eq!((none.added, none.no_room, none.limit), (0, 2, 0));
    // a drop on that desktop: nothing added, and said
    b.out.clear();
    b.drop_inventory(&[MINE], room.grid.rect((1, 1)).center());
    assert_eq!(b.notices(), vec![NoRoom::Full.text().as_str()]);
    assert!(!b.out.contains(&Request::Changed));
    assert!(
        crate::ui::inventory::drag::carried_ids(&b.ctx).is_none(),
        "nothing left for the world"
    );
    // the desktop's menu (on the room's edge, past its last column: every
    // cell has a shortcut): Add a command is greyed and opens nothing
    let edge = pos2(room.rect.right() - 4.0, room.rect.center().y);
    assert!(room.grid.cell_at(edge).is_none(), "the bench's room has a strip past its cells");
    b.time += 2.0;
    b.click_with(edge, egui::PointerButton::Secondary);
    let add = b
        .drawn(aurora_i18n::tr!("fs-desktop-add-command"))
        .expect("Add a command is listed");
    b.pointer = add.center();
    b.frames(3);
    b.click(add.center());
    b.frames(2);
    let (_, label, ..) = *super::targets::addable()
        .iter()
        .find(|(_, _, _, refused)| refused.is_none())
        .expect("a command to add");
    assert!(b.drawn(label).is_none(), "no list of commands: {:?}", b.texts);
    b.picture("desktop-full-menu");
    // a cell made free: the same entry lists the commands again
    b.frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    b.desktop.remove(c0).expect("removed");
    b.frames(2);
    b.time += 2.0;
    b.click_with(edge, egui::PointerButton::Secondary);
    let add = b
        .drawn(aurora_i18n::tr!("fs-desktop-add-command"))
        .expect("Add a command is listed");
    b.click(add.center());
    b.frames(3);
    assert!(b.drawn(label).is_some(), "the commands are listed: {:?}", b.texts);
}

#[test]
fn the_account_s_limit_is_said_rather_than_a_full_room() {
    let mut b = Bench::new();
    b.frames(2);
    // the shortcuts kept on a monitor not in use
    for n in 0..MAX_SHORTCUTS {
        b.add(&format!("c{n}"), 5, ((n / 8) as u16, (n % 8) as u16));
    }
    let room = b.room(0);
    assert_eq!(surface::no_room(&b.desktop, &room.grid, &[]), Some(NoRoom::Limit));
    assert_eq!(
        surface::add_command(&mut b.desktop, &room, &[], "people", None),
        surface::CommandAdded::Refused(NoRoom::Limit)
    );
    let added = super::add_folders(&mut b.desktop, &[room], &[Uuid::from_u128(MINE)], None, None);
    assert_eq!((added.added, added.no_room, added.limit), (0, 0, 1));
    b.out.clear();
    b.drop_inventory(&[MINE], room.grid.rect((1, 1)).center());
    assert_eq!(b.notices(), vec![NoRoom::Limit.text().as_str()]);
}

// ---- checkpoint 3: editing, the toolbar, the keyboard ----

impl Bench {
    /// A key pressed then released.
    fn key(&mut self, key: egui::Key) {
        for pressed in [true, false] {
            self.frame(vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![egui::Event::Text(text.to_owned())]);
    }

    /// Opens the menu under `at` and clicks its entry `label`.
    fn menu(&mut self, at: Pos2, label: &str) {
        self.time += 2.0;
        self.click_with(at, egui::PointerButton::Secondary);
        let entry = self
            .drawn(label)
            .unwrap_or_else(|| panic!("“{label}” is in the menu; drawn: {:?}", self.texts));
        self.click(entry.center());
        self.frames(3);
    }

    /// A toolbar button of `command` held over `at` then released there.
    fn drop_button(&mut self, command: &str, at: Pos2) {
        self.ctx.data_mut(|d| d.insert_temp(toolbar_cargo_id(), command.to_owned()));
        self.pointer = at;
        self.frames(2);
        self.frame(vec![self.press(at, egui::PointerButton::Primary, false)]);
        self.ctx.data_mut(|d| d.remove_temp::<String>(toolbar_cargo_id()));
        self.frames(2);
    }

    fn name_of(&self, id: u32) -> Option<String> {
        self.desktop.shortcut(id).expect("a shortcut").name.clone()
    }

    fn place_of(&self, id: u32) -> (u8, (u16, u16)) {
        let shortcut = self.desktop.shortcut(id).expect("a shortcut");
        (shortcut.slot, shortcut.cell())
    }
}

#[test]
fn a_shortcut_is_renamed_in_place_from_its_menu_and_with_f2() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    b.frames(3);
    b.menu(b.cell_of(people).center(), aurora_i18n::tr!("fs-desktop-rename"));
    assert_eq!(surface::state(&b.ctx).rename, Some(people));
    assert_eq!(surface::rename_text(&b.ctx), label_of("people"), "the field starts from the name shown");
    // the name is selected: typing replaces it; Enter keeps
    b.type_text("My friends");
    b.picture("desktop-rename");
    b.key(egui::Key::Enter);
    b.frames(1);
    assert_eq!(b.name_of(people).as_deref(), Some("My friends"));
    assert_eq!(surface::state(&b.ctx).rename, None);
    assert!(b.out.contains(&Request::Changed));
    assert!(b.drawn("My friends").is_some(), "its own name is under its icon");
    // F2 on the focused shortcut; Esc leaves the name as it was
    b.focus = true;
    b.frames(2);
    b.key(egui::Key::F2);
    b.frames(1);
    assert_eq!(surface::state(&b.ctx).rename, Some(people));
    assert_eq!(surface::rename_text(&b.ctx), "My friends");
    b.type_text("Something else");
    b.key(egui::Key::Escape);
    b.frames(1);
    assert_eq!(surface::state(&b.ctx).rename, None);
    assert_eq!(b.name_of(people).as_deref(), Some("My friends"));
    // its target's own name typed back: it has no name of its own again
    b.key(egui::Key::F2);
    b.frames(1);
    assert_eq!(surface::state(&b.ctx).rename, Some(people), "the keyboard stayed on the shortcut");
    b.type_text(label_of("people"));
    b.key(egui::Key::Enter);
    b.frames(1);
    assert_eq!(b.name_of(people), None);
    // a name is never longer than the record keeps
    b.key(egui::Key::F2);
    b.frames(1);
    b.type_text(&"x".repeat(200));
    b.key(egui::Key::Enter);
    b.frames(1);
    assert_eq!(b.name_of(people).map(|n| n.chars().count()), Some(super::store::MAX_NAME));
}

#[test]
fn a_folder_shortcut_s_name_is_not_edited_while_rlva_hides_the_inventory() {
    let mut b = Bench::new();
    let folder = b.add_folder(8000, 0, (0, 0));
    b.no_inventory = true;
    b.frames(3);
    b.focus = true;
    b.frames(2);
    b.key(egui::Key::F2);
    b.frames(1);
    assert_eq!(surface::state(&b.ctx).rename, None);
    // its menu's entry is greyed: a click on it changes nothing
    b.menu(b.cell_of(folder).center(), aurora_i18n::tr!("fs-desktop-rename"));
    assert_eq!(surface::state(&b.ctx).rename, None);
    assert_eq!(b.name_of(folder), None);
}

#[test]
fn the_icon_menu_gives_an_icon_and_the_default_back() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    b.frames(3);
    let icon = |b: &Bench| b.desktop.shortcut(people).expect("a shortcut").icon.clone();
    b.menu(b.cell_of(people).center(), aurora_i18n::tr!("fs-desktop-icon"));
    b.picture("desktop-icon-menu");
    let star: Rect = b
        .ctx
        .data(|d| d.get_temp(surface::icon_cell_id("star-filled")))
        .expect("the icons are listed");
    b.click(star.center());
    b.frames(2);
    assert_eq!(icon(&b).as_deref(), Some("star-filled"));
    assert!(b.out.contains(&Request::Changed));
    // Default: the command's own icon again
    b.menu(b.cell_of(people).center(), aurora_i18n::tr!("fs-desktop-icon"));
    let default = b.drawn(aurora_i18n::tr!("fs-desktop-icon-default")).expect("Default is listed");
    b.click(default.center());
    b.frames(2);
    assert_eq!(icon(&b), None);
}

#[test]
fn a_desktop_is_arranged_by_name_and_a_shortcut_moved_to_another_monitor() {
    let mut b = Bench::new();
    let map = b.add("worldmap", 0, (2, 1));
    let build = b.add("build", 0, (0, 3));
    let people = b.add("people", 0, (1, 0));
    b.desktop.rename(map, "aaa first");
    b.frames(3);
    // from the desktop's menu, on a free cell
    let free = b.room(0).grid.rect((3, 3)).center();
    b.menu(free, aurora_i18n::tr!("fs-desktop-arrange"));
    let mut named = vec![
        ("aaa first".to_owned(), map),
        (label_of("build").to_lowercase(), build),
        (label_of("people").to_lowercase(), people),
    ];
    named.sort();
    for (n, (_, id)) in named.iter().enumerate() {
        assert_eq!(b.place_of(*id), (0, (0, n as u16)), "down the first column, by name");
    }
    assert!(b.out.contains(&Request::Changed));
    // Move to › the third monitor (the world is on the second: no desktop there)
    let monitor = surface::monitor_label(2);
    b.menu(b.cell_of(people).center(), aurora_i18n::tr!("fs-desktop-move-to"));
    assert!(b.drawn(&surface::monitor_label(1)).is_none(), "only monitors that have a desktop");
    let entry = b.drawn(&monitor).expect("the third monitor is listed");
    b.click(entry.center());
    b.frames(2);
    assert_eq!(b.place_of(people), (2, (0, 0)), "its first free cell");
    // that desktop has the same thing already: it stays, and says so
    let twin = b.add("build", 2, (1, 1));
    b.out.clear();
    b.menu(b.cell_of(build).center(), aurora_i18n::tr!("fs-desktop-move-to"));
    let entry = b.drawn(&monitor).expect("the third monitor is listed");
    b.click(entry.center());
    b.frames(2);
    assert_eq!(b.desktop.shortcut(build).expect("kept").slot, 0);
    assert_eq!(b.notices(), vec![aurora_i18n::tr!("fs-desktop-add-existing")]);
    assert_eq!(surface::state(&b.ctx).selected, Some(twin));
}

#[test]
fn the_desktop_s_menu_opens_the_toolbar_buttons_window() {
    let mut b = Bench::new();
    b.frames(3);
    let free = b.room(0).grid.rect((1, 1)).center();
    b.menu(free, aurora_i18n::tr!("toolbar-edit"));
    assert!(b.out.contains(&Request::ToolbarButtons));
}

#[test]
fn a_toolbar_button_dropped_on_a_desktop_adds_its_command() {
    let mut b = Bench::new();
    b.frames(3);
    let grid = b.room(2).grid;
    b.drop_button("people", grid.rect((1, 2)).center());
    let added = b.desktop.shortcuts.first().expect("added").clone();
    assert_eq!(added.target, Target::command("people"));
    assert_eq!((added.slot, added.cell()), (2, (1, 2)), "in the cell under the pointer");
    assert!(b.out.contains(&Request::Changed));
    assert_eq!(surface::state(&b.ctx).selected, Some(added.id));
    // the same button again: the one that desktop has, not a second
    b.drop_button("people", grid.rect((3, 0)).center());
    assert_eq!(b.desktop.shortcuts.len(), 1);
    // a button that is its own control: refused, and why
    b.out.clear();
    b.drop_button("favorites", grid.rect((3, 0)).center());
    assert_eq!(b.desktop.shortcuts.len(), 1);
    assert_eq!(b.notices(), vec![super::targets::Greyed::ButtonOnly.text()]);
    // released over the world: not the desktop's
    b.drop_button("build", pos2(960.0, 200.0));
    assert_eq!(b.desktop.shortcuts.len(), 1);
    assert!(b.world_untouched());
}

#[test]
fn a_command_sent_from_its_toolbar_button_goes_to_the_first_free_cell() {
    use surface::{CommandAdded, add_command_first};
    let mut b = Bench::new();
    b.add("people", 0, (0, 0));
    b.frames(3);
    let rooms = b.rooms();
    let CommandAdded::New(build) = add_command_first(&mut b.desktop, &rooms, "build") else {
        panic!("added");
    };
    assert_eq!(b.place_of(build), (0, (0, 1)));
    assert_eq!(add_command_first(&mut b.desktop, &rooms, "build"), CommandAdded::Existing(build));
    let never = CommandAdded::Never(super::targets::Greyed::ButtonOnly);
    assert_eq!(add_command_first(&mut b.desktop, &rooms, "favorites"), never);
    assert_eq!(add_command_first(&mut b.desktop, &[], "favorites"), never);
    // without a room (the world fills the window) it is kept for later
    let CommandAdded::New(map) = add_command_first(&mut b.desktop, &[], "worldmap") else {
        panic!("kept");
    };
    assert_eq!(b.place_of(map), (0, (0, 2)), "a cell no shortcut of that monitor keeps");
    // what the notice says
    let on = aurora_i18n::tr!("fs-desktop-added-on");
    let hidden = aurora_i18n::tr!("fs-desktop-added-hidden");
    let text = super::command_notice(CommandAdded::New(map), true, false);
    assert!(text.starts_with(aurora_i18n::tr!("fs-desktop-command-added")) && text.contains(on) && text.contains(hidden));
    let text = super::command_notice(CommandAdded::New(map), false, true);
    assert_eq!(text, aurora_i18n::tr!("fs-desktop-command-added"));
    assert_eq!(
        super::command_notice(CommandAdded::Existing(map), false, true),
        aurora_i18n::tr!("fs-desktop-add-existing")
    );
    assert_eq!(super::command_notice(never, false, true), super::targets::Greyed::ButtonOnly.text());
    assert_eq!(
        super::command_notice(CommandAdded::Refused(NoRoom::Full), false, true),
        NoRoom::Full.text()
    );
}

/// The texts of a frame's shapes, and where.
fn texts_of(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, Rect)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

/// The hook of `ToolbarUi::context`: a toolbar button's menu has *Add to
/// desktop*, the toolbar's own has not.
#[test]
fn a_toolbar_button_s_menu_asks_to_add_its_command() {
    let ctx = egui::Context::default();
    let p = crate::theme::Theme::default().skin_palette();
    let label = aurora_i18n::tr!("fs-desktop-add");
    let mut entries: Vec<Rect> = Vec::new();
    for step in 0..5 {
        let at = entries.first().map(Rect::center);
        let events = match (step, at) {
            (2, Some(at)) => vec![egui::Event::PointerMoved(at)],
            (3 | 4, Some(pos)) => vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: step == 3,
                modifiers: egui::Modifiers::NONE,
            }],
            _ => Vec::new(),
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
                time: Some(f64::from(step) / 60.0),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                // a button, the toolbar itself, a button that is its own control
                super::toolbar_menu(ui, &p, Some("people"));
                super::toolbar_menu(ui, &p, None);
                super::toolbar_menu(ui, &p, Some("favorites"));
            },
        );
        entries = texts_of(&output.shapes)
            .into_iter()
            .filter(|(text, _)| text == label)
            .map(|(_, rect)| rect)
            .collect();
        assert_eq!(entries.len(), 2, "one entry per button, none for the toolbar");
        output.textures_delta.clear();
    }
    assert_eq!(super::take_asks(&ctx), vec![super::Ask::AddCommand("people".to_owned())]);
    assert!(super::take_asks(&ctx).is_empty(), "taken once");
    // the greyed one says why, and is never added
    assert_eq!(
        super::targets::refuses_adding("favorites"),
        Some(super::targets::Greyed::ButtonOnly)
    );
    assert_eq!(super::targets::refuses_adding("people"), None);
    // (what has nothing to do now may still be added)
    assert_eq!(super::targets::refuses_adding("photo_roll"), None);
}

#[test]
fn the_keyboard_goes_to_the_desktop_and_the_arrows_from_shortcut_to_shortcut() {
    let mut b = Bench::new();
    let people = b.add("people", 0, (0, 0));
    let build = b.add("build", 0, (0, 1));
    let map = b.add("worldmap", 0, (2, 0));
    b.frames(3);
    let selected = |b: &Bench| surface::state(&b.ctx).selected;
    let focused = |b: &Bench, id: u32| b.ctx.memory(|m| m.has_focus(surface::shortcut_id(0, id)));
    // the key action: nothing selected, so the first shortcut
    b.focus = true;
    b.frames(2);
    assert_eq!(selected(&b), Some(people));
    assert!(focused(&b, people));
    b.key(egui::Key::ArrowDown);
    assert_eq!(selected(&b), Some(build));
    assert!(focused(&b, build));
    b.key(egui::Key::ArrowUp);
    assert_eq!(selected(&b), Some(people));
    b.key(egui::Key::ArrowRight);
    assert_eq!(selected(&b), Some(map));
    // at the edge it stays
    b.key(egui::Key::ArrowRight);
    assert_eq!(selected(&b), Some(map));
    assert!(focused(&b, map));
    // Enter opens the focused one, Delete removes it
    b.key(egui::Key::Enter);
    assert_eq!(b.ran(), vec!["worldmap"]);
    b.key(egui::Key::Delete);
    b.frames(1);
    assert!(b.desktop.shortcut(map).is_none());
    // with a shortcut selected, the keyboard goes to that one
    b.time += 2.0;
    b.click(b.cell_of(build).center());
    assert!(!focused(&b, build), "a click selects without taking the keyboard");
    b.focus = true;
    b.frames(2);
    assert!(focused(&b, build));
    // a key without the keyboard on the desktop does nothing there
    let mut other = Bench::new();
    let kept = other.add("people", 0, (0, 0));
    other.frames(3);
    other.click(other.cell_of(kept).center());
    other.key(egui::Key::Delete);
    other.key(egui::Key::F2);
    assert!(other.desktop.shortcut(kept).is_some());
    assert_eq!(surface::state(&other.ctx).rename, None);
}

#[test]
fn shortcuts_on_a_monitor_not_in_use_are_counted_and_brought_back() {
    let mut b = Bench::new();
    let here = b.add("people", 0, (0, 0));
    // no sixth monitor; the world fills the second
    let away = b.add("build", 5, (3, 3));
    let twin = b.add("people", 1, (0, 0));
    let map = b.add("worldmap", 1, (0, 1));
    b.frames(3);
    assert_eq!(super::unused(&b.ctx, &b.desktop), 3);
    let brought = super::bring_here(&b.ctx, &mut b.desktop);
    assert_eq!(
        brought,
        surface::Brought {
            moved: 2,
            merged: 1,
            left: 0
        }
    );
    assert_eq!(super::unused(&b.ctx, &b.desktop), 0);
    assert_eq!(b.place_of(here), (0, (0, 0)), "what was there stays");
    assert_eq!(b.place_of(away), (0, (0, 1)));
    assert_eq!(b.place_of(map), (0, (0, 2)));
    assert!(b.desktop.shortcut(twin).is_none(), "the first desktop had that one already");
    assert_eq!(super::bring_here(&b.ctx, &mut b.desktop), surface::Brought::default());
    // a first desktop that is full keeps them where they were
    let mut full = Bench::new();
    full.frames(3);
    fill(&mut full);
    full.add("gestures", 5, (0, 0));
    full.frames(3);
    let brought = super::bring_here(&full.ctx, &mut full.desktop);
    assert_eq!((brought.moved, brought.left), (0, 1));
    assert_eq!(super::unused(&full.ctx, &full.desktop), 1);
    // while no desktop shows at all, none is "on a monitor not in use"
    b.add("gestures", 7, (0, 0));
    b.world = None;
    b.frames(2);
    assert_eq!(super::unused(&b.ctx, &b.desktop), 0);
    assert_eq!(super::bring_here(&b.ctx, &mut b.desktop), surface::Brought::default());
}
